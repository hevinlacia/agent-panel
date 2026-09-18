use super::*;

pub(crate) fn normalize_merge_target(raw: &str) -> ApiResult<String> {
    let target = raw.trim().to_lowercase();
    match target.as_str() {
        "test" | "uat" => Ok(target),
        _ => Err(ApiError::bad_request("target must be test or uat")),
    }
}

#[derive(Debug, Clone)]
pub(crate) struct MergeRequest {
    pub(crate) target: String,
    pub(crate) target_branch: String,
    pub(crate) repo_kind: Option<String>,
}

pub(crate) fn normalize_merge_request(form: &MergeBranchForm) -> ApiResult<MergeRequest> {
    let explicit_branch = form
        .target_branch
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_string);
    let target = if let Some(target) = form
        .target
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
    {
        normalize_merge_target(target)?
    } else if let Some(branch) = explicit_branch.as_deref() {
        target_from_branch(branch)?
    } else {
        return Err(ApiError::bad_request("targetBranch is required"));
    };
    let target_branch = explicit_branch
        .unwrap_or_else(|| if target == "test" { "test" } else { "uat" }.to_string());
    let repo_kind = form
        .repo_kind
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(normalize_repo_kind)
        .transpose()?;
    Ok(MergeRequest {
        target,
        target_branch,
        repo_kind,
    })
}

pub(crate) fn target_from_branch(branch: &str) -> ApiResult<String> {
    if branch == "test" {
        Ok("test".to_string())
    } else if branch == "master" || branch == "uat" || branch.starts_with("UAT-") {
        Ok("uat".to_string())
    } else {
        Err(ApiError::bad_request(
            "targetBranch must be test, master, uat, or UAT-*",
        ))
    }
}

pub(crate) fn normalize_repo_kind(raw: &str) -> ApiResult<String> {
    let kind = raw.trim().to_lowercase();
    match kind.as_str() {
        "frontend" | "front" | "web" | "前端" => Ok("frontend".to_string()),
        "backend" | "back" | "server" | "后端" => Ok("backend".to_string()),
        _ => Err(ApiError::bad_request(
            "repoKind must be frontend or backend",
        )),
    }
}

pub(crate) fn merge_option_target(branch: &str) -> &'static str {
    if branch == "test" {
        "test"
    } else {
        "uat"
    }
}

pub(crate) fn merge_option_label(kind: &str, branch: &str) -> String {
    match (kind, branch) {
        ("frontend", "test") => "前端 test".to_string(),
        ("frontend", "master") => "前端 UAT (master)".to_string(),
        ("backend", "test") => "后端 test".to_string(),
        ("backend", "uat") => "后端 UAT (uat)".to_string(),
        ("backend", branch) if branch.starts_with("UAT-") => format!("后端 UAT ({branch})"),
        _ => branch.to_string(),
    }
}

pub(crate) fn default_merge_selection(
    status: &str,
    kind: &str,
    options: &[String],
) -> Option<String> {
    match status {
        "自测中" => options.iter().find(|v| v.as_str() == "test").cloned(),
        "测试中" if kind == "frontend" => {
            options.iter().find(|v| v.as_str() == "master").cloned()
        }
        "测试中" if kind == "backend" => {
            // 优先选统一命名的小写 uat（WMS 2026-08 起后端 UAT 分支统一为 uat），无则回退 UAT-* 历史分支
            options
                .iter()
                .find(|v| v.as_str() == "uat")
                .cloned()
                .or_else(|| options.iter().find(|v| v.starts_with("UAT-")).cloned())
        }
        _ => None,
    }
}

pub(crate) async fn build_merge_options(scope: &BranchScope, req_status: &str) -> Value {
    let mut has_frontend = false;
    let mut has_backend = false;
    let mut backend_uat: Option<String> = None;
    for repo in &scope.repos {
        if is_pda_client_repo(repo) {
            continue;
        }
        let Some(project_path) =
            resolve_code_review_project_path(repo.path.as_deref(), &repo.repo_name)
        else {
            continue;
        };
        if is_frontend_repo(repo) {
            has_frontend = true;
        } else {
            has_backend = true;
            if backend_uat.is_none() && project_path.exists() {
                backend_uat = detect_latest_uat_branch(&project_path).await;
            }
        }
    }
    let frontend_branches = if has_frontend {
        vec!["test".to_string(), "master".to_string()]
    } else {
        Vec::new()
    };
    let mut backend_branches = if has_backend {
        vec!["test".to_string()]
    } else {
        Vec::new()
    };
    if let Some(branch) = backend_uat {
        backend_branches.push(branch);
    }
    json!({
        "frontend": merge_options_for_kind("frontend", &frontend_branches, req_status),
        "backend": merge_options_for_kind("backend", &backend_branches, req_status),
    })
}

pub(crate) fn merge_options_for_kind(kind: &str, branches: &[String], req_status: &str) -> Value {
    let values = branches
        .iter()
        .map(|branch| {
            json!({
                "value": branch,
                "label": merge_option_label(kind, branch),
                "target": merge_option_target(branch),
            })
        })
        .collect::<Vec<_>>();
    json!({
        "repoKind": kind,
        "options": values,
        "defaultValue": default_merge_selection(req_status, kind, branches),
    })
}

pub(crate) fn merge_overall_status(results: &[Value]) -> &'static str {
    let mut has_conflict = false;
    let mut has_failed = false;
    let mut has_merged = false;
    let mut has_skipped = false;
    let mut has_pending = false;
    let mut has_idle = false;
    for item in results {
        match item
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or_default()
        {
            "conflict" => has_conflict = true,
            "failed" => has_failed = true,
            "merged" | "upToDate" => has_merged = true,
            "skipped" => has_skipped = true,
            "pending" => has_pending = true,
            "idle" => has_idle = true,
            _ => {}
        }
    }
    if has_conflict {
        "conflict"
    } else if has_merged && (has_failed || has_pending) {
        "partial"
    } else if has_failed {
        "failed"
    } else if has_pending {
        "pending"
    } else if has_merged {
        "merged"
    } else if has_skipped && !has_idle {
        "skipped"
    } else if has_idle {
        "idle"
    } else {
        "empty"
    }
}
