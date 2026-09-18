use super::*;

/// 读取 code-review.json 里聚合出的风险标签与库存风险标记。
pub(crate) async fn review_snapshot_drifts(req_dir: &Path) -> Vec<ReviewSnapshotDrift> {
    let Some(review) = read_json_if_exists(&req_dir.join(CODE_REVIEW_FILE)).await else {
        return Vec::new();
    };
    let drifts = review_snapshot_drifts_from_value(&review).await;
    if !drifts.is_empty() {
        if reviewed_incremental_covers_drifts(req_dir, &drifts).await {
            return Vec::new();
        }
        if review_artifact_newer_than_review_docs(req_dir, CODE_REVIEW_INCREMENTAL_FILE).await {
            let incremental_drifts = incremental_review_drifts(req_dir).await;
            if !incremental_drifts.is_empty() {
                return incremental_drifts;
            }
        }
        return drifts;
    }
    if review_artifact_newer_than_review_docs(req_dir, CODE_REVIEW_INCREMENTAL_FILE).await {
        return incremental_review_drifts(req_dir).await;
    }
    Vec::new()
}

pub(crate) async fn review_snapshot_drifts_for_incremental(
    req_dir: &Path,
) -> Vec<ReviewSnapshotDrift> {
    let Some(review) = read_json_if_exists(&req_dir.join(CODE_REVIEW_FILE)).await else {
        return Vec::new();
    };
    let drifts = review_snapshot_drifts_from_value(&review).await;
    if !drifts.is_empty() {
        return drifts;
    }
    review
        .get("previousReviewedSnapshot")
        .and_then(|v| v.get("staleRepos"))
        .and_then(Value::as_array)
        .map(|repos| {
            repos
                .iter()
                .filter_map(review_snapshot_drift_from_stale_repo_value)
                .collect()
        })
        .unwrap_or_default()
}

pub(crate) async fn review_snapshot_drifts_from_value(review: &Value) -> Vec<ReviewSnapshotDrift> {
    let Some(repos) = review.get("repos").and_then(Value::as_array) else {
        return Vec::new();
    };
    let mut drifts = Vec::new();
    for repo in repos {
        let repo_name = value_string(repo, "repoName").unwrap_or_else(|| "repo".to_string());
        let branch = value_string(repo, "branch").unwrap_or_default();
        let reviewed_target_ref = value_string(repo, "resolvedTargetRef").unwrap_or_default();
        let reviewed_target_commit = value_string(repo, "targetCommit").unwrap_or_default();
        let Some(project_path) = value_string(repo, "projectPath").map(PathBuf::from) else {
            continue;
        };
        if branch.trim().is_empty()
            || reviewed_target_commit.trim().is_empty()
            || !project_path.exists()
        {
            continue;
        }
        let (current_target_ref, _) = resolve_target_ref(&project_path, &branch).await;
        let current_target_commit = match resolve_commit(&project_path, &current_target_ref).await {
            Value::String(v) => v,
            _ => String::new(),
        };
        if !current_target_commit.is_empty() && current_target_commit != reviewed_target_commit {
            drifts.push(ReviewSnapshotDrift {
                repo_name,
                branch,
                project_path: Some(project_path),
                reviewed_target_ref,
                reviewed_target_commit,
                current_target_ref,
                current_target_commit,
            });
        }
    }
    drifts
}

pub(crate) async fn incremental_review_drifts(req_dir: &Path) -> Vec<ReviewSnapshotDrift> {
    let Some(review) = read_json_if_exists(&req_dir.join(CODE_REVIEW_INCREMENTAL_FILE)).await
    else {
        return Vec::new();
    };
    let Some(repos) = review.get("repos").and_then(Value::as_array) else {
        return Vec::new();
    };
    repos
        .iter()
        .filter_map(incremental_review_drift_from_repo)
        .collect()
}

pub(crate) fn incremental_review_drift_from_repo(repo: &Value) -> Option<ReviewSnapshotDrift> {
    let reviewed_target_commit =
        value_string(repo, "coverageFromCommit").or_else(|| value_string(repo, "baseCommit"))?;
    let current_target_commit =
        value_string(repo, "coverageToCommit").or_else(|| value_string(repo, "targetCommit"))?;
    if reviewed_target_commit == current_target_commit {
        return None;
    }
    Some(ReviewSnapshotDrift {
        repo_name: value_string(repo, "repoName").unwrap_or_else(|| "repo".to_string()),
        branch: value_string(repo, "branch").unwrap_or_default(),
        project_path: value_string(repo, "projectPath").map(PathBuf::from),
        reviewed_target_ref: value_string(repo, "reviewedTargetRef")
            .or_else(|| value_string(repo, "baseCommit"))
            .unwrap_or_default(),
        reviewed_target_commit,
        current_target_ref: value_string(repo, "currentTargetRef")
            .or_else(|| value_string(repo, "targetCommit"))
            .unwrap_or_default(),
        current_target_commit,
    })
}

pub(crate) fn review_snapshot_drift_from_stale_repo_value(
    repo: &Value,
) -> Option<ReviewSnapshotDrift> {
    let reviewed_target_commit = value_string(repo, "reviewedTargetCommit")?;
    let current_target_commit = value_string(repo, "currentTargetCommit")?;
    if reviewed_target_commit == current_target_commit {
        return None;
    }
    Some(ReviewSnapshotDrift {
        repo_name: value_string(repo, "repoName").unwrap_or_else(|| "repo".to_string()),
        branch: value_string(repo, "branch").unwrap_or_default(),
        project_path: value_string(repo, "projectPath").map(PathBuf::from),
        reviewed_target_ref: value_string(repo, "reviewedTargetRef").unwrap_or_default(),
        reviewed_target_commit,
        current_target_ref: value_string(repo, "currentTargetRef").unwrap_or_default(),
        current_target_commit,
    })
}

pub(crate) async fn reviewed_incremental_covers_drifts(
    req_dir: &Path,
    drifts: &[ReviewSnapshotDrift],
) -> bool {
    if drifts.is_empty()
        || review_artifact_newer_than_review_docs(req_dir, CODE_REVIEW_INCREMENTAL_FILE).await
    {
        return false;
    }
    let Some(review) = read_json_if_exists(&req_dir.join(CODE_REVIEW_INCREMENTAL_FILE)).await
    else {
        return false;
    };
    let Some(repos) = review.get("repos").and_then(Value::as_array) else {
        return false;
    };
    drifts.iter().all(|drift| {
        repos
            .iter()
            .any(|repo| incremental_repo_covers_drift(repo, drift))
    })
}

pub(crate) fn incremental_repo_covers_drift(repo: &Value, drift: &ReviewSnapshotDrift) -> bool {
    if repo
        .get("linearHistory")
        .and_then(Value::as_bool)
        .is_some_and(|v| !v)
    {
        return false;
    }
    incremental_review_drift_from_repo(repo)
        .map(|inc| {
            inc.repo_name == drift.repo_name
                && inc.branch == drift.branch
                && inc.reviewed_target_commit == drift.reviewed_target_commit
                && inc.current_target_commit == drift.current_target_commit
        })
        .unwrap_or(false)
}

pub(crate) async fn review_artifact_newer_than_review_docs(req_dir: &Path, artifact: &str) -> bool {
    let Some(artifact_updated_at) = file_modified_ms(&req_dir.join(artifact)).await else {
        return false;
    };
    let newest_review_doc = ["review.md", "code-review-ai.md"]
        .iter()
        .filter_map(|name| std::fs::metadata(req_dir.join(name)).ok())
        .filter_map(|meta| meta.modified().ok())
        .map(system_time_to_ms)
        .max()
        .unwrap_or(0);
    newest_review_doc <= 0 || artifact_updated_at > newest_review_doc
}

pub(crate) fn value_string(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_string)
}

pub(crate) fn review_snapshot_drift_json(drift: ReviewSnapshotDrift) -> Value {
    json!({
        "repoName": drift.repo_name,
        "branch": drift.branch,
        "projectPath": drift.project_path.map(|p| p.to_string_lossy().to_string()),
        "reviewedTargetRef": drift.reviewed_target_ref,
        "reviewedTargetCommit": drift.reviewed_target_commit,
        "currentTargetRef": drift.current_target_ref,
        "currentTargetCommit": drift.current_target_commit,
    })
}

#[cfg(test)]
pub(crate) fn review_snapshot_drift_from_repo_value(
    repo: &Value,
    current_target_ref: &str,
    current_target_commit: &str,
) -> Option<ReviewSnapshotDrift> {
    let reviewed_target_commit = value_string(repo, "targetCommit")?;
    let current_target_commit = current_target_commit.trim();
    if current_target_commit.is_empty() || current_target_commit == reviewed_target_commit {
        return None;
    }
    Some(ReviewSnapshotDrift {
        repo_name: value_string(repo, "repoName").unwrap_or_else(|| "repo".to_string()),
        branch: value_string(repo, "branch").unwrap_or_default(),
        project_path: value_string(repo, "projectPath").map(PathBuf::from),
        reviewed_target_ref: value_string(repo, "resolvedTargetRef").unwrap_or_default(),
        reviewed_target_commit,
        current_target_ref: current_target_ref.trim().to_string(),
        current_target_commit: current_target_commit.to_string(),
    })
}

/// 读取 code-review.json 里聚合出的风险标签与库存风险标记。
pub(crate) async fn code_review_risk_for(req_dir: &Path) -> (Vec<String>, bool) {
    let mut tags = Vec::<String>::new();
    let mut inventory_risk = false;
    if let Ok(raw) = fs::read_to_string(&req_dir.join(CODE_REVIEW_FILE)).await {
        if let Ok(value) = serde_json::from_str::<Value>(&raw) {
            if let Some(repos) = value.get("repos").and_then(Value::as_array) {
                for repo in repos {
                    if repo
                        .get("inventoryRisk")
                        .and_then(Value::as_bool)
                        .unwrap_or(false)
                    {
                        inventory_risk = true;
                    }
                    if let Some(repo_tags) = repo.get("riskTags").and_then(Value::as_array) {
                        for tag in repo_tags {
                            if let Some(s) = tag.as_str() {
                                if !tags.contains(&s.to_string()) {
                                    tags.push(s.to_string());
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    (tags, inventory_risk)
}

/// 判断 review 文档是否包含库存账本专项评估的证据。
/// 命中库存风险时，要求至少出现库存专项小节，或命中 2 个以上账本关键点。
pub(crate) fn review_has_inventory_evidence(raw: &str) -> bool {
    let lower = raw.to_lowercase();
    if lower.contains("库存账本")
        || lower.contains("库存专项")
        || lower.contains("库存风险评估")
        || lower.contains("库存评估")
    {
        return true;
    }
    let keywords = [
        "可用量",
        "超卖",
        "占用",
        "释放",
        "回库",
        "onhandqty",
        "allocatedqty",
        "redis",
    ];
    keywords.iter().filter(|k| lower.contains(**k)).count() >= 2
}

pub(crate) fn review_gate_section(raw: &str, heading_keywords: &[&str]) -> Option<String> {
    let mut start: Option<(usize, usize)> = None;
    let lines: Vec<&str> = raw.lines().collect();
    for (idx, line) in lines.iter().enumerate() {
        if let Some((level, text)) = parse_markdown_heading(line) {
            let normalized = text.to_lowercase();
            if heading_keywords
                .iter()
                .any(|keyword| normalized.contains(&keyword.to_lowercase()))
            {
                start = Some((idx, level));
                break;
            }
        }
    }
    let (start_idx, start_level) = start?;
    let mut end_idx = lines.len();
    for (idx, line) in lines.iter().enumerate().skip(start_idx + 1) {
        if let Some((level, _)) = parse_markdown_heading(line) {
            if level <= start_level {
                end_idx = idx;
                break;
            }
        }
    }
    Some(lines[start_idx + 1..end_idx].join("\n"))
}

pub(crate) fn review_section_is_empty(section: &str) -> bool {
    !section.lines().any(|line| {
        let clean = line
            .trim()
            .trim_start_matches(['-', '*', ' ', '\t'])
            .trim_start_matches("[ ]")
            .trim_start_matches("[x]")
            .trim_start_matches("[X]")
            .trim()
            .trim_matches('。')
            .trim_matches('.')
            .trim();
        if clean.is_empty() {
            return false;
        }
        let lower = clean.to_lowercase();
        !matches!(
            lower.as_str(),
            "无" | "none" | "n/a" | "na" | "暂无" | "-" | "无。"
        ) && !clean.starts_with("无，")
            && !clean.starts_with("无；")
            && !clean.starts_with("无 ")
    })
}
