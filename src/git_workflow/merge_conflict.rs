//! 合并冲突自动分流（细粒度三方合并）
//!
//! 算法移植自 git-conflict-resolve skill 的 git_conflict_resolve.py：
//! git 合并冲突时 index 保留三方完整版本（stage 1=base, 2=ours, 3=theirs）。
//! 对 base→ours、base→theirs 各算一份行级 diff（similar crate），在 base 行
//! 索引上做区间合成，把无歧义差异段自动解决，真冲突段输出为结构化 segments
//! 供冲突解决页（/requirement-merge-conflict）人工处理。
//!
//! 安全原则（不可妥协）：只自动解决数学上无歧义的差异段（一方未改取另一方、
//! 双方改成相同、单方插入、two-way 拆分无歧义的插入竞争），有歧义一律保留为
//! 真冲突；mode/二进制/modify-delete/嵌套标记一律 unresolvable 交给人工决策。

use super::*;

use similar::{capture_diff_slices, Algorithm, DiffOp, DiffTag};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

/// ours（目标分支）冲突块起始标记（与 skill 脚本保持一致）
pub(crate) const MARK_OURS: &str = "<<<<<<< ours";
/// ours/theirs 分隔标记
pub(crate) const MARK_SEP: &str = "=======";
/// theirs（需求分支）冲突块结束标记
pub(crate) const MARK_THEIRS: &str = ">>>>>>> theirs";

const GIT_TIMEOUT_MS: u64 = 60_000;

// ---------------------------------------------------------------- 数据模型

/// 合并结果中的一个片段：Plain = 已无歧义合并的内容，Conflict = 真冲突块
#[derive(Debug, Clone)]
pub(crate) enum MergeSegment {
    /// 已确定（自动合并/双方一致）的连续行
    Plain(Vec<String>),
    /// 真冲突块，携带三方内容与两侧在全文件中的行区间（0-based 半开）
    Conflict(ConflictHunk),
}

#[derive(Debug, Clone)]
pub(crate) struct ConflictHunk {
    /// 冲突序号（文件内从 0 递增，供前端定位/回填）
    pub(crate) index: usize,
    /// content = 同一 base 区间双方都改且不同；insert-race = 双方在同一空位插入竞争
    pub(crate) kind: &'static str,
    pub(crate) base: Vec<String>,
    pub(crate) ours: Vec<String>,
    pub(crate) theirs: Vec<String>,
    pub(crate) ours_start: usize,
    pub(crate) ours_end: usize,
    pub(crate) theirs_start: usize,
    pub(crate) theirs_end: usize,
}

/// 单个冲突文件的处理结果
#[derive(Debug)]
pub(crate) struct FileConflict {
    pub(crate) path: String,
    /// resolved（已全部解决并写入）/ partial（自动部分 + 剩余标记）/ unresolvable / binary
    pub(crate) status: &'static str,
    pub(crate) reason: Option<String>,
    pub(crate) segments: Vec<MergeSegment>,
    /// 自动解决的差异段数（仅统计存在分歧后无歧义合入的段，equal 不计）
    pub(crate) auto_hunks: usize,
    pub(crate) has_ours: bool,
    pub(crate) has_theirs: bool,
    /// stage2/stage3 原文（供解决页三栏展示；unresolvable/二进制时为空）
    pub(crate) ours_text: String,
    pub(crate) theirs_text: String,
}

#[derive(Debug, Default)]
pub(crate) struct MergeConflictReport {
    pub(crate) files: Vec<FileConflict>,
    pub(crate) auto_resolved_files: usize,
    pub(crate) auto_hunks: usize,
    pub(crate) remaining_files: usize,
    pub(crate) remaining_hunks: usize,
    pub(crate) unresolvable_files: usize,
}

impl MergeConflictReport {
    /// 是否全部冲突都已自动解决（无剩余真冲突、无 unresolvable/binary）
    pub(crate) fn all_resolved(&self) -> bool {
        self.remaining_hunks == 0 && self.unresolvable_files == 0
    }
}

#[derive(Debug)]
enum BlobText {
    Text(String),
    Binary,
}

#[derive(Debug, Clone, Copy)]
struct Op {
    tag: DiffTag,
    i1: usize,
    i2: usize,
    j1: usize,
    j2: usize,
}

// ---------------------------------------------------------------- 行工具

/// 按 \n 分行并保留行尾（与 git 行语义一致；最后无换行的残余行保留原样）
fn split_lines(text: &str) -> Vec<String> {
    let mut lines = Vec::new();
    let mut rest = text;
    while let Some(pos) = rest.find('\n') {
        lines.push(rest[..=pos].to_string());
        rest = &rest[pos + 1..];
    }
    if !rest.is_empty() {
        lines.push(rest.to_string());
    }
    lines
}

/// 输出文件前规范化：除最后一行外必须以 \n 结尾（否则冲突标记粘连）；末行保持原样
fn normalize_newlines(lines: Vec<String>) -> Vec<String> {
    let last = lines.len().saturating_sub(1);
    lines
        .into_iter()
        .enumerate()
        .map(|(i, mut l)| {
            if i < last && !l.ends_with('\n') {
                l.push('\n');
            }
            l
        })
        .collect()
}

/// 保守检测三方版本中是否已含冲突标记（嵌套冲突一律不自动处理）
fn has_conflict_marker(lines: &[String]) -> bool {
    lines.iter().any(|l| {
        let s = l.trim_end_matches(['\n', '\r']);
        s.starts_with("<<<<<<< ")
            || s.starts_with(">>>>>>> ")
            || s.starts_with("||||||| ")
            || s == MARK_SEP
    })
}

/// 校验用户提交的解决内容：含确定性的冲突标记时拒绝（单独的 ======= 行允许，
/// markdown/RST 里合法出现）
fn find_conflict_marker_in_content(content: &str) -> Option<&'static str> {
    for line in content.lines() {
        let s = line.trim_end_matches('\r');
        if s.starts_with("<<<<<<< ") {
            return Some("<<<<<<<");
        }
        if s.starts_with(">>>>>>> ") {
            return Some(">>>>>>>");
        }
        if s.starts_with("||||||| ") {
            return Some("|||||||");
        }
    }
    None
}

fn to_ops(ops: &[DiffOp]) -> Vec<Op> {
    ops.iter()
        .map(|op| Op {
            tag: op.tag(),
            i1: op.old_range().start,
            i2: op.old_range().end,
            j1: op.new_range().start,
            j2: op.new_range().end,
        })
        .collect()
}

// ---------------------------------------------------------------- 核心算法

/// 双方在同一 base 空位插入内容的竞争合并（典型：both-added 同名文件）。
///
/// 保守规则（防"处理错"）：
///   - equal 块：两边共有且对齐一致 → 输出一份
///   - unequal 块双方非空 → 冲突（插入内容本身不同）
///   - unequal 块单侧独有：独有行只要在对方内容中出现过（顺序/重复歧义，
///     如 [A,B] vs [B,A]、[X] vs [X,X] 的对齐是多义的）→ 整块冲突；
///     确实是对方没有的新增行 → 保留（新增语义无歧义）
///
/// 返回 (合并输出行, 冲突子块在 a/b 空间内的区间列表)
fn two_way_merge(a: &[String], b: &[String]) -> (Vec<String>, Vec<(usize, usize, usize, usize)>) {
    let ops = to_ops(&capture_diff_slices(Algorithm::Myers, a, b));
    let a_set: HashSet<&String> = a.iter().collect();
    let b_set: HashSet<&String> = b.iter().collect();
    let mut out = Vec::new();
    let mut hunks = Vec::new();
    for op in &ops {
        let a_s = &a[op.i1..op.i2];
        let b_s = &b[op.j1..op.j2];
        if op.tag == DiffTag::Equal {
            out.extend_from_slice(a_s);
        } else if !a_s.is_empty() && !b_s.is_empty() {
            hunks.push((op.i1, op.i2, op.j1, op.j2));
        } else if !a_s.is_empty() {
            if a_s.iter().any(|l| b_set.contains(l)) {
                hunks.push((op.i1, op.i2, op.j1, op.j2));
            } else {
                out.extend_from_slice(a_s);
            }
        } else if !b_s.is_empty() {
            if b_s.iter().any(|l| a_set.contains(l)) {
                hunks.push((op.i1, op.i2, op.j1, op.j2));
            } else {
                out.extend_from_slice(b_s);
            }
        }
    }
    (out, hunks)
}

/// 行级三方合并：base→ours 与 base→theirs 两份 opcode 在 base 索引上合成。
/// 返回 (segments, 自动解决差异段数)；算法内部不变量破坏时返回 Err（调用方
/// 保留 git 原始冲突状态，绝不带错写文件）。
fn merge3(
    base: &[String],
    ours: &[String],
    theirs: &[String],
) -> Result<(Vec<MergeSegment>, usize), String> {
    let ops_a = to_ops(&capture_diff_slices(Algorithm::Myers, base, ours));
    let ops_b = to_ops(&capture_diff_slices(Algorithm::Myers, base, theirs));
    // insert op 预索引：base 位置 -> (j1, j2)
    let mut ins_a: HashMap<usize, (usize, usize)> = HashMap::new();
    let mut ins_b: HashMap<usize, (usize, usize)> = HashMap::new();
    for op in &ops_a {
        if op.i1 == op.i2 && op.j2 > op.j1 {
            ins_a.insert(op.i1, (op.j1, op.j2));
        }
    }
    for op in &ops_b {
        if op.i1 == op.i2 && op.j2 > op.j1 {
            ins_b.insert(op.i1, (op.j1, op.j2));
        }
    }

    let mut pts: BTreeSet<usize> = BTreeSet::new();
    pts.insert(0);
    pts.insert(base.len());
    for op in ops_a.iter().chain(ops_b.iter()) {
        pts.insert(op.i1);
        pts.insert(op.i2);
    }

    let mut segments: Vec<MergeSegment> = Vec::new();
    let mut auto = 0usize;
    let mut conflict_index = 0usize;
    let mut ia = 0usize;
    let mut ib = 0usize;

    let mut push_conflict = |segments: &mut Vec<MergeSegment>,
                             conflict_index: &mut usize,
                             kind: &'static str,
                             base_lines: Vec<String>,
                             ours_lines: Vec<String>,
                             theirs_lines: Vec<String>,
                             ours_range: (usize, usize),
                             theirs_range: (usize, usize)| {
        segments.push(MergeSegment::Conflict(ConflictHunk {
            index: *conflict_index,
            kind,
            base: base_lines,
            ours: ours_lines,
            theirs: theirs_lines,
            ours_start: ours_range.0,
            ours_end: ours_range.1,
            theirs_start: theirs_range.0,
            theirs_end: theirs_range.1,
        }));
        *conflict_index += 1;
    };

    // 在 ops 中找到恰好覆盖 [s,t) 的非零长 op（断点保证存在；找不到即内部不变量破坏）
    fn covering<'a>(ops: &'a [Op], i: &mut usize, s: usize, t: usize) -> Result<&'a Op, String> {
        while *i < ops.len() {
            let op = &ops[*i];
            if op.i2 <= s || op.i1 == op.i2 {
                *i += 1;
                continue;
            }
            if !(op.i1 <= s && op.i2 >= t) {
                return Err(format!(
                    "internal: op [{},{}] does not cover [{},{})",
                    op.i1, op.i2, s, t
                ));
            }
            return Ok(op);
        }
        Err("internal: no covering op found".to_string())
    }

    let mut prev: Option<usize> = None;
    for &pt in &pts {
        // 1) 非零段 [prev, pt)
        if let Some(s) = prev {
            if pt > s {
                let t = pt;
                let a_op = covering(&ops_a, &mut ia, s, t)?;
                let b_op = covering(&ops_b, &mut ib, s, t)?;
                let a_slice = &ours[a_op.j1 + (s - a_op.i1)..a_op.j1 + (t - a_op.i1)];
                let b_slice = &theirs[b_op.j1 + (s - b_op.i1)..b_op.j1 + (t - b_op.i1)];
                let a_eq = a_op.tag == DiffTag::Equal;
                let b_eq = b_op.tag == DiffTag::Equal;
                if a_eq && b_eq {
                    // 三方一致，直接输出（不计入自动解决数：无分歧发生）
                    segments.push(MergeSegment::Plain(base[s..t].to_vec()));
                } else if a_eq {
                    // ours 未改此区间，theirs 改了 → 取 theirs（无歧义）
                    segments.push(MergeSegment::Plain(b_slice.to_vec()));
                    auto += 1;
                } else if b_eq {
                    // theirs 未改此区间，ours 改了 → 取 ours（无歧义）
                    segments.push(MergeSegment::Plain(a_slice.to_vec()));
                    auto += 1;
                } else if a_slice == b_slice {
                    // 双方改成相同 → 取一份（结果确定）
                    segments.push(MergeSegment::Plain(a_slice.to_vec()));
                    auto += 1;
                } else {
                    // 真冲突：同一 base 区间双方都改且不同
                    push_conflict(
                        &mut segments,
                        &mut conflict_index,
                        "content",
                        base[s..t].to_vec(),
                        a_slice.to_vec(),
                        b_slice.to_vec(),
                        (a_op.j1 + (s - a_op.i1), a_op.j1 + (t - a_op.i1)),
                        (b_op.j1 + (s - b_op.i1), b_op.j1 + (t - b_op.i1)),
                    );
                }
            }
        }
        // 2) pt 处的插入
        let aj = ins_a.get(&pt).copied();
        let bj = ins_b.get(&pt).copied();
        if aj.is_some() || bj.is_some() {
            let a_lines: Vec<String> = match aj {
                Some((j1, j2)) => ours[j1..j2].to_vec(),
                None => Vec::new(),
            };
            let b_lines: Vec<String> = match bj {
                Some((j1, j2)) => theirs[j1..j2].to_vec(),
                None => Vec::new(),
            };
            if aj.is_some() && bj.is_some() {
                if a_lines == b_lines {
                    // 双方插入相同内容 → 一份
                    segments.push(MergeSegment::Plain(a_lines));
                    auto += 1;
                } else {
                    // insert 竞争：two-way 拆分（保守）
                    let (merged, subs) = two_way_merge(&a_lines, &b_lines);
                    if !merged.is_empty() {
                        segments.push(MergeSegment::Plain(merged));
                    }
                    for &(i1, i2, j1, j2) in &subs {
                        push_conflict(
                            &mut segments,
                            &mut conflict_index,
                            "insert-race",
                            Vec::new(),
                            a_lines[i1..i2].to_vec(),
                            b_lines[j1..j2].to_vec(),
                            (aj.unwrap().0 + i1, aj.unwrap().0 + i2),
                            (bj.unwrap().0 + j1, bj.unwrap().0 + j2),
                        );
                    }
                    if subs.is_empty() {
                        auto += 1;
                    }
                }
            } else if aj.is_some() {
                // 单方插入、对方在该位置无任何动作 → 无歧义
                segments.push(MergeSegment::Plain(a_lines));
                auto += 1;
            } else {
                segments.push(MergeSegment::Plain(b_lines));
                auto += 1;
            }
        }
        prev = Some(pt);
    }
    Ok((segments, auto))
}

/// 把 segments 渲染成带冲突标记的文件内容（写入 partial/resolved 文件用，
/// 与 skill 脚本输出的标记格式一致）。写前统一规范化行尾：除末行外必须
/// 以 \n 结尾，否则冲突标记会与上一行粘连。
pub(crate) fn render_with_markers(segments: &[MergeSegment]) -> String {
    let mut lines: Vec<String> = Vec::new();
    for seg in segments {
        match seg {
            MergeSegment::Plain(seg_lines) => lines.extend(seg_lines.iter().cloned()),
            MergeSegment::Conflict(h) => {
                lines.push(format!("{MARK_OURS}\n"));
                lines.extend(h.ours.iter().cloned());
                lines.push(format!("{MARK_SEP}\n"));
                lines.extend(h.theirs.iter().cloned());
                lines.push(format!("{MARK_THEIRS}\n"));
            }
        }
    }
    normalize_newlines(lines).concat()
}

// ---------------------------------------------------------------- git 基础

/// 字节版 git 执行器（blob 内容不能走 lossy String，否则二进制/编码会被破坏）
async fn git_bytes(cwd: &Path, args: &[&str], timeout_ms: u64) -> Result<Vec<u8>, String> {
    let fut = Command::new("git").args(args).current_dir(cwd).output();
    match timeout(Duration::from_millis(timeout_ms), fut).await {
        Ok(Ok(output)) if output.status.success() => Ok(output.stdout),
        Ok(Ok(output)) => Err(format!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        )),
        Ok(Err(err)) => Err(format!("git {} 启动失败: {err}", args.join(" "))),
        Err(_) => Err(format!(
            "git {} timed out after {timeout_ms}ms",
            args.join(" ")
        )),
    }
}

async fn read_blob(worktree: &Path, oid: &str) -> Result<BlobText, String> {
    let data = git_bytes(worktree, &["cat-file", "blob", oid], GIT_TIMEOUT_MS).await?;
    if data.contains(&0u8) {
        return Ok(BlobText::Binary);
    }
    // 非 UTF-8 文本按二进制保守处理（避免 lossy 重写破坏源文件字节）
    match String::from_utf8(data) {
        Ok(s) => Ok(BlobText::Text(s)),
        Err(_) => Ok(BlobText::Binary),
    }
}

type UnmergedStages = BTreeMap<u32, (String, String)>;

/// git ls-files -u -z → {path: {stage: (mode, oid)}}；记录格式 <mode> <oid> <stage>\t<path>\0
async fn collect_unmerged(worktree: &Path) -> Result<BTreeMap<String, UnmergedStages>, String> {
    let raw = git_bytes(worktree, &["ls-files", "-u", "-z"], GIT_TIMEOUT_MS).await?;
    let mut entries: BTreeMap<String, UnmergedStages> = BTreeMap::new();
    for rec in raw.split(|b| *b == 0) {
        if rec.is_empty() {
            continue;
        }
        let text = String::from_utf8_lossy(rec);
        let (meta, path) = match text.split_once('\t') {
            Some(v) => v,
            None => return Err(format!("unexpected ls-files -u record: {text:?}")),
        };
        let parts: Vec<&str> = meta.split_whitespace().collect();
        if parts.len() != 3 || !parts[2].chars().all(|c| c.is_ascii_digit()) {
            return Err(format!("unexpected ls-files -u record: {text:?}"));
        }
        let stage: u32 = parts[2]
            .parse()
            .map_err(|_| format!("bad stage: {text:?}"))?;
        entries
            .entry(path.to_string())
            .or_default()
            .insert(stage, (parts[0].to_string(), parts[1].to_string()));
    }
    Ok(entries)
}

/// worktree 中是否处于合并状态（存在 MERGE_HEAD）
pub(crate) async fn has_merge_head(worktree: &Path) -> bool {
    git(
        worktree,
        &["rev-parse", "-q", "--verify", "MERGE_HEAD"],
        30_000,
        COMMAND_OUTPUT_LIMIT,
    )
    .await
    .ok
}

/// 当前仍未合并（unmerged）的文件路径列表
pub(crate) async fn unmerged_paths(worktree: &Path) -> Vec<String> {
    collect_unmerged(worktree)
        .await
        .map(|m| m.keys().cloned().collect())
        .unwrap_or_default()
}

// ---------------------------------------------------------------- 文件处理

async fn write_worktree_file(worktree: &Path, path: &str, content: &str) -> Result<(), String> {
    let target = worktree.join(path);
    if let Some(parent) = target.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| format!("创建目录失败 {}: {e}", parent.display()))?;
    }
    tokio::fs::write(&target, content.as_bytes())
        .await
        .map_err(|e| format!("写文件失败 {}: {e}", target.display()))
}

async fn git_add(worktree: &Path, path: &str) -> Result<(), String> {
    let r = git(
        worktree,
        &["add", "--", path],
        GIT_TIMEOUT_MS,
        COMMAND_OUTPUT_LIMIT,
    )
    .await;
    if r.ok {
        Ok(())
    } else {
        Err(short_err(&r))
    }
}

async fn stage_side(
    worktree: &Path,
    path: &str,
    stages: &UnmergedStages,
    side: u32,
) -> Result<(), String> {
    if !stages.contains_key(&side) {
        return Err(format!("index 中不存在 stage {side} 版本：{path}"));
    }
    // 用 git checkout 按 index stage 恢复（文本/二进制/mode 均适用），再 stage
    let side_flag = if side == 2 { "--ours" } else { "--theirs" };
    let checkout = git(
        worktree,
        &["checkout", side_flag, "--", path],
        GIT_TIMEOUT_MS,
        COMMAND_OUTPUT_LIMIT,
    )
    .await;
    if !checkout.ok {
        return Err(short_err(&checkout));
    }
    git_add(worktree, path).await
}

async fn stage_delete(worktree: &Path, path: &str) -> Result<(), String> {
    let r = git(
        worktree,
        &["rm", "-f", "--", path],
        GIT_TIMEOUT_MS,
        COMMAND_OUTPUT_LIMIT,
    )
    .await;
    if r.ok {
        Ok(())
    } else {
        Err(short_err(&r))
    }
}

/// 处理单个 unmerged 文件；write=true 时写出结果（完全解决 → 写文件并 git add；
/// partial → 写入自动部分 + 剩余冲突标记但不 stage）
async fn process_unmerged_file(
    worktree: &Path,
    path: &str,
    stages: &UnmergedStages,
    write: bool,
) -> FileConflict {
    let mut fc = FileConflict {
        path: path.to_string(),
        status: "unresolvable",
        reason: None,
        segments: Vec::new(),
        auto_hunks: 0,
        has_ours: stages.contains_key(&2),
        has_theirs: stages.contains_key(&3),
        ours_text: String::new(),
        theirs_text: String::new(),
    };

    // modify/delete：语义选择，必须由人工决策
    if !stages.contains_key(&2) || !stages.contains_key(&3) {
        if stages.contains_key(&2) {
            fc.reason = Some(
                "deleted-by-them：本仓（目标分支）修改了该文件，需求分支删除了它。可保留本仓改动或接受删除"
                    .to_string(),
            );
        } else {
            fc.reason = Some(
                "deleted-by-us：本仓（目标分支）删除了该文件，需求分支修改了它。可保留需求分支版本或接受删除"
                    .to_string(),
            );
        }
        return fc;
    }

    let (mode_o, oid_o) = stages[&2].clone();
    let (mode_t, oid_t) = stages[&3].clone();
    let base_stage = stages.get(&1);

    if mode_o != mode_t {
        fc.reason = Some(format!("mode 冲突：ours {mode_o} vs theirs {mode_t}"));
        return fc;
    }
    if mode_o != "100644" && mode_o != "100755" {
        fc.reason = Some(format!("暂不支持的文件模式 {mode_o}（symlink/typechange）"));
        return fc;
    }

    let (our_text, their_text, base_text) = {
        let o = read_blob(worktree, &oid_o).await;
        let t = read_blob(worktree, &oid_t).await;
        let b = match base_stage {
            Some((_, oid)) => read_blob(worktree, oid).await,
            None => Ok(BlobText::Text(String::new())),
        };
        match (o, t, b) {
            (Ok(BlobText::Text(a)), Ok(BlobText::Text(b2)), Ok(BlobText::Text(c))) => (a, b2, c),
            _ => {
                fc.status = "binary";
                fc.reason = Some("二进制/非 UTF-8 内容无法按行合并".to_string());
                return fc;
            }
        }
    };
    fc.ours_text = our_text.clone();
    fc.theirs_text = their_text.clone();

    let ours = split_lines(&our_text);
    let theirs = split_lines(&their_text);
    let base = split_lines(&base_text);
    if has_conflict_marker(&ours) || has_conflict_marker(&theirs) || has_conflict_marker(&base) {
        fc.reason = Some("三方版本中已含冲突标记（嵌套冲突），不自动处理".to_string());
        return fc;
    }

    let (segments, auto) = match merge3(&base, &ours, &theirs) {
        Ok(v) => v,
        Err(err) => {
            fc.reason = Some(format!("三方合并内部异常，保守保留原始冲突：{err}"));
            return fc;
        }
    };
    let conflict_count = segments
        .iter()
        .filter(|s| matches!(s, MergeSegment::Conflict(_)))
        .count();
    fc.segments = segments;
    fc.auto_hunks = auto;
    if conflict_count == 0 {
        fc.status = "resolved";
        if write {
            let content = render_with_markers(&fc.segments);
            if let Err(err) = write_worktree_file(worktree, path, &content).await {
                fc.status = "unresolvable";
                fc.reason = Some(format!("写回解决结果失败：{err}"));
                return fc;
            }
            if let Err(err) = git_add(worktree, path).await {
                fc.status = "unresolvable";
                fc.reason = Some(format!("git add 失败：{err}"));
            }
        }
    } else {
        fc.status = "partial";
        if write {
            let content = render_with_markers(&fc.segments);
            // partial：写入自动部分 + 剩余标记，保持 unmerged、不 git add
            let _ = write_worktree_file(worktree, path, &content).await;
        }
    }
    fc
}

/// 对 worktree 中全部 unmerged 文件做细粒度三方合并；write=true 时写出结果并
/// stage 完全解决的文件。任一文件处理抛内部错误时整体报错（不写任何文件）。
pub(crate) async fn auto_resolve_worktree_conflicts(
    worktree: &Path,
    write: bool,
) -> Result<MergeConflictReport, String> {
    let entries = collect_unmerged(worktree).await?;
    let mut report = MergeConflictReport::default();
    for (path, stages) in &entries {
        let fc = process_unmerged_file(worktree, path, stages, write).await;
        match fc.status {
            "resolved" => {
                report.auto_resolved_files += 1;
            }
            "partial" => {
                report.remaining_files += 1;
                report.remaining_hunks += fc
                    .segments
                    .iter()
                    .filter(|s| matches!(s, MergeSegment::Conflict(_)))
                    .count();
            }
            _ => {
                report.unresolvable_files += 1;
            }
        }
        report.auto_hunks += fc.auto_hunks;
        report.files.push(fc);
    }
    Ok(report)
}

// ---------------------------------------------------------------- 人工解决

pub(crate) enum ResolveAction {
    /// 用户在冲突页编辑出的完整文件内容
    Content(String),
    /// 保留 ours（stage2）版本并 stage
    KeepOurs,
    /// 保留 theirs（stage3）版本并 stage
    KeepTheirs,
    /// 接受删除（git rm）
    AcceptDelete,
}

pub(crate) fn resolve_action_from_str(
    action: Option<&str>,
    content: Option<String>,
) -> Result<ResolveAction, String> {
    match action {
        Some("keep-ours") => Ok(ResolveAction::KeepOurs),
        Some("keep-theirs") => Ok(ResolveAction::KeepTheirs),
        Some("accept-delete") => Ok(ResolveAction::AcceptDelete),
        Some("content") | None => match content {
            Some(c) => Ok(ResolveAction::Content(c)),
            None => Err("缺少 content 或 action".to_string()),
        },
        Some(other) => Err(format!("未知 action：{other}")),
    }
}

/// 应用人工解决结果；成功后该文件退出 unmerged 状态
pub(crate) async fn apply_resolution(
    worktree: &Path,
    path: &str,
    action: ResolveAction,
) -> Result<(), String> {
    match action {
        ResolveAction::Content(content) => {
            if let Some(marker) = find_conflict_marker_in_content(&content) {
                return Err(format!(
                    "解决内容中仍含冲突标记 {marker}，请先处理所有冲突块"
                ));
            }
            write_worktree_file(worktree, path, &content).await?;
            git_add(worktree, path).await
        }
        ResolveAction::KeepOurs | ResolveAction::KeepTheirs => {
            let entries = collect_unmerged(worktree).await?;
            let Some(stages) = entries.get(path) else {
                return Err(format!("该文件已不在冲突状态：{path}"));
            };
            let side = if matches!(action, ResolveAction::KeepOurs) {
                2
            } else {
                3
            };
            stage_side(worktree, path, stages, side).await
        }
        ResolveAction::AcceptDelete => {
            let entries = collect_unmerged(worktree).await?;
            if !entries.contains_key(path) {
                return Err(format!("该文件已不在冲突状态：{path}"));
            }
            stage_delete(worktree, path).await
        }
    }
}

/// 完成合并：无 unmerged → （有 MERGE_HEAD 则）commit --no-edit → push。
/// 返回成功消息；失败时 Err（worktree 保持现状，可重试或人工处理）。
pub(crate) async fn complete_merge(worktree: &Path, target_branch: &str) -> Result<String, String> {
    let remaining = unmerged_paths(worktree).await;
    if !remaining.is_empty() {
        return Err(format!(
            "仍有 {} 个文件未解决冲突：{}",
            remaining.len(),
            remaining.join(", ")
        ));
    }
    if has_merge_head(worktree).await {
        let commit = git(
            worktree,
            &["commit", "--no-edit"],
            GIT_TIMEOUT_MS,
            COMMAND_OUTPUT_LIMIT,
        )
        .await;
        if !commit.ok {
            return Err(format!("git commit 失败：{}", short_err(&commit)));
        }
    }
    let push_ref = format!("HEAD:refs/heads/{target_branch}");
    let push = git(
        worktree,
        &["push", "origin", &push_ref],
        120_000,
        COMMAND_OUTPUT_LIMIT,
    )
    .await;
    if !push.ok {
        return Err(format!("推送 {target_branch} 失败：{}", short_err(&push)));
    }
    Ok(format!("合并已提交并推送到 {target_branch}"))
}

// ---------------------------------------------------------------- 详情 DTO

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum SegmentDto {
    Plain {
        lines: Vec<String>,
    },
    Conflict {
        index: usize,
        kind: &'static str,
        base: Vec<String>,
        ours: Vec<String>,
        theirs: Vec<String>,
        ours_start: usize,
        ours_end: usize,
        theirs_start: usize,
        theirs_end: usize,
    },
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ConflictFileDto {
    path: String,
    status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<String>,
    auto_hunks: usize,
    conflict_count: usize,
    has_ours: bool,
    has_theirs: bool,
    ours_text: String,
    theirs_text: String,
    segments: Vec<SegmentDto>,
}

fn file_dto(fc: &FileConflict) -> ConflictFileDto {
    let segments = fc
        .segments
        .iter()
        .map(|s| match s {
            MergeSegment::Plain(lines) => SegmentDto::Plain {
                lines: lines.clone(),
            },
            MergeSegment::Conflict(h) => SegmentDto::Conflict {
                index: h.index,
                kind: h.kind,
                base: h.base.clone(),
                ours: h.ours.clone(),
                theirs: h.theirs.clone(),
                ours_start: h.ours_start,
                ours_end: h.ours_end,
                theirs_start: h.theirs_start,
                theirs_end: h.theirs_end,
            },
        })
        .collect();
    ConflictFileDto {
        path: fc.path.clone(),
        status: fc.status,
        reason: fc.reason.clone(),
        auto_hunks: fc.auto_hunks,
        conflict_count: fc
            .segments
            .iter()
            .filter(|s| matches!(s, MergeSegment::Conflict(_)))
            .count(),
        has_ours: fc.has_ours,
        has_theirs: fc.has_theirs,
        ours_text: fc.ours_text.clone(),
        theirs_text: fc.theirs_text.clone(),
        segments,
    }
}

/// 冲突解决页详情：无状态地从 worktree index 三方版本重算 segments。
/// mergeState: conflicts=还有未解决冲突 / readyToCommit=已解决待提交 /
/// readyToPush=已提交待推送（此前自动完成时 commit/push 失败的遗留）。
pub(crate) async fn build_conflict_detail(worktree: &Path) -> Result<Value, String> {
    let report = auto_resolve_worktree_conflicts(worktree, false).await?;
    let has_unmerged = !report.files.is_empty();
    let merge_head = has_merge_head(worktree).await;
    let merge_state = if has_unmerged {
        "conflicts"
    } else if merge_head {
        "readyToCommit"
    } else {
        "readyToPush"
    };
    let files: Vec<ConflictFileDto> = report.files.iter().map(file_dto).collect();
    let conflicts = files.iter().map(|f| f.conflict_count).sum::<usize>();
    Ok(json!({
        "mergeState": merge_state,
        "summary": {
            "files": files.len(),
            "conflicts": conflicts,
            "autoResolvedFiles": report.auto_resolved_files,
            "autoHunks": report.auto_hunks,
            "unresolvableFiles": report.unresolvable_files,
        },
        "files": files,
    }))
}

// ---------------------------------------------------------------- tests

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| format!("{s}\n")).collect()
    }

    fn conflicts_of(segments: &[MergeSegment]) -> Vec<&ConflictHunk> {
        segments
            .iter()
            .filter_map(|s| match s {
                MergeSegment::Conflict(h) => Some(h),
                _ => None,
            })
            .collect()
    }

    fn plain_text(segments: &[MergeSegment]) -> String {
        let mut out = String::new();
        for s in segments {
            if let MergeSegment::Plain(ls) = s {
                for l in ls {
                    out.push_str(l);
                }
            }
        }
        out
    }

    #[test]
    fn one_side_change_is_auto_resolved() {
        let base = lines(&["a", "b", "c"]);
        let ours = lines(&["a", "B", "c"]);
        let theirs = base.clone();
        let (segments, auto) = merge3(&base, &ours, &theirs).unwrap();
        assert!(conflicts_of(&segments).is_empty());
        assert_eq!(plain_text(&segments), "a\nB\nc\n");
        assert_eq!(auto, 1);
    }

    #[test]
    fn both_same_change_is_auto_resolved() {
        let base = lines(&["a", "b", "c"]);
        let ours = lines(&["a", "B", "c"]);
        let theirs = lines(&["a", "B", "c"]);
        let (segments, auto) = merge3(&base, &ours, &theirs).unwrap();
        assert!(conflicts_of(&segments).is_empty());
        assert_eq!(plain_text(&segments), "a\nB\nc\n");
        assert_eq!(auto, 1);
    }

    #[test]
    fn single_side_insert_is_auto_resolved() {
        let base = lines(&["a", "c"]);
        let ours = lines(&["a", "x", "c"]);
        let theirs = base.clone();
        let (segments, auto) = merge3(&base, &ours, &theirs).unwrap();
        assert!(conflicts_of(&segments).is_empty());
        assert_eq!(plain_text(&segments), "a\nx\nc\n");
        assert_eq!(auto, 1);
    }

    #[test]
    fn true_content_conflict_is_kept() {
        let base = lines(&["a", "b", "c"]);
        let ours = lines(&["a", "B1", "c"]);
        let theirs = lines(&["a", "B2", "c"]);
        let (segments, auto) = merge3(&base, &ours, &theirs).unwrap();
        let conflicts = conflicts_of(&segments);
        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0].kind, "content");
        assert_eq!(conflicts[0].ours, lines(&["B1"]));
        assert_eq!(conflicts[0].theirs, lines(&["B2"]));
        assert_eq!(conflicts[0].base, lines(&["b"]));
        // 行区间指向 ours/theirs 全文件中的位置（0-based 半开）
        assert_eq!((conflicts[0].ours_start, conflicts[0].ours_end), (1, 2));
        assert_eq!((conflicts[0].theirs_start, conflicts[0].theirs_end), (1, 2));
        assert_eq!(auto, 0);
        // 渲染标记
        let markers = render_with_markers(&segments);
        assert!(markers.contains("<<<<<<< ours\nB1\n=======\nB2\n>>>>>>> theirs\n"));
    }

    #[test]
    fn insert_race_reorder_competition_is_kept_as_conflict() {
        // 重排竞争 [A,B] vs [B,A]：顺序歧义，必须保留冲突（保守性回归）。
        // 注意：不同 diff 算法可能对齐成 1 个 replace 或 delete+insert 两个子块，
        // 保守性只要求冲突不被吞掉，不要求块数一致。
        let base = lines(&[]);
        let ours = lines(&["A", "B"]);
        let theirs = lines(&["B", "A"]);
        let (segments, _) = merge3(&base, &ours, &theirs).unwrap();
        let conflicts = conflicts_of(&segments);
        assert!(
            !conflicts.is_empty(),
            "reorder competition must stay conflicted"
        );
        assert!(conflicts.iter().all(|h| h.kind == "insert-race"));
    }

    #[test]
    fn two_way_unique_addition_after_anchor_auto_merges() {
        // 单侧独有且对方内容中不出现过的新增行：新增语义无歧义 → 保留自动合并。
        // 注意 [X,A] vs [X,B] 这种“同位置双方都改不同内容”的情况，无论 difflib
        // 还是 Myers 都对齐成 replace → 保留冲突（见 two_way_opposing_replacement_conflicts）。
        let a = lines(&["A"]);
        let b = lines(&["A", "B"]);
        let (out, subs) = two_way_merge(&a, &b);
        assert!(subs.is_empty(), "unique addition must not conflict");
        let text: String = out.concat();
        assert!(text.contains("A\n") && text.contains("B\n"));
    }

    #[test]
    fn two_way_opposing_replacement_conflicts() {
        // 双方在同一位置插入不同内容且无共享行：整块冲突
        let a = lines(&["A1", "A2"]);
        let b = lines(&["B1", "B2"]);
        let (_, subs) = two_way_merge(&a, &b);
        assert!(!subs.is_empty());
    }

    #[test]
    fn insert_race_duplicate_competition_is_kept_as_conflict() {
        // 重复行竞争 [X] vs [X,X]：对齐多义，必须保留冲突（保守性回归）
        let base = lines(&[]);
        let ours = lines(&["X"]);
        let theirs = lines(&["X", "X"]);
        let (segments, _) = merge3(&base, &ours, &theirs).unwrap();
        assert!(!conflicts_of(&segments).is_empty());
    }

    #[test]
    fn content_marker_validation() {
        assert_eq!(find_conflict_marker_in_content("ok\nfine\n"), None);
        assert!(find_conflict_marker_in_content("ok\n<<<<<<< ours\n").is_some());
        assert!(find_conflict_marker_in_content("ok\n>>>>>>> theirs\n").is_some());
        // 单独的 ======= 行在 markdown/RST 中合法，允许
        assert_eq!(find_conflict_marker_in_content("title\n=======\n"), None);
    }

    #[tokio::test]
    async fn full_repo_flow_auto_resolve_and_manual_finish() {
        use std::process::Command as StdCommand;

        fn run(dir: &Path, args: &[&str]) {
            let out = StdCommand::new("git")
                .args(args)
                .current_dir(dir)
                .env("GIT_AUTHOR_NAME", "t")
                .env("GIT_AUTHOR_EMAIL", "t@t")
                .env("GIT_COMMITTER_NAME", "t")
                .env("GIT_COMMITTER_EMAIL", "t@t")
                .output()
                .expect("git ok");
            assert!(
                out.status.success(),
                "git {args:?} failed: {}",
                String::from_utf8_lossy(&out.stderr)
            );
        }

        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let remote = root.join("remote.git");
        let repo = root.join("repo");
        run(
            root,
            &[
                "init",
                "--bare",
                "--initial-branch=main",
                remote.to_str().unwrap(),
            ],
        );
        run(
            root,
            &["clone", remote.to_str().unwrap(), repo.to_str().unwrap()],
        );
        run(&repo, &["config", "user.email", "t@t"]);
        run(&repo, &["config", "user.name", "t"]);

        // base 提交
        std::fs::write(repo.join("app.txt"), "line1\nline2\nline3\n").unwrap();
        run(&repo, &["add", "."]);
        run(&repo, &["commit", "-m", "base"]);
        run(&repo, &["push", "origin", "HEAD:refs/heads/main"]);

        // 需求分支改 line2
        run(&repo, &["checkout", "-b", "feat/req"]);
        std::fs::write(repo.join("app.txt"), "line1\nreq-line2\nline3\n").unwrap();
        run(&repo, &["commit", "-am", "req change"]);

        // 目标分支 main 同区域改成不同内容（真冲突）+ 新增文件（both-added 会走 merge3 无冲突路径这里不造）
        run(&repo, &["checkout", "main"]);
        std::fs::write(repo.join("app.txt"), "line1\nmain-line2\nline3\n").unwrap();
        run(&repo, &["commit", "-am", "main change"]);

        // 触发合并冲突
        let merge = StdCommand::new("git")
            .args(["merge", "--no-ff", "--no-edit", "feat/req"])
            .current_dir(&repo)
            .output()
            .unwrap();
        assert!(!merge.status.success(), "expected merge conflict");

        // 自动分流：此场景 1 处真冲突 → partial
        let report = auto_resolve_worktree_conflicts(&repo, true).await.unwrap();
        assert_eq!(report.remaining_files, 1);
        assert_eq!(report.remaining_hunks, 1);
        assert_eq!(report.unresolvable_files, 0);
        let partial = std::fs::read_to_string(repo.join("app.txt")).unwrap();
        assert!(partial.contains("<<<<<<< ours"));
        assert!(partial.contains("main-line2"));
        assert!(partial.contains("req-line2"));
        assert!(partial.contains(">>>>>>> theirs"));

        // 详情：conflicts 状态 + 结构化 segments
        let detail = build_conflict_detail(&repo).await.unwrap();
        assert_eq!(detail["mergeState"], "conflicts");
        assert_eq!(detail["summary"]["conflicts"], 1);

        // 人工解决：取需求分支（theirs）版本
        apply_resolution(
            &repo,
            "app.txt",
            ResolveAction::Content("line1\nreq-line2\nline3\n".to_string()),
        )
        .await
        .unwrap();
        assert!(unmerged_paths(&repo).await.is_empty());

        // 完成合并：commit + push 到 bare remote
        let msg = complete_merge(&repo, "main").await.unwrap();
        assert!(msg.contains("main"));
        let detail = build_conflict_detail(&repo).await.unwrap();
        assert_eq!(detail["mergeState"], "readyToPush");

        // 远端确实收到合并提交
        run(&repo, &["fetch", "origin"]);
        let out = StdCommand::new("git")
            .args(["rev-parse", "origin/main"])
            .current_dir(&repo)
            .output()
            .unwrap();
        let remote_head = String::from_utf8_lossy(&out.stdout).trim().to_string();
        let out = StdCommand::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(&repo)
            .output()
            .unwrap();
        assert_eq!(remote_head, String::from_utf8_lossy(&out.stdout).trim());
    }

    #[tokio::test]
    async fn modify_delete_flow_and_keep_ours() {
        use std::process::Command as StdCommand;

        fn run(dir: &Path, args: &[&str]) {
            let out = StdCommand::new("git")
                .args(args)
                .current_dir(dir)
                .env("GIT_AUTHOR_NAME", "t")
                .env("GIT_AUTHOR_EMAIL", "t@t")
                .env("GIT_COMMITTER_NAME", "t")
                .env("GIT_COMMITTER_EMAIL", "t@t")
                .output()
                .expect("git ok");
            assert!(
                out.status.success(),
                "git {args:?} failed: {}",
                String::from_utf8_lossy(&out.stderr)
            );
        }

        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        run(
            tmp.path(),
            &["init", "--initial-branch=main", repo.to_str().unwrap()],
        );
        run(&repo, &["config", "user.email", "t@t"]);
        run(&repo, &["config", "user.name", "t"]);

        std::fs::write(repo.join("keep.txt"), "keep\n").unwrap();
        std::fs::write(repo.join("gone.txt"), "v1\n").unwrap();
        run(&repo, &["add", "."]);
        run(&repo, &["commit", "-m", "base"]);

        run(&repo, &["checkout", "-b", "feat/req"]);
        std::fs::write(repo.join("keep.txt"), "keep-req\n").unwrap();
        std::fs::remove_file(repo.join("gone.txt")).unwrap();
        run(&repo, &["commit", "-am", "req edit + delete"]);

        run(&repo, &["checkout", "main"]);
        std::fs::write(repo.join("keep.txt"), "keep-main\n").unwrap();
        std::fs::write(repo.join("gone.txt"), "v2-main\n").unwrap();
        run(&repo, &["commit", "-am", "main edit + keep gone"]);

        let merge = StdCommand::new("git")
            .args(["merge", "--no-ff", "--no-edit", "feat/req"])
            .current_dir(&repo)
            .output()
            .unwrap();
        assert!(!merge.status.success());

        let report = auto_resolve_worktree_conflicts(&repo, true).await.unwrap();
        // keep.txt 真冲突 1 处；gone.txt ours 修改 / theirs 删除 → unresolvable
        assert_eq!(report.remaining_hunks, 1);
        assert_eq!(report.unresolvable_files, 1);
        let gone = report.files.iter().find(|f| f.path == "gone.txt").unwrap();
        assert_eq!(gone.status, "unresolvable");
        assert!(gone.reason.as_deref().unwrap().contains("deleted-by-them"));
        assert!(gone.has_ours && !gone.has_theirs);

        // keep.txt 人工解决；gone.txt 保留 ours（main 修改后的 v2-main）并 stage
        apply_resolution(
            &repo,
            "keep.txt",
            ResolveAction::Content("merged-both\n".to_string()),
        )
        .await
        .unwrap();
        apply_resolution(&repo, "gone.txt", ResolveAction::KeepOurs)
            .await
            .unwrap();
        assert!(unmerged_paths(&repo).await.is_empty());

        let commit = git(
            &repo,
            &["commit", "--no-edit"],
            30_000,
            COMMAND_OUTPUT_LIMIT,
        )
        .await;
        assert!(commit.ok, "commit should succeed: {}", short_err(&commit));
        // keep.txt 人工内容，gone.txt 保留了 ours 版本
        assert_eq!(
            std::fs::read_to_string(repo.join("keep.txt")).unwrap(),
            "merged-both\n"
        );
        assert_eq!(
            std::fs::read_to_string(repo.join("gone.txt")).unwrap(),
            "v2-main\n"
        );
    }

    #[tokio::test]
    async fn keep_theirs_restores_deleted_side() {
        use std::process::Command as StdCommand;

        fn run(dir: &Path, args: &[&str]) {
            let out = StdCommand::new("git")
                .args(args)
                .current_dir(dir)
                .env("GIT_AUTHOR_NAME", "t")
                .env("GIT_AUTHOR_EMAIL", "t@t")
                .env("GIT_COMMITTER_NAME", "t")
                .env("GIT_COMMITTER_EMAIL", "t@t")
                .output()
                .expect("git ok");
            assert!(out.status.success());
        }

        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        run(
            tmp.path(),
            &["init", "--initial-branch=main", repo.to_str().unwrap()],
        );
        run(&repo, &["config", "user.email", "t@t"]);
        run(&repo, &["config", "user.name", "t"]);

        std::fs::write(repo.join("f.txt"), "v1\n").unwrap();
        run(&repo, &["add", "."]);
        run(&repo, &["commit", "-m", "base"]);

        run(&repo, &["checkout", "-b", "feat/req"]);
        std::fs::write(repo.join("f.txt"), "v2-req\n").unwrap();
        run(&repo, &["commit", "-am", "req edit"]);

        run(&repo, &["checkout", "main"]);
        std::fs::remove_file(repo.join("f.txt")).unwrap();
        run(&repo, &["commit", "-am", "main delete"]);

        let merge = StdCommand::new("git")
            .args(["merge", "--no-ff", "--no-edit", "feat/req"])
            .current_dir(&repo)
            .output()
            .unwrap();
        assert!(!merge.status.success());

        let report = auto_resolve_worktree_conflicts(&repo, true).await.unwrap();
        assert_eq!(report.unresolvable_files, 1);
        let f = report.files.iter().find(|x| x.path == "f.txt").unwrap();
        assert!(f.reason.as_deref().unwrap().contains("deleted-by-us"));

        apply_resolution(&repo, "f.txt", ResolveAction::KeepTheirs)
            .await
            .unwrap();
        assert!(unmerged_paths(&repo).await.is_empty());
        assert_eq!(
            std::fs::read_to_string(repo.join("f.txt")).unwrap(),
            "v2-req\n"
        );
    }
}
