use std::{
    collections::HashMap,
    env,
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::{anyhow, Result};
use regex::Regex;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::{process::Command, time::timeout};

use crate::*;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CodeReviewForm {
    pub(crate) req_id: String,
    #[serde(default)]
    pub(crate) base_ref: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SyncBaseForm {
    pub(crate) req_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProdMrForm {
    pub(crate) req_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MergeBranchForm {
    pub(crate) req_id: String,
    #[serde(default)]
    pub(crate) target: Option<String>,
    #[serde(default)]
    pub(crate) target_branch: Option<String>,
    #[serde(default)]
    pub(crate) repo_kind: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BranchScope {
    #[serde(default)]
    pub(crate) version: i64,
    #[serde(default)]
    pub(crate) updated_at: i64,
    #[serde(default)]
    pub(crate) repos: Vec<BranchRepo>,
    #[serde(default)]
    pub(crate) fallback: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BranchRepo {
    #[serde(default)]
    pub(crate) repo_name: String,
    #[serde(default)]
    pub(crate) branches: Vec<String>,
    #[serde(default)]
    pub(crate) role: Option<String>,
    #[serde(default, alias = "projectPath")]
    pub(crate) path: Option<String>,
    #[serde(default)]
    pub(crate) base_ref: Option<String>,
    #[serde(default)]
    pub(crate) test_target_branch: Option<String>,
    #[serde(default)]
    pub(crate) uat_target_branch: Option<String>,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CodeReviewFileStat {
    pub(crate) path: String,
    pub(crate) status: String,
    pub(crate) additions: i64,
    pub(crate) deletions: i64,
    pub(crate) risk_tags: Vec<String>,
}

#[derive(Debug)]
pub(crate) struct GitCommandResult {
    pub(crate) ok: bool,
    pub(crate) code: Option<i32>,
    pub(crate) command: String,
    pub(crate) stdout: String,
    pub(crate) stderr: String,
    pub(crate) output_truncated: bool,
    pub(crate) timed_out: bool,
}

#[derive(Debug)]
pub(crate) struct BaseRefInfo {
    pub(crate) base_ref: String,
    pub(crate) remote: String,
    pub(crate) remote_branch: String,
    pub(crate) local_branch: String,
}

pub(crate) async fn read_branch_scope(req_dir: &Path) -> Result<Option<BranchScope>> {
    let Some(raw) = read_json_if_exists(&req_dir.join(BRANCH_SCOPE_FILE)).await else {
        return Ok(None);
    };
    let mut scope: BranchScope = serde_json::from_value(raw).unwrap_or_default();
    scope.repos.retain(|repo| !repo.repo_name.trim().is_empty());
    for repo in &mut scope.repos {
        repo.repo_name = repo.repo_name.trim().to_string();
        repo.branches = repo
            .branches
            .iter()
            .map(|b| b.trim().to_string())
            .filter(|b| !b.is_empty())
            .collect();
    }
    if scope.repos.is_empty() {
        return Ok(None);
    }
    if scope.version <= 0 {
        scope.version = 1;
    }
    if scope.updated_at <= 0 {
        scope.updated_at = now_ms();
    }
    Ok(Some(scope))
}

pub(crate) async fn run_code_review_scan(
    req_dir: &Path,
    req_id: &str,
    scope: &BranchScope,
) -> Result<Value> {
    let previous_review = read_json_if_exists(&req_dir.join(CODE_REVIEW_FILE)).await;
    let previous_drifts = review_snapshot_drifts(req_dir).await;
    let mut repos = Vec::new();
    for repo in &scope.repos {
        let branches = if repo.branches.is_empty() {
            vec![String::new()]
        } else {
            repo.branches.clone()
        };
        for branch in branches {
            repos.push(scan_repo_branch(repo, &branch).await);
        }
    }
    let mut risk_tags = Vec::<String>::new();
    let mut inventory_risk = false;
    for repo in &repos {
        if let Some(tags) = repo.get("riskTags").and_then(Value::as_array) {
            for tag in tags {
                if let Some(s) = tag.as_str() {
                    if !risk_tags.contains(&s.to_string()) {
                        risk_tags.push(s.to_string());
                    }
                }
            }
        }
        if repo
            .get("inventoryRisk")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            inventory_risk = true;
        }
    }
    let previous_snapshot = previous_review
        .as_ref()
        .filter(|_| !previous_drifts.is_empty())
        .map(|value| {
            json!({
                "updatedAt": value.get("updatedAt").cloned().unwrap_or(Value::Null),
                "staleRepos": previous_drifts.iter().cloned().map(review_snapshot_drift_json).collect::<Vec<_>>(),
                "repos": value.get("repos").cloned().unwrap_or_else(|| json!([])),
                "note": "preserved because a full diff refresh replaced a stale reviewed snapshot; prefer incremental review when possible",
            })
        });
    let review = json!({
        "version": 1,
        "reqId": req_id,
        "updatedAt": now_ms(),
        "baseRef": "origin/master",
        "frontendBaseRef": "origin/production",
        "backendBaseRef": "origin/master",
        "sourceFallback": scope.fallback,
        "riskTags": risk_tags,
        "inventoryRisk": inventory_risk,
        "previousReviewedSnapshot": previous_snapshot,
        "repos": repos,
    });
    atomic_write_json(&req_dir.join(CODE_REVIEW_FILE), &review).await?;
    Ok(review)
}

pub(crate) async fn run_code_review_incremental_scan(
    req_dir: &Path,
    req_id: &str,
) -> Result<Value> {
    let drifts = review_snapshot_drifts_for_incremental(req_dir).await;
    if drifts.is_empty() {
        return Err(anyhow!(
            "当前 code-review.json 未发现 reviewed target commit 与当前 HEAD 的差异，无需生成增量审查包"
        ));
    }
    let mut repos = Vec::new();
    for drift in &drifts {
        repos.push(scan_incremental_review_drift(drift).await);
    }
    let mut risk_tags = Vec::<String>::new();
    let mut inventory_risk = false;
    for repo in &repos {
        if let Some(tags) = repo.get("riskTags").and_then(Value::as_array) {
            for tag in tags {
                if let Some(s) = tag.as_str() {
                    if !risk_tags.contains(&s.to_string()) {
                        risk_tags.push(s.to_string());
                    }
                }
            }
        }
        if repo
            .get("inventoryRisk")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            inventory_risk = true;
        }
    }
    let review = json!({
        "version": 1,
        "reqId": req_id,
        "updatedAt": now_ms(),
        "mode": "incremental",
        "sourceSnapshot": CODE_REVIEW_FILE,
        "baseDescription": "reviewed targetCommit from the last full code-review snapshot",
        "targetDescription": "current requirement branch HEAD",
        "riskTags": risk_tags,
        "inventoryRisk": inventory_risk,
        "repos": repos,
    });
    atomic_write_json(&req_dir.join(CODE_REVIEW_INCREMENTAL_FILE), &review).await?;
    Ok(review)
}

pub(crate) async fn scan_incremental_review_drift(drift: &ReviewSnapshotDrift) -> Value {
    let mut warnings = Vec::<String>::new();
    let Some(project_path) = drift.project_path.as_ref() else {
        return json!({
            "repoName": drift.repo_name,
            "branch": drift.branch,
            "mode": "incremental",
            "baseCommit": drift.reviewed_target_commit,
            "targetCommit": drift.current_target_commit,
            "files": Vec::<CodeReviewFileStat>::new(),
            "additions": 0,
            "deletions": 0,
            "riskTags": Vec::<String>::new(),
            "inventoryRisk": false,
            "linearHistory": false,
            "diff": "",
            "diffTruncated": false,
            "warnings": ["缺少 projectPath，无法生成增量 diff"],
            "error": "missing projectPath",
        });
    };
    let ancestor = git(
        project_path,
        &[
            "merge-base",
            "--is-ancestor",
            &drift.reviewed_target_commit,
            &drift.current_target_commit,
        ],
        30_000,
        COMMAND_OUTPUT_LIMIT,
    )
    .await;
    let linear_history = ancestor.ok;
    if !linear_history {
        warnings.push("上次审查 commit 不是当前 HEAD 的祖先，分支可能 rebase/force-push；为安全起见建议重新做全量审查".into());
    }
    let commit_range = format!(
        "{}..{}",
        drift.reviewed_target_commit, drift.current_target_commit
    );
    let commits = git(
        project_path,
        &[
            "log",
            "--oneline",
            "--decorate=short",
            "--max-count=80",
            &commit_range,
        ],
        30_000,
        COMMAND_OUTPUT_LIMIT,
    )
    .await;
    if !commits.ok {
        warnings.push(format!("增量提交列表读取失败：{}", short_err(&commits)));
    }
    let name_status = git(
        project_path,
        &[
            "diff",
            "--name-status",
            "--find-renames",
            &drift.reviewed_target_commit,
            &drift.current_target_commit,
            "--",
        ],
        30_000,
        COMMAND_OUTPUT_LIMIT,
    )
    .await;
    if !name_status.ok {
        warnings.push(format!("增量文件列表读取失败：{}", short_err(&name_status)));
    }
    let numstat = git(
        project_path,
        &[
            "diff",
            "--numstat",
            "--find-renames",
            &drift.reviewed_target_commit,
            &drift.current_target_commit,
            "--",
        ],
        30_000,
        COMMAND_OUTPUT_LIMIT,
    )
    .await;
    if !numstat.ok {
        warnings.push(format!("增量增删行统计读取失败：{}", short_err(&numstat)));
    }
    let diff = git(
        project_path,
        &[
            "diff",
            "--no-ext-diff",
            "--no-color",
            "--find-renames",
            "--unified=80",
            &drift.reviewed_target_commit,
            &drift.current_target_commit,
            "--",
        ],
        60_000,
        DIFF_OUTPUT_LIMIT,
    )
    .await;
    if !diff.ok {
        warnings.push(format!("增量 Diff 读取失败：{}", short_err(&diff)));
    }
    let files = merge_file_stats(&name_status.stdout, &numstat.stdout);
    let additions: i64 = files.iter().map(|f| f.additions).sum();
    let deletions: i64 = files.iter().map(|f| f.deletions).sum();
    let risk_tags = aggregate_risk_tags(&files);
    let inventory_risk = risk_tags.iter().any(|t| t == "库存");
    json!({
        "repoName": drift.repo_name,
        "projectPath": project_path.to_string_lossy(),
        "branch": drift.branch,
        "mode": "incremental",
        "reviewedTargetRef": drift.reviewed_target_ref,
        "currentTargetRef": drift.current_target_ref,
        "baseCommit": drift.reviewed_target_commit,
        "targetCommit": drift.current_target_commit,
        "coverageFromCommit": drift.reviewed_target_commit,
        "coverageToCommit": drift.current_target_commit,
        "linearHistory": linear_history,
        "commits": if commits.ok { commits.stdout.lines().filter(|l| !l.trim().is_empty()).map(str::to_string).collect::<Vec<_>>() } else { Vec::<String>::new() },
        "files": files,
        "additions": additions,
        "deletions": deletions,
        "riskTags": risk_tags,
        "inventoryRisk": inventory_risk,
        "diff": if diff.ok { diff.stdout.clone() } else { String::new() },
        "diffTruncated": diff.output_truncated,
        "warnings": warnings,
        "error": if diff.ok { Value::Null } else { Value::String(short_err(&diff)) },
    })
}

pub(crate) async fn run_master_diff_scan(
    req_id: &str,
    scope: &BranchScope,
    base_ref: &str,
) -> Result<Value> {
    let mut repos = Vec::new();
    for repo in &scope.repos {
        let branches = if repo.branches.is_empty() {
            vec![String::new()]
        } else {
            repo.branches.clone()
        };
        let repo_base = if repo.role.as_deref() == Some("前端")
            || repo.path.as_deref().unwrap_or("").contains("/frontend/")
        {
            "origin/production"
        } else {
            base_ref
        };
        for branch in branches {
            repos.push(scan_repo_branch_with_base(repo, &branch, Some(repo_base)).await);
        }
    }
    Ok(json!({
        "version": 1,
        "reqId": req_id,
        "updatedAt": now_ms(),
        "baseRef": base_ref,
        "frontendBaseRef": "origin/production",
        "backendBaseRef": "origin/master",
        "sourceFallback": scope.fallback,
        "repos": repos,
    }))
}

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

#[derive(Debug, Clone)]
struct ProdDiffStat {
    files: i64,
    additions: i64,
    deletions: i64,
    no_diff: bool,
}

/// 计算需求分支相较于生产分支(target)的差异统计。
/// fetch 远端 target 后用三点 diff 比对;返回 None 表示无法判断(分支缺失/命令失败)。
async fn compute_prod_diff_stat(
    project_path: &Path,
    target_branch: &str,
    source_branch: &str,
) -> Option<ProdDiffStat> {
    // fetch 远端生产分支,确保 diff 基线最新
    let _ = git(
        project_path,
        &["fetch", "origin", target_branch],
        60_000,
        COMMAND_OUTPUT_LIMIT,
    )
    .await;
    let base_ref = format!("origin/{target_branch}");
    // 解析需求分支 ref:先本地,再 origin/<source>
    let local_ref = format!("{source_branch}^{{commit}}");
    let local = git(
        project_path,
        &["rev-parse", "--verify", &local_ref],
        30_000,
        COMMAND_OUTPUT_LIMIT,
    )
    .await;
    let source_ref = if local.ok {
        source_branch.to_string()
    } else {
        let remote = format!("origin/{source_branch}");
        let remote_ref = format!("{remote}^{{commit}}");
        let r = git(
            project_path,
            &["rev-parse", "--verify", &remote_ref],
            30_000,
            COMMAND_OUTPUT_LIMIT,
        )
        .await;
        if r.ok {
            remote
        } else {
            return None;
        }
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
    Some(ProdDiffStat {
        files,
        additions,
        deletions,
        no_diff: files == 0,
    })
}

pub(crate) async fn generate_prod_mrs(
    req: &Requirement,
    scope: &BranchScope,
) -> Result<Vec<Value>> {
    let token = gitlab_api_token()?;
    let api_base = env::var("GITLAB_API_URL").unwrap_or_else(|_| DEFAULT_GITLAB_API_URL.into());
    let client = Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .context("build GitLab API client")?;
    let mut results = Vec::new();

    for repo in &scope.repos {
        let target_branch = detect_prod_target_branch(repo);
        let branches = if repo.branches.is_empty() {
            vec![String::new()]
        } else {
            repo.branches.clone()
        };
        let Some(project_path) =
            resolve_code_review_project_path(repo.path.as_deref(), &repo.repo_name)
        else {
            for branch in branches {
                results.push(prod_mr_result(
                    repo,
                    &branch,
                    &target_branch,
                    "failed",
                    None,
                    Some("branches.json 缺少 path"),
                    None,
                ));
            }
            continue;
        };
        if !project_path.exists() {
            for branch in branches {
                results.push(prod_mr_result(
                    repo,
                    &branch,
                    &target_branch,
                    "failed",
                    None,
                    Some(&format!(
                        "仓库路径不存在：{}",
                        project_path.to_string_lossy()
                    )),
                    None,
                ));
            }
            continue;
        }
        let remote = git(
            &project_path,
            &["config", "--get", "remote.origin.url"],
            30_000,
            COMMAND_OUTPUT_LIMIT,
        )
        .await;
        let project_namespace = if remote.ok {
            gitlab_project_path_from_remote(remote.stdout.trim())
        } else {
            None
        };
        let Some(project_namespace) = project_namespace else {
            for branch in branches {
                results.push(prod_mr_result(
                    repo,
                    &branch,
                    &target_branch,
                    "failed",
                    None,
                    Some(&format!("无法解析 GitLab 项目路径：{}", short_err(&remote))),
                    Some(project_path.to_string_lossy().as_ref()),
                ));
            }
            continue;
        };
        for branch in branches {
            let branch_trim = branch.trim();
            // 先计算需求分支相较于生产分支的差异,无差异则跳过 MR 生成
            let stat = if branch_trim.is_empty() {
                None
            } else {
                compute_prod_diff_stat(&project_path, &target_branch, branch_trim).await
            };
            if let Some(s) = &stat {
                if s.no_diff {
                    let mut v = prod_mr_result(
                        repo,
                        branch_trim,
                        &target_branch,
                        "no_diff",
                        None,
                        Some(&format!("相较于生产分支 {target_branch} 无差异,未生成 MR")),
                        Some(project_path.to_string_lossy().as_ref()),
                    );
                    if let Some(obj) = v.as_object_mut() {
                        obj.insert("diffFiles".into(), json!(s.files));
                        obj.insert("diffAdditions".into(), json!(s.additions));
                        obj.insert("diffDeletions".into(), json!(s.deletions));
                    }
                    results.push(v);
                    continue;
                }
            }
            let mut v = create_or_reuse_gitlab_mr(
                &client,
                &api_base,
                &token,
                req,
                repo,
                &project_namespace,
                &project_path,
                &branch,
                &target_branch,
            )
            .await;
            if let (Some(s), Some(obj)) = (&stat, v.as_object_mut()) {
                obj.insert("diffFiles".into(), json!(s.files));
                obj.insert("diffAdditions".into(), json!(s.additions));
                obj.insert("diffDeletions".into(), json!(s.deletions));
            }
            results.push(v);
        }
    }
    Ok(results)
}

pub(crate) async fn create_or_reuse_gitlab_mr(
    client: &Client,
    api_base: &str,
    token: &str,
    req: &Requirement,
    repo: &BranchRepo,
    project_namespace: &str,
    project_path: &Path,
    source_branch: &str,
    target_branch: &str,
) -> Value {
    let source_branch = source_branch.trim();
    if source_branch.is_empty() {
        return prod_mr_result(
            repo,
            "(未指定分支)",
            target_branch,
            "skipped",
            None,
            Some("branches.json 缺少需求分支"),
            Some(project_path.to_string_lossy().as_ref()),
        );
    }
    let project_key = percent_encode(project_namespace);
    let url = format!(
        "{}/projects/{}/merge_requests",
        api_base.trim_end_matches('/'),
        project_key
    );
    match find_existing_gitlab_mr(client, token, &url, source_branch, target_branch).await {
        Ok(Some(mr)) => {
            return prod_mr_result(
                repo,
                source_branch,
                target_branch,
                "reused",
                Some(&mr),
                None,
                Some(project_path.to_string_lossy().as_ref()),
            );
        }
        Ok(None) => {}
        Err(err) => {
            return prod_mr_result(
                repo,
                source_branch,
                target_branch,
                "failed",
                None,
                Some(&err),
                Some(project_path.to_string_lossy().as_ref()),
            );
        }
    }

    let title = format!(
        "{} 生产发布：{} {} -> {}",
        req.id, repo.repo_name, source_branch, target_branch
    );
    let description = format!(
        "Agent Panel 自动创建生产 MR。\n\n- Req: `{}` {}\n- Repo: `{}`\n- Source: `{}`\n- Target: `{}`\n\n请组长审批后合入生产分支。",
        req.id, req.title, repo.repo_name, source_branch, target_branch
    );
    let form = [
        ("source_branch", source_branch),
        ("target_branch", target_branch),
        ("title", title.as_str()),
        ("description", description.as_str()),
        ("remove_source_branch", "false"),
    ];
    let response = client
        .post(&url)
        .header("PRIVATE-TOKEN", token)
        .form(&form)
        .send()
        .await;
    let response = match response {
        Ok(response) => response,
        Err(err) => {
            return prod_mr_result(
                repo,
                source_branch,
                target_branch,
                "failed",
                None,
                Some(&format!("GitLab API request failed: {err}")),
                Some(project_path.to_string_lossy().as_ref()),
            );
        }
    };
    let status = response.status();
    let text = response.text().await.unwrap_or_default();
    if !status.is_success() {
        if let Ok(Some(mr)) =
            find_existing_gitlab_mr(client, token, &url, source_branch, target_branch).await
        {
            return prod_mr_result(
                repo,
                source_branch,
                target_branch,
                "reused",
                Some(&mr),
                Some("创建返回非成功状态，但已找到可复用 open MR"),
                Some(project_path.to_string_lossy().as_ref()),
            );
        }
        return prod_mr_result(
            repo,
            source_branch,
            target_branch,
            "failed",
            None,
            Some(&format!(
                "GitLab API HTTP {}: {}",
                status.as_u16(),
                compact_http_body(&text)
            )),
            Some(project_path.to_string_lossy().as_ref()),
        );
    }
    let mr: Value =
        serde_json::from_str(&text).unwrap_or_else(|_| json!({ "raw": compact_http_body(&text) }));
    prod_mr_result(
        repo,
        source_branch,
        target_branch,
        "created",
        Some(&mr),
        None,
        Some(project_path.to_string_lossy().as_ref()),
    )
}

pub(crate) async fn find_existing_gitlab_mr(
    client: &Client,
    token: &str,
    url: &str,
    source_branch: &str,
    target_branch: &str,
) -> std::result::Result<Option<Value>, String> {
    let response = client
        .get(url)
        .header("PRIVATE-TOKEN", token)
        .query(&[
            ("state", "opened"),
            ("source_branch", source_branch),
            ("target_branch", target_branch),
            ("per_page", "20"),
        ])
        .send()
        .await
        .map_err(|err| format!("GitLab API request failed: {err}"))?;
    let status = response.status();
    let text = response.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(format!(
            "GitLab API HTTP {}: {}",
            status.as_u16(),
            compact_http_body(&text)
        ));
    }
    let value: Value = serde_json::from_str(&text)
        .map_err(|err| format!("GitLab API response is not JSON: {err}"))?;
    Ok(value
        .as_array()
        .and_then(|arr| arr.iter().find(|item| item.is_object()).cloned()))
}

pub(crate) fn prod_mr_result(
    repo: &BranchRepo,
    source_branch: &str,
    target_branch: &str,
    status: &str,
    mr: Option<&Value>,
    error: Option<&str>,
    project_path: Option<&str>,
) -> Value {
    json!({
        "repoName": repo.repo_name,
        "role": repo.role,
        "projectPath": project_path,
        "sourceBranch": source_branch,
        "targetBranch": target_branch,
        "status": status,
        "iid": mr.and_then(|m| m.get("iid")).cloned().unwrap_or(Value::Null),
        "webUrl": mr.and_then(|m| m.get("web_url")).cloned().unwrap_or(Value::Null),
        "title": mr.and_then(|m| m.get("title")).cloned().unwrap_or(Value::Null),
        "error": error,
    })
}

pub(crate) fn detect_prod_target_branch(repo: &BranchRepo) -> String {
    let role = repo.role.as_deref().unwrap_or_default();
    let path = repo.path.as_deref().unwrap_or_default();
    if role.contains("前端") || path.contains("/frontend/") || path.contains("\\frontend\\") {
        "production".into()
    } else {
        "master".into()
    }
}

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
    } else if branch == "master" || branch.starts_with("UAT-") {
        Ok("uat".to_string())
    } else {
        Err(ApiError::bad_request(
            "targetBranch must be test, master, or UAT-*",
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
        "测试中" if kind == "backend" => options.iter().find(|v| v.starts_with("UAT-")).cloned(),
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

pub(crate) async fn merge_requirement_branches(
    scope: &BranchScope,
    request: &MergeRequest,
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
            results.push(merge_repo_branch(repo, &branch, request).await);
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
        target_branch == "test" || target_branch.starts_with("UAT-")
    }
}

pub(crate) async fn detect_latest_uat_branch(project_path: &Path) -> Option<String> {
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

pub(crate) fn gitlab_api_token() -> Result<String> {
    let token = env::var("GITLAB_TOKEN")
        .or_else(|_| env::var("GL_TOKEN"))
        .or_else(|_| read_agent_panel_env_var("GITLAB_TOKEN"))
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .ok_or_else(|| anyhow!("missing GitLab token: set GITLAB_TOKEN / GL_TOKEN, or create .env.agent with GITLAB_TOKEN"))?;
    Ok(token)
}

pub(crate) fn read_agent_panel_env_var(key: &str) -> std::result::Result<String, env::VarError> {
    let path = env::current_dir()
        .map(|cwd| cwd.join(".env.agent"))
        .unwrap_or_else(|_| PathBuf::from(".env.agent"));
    let text = std::fs::read_to_string(path).map_err(|_| env::VarError::NotPresent)?;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let trimmed = trimmed
            .strip_prefix("export ")
            .unwrap_or(trimmed)
            .trim_start();
        let Some((name, value)) = trimmed.split_once('=') else {
            continue;
        };
        if name.trim() == key {
            return Ok(value
                .trim()
                .trim_matches('"')
                .trim_matches('\'')
                .to_string());
        }
    }
    Err(env::VarError::NotPresent)
}

pub(crate) fn gitlab_project_path_from_remote(remote: &str) -> Option<String> {
    let mut value = remote.trim().trim_end_matches(".git").to_string();
    if value.is_empty() {
        return None;
    }
    if let Some(rest) = value.strip_prefix("ssh://") {
        return rest
            .split_once('/')
            .map(|(_, path)| path.trim_matches('/').to_string())
            .filter(|path| !path.is_empty());
    }
    if let Some(rest) = value
        .strip_prefix("http://")
        .or_else(|| value.strip_prefix("https://"))
    {
        return rest
            .split_once('/')
            .map(|(_, path)| path.trim_matches('/').to_string())
            .filter(|path| !path.is_empty());
    }
    if let Some((prefix, path)) = value.split_once(':') {
        if prefix.contains('@') {
            let path = path.trim_matches('/').to_string();
            if !path.is_empty() {
                return Some(path);
            }
        }
    }
    if value.starts_with('/') {
        value = value.trim_matches('/').to_string();
    }
    (!value.is_empty()).then_some(value)
}

pub(crate) fn percent_encode(value: &str) -> String {
    let mut out = String::new();
    for b in value.as_bytes() {
        match *b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*b as char)
            }
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

pub(crate) fn compact_http_body(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(600)
        .collect()
}

pub(crate) async fn scan_repo_branch(repo: &BranchRepo, branch: &str) -> Value {
    scan_repo_branch_with_base(repo, branch, None).await
}

pub(crate) async fn scan_repo_branch_with_base(
    repo: &BranchRepo,
    branch: &str,
    forced_base_ref: Option<&str>,
) -> Value {
    let mut warnings = Vec::<String>::new();
    let base_ref = forced_base_ref
        .filter(|v| !v.trim().is_empty())
        .map(str::trim)
        .map(str::to_string)
        .or_else(|| {
            repo.base_ref
                .as_deref()
                .filter(|v| !v.trim().is_empty())
                .map(str::trim)
                .map(str::to_string)
        })
        .unwrap_or_else(|| detect_default_base_ref(repo));
    let base_info = parse_base_ref(&base_ref);
    let branch = branch.trim();
    let project_path = resolve_code_review_project_path(repo.path.as_deref(), &repo.repo_name);
    let Some(project_path) = project_path else {
        return empty_repo_snapshot(
            repo,
            branch,
            &base_info,
            warnings,
            "branches.json 缺少 path",
        );
    };
    if !project_path.exists() {
        return empty_repo_snapshot(
            repo,
            branch,
            &base_info,
            warnings,
            &format!("仓库路径不存在：{}", project_path.to_string_lossy()),
        );
    }
    if branch.is_empty() {
        return empty_repo_snapshot(
            repo,
            "(未指定分支)",
            &base_info,
            warnings,
            "branches.json 缺少需求分支",
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
        return empty_repo_snapshot(
            repo,
            branch,
            &base_info,
            warnings,
            "projectPath 不是 Git 仓库",
        );
    }

    let current_branch = git(
        &project_path,
        &["rev-parse", "--abbrev-ref", "HEAD"],
        30_000,
        COMMAND_OUTPUT_LIMIT,
    )
    .await;
    let dirty_state = git(
        &project_path,
        &["status", "--porcelain"],
        30_000,
        COMMAND_OUTPUT_LIMIT,
    )
    .await;
    let (target_ref, target_warning) = resolve_target_ref(&project_path, branch).await;
    if let Some(warning) = target_warning {
        warnings.push(warning);
    }
    // fetch 远端需求分支,确保 origin/<branch> 作为回退 ref 时是最新的
    // (本地分支存在时扫描优先用本地分支,此 fetch 无副作用)
    let _ = git(
        &project_path,
        &["fetch", "origin", branch],
        60_000,
        COMMAND_OUTPUT_LIMIT,
    )
    .await;

    let commit_range = format!("{}..{}", base_info.base_ref, target_ref);
    let diff_range = format!("{}...{}", base_info.base_ref, target_ref);
    let commits = git(
        &project_path,
        &[
            "log",
            "--oneline",
            "--decorate=short",
            "--max-count=80",
            &commit_range,
        ],
        30_000,
        COMMAND_OUTPUT_LIMIT,
    )
    .await;
    if !commits.ok {
        warnings.push(format!("提交列表读取失败：{}", short_err(&commits)));
    }
    let name_status = git(
        &project_path,
        &["diff", "--name-status", "--find-renames", &diff_range, "--"],
        30_000,
        COMMAND_OUTPUT_LIMIT,
    )
    .await;
    if !name_status.ok {
        warnings.push(format!("文件列表读取失败：{}", short_err(&name_status)));
    }
    let numstat = git(
        &project_path,
        &["diff", "--numstat", "--find-renames", &diff_range, "--"],
        30_000,
        COMMAND_OUTPUT_LIMIT,
    )
    .await;
    if !numstat.ok {
        warnings.push(format!("增删行统计读取失败：{}", short_err(&numstat)));
    }
    let diff = git(
        &project_path,
        &[
            "diff",
            "--no-ext-diff",
            "--no-color",
            "--find-renames",
            "--unified=80",
            &diff_range,
            "--",
        ],
        60_000,
        DIFF_OUTPUT_LIMIT,
    )
    .await;
    if !diff.ok {
        warnings.push(format!("Diff 读取失败：{}", short_err(&diff)));
    }

    let files = merge_file_stats(&name_status.stdout, &numstat.stdout);
    let additions: i64 = files.iter().map(|f| f.additions).sum();
    let deletions: i64 = files.iter().map(|f| f.deletions).sum();
    let risk_tags = aggregate_risk_tags(&files);
    let inventory_risk = risk_tags.iter().any(|t| t == "库存");
    json!({
        "repoName": repo.repo_name,
        "projectPath": project_path.to_string_lossy(),
        "branch": branch,
        "resolvedTargetRef": target_ref,
        "targetCommit": resolve_commit(&project_path, &target_ref).await,
        "baseRef": base_info.base_ref,
        "baseCommit": resolve_commit(&project_path, &base_info.base_ref).await,
        "currentBranch": current_branch.ok.then(|| current_branch.stdout.trim().to_string()),
        "dirty": dirty_state.ok && !dirty_state.stdout.trim().is_empty(),
        "baseUpdate": read_only_base_update(&base_info),
        "commits": if commits.ok { commits.stdout.lines().filter(|l| !l.trim().is_empty()).map(str::to_string).collect::<Vec<_>>() } else { Vec::<String>::new() },
        "files": files,
        "additions": additions,
        "deletions": deletions,
        "riskTags": risk_tags,
        "inventoryRisk": inventory_risk,
        "diff": if diff.ok { diff.stdout.clone() } else { String::new() },
        "diffTruncated": diff.output_truncated,
        "warnings": warnings,
        "error": if diff.ok || additions + deletions > 0 { Value::Null } else { Value::String(short_err(&diff)) },
    })
}

pub(crate) fn empty_repo_snapshot(
    repo: &BranchRepo,
    branch: &str,
    base_info: &BaseRefInfo,
    warnings: Vec<String>,
    error: &str,
) -> Value {
    json!({
        "repoName": repo.repo_name,
        "projectPath": resolve_code_review_project_path(repo.path.as_deref(), &repo.repo_name).or_else(|| repo.path.as_ref().map(PathBuf::from)).map(|p| p.to_string_lossy().to_string()),
        "branch": branch,
        "resolvedTargetRef": branch,
        "targetCommit": Value::Null,
        "baseRef": base_info.base_ref,
        "baseCommit": Value::Null,
        "dirty": false,
        "baseUpdate": read_only_base_update(base_info),
        "commits": Vec::<String>::new(),
        "files": Vec::<CodeReviewFileStat>::new(),
        "additions": 0,
        "deletions": 0,
        "diff": "",
        "diffTruncated": false,
        "warnings": warnings,
        "error": error,
    })
}

pub(crate) fn detect_default_base_ref(repo: &BranchRepo) -> String {
    let role = repo.role.as_deref().unwrap_or_default();
    let path = repo.path.as_deref().unwrap_or_default();
    if role == "前端" || path.contains("/frontend/") {
        "origin/production".to_string()
    } else {
        "origin/master".to_string()
    }
}

pub(crate) fn parse_base_ref(input: &str) -> BaseRefInfo {
    let base_ref = if input.trim().is_empty() {
        "origin/master"
    } else {
        input.trim()
    }
    .to_string();
    if base_ref.contains('/') && !base_ref.starts_with("refs/") {
        let mut parts = base_ref.split('/');
        let remote = parts.next().unwrap_or("origin").to_string();
        let remote_branch = parts.collect::<Vec<_>>().join("/");
        let remote_branch = if remote_branch.is_empty() {
            "master".to_string()
        } else {
            remote_branch
        };
        BaseRefInfo {
            base_ref,
            remote,
            local_branch: remote_branch.clone(),
            remote_branch,
        }
    } else {
        BaseRefInfo {
            base_ref: base_ref.clone(),
            remote: "origin".to_string(),
            remote_branch: base_ref.clone(),
            local_branch: base_ref,
        }
    }
}

pub(crate) fn read_only_base_update(info: &BaseRefInfo) -> Value {
    json!({
        "ok": true,
        "remote": info.remote,
        "remoteBranch": info.remote_branch,
        "localBranch": info.local_branch,
        "steps": [{
            "label": "read local git refs",
            "command": "fetch/pull skipped by Rust panel read-only scan",
            "ok": true,
        }],
    })
}

pub(crate) async fn resolve_commit(repo_path: &Path, reference: &str) -> Value {
    let reference = reference.trim();
    if reference.is_empty() {
        return Value::Null;
    }
    let commit_ref = format!("{reference}^{{commit}}");
    let result = git(
        repo_path,
        &["rev-parse", "--verify", &commit_ref],
        30_000,
        COMMAND_OUTPUT_LIMIT,
    )
    .await;
    if result.ok {
        Value::String(result.stdout.trim().to_string())
    } else {
        Value::Null
    }
}

pub(crate) async fn resolve_target_ref(repo_path: &Path, branch: &str) -> (String, Option<String>) {
    let local_ref = format!("{}^{{commit}}", branch);
    let local = git(
        repo_path,
        &["rev-parse", "--verify", &local_ref],
        30_000,
        COMMAND_OUTPUT_LIMIT,
    )
    .await;
    if local.ok {
        return (branch.to_string(), None);
    }
    let remote_branch = format!("origin/{}", branch);
    let remote_ref = format!("{}^{{commit}}", remote_branch);
    let remote = git(
        repo_path,
        &["rev-parse", "--verify", &remote_ref],
        30_000,
        COMMAND_OUTPUT_LIMIT,
    )
    .await;
    if remote.ok {
        return (
            remote_branch.clone(),
            Some(format!("本地分支 {branch} 不存在，已使用 {remote_branch}")),
        );
    }
    (
        branch.to_string(),
        Some(format!("无法验证需求分支 {branch}，diff 可能失败")),
    )
}

pub(crate) fn resolve_code_review_project_path(
    project_path: Option<&str>,
    repo_name: &str,
) -> Option<PathBuf> {
    let raw = project_path?.trim();
    if raw.is_empty() {
        return None;
    }
    let expanded = if raw == "~" {
        home_dir().ok()?
    } else if let Some(rest) = raw.strip_prefix("~/") {
        home_dir().ok()?.join(rest)
    } else {
        PathBuf::from(raw)
    };
    let resolved = if expanded.is_absolute() {
        expanded
    } else {
        env::current_dir().ok()?.join(expanded)
    };
    if resolved.exists() {
        return Some(resolved);
    }
    let leaf = if repo_name.trim().is_empty() {
        resolved.file_name()?.to_string_lossy().to_string()
    } else {
        repo_name.trim().to_string()
    };
    let mut roots = Vec::new();
    if let Some(parent) = resolved.parent() {
        roots.push(parent.to_path_buf());
    }
    if let Ok(home) = home_dir() {
        roots.push(home.join("Developer/company/WMS"));
    }
    for root in roots {
        for area in ["backend", "frontend", "pda", "infra"] {
            let candidate = root.join(area).join(&leaf);
            if candidate.exists() {
                return Some(candidate);
            }
        }
    }
    Some(resolved)
}

pub(crate) fn merge_file_stats(
    name_status_out: &str,
    numstat_out: &str,
) -> Vec<CodeReviewFileStat> {
    let mut by_path: HashMap<String, CodeReviewFileStat> = HashMap::new();
    for line in name_status_out.lines().filter(|l| !l.trim().is_empty()) {
        let cols: Vec<&str> = line.split('\t').collect();
        let status = cols.first().copied().unwrap_or("M").to_string();
        let path = if cols.len() >= 3 && (status.starts_with('R') || status.starts_with('C')) {
            cols[2]
        } else {
            cols.get(1).copied().unwrap_or_default()
        };
        if !path.is_empty() {
            by_path.insert(
                path.to_string(),
                CodeReviewFileStat {
                    path: path.to_string(),
                    status,
                    additions: 0,
                    deletions: 0,
                    risk_tags: Vec::new(),
                },
            );
        }
    }
    for line in numstat_out.lines().filter(|l| !l.trim().is_empty()) {
        let cols: Vec<&str> = line.split('\t').collect();
        if cols.len() < 3 {
            continue;
        }
        let path = normalize_numstat_path(&cols[2..].join("\t"));
        let entry = by_path
            .entry(path.clone())
            .or_insert_with(|| CodeReviewFileStat {
                path: path.clone(),
                status: "M".to_string(),
                additions: 0,
                deletions: 0,
                risk_tags: Vec::new(),
            });
        entry.additions = cols[0].parse::<i64>().unwrap_or(0);
        entry.deletions = cols[1].parse::<i64>().unwrap_or(0);
    }
    let mut files: Vec<CodeReviewFileStat> = by_path
        .into_values()
        .map(|mut f| {
            f.risk_tags = classify_code_review_risk_tags(&f);
            f
        })
        .collect();
    files.sort_by(|a, b| a.path.cmp(&b.path));
    files
}

pub(crate) fn normalize_numstat_path(raw: &str) -> String {
    Regex::new(r"=>\s*(.*)$")
        .ok()
        .and_then(|re| {
            re.captures(raw)
                .and_then(|c| c.get(1).map(|m| m.as_str().to_string()))
        })
        .unwrap_or_else(|| raw.to_string())
        .replace(['{', '}'], "")
        .trim()
        .to_string()
}

pub(crate) fn classify_code_review_risk_tags(file: &CodeReviewFileStat) -> Vec<String> {
    let p = file.path.to_lowercase();
    let mut tags = Vec::new();
    if p.contains("/test/") || p.contains("src/test") {
        tags.push("测试".to_string());
    }
    if p.contains("controller") || p.contains("resource") || p.contains("/api/") {
        tags.push("API".to_string());
    }
    if p.contains("service") || p.contains("manager") {
        tags.push("Service".to_string());
    }
    if p.contains("mapper")
        || p.ends_with(".xml")
        || p.contains("dao")
        || p.ends_with(".sql")
        || p.ends_with("pom.xml")
    {
        tags.push("DB".to_string());
    }
    if p.contains("listener")
        || p.contains("consumer")
        || p.contains("kafka")
        || p.contains("rocket")
        || p.contains("rabbit")
        || p.contains("mq")
    {
        tags.push("MQ".to_string());
    }
    if p.contains("config")
        || p.ends_with(".yml")
        || p.ends_with(".yaml")
        || p.ends_with(".properties")
    {
        tags.push("配置".to_string());
    }
    // 库存高危风险：命中库存相关文件/表，门禁将强制要求库存账本专项评估
    let inventory_hints = [
        "inventorycache",
        "inventorychange",
        "inventoryallocation",
        "shipmentheaderservice",
        "shipmentdetailservice",
        "shipmentrollback",
        "backorder",
        "location_inventory",
        "shipment_alloc_request",
        "shipment_detail",
        "shipment_header",
        "inventoryprestatus",
        "onhandqty",
        "allocatedqty",
    ];
    if inventory_hints.iter().any(|h| p.contains(h)) {
        tags.push("库存".to_string());
    }
    if file.additions + file.deletions >= 500 {
        tags.push("大改动".to_string());
    }
    tags
}

pub(crate) fn aggregate_risk_tags(files: &[CodeReviewFileStat]) -> Vec<String> {
    let mut tags = Vec::<String>::new();
    for file in files {
        for tag in &file.risk_tags {
            if !tags.contains(tag) {
                tags.push(tag.clone());
            }
        }
    }
    tags
}

pub(crate) async fn git(
    cwd: &Path,
    args: &[&str],
    timeout_ms: u64,
    max_output: usize,
) -> GitCommandResult {
    let command = std::iter::once("git".to_string())
        .chain(args.iter().map(|a| shell_quote(a)))
        .collect::<Vec<_>>()
        .join(" ");
    let fut = Command::new("git").args(args).current_dir(cwd).output();
    match timeout(Duration::from_millis(timeout_ms), fut).await {
        Ok(Ok(output)) => {
            let (stdout, stdout_truncated) = limit_output(
                String::from_utf8_lossy(&output.stdout).to_string(),
                max_output,
            );
            let (stderr, stderr_truncated) = limit_output(
                String::from_utf8_lossy(&output.stderr).to_string(),
                max_output,
            );
            GitCommandResult {
                ok: output.status.success(),
                code: output.status.code(),
                command,
                stdout,
                stderr,
                output_truncated: stdout_truncated || stderr_truncated,
                timed_out: false,
            }
        }
        Ok(Err(err)) => GitCommandResult {
            ok: false,
            code: None,
            command,
            stdout: String::new(),
            stderr: err.to_string(),
            output_truncated: false,
            timed_out: false,
        },
        Err(_) => GitCommandResult {
            ok: false,
            code: None,
            command,
            stdout: String::new(),
            stderr: format!("timed out after {timeout_ms}ms"),
            output_truncated: false,
            timed_out: true,
        },
    }
}

pub(crate) fn limit_output(value: String, max: usize) -> (String, bool) {
    if value.len() <= max {
        return (value, false);
    }
    (value.chars().take(max).collect::<String>(), true)
}

pub(crate) fn compact(value: &str, max: usize) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.len() > max {
        Some(format!(
            "{}…",
            trimmed.chars().take(max).collect::<String>()
        ))
    } else {
        Some(trimmed.to_string())
    }
}

pub(crate) fn short_err(result: &GitCommandResult) -> String {
    compact(&result.stderr, 600)
        .or_else(|| compact(&result.stdout, 600))
        .unwrap_or_else(|| match result.code {
            Some(code) => format!("{} exited {code}", result.command),
            None if result.timed_out => format!("{} timed out", result.command),
            None => format!("{} failed", result.command),
        })
}
