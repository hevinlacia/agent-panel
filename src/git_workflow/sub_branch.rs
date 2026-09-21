use super::*;

/// 子需求分支名：`<父分支>-sub<n>`。子分支以父分支为 base，合回父分支；
/// 永不直接触碰 test/UAT/生产分支（环境集成由父需求统一执行）。
pub(crate) fn sub_branch_name(parent_branch: &str, sub_n: u64) -> String {
    format!("{}-sub{}", parent_branch.trim_end_matches('-'), sub_n)
}

/// 为子需求初始化分支（可选 worktree）：以父分支当前指向为 base 创建 `<父分支>-sub<n>`。
///
/// - 分支已存在则跳过创建（不移动已有分支，幂等）；
/// - 创建后 push -u origin（失败仅警告，不阻断本地并行开发）；
/// - worktree 路径 = `<repo>/.worktrees/<subReqId>`（WMS worktree 约定位置）。
pub(crate) async fn init_sub_branch_repos(
    scope: &BranchScope,
    sub_n: u64,
    sub_req_id: &str,
    create_worktree: bool,
) -> Vec<Value> {
    let mut results = Vec::new();
    for repo in &scope.repos {
        let branches = if repo.branches.is_empty() {
            vec![String::new()]
        } else {
            repo.branches.clone()
        };
        for parent_branch in branches {
            results.push(
                init_sub_branch_repo(repo, &parent_branch, sub_n, sub_req_id, create_worktree)
                    .await,
            );
        }
    }
    results
}

async fn init_sub_branch_repo(
    repo: &BranchRepo,
    parent_branch: &str,
    sub_n: u64,
    sub_req_id: &str,
    create_worktree: bool,
) -> Value {
    let sub_branch = sub_branch_name(parent_branch, sub_n);
    let skipped = |status: &str, message: String| {
        json!({
            "repoName": repo.repo_name,
            "role": repo.role,
            "parentBranch": parent_branch,
            "subBranch": sub_branch,
            "status": status,
            "message": message,
            "warnings": Vec::<String>::new(),
            "commands": Vec::<String>::new(),
        })
    };
    let Some(project_path) =
        resolve_code_review_project_path(repo.path.as_deref(), &repo.repo_name)
    else {
        return skipped("skipped", "branches.json 缺少 path".into());
    };
    if !project_path.exists() {
        return skipped(
            "skipped",
            format!("仓库路径不存在：{}", project_path.to_string_lossy()),
        );
    }
    if parent_branch.trim().is_empty() {
        return skipped("skipped", "父需求 branches.json 缺少分支".into());
    }
    let git_root = git(
        &project_path,
        &["rev-parse", "--show-toplevel"],
        30_000,
        COMMAND_OUTPUT_LIMIT,
    )
    .await;
    if !git_root.ok {
        return skipped("failed", "仓库路径不是 Git 仓库".into());
    }
    let _ = git(
        &project_path,
        &["fetch", "origin", parent_branch],
        60_000,
        COMMAND_OUTPUT_LIMIT,
    )
    .await;
    let Some(parent_ref) = resolve_branch_ref_local_first(&project_path, parent_branch).await
    else {
        return skipped("failed", format!("无法解析父分支 {parent_branch}"));
    };
    let mut commands = Vec::new();
    let mut warnings = Vec::new();
    let local_ref = format!("refs/heads/{sub_branch}");
    let exists = git(
        &project_path,
        &["rev-parse", "--verify", "--quiet", &local_ref],
        30_000,
        COMMAND_OUTPUT_LIMIT,
    )
    .await;
    let branch_status = if exists.ok {
        "exists"
    } else {
        let create = git(
            &project_path,
            &["branch", &sub_branch, &parent_ref],
            30_000,
            COMMAND_OUTPUT_LIMIT,
        )
        .await;
        if !create.ok {
            return skipped(
                "failed",
                format!("创建分支 {sub_branch} 失败：{}", short_err(&create)),
            );
        }
        commands.push(create.command);
        "created"
    };
    let push = git(
        &project_path,
        &["push", "-u", "origin", &sub_branch],
        120_000,
        COMMAND_OUTPUT_LIMIT,
    )
    .await;
    if push.ok {
        commands.push(push.command);
    } else {
        warnings.push(format!(
            "push -u origin {sub_branch} 失败（本地分支已就绪）：{}",
            short_err(&push)
        ));
    }
    let worktree_path = project_path.join(".worktrees").join(sub_req_id);
    let mut worktree_status = "skipped";
    if create_worktree {
        if worktree_path.exists() {
            worktree_status = "exists";
            warnings.push(format!(
                "worktree 已存在：{}（未重复创建）",
                worktree_path.display()
            ));
        } else {
            let add = git(
                &project_path,
                &[
                    "worktree",
                    "add",
                    worktree_path.to_string_lossy().as_ref(),
                    &sub_branch,
                ],
                60_000,
                COMMAND_OUTPUT_LIMIT,
            )
            .await;
            if add.ok {
                commands.push(add.command);
                worktree_status = "created";
            } else {
                worktree_status = "failed";
                warnings.push(format!("创建 worktree 失败：{}", short_err(&add)));
            }
        }
    }
    json!({
        "repoName": repo.repo_name,
        "role": repo.role,
        "projectPath": project_path.to_string_lossy(),
        "parentBranch": parent_branch,
        "parentRef": parent_ref,
        "subBranch": sub_branch,
        "branchStatus": branch_status,
        "worktreeStatus": worktree_status,
        "worktreePath": worktree_path.to_string_lossy(),
        "status": if worktree_status == "failed" { "partial" } else { "ok" },
        "message": format!("分支 {sub_branch}（base={parent_branch}）"),
        "warnings": warnings,
        "commands": commands,
    })
}

/// 通用隔离 worktree 合并：在独立 merge worktree + 临时分支上把 source_branch
/// 合入 target_branch，成功后推送目标分支。冲突时保留 worktree 供人工处理
/// （把目标分支合入源侧的冲突在"目标分支被合入的一方"解决——与 WMS 冲突规范一致：
/// 子需求场景冲突一律在子分支侧解决，即同步父分支方向）。
/// 与 merge_repo_branch 的差异：目标分支由调用方显式给定（需求分支），不走
/// test/uat 环境分支解析，因此可同时用于「父→子同步」和「子→父合入」两个方向。
pub(crate) async fn merge_branch_pair(
    repo: &BranchRepo,
    source_branch: &str,
    target_branch: &str,
    target_label: &str,
) -> Value {
    let source_branch = source_branch.trim();
    let target_branch = target_branch.trim();
    let failed = |status: &str, message: String| {
        merge_result(
            repo,
            source_branch,
            target_label,
            Some(target_branch),
            status,
            &message,
            None,
            Vec::new(),
            Vec::new(),
            Vec::new(),
        )
    };
    let Some(project_path) =
        resolve_code_review_project_path(repo.path.as_deref(), &repo.repo_name)
    else {
        return failed("failed", "branches.json 缺少 path".into());
    };
    if !project_path.exists() {
        return failed(
            "failed",
            format!("仓库路径不存在：{}", project_path.to_string_lossy()),
        );
    }
    if source_branch.is_empty() || target_branch.is_empty() {
        return failed("skipped", "缺少源分支或目标分支".into());
    }
    // 防御：子需求两条合并方向的目标/源都不应是生产分支。
    if is_production_target_branch(repo, target_branch) {
        return failed(
            "skipped",
            "目标分支是生产分支，已拦截；子需求只与父需求分支交互".into(),
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
        return failed("failed", "仓库路径不是 Git 仓库".into());
    }
    let worktree_path =
        merge_worktree_path(&project_path, target_label, source_branch, target_branch);
    if worktree_path.exists() {
        let existing = inspect_merge_worktree(
            repo,
            source_branch,
            target_label,
            target_branch,
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
            return failed(
                "failed",
                format!("旧 merge worktree 清理失败：{}", short_err(&remove)),
            );
        }
    }
    let mut warnings = Vec::new();
    for branch_to_fetch in [target_branch, source_branch] {
        let fetch = git(
            &project_path,
            &["fetch", "origin", branch_to_fetch],
            60_000,
            COMMAND_OUTPUT_LIMIT,
        )
        .await;
        if !fetch.ok {
            warnings.push(format!(
                "fetch {branch_to_fetch} 失败（远端不存在时忽略）：{}",
                short_err(&fetch)
            ));
        }
    }
    let Some(target_ref) = resolve_branch_ref_local_first(&project_path, target_branch).await
    else {
        return failed("failed", format!("无法解析目标分支 {target_branch}"));
    };
    let Some(source_ref) = resolve_branch_ref_local_first(&project_path, source_branch).await
    else {
        return failed("failed", format!("无法解析源分支 {source_branch}"));
    };
    if let Err(err) = fs::create_dir_all(worktree_path.parent().unwrap_or(&project_path)).await {
        return failed("failed", format!("创建 merge worktree 目录失败：{err}"));
    }
    let temp_branch = merge_temp_branch(target_label, source_branch, target_branch);
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
        return failed(
            "failed",
            format!("创建 merge worktree 失败：{}", short_err(&add)),
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
            target_label,
            Some(target_branch),
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
            target_label,
            Some(target_branch),
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
        target_label,
        Some(target_branch),
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

/// 子需求方向的分支 ref 解析：本地分支优先，回退 origin/<branch>。
/// 与 resolve_branch_ref（远端优先，适合环境分支）相反：父子分支都以本地开发进度为准，
/// 远端 tracking ref 可能滞后（用户本地推送给远端的节奏不固定）。
async fn resolve_branch_ref_local_first(project_path: &Path, branch: &str) -> Option<String> {
    let local_ref = format!("{}^{{commit}}", branch);
    let local = git(
        project_path,
        &["rev-parse", "--verify", &local_ref],
        30_000,
        COMMAND_OUTPUT_LIMIT,
    )
    .await;
    if local.ok {
        return Some(branch.to_string());
    }
    let remote_branch = format!("origin/{branch}");
    let remote_ref = format!("{}^{{commit}}", remote_branch);
    let remote = git(
        project_path,
        &["rev-parse", "--verify", &remote_ref],
        30_000,
        COMMAND_OUTPUT_LIMIT,
    )
    .await;
    remote.ok.then_some(remote_branch)
}
