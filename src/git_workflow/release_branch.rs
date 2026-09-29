use super::*;

/// 判断分支是否已存在于远端（`git ls-remote --heads origin <branch>`）。
/// 用于整合需求发布分支命名唯一性兜底：registry 序号之外再确认远端无同名分支。
pub(crate) async fn remote_branch_exists(project_path: &Path, branch: &str) -> bool {
    let result = git(
        project_path,
        &["ls-remote", "--heads", "origin", branch],
        60_000,
        COMMAND_OUTPUT_LIMIT,
    )
    .await;
    result.ok && !result.stdout.trim().is_empty()
}

/// 创建整合需求发布分支（单仓）：以 `origin/<base_branch>`（生产分支）当前指向为 base
/// 创建本地分支并 push -u。分支已存在则跳过（幂等，不移动已有分支）。
/// 与子需求分支不同：发布分支不需要 worktree（开发不直接在其上进行，集成走隔离 merge worktree）。
pub(crate) async fn create_release_branch_repo(
    repo: &BranchRepo,
    base_branch: &str,
    branch_name: &str,
) -> Value {
    let skipped = |status: &str, message: String| {
        json!({
            "repoName": repo.repo_name,
            "role": repo.role,
            "baseBranch": base_branch,
            "branch": branch_name,
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
    let fetch = git(
        &project_path,
        &["fetch", "origin", base_branch],
        60_000,
        COMMAND_OUTPUT_LIMIT,
    )
    .await;
    let mut warnings = Vec::new();
    if !fetch.ok {
        warnings.push(format!(
            "fetch origin {base_branch} 失败：{}",
            short_err(&fetch)
        ));
    }
    let base_ref = format!("origin/{base_branch}");
    let base_commit = git(
        &project_path,
        &["rev-parse", "--verify", &format!("{base_ref}^{{commit}}")],
        30_000,
        COMMAND_OUTPUT_LIMIT,
    )
    .await;
    if !base_commit.ok {
        return skipped(
            "failed",
            format!(
                "无法解析生产分支 {base_branch}（远端 origin/{base_branch} 不存在？）：{}",
                short_err(&base_commit)
            ),
        );
    }
    let base_sha = base_commit.stdout.trim().to_string();
    let base_short: String = base_sha.chars().take(8).collect();
    let mut commands = Vec::new();
    let local_ref = format!("refs/heads/{branch_name}");
    let exists = git(
        &project_path,
        &["rev-parse", "--verify", "--quiet", &local_ref],
        30_000,
        COMMAND_OUTPUT_LIMIT,
    )
    .await;
    let branch_status = if exists.ok {
        // 已有本地分支：校验它确实指向生产分支基点附近（不强制移动，避免覆盖人工进度）。
        warnings.push("本地分支已存在（未重建，保持现状）".to_string());
        "exists"
    } else {
        let create = git(
            &project_path,
            &["branch", &branch_name, &base_sha],
            30_000,
            COMMAND_OUTPUT_LIMIT,
        )
        .await;
        if !create.ok {
            return skipped(
                "failed",
                format!("创建分支 {branch_name} 失败：{}", short_err(&create)),
            );
        }
        commands.push(create.command);
        "created"
    };
    let push = git(
        &project_path,
        &["push", "-u", "origin", &branch_name],
        120_000,
        COMMAND_OUTPUT_LIMIT,
    )
    .await;
    if push.ok {
        commands.push(push.command);
    } else {
        // 远端已有同名分支（他人/上次已推）不算失败；其余 push 失败降级为 partial 警告。
        if remote_branch_exists(&project_path, branch_name).await {
            warnings.push("远端同名分支已存在（push 被拒，保持远端现状）".to_string());
        } else {
            warnings.push(format!(
                "push -u origin {branch_name} 失败（本地分支已就绪）：{}",
                short_err(&push)
            ));
        }
    }
    json!({
        "repoName": repo.repo_name,
        "role": repo.role,
        "projectPath": project_path.to_string_lossy(),
        "baseBranch": base_branch,
        "baseCommit": base_sha,
        "branch": branch_name,
        "branchStatus": branch_status,
        "status": "ok",
        "message": format!("发布分支 {branch_name}（base=origin/{base_branch}@{base_short}）"),
        "warnings": warnings,
        "commands": commands,
    })
}

/// 发布分支相对于生产分支的差异统计：与生产 MR 的三点 diff 同口径，但发布分支是
/// 远端权威的共享集成分支，源解析远端优先（本地同名分支可能在隔离 worktree 合并后滞后，
/// 本地优先会把未拉取的合入误判为无差异）。
pub(crate) async fn release_branch_diff_stat(
    project_path: &Path,
    base_branch: &str,
    branch_name: &str,
) -> Option<(i64, i64, i64)> {
    let _ = git(
        project_path,
        &["fetch", "origin", base_branch],
        60_000,
        COMMAND_OUTPUT_LIMIT,
    )
    .await;
    let _ = git(
        project_path,
        &["fetch", "origin", branch_name],
        60_000,
        COMMAND_OUTPUT_LIMIT,
    )
    .await;
    let base_ref = format!("origin/{base_branch}");
    let remote = format!("origin/{branch_name}");
    let source_ref =
        if git(
            project_path,
            &["rev-parse", "--verify", &format!("{remote}^{{commit}}")],
            30_000,
            COMMAND_OUTPUT_LIMIT,
        )
        .await
        .ok
        {
            remote
        } else {
            branch_name.to_string()
        };
    let range = format!("{base_ref}...{source_ref}");
    let numstat = git(
        project_path,
        &["diff", "--numstat", "--find-renames", &range, "--"],
        30_000,
        COMMAND_OUTPUT_LIMIT,
    )
    .await;
    if !numstat.ok {
        return None;
    }
    let mut files = 0i64;
    let mut additions = 0i64;
    let mut deletions = 0i64;
    for line in numstat.stdout.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        files += 1;
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 2 {
            if let Ok(a) = parts[0].parse::<i64>() {
                additions += a;
            }
            if let Ok(d) = parts[1].parse::<i64>() {
                deletions += d;
            }
        }
    }
    Some((files, additions, deletions))
}
