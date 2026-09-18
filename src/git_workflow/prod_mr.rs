use super::*;

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

/// 判断目标分支是否属于生产分支：后端 master、前端 production。
/// issue 家族（线上问题/测试问题）的排查/复现代码只允许合入 test/UAT 环境分支，
/// 该函数用于在 merge 时拦截生产分支（包括 branches.json uat_target_branch 显式覆盖的场景）。
pub(crate) fn is_production_target_branch(repo: &BranchRepo, target_branch: &str) -> bool {
    if is_pda_client_repo(repo) {
        return false;
    }
    if is_frontend_repo(repo) {
        target_branch == "production"
    } else {
        target_branch == "master"
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
