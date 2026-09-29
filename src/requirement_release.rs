use axum::{extract::{Query, State}, Json};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::Path;

use crate::*;

/// 整合需求发布分支登记文件：父需求（整合需求）目录下的 `release-branches.json`。
/// 与 branches.json（需求分支登记）平行：发布分支是跨子需求的集成/发布载体，
/// 以生产分支为 base，子需求分支合入后随发布分支统一走生产 MR。
pub(crate) const RELEASE_BRANCH_STATUS_ACTIVE: &str = "active";
pub(crate) const RELEASE_BRANCH_STATUS_RELEASED: &str = "released";

/// 整合需求发布分支操作入参（create / merge-sub / sync-prod / prod-mr / mark-released 共用）。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ReleaseBranchOpForm {
    pub(crate) req_id: String,
    /// merge-sub / sync-prod / prod-mr / mark-released 专用：目标发布分支（id 如 `r1` 或完整分支名），
    /// 缺省取最新创建的一条。
    #[serde(default)]
    pub(crate) branch: Option<String>,
    /// create 专用：仅对这些仓库创建发布分支（repoName 过滤，缺省 = 全部子需求仓库并集）。
    #[serde(default)]
    pub(crate) repos: Option<Vec<String>>,
    /// create 专用：分支名前缀覆盖（如 `hevin.yang/release`）；缺省从子需求已登记分支自动推导。
    #[serde(default)]
    pub(crate) branch_prefix: Option<String>,
    /// create 专用：分支名附加标签（`...-<label>-r<n>`），提升可读性；缺省无标签。
    #[serde(default)]
    pub(crate) label: Option<String>,
    /// merge-sub 专用：要合入的子需求 reqId。
    #[serde(default)]
    pub(crate) sub_req_id: Option<String>,
    /// mark-released 专用：发布备注（发版单/上线窗口等）。
    #[serde(default)]
    pub(crate) note: Option<String>,
    /// 高风险操作（merge-sub 修改并推送发布分支 / mark-released 批量推进子需求状态）显式确认。
    #[serde(default)]
    pub(crate) confirm: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ReleaseBranchesFile {
    #[serde(default)]
    pub(crate) version: i64,
    #[serde(default)]
    pub(crate) updated_at: i64,
    #[serde(default)]
    pub(crate) branches: Vec<ReleaseBranchEntry>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ReleaseBranchEntry {
    #[serde(default)]
    pub(crate) id: String,
    /// 完整分支名（所有仓库同名）。
    #[serde(default)]
    pub(crate) name: String,
    #[serde(default)]
    pub(crate) created_at: i64,
    /// active | released
    #[serde(default = "default_release_branch_status")]
    pub(crate) status: String,
    #[serde(default)]
    pub(crate) repos: Vec<ReleaseBranchRepo>,
    #[serde(default)]
    pub(crate) merged_subs: Vec<ReleaseMergedSub>,
    #[serde(default)]
    pub(crate) released_at: Option<i64>,
    #[serde(default)]
    pub(crate) note: Option<String>,
}

fn default_release_branch_status() -> String {
    RELEASE_BRANCH_STATUS_ACTIVE.to_string()
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ReleaseBranchRepo {
    #[serde(default)]
    pub(crate) repo_name: String,
    #[serde(default)]
    pub(crate) role: Option<String>,
    #[serde(default)]
    pub(crate) path: Option<String>,
    /// 生产分支（master / production），发布分支的 base。
    #[serde(default)]
    pub(crate) base_branch: String,
    /// 创建时 origin/<base_branch> 指向。
    #[serde(default)]
    pub(crate) base_commit: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ReleaseMergedSub {
    #[serde(default)]
    pub(crate) req_id: String,
    #[serde(default)]
    pub(crate) title: Option<String>,
    #[serde(default)]
    pub(crate) merged_at: i64,
    /// 本次合并涉及的仓库；空 = 纯配置子需求（无代码合并，仅登记随本发布分支上线）。
    #[serde(default)]
    pub(crate) repos: Vec<String>,
    #[serde(default)]
    pub(crate) note: Option<String>,
}

pub(crate) async fn read_release_branches(req_dir: &Path) -> ReleaseBranchesFile {
    read_json_if_exists(&req_dir.join(RELEASE_BRANCHES_FILE))
        .await
        .and_then(|raw| serde_json::from_value(raw).ok())
        .unwrap_or_default()
}

pub(crate) async fn write_release_branches(
    req_dir: &Path,
    file: &ReleaseBranchesFile,
) -> Result<()> {
    let mut doc = serde_json::to_value(file)?;
    if let Some(obj) = doc.as_object_mut() {
        obj.insert("updatedAt".to_string(), json!(now_ms()));
        if obj.get("version").and_then(Value::as_i64).unwrap_or(0) < 1 {
            obj.insert("version".to_string(), json!(1));
        }
    }
    atomic_write_json(&req_dir.join(RELEASE_BRANCHES_FILE), &doc).await
}

/// 加载整合需求（父需求）：不能是需求组（组无分支维度），也不能是子需求（只允许一层）。
async fn load_consolidated_parent(state: &AppState, req_id: &str) -> ApiResult<Requirement> {
    let req = get_real_requirement(state, req_id)
        .await
        .map_err(|_| ApiError::bad_request(format!("需求不存在：{req_id}")))?;
    if req.is_sub_req || req.parent_req_id.is_some() {
        return Err(ApiError::bad_request(format!(
            "{} 是子需求；整合需求发布分支挂在父需求（整合需求）上",
            req.id
        )));
    }
    if req.group_members.is_some() {
        return Err(ApiError::bad_request(format!(
            "{} 是引用式需求组；发布分支功能面向「整合需求 + 子需求」模型（成员需求独立走各自流程）",
            req.id
        )));
    }
    Ok(req)
}

/// 收集整合需求下全部子需求，并合并它们的分支登记（branches.json round 1）得到发布分支仓库范围：
/// 按 repoName 去重（保留首个 role/path 登记）。
async fn collect_release_repo_scope(
    state: &AppState,
    parent: &Requirement,
    repos_filter: Option<&Vec<String>>,
) -> ApiResult<Vec<BranchRepo>> {
    let all = scan_hermes_requirements(state).await?;
    let mut sub_ids: Vec<String> = all
        .iter()
        .filter(|r| r.parent_req_id.as_deref() == Some(parent.id.as_str()))
        .map(|r| r.id.clone())
        .collect();
    sub_ids.sort();
    if sub_ids.is_empty() {
        return Err(ApiError::bad_request(format!(
            "{} 下没有子需求；整合需求发布分支的仓库范围来自子需求分支登记（先 create-sub 并登记子需求分支）",
            parent.id
        )));
    }
    let mut merged: Vec<BranchRepo> = Vec::new();
    let mut per_sub_repos: Vec<(String, Vec<String>)> = Vec::new();
    for sub_id in &sub_ids {
        let Ok(sub) = get_real_requirement(state, sub_id).await else {
            continue;
        };
        let Ok(dir) = req_dir_path(&sub) else {
            continue;
        };
        if let Some(scope) = read_branch_scope(&dir).await? {
            let names: Vec<String> = scope.repos.iter().map(|r| r.repo_name.clone()).collect();
            per_sub_repos.push((sub_id.clone(), names));
            for repo in scope.repos {
                if merged
                    .iter()
                    .any(|r| r.repo_name.eq_ignore_ascii_case(&repo.repo_name))
                {
                    continue;
                }
                merged.push(repo);
            }
        }
    }
    if let Some(filter) = repos_filter {
        let wanted: Vec<String> = filter.iter().map(|s| s.trim().to_string()).collect();
        merged.retain(|r| wanted.iter().any(|w| w.eq_ignore_ascii_case(&r.repo_name)));
        if merged.is_empty() {
            return Err(ApiError::bad_request(
                "repos 过滤后没有匹配的仓库；请检查 repos 参数或子需求分支登记",
            ));
        }
    }
    if merged.is_empty() {
        return Err(ApiError::bad_request(format!(
            "子需求（{}）都没有登记分支（branches.json）；纯配置子需求无法确定发布分支仓库范围",
            sub_ids.join(", ")
        )));
    }
    Ok(merged)
}

/// 从子需求已登记分支推导发布分支名前缀：
/// `hevin.yang/feature/WMS-143-x` → `hevin.yang/feature` →（feature→release）`hevin.yang/release`；
/// 无 `/` 或无法识别时回退 `release`。`branch_prefix` 显式传参时优先。
pub(crate) fn derive_release_branch_prefix(registered_branches: &[String], explicit: Option<&str>) -> String {
    if let Some(prefix) = explicit.map(str::trim).filter(|v| !v.is_empty()) {
        // 分支名不允许空白字符：去除内部空格后再去首尾斜杠。
        let cleaned: String = prefix.chars().filter(|c| !c.is_whitespace()).collect();
        return cleaned.trim_matches('/').to_string();
    }
    for branch in registered_branches {
        let trimmed = branch.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Some(prefix) = trimmed.rsplit_once('/') {
            let prefix = prefix.0.trim();
            if prefix.is_empty() {
                continue;
            }
            return if let Some(stripped) = prefix.strip_suffix("/feature") {
                format!("{stripped}/release")
            } else if prefix == "feature" {
                "release".to_string()
            } else {
                prefix.to_string()
            };
        }
    }
    "release".to_string()
}

pub(crate) fn release_branch_seq(id: &str) -> u64 {
    id.trim()
        .rsplit_once('r')
        .and_then(|(_, n)| n.parse().ok())
        .unwrap_or(0)
}

/// 分配发布分支序号：registry 最大 r<n> + 1；若远端已存在同名分支（残留/撞名）继续 +1。
async fn allocate_release_branch(
    scope: &[(BranchRepo, PathBuf)],
    base_name_template: impl Fn(u64) -> String,
    next_seq: u64,
) -> (u64, String) {
    let mut n = next_seq;
    loop {
        let name = base_name_template(n);
        let mut taken = false;
        for (repo, project_path) in scope {
            if remote_branch_exists(project_path, &name).await {
                tracing::warn!(
                    repo = %repo.repo_name,
                    branch = %name,
                    "release branch name already exists on remote, bumping seq"
                );
                taken = true;
                break;
            }
        }
        if !taken {
            return (n, name);
        }
        n += 1;
    }
}

pub(crate) fn release_branch_id_by_ref<'a>(
    file: &'a ReleaseBranchesFile,
    branch_ref: Option<&str>,
) -> ApiResult<&'a ReleaseBranchEntry> {
    if file.branches.is_empty() {
        return Err(ApiError::bad_request(
            "整合需求还没有发布分支；先调 POST /api/requirement/release-branch/create 创建",
        ));
    }
    let Some(key) = branch_ref.map(str::trim).filter(|v| !v.is_empty()) else {
        // 缺省取最新创建的一条（active 优先：released 分支不应再被合入/MR）。
        return Ok(file
            .branches
            .iter()
            .find(|b| b.status == RELEASE_BRANCH_STATUS_ACTIVE)
            .or_else(|| file.branches.last())
            .unwrap());
    };
    file.branches
        .iter()
        .find(|b| b.id == key || b.name == key)
        .ok_or_else(|| {
            ApiError::bad_request(format!(
                "发布分支 {key} 不存在；可用：{}",
                file.branches
                    .iter()
                    .map(|b| b.id.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ))
        })
}

async fn append_note_quiet(state: &AppState, req_id: &str, title: &str, text: &str) {
    let _ = append_requirement_note(
        state,
        RequirementNoteForm {
            req_id: req_id.to_string(),
            text: text.to_string(),
            title: Some(title.to_string()),
            session_id: None,
            dry_run: Some(false),
        },
    )
    .await;
}

fn repo_project_path(repo: &BranchRepo) -> ApiResult<PathBuf> {
    resolve_code_review_project_path(repo.path.as_deref(), &repo.repo_name).ok_or_else(|| {
        ApiError::bad_request(format!("仓库 {} 缺少 path，无法定位项目目录", repo.repo_name))
    })
}

/// POST /api/requirement/release-branch/create
///
/// 为整合需求创建一条发布分支（每仓同名）：以各仓生产分支（master/production）当前指向为 base
/// 创建并 push。序号自动递增（r1、r2…），远端已存在同名分支时自动跳号保证命名不重复。
pub(crate) async fn api_release_branch_create(
    State(state): State<AppState>,
    form: FormOrJson<ReleaseBranchOpForm>,
) -> ApiResult<Json<Value>> {
    let body = form.0;
    let parent = load_consolidated_parent(&state, &body.req_id).await?;
    let dir = req_dir_path(&parent)?;
    ensure_requirement_dir_writable(&state, &dir).await?;
    let scope = collect_release_repo_scope(&state, &parent, body.repos.as_ref()).await?;
    let mut pairs: Vec<(BranchRepo, PathBuf)> = Vec::new();
    for repo in &scope {
        pairs.push((repo.clone(), repo_project_path(repo)?));
    }
    // PDA 客户端仓（APK 打包发布）不参与生产分支合并，跳过发布分支创建。
    let mut skipped: Vec<Value> = Vec::new();
    pairs.retain(|(repo, _)| {
        if is_pda_client_repo(repo) {
            skipped.push(json!({
                "repoName": repo.repo_name,
                "status": "skipped",
                "message": "PDA 客户端仓走 APK 打包发布，不创建发布分支",
            }));
            false
        } else {
            true
        }
    });
    if pairs.is_empty() {
        return Err(ApiError::bad_request(
            "过滤后没有可创建发布分支的仓库（PDA 客户端仓不参与）",
        ));
    }
    let prefix = {
        let registered: Vec<String> = scope
            .iter()
            .flat_map(|r| r.branches.iter().cloned())
            .collect();
        derive_release_branch_prefix(&registered, body.branch_prefix.as_deref())
    };
    let label_seg = match body.label.as_deref().map(str::trim).filter(|v| !v.is_empty()) {
        Some(label) => {
            let safe = ensure_safe_segment(label, "label")?;
            format!("-{safe}")
        }
        None => String::new(),
    };
    let registry = read_release_branches(&dir).await;
    let next_seq = registry
        .branches
        .iter()
        .map(|b| release_branch_seq(&b.id))
        .max()
        .unwrap_or(0)
        + 1;
    let template = |n: u64| format!("{prefix}/{}{label_seg}-r{n}", parent.id);
    let (seq, branch_name) =
        allocate_release_branch(&pairs, template, next_seq).await;
    let mut repo_results = Vec::new();
    let mut entry_repos = Vec::new();
    for (repo, _path) in &pairs {
        let base_branch = detect_prod_target_branch(repo);
        let result = create_release_branch_repo(repo, &base_branch, &branch_name).await;
        let ok = result.get("status").and_then(Value::as_str) == Some("ok");
        if ok {
            entry_repos.push(ReleaseBranchRepo {
                repo_name: repo.repo_name.clone(),
                role: repo.role.clone(),
                path: repo.path.clone(),
                base_branch,
                base_commit: result
                    .get("baseCommit")
                    .and_then(Value::as_str)
                    .map(str::to_string),
            });
        }
        repo_results.push(result);
    }
    let all_ok = !entry_repos.is_empty()
        && entry_repos.len() == pairs.len();
    if all_ok {
        let repo_names: Vec<String> = entry_repos.iter().map(|r| r.repo_name.clone()).collect();
        let entry = ReleaseBranchEntry {
            id: format!("r{seq}"),
            name: branch_name.clone(),
            created_at: now_ms(),
            status: RELEASE_BRANCH_STATUS_ACTIVE.to_string(),
            repos: entry_repos,
            merged_subs: Vec::new(),
            released_at: None,
            note: None,
        };
        let mut next_registry = registry;
        next_registry.branches.push(entry);
        write_release_branches(&dir, &next_registry).await?;
        append_note_quiet(
            &state,
            &parent.id,
            "创建整合发布分支",
            &format!(
                "创建发布分支 `{branch_name}`（r{seq}，base=各仓生产分支）：覆盖仓库 {}；后续把子需求分支合入该分支做集成，发布走发布分支 → 生产分支 MR。",
                repo_names.join(", ")
            ),
        )
        .await;
    }
    Ok(Json(json!({
        "ok": all_ok,
        "reqId": parent.id,
        "branch": if all_ok { json!(branch_name) } else { Value::Null },
        "branchId": if all_ok { json!(format!("r{seq}")) } else { Value::Null },
        "repos": repo_results,
        "skipped": skipped,
        "message": if all_ok {
            "发布分支已创建并推送".to_string()
        } else {
            "部分仓库创建失败：仅全部成功才写入登记，请修复后重试".to_string()
        },
    })))
}

/// GET /api/requirement/release-branch/list?id=<整合需求 reqId>
///
/// 返回发布分支登记 + 每条分支的子需求合并清单（带当前状态）和每仓相对生产分支的差异统计。
pub(crate) async fn api_release_branch_list(
    State(state): State<AppState>,
    Query(query): Query<IdQuery>,
) -> ApiResult<Json<Value>> {
    let id = query.id.or(query.req_id).unwrap_or_default();
    let parent = load_consolidated_parent(&state, &id).await?;
    let dir = req_dir_path(&parent)?;
    let registry = read_release_branches(&dir).await;
    let mut branches = Vec::new();
    for entry in &registry.branches {
        let mut merged_subs = Vec::new();
        for sub in &entry.merged_subs {
            let sub_req = get_real_requirement(&state, &sub.req_id).await.ok();
            merged_subs.push(json!({
                "reqId": sub.req_id,
                "title": sub_req.as_ref().map(|r| r.title.clone()).or_else(|| sub.title.clone()),
                "status": sub_req.as_ref().map(|r| r.status.clone()),
                "mergedAt": sub.merged_at,
                "repos": sub.repos,
                "note": sub.note,
                "found": sub_req.is_some(),
            }));
        }
        let mut repos = Vec::new();
        for repo in &entry.repos {
            let mut item = json!({
                "repoName": repo.repo_name,
                "role": repo.role,
                "baseBranch": repo.base_branch,
                "baseCommit": repo.base_commit,
            });
            // branches.json 风格路径可能含 ~ 前缀，统一走 resolve 展开；展开失败则跳过 diff 统计。
            if let Some(project_path) =
                resolve_code_review_project_path(repo.path.as_deref(), &repo.repo_name)
            {
                if let Some((files, additions, deletions)) =
                    release_branch_diff_stat(&project_path, &repo.base_branch, &entry.name).await
                {
                    item["diffFiles"] = json!(files);
                    item["diffAdditions"] = json!(additions);
                    item["diffDeletions"] = json!(deletions);
                }
            }
            repos.push(item);
        }
        branches.push(json!({
            "id": entry.id,
            "name": entry.name,
            "createdAt": entry.created_at,
            "status": entry.status,
            "releasedAt": entry.released_at,
            "note": entry.note,
            "repos": repos,
            "mergedSubs": merged_subs,
        }));
    }
    Ok(Json(json!({
        "ok": true,
        "reqId": parent.id,
        "generatedAt": now_ms(),
        "branches": branches,
    })))
}

/// POST /api/requirement/release-branch/merge-sub
///
/// 把子需求分支合入整合需求发布分支（逐仓，隔离 merge worktree + 推送）。
/// 纯配置子需求（无 branches.json）允许登记：不合并代码，仅记入发布分支 mergedSubs 清单。
/// 全部成功后子需求自动推进（需求创建 → 开发中 → 已合入；更高状态保留）。
pub(crate) async fn api_release_branch_merge_sub(
    State(state): State<AppState>,
    form: FormOrJson<ReleaseBranchOpForm>,
) -> ApiResult<Json<Value>> {
    let body = form.0;
    if !body.confirm.unwrap_or(false) {
        return Err(ApiError::bad_request(
            "合入发布分支会修改并推送整合发布分支（多子需求共享的集成分支），属于高风险操作：请求必须带 confirm=true",
        ));
    }
    let parent = load_consolidated_parent(&state, &body.req_id).await?;
    let dir = req_dir_path(&parent)?;
    let sub_req_id = body
        .sub_req_id
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .ok_or_else(|| ApiError::bad_request("缺少 subReqId（要合入的子需求）"))?;
    let sub = get_real_requirement(&state, sub_req_id)
        .await
        .map_err(|_| ApiError::bad_request(format!("子需求不存在：{sub_req_id}")))?;
    if sub.parent_req_id.as_deref() != Some(parent.id.as_str()) {
        return Err(ApiError::bad_request(format!(
            "{} 不是 {} 的子需求（parent-req-id 不匹配）",
            sub.id, parent.id
        )));
    }
    if matches!(sub.status.as_str(), "已发布" | "已取消") {
        return Err(ApiError::bad_request(format!(
            "子需求 {} 已是终态（{}），不可再合入",
            sub.id, sub.status
        )));
    }
    let mut registry = read_release_branches(&dir).await;
    let entry_idx = {
        let entry = release_branch_id_by_ref(&registry, body.branch.as_deref())?;
        if entry.status != RELEASE_BRANCH_STATUS_ACTIVE {
            return Err(ApiError::bad_request(format!(
                "发布分支 {} 已发布（released）；请先创建新发布分支（POST /api/requirement/release-branch/create）",
                entry.name
            )));
        }
        registry
            .branches
            .iter()
            .position(|b| b.id == entry.id)
            .ok_or_else(|| ApiError::bad_request("发布分支登记缺失"))?
    };
    let mut entry = registry.branches[entry_idx].clone();
    let sub_dir = req_dir_path(&sub)?;
    let sub_scope = read_branch_scope(&sub_dir).await?;
    let mut repo_results = Vec::new();
    let mut merged_repo_names: Vec<String> = Vec::new();
    match &sub_scope {
        None => {
            // 纯配置子需求：无代码合并，仅登记到发布分支的上线清单。
            repo_results.push(json!({
                "repoName": null,
                "status": "skipped",
                "message": "纯配置子需求（未登记分支）：不合并代码，仅登记随该发布分支上线",
            }));
        }
        Some(scope) => {
            for repo in &scope.repos {
                let Some(_release_repo) = entry
                    .repos
                    .iter()
                    .find(|r| r.repo_name.eq_ignore_ascii_case(&repo.repo_name))
                else {
                    repo_results.push(json!({
                        "repoName": repo.repo_name,
                        "status": "skipped",
                        "message": "发布分支未包含该仓库（创建发布分支时子需求尚未登记？）：请为新仓库重建发布分支或另建分支",
                    }));
                    continue;
                };
                if let Some(filter) = body.repos.as_ref() {
                    if !filter
                        .iter()
                        .any(|n| n.trim().eq_ignore_ascii_case(&repo.repo_name))
                    {
                        continue;
                    }
                }
                let sub_branch = repo.branches.first().cloned().unwrap_or_default();
                let result = merge_branch_pair_ex(
                    repo,
                    &sub_branch,
                    &entry.name,
                    "release-merge",
                    false,
                    true,
                )
                .await;
                let status = result
                    .get("status")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                if matches!(status.as_str(), "merged" | "upToDate") {
                    merged_repo_names.push(repo.repo_name.clone());
                }
                repo_results.push(result);
            }
        }
    }
    // 全部成功才算 ok：conflict/failed、或发布分支未覆盖子需求仓库，都视为失败（不登记、不改状态）。
    let all_ok = !repo_results.is_empty()
        && repo_results.iter().all(|r| {
            matches!(
                r.get("status").and_then(Value::as_str),
                Some("merged") | Some("upToDate")
            ) || (r.get("status").and_then(Value::as_str) == Some("skipped")
                && r.get("message").and_then(Value::as_str)
                    .map(|m| m.contains("纯配置子需求"))
                    .unwrap_or(false))
        });
    let mut status_state = Value::Null;
    let entry_name = entry.name.clone();
    let entry_id = entry.id.clone();
    if all_ok {
        // 更新 mergedSubs：同子需求重复合入（拉取后续 commit）时刷新 mergedAt/repos。
        let merged_at = now_ms();
        let repos = merged_repo_names.clone();
        if let Some(existing) = entry
            .merged_subs
            .iter_mut()
            .find(|m| m.req_id == sub.id)
        {
            existing.merged_at = merged_at;
            if !repos.is_empty() {
                existing.repos = repos.clone();
            }
            existing.title = Some(sub.title.clone());
        } else {
            entry.merged_subs.push(ReleaseMergedSub {
                req_id: sub.id.clone(),
                title: Some(sub.title.clone()),
                merged_at,
                repos: repos.clone(),
                note: if sub_scope.is_none() {
                    Some("纯配置子需求，无代码合并".to_string())
                } else {
                    None
                },
            });
        }
        registry.branches[entry_idx] = entry;
        write_release_branches(&dir, &registry).await?;
        // 状态自动补齐：需求创建 → 开发中 → 已合入（系统内部流转，不走门禁）；
        // 已在 自测中/测试中/发布就绪 的子需求保留当前进度，仅登记合并事实。
        let sub_dir_str = sub_dir.to_string_lossy().to_string();
        if sub.status == "需求创建" {
            write_requirement_status_checked(
                &sub_dir_str,
                "开发中",
                Some("合入发布分支前自动推进"),
                GateCheckMode::None,
            )
            .await?;
        }
        if matches!(sub.status.as_str(), "需求创建" | "开发中") {
            status_state = write_requirement_status_checked(
                &sub_dir_str,
                "已合入",
                Some(&format!(
                    "子分支已合入整合需求 {} 发布分支 {}",
                    parent.id, entry_name
                )),
                GateCheckMode::None,
            )
            .await?;
            if let Some(st) = status_state.as_object() {
                if st.get("changed").and_then(Value::as_bool) == Some(true) {
                    record_status_transition_event(&state, &sub, &status_state, None)
                        .await
                        .ok();
                }
            }
        }
        append_note_quiet(
            &state,
            &sub.id,
            "合入发布分支",
            &format!(
                "子分支已合入整合需求 {} 发布分支 `{}`（{}）；发布走该分支 → 生产分支 MR。",
                parent.id,
                entry_name,
                if merged_repo_names.is_empty() {
                    "纯配置子需求，无代码合并".to_string()
                } else {
                    format!("仓库：{}", merged_repo_names.join(", "))
                }
            ),
        )
        .await;
        append_note_quiet(
            &state,
            &parent.id,
            "子需求合入发布分支",
            &format!(
                "子需求 `{}`（{}）已合入发布分支 `{}`；发布分支当前累计子需求：{}。",
                sub.id,
                sub.title,
                entry_name,
                registry.branches[entry_idx]
                    .merged_subs
                    .iter()
                    .map(|m| m.req_id.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        )
        .await;
    }
    Ok(Json(json!({
        "ok": all_ok,
        "reqId": parent.id,
        "subReqId": sub.id,
        "branch": entry_name,
        "branchId": entry_id,
        "repos": repo_results,
        "statusState": status_state,
    })))
}

/// POST /api/requirement/release-branch/sync-prod
///
/// 把各仓生产分支最新成果合入发布分支（发布分支存在期间 master 前进时拉齐，
/// 冲突在发布分支侧解决——与「把生产分支合入需求分支解冲突」的 WMS 规范一致）。
pub(crate) async fn api_release_branch_sync_prod(
    State(state): State<AppState>,
    form: FormOrJson<ReleaseBranchOpForm>,
) -> ApiResult<Json<Value>> {
    let body = form.0;
    if !body.confirm.unwrap_or(false) {
        return Err(ApiError::bad_request(
            "同步生产分支会修改并推送整合发布分支，属于高风险操作：请求必须带 confirm=true",
        ));
    }
    let parent = load_consolidated_parent(&state, &body.req_id).await?;
    let dir = req_dir_path(&parent)?;
    let registry = read_release_branches(&dir).await;
    let entry = release_branch_id_by_ref(&registry, body.branch.as_deref())?.clone();
    if entry.status != RELEASE_BRANCH_STATUS_ACTIVE {
        return Err(ApiError::bad_request(format!(
            "发布分支 {} 已发布（released），无需再同步生产分支；如需继续集成请创建新发布分支",
            entry.name
        )));
    }
    let mut repo_results = Vec::new();
    for repo in &entry.repos {
        let branch_repo = BranchRepo {
            repo_name: repo.repo_name.clone(),
            branches: vec![entry.name.clone()],
            role: repo.role.clone(),
            path: repo.path.clone(),
            base_ref: None,
            test_target_branch: None,
            uat_target_branch: None,
        };
        let result = merge_branch_pair_ex(
            &branch_repo,
            &repo.base_branch,
            &entry.name,
            "release-sync",
            true,
            true,
        )
        .await;
        repo_results.push(result);
    }
    let all_ok = !repo_results.is_empty()
        && repo_results.iter().all(|r| {
            matches!(
                r.get("status").and_then(Value::as_str),
                Some("merged") | Some("upToDate")
            )
        });
    if all_ok {
        append_note_quiet(
            &state,
            &parent.id,
            "同步生产分支",
            &format!(
                "已把各仓生产分支最新成果合入发布分支 `{}`（逐仓）",
                entry.name
            ),
        )
        .await;
    }
    Ok(Json(json!({
        "ok": all_ok,
        "reqId": parent.id,
        "branch": entry.name,
        "branchId": entry.id,
        "repos": repo_results,
    })))
}

/// POST /api/requirement/release-branch/prod-mr
///
/// 按发布分支生成生产 MR（发布分支 → 各仓生产分支）：复用 /api/requirement/prod-mrs 的
/// GitLab MR 创建/复用逻辑；MR 合入由用户在 GitLab 完成后，调 mark-released 封版。
pub(crate) async fn api_release_branch_prod_mr(
    State(state): State<AppState>,
    form: FormOrJson<ReleaseBranchOpForm>,
) -> ApiResult<Json<Value>> {
    let body = form.0;
    let parent = load_consolidated_parent(&state, &body.req_id).await?;
    if parent
        .category
        .as_deref()
        .map(is_issue_category)
        .unwrap_or(false)
    {
        return Err(ApiError::bad_request(
            "线上问题/测试问题的分支代码仅用于测试环境复现与验证，禁止合入生产分支，不生成生产 MR",
        ));
    }
    let dir = req_dir_path(&parent)?;
    let registry = read_release_branches(&dir).await;
    let entry = release_branch_id_by_ref(&registry, body.branch.as_deref())?.clone();
    if entry.repos.is_empty() {
        return Err(ApiError::bad_request("发布分支没有仓库登记，无法生成 MR"));
    }
    let scope = BranchScope {
        version: 1,
        updated_at: now_ms(),
        repos: entry
            .repos
            .iter()
            .map(|r| BranchRepo {
                repo_name: r.repo_name.clone(),
                branches: vec![entry.name.clone()],
                role: r.role.clone(),
                path: r.path.clone(),
                base_ref: None,
                test_target_branch: None,
                uat_target_branch: None,
            })
            .collect(),
        fallback: false,
        round: 1,
    };
    let results = generate_prod_mrs(&parent, &scope).await?;
    let mr_urls: Vec<String> = results
        .iter()
        .filter_map(|r| {
            r.get("webUrl")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .collect();
    if !mr_urls.is_empty() {
        append_note_quiet(
            &state,
            &parent.id,
            "生成发布分支生产 MR",
            &format!(
                "发布分支 `{}` → 生产分支 MR：\n{}",
                entry.name,
                mr_urls
                    .iter()
                    .map(|u| format!("- {u}"))
                    .collect::<Vec<_>>()
                    .join("\n")
            ),
        )
        .await;
    }
    Ok(Json(json!({
        "ok": true,
        "reqId": parent.id,
        "branch": entry.name,
        "branchId": entry.id,
        "generatedAt": now_ms(),
        "results": results,
    })))
}

/// POST /api/requirement/release-branch/mark-released
///
/// 封版发布分支（生产 MR 已合入后人工确认）：登记 released 状态，并把 mergedSubs 里
/// 尚未终态的子需求统一推进「已发布」（系统内部流转，不走门禁）。
pub(crate) async fn api_release_branch_mark_released(
    State(state): State<AppState>,
    form: FormOrJson<ReleaseBranchOpForm>,
) -> ApiResult<Json<Value>> {
    let body = form.0;
    if !body.confirm.unwrap_or(false) {
        return Err(ApiError::bad_request(
            "封版会把发布分支标记为 released 并把其覆盖的子需求推进「已发布」，属于批量状态变更：请求必须带 confirm=true；请确认生产 MR 已全部合入",
        ));
    }
    let parent = load_consolidated_parent(&state, &body.req_id).await?;
    let dir = req_dir_path(&parent)?;
    ensure_requirement_dir_writable(&state, &dir).await?;
    let mut registry = read_release_branches(&dir).await;
    let entry_idx = {
        let entry = release_branch_id_by_ref(&registry, body.branch.as_deref())?;
        registry
            .branches
            .iter()
            .position(|b| b.id == entry.id)
            .ok_or_else(|| ApiError::bad_request("发布分支登记缺失"))?
    };
    if registry.branches[entry_idx].status == RELEASE_BRANCH_STATUS_RELEASED {
        return Err(ApiError::bad_request(format!(
            "发布分支 {} 已是 released 状态，无需重复封版",
            registry.branches[entry_idx].name
        )));
    }
    registry.branches[entry_idx].status = RELEASE_BRANCH_STATUS_RELEASED.to_string();
    registry.branches[entry_idx].released_at = Some(now_ms());
    registry.branches[entry_idx].note = body
        .note
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_string);
    let entry = registry.branches[entry_idx].clone();
    write_release_branches(&dir, &registry).await?;
    let mut advanced = Vec::new();
    for merged in &entry.merged_subs {
        let Ok(sub) = get_real_requirement(&state, &merged.req_id).await else {
            continue;
        };
        if matches!(sub.status.as_str(), "已发布" | "已取消") {
            continue;
        }
        let Ok(sub_dir) = req_dir_path(&sub) else {
            continue;
        };
        let note = format!(
            "随整合需求 {} 发布分支 `{}` 发布（生产 MR 已合入）",
            parent.id, entry.name
        );
        let Ok(st) = write_requirement_status_checked(
            sub_dir.to_string_lossy().as_ref(),
            "已发布",
            Some(&note),
            GateCheckMode::None,
        )
        .await
        else {
            continue;
        };
        if st.get("changed").and_then(Value::as_bool) == Some(true) {
            record_status_transition_event(&state, &sub, &st, Some(&note))
                .await
                .ok();
            advanced.push(sub.id.clone());
        }
    }
    append_note_quiet(
        &state,
        &parent.id,
        "发布分支封版",
        &format!(
            "发布分支 `{}` 已标记 released（{}）；随发子需求推进已发布：{}。",
            entry.name,
            registry.branches[entry_idx]
                .note
                .clone()
                .unwrap_or_else(|| "生产 MR 已合入".to_string()),
            if advanced.is_empty() {
                "无".to_string()
            } else {
                advanced.join(", ")
            }
        ),
    )
    .await;
    Ok(Json(json!({
        "ok": true,
        "reqId": parent.id,
        "branch": entry.name,
        "branchId": entry.id,
        "status": RELEASE_BRANCH_STATUS_RELEASED,
        "advancedSubs": advanced,
    })))
}
