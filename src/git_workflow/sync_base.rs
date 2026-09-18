use super::*;

/// 同步单个仓库的本地生产基线分支到最新远端:
/// 1. git fetch <remote> <base-branch>(更新 remote-tracking ref,diff 基线即从此读取)
/// 2. 把本地 <base-branch> 分支指向 <remote>/<base-branch>:
///    - 当前 HEAD == base-branch 且工作区干净 -> git reset --hard
///    - 当前 HEAD == base-branch 但工作区脏 -> 跳过 reset,仅 fetch(保护未提交改动)
///    - 当前 HEAD != base-branch -> git branch -f(不动工作区)
pub(crate) async fn sync_repo_base_branch(repo: &BranchRepo) -> Value {
    let repo_name = repo.repo_name.clone();
    let project_path = match resolve_code_review_project_path(repo.path.as_deref(), &repo.repo_name)
    {
        Some(p) => p,
        None => {
            return json!({
                "repoName": repo_name,
                "ok": false,
                "status": "skipped",
                "message": "branches.json 缺少 path",
            })
        }
    };
    if !project_path.exists() {
        return json!({
            "repoName": repo_name,
            "ok": false,
            "status": "skipped",
            "message": format!("仓库路径不存在：{}", project_path.to_string_lossy()),
        });
    }
    let git_root = git(
        &project_path,
        &["rev-parse", "--show-toplevel"],
        30_000,
        COMMAND_OUTPUT_LIMIT,
    )
    .await;
    if !git_root.ok {
        return json!({
            "repoName": repo_name,
            "ok": false,
            "status": "skipped",
            "message": "projectPath 不是 Git 仓库",
        });
    }

    // base ref 的确定与 code review scan 保持一致
    let base_ref = repo
        .base_ref
        .as_deref()
        .filter(|v| !v.trim().is_empty())
        .map(str::trim)
        .map(str::to_string)
        .unwrap_or_else(|| detect_default_base_ref(repo));
    let base_info = parse_base_ref(&base_ref);
    let local_branch = base_info.local_branch.clone();
    let remote_ref = format!("{}/{}", base_info.remote, base_info.remote_branch);
    let mut warnings = Vec::<String>::new();

    // 1. fetch 远端分支,更新 remote-tracking ref(diff 基线即从此读取)
    let fetch = git(
        &project_path,
        &[
            "fetch",
            base_info.remote.as_str(),
            base_info.remote_branch.as_str(),
        ],
        60_000,
        COMMAND_OUTPUT_LIMIT,
    )
    .await;
    if !fetch.ok {
        return json!({
            "repoName": repo_name,
            "ok": false,
            "status": "fetch_failed",
            "baseRef": base_info.base_ref,
            "remoteRef": remote_ref,
            "message": format!("fetch {remote_ref} 失败：{}", short_err(&fetch)),
            "warnings": warnings,
        });
    }

    // fetch 后 remote-tracking ref 的最新 commit
    let after = git(
        &project_path,
        &["rev-parse", "--short", &remote_ref],
        30_000,
        COMMAND_OUTPUT_LIMIT,
    )
    .await;
    let after_commit = if after.ok {
        after.stdout.trim().to_string()
    } else {
        String::new()
    };

    // 2. 检查本地 base 分支是否存在
    let local_ref = format!("refs/heads/{local_branch}");
    let verify_local = git(
        &project_path,
        &["rev-parse", "--verify", "--quiet", &local_ref],
        30_000,
        COMMAND_OUTPUT_LIMIT,
    )
    .await;
    if !verify_local.ok {
        // 本地分支不存在,只 fetch 不 reset(不主动创建本地分支)
        return json!({
            "repoName": repo_name,
            "ok": true,
            "status": "fetched_no_local",
            "baseRef": base_info.base_ref,
            "remoteRef": remote_ref,
            "localBranch": local_branch,
            "afterCommit": after_commit,
            "message": format!("本地分支 {local_branch} 不存在,已 fetch {remote_ref},未 reset"),
            "warnings": warnings,
        });
    }

    // 本地分支 reset 前的 commit
    let before = git(
        &project_path,
        &["rev-parse", "--short", &local_ref],
        30_000,
        COMMAND_OUTPUT_LIMIT,
    )
    .await;
    let before_commit = if before.ok {
        before.stdout.trim().to_string()
    } else {
        String::new()
    };

    // 当前 HEAD(判断是否 checkout 在 base 分支)
    let head = git(
        &project_path,
        &["rev-parse", "--abbrev-ref", "HEAD"],
        30_000,
        COMMAND_OUTPUT_LIMIT,
    )
    .await;
    let current_branch = if head.ok {
        head.stdout.trim().to_string()
    } else {
        String::new()
    };

    if current_branch == local_branch {
        // 当前 checkout 在 base 分支:reset --hard 前必须确认工作区干净
        let dirty = git(
            &project_path,
            &["status", "--porcelain"],
            30_000,
            COMMAND_OUTPUT_LIMIT,
        )
        .await;
        if dirty.ok && !dirty.stdout.trim().is_empty() {
            warnings.push(format!(
                "当前在 {local_branch} 但工作区有未提交改动,已跳过 reset 以免丢弃"
            ));
            return json!({
                "repoName": repo_name,
                "ok": true,
                "status": "dirty_skipped",
                "baseRef": base_info.base_ref,
                "remoteRef": remote_ref,
                "localBranch": local_branch,
                "currentBranch": current_branch,
                "beforeCommit": before_commit,
                "afterCommit": after_commit,
                "message": format!("工作区不干净,已 fetch {remote_ref} 但跳过 reset"),
                "warnings": warnings,
            });
        }
        let reset = git(
            &project_path,
            &["reset", "--hard", &remote_ref],
            60_000,
            COMMAND_OUTPUT_LIMIT,
        )
        .await;
        if !reset.ok {
            warnings.push(format!(
                "reset --hard {remote_ref} 失败：{}",
                short_err(&reset)
            ));
            return json!({
                "repoName": repo_name,
                "ok": false,
                "status": "reset_failed",
                "baseRef": base_info.base_ref,
                "remoteRef": remote_ref,
                "localBranch": local_branch,
                "currentBranch": current_branch,
                "beforeCommit": before_commit,
                "afterCommit": after_commit,
                "message": format!("reset 失败：{}", short_err(&reset)),
                "warnings": warnings,
            });
        }
        return json!({
            "repoName": repo_name,
            "ok": true,
            "status": "reset",
            "baseRef": base_info.base_ref,
            "remoteRef": remote_ref,
            "localBranch": local_branch,
            "currentBranch": current_branch,
            "beforeCommit": before_commit,
            "afterCommit": after_commit,
            "message": format!("{local_branch} 已 reset 到 {remote_ref}"),
            "warnings": warnings,
        });
    }

    // 当前不在 base 分支:用 git branch -f 把本地 base 分支指向远端(不动工作区)
    let branch_f = git(
        &project_path,
        &["branch", "-f", local_branch.as_str(), &remote_ref],
        30_000,
        COMMAND_OUTPUT_LIMIT,
    )
    .await;
    if !branch_f.ok {
        warnings.push(format!(
            "git branch -f {local_branch} {remote_ref} 失败：{}",
            short_err(&branch_f)
        ));
        return json!({
            "repoName": repo_name,
            "ok": false,
            "status": "update_ref_failed",
            "baseRef": base_info.base_ref,
            "remoteRef": remote_ref,
            "localBranch": local_branch,
            "currentBranch": current_branch,
            "beforeCommit": before_commit,
            "afterCommit": after_commit,
            "message": format!("更新本地分支指向失败：{}", short_err(&branch_f)),
            "warnings": warnings,
        });
    }
    json!({
        "repoName": repo_name,
        "ok": true,
        "status": "updated",
        "baseRef": base_info.base_ref,
        "remoteRef": remote_ref,
        "localBranch": local_branch,
        "currentBranch": current_branch,
        "beforeCommit": before_commit,
        "afterCommit": after_commit,
        "message": format!("{local_branch} 已更新到 {remote_ref}(当前 checkout 在 {current_branch},工作区未动)"),
        "warnings": warnings,
    })
}
