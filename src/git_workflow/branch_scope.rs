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
