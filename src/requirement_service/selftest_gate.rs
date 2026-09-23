use super::*;

const SELFTEST_SECTION: &str = "自测清单";

/// 自测清单三个必选分类小节（`###` 级标题）。
const CATEGORY_MAIN: &str = "主流程测试";
const CATEGORY_BOUNDARY: &str = "边界场景测试";
const CATEGORY_CONCURRENCY: &str = "高并发/大流量场景测试";
const RISK_ANALYSIS_HEADING: &str = "风险场景分析";

/// 「自测清单」结果单元格的分类。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SelftestResult {
    /// 通过
    Pass,
    /// 执行了但失败（必须写明原因；存在即门禁不放行）
    Fail,
    /// 被阻碍无法知道结果：无法测试/跳过等（必须写明原因；放行但门禁警示）
    CannotTest,
    /// 缺少测试结果（门禁不通过）
    Missing,
}

/// 结果单元格为这些值（trim + 小写后整格匹配）时视为缺少测试结果。
const MISSING_RESULT_CELLS: [&str; 15] = [
    "", "-", "—", "--", "/", "无", "空", "待测试", "待测", "未测试", "未测", "待执行", "待补充",
    "待定", "未执行",
];

/// 原因/说明单元格为这些值时视为没有实际内容。
const PLACEHOLDER_CELLS: [&str; 10] = [
    "", "-", "—", "--", "/", "无", "空", "待补充", "todo", "tbd",
];

/// 自测清单校验结果：problems 非空 = 门禁不放行；warnings 非空 = 放行但需警示（结果未知）。
#[derive(Debug, Default)]
pub(crate) struct SelftestChecklistEval {
    pub(crate) problems: Vec<String>,
    pub(crate) warnings: Vec<String>,
}

/// 自测门禁：自测中推进「测试中」前，test.md 必须有「自测清单」且每项都有测试结果。
/// 全部通过 → 放行；存在「无法测试/跳过」（有原因）→ 放行但警示；存在「失败」→ 不放行。
pub(crate) async fn ensure_selftest_checklist_allows_testing(req: &Requirement) -> ApiResult<()> {
    let eval = selftest_checklist_eval(req).await;
    if eval.problems.is_empty() {
        return Ok(());
    }
    Err(selftest_gate_error(eval.problems))
}

/// 收集自测清单当前问题；空列表 = 通过。供门禁拦截与状态流转卡片只读评估复用。
pub(crate) async fn selftest_checklist_problems(req: &Requirement) -> Vec<String> {
    selftest_checklist_eval(req).await.problems
}

/// 收集自测清单评估（问题 + 警示）：供状态流转卡片展示 ⚠。
pub(crate) async fn selftest_checklist_eval(req: &Requirement) -> SelftestChecklistEval {
    // 抢修模式：关联了线上问题的需求默认放行自测门禁（速度优先；用户明确要求自测时再按常规维护）。
    if req.is_hotfix() {
        return SelftestChecklistEval::default();
    }
    let Some(dir) = req.req_dir.as_deref() else {
        return SelftestChecklistEval::default();
    };
    let path = PathBuf::from(dir).join("test.md");
    if !path.is_file() {
        return SelftestChecklistEval {
            problems: vec!["未找到 test.md".to_string()],
            warnings: Vec::new(),
        };
    }
    let body = tokio::fs::read_to_string(&path).await.unwrap_or_default();
    validate_selftest_checklist(&body)
}

fn selftest_gate_error(problems: Vec<String>) -> ApiError {
    ApiError::bad_request(format!(
        "自测门禁未通过：自测中推进「测试中」前，test.md 的「## {SELFTEST_SECTION}」必须分三类小节（主流程测试 / 边界场景测试 / 高并发·大流量场景测试），每项都要有测试结果（通过/失败/无法测试）。全部通过直接放行；存在「无法测试/跳过」（写明原因）放行但门禁警示结果未知；存在「失败」不放行，失败项必须写明具体原因。当前问题：\n{}\n期望格式：\n### {CATEGORY_MAIN}\n| # | 自测项 | 结果 | 失败/无法测试原因 |\n| --- | --- | --- | --- |\n| 1 | 下单主流程 | 通过 | - |\n\n### {CATEGORY_BOUNDARY}\n#### {RISK_ANALYSIS_HEADING}\n- boxCode 传 null/空串时查询落空\n#### 测试清单\n| # | 自测项 | 结果 | 失败/无法测试原因 |\n| --- | --- | --- | --- |\n| 1 | boxCode=null 查询 | 通过 | - |\n\n### {CATEGORY_CONCURRENCY}\n#### {RISK_ANALYSIS_HEADING}\n- MQ 重复消费时库存重复释放\n#### 测试清单\n| # | 自测项 | 结果 | 失败/无法测试原因 |\n| --- | --- | --- | --- |\n| 1 | 同一单 MQ 重复投递 | 通过 | - |",
        problems
            .iter()
            .map(|p| format!("- {p}"))
            .collect::<Vec<_>>()
            .join("\n")
    ))
}

/// 按分类标题关键字归类小节；返回 None 表示不属于任何分类。
fn category_of_heading(title: &str) -> Option<&'static str> {
    if title.contains("主流程") {
        Some(CATEGORY_MAIN)
    } else if title.contains("边界") {
        Some(CATEGORY_BOUNDARY)
    } else if title.contains("并发") || title.contains("大流量") || title.contains("压测") {
        Some(CATEGORY_CONCURRENCY)
    } else {
        None
    }
}

/// 把「自测清单」小节拆成分类块：返回（分类块列表，未归属任何分类的孤儿内容）。
/// 分类块从分类标题开始，到下一个分类标题或同级/更高级标题为止；
/// 更深层标题（如 `#### 风险场景分析`）留在块内。
fn split_category_blocks(section: &str) -> (Vec<(&'static str, String)>, String) {
    let mut blocks: Vec<(&'static str, String)> = Vec::new();
    let mut current: Option<(&'static str, usize, String)> = None;
    let mut orphan = String::new();
    for line in section.lines() {
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed.strip_prefix('#') {
            let level = rest.chars().take_while(|c| *c == '#').count();
            let title = rest.trim_start_matches('#').trim();
            let closes_current = matches!(&current, Some((_, cur_level, _)) if level <= *cur_level);
            if let Some(cat) = category_of_heading(title) {
                if let Some((cat_done, _, buf_done)) = current.take() {
                    blocks.push((cat_done, buf_done));
                }
                current = Some((cat, level, String::new()));
                continue;
            }
            if closes_current {
                if let Some((cat_done, _, buf_done)) = current.take() {
                    blocks.push((cat_done, buf_done));
                }
                orphan.push_str(line);
                orphan.push('\n');
                continue;
            }
            match current.as_mut() {
                Some((_, _, buf)) => {
                    buf.push_str(line);
                    buf.push('\n');
                }
                None => {
                    orphan.push_str(line);
                    orphan.push('\n');
                }
            }
            continue;
        }
        match current.as_mut() {
            Some((_, _, buf)) => {
                buf.push_str(line);
                buf.push('\n');
            }
            None => {
                orphan.push_str(line);
                orphan.push('\n');
            }
        }
    }
    if let Some((cat, _, buf)) = current.take() {
        blocks.push((cat, buf));
    }
    (blocks, orphan)
}

/// 截取分类块内「风险场景分析」小节正文（到下一个同级或更高级标题为止）。
fn extract_risk_analysis(body: &str) -> Option<String> {
    let mut section: Option<(usize, String)> = None;
    for line in body.lines() {
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed.strip_prefix('#') {
            let level = rest.chars().take_while(|c| *c == '#').count();
            let title = rest.trim_start_matches('#').trim();
            match section.as_mut() {
                Some((sec_level, buf)) => {
                    if level <= *sec_level {
                        break;
                    }
                    buf.push_str(line);
                    buf.push('\n');
                }
                None => {
                    if title.contains(RISK_ANALYSIS_HEADING) {
                        section = Some((level, String::new()));
                    }
                }
            }
            continue;
        }
        if let Some((_, buf)) = section.as_mut() {
            buf.push_str(line);
            buf.push('\n');
        }
    }
    section.map(|(_, buf)| buf)
}

/// 行内容去掉列表标记后是否有实质内容（≥4 字符且非占位符）。
fn meaningful_line(line: &str) -> bool {
    let v = line
        .trim()
        .trim_start_matches(['-', '*', '>', ' '])
        .trim()
        .trim_start_matches(|c: char| c.is_ascii_digit() || c == '.' || c == '、');
    meaningful_reason(v).is_some()
}

/// 分类块是否标注了不适用且有实质原因（`不适用：<原因>`）。
fn category_na_with_reason(body: &str) -> Option<bool> {
    let line = body.lines().find(|l| {
        let t = l.trim();
        !t.starts_with('|') && !t.starts_with('#') && t.contains("不适用")
    })?;
    let reason = line
        .trim()
        .trim_start_matches(['-', '*', ' '])
        .replacen("不适用", "", 1);
    Some(meaningful_reason(reason.trim()).is_some())
}

/// 「无风险」结论文行是否有依据（去掉关键词与标点后仍有实质内容）。
fn line_concludes_no_risk(line: &str) -> bool {
    let lower = line.to_lowercase();
    if !(lower.contains("无风险") || lower.contains("没有风险") || lower.contains("无并发风险") || lower.contains("无边界风险")) {
        return false;
    }
    let mut v = line.trim().to_string();
    for m in ["无风险", "没有风险", "无并发风险", "无边界风险", "风险场景", "不适用"] {
        v = v.replace(m, "");
    }
    let v = v.trim_matches(|c: char| {
        matches!(
            c,
            ':' | '：' | ',' | '，' | ';' | '；' | '、' | '(' | ')' | '（' | '）' | '-' | '—' | ' '
                | '.' | '。'
        )
    });
    v.chars().count() >= 4
}

/// 纯函数校验：返回问题 + 警示。problems 非空 = 不放行（缺分类/缺结果/存在失败项）；
/// warnings 非空 = 放行但警示（存在无法测试/跳过的项，结果未知）。
pub(crate) fn validate_selftest_checklist(body: &str) -> SelftestChecklistEval {
    let mut eval = SelftestChecklistEval::default();
    let Some(section) = extract_selftest_section(body) else {
        eval.problems.push(format!("未找到「## {SELFTEST_SECTION}」小节"));
        return eval;
    };
    let (blocks, orphan) = split_category_blocks(&section);
    for cat in [CATEGORY_MAIN, CATEGORY_BOUNDARY, CATEGORY_CONCURRENCY] {
        if !blocks.iter().any(|(c, _)| *c == cat) {
            eval.problems.push(format!(
                "缺少「### {cat}」分类小节：自测清单必须分三类（主流程测试 / 边界场景测试 / 高并发·大流量场景测试）"
            ));
        }
    }
    for (cat, block) in &blocks {
        let (problems, warnings) = validate_category_block(cat, block);
        eval.problems.extend(problems);
        eval.warnings.extend(warnings);
    }
    // 未归属任何分类的表格（旧版平铺格式/分类小节外的表）也逐项检查，
    // 让 agent 一次看到结构与条目两类问题。
    let (orphan_count, orphan_problems, orphan_warnings) =
        validate_table_rows(&parse_table_rows(&orphan), "未归类表格");
    eval.problems.extend(orphan_problems);
    eval.warnings.extend(orphan_warnings);
    if !orphan.trim().is_empty() && orphan_count == 0 && blocks.is_empty() {
        eval.problems.push(format!(
            "「## {SELFTEST_SECTION}」里没有任何测试项目，请按三类分类小节逐项列出测试项"
        ));
    }
    eval
}

/// 校验单个分类块：逐项结果检查 + 边界/并发类的风险分析与清单检查。
/// 返回（问题，警示）。
fn validate_category_block(cat: &str, body: &str) -> (Vec<String>, Vec<String>) {
    let mut problems = Vec::new();
    let mut warnings = Vec::new();
    if cat != CATEGORY_MAIN {
        match category_na_with_reason(body) {
            Some(false) => {
                problems.push(format!(
                    "「{cat}」标注了不适用但没写明原因：请用 `不适用：<具体原因>`（如：纯文案改动，无边界输入与并发路径）"
                ));
                return (problems, warnings);
            }
            Some(true) => return (problems, warnings),
            None => {}
        }
    }
    let rows = parse_table_rows(body);
    let (item_count, item_problems, item_warnings) = validate_table_rows(&rows, cat);
    problems.extend(item_problems);
    warnings.extend(item_warnings);
    if cat == CATEGORY_MAIN {
        if item_count == 0 {
            problems.push(format!(
                "「{cat}」表格里没有任何测试项目，请逐项列出主流程测试项"
            ));
        }
        return (problems, warnings);
    }
    // 边界 / 高并发·大流量：先分析风险场景，再列清单，再完成测试。
    let Some(analysis) = extract_risk_analysis(body) else {
        problems.push(format!(
            "「{cat}」缺少「#### {RISK_ANALYSIS_HEADING}」小节：必须先分析出风险场景（每行一条），再把风险转成测试清单并完成；确认无风险写 `无风险场景：<依据>`，整类不适用写 `不适用：<原因>`"
        ));
        return (problems, warnings);
    };
    if !analysis.lines().any(meaningful_line) {
        problems.push(format!(
            "「{cat}」的风险场景分析为空：请逐行列出分析出的风险场景，或写明 `无风险场景：<依据>`"
        ));
        return (problems, warnings);
    }
    let concludes_no_risk = analysis.lines().any(line_concludes_no_risk);
    if item_count == 0 && !concludes_no_risk {
        problems.push(format!(
            "「{cat}」已列出风险场景但没有测试清单表：把每个风险场景转成可执行的测试项并逐项填写结果；确认无风险时写 `无风险场景：<依据>`"
        ));
    }
    (problems, warnings)
}

/// 逐项校验表格行：每项必须有测试结果；失败 → 问题（不放行）；无法测试/跳过（有原因）→ 警示。
/// 返回（有效测试项数，问题列表，警示列表）。
fn validate_table_rows(rows: &[Vec<String>], label: &str) -> (usize, Vec<String>, Vec<String>) {
    let Some(header) = rows.first() else {
        return (0, Vec::new(), Vec::new());
    };
    let Some(result_col) = find_result_column(header) else {
        return (
            0,
            vec![format!(
                "「{label}」表头缺少「结果」列，请使用表头：| # | 自测项 | 结果 | 失败/无法测试原因 |"
            )],
            Vec::new(),
        );
    };
    let mut problems = Vec::new();
    let mut warnings = Vec::new();
    let mut item_count = 0usize;
    for row in rows.iter().skip(1) {
        if is_header_row(row) || row.iter().all(|c| c.is_empty()) {
            continue;
        }
        item_count += 1;
        let item = row_item_label(row, result_col, item_count);
        let label = format!("{label} · {item}");
        let Some(result_cell) = row.get(result_col) else {
            problems.push(format!("「{label}」缺少测试结果"));
            continue;
        };
        match classify_result_cell(result_cell) {
            SelftestResult::Pass => {}
            SelftestResult::Fail => {
                let reason = row
                    .get(result_col + 1..)
                    .and_then(|cells| cells.iter().find_map(|c| meaningful_reason(c)))
                    .or_else(|| {
                        if has_inline_reason(result_cell) {
                            Some(String::new())
                        } else {
                            None
                        }
                    });
                if reason.is_none() {
                    problems.push(format!(
                        "「{label}」结果为「{}」（失败），必须写明具体的失败原因与影响，不能只写「-」或留空；存在失败项门禁不放行",
                        result_cell.trim()
                    ));
                } else {
                    problems.push(format!(
                        "「{label}」存在失败项（原因已记录）：测试不通过门禁不放行，修复后把结果改为通过再推进"
                    ));
                }
            }
            SelftestResult::CannotTest => {
                let reason = row
                    .get(result_col + 1..)
                    .and_then(|cells| cells.iter().find_map(|c| meaningful_reason(c)))
                    .or_else(|| {
                        if has_inline_reason(result_cell) {
                            Some(String::new())
                        } else {
                            None
                        }
                    });
                if reason.is_none() {
                    problems.push(format!(
                        "「{label}」结果为「{}」（无法测试），必须写明具体原因（如 test 环境缺依赖、服务未部署等确实无法完成的场景），不能只写「-」或留空",
                        result_cell.trim()
                    ));
                } else {
                    warnings.push(format!(
                        "「{label}」无法测试，结果未知——门禁放行但上线前需确认该场景"
                    ));
                }
            }
            SelftestResult::Missing => {
                let shown = result_cell.trim();
                let shown = if shown.is_empty() { "空" } else { shown };
                problems.push(format!(
                    "「{label}」缺少测试结果（当前：{shown}）；每项必须填写 通过/失败/无法测试"
                ));
            }
        }
    }
    (item_count, problems, warnings)
}

/// 截取 test.md 中「自测清单」小节正文（从匹配标题到下一个同级或更高级标题之前）。
fn extract_selftest_section(body: &str) -> Option<String> {
    let mut section: Option<(usize, String)> = None;
    for line in body.lines() {
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed.strip_prefix('#') {
            let level = rest.chars().take_while(|c| *c == '#').count();
            let title = rest.trim_start_matches('#').trim();
            match section.as_mut() {
                Some((sec_level, buf)) => {
                    if level <= *sec_level {
                        break;
                    }
                    buf.push_str(line);
                    buf.push('\n');
                }
                None => {
                    if title.contains(SELFTEST_SECTION) {
                        section = Some((level, String::new()));
                    }
                }
            }
            continue;
        }
        if let Some((_, buf)) = section.as_mut() {
            buf.push_str(line);
            buf.push('\n');
        }
    }
    section.map(|(_, buf)| buf)
}

/// 解析小节内的 markdown 表格行；跳过分隔行与全空行。
fn parse_table_rows(section: &str) -> Vec<Vec<String>> {
    let mut rows = Vec::new();
    for line in section.lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with('|') {
            continue;
        }
        let cells: Vec<String> = trimmed
            .trim_matches('|')
            .split('|')
            .map(|c| c.trim().to_string())
            .collect();
        if cells.iter().all(|c| c.is_empty() || is_separator_cell(c)) {
            continue;
        }
        rows.push(cells);
    }
    rows
}

fn is_separator_cell(cell: &str) -> bool {
    let inner = cell.trim_matches(':');
    !inner.is_empty() && inner.chars().all(|c| c == '-')
}

fn is_header_row(row: &[String]) -> bool {
    row.iter().any(|c| c.contains("自测项")) && row.iter().any(|c| c.contains("结果"))
}

fn find_result_column(header: &[String]) -> Option<usize> {
    header
        .iter()
        .position(|c| c.contains("结果") || c.to_lowercase().contains("result"))
}

fn classify_result_cell(cell: &str) -> SelftestResult {
    let v = cell.trim().to_lowercase();
    if MISSING_RESULT_CELLS.contains(&v.as_str())
        || matches!(v.as_str(), "todo" | "pending" | "n/a" | "na" | "⬜")
    {
        return SelftestResult::Missing;
    }
    // 先判失败再判无法测试：「无法通过」属于失败而非无法测试。
    let fail_markers = ["未通过", "不通过", "无法通过", "失败", "fail", "❌", "✖", "报错", "bug"];
    if fail_markers.iter().any(|m| v.contains(m)) {
        return SelftestResult::Fail;
    }
    let cannot_test_markers = [
        "无法测试",
        "无法验证",
        "没法测试",
        "不能测试",
        "阻塞",
        "blocked",
        "跳过",
        "skip",
    ];
    if cannot_test_markers.iter().any(|m| v.contains(m)) {
        return SelftestResult::CannotTest;
    }
    let pass_markers = ["通过", "成功", "pass", "ok", "✅", "✔"];
    if pass_markers.iter().any(|m| v.contains(m)) {
        return SelftestResult::Pass;
    }
    SelftestResult::Missing
}

/// 结果列内联写了原因（如「失败：test 环境未部署」）时也算有原因。
fn has_inline_reason(result_cell: &str) -> bool {
    let mut v = result_cell.trim().to_lowercase();
    for m in [
        "未通过", "不通过", "无法通过", "失败", "fail", "❌", "✖", "报错", "bug", "无法测试",
        "无法验证", "没法测试", "不能测试", "阻塞", "blocked", "跳过", "skip", "通过", "成功",
        "pass", "ok", "✅", "✔", "⬜",
    ] {
        v = v.replace(m, "");
    }
    let v = v.trim_matches(|c: char| {
        matches!(
            c,
            ':' | '：' | ',' | '，' | ';' | '；' | '、' | '(' | ')' | '（' | '）' | '-' | '—' | ' '
        )
    });
    v.chars().count() >= 4
}

fn meaningful_reason(cell: &str) -> Option<String> {
    let v = cell.trim();
    if PLACEHOLDER_CELLS.contains(&v.to_lowercase().as_str()) {
        return None;
    }
    if v.chars().count() < 4 {
        return None;
    }
    Some(v.to_string())
}

/// 从结果列之前的单元格中取测试项名称（跳过序号列和占位符）。
fn row_item_label(row: &[String], result_col: usize, row_no: usize) -> String {
    let end = result_col.min(row.len());
    row[..end]
        .iter()
        .rev()
        .find(|c| {
            !c.is_empty()
                && !PLACEHOLDER_CELLS.contains(&c.to_lowercase().as_str())
                && !c.chars().all(|ch| ch.is_ascii_digit())
        })
        .cloned()
        .unwrap_or_else(|| format!("第 {row_no} 项"))
}
