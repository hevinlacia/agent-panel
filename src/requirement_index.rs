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
    pub(crate) ones: Option<String>,
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
    pub(crate) experience_summary_job: Option<Value>,
    pub(crate) alignment_path: Option<String>,
    pub(crate) prd_path: Option<String>,
    pub(crate) effort_estimate: Option<Value>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StatusCount {
    pub(crate) status: String,
    pub(crate) count: usize,
    pub(crate) percent: f64,
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
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AssociationsStore {
    #[serde(default = "associations_version")]
    pub(crate) version: u8,
    #[serde(default)]
    pub(crate) associations: HashMap<String, Vec<String>>,
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
        });
    }
    let raw = fs::read_to_string(path).await.unwrap_or_default();
    Ok(serde_json::from_str(&raw).unwrap_or(AssociationsStore {
        version: 2,
        associations: HashMap::new(),
    }))
}

pub(crate) async fn save_associations(state: &AppState, store: &AssociationsStore) -> Result<()> {
    atomic_write_json(&associations_path(state), store).await
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
        ones,
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
        experience_summary_job: normalize_experience_summary_job_value(
            &id,
            dir,
            read_experience_summary_job(dir).await?,
        ),
        alignment_path: path_if_exists(dir.join("alignment.md")),
        prd_path: path_if_exists(dir.join("prd.md")),
        effort_estimate: effort,
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
    let store = load_associations(state).await?;
    for req in &mut reqs {
        req.session_ids = store.associations.get(&req.id).cloned().unwrap_or_default();
    }
    reqs.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    Ok(reqs)
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
        ones: None,
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
        experience_summary_job: None,
        alignment_path: None,
        prd_path: None,
        effort_estimate: None,
    }
}

pub(crate) fn build_dashboard_stats(requirements: Vec<Requirement>, now: i64) -> DashboardStats {
    let real: Vec<Requirement> = requirements
        .into_iter()
        .filter(|r| r.id != DEFAULT_REQ_ID)
        .collect();
    let total = real.len();
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
    }
}
