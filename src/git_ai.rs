use std::{
    env,
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};

use axum::{extract::State, Json};
use regex::Regex;
use serde_json::{json, Value};
use tokio::{
    fs,
    process::Command,
    time::{sleep, timeout},
};
use uuid::Uuid;

use crate::{
    atomic_write_json, compact, home_dir, limit_output, now_ms, read_json_if_exists, shell_quote,
    ApiError, ApiResult, AppState, GitCommandResult, COMMAND_OUTPUT_LIMIT,
};

pub(crate) async fn api_git_ai_health(State(state): State<AppState>) -> Json<Value> {
    let home = home_dir().unwrap_or_default();
    let store_path = state.data_dir.join("git-ai-suspects.json");
    let cli = read_git_ai_cli_health(&home).await;
    let pi_extension = read_pi_git_ai_extension_health(&home).await;
    Json(json!({
        "generatedAt": now_ms(),
        "storePath": store_path,
        "cli": cli,
        "piExtension": pi_extension,
    }))
}

async fn run_output(cmd: &str, args: &[&str], timeout_ms: u64) -> (Option<i32>, String, String) {
    let fut = Command::new(cmd).args(args).output();
    match timeout(Duration::from_millis(timeout_ms), fut).await {
        Ok(Ok(output)) => (
            output.status.code(),
            String::from_utf8_lossy(&output.stdout).to_string(),
            String::from_utf8_lossy(&output.stderr).to_string(),
        ),
        Ok(Err(err)) => (None, String::new(), err.to_string()),
        Err(_) => (None, String::new(), "timeout".into()),
    }
}

async fn find_git_ai_binary(home: &Path) -> Option<PathBuf> {
    if let Ok(path) = env::var("GIT_AI_BIN") {
        let p = PathBuf::from(path);
        if p.exists() {
            return Some(p);
        }
    }
    let (code, stdout, _) = run_output("bash", &["-lc", "command -v git-ai"], 2_000).await;
    let found = stdout.trim();
    if code == Some(0) && !found.is_empty() {
        let p = PathBuf::from(found);
        if p.exists() {
            return Some(p);
        }
    }
    let default = home.join(".git-ai/bin/git-ai");
    if default.exists() {
        Some(default)
    } else {
        None
    }
}

fn parse_trace2_socket(target: Option<&str>) -> Option<String> {
    let target = target?.trim();
    if target.is_empty() {
        return None;
    }
    let marker = "af_unix:stream:";
    if let Some(idx) = target.find(marker) {
        let socket = target[idx + marker.len()..].trim();
        if socket.is_empty() {
            None
        } else {
            Some(socket.to_string())
        }
    } else {
        Some(target.to_string())
    }
}

async fn read_text_safe(path: &Path) -> String {
    fs::read_to_string(path).await.unwrap_or_default()
}

async fn is_executable(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = fs::metadata(path).await {
            return meta.permissions().mode() & 0o111 != 0;
        }
    }
    #[cfg(not(unix))]
    {
        return path.exists();
    }
    false
}

async fn hook_health(path: Option<PathBuf>, kind: &str) -> Value {
    let Some(path) = path else {
        return json!({ "path": null, "exists": false, "mode": "missing", "recordsToAgentPanel": false, "executable": false });
    };
    let text = read_text_safe(&path).await;
    let exists = path.exists();
    let records = text.contains("record_git_ai_suspect") && text.contains("AGENT_PANEL_STORE");
    let mut mode = if exists && records {
        "record"
    } else if exists {
        "present"
    } else {
        "missing"
    };
    if kind == "pre-push"
        && text.contains("GIT_AI_PUSH_MODE")
        && text.contains("block")
        && !text.contains("record")
    {
        mode = "block";
    }
    if kind == "post-commit" && text.contains("NO_BLOCK") && !text.contains("GIT_AI_BLOCK") {
        mode = "block";
    }
    json!({
        "path": path,
        "exists": exists,
        "mode": mode,
        "recordsToAgentPanel": records,
        "executable": is_executable(&path).await,
    })
}

async fn read_git_ai_cli_health(home: &Path) -> Value {
    let binary = find_git_ai_binary(home).await;
    let (installed, version, daemon_ok, daemon_message) = if let Some(bin) = &binary {
        let (_, version_out, version_err) =
            run_output(bin.to_string_lossy().as_ref(), &["--version"], 3_000).await;
        let version = version_out.trim().to_string();
        let version = if version.is_empty() {
            version_err.trim().to_string()
        } else {
            version
        };
        let (_, bg_out, bg_err) =
            run_output(bin.to_string_lossy().as_ref(), &["bg", "status"], 4_000).await;
        let mut ok = false;
        let mut message = if bg_err.trim().is_empty() {
            bg_out.trim().to_string()
        } else {
            bg_err.trim().to_string()
        };
        if let Ok(parsed) = serde_json::from_str::<Value>(&bg_out) {
            ok = parsed.get("ok").and_then(Value::as_bool).unwrap_or(false)
                && parsed
                    .pointer("/data/last_error")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .is_empty();
            message = parsed
                .pointer("/data/last_error")
                .and_then(Value::as_str)
                .map(str::to_string)
                .unwrap_or_else(|| {
                    if ok {
                        "running".into()
                    } else {
                        "not running".into()
                    }
                });
        }
        (
            true,
            if version.is_empty() {
                None
            } else {
                Some(version)
            },
            ok,
            Some(message),
        )
    } else {
        (false, None, false, Some("git-ai binary missing".into()))
    };
    let (trace_code, trace_out, _) =
        run_output("git", &["config", "--global", "trace2.eventtarget"], 2_000).await;
    let trace2_target = if trace_code == Some(0) {
        Some(trace_out.trim().to_string()).filter(|s| !s.is_empty())
    } else {
        None
    };
    let trace2_socket = parse_trace2_socket(trace2_target.as_deref());
    let trace2_socket_exists = trace2_socket
        .as_ref()
        .map(|s| Path::new(s).exists())
        .unwrap_or(false);
    let (hooks_code, hooks_out, _) =
        run_output("git", &["config", "--global", "core.hooksPath"], 2_000).await;
    let hooks_path = if hooks_code == Some(0) {
        Some(hooks_out.trim().to_string()).filter(|s| !s.is_empty())
    } else {
        None
    };
    let hooks_dir = hooks_path.as_ref().map(PathBuf::from);
    let post_hook = hook_health(
        hooks_dir.as_ref().map(|p| p.join("post-commit")),
        "post-commit",
    )
    .await;
    let pre_hook = hook_health(hooks_dir.as_ref().map(|p| p.join("pre-push")), "pre-push").await;
    json!({
        "binaryPath": binary,
        "installed": installed,
        "version": version,
        "daemonOk": daemon_ok,
        "daemonMessage": daemon_message,
        "trace2Target": trace2_target,
        "trace2Socket": trace2_socket,
        "trace2SocketExists": trace2_socket_exists,
        "hooksPath": hooks_path,
        "postCommitHook": post_hook,
        "prePushHook": pre_hook,
    })
}

fn tracked_tools(text: &str) -> Vec<&'static str> {
    let mut out = Vec::new();
    if text.contains("edit") {
        out.push("edit");
    }
    if text.contains("write") {
        out.push("write");
    }
    if text.contains("tool === \"bash\"") || text.contains("bash") {
        out.push("bash");
    }
    out.sort_unstable();
    out.dedup();
    out
}

async fn read_pi_git_ai_extension_health(home: &Path) -> Value {
    let global = home.join(".pi/agent/extensions/git-ai.ts");
    let source = home.join("Developer/infra/ai-code-config/core/pi/agent/extensions/git-ai.ts");
    let text = read_text_safe(&global).await;
    let global_exists = global.exists();
    let source_exists = source.exists();
    let source_matches = if global_exists && source_exists {
        text == read_text_safe(&source).await
    } else {
        false
    };
    let bin_match = Regex::new(r#"const GIT_AI_BIN = process\.env\.GIT_AI_BIN \|\| \"([^\"]+)\""#)
        .ok()
        .and_then(|re| {
            re.captures(&text)
                .and_then(|c| c.get(1).map(|m| m.as_str().to_string()))
        });
    let binary_path = env::var("GIT_AI_BIN")
        .ok()
        .or(bin_match)
        .unwrap_or_else(|| {
            home.join(".git-ai/bin/git-ai")
                .to_string_lossy()
                .to_string()
        });
    let binary_exists = Path::new(&binary_path).exists();
    let registers_status =
        text.contains("ctx.ui.setStatus(\"git-ai\"") || text.contains("ctx.ui.setStatus('git-ai'");
    let tools = tracked_tools(&text);
    let mut problems = Vec::new();
    if !global_exists {
        problems.push("global extension missing");
    }
    if !binary_exists {
        problems.push("git-ai binary missing for extension");
    }
    if !registers_status {
        problems.push("no git-ai UI status registration");
    }
    if tools.is_empty() {
        problems.push("no tracked tools detected");
    }
    if !source_matches {
        problems.push("runtime extension differs from config source");
    }
    let status = if problems.is_empty() {
        "ok"
    } else if problems.iter().any(|p| p.contains("missing")) {
        "error"
    } else {
        "warn"
    };
    json!({
        "globalPath": global,
        "sourcePath": source,
        "globalExists": global_exists,
        "sourceExists": source_exists,
        "sourceMatchesGlobal": source_matches,
        "autoDiscoveryPath": true,
        "gitAiBinaryExistsForExtension": binary_exists,
        "registersStatus": registers_status,
        "tracksTools": tools,
        "status": status,
        "message": if problems.is_empty() { "Pi auto-discovery path is configured and git-ai extension looks ready".to_string() } else { problems.join("; ") },
    })
}

fn num_or_null(value: Option<&Value>) -> Option<f64> {
    match value {
        Some(Value::Number(n)) => n.as_f64(),
        Some(Value::String(s)) => s.parse::<f64>().ok(),
        _ => None,
    }
}

fn company_has_ai_mark(payload: &Value) -> bool {
    let ai_note = payload.get("ai_note");
    let stats = payload.get("stats");
    num_or_null(ai_note.and_then(|v| v.get("ai_lines_total"))).unwrap_or(0.0) > 0.0
        || num_or_null(ai_note.and_then(|v| v.get("frontmatter_ai_lines"))).unwrap_or(0.0) > 0.0
        || num_or_null(ai_note.and_then(|v| v.get("prompts_count"))).unwrap_or(0.0) > 0.0
        || num_or_null(stats.and_then(|v| v.get("ai_additions"))).unwrap_or(0.0) > 0.0
        || num_or_null(stats.and_then(|v| v.get("ai_rate"))).unwrap_or(0.0) > 0.0
}

async fn check_company_ai_mark(client: &reqwest::Client, record: &Value) -> Value {
    let project_name = record
        .get("projectName")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let commit_sha = record
        .get("commitSha")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if project_name.is_empty() || commit_sha.is_empty() {
        return json!({ "companyStatus": "check_failed", "companyError": "missing projectName or commitSha" });
    }
    let endpoint = env::var("AGENT_PANEL_AI_STATS_CHECK_URL")
        .unwrap_or_else(|_| "http://10.24.12.40/api/ai-stats/check-commit".into());
    let mut query: Vec<(&str, &str)> =
        vec![("project_name", project_name), ("commit_sha", commit_sha)];
    if let Some(gitlab_id) = record
        .get("gitlabProjectId")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
    {
        query.push(("gitlab_project_id", gitlab_id));
    }
    // Retry once on transient send errors (connection reset / timeout) so a
    // brief network blip doesn't permanently mark the record as check_failed.
    let mut resp = None;
    let mut last_err = String::new();
    for attempt in 0..2u8 {
        if attempt == 1 {
            sleep(Duration::from_millis(800)).await;
        }
        let req = client
            .get(endpoint.as_str())
            .query(&query)
            .header("Accept", "application/json")
            .timeout(Duration::from_secs(6));
        match req.send().await {
            Ok(v) => {
                resp = Some(v);
                break;
            }
            Err(err) => last_err = err.to_string(),
        }
    }
    let resp = match resp {
        Some(v) => v,
        None => return json!({ "companyStatus": "check_failed", "companyError": last_err }),
    };
    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    let payload: Value = serde_json::from_str(&text).unwrap_or_else(|_| json!({ "detail": text }));
    if !status.is_success() {
        return json!({ "companyStatus": "check_failed", "companyError": payload.get("detail").and_then(Value::as_str).unwrap_or("company API HTTP error") });
    }
    if payload.get("detail").is_some() && payload.get("commit").is_none() {
        return json!({ "companyStatus": "not_found", "companyError": payload.get("detail").and_then(Value::as_str).unwrap_or("not found") });
    }
    let Some(commit) = payload.get("commit") else {
        return json!({ "companyStatus": "check_failed", "companyError": "公司接口未返回 commit 对象" });
    };
    let stats = payload.get("stats").unwrap_or(&Value::Null);
    let ai_note = payload.get("ai_note").unwrap_or(&Value::Null);
    json!({
        "companyStatus": if company_has_ai_mark(&payload) { "confirmed_ai" } else { "missing_ai" },
        "companyError": Value::Null,
        "commitWebUrl": commit.get("web_url").cloned().unwrap_or(Value::Null),
        "commitTitle": commit.get("title").cloned().unwrap_or(Value::Null),
        "committedAt": commit.get("committed_at").cloned().unwrap_or(Value::Null),
        "originBranch": commit.get("origin_branch").or_else(|| commit.get("branch")).cloned().unwrap_or(Value::Null),
        "additions": commit.get("additions").cloned().unwrap_or(Value::Null),
        "deletions": commit.get("deletions").cloned().unwrap_or(Value::Null),
        "aiRate": stats.get("ai_rate").cloned().unwrap_or(Value::Null),
        "aiLines": stats.get("ai_additions").or_else(|| ai_note.get("ai_lines_total")).cloned().unwrap_or(Value::Null),
        "humanLines": stats.get("human_additions").cloned().unwrap_or(Value::Null),
    })
}

fn apply_company_result(record: &mut Value, result: Value, checked_at: i64) {
    let Some(obj) = record.as_object_mut() else {
        return;
    };
    obj.insert("companyCheckedAt".into(), json!(checked_at));
    for key in [
        "companyStatus",
        "companyError",
        "commitWebUrl",
        "commitTitle",
        "committedAt",
        "originBranch",
        "additions",
        "deletions",
        "aiRate",
        "aiLines",
        "humanLines",
    ] {
        if let Some(value) = result.get(key) {
            if !value.is_null() || key == "companyError" {
                obj.insert(key.into(), value.clone());
            }
        }
    }
}

pub(crate) async fn api_git_ai_suspects_refresh(
    State(state): State<AppState>,
    Json(payload): Json<Value>,
) -> ApiResult<Json<Value>> {
    let limit = payload.get("limit").and_then(Value::as_u64).unwrap_or(200) as usize;
    let force = payload
        .get("force")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let store_path = state.data_dir.join("git-ai-suspects.json");
    let mut store = read_json_if_exists(&store_path)
        .await
        .unwrap_or_else(|| json!({ "version": 1, "records": [] }));
    let mut records = store
        .get("records")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    records.sort_by_key(|r| -(r.get("lastSeenAt").and_then(Value::as_i64).unwrap_or(0)));
    let count = records.len().min(limit);
    // Reuse one HTTP client (pooled connections) for the whole batch. By
    // default skip records already confirmed by the company -- they don't
    // change, and re-checking all of them only amplifies transient failures.
    // Set force=true to re-check every record.
    let client = reqwest::Client::builder()
        .pool_idle_timeout(Duration::from_secs(30))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());
    for record in records.iter_mut().take(count) {
        if !force && record.get("companyStatus").and_then(Value::as_str) == Some("confirmed_ai") {
            continue;
        }
        let result = check_company_ai_mark(&client, record).await;
        apply_company_result(record, result, now_ms());
    }
    store["records"] = Value::Array(records);
    atomic_write_json(&store_path, &store).await?;
    Ok(Json(api_git_ai_suspects_payload(&state).await))
}

async fn api_git_ai_suspects_payload(state: &AppState) -> Value {
    let store_path = state.data_dir.join("git-ai-suspects.json");
    let records = read_json_if_exists(&store_path)
        .await
        .and_then(|v| v.get("records").and_then(Value::as_array).cloned())
        .unwrap_or_default();
    let mut pending = 0;
    let mut confirmed_ai = 0;
    let mut missing_ai = 0;
    let mut not_found = 0;
    let mut check_failed = 0;
    for record in &records {
        match record
            .get("companyStatus")
            .and_then(Value::as_str)
            .unwrap_or("pending")
        {
            "confirmed_ai" => confirmed_ai += 1,
            "missing_ai" => missing_ai += 1,
            "not_found" => not_found += 1,
            "check_failed" => check_failed += 1,
            _ => pending += 1,
        }
    }
    json!({
        "records": records,
        "stats": { "total": pending + not_found + check_failed, "pending": pending, "confirmedAi": confirmed_ai, "missingAi": missing_ai, "notFound": not_found, "checkFailed": check_failed },
        "generatedAt": now_ms()
    })
}

pub(crate) async fn api_git_ai_suspects(State(state): State<AppState>) -> Json<Value> {
    Json(api_git_ai_suspects_payload(&state).await)
}

/// One-click AI-note fix for a single suspect commit.
///
/// Workflow mirrors the user's spec:
///   1. In the commit's repo: `git fetch origin refs/notes/ai`,
///      `git notes --ref=ai merge -s cat_sort_uniq FETCH_HEAD`,
///      `git push origin refs/notes/ai` (with GIT_AI_SKIP=1 to bypass
///      the git-ai pre-push guard).
///   2. Sleep 4s, then re-query the company check-commit API.
///   3. If the company still says missing, spawn a non-interactive pi agent
///      with the `git-ai-fix-note` skill so it generates and pushes a fresh
///      note for that specific commit.
///
/// The pi agent runs detached (no .await on its completion) so the HTTP
/// response returns quickly with a "dispatched" status; the frontend polls
/// the suspects feed afterwards to see the updated company status.
pub(crate) async fn api_git_ai_suspect_fix_note(
    State(state): State<AppState>,
    Json(payload): Json<Value>,
) -> ApiResult<Json<Value>> {
    let id = payload
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    if id.is_empty() {
        return Err(ApiError::bad_request("missing record id"));
    }

    // Resolve the stored suspect record so we know repoPath / projectName / commitSha.
    let store_path = state.data_dir.join("git-ai-suspects.json");
    let store = read_json_if_exists(&store_path)
        .await
        .unwrap_or_else(|| json!({ "version": 1, "records": [] }));
    let record = store
        .get("records")
        .and_then(Value::as_array)
        .and_then(|arr| {
            arr.iter()
                .find(|r| r.get("id").and_then(Value::as_str) == Some(&id))
        })
        .cloned()
        .ok_or_else(|| ApiError::bad_request(format!("suspect record not found: {id}")))?;

    let project_name = record
        .get("projectName")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let commit_sha = record
        .get("commitSha")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let repo_path = resolve_git_ai_repo_path(&record);
    let Some(repo_path) = repo_path else {
        return Err(ApiError::bad_request(
            "record has no repoPath and the repo could not be located under ~/Developer/company/WMS",
        ));
    };
    if !repo_path.is_dir() {
        return Err(ApiError::bad_request(format!(
            "repo path does not exist: {}",
            repo_path.display()
        )));
    }

    // Step 1: re-push local notes to the remote.
    let push_steps = repush_git_ai_notes(&repo_path).await;

    // Step 2: wait 4s then re-check the company API.
    sleep(Duration::from_secs(4)).await;
    let client = reqwest::Client::new();
    let recheck = check_company_ai_mark(&client, &record).await;
    let still_missing = recheck
        .get("companyStatus")
        .and_then(Value::as_str)
        .map(|s| s != "confirmed_ai")
        .unwrap_or(true);

    // Persist the recheck result onto the stored record immediately.
    {
        let mut store = store;
        if let Some(records) = store.get_mut("records").and_then(Value::as_array_mut) {
            if let Some(rec) = records
                .iter_mut()
                .find(|r| r.get("id").and_then(Value::as_str) == Some(&id))
            {
                apply_company_result(rec, recheck.clone(), now_ms());
            }
        }
        atomic_write_json(&store_path, &store).await.ok();
    }

    let mut result = json!({
        "ok": true,
        "recheck": recheck,
        "pushSteps": push_steps,
        "stillMissing": still_missing,
    });

    // Step 3: if still missing, dispatch a non-interactive pi agent with the
    // git-ai-fix-note skill. The skill path resolves to the WMS project-local
    // copy (symlinked into ~/.agents/skills as well).
    if still_missing {
        let skill_path = home_dir()
            .ok()
            .map(|h| h.join("Developer/company/WMS/.agents/skills/git-ai-fix-note/SKILL.md"))
            .filter(|p| p.exists());
        match skill_path {
            Some(path) => {
                let prompt = format!(
                    "为 commit {commit_sha} 补全缺失的 git-ai 作者标注信息（git notes --ref=ai）。\
                     仓库路径：{repo}。项目名：{project}。\
                     先执行 git-ai-fix-note skill 的完整流程，确认目标 commit 缺失 AI note 后再补标；\
                     禁止用 --force 覆盖已有完整 note。",
                    repo = repo_path.display(),
                    project = project_name,
                );
                let session_id = Uuid::new_v4().to_string();
                let status = spawn_pi_fix_note_agent(
                    &repo_path,
                    &session_id,
                    &path.to_string_lossy(),
                    &prompt,
                )
                .await;
                result["piAgent"] = json!({
                    "dispatched": status.ok,
                    "sessionId": session_id,
                    "skillPath": path,
                    "message": if status.ok {
                        "pi agent 已在后台启动，正在用 git-ai-fix-note skill 补标".to_string()
                    } else {
                        status.message
                    },
                });
            }
            None => {
                result["piAgent"] = json!({
                    "dispatched": false,
                    "message": "未找到 git-ai-fix-note skill；请手动运行 pi 并加载该 skill 补标",
                });
            }
        }
    }

    Ok(Json(result))
}

/// Re-push local refs/notes/ai to the remote for the commit's repo.
/// Returns the three step results so the UI can surface failures.
async fn repush_git_ai_notes(repo_path: &Path) -> Vec<Value> {
    let env = vec![("GIT_AI_SKIP", "1")];
    let steps = [
        ("fetch notes", vec!["fetch", "origin", "refs/notes/ai"]),
        (
            "merge notes",
            vec![
                "notes",
                "--ref=ai",
                "merge",
                "-s",
                "cat_sort_uniq",
                "FETCH_HEAD",
            ],
        ),
        (
            "push notes",
            vec!["push", "origin", "refs/notes/ai:refs/notes/ai"],
        ),
    ];
    let mut out = Vec::new();
    for (label, args) in steps {
        let res = git_with_env(repo_path, &args, &env, 30_000, COMMAND_OUTPUT_LIMIT).await;
        out.push(json!({
            "label": label,
            "command": format!("GIT_AI_SKIP=1 git {}", args.join(" ")),
            "ok": res.ok,
            "stdout": compact(&res.stdout, 600),
            "stderr": compact(&res.stderr, 600),
        }));
    }
    out
}

struct SpawnResult {
    ok: bool,
    message: String,
}

/// Spawn a detached `pi -p --skill <skill> <prompt>` process in the repo.
/// The process is detached so the HTTP request returns immediately; the
/// agent writes its own session JSONL which the panel can inspect later.
async fn spawn_pi_fix_note_agent(
    repo_path: &Path,
    session_id: &str,
    skill_path: &str,
    prompt: &str,
) -> SpawnResult {
    let mut cmd = Command::new("pi");
    cmd.current_dir(repo_path)
        .arg("-p")
        .arg("--session-id")
        .arg(session_id)
        .arg("--name")
        .arg(format!("git-ai-fix-note {}", &commit_short(session_id)))
        .arg("--skill")
        .arg(skill_path)
        .arg("--tools")
        .arg("bash,read,write")
        .arg("--approve")
        .arg(prompt)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    match cmd.spawn() {
        Ok(child) => {
            let pid = child.id();
            tracing::info!(
                ?pid,
                %session_id,
                "spawned pi git-ai-fix-note agent"
            );
            SpawnResult {
                ok: true,
                message: format!("pi agent spawned (pid {})", pid.unwrap_or(0)),
            }
        }
        Err(err) => SpawnResult {
            ok: false,
            message: format!("failed to spawn pi agent: {err}"),
        },
    }
}

fn commit_short(s: &str) -> &str {
    s.get(..8).unwrap_or(s)
}

/// Resolve the on-disk repo path for a suspect record. Uses repoPath when
/// present; otherwise looks for `yl-cwhsea-wms-<projectName>` (stripping the
/// `yl-cwhsea-wms-` prefix from projectName for leaf matching) under the
/// WMS backend/frontend/pda/infra areas.
fn resolve_git_ai_repo_path(record: &Value) -> Option<PathBuf> {
    if let Some(path) = record
        .get("repoPath")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
    {
        let p = PathBuf::from(path);
        if p.is_dir() {
            return Some(p);
        }
    }
    let project = record
        .get("projectName")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if project.is_empty() {
        return None;
    }
    let home = home_dir().ok()?;
    let leaf = if let Some(stripped) = project.strip_prefix("yl-cwhsea-wms-") {
        format!("yl-cwhsea-wms-{stripped}")
    } else if project.starts_with("yl-cwhsea-wms") {
        project.to_string()
    } else {
        format!("yl-cwhsea-wms-{project}")
    };
    for area in ["backend", "frontend", "pda", "infra"] {
        let candidate = home.join("Developer/company/WMS").join(area).join(&leaf);
        if candidate.is_dir() {
            return Some(candidate);
        }
    }
    None
}

/// Like `git()` but with extra environment variables (e.g. GIT_AI_SKIP=1).
async fn git_with_env(
    cwd: &Path,
    args: &[&str],
    env: &[(&str, &str)],
    timeout_ms: u64,
    max_output: usize,
) -> GitCommandResult {
    let command = std::iter::once("git".to_string())
        .chain(args.iter().map(|a| shell_quote(a)))
        .collect::<Vec<_>>()
        .join(" ");
    let mut cmd = Command::new("git");
    cmd.args(args).current_dir(cwd);
    for (k, v) in env {
        cmd.env(k, v);
    }
    let fut = cmd.output();
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
