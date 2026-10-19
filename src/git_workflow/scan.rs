use super::*;

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

    // fetch base 的远端分支,保证 origin/master 等远端跟踪引用反映最新远端状态
    // (离线等 fetch 失败不阻塞扫描);随后解析 base:优先远端跟踪引用,
    // 缺失时回退本地分支(与 resolve_target_ref 的回退链对称)。
    let base_fetch = git(
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
    let (base_used, base_warning) = resolve_base_ref(&project_path, &base_info).await;
    // fetch 失败但仍用远端跟踪引用时提示可能过期;回退本地分支的场景已有回退警告,不再叠加。
    if !base_fetch.ok && base_warning.is_none() {
        warnings.push(format!(
            "fetch {} {} 失败,基线 {} 可能不是最新远端状态",
            base_info.remote, base_info.remote_branch, base_used
        ));
    }
    if let Some(warning) = base_warning {
        warnings.push(warning);
    }

    let commit_range = format!("{}..{}", base_used, target_ref);
    let diff_range = format!("{}...{}", base_used, target_ref);
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
    let base_commit = resolve_commit(&project_path, &base_used).await;
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
        "baseRef": base_used,
        "baseCommit": base_commit,
        "currentBranch": current_branch.ok.then(|| current_branch.stdout.trim().to_string()),
        "dirty": dirty_state.ok && !dirty_state.stdout.trim().is_empty(),
        "baseUpdate": base_update_info(&base_info, &base_used),
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

/// base 解析:优先远端跟踪引用(如 origin/master,扫描前已 fetch 刷新),
/// 缺失时回退本地分支(如 master)。返回实际可用的 ref 与可选警告。
pub(crate) async fn resolve_base_ref(
    repo_path: &Path,
    base_info: &BaseRefInfo,
) -> (String, Option<String>) {
    if resolve_commit(repo_path, &base_info.base_ref)
        .await
        .is_string()
    {
        return (base_info.base_ref.clone(), None);
    }
    if base_info.local_branch != base_info.base_ref
        && resolve_commit(repo_path, &base_info.local_branch)
            .await
            .is_string()
    {
        return (
            base_info.local_branch.clone(),
            Some(format!(
                "远端引用 {} 不存在,已回退本地分支 {}",
                base_info.base_ref, base_info.local_branch
            )),
        );
    }
    (
        base_info.base_ref.clone(),
        Some(format!("无法验证基线 {},diff 可能失败", base_info.base_ref)),
    )
}

/// 成功扫描路径的 baseUpdate:已 fetch 远端分支并解析出实际使用的 base ref。
pub(crate) fn base_update_info(info: &BaseRefInfo, resolved_base_ref: &str) -> Value {
    json!({
        "ok": true,
        "remote": info.remote,
        "remoteBranch": info.remote_branch,
        "localBranch": info.local_branch,
        "resolvedBaseRef": resolved_base_ref,
        "steps": [{
            "label": "fetch remote branch then resolve base ref",
            "command": format!(
                "git fetch {} {} && git rev-parse --verify {}^{{commit}}",
                info.remote, info.remote_branch, resolved_base_ref
            ),
            "ok": true,
        }],
    })
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
