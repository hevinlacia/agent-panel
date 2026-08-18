use std::{
    collections::{HashMap, HashSet},
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

pub(crate) async fn api_sessions(
    State(state): State<AppState>,
    Query(query): Query<IdQuery>,
) -> ApiResult<Json<Value>> {
    let sessions = scan_pi_sessions(&state, query.days).await?;
    let mut summary: HashMap<String, usize> = HashMap::new();
    for s in &sessions {
        *summary.entry(s.status.clone()).or_default() += 1;
    }
    Ok(Json(
        json!({ "summary": summary, "sessions": sessions, "harness": "pi", "days": query.days.unwrap_or(7) }),
    ))
}

pub(crate) async fn api_session(
    State(state): State<AppState>,
    Query(query): Query<IdQuery>,
) -> ApiResult<Json<Value>> {
    let id = query.id.unwrap_or_default();
    let session = scan_pi_sessions(&state, None)
        .await?
        .into_iter()
        .find(|s| s.id == id);
    Ok(Json(json!({ "session": session, "terminalRemoved": true })))
}

pub(crate) async fn api_session_log(
    State(state): State<AppState>,
    Query(query): Query<IdQuery>,
) -> ApiResult<Json<Value>> {
    let id = clean_required_opt(query.id.as_deref(), "id")?;
    let cursor = query.cursor.unwrap_or(0);
    let limit = query.limit.unwrap_or(80).clamp(1, 300);
    let Some(path) = find_pi_session_path(&state, &id).await? else {
        return Err(ApiError::bad_request(format!("session not found: {id}")));
    };
    let meta = fs::metadata(&path).await.ok();
    let updated_at = meta
        .as_ref()
        .and_then(|m| m.modified().ok())
        .map(system_time_to_ms)
        .unwrap_or(0);
    let raw = fs::read_to_string(&path).await.unwrap_or_default();
    let lines: Vec<&str> = raw.lines().filter(|line| !line.trim().is_empty()).collect();
    let total = lines.len();
    let start = cursor.min(total);
    let end = (start + limit).min(total);
    let entries: Vec<Value> = lines[start..end]
        .iter()
        .enumerate()
        .filter_map(|(idx, line)| parse_session_log_entry(start + idx, line))
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
    let found = scan_pi_sessions_filtered(&state, None, Some(&set)).await?;
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
