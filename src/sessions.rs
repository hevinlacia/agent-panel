use std::{
    collections::{HashMap, HashSet},
    io::Read,
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};

use anyhow::Result;
use axum::{
    extract::{Query, State},
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::fs;
use uuid::Uuid;
use walkdir::WalkDir;

use crate::{
    clean_required_opt, compact, now_ms, parse_date_ms, system_time_to_ms, ApiError, ApiResult,
    AppState, IdQuery,
};

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SessionInfo {
    pub(crate) id: String,
    pub(crate) title: String,
    pub(crate) status: String,
    pub(crate) agent: String,
    pub(crate) source: String,
    pub(crate) path: String,
    pub(crate) directory: String,
    pub(crate) worktree: String,
    pub(crate) created: i64,
    pub(crate) updated: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) model_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) model_provider: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) provider: Option<String>,
    pub(crate) tokens_input: u64,
    pub(crate) tokens_output: u64,
    pub(crate) tokens_reasoning: u64,
    pub(crate) tokens_cache_read: u64,
    pub(crate) tokens_cache_write: u64,
    pub(crate) cost: f64,
    pub(crate) message_count: u64,
    pub(crate) user_message_count: u64,
    pub(crate) assistant_message_count: u64,
    pub(crate) tool_result_count: u64,
    pub(crate) tool_call_count: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) thinking_level: Option<String>,
}

pub(crate) fn current_harness(state: &AppState) -> String {
    // Mirrors harness.rs: persisted agent-panel config `harness` field, default pi.
    let path = state.data_dir.join("config.json");
    if let Ok(raw) = std::fs::read_to_string(path) {
        if let Ok(v) = serde_json::from_str::<Value>(&raw) {
            if let Some(h) = v.get("harness").and_then(Value::as_str) {
                let h = h.trim().to_ascii_lowercase();
                if h == "dsh" || h == "deepseek-harness" || h == "deepseek_harness" {
                    return "dsh".into();
                }
            }
        }
    }
    "pi".into()
}

pub(crate) async fn scan_sessions_for_current_harness(
    state: &AppState,
    days: Option<i64>,
) -> Result<Vec<SessionInfo>> {
    if current_harness(state) == "dsh" {
        scan_dsh_sessions(state, days).await
    } else {
        scan_pi_sessions(state, days).await
    }
}

/// Unassociated sessions of the current harness, newest first, with sessions
/// whose working directory sits inside the requirement's project root (when
/// known) sorted to the front. Used by the requirement detail page to let a
/// user pick a session it just opened in dsh-tui and link it to the requirement.
pub(crate) async fn scan_session_candidates(
    state: &AppState,
    project_root: Option<&str>,
    exclude_ids: &HashSet<String>,
) -> Result<Vec<SessionInfo>> {
    let harness = current_harness(state);
    let all = if harness == "dsh" {
        scan_dsh_sessions(state, None).await?
    } else {
        scan_pi_sessions(state, None).await?
    };
    let root = project_root.map(|r| r.trim_end_matches('/').to_string()).unwrap_or_default();
    let mut out = Vec::new();
    for s in all {
        if s.id.is_empty() || exclude_ids.contains(&s.id) {
            continue;
        }
        let bare = s.id.trim_start_matches("session-");
        if exclude_ids.contains(bare) {
            continue;
        }
        out.push(s);
        if out.len() >= 60 {
            break;
        }
    }
    out.sort_by(|a, b| {
        let am = directory_matches_project(&a.directory, &root);
        let bm = directory_matches_project(&b.directory, &root);
        bm.cmp(&am).then(b.updated.cmp(&a.updated))
    });
    Ok(out)
}

fn directory_matches_project(dir: &str, root: &str) -> bool {
    if root.is_empty() {
        return false;
    }
    let dir = dir.trim_end_matches('/');
    let root = root.trim_end_matches('/');
    !dir.is_empty() && (dir == root || dir.starts_with(&format!("{}/", root)))
}

pub(crate) async fn api_sessions(
    State(state): State<AppState>,
    Query(query): Query<IdQuery>,
) -> ApiResult<Json<Value>> {
    let harness = current_harness(&state);
    let sessions = if harness == "dsh" {
        scan_dsh_sessions(&state, query.days).await?
    } else {
        scan_pi_sessions(&state, query.days).await?
    };
    let mut summary: HashMap<String, usize> = HashMap::new();
    for s in &sessions {
        *summary.entry(s.status.clone()).or_default() += 1;
    }
    Ok(Json(
        json!({ "summary": summary, "sessions": sessions, "harness": harness, "days": query.days.unwrap_or(7) }),
    ))
}

pub(crate) async fn api_session(
    State(state): State<AppState>,
    Query(query): Query<IdQuery>,
) -> ApiResult<Json<Value>> {
    let id = query.id.unwrap_or_default();
    let harness = current_harness(&state);
    let session = if harness == "dsh" {
        scan_dsh_sessions(&state, None)
            .await?
            .into_iter()
            .find(|s| s.id == id)
    } else {
        scan_pi_sessions(&state, None)
            .await?
            .into_iter()
            .find(|s| s.id == id)
    };
    // Fallback: if not found in current harness, try the other one so direct links still resolve.
    let session = if session.is_some() {
        session
    } else if harness == "dsh" {
        scan_pi_sessions(&state, None).await?.into_iter().find(|s| s.id == id)
    } else {
        scan_dsh_sessions(&state, None).await?.into_iter().find(|s| s.id == id)
    };
    Ok(Json(json!({ "session": session, "terminalRemoved": true, "harness": harness })))
}

pub(crate) async fn api_session_log(
    State(state): State<AppState>,
    Query(query): Query<IdQuery>,
) -> ApiResult<Json<Value>> {
    let id = clean_required_opt(query.id.as_deref(), "id")?;
    let cursor = query.cursor.unwrap_or(0);
    let limit = query.limit.unwrap_or(80).clamp(1, 300);
    // Try both roots so log works regardless of current harness.
    let path = find_pi_session_path(&state, &id)
        .await?
        .or(find_dsh_session_path(&state, &id).await?);
    let Some(path) = path else {
        return Err(ApiError::bad_request(format!("session not found: {id}")));
    };
    let is_dsh = path.extension().and_then(|s| s.to_str()) == Some("zstd")
        || path.to_string_lossy().contains(".dsh/");
    let meta = fs::metadata(&path).await.ok();
    let updated_at = meta
        .as_ref()
        .and_then(|m| m.modified().ok())
        .map(system_time_to_ms)
        .unwrap_or(0);
    let raw = if is_dsh {
        read_dsh_session_text(&path).await.unwrap_or_default()
    } else {
        fs::read_to_string(&path).await.unwrap_or_default()
    };
    let lines: Vec<String> = raw.lines().filter(|line| !line.trim().is_empty()).map(|s| s.to_string()).collect();
    let total = lines.len();
    let start = cursor.min(total);
    let end = (start + limit).min(total);
    let entries: Vec<Value> = lines[start..end]
        .iter()
        .enumerate()
        .filter_map(|(idx, line)| {
            if is_dsh {
                parse_dsh_log_entry(start + idx, line)
            } else {
                parse_session_log_entry(start + idx, line)
            }
        })
        .collect();
    Ok(Json(json!({
        "ok": true,
        "sessionId": id,
        "path": path.to_string_lossy(),
        "cursor": end,
        "total": total,
        "hasMore": end < total,
        "updatedAt": updated_at,
        "entries": entries,
    })))
}

pub(crate) async fn api_sessions_resolve(
    State(state): State<AppState>,
    Query(query): Query<IdQuery>,
) -> ApiResult<Json<Value>> {
    let ids: Vec<String> = query
        .ids
        .as_deref()
        .map(|s| {
            s.split(',')
                .map(|x| x.trim().to_string())
                .filter(|x| !x.is_empty())
                .collect()
        })
        .unwrap_or_default();
    if ids.is_empty() {
        return Ok(Json(json!({ "sessions": [], "missing": [] })));
    }
    let set: HashSet<String> = ids.iter().cloned().collect();
    let harness = current_harness(&state);
    // Resolve within current harness, then fallback to the other so chips still show.
    let mut found = if harness == "dsh" {
        scan_dsh_sessions_filtered(&state, None, Some(&set)).await?
    } else {
        scan_pi_sessions_filtered(&state, None, Some(&set)).await?
    };
    if found.len() < set.len() {
        let have: HashSet<String> = found.iter().map(|s| s.id.clone()).collect();
        let missing_ids: HashSet<String> = set.difference(&have).cloned().collect();
        let extra = if harness == "dsh" {
            scan_pi_sessions_filtered(&state, None, Some(&missing_ids)).await?
        } else {
            scan_dsh_sessions_filtered(&state, None, Some(&missing_ids)).await?
        };
        found.extend(extra);
    }
    let by_id: HashMap<&str, &SessionInfo> = found.iter().map(|s| (s.id.as_str(), s)).collect();
    let mut sessions: Vec<&SessionInfo> = Vec::new();
    let mut missing: Vec<String> = Vec::new();
    for id in &ids {
        match by_id.get(id.as_str()) {
            Some(session) => sessions.push(session),
            None => missing.push(id.clone()),
        }
    }
    Ok(Json(json!({ "sessions": sessions, "missing": missing })))
}

pub(crate) async fn scan_pi_sessions(
    state: &AppState,
    days: Option<i64>,
) -> Result<Vec<SessionInfo>> {
    scan_pi_sessions_filtered(state, days, None).await
}

async fn find_pi_session_path(state: &AppState, id: &str) -> Result<Option<PathBuf>> {
    let id = id.trim();
    if id.is_empty() {
        return Ok(None);
    }
    let root = state.pi_session_root.as_ref();
    if !root.is_dir() {
        return Ok(None);
    }
    for entry in WalkDir::new(root)
        .min_depth(2)
        .max_depth(2)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if !path.is_file() || path.extension().and_then(|s| s.to_str()) != Some("jsonl") {
            continue;
        }
        let raw = fs::read_to_string(path).await.unwrap_or_default();
        let Some(first) = raw.lines().find(|l| !l.trim().is_empty()) else {
            continue;
        };
        let Ok(header) = serde_json::from_str::<Value>(first) else {
            continue;
        };
        let session_id = header.get("id").and_then(Value::as_str).unwrap_or_default();
        if session_id == id || session_id.starts_with(id) {
            return Ok(Some(path.to_path_buf()));
        }
    }
    Ok(None)
}

async fn find_dsh_session_path(state: &AppState, id: &str) -> Result<Option<PathBuf>> {
    let id = id.trim();
    if id.is_empty() {
        return Ok(None);
    }
    let root = state.dsh_session_root.as_ref();
    if !root.is_dir() {
        return Ok(None);
    }
    // DSH: ~/.dsh/sessions/<workspace>/<sessionId>/session.jsonl.zstd
    // Workspace dir is encoded cwd (e.g. --home-hevin-Developer--), unknown to caller, so scan.
    for entry in WalkDir::new(root)
        .min_depth(2)
        .max_depth(3)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
        if name != "session.jsonl.zstd" && name != "session.jsonl" {
            continue;
        }
        // Prefer header id without decompressing full file when possible: dir name often is session id
        if let Some(parent) = path.parent().and_then(|p| p.file_name()).and_then(|s| s.to_str()) {
            if parent == id || parent.starts_with(id) || id.starts_with(parent) {
                return Ok(Some(path.to_path_buf()));
            }
        }
        // Fallback: read header id
        let raw = read_dsh_session_text(path).await.unwrap_or_default();
        let Some(first) = raw.lines().find(|l| !l.trim().is_empty()) else {
            continue;
        };
        let Ok(header) = serde_json::from_str::<Value>(first) else {
            continue;
        };
        let sid = header.get("id").and_then(Value::as_str).unwrap_or_default();
        if sid == id || sid.starts_with(id) || id == sid {
            return Ok(Some(path.to_path_buf()));
        }
        // Also handle legacy "session-xxx" dir names
        if sid.trim_start_matches("session-") == id.trim_start_matches("session-") {
            // exact compare already above; this covers bare uuid vs session-<uuid>
            let a = sid.trim_start_matches("session-");
            let b = id.trim_start_matches("session-");
            if a == b {
                return Ok(Some(path.to_path_buf()));
            }
        }
    }
    Ok(None)
}

async fn read_dsh_session_text(path: &Path) -> Result<String> {
    let bytes = fs::read(path).await?;
    // session.jsonl.zstd is zstd-compressed; session.jsonl is plain
    if path.extension().and_then(|s| s.to_str()) == Some("zstd") {
        let text = tokio::task::spawn_blocking(move || {
            let mut dec = zstd::stream::Decoder::new(bytes.as_slice())?;
            let mut out = String::new();
            dec.read_to_string(&mut out)?;
            Ok::<String, anyhow::Error>(out)
        })
        .await??;
        Ok(text)
    } else {
        Ok(String::from_utf8_lossy(&bytes).to_string())
    }
}

fn parse_session_log_entry(line_no: usize, line: &str) -> Option<Value> {
    let value: Value = serde_json::from_str(line).ok()?;
    let entry_type = value
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let timestamp = value
        .get("timestamp")
        .and_then(Value::as_str)
        .and_then(parse_date_ms);
    match entry_type {
        "session" => Some(json!({
            "line": line_no,
            "type": "session",
            "timestamp": timestamp,
            "title": "Session started",
            "text": value.get("cwd").and_then(Value::as_str).unwrap_or_default(),
            "rawType": entry_type,
        })),
        "session_info" => Some(json!({
            "line": line_no,
            "type": "info",
            "timestamp": timestamp,
            "title": "Session info",
            "text": value.get("name").and_then(Value::as_str).unwrap_or_default(),
            "rawType": entry_type,
        })),
        "model_change" => Some(json!({
            "line": line_no,
            "type": "info",
            "timestamp": timestamp,
            "title": "Model change",
            "text": format!("{} / {}", value.get("provider").and_then(Value::as_str).unwrap_or("-"), value.get("modelId").and_then(Value::as_str).unwrap_or("-")),
            "rawType": entry_type,
        })),
        "thinking_level_change" => Some(json!({
            "line": line_no,
            "type": "info",
            "timestamp": timestamp,
            "title": "Thinking level",
            "text": value.get("thinkingLevel").and_then(Value::as_str).unwrap_or_default(),
            "rawType": entry_type,
        })),
        "message" => {
            let msg = value.get("message")?;
            let role = msg.get("role").and_then(Value::as_str).unwrap_or("message");
            let mut text_parts = Vec::new();
            let mut tools = Vec::new();
            if let Some(parts) = msg.get("content").and_then(Value::as_array) {
                for part in parts {
                    match part.get("type").and_then(Value::as_str).unwrap_or_default() {
                        "text" => {
                            if let Some(text) = part.get("text").and_then(Value::as_str) {
                                text_parts.push(text.to_string());
                            }
                        }
                        "toolCall" => {
                            tools.push(json!({
                                "kind": "call",
                                "name": part.get("toolName").or_else(|| part.get("name")).and_then(Value::as_str).unwrap_or("tool"),
                                "id": part.get("toolCallId").or_else(|| part.get("id")).and_then(Value::as_str).unwrap_or_default(),
                            }));
                        }
                        "toolResult" => {
                            tools.push(json!({
                                "kind": "result",
                                "name": part.get("toolName").or_else(|| part.get("name")).and_then(Value::as_str).unwrap_or("tool"),
                                "id": part.get("toolCallId").or_else(|| part.get("id")).and_then(Value::as_str).unwrap_or_default(),
                            }));
                            if let Some(text) = tool_result_text(part) {
                                text_parts.push(text);
                            }
                        }
                        _ => {}
                    }
                }
            }
            if text_parts.is_empty() {
                text_parts.push(text_from_user_message(msg));
            }
            let usage = msg.get("usage").cloned().unwrap_or(Value::Null);
            Some(json!({
                "line": line_no,
                "type": role,
                "timestamp": timestamp,
                "title": role,
                "text": text_parts.join("\n\n"),
                "tools": tools,
                "usage": usage,
                "rawType": entry_type,
            }))
        }
        other => Some(json!({
            "line": line_no,
            "type": "event",
            "timestamp": timestamp,
            "title": other,
            "text": compact(&line.to_string(), 1200).unwrap_or_default(),
            "rawType": entry_type,
        })),
    }
}

fn parse_dsh_log_entry(line_no: usize, line: &str) -> Option<Value> {
    let value: Value = serde_json::from_str(line).ok()?;
    let entry_type = value.get("type").and_then(Value::as_str).unwrap_or("unknown");
    // DSH uses numeric `time` (ms since epoch), not RFC3339 `timestamp`
    let timestamp = value
        .get("time")
        .and_then(Value::as_i64)
        .or_else(|| value.get("timestamp").and_then(Value::as_str).and_then(parse_date_ms));
    let data = value.get("data");
    match entry_type {
        "session" => Some(json!({
            "line": line_no,
            "type": "session",
            "timestamp": timestamp,
            "title": "Session started",
            "text": value.get("cwd").and_then(Value::as_str).unwrap_or_default(),
            "rawType": entry_type,
        })),
        "session/title" => Some(json!({
            "line": line_no,
            "type": "info",
            "timestamp": timestamp,
            "title": "Session title",
            "text": data.and_then(|d| d.get("title")).and_then(Value::as_str).unwrap_or_default(),
            "rawType": entry_type,
        })),
        "user/message" => {
            let d = data?;
            let content = d.get("content").and_then(Value::as_array);
            let mut text = String::new();
            if let Some(parts) = content {
                for p in parts {
                    if p.get("type").and_then(Value::as_str) == Some("text") {
                        if let Some(t) = p.get("text").and_then(Value::as_str) {
                            if !text.is_empty() { text.push_str("\n\n"); }
                            text.push_str(t);
                        }
                    }
                }
            }
            // source kind for hint
            let source_kind = d.get("source").and_then(|s| s.get("kind")).and_then(Value::as_str).unwrap_or("user");
            Some(json!({
                "line": line_no,
                "type": "user",
                "timestamp": timestamp,
                "title": if source_kind == "user" { "user".to_string() } else { format!("user ({source_kind})") },
                "text": text,
                "rawType": entry_type,
            }))
        }
        "assistant/message" => {
            let d = data?;
            let msg = d.get("message")?;
            let role = msg.get("role").and_then(Value::as_str).unwrap_or("assistant");
            let mut text_parts: Vec<String> = Vec::new();
            let mut tools: Vec<Value> = Vec::new();
            if let Some(parts) = msg.get("content").and_then(Value::as_array) {
                for p in parts {
                    match p.get("type").and_then(Value::as_str).unwrap_or("") {
                        "text" => {
                            if let Some(t) = p.get("text").and_then(Value::as_str) {
                                text_parts.push(t.to_string());
                            }
                        }
                        "tool-call" | "tool_call" => {
                            tools.push(json!({
                                "kind": "call",
                                "name": p.get("name").or_else(|| p.get("toolName")).and_then(Value::as_str).unwrap_or("tool"),
                                "id": p.get("id").or_else(|| p.get("toolCallId")).and_then(Value::as_str).unwrap_or_default(),
                            }));
                        }
                        _ => {}
                    }
                }
            }
            let usage = d.get("usage").cloned().or_else(|| msg.get("usage").cloned()).unwrap_or(Value::Null);
            let provider = msg.get("source").and_then(|s| s.get("provider")).and_then(Value::as_str)
                .or_else(|| d.get("provider").and_then(Value::as_str))
                .unwrap_or("");
            let model = msg.get("source").and_then(|s| s.get("model")).and_then(Value::as_str)
                .or_else(|| d.get("model").and_then(Value::as_str))
                .unwrap_or("");
            let meta = if !provider.is_empty() || !model.is_empty() {
                format!("{provider}/{model}")
            } else { String::new() };
            Some(json!({
                "line": line_no,
                "type": role,
                "timestamp": timestamp,
                "title": if meta.is_empty() { role.to_string() } else { format!("{role} · {meta}") },
                "text": text_parts.join("\n\n"),
                "tools": tools,
                "usage": usage,
                "rawType": entry_type,
            }))
        }
        "tool/call" => {
            let d = data?;
            let name = d.get("name").and_then(Value::as_str).unwrap_or("tool");
            let args = d.get("arguments").and_then(Value::as_str).unwrap_or("");
            Some(json!({
                "line": line_no,
                "type": "tool_call",
                "timestamp": timestamp,
                "title": format!("tool/call · {name}"),
                "text": args.chars().take(4000).collect::<String>(),
                "tools": [{"kind": "call", "name": name, "id": d.get("callId").and_then(Value::as_str).unwrap_or_default()}],
                "rawType": entry_type,
            }))
        }
        "tool/result" => {
            let d = data?;
            let msg = d.get("message");
            let mut text = String::new();
            if let Some(content) = msg.and_then(|m| m.get("content")).and_then(Value::as_array) {
                for p in content {
                    // tool-result content is often [{type:"tool-result", content:[{type:"text", text:"..."}]}]
                    if let Some(inner) = p.get("content").and_then(Value::as_array) {
                        for c in inner {
                            if let Some(t) = c.get("text").and_then(Value::as_str) {
                                if !text.is_empty() { text.push_str("\n\n"); }
                                text.push_str(t);
                            }
                        }
                    } else if let Some(t) = p.get("text").and_then(Value::as_str) {
                        if !text.is_empty() { text.push_str("\n\n"); }
                        text.push_str(t);
                    }
                    if let Some(s) = p.get("content").and_then(Value::as_str) {
                        if !text.is_empty() { text.push_str("\n\n"); }
                        text.push_str(&s);
                    }
                }
            }
            if text.is_empty() {
                text = compact(&line.to_string(), 2000).unwrap_or_default();
            } else {
                text = text.chars().take(4000).collect();
            }
            Some(json!({
                "line": line_no,
                "type": "tool_result",
                "timestamp": timestamp,
                "title": "tool/result",
                "text": text,
                "tools": [{"kind": "result", "name": "tool", "id": d.get("message").and_then(|m| m.get("source")).and_then(|s| s.get("callId")).and_then(Value::as_str).unwrap_or_default()}],
                "rawType": entry_type,
            }))
        }
        "request/header" | "request/context" | "session/title-llm-request" | "permission/preset" | "sandbox/mode" | "approval/policy" | "agent/inbox/spliced" | "turn/start" | "turn/end" | "step/start" | "step/end" => {
            let text = data.map(|d| compact(&d.to_string(), 1200).unwrap_or_default()).unwrap_or_default();
            Some(json!({
                "line": line_no,
                "type": "event",
                "timestamp": timestamp,
                "title": entry_type,
                "text": text,
                "rawType": entry_type,
            }))
        }
        _ if entry_type.starts_with("assistant/chunk") || entry_type == "text-chunks" || entry_type == "tool-call-chunks" => {
            // Streaming internals — collapse to event to avoid flooding
            Some(json!({
                "line": line_no,
                "type": "event",
                "timestamp": timestamp,
                "title": entry_type,
                "text": compact(&line.to_string(), 1200).unwrap_or_default(),
                "rawType": entry_type,
            }))
        }
        _ => Some(json!({
            "line": line_no,
            "type": "event",
            "timestamp": timestamp,
            "title": entry_type,
            "text": compact(&line.to_string(), 1200).unwrap_or_default(),
            "rawType": entry_type,
        })),
    }
}

fn tool_result_text(part: &Value) -> Option<String> {
    part.get("text")
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| {
            part.get("content").and_then(|v| {
                if let Some(s) = v.as_str() {
                    Some(s.to_string())
                } else if v.is_null() {
                    None
                } else {
                    serde_json::to_string(v).ok()
                }
            })
        })
        .map(|s| s.chars().take(4_000).collect())
}

async fn scan_pi_sessions_filtered(
    state: &AppState,
    days: Option<i64>,
    ids: Option<&HashSet<String>>,
) -> Result<Vec<SessionInfo>> {
    let root = state.pi_session_root.as_ref();
    if !root.is_dir() {
        return Ok(Vec::new());
    }
    let cutoff = days.filter(|d| *d > 0).map(|d| now_ms() - d * 86_400_000);
    let mut out = Vec::new();
    for entry in WalkDir::new(root)
        .min_depth(2)
        .max_depth(2)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if !path.is_file() || path.extension().and_then(|s| s.to_str()) != Some("jsonl") {
            continue;
        }
        if let Some(session) = read_pi_session_file(path).await {
            match ids {
                Some(ids) => {
                    if ids.contains(&session.id) {
                        out.push(session);
                    }
                }
                None => {
                    if cutoff
                        .map(|c| session.updated >= c || session.created >= c)
                        .unwrap_or(true)
                    {
                        out.push(session);
                    }
                }
            }
        }
    }
    out.sort_by(|a, b| b.updated.cmp(&a.updated));
    if ids.is_none() {
        out.truncate(200);
    }
    Ok(out)
}

async fn scan_dsh_sessions_filtered(
    state: &AppState,
    days: Option<i64>,
    ids: Option<&HashSet<String>>,
) -> Result<Vec<SessionInfo>> {
    let root = state.dsh_session_root.as_ref();
    if !root.is_dir() {
        return Ok(Vec::new());
    }
    let cutoff = days.filter(|d| *d > 0).map(|d| now_ms() - d * 86_400_000);
    let mut out = Vec::new();
    for entry in WalkDir::new(root)
        .min_depth(2)
        .max_depth(3)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
        if name != "session.jsonl.zstd" && name != "session.jsonl" {
            continue;
        }
        if let Some(session) = read_dsh_session_file(path).await {
            match ids {
                Some(ids) => {
                    // DSH dirs may be `session-<uuid>` while id is bare uuid; normalize both
                    let bare = session.id.trim_start_matches("session-");
                    let matches = ids.contains(&session.id) || ids.contains(bare) || ids.iter().any(|q| q.trim_start_matches("session-") == bare);
                    if matches {
                        out.push(session);
                    }
                }
                None => {
                    if cutoff
                        .map(|c| session.updated >= c || session.created >= c)
                        .unwrap_or(true)
                    {
                        out.push(session);
                    }
                }
            }
        }
    }
    out.sort_by(|a, b| b.updated.cmp(&a.updated));
    if ids.is_none() {
        out.truncate(200);
    }
    Ok(out)
}

async fn scan_dsh_sessions(state: &AppState, days: Option<i64>) -> Result<Vec<SessionInfo>> {
    scan_dsh_sessions_filtered(state, days, None).await
}

async fn read_pi_session_file(path: &Path) -> Option<SessionInfo> {
    let meta = fs::metadata(path).await.ok()?;
    let raw = fs::read_to_string(path).await.ok()?;
    let mut lines = raw.lines().filter(|l| !l.trim().is_empty());
    let header: Value = serde_json::from_str(lines.next()?).ok()?;
    if header.get("type")?.as_str()? != "session" {
        return None;
    }
    let id = header.get("id")?.as_str()?.to_string();
    if Uuid::parse_str(&id).is_err() {
        return None;
    }
    let cwd = header
        .get("cwd")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let created = header
        .get("timestamp")
        .and_then(Value::as_str)
        .and_then(parse_date_ms)
        .unwrap_or_else(|| system_time_to_ms(meta.created().unwrap_or(UNIX_EPOCH)));
    let updated = system_time_to_ms(meta.modified().unwrap_or(UNIX_EPOCH));
    let mut title = String::new();
    let mut model_id: Option<String> = None;
    let mut provider: Option<String> = None;
    let mut thinking_level: Option<String> = None;
    let mut message_count = 0;
    let mut user_message_count = 0;
    let mut assistant_message_count = 0;
    let mut tool_result_count = 0;
    let mut tool_call_count = 0;
    let mut tokens_input = 0;
    let mut tokens_output = 0;
    let mut tokens_reasoning = 0;
    let mut tokens_cache_read = 0;
    let mut tokens_cache_write = 0;
    let mut cost = 0.0;
    for line in lines {
        let entry: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => continue,
        };
        match entry
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default()
        {
            "model_change" => {
                model_id = entry
                    .get("modelId")
                    .and_then(Value::as_str)
                    .map(|s| s.to_string())
                    .or(model_id);
                provider = entry
                    .get("provider")
                    .and_then(Value::as_str)
                    .map(|s| s.to_string())
                    .or(provider);
            }
            "thinking_level_change" => {
                thinking_level = entry
                    .get("thinkingLevel")
                    .and_then(Value::as_str)
                    .map(|s| s.to_string())
                    .or(thinking_level)
            }
            "session_info" => {
                if title.is_empty() {
                    title = entry
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .trim()
                        .chars()
                        .take(200)
                        .collect();
                }
            }
            "message" => {
                message_count += 1;
                let Some(msg) = entry.get("message") else {
                    continue;
                };
                if title.is_empty() {
                    title = text_from_user_message(msg);
                }
                match msg.get("role").and_then(Value::as_str).unwrap_or_default() {
                    "user" => user_message_count += 1,
                    "assistant" => assistant_message_count += 1,
                    "toolResult" => tool_result_count += 1,
                    _ => {}
                }
                provider = msg
                    .get("provider")
                    .and_then(Value::as_str)
                    .map(|s| s.to_string())
                    .or(provider);
                model_id = msg
                    .get("model")
                    .and_then(Value::as_str)
                    .map(|s| s.to_string())
                    .or(model_id);
                if let Some(parts) = msg.get("content").and_then(Value::as_array) {
                    tool_call_count += parts
                        .iter()
                        .filter(|p| p.get("type").and_then(Value::as_str) == Some("toolCall"))
                        .count() as u64;
                }
                if let Some(usage) = msg.get("usage") {
                    tokens_input += usage.get("input").and_then(Value::as_u64).unwrap_or(0);
                    tokens_output += usage.get("output").and_then(Value::as_u64).unwrap_or(0);
                    tokens_reasoning += usage.get("reasoning").and_then(Value::as_u64).unwrap_or(0);
                    tokens_cache_read +=
                        usage.get("cacheRead").and_then(Value::as_u64).unwrap_or(0);
                    tokens_cache_write +=
                        usage.get("cacheWrite").and_then(Value::as_u64).unwrap_or(0);
                    cost += usage
                        .get("cost")
                        .and_then(|c| c.get("total"))
                        .and_then(Value::as_f64)
                        .unwrap_or(0.0);
                }
            }
            _ => {}
        }
    }
    if title.is_empty() {
        title = format!("pi {}", &id[..8]);
    }
    let model = model_id.clone();
    let model_provider = provider.clone();
    Some(SessionInfo {
        id,
        title,
        status: status_from_updated(updated),
        agent: "pi".into(),
        source: "fs".into(),
        path: path.to_string_lossy().to_string(),
        directory: cwd.clone(),
        worktree: derive_worktree(&cwd),
        created,
        updated,
        model_id,
        model_provider,
        model,
        provider,
        tokens_input,
        tokens_output,
        tokens_reasoning,
        tokens_cache_read,
        tokens_cache_write,
        cost,
        message_count,
        user_message_count,
        assistant_message_count,
        tool_result_count,
        tool_call_count,
        thinking_level,
    })
}

async fn read_dsh_session_file(path: &Path) -> Option<SessionInfo> {
    // DSH session file may be zstd-compressed
    let meta = fs::metadata(path).await.ok()?;
    let raw = read_dsh_session_text(path).await.ok()?;
    let mut lines = raw.lines().filter(|l| !l.trim().is_empty());
    let header: Value = serde_json::from_str(lines.next()?).ok()?;
    if header.get("type").and_then(Value::as_str) != Some("session") {
        return None;
    }
    let id = header.get("id").and_then(Value::as_str)?.to_string();
    // DSH ids are uuids (bare) or session-<uuid> dirs; allow both
    let bare = id.trim_start_matches("session-");
    if Uuid::parse_str(bare).is_err() {
        return None;
    }
    let cwd = header.get("cwd").and_then(Value::as_str).unwrap_or_default().to_string();
    let created = header.get("createdAt").and_then(Value::as_i64)
        .unwrap_or_else(|| system_time_to_ms(meta.created().unwrap_or(UNIX_EPOCH)));
    let updated = system_time_to_ms(meta.modified().unwrap_or(UNIX_EPOCH));
    let mut title = String::new();
    let mut provider: Option<String> = None;
    let mut model_id: Option<String> = None;
    let thinking_level: Option<String> = None;
    let mut message_count: u64 = 0;
    let mut user_count: u64 = 0;
    let mut assistant_count: u64 = 0;
    let mut tool_call_count: u64 = 0;
    let mut tool_result_count: u64 = 0;
    let mut tokens_input: u64 = 0;
    let mut tokens_output: u64 = 0;
    let mut tokens_reasoning: u64 = 0;
    let mut tokens_cache_read: u64 = 0;
    let mut tokens_cache_write: u64 = 0;
    let mut cost: f64 = 0.0;

    for line in lines {
        let entry: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let t = entry.get("type").and_then(Value::as_str).unwrap_or("");
        match t {
            "session/title" => {
                let cand = entry.get("data").and_then(|d| d.get("title")).and_then(Value::as_str).unwrap_or("").trim();
                // Prefer provider-generated title over fallback (later entry wins if more specific)
                let source_kind = entry.get("data").and_then(|d| d.get("source")).and_then(|s| s.get("kind")).and_then(Value::as_str).unwrap_or("");
                if !cand.is_empty() {
                    if source_kind == "provider" || title.is_empty() {
                        title = cand.chars().take(200).collect();
                    }
                }
            }
            "request/header" => {
                let h = entry.get("data").and_then(|d| d.get("header"));
                if let Some(cfg) = h.and_then(|v| v.get("config")) {
                    provider = cfg.get("provider").and_then(Value::as_str).map(|s| s.to_string()).or(provider);
                    model_id = cfg.get("model").and_then(Value::as_str).map(|s| s.to_string()).or(model_id);
                }
            }
            "request/context" => {
                let d = entry.get("data");
                provider = d.and_then(|v| v.get("provider")).and_then(Value::as_str).map(|s| s.to_string()).or(provider);
                model_id = d.and_then(|v| v.get("model")).and_then(Value::as_str).map(|s| s.to_string()).or(model_id);
            }
            "user/message" => {
                message_count += 1;
                user_count += 1;
                if title.is_empty() {
                    let txt = entry.get("data").and_then(|d| d.get("content")).and_then(Value::as_array)
                        .map(|arr| arr.iter().filter_map(|p| if p.get("type").and_then(Value::as_str)==Some("text") { p.get("text").and_then(Value::as_str) } else { None }).collect::<Vec<_>>().join(" "))
                        .unwrap_or_default();
                    let compact = txt.split_whitespace().collect::<Vec<_>>().join(" ");
                    if !compact.is_empty() {
                        title = compact.chars().take(200).collect();
                    }
                }
            }
            "assistant/message" => {
                message_count += 1;
                assistant_count += 1;
                let d = entry.get("data");
                let msg = d.and_then(|v| v.get("message"));
                if let Some(src) = msg.and_then(|m| m.get("source")) {
                    provider = src.get("provider").and_then(Value::as_str).map(|s| s.to_string()).or(provider);
                    model_id = src.get("model").and_then(Value::as_str).map(|s| s.to_string()).or(model_id);
                }
                if let Some(parts) = msg.and_then(|m| m.get("content")).and_then(Value::as_array) {
                    tool_call_count += parts.iter().filter(|p| p.get("type").and_then(Value::as_str)==Some("tool-call")).count() as u64;
                }
                if let Some(u) = d.and_then(|v| v.get("usage")) {
                    tokens_input += u.get("inputTokens").and_then(Value::as_u64).or_else(|| u.get("input").and_then(Value::as_u64)).unwrap_or(0);
                    tokens_output += u.get("outputTokens").and_then(Value::as_u64).or_else(|| u.get("output").and_then(Value::as_u64)).unwrap_or(0);
                    tokens_reasoning += u.get("reasoning").and_then(Value::as_u64).unwrap_or(0);
                    tokens_cache_read += u.get("cacheReadTokens").and_then(Value::as_u64).or_else(|| u.get("cacheRead").and_then(Value::as_u64)).unwrap_or(0);
                    tokens_cache_write += u.get("cacheWriteTokens").and_then(Value::as_u64).or_else(|| u.get("cacheWrite").and_then(Value::as_u64)).unwrap_or(0);
                    cost += u.get("cost").and_then(|c| c.get("total")).and_then(Value::as_f64).unwrap_or(0.0);
                }
            }
            "tool/call" => {
                tool_call_count += 1;
            }
            "tool/result" => {
                tool_result_count += 1;
            }
            _ => {}
        }
    }
    if title.is_empty() {
        let short = bare.get(..8).unwrap_or(bare);
        title = format!("dsh {}", short);
    }
    let model = model_id.clone();
    let model_provider = provider.clone();
    Some(SessionInfo {
        id: bare.to_string(),
        title,
        status: status_from_updated(updated),
        agent: "dsh".into(),
        source: "fs".into(),
        path: path.to_string_lossy().to_string(),
        directory: cwd.clone(),
        worktree: derive_worktree(&cwd),
        created,
        updated,
        model_id,
        model_provider,
        model,
        provider,
        tokens_input,
        tokens_output,
        tokens_reasoning,
        tokens_cache_read,
        tokens_cache_write,
        cost,
        message_count,
        user_message_count: user_count,
        assistant_message_count: assistant_count,
        tool_result_count,
        tool_call_count,
        thinking_level,
    })
}

fn text_from_user_message(msg: &Value) -> String {
    if msg.get("role").and_then(Value::as_str) != Some("user") {
        return String::new();
    }
    let mut out = String::new();
    if let Some(parts) = msg.get("content").and_then(Value::as_array) {
        for part in parts {
            if part.get("type").and_then(Value::as_str) == Some("text") {
                if let Some(text) = part.get("text").and_then(Value::as_str) {
                    if !out.is_empty() {
                        out.push(' ');
                    }
                    out.push_str(text.trim());
                }
            }
        }
    }
    let compact = out.split_whitespace().collect::<Vec<_>>().join(" ");
    compact.chars().take(200).collect()
}

fn status_from_updated(updated: i64) -> String {
    let age = now_ms() - updated;
    if age < 5 * 60_000 {
        "running".into()
    } else if age < 24 * 60 * 60_000 {
        "idle".into()
    } else {
        "stale".into()
    }
}

fn derive_worktree(cwd: &str) -> String {
    Path::new(cwd)
        .file_name()
        .and_then(|v| v.to_str())
        .unwrap_or(cwd)
        .to_string()
}
