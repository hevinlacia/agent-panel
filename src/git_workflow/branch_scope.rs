use super::*;

/// Branch-scope file name for a registration round: round 1 is the original
/// `branches.json`; round n (n >= 2) is `branches-round-n.json`, created after
/// the previous round's branches were merged into the production branch and
/// sealed. Each round registers its own repo/branch set independently.
pub(crate) fn branch_scope_file_for_round(round: u32) -> String {
    if round <= 1 {
        BRANCH_SCOPE_FILE.to_string()
    } else {
        format!("branches-round-{round}.json")
    }
}

pub(crate) async fn read_branch_scope(req_dir: &Path) -> Result<Option<BranchScope>> {
    read_branch_scope_round(req_dir, 1).await
}

/// Read the branch scope of a specific registration round; returns None when
/// the round file does not exist or carries no usable repo entries.
pub(crate) async fn read_branch_scope_round(
    req_dir: &Path,
    round: u32,
) -> Result<Option<BranchScope>> {
    let Some(raw) = read_json_if_exists(&req_dir.join(branch_scope_file_for_round(round))).await
    else {
        return Ok(None);
    };
    let mut scope: BranchScope = serde_json::from_value(raw).unwrap_or_default();
    scope.round = round;
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

/// 判断单仓库单分支快照是否“真正无文件差异”：files 为空，且 diff/文件列表读取无失败、
/// 无截断。diff 命令失败（error 非空）或输出被截断时不判空，避免把“读不到 diff”误当成“没有差异”。
fn snapshot_has_no_file_diff(snapshot: &Value) -> bool {
    let files_empty = snapshot
        .get("files")
        .and_then(Value::as_array)
        .map(|files| files.is_empty())
        .unwrap_or(true);
    if !files_empty {
        return false;
    }
    if snapshot.get("error").map(Value::is_null) != Some(true) {
        return false;
    }
    if snapshot
        .get("diffTruncated")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return false;
    }
    // name-status/numstat 读取失败时 files 必为空，但差异可能真实存在，不能判空。
    snapshot
        .get("warnings")
        .and_then(Value::as_array)
        .map(|warnings| {
            !warnings.iter().any(|w| {
                w.as_str()
                    .map(|s| s.contains("文件列表读取失败") || s.contains("Diff 读取失败"))
                    .unwrap_or(false)
            })
        })
        .unwrap_or(true)
}

/// 生成代码差异后自动清理登记：某应用本轮登记的全部分支快照都无文件差异时，
/// 把该应用从本轮次登记文件（branches.json / branches-round-<n>.json）移除并原子写回
/// （刷新 updated_at、version 补齐到 2）。典型场景：需求分支已合入生产（三点 diff 为空）
/// 后登记残留。全部应用都无差异时不写文件——登记不允许清空（与 PUT 语义一致），
/// 只返回候选由调用方提示。返回 (被移除的 repoName 列表, 登记文件是否写回)。
pub(crate) async fn prune_no_diff_repos(
    req_dir: &Path,
    scope: &BranchScope,
    review: &Value,
) -> Result<(Vec<String>, bool)> {
    let snapshots = review.get("repos").and_then(Value::as_array);
    let Some(snapshots) = snapshots else {
        return Ok((Vec::new(), false));
    };
    let removed: Vec<String> = scope
        .repos
        .iter()
        .filter(|repo| {
            let branch_snapshots: Vec<&Value> = snapshots
                .iter()
                .filter(|s| {
                    s.get("repoName").and_then(Value::as_str) == Some(repo.repo_name.as_str())
                })
                .collect();
            // 至少有一个分支快照，且全部分支都无文件差异才移除整个应用。
            !branch_snapshots.is_empty()
                && branch_snapshots
                    .iter()
                    .all(|s| snapshot_has_no_file_diff(s))
        })
        .map(|repo| repo.repo_name.clone())
        .collect();
    if removed.is_empty() {
        return Ok((removed, false));
    }
    let remaining: Vec<BranchRepo> = scope
        .repos
        .iter()
        .filter(|repo| !removed.contains(&repo.repo_name))
        .cloned()
        .collect();
    if remaining.is_empty() {
        return Ok((removed, false));
    }
    let mut next = scope.clone();
    next.repos = remaining;
    next.updated_at = now_ms();
    if next.version < 2 {
        next.version = 2;
    }
    let file = branch_scope_file_for_round(scope.round.max(1));
    atomic_write_json(&req_dir.join(&file), &next).await?;
    Ok((removed, true))
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BranchRoundInfo {
    pub(crate) round: u32,
    pub(crate) file: String,
    pub(crate) updated_at: i64,
    pub(crate) repo_count: usize,
    pub(crate) branch_count: usize,
    /// True when a newer round exists: this round's branches were merged into
    /// the production branch and the file is sealed (registered once, never
    /// updated afterwards).
    pub(crate) sealed: bool,
}

/// List all branch-registration rounds found in the requirement directory
/// (`branches.json` + `branches-round-*.json`), ordered by round ascending.
/// Rounds whose file is missing/unreadable are skipped silently.
pub(crate) async fn list_branch_scope_rounds(req_dir: &Path) -> Result<Vec<BranchRoundInfo>> {
    let pattern = Regex::new(r"^branches-round-(\d+)\.json$")?;
    let mut rounds: Vec<u32> = Vec::new();
    if req_dir.join(BRANCH_SCOPE_FILE).is_file() {
        rounds.push(1);
    }
    let mut entries = tokio::fs::read_dir(req_dir).await?;
    while let Some(entry) = entries.next_entry().await? {
        let name = entry.file_name().to_string_lossy().to_string();
        if let Some(cap) = pattern.captures(&name) {
            let n: u32 = cap
                .get(1)
                .and_then(|m| m.as_str().parse().ok())
                .unwrap_or(0);
            if n >= 2 {
                rounds.push(n);
            }
        }
    }
    rounds.sort_unstable();
    rounds.dedup();
    let latest = rounds.last().copied();
    let mut out = Vec::new();
    for round in rounds {
        let Some(scope) = read_branch_scope_round(req_dir, round).await? else {
            continue;
        };
        let branch_count = scope.repos.iter().map(|r| r.branches.len()).sum();
        out.push(BranchRoundInfo {
            round,
            file: branch_scope_file_for_round(round),
            updated_at: scope.updated_at,
            repo_count: scope.repos.len(),
            branch_count,
            sealed: latest.is_some_and(|n| round < n),
        });
    }
    Ok(out)
}

/// Create the next branch-registration round file for a requirement: copy the
/// latest round's repo structure (repoName/role/path/baseRef kept) with all
/// branch lists cleared, so the new round starts blank for registering
/// post-production fix branches. Creating a round seals the previous one.
pub(crate) async fn create_branch_scope_round(
    req_dir: &Path,
) -> Result<(u32, PathBuf, BranchScope)> {
    let rounds = list_branch_scope_rounds(req_dir).await?;
    let latest = rounds
        .last()
        .map(|r| r.round)
        .ok_or_else(|| anyhow!("missing {BRANCH_SCOPE_FILE}; run req-branches-update first"))?;
    let next = latest + 1;
    let path = req_dir.join(branch_scope_file_for_round(next));
    if path.exists() {
        return Err(anyhow!(
            "{} already exists",
            branch_scope_file_for_round(next)
        ));
    }
    let mut scope = read_branch_scope_round(req_dir, latest)
        .await?
        .ok_or_else(|| anyhow!("failed to read round {latest} branch scope"))?;
    for repo in &mut scope.repos {
        repo.branches.clear();
    }
    scope.fallback = false;
    scope.round = next;
    scope.updated_at = now_ms();
    let mut doc = serde_json::to_value(&scope)?;
    if let Some(obj) = doc.as_object_mut() {
        obj.insert("round".to_string(), json!(next));
    }
    atomic_write_json(&path, &doc).await?;
    Ok((next, path, scope))
}
