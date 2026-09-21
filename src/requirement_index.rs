use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};

use anyhow::{anyhow, Result};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::fs;

use crate::*;

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Requirement {
    pub(crate) id: String,
    pub(crate) title: String,
    pub(crate) status: String,
    pub(crate) projects: Vec<String>,
    pub(crate) project: String,
    pub(crate) group_path: Vec<String>,
    pub(crate) description: String,
    pub(crate) session_ids: Vec<String>,
    pub(crate) category: Option<String>,
    /// 需求推动方：产品推动（默认）/ 开发推动；开发推动必须先完成测试场景文档。
    pub(crate) source: String,
    pub(crate) ones: Option<String>,
    /// 绑定的线上问题 req id 列表（meta.md issues 字段，仅普通需求使用）。
    pub(crate) issues: Vec<String>,
    pub(crate) plan_release: Option<String>,
    pub(crate) created_at: i64,
    pub(crate) updated_at: i64,
    pub(crate) completed_at: Option<i64>,
    pub(crate) req_dir: Option<String>,
    pub(crate) meta_path: Option<String>,
    pub(crate) background_path: Option<String>,
    pub(crate) branch_path: Option<String>,
    pub(crate) test_path: Option<String>,
    pub(crate) notes_path: Option<String>,
    pub(crate) config_path: Option<String>,
    pub(crate) impact_path: Option<String>,
    pub(crate) memory_path: Option<String>,
    pub(crate) review_path: Option<String>,
    pub(crate) technical_plan_path: Option<String>,
    pub(crate) release_manifest_path: Option<String>,
    pub(crate) release_check_path: Option<String>,
    pub(crate) experience_summary_path: Option<String>,
    pub(crate) troubleshooting_path: Option<String>,
    pub(crate) incident_path: Option<String>,
    pub(crate) root_cause_path: Option<String>,
    pub(crate) test_scenario_path: Option<String>,
    pub(crate) experience_summary_job: Option<Value>,
    pub(crate) alignment_path: Option<String>,
    pub(crate) prd_path: Option<String>,
    pub(crate) effort_estimate: Option<Value>,
    /// 引用式需求组：本需求 group.json 的成员列表（Some 且非空 = 本需求是组）。
    pub(crate) group_members: Option<Vec<GroupMemberRef>>,
    /// 组发布策略：together | independent。
    pub(crate) group_policy: Option<String>,
    /// 本需求作为成员所属的需求组 req id 列表（扫描回填）。
    pub(crate) member_of: Vec<String>,
    /// 组聚合状态 = min(成员需求流状态序数)；非组或无可计算成员时为 None。
    pub(crate) group_status: Option<String>,
    /// 组瓶颈成员（聚合状态来源，最慢成员 req id）。
    pub(crate) group_bottleneck: Option<String>,
    /// 子需求：meta.md frontmatter `parent-req-id`；None = 不是子需求。
    pub(crate) parent_req_id: Option<String>,
    /// 派生字段：是否子需求（parent_req_id 非空）。
    pub(crate) is_sub_req: bool,
    /// 本需求作为父需求时拆出的子需求列表（扫描回填，按 reqId 查找子需求回填）。
    pub(crate) sub_reqs: Vec<SubReqRef>,
}

/// 引用式需求组成员引用。磁盘格式（group.json）只要求 `reqId`（可选 `note`）；
/// `title`/`status`/`found`/`nested` 由扫描时回填，仅存在于内存和 API 输出。
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GroupMemberRef {
    pub(crate) req_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) note: Option<String>,
    /// 扫描回填：成员需求标题。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) title: Option<String>,
    /// 扫描回填：成员当前状态。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) status: Option<String>,
    /// 扫描回填：成员需求是否在索引中找到（失效引用保留原样并标记）。
    #[serde(default = "group_member_default_found")]
    pub(crate) found: bool,
    /// 扫描回填：成员自身也是需求组（不支持嵌套，validate 报 problem）。
    #[serde(default)]
    pub(crate) nested: bool,
}

fn group_member_default_found() -> bool {
    true
}

/// group.json：引用式需求组定义。成员通过 reqId 引用独立存在的需求，
/// 不做目录搬家；组状态为派生值（min 成员需求流状态），不能手动设置。
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GroupFile {
    #[serde(default = "group_file_version")]
    pub(crate) version: u8,
    /// 发布策略：together（整体发布，release-check 聚合预检）| independent（成员独立发布，默认）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) release_policy: Option<String>,
    #[serde(default)]
    pub(crate) members: Vec<GroupMemberRef>,
}

fn group_file_version() -> u8 {
    1
}

/// 子需求引用（父需求视角）。子需求自身是完整需求记录（meta.md 写 parent-req-id，
/// 目录平铺在 req 根下）；title/status/found 由扫描回填，仅存在于内存和 API 输出。
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SubReqRef {
    pub(crate) req_id: String,
    /// 扫描回填：子需求标题。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) title: Option<String>,
    /// 扫描回填：子需求当前状态（子需求轻量状态机）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) status: Option<String>,
    /// 扫描回填：子需求是否在索引中找到。
    #[serde(default)]
    pub(crate) found: bool,
    /// 扫描回填：子需求是否已合入主需求（status=已合入）。
    #[serde(default)]
    pub(crate) merged: bool,
    /// 扫描回填：子需求是否已取消。
    #[serde(default)]
    pub(crate) cancelled: bool,
}

/// 从子需求 req id 提取序号：`WMS-049-S1-xxx` -> 1，`WMS-049-S2` -> 2。
pub(crate) fn sub_req_seq(req_id: &str) -> Option<u64> {
    let re = Regex::new(r"-S(\d+)(-|$)").ok()?;
    let caps = re.captures(req_id)?;
    caps[1].parse::<u64>().ok()
}

pub(crate) const GROUP_RELEASE_POLICIES: &[&str] = &["together", "independent"];

/// 需求流状态序数（越小越早）；线上问题/测试问题轻流程状态不参与聚合。
pub(crate) fn requirement_flow_status_rank(status: &str) -> Option<usize> {
    REQ_FLOW_STATUSES.iter().position(|s| *s == status)
}

/// 读取需求目录下的 group.json；文件不存在或解析失败返回 None（validate 会报告解析问题）。
pub(crate) async fn load_group_json(dir: &Path) -> Option<GroupFile> {
    let raw = fs::read_to_string(dir.join(GROUP_FILE)).await.ok()?;
    serde_json::from_str(&raw).ok()
}

/// 判断需求是否是引用式需求组（有非空 group.json 成员）。
pub(crate) async fn is_requirement_group(dir: &Path) -> bool {
    load_group_json(dir)
        .await
        .map(|g| !g.members.is_empty())
        .unwrap_or(false)
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StatusCount {
    pub(crate) status: String,
    pub(crate) count: usize,
    pub(crate) percent: f64,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ReleaseDayCount {
    pub(crate) date: String,
    pub(crate) count: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RequirementDuration {
    pub(crate) req: Requirement,
    pub(crate) duration_ms: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DashboardStats {
    pub(crate) total: usize,
    pub(crate) status_counts: Vec<StatusCount>,
    pub(crate) durations: Vec<RequirementDuration>,
    pub(crate) avg_delivery_ms: i64,
    pub(crate) median_delivery_ms: i64,
    pub(crate) max_delivery_ms: i64,
    pub(crate) completed_count: usize,
    pub(crate) in_progress_count: usize,
    pub(crate) release_schedule: Vec<ReleaseDayCount>,
    pub(crate) next_release: Option<ReleaseDayCount>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AssociationsStore {
    #[serde(default = "associations_version")]
    pub(crate) version: u8,
    #[serde(default)]
    pub(crate) associations: HashMap<String, Vec<String>>,
    /// Per-requirement pending (not-yet-used) terminal launch command. One per
    /// requirement: repeated "copy command" clicks reuse it until the session
    /// id has been used (a session file exists in the harness store), then the
    /// next copy auto-generates a fresh one. Force refresh discards it.
    #[serde(default)]
    pub(crate) pending_commands: HashMap<String, PendingSessionCommand>,
}

/// A generated-but-maybe-unused terminal launch command for a requirement.
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PendingSessionCommand {
    pub(crate) session_id: String,
    pub(crate) command: String,
    /// Harness the command targets ("pi" | "dsh-tui"); a stored command for a
    /// different harness than the current one is stale and regenerated.
    pub(crate) harness: String,
    pub(crate) context_path: String,
    pub(crate) created_at: i64,
}

pub(crate) fn associations_version() -> u8 {
    2
}

pub(crate) fn associations_path(state: &AppState) -> PathBuf {
    state.data_dir.join(ASSOCIATIONS_FILE)
}

pub(crate) async fn load_associations(state: &AppState) -> Result<AssociationsStore> {
    let path = associations_path(state);
    if !path.exists() {
        return Ok(AssociationsStore {
            version: 2,
            associations: HashMap::new(),
            pending_commands: HashMap::new(),
        });
    }
    let raw = fs::read_to_string(path).await.unwrap_or_default();
    Ok(serde_json::from_str(&raw).unwrap_or(AssociationsStore {
        version: 2,
        associations: HashMap::new(),
        pending_commands: HashMap::new(),
    }))
}

pub(crate) async fn save_associations(state: &AppState, store: &AssociationsStore) -> Result<()> {
    atomic_write_json(&associations_path(state), store).await
}

/// Read the requirement's pending launch command, if any (no side effects).
pub(crate) async fn load_pending_command(
    state: &AppState,
    req_id: &str,
) -> Result<Option<PendingSessionCommand>> {
    Ok(load_associations(state)
        .await?
        .pending_commands
        .get(req_id)
        .cloned())
}

/// Insert/replace (`Some`) or remove (`None`) the requirement's pending
/// launch command and persist the store.
pub(crate) async fn save_pending_command(
    state: &AppState,
    req_id: &str,
    pending: Option<PendingSessionCommand>,
) -> Result<()> {
    let mut store = load_associations(state).await?;
    match pending {
        Some(cmd) => {
            store.pending_commands.insert(req_id.to_string(), cmd);
        }
        None => {
            store.pending_commands.remove(req_id);
        }
    }
    save_associations(state, &store).await
}

pub(crate) async fn associate_session(
    state: &AppState,
    req_id: &str,
    session_id: &str,
) -> Result<()> {
    if req_id.trim().is_empty() || session_id.trim().is_empty() {
        return Ok(());
    }
    let mut store = load_associations(state).await?;
    for (k, sids) in store.associations.iter_mut() {
        if k != req_id {
            sids.retain(|s| s != session_id);
        }
    }
    store.associations.retain(|_, sids| !sids.is_empty());
    let entry = store.associations.entry(req_id.to_string()).or_default();
    if !entry.iter().any(|s| s == session_id) {
        entry.push(session_id.to_string());
    }
    save_associations(state, &store).await
}

pub(crate) async fn dissociate_session(
    state: &AppState,
    req_id: &str,
    session_id: &str,
) -> Result<()> {
    let mut store = load_associations(state).await?;
    if let Some(sids) = store.associations.get_mut(req_id) {
        sids.retain(|s| s != session_id);
        if sids.is_empty() {
            store.associations.remove(req_id);
        }
    }
    save_associations(state, &store).await
}

/// 解析 session 关联目标：需求组 → 组自身 + 所有成员；普通需求 → 自身。
/// 一个 session 只归属一个需求维度，组绑定时需要原子地同时绑定组与成员。
pub(crate) async fn association_target_ids(state: &AppState, req_id: &str) -> Result<Vec<String>> {
    let mut ids = vec![req_id.to_string()];
    if let Ok(req) = get_real_requirement(state, req_id).await {
        if let Some(dir) = req.req_dir.clone() {
            if let Some(group) = load_group_json(Path::new(&dir)).await {
                for member in group.members {
                    if member.req_id != req_id {
                        ids.push(member.req_id);
                    }
                }
            }
        }
    }
    Ok(unique_strings(ids))
}

/// 把 session 同时绑定到多个需求（组 + 成员），并从不在目标列表中的需求移除该
/// session（保持"一个 session 归属一个需求维度"的既有语义）。
pub(crate) async fn associate_sessions_multi(
    state: &AppState,
    req_ids: &[String],
    session_id: &str,
) -> Result<()> {
    if req_ids.is_empty() || session_id.trim().is_empty() {
        return Ok(());
    }
    let mut store = load_associations(state).await?;
    for (k, sids) in store.associations.iter_mut() {
        if !req_ids.contains(k) {
            sids.retain(|s| s != session_id);
        }
    }
    store.associations.retain(|_, sids| !sids.is_empty());
    for id in req_ids {
        let entry = store.associations.entry(id.clone()).or_default();
        if !entry.iter().any(|s| s == session_id) {
            entry.push(session_id.to_string());
        }
    }
    save_associations(state, &store).await
}

pub(crate) async fn resolve_req_scan_dirs(state: &AppState) -> Result<Vec<PathBuf>> {
    let cfg = read_config(state).await?;
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for root in cfg.requirement_scan_roots {
        let root_path = PathBuf::from(root);
        for sub in [".agents/req", "req"] {
            let candidate = root_path.join(sub);
            if candidate.is_dir() {
                let key = candidate.to_string_lossy().to_string();
                if seen.insert(key) {
                    out.push(candidate);
                }
            }
        }
    }
    Ok(out)
}

pub(crate) async fn scan_hermes_requirements(state: &AppState) -> Result<Vec<Requirement>> {
    let mut out = Vec::new();
    let dirs = resolve_req_scan_dirs(state).await?;
    let mut seen = HashSet::new();
    for dir in dirs {
        scan_req_dir(&dir, &mut out).await?;
    }
    out.retain(|r| seen.insert(r.id.clone()));
    Ok(out)
}

pub(crate) async fn scan_req_dir(req_dir: &Path, out: &mut Vec<Requirement>) -> Result<()> {
    let mut entries = match fs::read_dir(req_dir).await {
        Ok(v) => v,
        Err(_) => return Ok(()),
    };
    while let Ok(Some(entry)) = entries.next_entry().await {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') || name == "README.md" {
            continue;
        }
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let project = if name == "_default" {
            DEFAULT_PROJECT_NAME.to_string()
        } else {
            name.clone()
        };
        collect_requirements_recursive(&path, vec![project], vec![], out, 0).await?;
    }
    Ok(())
}

pub(crate) async fn collect_requirements_recursive(
    root: &Path,
    projects: Vec<String>,
    group_path: Vec<String>,
    out: &mut Vec<Requirement>,
    depth: usize,
) -> Result<()> {
    if depth > 6 || !root.is_dir() {
        return Ok(());
    }
    let has_meta = root.join("meta.md").is_file();
    let mut child_dirs = Vec::new();
    let mut rd = match fs::read_dir(root).await {
        Ok(v) => v,
        Err(_) => return Ok(()),
    };
    while let Ok(Some(entry)) = rd.next_entry().await {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') || name == "README.md" {
            continue;
        }
        let path = entry.path();
        if path.is_dir() {
            let has_child_meta = path.join("meta.md").is_file();
            child_dirs.push((name, path, has_child_meta));
        }
    }
    let has_nested_req = child_dirs.iter().any(|(_, _, has)| *has);
    let mut current_projects = projects.clone();
    if has_meta && has_nested_req {
        let dir_name = root
            .file_name()
            .and_then(|v| v.to_str())
            .unwrap_or(DEFAULT_PROJECT_NAME)
            .to_string();
        current_projects.extend(read_requirement_project_tags(root, &dir_name).await);
        current_projects = unique_strings(current_projects);
    } else if has_meta {
        let dir_name = root
            .file_name()
            .and_then(|v| v.to_str())
            .unwrap_or("requirement")
            .to_string();
        if let Some(req) =
            load_requirement_from_dir(root, &dir_name, &projects, &group_path).await?
        {
            out.push(req);
        }
    }
    for (name, path, child_has_meta) in child_dirs {
        let next_group = if child_has_meta {
            group_path.clone()
        } else {
            append_group(&group_path, name)
        };
        Box::pin(collect_requirements_recursive(
            &path,
            current_projects.clone(),
            next_group,
            out,
            depth + 1,
        ))
        .await?;
    }
    Ok(())
}

pub(crate) async fn read_requirement_project_tags(dir: &Path, fallback: &str) -> Vec<String> {
    let path = dir.join("meta.md");
    let raw = fs::read_to_string(path).await.unwrap_or_default();
    let fm = parse_frontmatter(&raw);
    let mut values = Vec::new();
    values.extend(split_list(fm.fields.get("projects")));
    values.extend(split_list(fm.fields.get("project")));
    if let Some(title) = fm.fields.get("title") {
        values.push(title.clone());
    }
    if values.is_empty() {
        values.push(fallback.to_string());
    }
    unique_strings(values)
}

pub(crate) async fn load_requirement_from_dir(
    dir: &Path,
    dir_name: &str,
    parent_projects: &[String],
    group_path: &[String],
) -> Result<Option<Requirement>> {
    let meta_path = dir.join("meta.md");
    if !meta_path.is_file() {
        return Ok(None);
    }
    let meta = fs::metadata(dir).await?;
    let raw = fs::read_to_string(&meta_path).await.unwrap_or_default();
    let fm = parse_frontmatter(&raw);
    let mut id = fm
        .fields
        .get("req-id")
        .cloned()
        .unwrap_or_else(|| dir_name.to_string());
    if id.trim().is_empty() {
        id = dir_name.to_string();
    }
    let mut title = fm
        .fields
        .get("title")
        .cloned()
        .unwrap_or_else(|| dir_name.to_string());
    if title == dir_name {
        if let Some(caps) = Regex::new(r"(?im)^\s*-\s*Title\s*:\s*(.+?)\s*$")
            .unwrap()
            .captures(&raw)
        {
            title = caps
                .get(1)
                .map(|m| m.as_str().trim().to_string())
                .unwrap_or(title);
        }
    }
    let mut status = normalize_status(fm.fields.get("status")).unwrap_or_else(|| "开发中".into());
    let mut category =
        normalize_category(fm.fields.get("category")).unwrap_or_else(|| "需求".into());
    let ones = fm
        .fields
        .get("ones")
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty());
    let mut issues = split_list(fm.fields.get("issues"));
    issues = unique_strings(issues);
    let source = fm
        .fields
        .get("source")
        .and_then(|v| normalize_source(Some(v)))
        .unwrap_or_else(|| "产品推动".into());
    let plan_release = fm
        .fields
        .get("plan-release")
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty());
    let parent_req_id = fm
        .fields
        .get("parent-req-id")
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty());
    let is_sub_req = parent_req_id.is_some();
    let mut explicit_projects = Vec::new();
    explicit_projects.extend(split_list(fm.fields.get("project")));
    explicit_projects.extend(split_list(fm.fields.get("projects")));
    let (project_file_projects, project_file_group) = read_project_json(dir).await;
    explicit_projects.extend(project_file_projects);
    let projects = if explicit_projects.is_empty() {
        unique_strings(parent_projects.to_vec())
    } else {
        unique_strings(explicit_projects)
    };
    let projects = if projects.is_empty() {
        vec![DEFAULT_PROJECT_NAME.to_string()]
    } else {
        projects
    };
    let effective_group_path = project_file_group.unwrap_or_else(|| group_path.to_vec());
    let mut created_at = fm
        .fields
        .get("start-date")
        .and_then(|v| parse_date_ms(v))
        .unwrap_or_else(|| system_time_to_ms(meta.modified().unwrap_or(UNIX_EPOCH)));
    let mut updated_at = system_time_to_ms(meta.modified().unwrap_or(UNIX_EPOCH));
    let description = first_paragraph(&fm.body);
    let req_state = read_requirement_state(dir).await?;
    if let Some(state) = &req_state {
        if let Some(s) = state
            .get("status")
            .and_then(Value::as_str)
            .and_then(|s| normalize_status(Some(&s.to_string())))
        {
            status = s;
        }
        if let Some(c) = state
            .get("category")
            .and_then(Value::as_str)
            .and_then(|s| normalize_category(Some(&s.to_string())))
        {
            category = c;
        }
        if let Some(ts) = state.get("updatedAt").and_then(Value::as_i64) {
            updated_at = updated_at.max(ts);
        }
    }
    if created_at <= 0 {
        created_at = updated_at;
    }
    let completed_at = req_state.as_ref().and_then(extract_completed_at);
    let effort = read_json_if_exists(&dir.join("effort-estimate.json")).await;
    let project = projects
        .first()
        .cloned()
        .unwrap_or_else(|| DEFAULT_PROJECT_NAME.into());
    Ok(Some(Requirement {
        id: id.clone(),
        title,
        status,
        projects,
        project,
        group_path: effective_group_path,
        description,
        session_ids: Vec::new(),
        category: Some(category),
        source,
        ones,
        issues,
        plan_release,
        created_at,
        updated_at,
        completed_at,
        req_dir: Some(dir.to_string_lossy().to_string()),
        meta_path: Some(meta_path.to_string_lossy().to_string()),
        background_path: path_if_exists(dir.join("background.md")),
        branch_path: path_if_exists(dir.join("branch.md")),
        test_path: path_if_exists(dir.join("test.md")),
        notes_path: path_if_exists(dir.join("notes.md")),
        config_path: path_if_exists(dir.join("config-changes.md")),
        impact_path: path_if_exists(dir.join("impact.md")),
        memory_path: path_if_exists(dir.join("memory.md")),
        review_path: path_if_exists(dir.join("review.md")),
        technical_plan_path: path_if_exists(dir.join("technical-plan.md")),
        release_manifest_path: path_if_exists(dir.join("release-manifest.md")),
        release_check_path: path_if_exists(dir.join("release-check.md")),
        experience_summary_path: path_if_exists(dir.join("experience-summary.md")),
        troubleshooting_path: path_if_exists(dir.join("troubleshooting.md")),
        incident_path: path_if_exists(dir.join("incident.md")),
        root_cause_path: path_if_exists(dir.join("root-cause.md")),
        test_scenario_path: path_if_exists(dir.join("test-scenario.md")),
        experience_summary_job: normalize_experience_summary_job_value(
            &id,
            dir,
            read_experience_summary_job(dir).await?,
        ),
        alignment_path: path_if_exists(dir.join("alignment.md")),
        prd_path: path_if_exists(dir.join("prd.md")),
        effort_estimate: effort,
        group_members: None,
        group_policy: None,
        member_of: Vec::new(),
        group_status: None,
        group_bottleneck: None,
        parent_req_id,
        is_sub_req,
        sub_reqs: Vec::new(),
    }))
}

pub(crate) async fn read_project_json(dir: &Path) -> (Vec<String>, Option<Vec<String>>) {
    let path = dir.join("project.json");
    let Some(v) = read_json_if_exists(&path).await else {
        return (Vec::new(), None);
    };
    let mut projects = Vec::new();
    projects.extend(value_to_list(v.get("project")));
    projects.extend(value_to_list(v.get("projects")));
    let group = value_to_path(
        v.get("groupPath")
            .or_else(|| v.get("subproject"))
            .or_else(|| v.get("path")),
    );
    (unique_strings(projects), group)
}

pub(crate) async fn list_requirements(state: &AppState) -> Result<Vec<Requirement>> {
    let mut reqs = scan_hermes_requirements(state).await?;
    // 先加载 group.json（组定义），再解析成员引用与聚合状态。
    for req in &mut reqs {
        if let Some(dir) = req.req_dir.clone() {
            if let Some(group) = load_group_json(Path::new(&dir)).await {
                req.group_policy = group
                    .release_policy
                    .filter(|p| GROUP_RELEASE_POLICIES.contains(&p.as_str()))
                    .or(Some("independent".to_string()));
                if !group.members.is_empty() {
                    req.group_members = Some(group.members);
                }
            }
        }
    }
    resolve_group_links(&mut reqs);
    resolve_sub_req_links(&mut reqs);
    let store = load_associations(state).await?;
    for req in &mut reqs {
        req.session_ids = store.associations.get(&req.id).cloned().unwrap_or_default();
    }
    reqs.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    Ok(reqs)
}

/// 解析需求组成员引用并计算聚合状态。
///
/// - 成员按 reqId 全局查找（跨 scan root / project），回填 title/status/found/nested；
/// - 组聚合状态 = min(成员需求流状态序数)，瓶颈 = 聚合状态来源成员；
/// - 成员需求的 `member_of` 回填所属组 req id；
/// - 自引用/找不到的成员标记 found=false，不参与聚合。
pub(crate) fn resolve_group_links(reqs: &mut [Requirement]) {
    #[derive(Clone)]
    struct MemberSnapshot {
        title: String,
        status: String,
        is_group: bool,
    }
    let snapshot: HashMap<String, MemberSnapshot> = reqs
        .iter()
        .map(|r| {
            (
                r.id.clone(),
                MemberSnapshot {
                    title: r.title.clone(),
                    status: r.status.clone(),
                    is_group: r.group_members.is_some(),
                },
            )
        })
        .collect();
    let mut member_of: HashMap<String, Vec<String>> = HashMap::new();
    for req in reqs.iter_mut() {
        let Some(members) = &mut req.group_members else {
            continue;
        };
        let self_id = req.id.clone();
        let mut best: Option<(usize, String)> = None;
        for member in members.iter_mut() {
            if member.req_id == self_id {
                member.found = false;
                member.nested = false;
                member.title = None;
                member.status = None;
                continue;
            }
            match snapshot.get(&member.req_id) {
                Some(snap) => {
                    member.found = true;
                    member.nested = snap.is_group;
                    member.title = Some(snap.title.clone());
                    member.status = Some(snap.status.clone());
                    member_of
                        .entry(member.req_id.clone())
                        .or_default()
                        .push(self_id.clone());
                    if !snap.is_group {
                        if let Some(rank) = requirement_flow_status_rank(&snap.status) {
                            if best
                                .as_ref()
                                .map(|(best_rank, _)| rank < *best_rank)
                                .unwrap_or(true)
                            {
                                best = Some((rank, member.req_id.clone()));
                            }
                        }
                    }
                }
                None => {
                    member.found = false;
                    member.nested = false;
                    member.title = None;
                    member.status = None;
                }
            }
        }
        if let Some((rank, bottleneck)) = best {
            req.group_bottleneck = Some(bottleneck);
            req.group_status = REQ_FLOW_STATUSES.get(rank).map(|s| s.to_string());
        }
    }
    for req in reqs.iter_mut() {
        if let Some(groups) = member_of.remove(&req.id) {
            req.member_of = unique_strings(groups);
        }
    }
}

/// 解析子需求反向索引：父需求的 `sub_reqs` 回填所有 `parent-req-id` 指向它的子需求。
///
/// - 子需求是完整需求记录（meta.md 写 parent-req-id，目录平铺），无需父侧清单文件；
/// - 按 reqId 全局查找子需求，回填 title/status/found/merged/cancelled；
/// - 找不到父需求的子需求保留自身 parent_req_id 字段不变（validate 不强校验父存在性）。
pub(crate) fn resolve_sub_req_links(reqs: &mut [Requirement]) {
    let snapshot: HashMap<String, (String, String)> = reqs
        .iter()
        .map(|r| (r.id.clone(), (r.title.clone(), r.status.clone())))
        .collect();
    let mut sub_reqs_by_parent: HashMap<String, Vec<SubReqRef>> = HashMap::new();
    for req in reqs.iter() {
        let Some(parent_id) = req.parent_req_id.clone() else {
            continue;
        };
        let mut sub = SubReqRef {
            req_id: req.id.clone(),
            title: None,
            status: None,
            found: false,
            merged: false,
            cancelled: false,
        };
        if let Some((title, status)) = snapshot.get(&req.id) {
            sub.found = true;
            sub.title = Some(title.clone());
            sub.status = Some(status.clone());
            sub.merged = status == "已合入";
            sub.cancelled = status == "已取消";
        }
        sub_reqs_by_parent.entry(parent_id).or_default().push(sub);
    }
    for req in reqs.iter_mut() {
        if let Some(subs) = sub_reqs_by_parent.remove(&req.id) {
            // 按子需求序号排序（-S1、-S2…），找不到序号的排最后。
            let mut subs = subs;
            subs.sort_by_key(|s| sub_req_seq(&s.req_id).unwrap_or(u64::MAX));
            req.sub_reqs = subs;
        }
    }
}

pub(crate) async fn get_requirement(state: &AppState, id: &str) -> Result<Option<Requirement>> {
    if id == DEFAULT_REQ_ID {
        let store = load_associations(state).await?;
        let sessions = store
            .associations
            .get(DEFAULT_REQ_ID)
            .cloned()
            .unwrap_or_default();
        return Ok(Some(default_requirement(sessions)));
    }
    Ok(list_requirements(state)
        .await?
        .into_iter()
        .find(|r| r.id == id))
}

pub(crate) async fn get_real_requirement(state: &AppState, id: &str) -> Result<Requirement> {
    get_requirement(state, id)
        .await?
        .filter(|r| r.id != DEFAULT_REQ_ID)
        .ok_or_else(|| anyhow!("requirement not found: {id}"))
}

pub(crate) fn default_requirement(session_ids: Vec<String>) -> Requirement {
    let now = now_ms();
    Requirement {
        id: DEFAULT_REQ_ID.into(),
        title: "默认需求".into(),
        status: "开发中".into(),
        projects: vec![DEFAULT_PROJECT_NAME.into()],
        project: DEFAULT_PROJECT_NAME.into(),
        group_path: Vec::new(),
        description: "未关联到具体需求的 session 归属到此默认需求。".into(),
        session_ids,
        category: Some("需求".into()),
        source: "产品推动".into(),
        ones: None,
        issues: Vec::new(),
        plan_release: None,
        created_at: now,
        updated_at: now,
        completed_at: None,
        req_dir: None,
        meta_path: None,
        background_path: None,
        branch_path: None,
        test_path: None,
        notes_path: None,
        config_path: None,
        impact_path: None,
        memory_path: None,
        review_path: None,
        technical_plan_path: None,
        release_manifest_path: None,
        release_check_path: None,
        experience_summary_path: None,
        troubleshooting_path: None,
        incident_path: None,
        root_cause_path: None,
        test_scenario_path: None,
        experience_summary_job: None,
        alignment_path: None,
        prd_path: None,
        effort_estimate: None,
        group_members: None,
        group_policy: None,
        member_of: Vec::new(),
        group_status: None,
        group_bottleneck: None,
        parent_req_id: None,
        is_sub_req: false,
        sub_reqs: Vec::new(),
    }
}

pub(crate) fn local_date_key_from_ms(ms: i64) -> String {
    let date = chrono::DateTime::from_timestamp_millis(ms)
        .map(|dt| dt.with_timezone(&chrono::Local).date_naive())
        .unwrap_or_else(|| chrono::Local::now().date_naive());
    date.format("%Y-%m-%d").to_string()
}

/// 未来 14 天（含今天）每天登记的发版需求数，以及最近一个已登记日期（含今天，不限 14 天窗口）。
fn build_release_schedule(
    plan_counts: &HashMap<String, usize>,
    today_key: &str,
) -> (Vec<ReleaseDayCount>, Option<ReleaseDayCount>) {
    let today = chrono::NaiveDate::parse_from_str(today_key, "%Y-%m-%d").ok();
    let schedule = (0..14)
        .map(|i| {
            let date = today
                .and_then(|d| d.checked_add_signed(chrono::Duration::days(i)))
                .map(|d| d.format("%Y-%m-%d").to_string())
                .unwrap_or_else(|| today_key.to_string());
            ReleaseDayCount {
                count: plan_counts.get(&date).copied().unwrap_or(0),
                date,
            }
        })
        .collect();
    let next_release = plan_counts
        .iter()
        .filter(|(k, _)| k.as_str() >= today_key)
        .min_by_key(|(k, _)| *k)
        .map(|(k, v)| ReleaseDayCount {
            date: k.clone(),
            count: *v,
        });
    (schedule, next_release)
}

pub(crate) fn build_dashboard_stats(requirements: Vec<Requirement>, now: i64) -> DashboardStats {
    let real: Vec<Requirement> = requirements
        .into_iter()
        .filter(|r| r.id != DEFAULT_REQ_ID)
        .collect();
    let total = real.len();
    let mut plan_counts: HashMap<String, usize> = HashMap::new();
    for r in &real {
        let plan = r.plan_release.as_deref().map(str::trim).unwrap_or("");
        if plan.is_empty() || plan == "unknown" {
            continue;
        }
        if chrono::NaiveDate::parse_from_str(plan, "%Y-%m-%d").is_ok() {
            *plan_counts.entry(plan.to_string()).or_insert(0) += 1;
        }
    }
    let today_key = local_date_key_from_ms(now);
    let (release_schedule, next_release) = build_release_schedule(&plan_counts, &today_key);
    let status_counts = REQ_STATUSES
        .iter()
        .map(|status| {
            let count = real.iter().filter(|r| r.status == *status).count();
            let percent = if total > 0 {
                ((count as f64 / total as f64) * 1000.0).round() / 10.0
            } else {
                0.0
            };
            StatusCount {
                status: status.to_string(),
                count,
                percent,
            }
        })
        .collect();
    let mut durations: Vec<RequirementDuration> = real
        .into_iter()
        .map(|req| {
            let end = if req.status == "已完成" {
                req.updated_at
            } else {
                now
            };
            RequirementDuration {
                duration_ms: (end - req.created_at).max(0),
                req,
            }
        })
        .collect();
    durations.sort_by(|a, b| b.duration_ms.cmp(&a.duration_ms));
    let mut completed: Vec<i64> = durations
        .iter()
        .filter(|d| d.req.status == "已完成")
        .map(|d| d.duration_ms)
        .collect();
    completed.sort_unstable();
    let completed_count = completed.len();
    let avg = if completed.is_empty() {
        0
    } else {
        completed.iter().sum::<i64>() / completed.len() as i64
    };
    let median = if completed.is_empty() {
        0
    } else if completed.len() % 2 == 0 {
        (completed[completed.len() / 2 - 1] + completed[completed.len() / 2]) / 2
    } else {
        completed[completed.len() / 2]
    };
    let max = completed.last().copied().unwrap_or(0);
    DashboardStats {
        total,
        status_counts,
        durations,
        avg_delivery_ms: avg,
        median_delivery_ms: median,
        max_delivery_ms: max,
        completed_count,
        in_progress_count: total.saturating_sub(completed_count),
        release_schedule,
        next_release,
    }
}
