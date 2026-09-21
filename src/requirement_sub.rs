use axum::{extract::State, Json};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::*;

/// 子需求分支操作入参（init-branches / sync-parent / merge-to-parent 共用）。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SubBranchOpForm {
    pub(crate) req_id: String,
    /// init-branches 专用：是否同时创建 worktree（`<repo>/.worktrees/<reqId>`），默认 true。
    #[serde(default)]
    pub(crate) create_worktree: Option<bool>,
    /// init-branches 专用：仅登记这些仓库（repoName 过滤，缺省继承父需求全部仓库）。
    #[serde(default)]
    pub(crate) repos: Option<Vec<String>>,
    /// init-branches 专用：子需求 branches.json 已存在时是否重建（覆盖）。
    #[serde(default)]
    pub(crate) overwrite: Option<bool>,
    /// merge-to-parent 专用：显式确认（合并/推送父需求分支为高风险操作，UI 先展示选项再确认）。
    #[serde(default)]
    pub(crate) confirm: Option<bool>,
}

fn sub_n_from_id(req_id: &str) -> ApiResult<u64> {
    sub_req_seq(req_id).ok_or_else(|| {
        ApiError::bad_request(format!(
            "reqId `{req_id}` 不是子需求编号格式（缺少 -S<n> 段）"
        ))
    })
}

async fn load_sub_and_parent(
    state: &AppState,
    req_id: &str,
) -> ApiResult<(Requirement, Requirement)> {
    let sub = get_real_requirement(state, req_id).await?;
    if !sub.is_sub_req {
        return Err(ApiError::bad_request(format!(
            "{req_id} 不是子需求（meta.md 缺少 parent-req-id）"
        )));
    }
    let parent_id = sub
        .parent_req_id
        .clone()
        .ok_or_else(|| ApiError::bad_request("子需求缺少 parent-req-id"))?;
    let parent = get_real_requirement(state, &parent_id).await?;
    Ok((sub, parent))
}

/// 父需求当前分支范围：优先最新登记轮次（含生产后修复轮次），未登记则报错。
async fn load_parent_scope(parent: &Requirement) -> ApiResult<BranchScope> {
    let dir = req_dir_path(parent)?;
    let rounds = list_branch_scope_rounds(&dir).await?;
    let latest = rounds
        .last()
        .ok_or_else(|| {
            ApiError::bad_request(format!(
                "父需求 {} 未登记分支（branches.json），请先登记分支再初始化子需求分支",
                parent.id
            ))
        })?
        .round;
    read_branch_scope_round(&dir, latest)
        .await?
        .ok_or_else(|| ApiError::bad_request("父需求分支登记文件存在但无有效仓库"))
}

/// 从子需求 branches.json 读取范围；缺 baseRef 时报错（baseRef = 父分支，init 时写入）。
async fn load_sub_scope(sub: &Requirement) -> ApiResult<BranchScope> {
    let dir = req_dir_path(sub)?;
    read_branch_scope(&dir).await?.ok_or_else(|| {
        ApiError::bad_request(format!(
            "子需求 {} 未登记分支，请先调 POST /api/requirement/sub/init-branches",
            sub.id
        ))
    })
}

fn repo_filter_applies(form: &SubBranchOpForm, repo: &BranchRepo) -> bool {
    match form.repos.as_ref() {
        None => true,
        Some(names) => names
            .iter()
            .any(|n| n.trim().eq_ignore_ascii_case(repo.repo_name.trim())),
    }
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

/// POST /api/requirement/sub/init-branches
///
/// 以父需求分支为 base 初始化子需求分支：写子需求 branches.json（repo 级 baseRef=父分支，
/// branches=`<父分支>-sub<n>`），可选一键建分支 + worktree。幂等：分支/worktree 已存在则跳过。
pub(crate) async fn api_requirement_sub_init_branches(
    State(state): State<AppState>,
    form: FormOrJson<SubBranchOpForm>,
) -> ApiResult<Json<Value>> {
    let body = form.0;
    let (sub, parent) = load_sub_and_parent(&state, &body.req_id).await?;
    let sub_n = sub_n_from_id(&sub.id)?;
    let parent_scope = load_parent_scope(&parent).await?;
    let mut repos = Vec::new();
    for repo in &parent_scope.repos {
        if !repo_filter_applies(&body, repo) {
            continue;
        }
        let mut entry = repo.clone();
        entry.base_ref = repo.branches.first().cloned();
        entry.branches = repo
            .branches
            .iter()
            .map(|b| sub_branch_name(b, sub_n))
            .collect();
        entry.test_target_branch = None;
        entry.uat_target_branch = None;
        repos.push(entry);
    }
    if repos.is_empty() {
        return Err(ApiError::bad_request(
            "repos 过滤后没有匹配的仓库；请检查 repos 参数或父需求分支登记",
        ));
    }
    let sub_dir = req_dir_path(&sub)?;
    let scope_path = sub_dir.join(BRANCH_SCOPE_FILE);
    if scope_path.is_file() && !body.overwrite.unwrap_or(false) {
        return Err(ApiError::bad_request(
            "子需求 branches.json 已存在；确认重建请传 overwrite=true（会覆盖现有登记）",
        ));
    }
    let scope = BranchScope {
        version: 2,
        updated_at: now_ms(),
        repos,
        fallback: false,
        round: 1,
    };
    atomic_write_json(
        &scope_path,
        &serde_json::to_value(&scope).unwrap_or_default(),
    )
    .await?;
    let create_worktree = body.create_worktree.unwrap_or(true);
    let repo_results = init_sub_branch_repos(&scope, sub_n, &sub.id, create_worktree).await;
    let all_ok = repo_results
        .iter()
        .all(|r| !matches!(r.get("status").and_then(Value::as_str), Some("failed")));
    if all_ok {
        append_note_quiet(
            &state,
            &sub.id,
            "初始化分支",
            &format!(
                "以父需求 {} 分支为 base 初始化子分支：{}",
                parent.id,
                scope
                    .repos
                    .iter()
                    .flat_map(|r| r.branches.iter().cloned())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        )
        .await;
    }
    Ok(Json(json!({
        "ok": all_ok,
        "reqId": sub.id,
        "parentReqId": parent.id,
        "branchScopeFile": scope_path.to_string_lossy(),
        "repos": repo_results,
    })))
}

/// POST /api/requirement/sub/sync-parent
///
/// 同步父需求分支最新成果到子分支（逐仓把 baseRef=父分支 合入子分支）。
/// 兄弟子需求先后合入父分支后，靠本操作拉齐进度；冲突保留在 merge worktree 中
/// 由人工在子分支侧解决（符合「把生产/主分支合入自己分支解冲突」的 WMS 规范）。
pub(crate) async fn api_requirement_sub_sync_parent(
    State(state): State<AppState>,
    form: FormOrJson<SubBranchOpForm>,
) -> ApiResult<Json<Value>> {
    let body = form.0;
    let (sub, parent) = load_sub_and_parent(&state, &body.req_id).await?;
    let sub_scope = load_sub_scope(&sub).await?;
    let mut repo_results = Vec::new();
    for repo in &sub_scope.repos {
        if !repo_filter_applies(&body, repo) {
            continue;
        }
        let Some(parent_branch) = repo
            .base_ref
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty())
        else {
            repo_results.push(json!({
                "repoName": repo.repo_name,
                "status": "skipped",
                "message": "子需求 branches.json 缺少 baseRef（父分支）；请先 init-branches 或 overwrite 重建",
            }));
            continue;
        };
        let sub_branch = repo.branches.first().cloned().unwrap_or_default();
        repo_results.push(merge_branch_pair(repo, parent_branch, &sub_branch, "sub-sync").await);
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
            &sub.id,
            "同步主需求分支",
            &format!("已把父需求 {} 分支最新成果合入子分支（逐仓）", parent.id),
        )
        .await;
    }
    Ok(Json(json!({
        "ok": all_ok,
        "reqId": sub.id,
        "parentReqId": parent.id,
        "repos": repo_results,
    })))
}

/// POST /api/requirement/sub/merge-to-parent
///
/// 子需求合入主需求：逐仓把子分支合入父分支（隔离 worktree + 推送），全部成功后
/// 自动推进子需求状态到已合入（需求创建 → 开发中 → 已合入，中间态自动补齐）。
/// 需要 confirm=true 显式确认；不校验 Review Gate（代码审查在父需求内统一走）。
pub(crate) async fn api_requirement_sub_merge_to_parent(
    State(state): State<AppState>,
    form: FormOrJson<SubBranchOpForm>,
) -> ApiResult<Json<Value>> {
    let body = form.0;
    if !body.confirm.unwrap_or(false) {
        return Err(ApiError::bad_request(
            "合入主需求会修改父需求分支并推送，属于高风险操作：UI 需先展示合并选项，请求必须带 confirm=true",
        ));
    }
    let (sub, parent) = load_sub_and_parent(&state, &body.req_id).await?;
    if matches!(parent.status.as_str(), "经验总结" | "已完成") {
        return Err(ApiError::bad_request(format!(
            "父需求 {} 已进入 {}，不能再合入；如需继续并行请确认父需求状态",
            parent.id, parent.status
        )));
    }
    if matches!(sub.status.as_str(), "已合入" | "已取消") {
        return Err(ApiError::bad_request(format!(
            "子需求 {} 已是终态（{}），不可再合入",
            sub.id, sub.status
        )));
    }
    let sub_scope = load_sub_scope(&sub).await?;
    let mut repo_results = Vec::new();
    for repo in &sub_scope.repos {
        if !repo_filter_applies(&body, repo) {
            continue;
        }
        let sub_branch = repo.branches.first().cloned().unwrap_or_default();
        let Some(parent_branch) = repo
            .base_ref
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty())
        else {
            repo_results.push(json!({
                "repoName": repo.repo_name,
                "status": "skipped",
                "message": "子需求 branches.json 缺少 baseRef（父分支）；请先 init-branches 或 overwrite 重建",
            }));
            continue;
        };
        repo_results.push(merge_branch_pair(repo, &sub_branch, parent_branch, "sub-merge").await);
    }
    let all_ok = !repo_results.is_empty()
        && repo_results.iter().all(|r| {
            matches!(
                r.get("status").and_then(Value::as_str),
                Some("merged") | Some("upToDate")
            )
        });
    let mut status_state = Value::Null;
    if all_ok {
        let sub_dir = req_dir_path(&sub)?;
        // 状态自动补齐：需求创建 → 开发中 → 已合入（不走门禁，系统内部流转）。
        if sub.status == "需求创建" {
            write_requirement_status_checked(
                sub_dir.to_string_lossy().as_ref(),
                "开发中",
                Some("合入主需求前自动推进"),
                GateCheckMode::None,
            )
            .await?;
        }
        status_state = write_requirement_status_checked(
            sub_dir.to_string_lossy().as_ref(),
            "已合入",
            Some(&format!("子分支已全部合入父需求 {} 分支", parent.id)),
            GateCheckMode::None,
        )
        .await?;
        if let Some(st) = status_state.as_object() {
            // 状态实际变化时记录事件流（幂等：无变化不记）。
            if st.get("changed").and_then(Value::as_bool) == Some(true) {
                record_status_transition_event(&state, &sub, &status_state, None)
                    .await
                    .ok();
            }
        }
        append_note_quiet(
            &state,
            &sub.id,
            "合入主需求",
            &format!(
                "子分支已全部合入父需求 {} 分支，状态推进为已合入",
                parent.id
            ),
        )
        .await;
        append_note_quiet(
            &state,
            &parent.id,
            "子需求合入",
            &format!(
                "子需求 `{}` 已合入主需求分支，可在父需求内统一走 Review Gate 与环境集成",
                sub.id
            ),
        )
        .await;
    }
    Ok(Json(json!({
        "ok": all_ok,
        "reqId": sub.id,
        "parentReqId": parent.id,
        "repos": repo_results,
        "statusState": status_state,
    })))
}
