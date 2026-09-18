use super::*;

pub(crate) async fn merge_requirement_branches(
    scope: &BranchScope,
    request: &MergeRequest,
    block_prod_branches: bool,
) -> Vec<Value> {
    let mut results = Vec::new();
    for repo in &scope.repos {
        if let Some(kind) = request.repo_kind.as_deref() {
            if repo_kind(repo) != kind {
                continue;
            }
        }
        let branches = if repo.branches.is_empty() {
            vec![String::new()]
        } else {
            repo.branches.clone()
        };
        for branch in branches {
            results.push(merge_repo_branch(repo, &branch, request, block_prod_branches).await);
        }
    }
    results
}

pub(crate) async fn inspect_requirement_merge_status(
    scope: &BranchScope,
    target: Option<String>,
) -> Vec<Value> {
    let targets = match target.as_deref() {
        Some("test") | Some("uat") => vec![target.unwrap()],
        _ => vec!["test".to_string(), "uat".to_string()],
    };
    let mut results = Vec::new();
    for repo in &scope.repos {
        let branches = if repo.branches.is_empty() {
            vec![String::new()]
        } else {
            repo.branches.clone()
        };
        for target in &targets {
            for branch in &branches {
                results.push(inspect_repo_merge_status(repo, branch, target).await);
            }
        }
    }
    results
}

pub(crate) async fn merge_repo_branch(
    repo: &BranchRepo,
    source_branch: &str,
    request: &MergeRequest,
    block_prod_branches: bool,
) -> Value {
    let target = request.target.as_str();
    let source_branch = source_branch.trim();
    let Some(project_path) =
        resolve_code_review_project_path(repo.path.as_deref(), &repo.repo_name)
    else {
        return merge_result(
            repo,
            source_branch,
            target,
            None,
            "failed",
            "branches.json 缺少 path",
            None,
            Vec::new(),
            Vec::new(),
            Vec::new(),
        );
    };
    if !project_path.exists() {
        return merge_result(
            repo,
            source_branch,
            target,
            None,
            "failed",
            &format!("仓库路径不存在：{}", project_path.to_string_lossy()),
            Some(&project_path),
            Vec::new(),
            Vec::new(),
            Vec::new(),
        );
    }
    if source_branch.is_empty() {
        return merge_result(
            repo,
            "(未指定分支)",
            target,
            None,
            "skipped",
            "branches.json 缺少需求分支",
            Some(&project_path),
            Vec::new(),
            Vec::new(),
            Vec::new(),
        );
    }
    let git_root = git(
        &project_path,
        &["rev-parse", "--show-toplevel"],
        30_000,
        COMMAND_OUTPUT_LIMIT,
    )
    .await;
    if !git_root.ok {
        return merge_result(
            repo,
            source_branch,
            target,
            None,
            "failed",
            "projectPath 不是 Git 仓库",
            Some(&project_path),
            Vec::new(),
            Vec::new(),
            vec![git_root.command],
        );
    }

    let target_branch = if request.target_branch == "uat" {
        let Some(resolved) = merge_target_branch_for_repo(repo, target, &project_path).await else {
            return merge_result(
                repo,
                source_branch,
                target,
                None,
                "skipped",
                "当前仓库不适用该环境分支合并；如需启用，请在下拉框选择具体分支",
                Some(&project_path),
                Vec::new(),
                Vec::new(),
                Vec::new(),
            );
        };
        resolved
    } else {
        request.target_branch.clone()
    };
    if !target_branch_matches_repo(repo, &target_branch) {
        return merge_result(
            repo,
            source_branch,
            target,
            None,
            "skipped",
            "所选目标分支不适用于当前仓库类型",
            Some(&project_path),
            Vec::new(),
            Vec::new(),
            Vec::new(),
        );
    }
    if block_prod_branches && is_production_target_branch(repo, &target_branch) {
        return merge_result(
            repo,
            source_branch,
            target,
            Some(&target_branch),
            "skipped",
            "线上问题/测试问题的排查代码仅用于测试环境复现与验证，已拦截合入生产分支；正式生产修复请通过「创建修复需求」转普通需求承接",
            Some(&project_path),
            Vec::new(),
            Vec::new(),
            Vec::new(),
        );
    }
    let worktree_path = merge_worktree_path(&project_path, target, source_branch, &target_branch);
    if worktree_path.exists() {
        let existing = inspect_merge_worktree(
            repo,
            source_branch,
            target,
            &target_branch,
            &project_path,
            &worktree_path,
        )
        .await;
        let existing_status = existing
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if matches!(existing_status, "conflict" | "pending") {
            return existing;
        }
        let remove = git(
            &project_path,
            &[
                "worktree",
                "remove",
                "--force",
                worktree_path.to_string_lossy().as_ref(),
            ],
            30_000,
            COMMAND_OUTPUT_LIMIT,
        )
        .await;
        if !remove.ok {
            return merge_result(
                repo,
                source_branch,
                target,
                Some(&target_branch),
                "failed",
                &format!("旧 merge worktree 清理失败：{}", short_err(&remove)),
                Some(&project_path),
                Vec::new(),
                vec![worktree_path.to_string_lossy().to_string()],
                vec![remove.command],
            );
        }
    }

    let mut warnings = Vec::new();
    for branch_to_fetch in [&target_branch, source_branch] {
        let fetch = git(
            &project_path,
            &["fetch", "origin", branch_to_fetch],
            60_000,
            COMMAND_OUTPUT_LIMIT,
        )
        .await;
        if !fetch.ok {
            warnings.push(format!(
                "fetch {branch_to_fetch} 失败：{}",
                short_err(&fetch)
            ));
        }
    }
    let Some(target_ref) = resolve_branch_ref(&project_path, &target_branch).await else {
        return merge_result(
            repo,
            source_branch,
            target,
            Some(&target_branch),
            "failed",
            &format!("无法解析目标分支 {target_branch}"),
            Some(&project_path),
            Vec::new(),
            warnings,
            Vec::new(),
        );
    };
    let Some(source_ref) = resolve_branch_ref(&project_path, source_branch).await else {
        return merge_result(
            repo,
            source_branch,
            target,
            Some(&target_branch),
            "failed",
            &format!("无法解析需求分支 {source_branch}"),
            Some(&project_path),
            Vec::new(),
            warnings,
            Vec::new(),
        );
    };

    if let Err(err) = fs::create_dir_all(worktree_path.parent().unwrap_or(&project_path)).await {
        return merge_result(
            repo,
            source_branch,
            target,
            Some(&target_branch),
            "failed",
            &format!("创建 merge worktree 目录失败：{err}"),
            Some(&project_path),
            Vec::new(),
            warnings,
            Vec::new(),
        );
    }
    let temp_branch = merge_temp_branch(target, source_branch, &target_branch);
    let add = git(
        &project_path,
        &[
            "worktree",
            "add",
            "-B",
            &temp_branch,
            worktree_path.to_string_lossy().as_ref(),
            &target_ref,
        ],
        60_000,
        COMMAND_OUTPUT_LIMIT,
    )
    .await;
    if !add.ok {
        return merge_result(
            repo,
            source_branch,
            target,
            Some(&target_branch),
            "failed",
            &format!("创建 merge worktree 失败：{}", short_err(&add)),
            Some(&project_path),
            Vec::new(),
            warnings,
            vec![add.command],
        );
    }
    let merge = git(
        &worktree_path,
        &["merge", "--no-ff", "--no-edit", &source_ref],
        120_000,
        COMMAND_OUTPUT_LIMIT,
    )
    .await;
    if !merge.ok {
        let conflicts = conflicted_files(&worktree_path).await;
        let mut commands = vec![add.command.clone(), merge.command.clone()];
        let status = if conflicts.is_empty() {
            "failed"
        } else {
            "conflict"
        };
        let message = if conflicts.is_empty() {
            format!("合并失败：{}", short_err(&merge))
        } else {
            format!("合并冲突：{} 个文件需要处理", conflicts.len())
        };
        if conflicts.is_empty() {
            commands
                .extend(cleanup_merge_worktree(&project_path, &worktree_path, &temp_branch).await);
        }
        return merge_result(
            repo,
            source_branch,
            target,
            Some(&target_branch),
            status,
            &message,
            Some(&project_path),
            conflicts,
            warnings,
            commands,
        );
    }
    let push_ref = format!("HEAD:refs/heads/{target_branch}");
    let push = git(
        &worktree_path,
        &["push", "origin", &push_ref],
        120_000,
        COMMAND_OUTPUT_LIMIT,
    )
    .await;
    if !push.ok {
        let mut commands = vec![
            add.command.clone(),
            merge.command.clone(),
            push.command.clone(),
        ];
        commands.extend(cleanup_merge_worktree(&project_path, &worktree_path, &temp_branch).await);
        return merge_result(
            repo,
            source_branch,
            target,
            Some(&target_branch),
            "failed",
            &format!("推送目标分支失败：{}", short_err(&push)),
            Some(&project_path),
            Vec::new(),
            warnings,
            commands,
        );
    }
    let cleanup_commands =
        cleanup_merge_worktree(&project_path, &worktree_path, &temp_branch).await;
    if cleanup_commands.iter().any(|cmd| cmd.contains(" failed:")) {
        warnings.push("合并已推送，但临时 worktree/分支清理不完整".to_string());
    }
    let status = if merge.stdout.contains("Already up to date")
        || merge.stdout.contains("Already up-to-date")
    {
        "upToDate"
    } else {
        "merged"
    };
    merge_result(
        repo,
        source_branch,
        target,
        Some(&target_branch),
        status,
        "合并并推送完成",
        Some(&project_path),
        Vec::new(),
        warnings,
        vec![
            add.command,
            merge.command,
            push.command,
            cleanup_commands.join(" && "),
        ],
    )
}

pub(crate) async fn inspect_repo_merge_status(
    repo: &BranchRepo,
    source_branch: &str,
    target: &str,
) -> Value {
    let Some(project_path) =
        resolve_code_review_project_path(repo.path.as_deref(), &repo.repo_name)
    else {
        return merge_result(
            repo,
            source_branch,
            target,
            None,
            "failed",
            "branches.json 缺少 path",
            None,
            Vec::new(),
            Vec::new(),
            Vec::new(),
        );
    };
    if !project_path.exists() {
        return merge_result(
            repo,
            source_branch,
            target,
            None,
            "failed",
            &format!("仓库路径不存在：{}", project_path.to_string_lossy()),
            Some(&project_path),
            Vec::new(),
            Vec::new(),
            Vec::new(),
        );
    }
    if source_branch.trim().is_empty() {
        return merge_result(
            repo,
            "(未指定分支)",
            target,
            None,
            "skipped",
            "branches.json 缺少需求分支",
            Some(&project_path),
            Vec::new(),
            Vec::new(),
            Vec::new(),
        );
    }
    let Some(target_branch) = merge_target_branch_for_repo(repo, target, &project_path).await
    else {
        return merge_result(
            repo,
            source_branch,
            target,
            None,
            "skipped",
            "当前仓库不适用该环境分支合并",
            Some(&project_path),
            Vec::new(),
            Vec::new(),
            Vec::new(),
        );
    };
    let worktree_path = merge_worktree_path(&project_path, target, source_branch, &target_branch);
    inspect_merge_worktree(
        repo,
        source_branch,
        target,
        &target_branch,
        &project_path,
        &worktree_path,
    )
    .await
}

pub(crate) async fn inspect_merge_worktree(
    repo: &BranchRepo,
    source_branch: &str,
    target: &str,
    target_branch: &str,
    project_path: &Path,
    worktree_path: &Path,
) -> Value {
    if !worktree_path.exists() {
        return merge_result(
            repo,
            source_branch,
            target,
            Some(target_branch),
            "idle",
            "暂无未完成合并",
            Some(project_path),
            Vec::new(),
            Vec::new(),
            Vec::new(),
        );
    }
    let conflicts = conflicted_files(worktree_path).await;
    if conflicts.is_empty() {
        let status = git(
            worktree_path,
            &["status", "--porcelain"],
            30_000,
            COMMAND_OUTPUT_LIMIT,
        )
        .await;
        let message = if status.ok && status.stdout.trim().is_empty() {
            "merge worktree 存在但无未提交变更"
        } else {
            "merge worktree 存在，待人工检查"
        };
        return merge_result(
            repo,
            source_branch,
            target,
            Some(target_branch),
            "pending",
            message,
            Some(project_path),
            Vec::new(),
            Vec::new(),
            vec![status.command],
        );
    }
    merge_result(
        repo,
        source_branch,
        target,
        Some(target_branch),
        "conflict",
        &format!("合并冲突：{} 个文件需要处理", conflicts.len()),
        Some(project_path),
        conflicts,
        Vec::new(),
        Vec::new(),
    )
}

pub(crate) fn merge_result(
    repo: &BranchRepo,
    source_branch: &str,
    target: &str,
    target_branch: Option<&str>,
    status: &str,
    message: &str,
    project_path: Option<&Path>,
    conflict_files: Vec<String>,
    warnings: Vec<String>,
    commands: Vec<String>,
) -> Value {
    let worktree_path = project_path
        .and_then(|path| {
            target_branch.map(|branch| merge_worktree_path(path, target, source_branch, branch))
        })
        .map(|path| path.to_string_lossy().to_string());
    json!({
        "repoName": repo.repo_name,
        "role": repo.role,
        "projectPath": project_path.map(|p| p.to_string_lossy().to_string()),
        "sourceBranch": source_branch,
        "target": target,
        "targetBranch": target_branch,
        "status": status,
        "message": message,
        "conflictFiles": conflict_files,
        "worktreePath": worktree_path,
        "warnings": warnings,
        "commands": commands,
    })
}

pub(crate) async fn merge_target_branch_for_repo(
    repo: &BranchRepo,
    target: &str,
    project_path: &Path,
) -> Option<String> {
    let explicit = if target == "test" {
        repo.test_target_branch.as_deref()
    } else if target == "uat" {
        repo.uat_target_branch.as_deref()
    } else {
        None
    }
    .map(str::trim)
    .filter(|v| !v.is_empty())
    .map(str::to_string);
    if explicit.is_some() {
        return explicit;
    }
    if is_pda_client_repo(repo) {
        return None;
    }
    if target == "test" {
        return Some("test".to_string());
    }
    if target != "uat" {
        return None;
    }
    if is_frontend_repo(repo) {
        return Some("master".to_string());
    }
    detect_latest_uat_branch(project_path).await
}

pub(crate) fn is_frontend_repo(repo: &BranchRepo) -> bool {
    let role = repo.role.as_deref().unwrap_or_default();
    let path = repo.path.as_deref().unwrap_or_default();
    role.contains("前端") || path.contains("/frontend/") || path.contains("\\frontend\\")
}

pub(crate) fn is_pda_client_repo(repo: &BranchRepo) -> bool {
    let role = repo.role.as_deref().unwrap_or_default();
    let path = repo.path.as_deref().unwrap_or_default();
    role == "PDA" || path.contains("/pda/") || path.contains("\\pda\\")
}

pub(crate) fn repo_kind(repo: &BranchRepo) -> &'static str {
    if is_pda_client_repo(repo) {
        "pda"
    } else if is_frontend_repo(repo) {
        "frontend"
    } else {
        "backend"
    }
}

pub(crate) fn target_branch_matches_repo(repo: &BranchRepo, target_branch: &str) -> bool {
    if is_pda_client_repo(repo) {
        return false;
    }
    if is_frontend_repo(repo) {
        matches!(target_branch, "test" | "master")
    } else {
        target_branch == "test" || target_branch == "uat" || target_branch.starts_with("UAT-")
    }
}

pub(crate) async fn detect_latest_uat_branch(project_path: &Path) -> Option<String> {
    // WMS 后端 2026-08 起 UAT 分支统一命名为小写 uat，优先返回远端 uat 分支（如存在）
    let _ = git(
        project_path,
        &["fetch", "origin", "uat"],
        60_000,
        COMMAND_OUTPUT_LIMIT,
    )
    .await;
    let has_uat = git(
        project_path,
        &["rev-parse", "--verify", "origin/uat"],
        30_000,
        COMMAND_OUTPUT_LIMIT,
    )
    .await;
    if has_uat.ok {
        return Some("uat".to_string());
    }
    // 回退：扫描历史 UAT-* 前缀分支，取 committerdate 最新
    let _ = git(
        project_path,
        &[
            "fetch",
            "origin",
            "+refs/heads/UAT-*:refs/remotes/origin/UAT-*",
        ],
        60_000,
        COMMAND_OUTPUT_LIMIT,
    )
    .await;
    let result = git(
        project_path,
        &[
            "for-each-ref",
            "--sort=-committerdate",
            "--format=%(refname:short)",
            "refs/remotes/origin/UAT-*",
        ],
        30_000,
        COMMAND_OUTPUT_LIMIT,
    )
    .await;
    if !result.ok {
        return None;
    }
    result
        .stdout
        .lines()
        .map(str::trim)
        .find(|line| line.starts_with("origin/UAT-"))
        .map(|line| line.trim_start_matches("origin/").to_string())
}

pub(crate) async fn resolve_branch_ref(project_path: &Path, branch: &str) -> Option<String> {
    let remote_branch = format!("origin/{branch}");
    let remote_ref = format!("{}^{{commit}}", remote_branch);
    let remote = git(
        project_path,
        &["rev-parse", "--verify", &remote_ref],
        30_000,
        COMMAND_OUTPUT_LIMIT,
    )
    .await;
    if remote.ok {
        return Some(remote_branch);
    }
    let local_ref = format!("{}^{{commit}}", branch);
    let local = git(
        project_path,
        &["rev-parse", "--verify", &local_ref],
        30_000,
        COMMAND_OUTPUT_LIMIT,
    )
    .await;
    local.ok.then_some(branch.to_string())
}

pub(crate) async fn conflicted_files(worktree_path: &Path) -> Vec<String> {
    let result = git(
        worktree_path,
        &["diff", "--name-only", "--diff-filter=U"],
        30_000,
        COMMAND_OUTPUT_LIMIT,
    )
    .await;
    if !result.ok {
        return Vec::new();
    }
    result
        .stdout
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect()
}

pub(crate) async fn cleanup_merge_worktree(
    project_path: &Path,
    worktree_path: &Path,
    temp_branch: &str,
) -> Vec<String> {
    let remove = git(
        &project_path,
        &[
            "worktree",
            "remove",
            "--force",
            worktree_path.to_string_lossy().as_ref(),
        ],
        30_000,
        COMMAND_OUTPUT_LIMIT,
    )
    .await;
    let delete_branch = git(
        &project_path,
        &["branch", "-D", temp_branch],
        30_000,
        COMMAND_OUTPUT_LIMIT,
    )
    .await;
    vec![
        if remove.ok {
            remove.command
        } else {
            format!("{} failed: {}", remove.command, short_err(&remove))
        },
        if delete_branch.ok {
            delete_branch.command
        } else {
            format!(
                "{} failed: {}",
                delete_branch.command,
                short_err(&delete_branch)
            )
        },
    ]
}

pub(crate) fn merge_worktree_path(
    project_path: &Path,
    target: &str,
    source_branch: &str,
    target_branch: &str,
) -> PathBuf {
    let repo_leaf = project_path
        .file_name()
        .map(|v| v.to_string_lossy().to_string())
        .unwrap_or_else(|| "repo".to_string());
    project_path
        .parent()
        .unwrap_or(project_path)
        .join(".agent-panel-merge-worktrees")
        .join(repo_leaf)
        .join(target)
        .join(format!(
            "{}__{}",
            sanitize_ref_segment(target_branch),
            sanitize_ref_segment(source_branch)
        ))
}

pub(crate) fn merge_temp_branch(target: &str, source_branch: &str, target_branch: &str) -> String {
    format!(
        "agent-panel/merge/{}/{}/{}",
        sanitize_ref_segment(target),
        sanitize_ref_segment(target_branch),
        sanitize_ref_segment(source_branch)
    )
}

pub(crate) fn sanitize_ref_segment(value: &str) -> String {
    let mut out = String::new();
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
            out.push(ch);
        } else {
            out.push('-');
        }
    }
    let compact = out.trim_matches('-').to_string();
    if compact.is_empty() {
        "branch".to_string()
    } else {
        compact
    }
}
