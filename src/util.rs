use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{anyhow, Result};
use regex::Regex;
use serde::Serialize;
use serde_json::{json, Value};
use tokio::{fs, process::Command};

use crate::*;

pub(crate) fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

pub(crate) fn system_time_to_ms(value: SystemTime) -> i64 {
    value
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

pub(crate) fn home_dir() -> Result<PathBuf> {
    dirs::home_dir().ok_or_else(|| anyhow!("cannot resolve home directory"))
}

pub(crate) fn clean_required(value: &str, field: &str) -> ApiResult<String> {
    clean_optional(Some(value)).ok_or_else(|| ApiError::bad_request(format!("missing {field}")))
}

pub(crate) fn clean_required_opt(value: Option<&str>, field: &str) -> ApiResult<String> {
    clean_optional(value).ok_or_else(|| ApiError::bad_request(format!("missing {field}")))
}

pub(crate) fn clean_optional(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_string)
}

pub(crate) fn ensure_date_or_unknown(value: &str, field: &str) -> ApiResult<()> {
    if value == "unknown" || chrono::NaiveDate::parse_from_str(value, "%Y-%m-%d").is_ok() {
        Ok(())
    } else {
        Err(ApiError::bad_request(format!(
            "{field} must be YYYY-MM-DD or unknown"
        )))
    }
}

pub(crate) fn ensure_text_size(value: &str, field: &str) -> ApiResult<()> {
    if value.len() > 300_000 {
        Err(ApiError::bad_request(format!("{field} is too large")))
    } else {
        Ok(())
    }
}

pub(crate) fn today_ymd() -> String {
    chrono::Utc::now()
        .date_naive()
        .format("%Y-%m-%d")
        .to_string()
}

pub(crate) fn split_list(value: Option<&String>) -> Vec<String> {
    value
        .map(|v| {
            v.split([',', '，'])
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

pub(crate) fn value_to_list(value: Option<&Value>) -> Vec<String> {
    match value {
        Some(Value::Array(arr)) => arr
            .iter()
            .filter_map(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect(),
        Some(Value::String(s)) => s
            .split([',', '，'])
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect(),
        _ => Vec::new(),
    }
}

pub(crate) fn value_to_path(value: Option<&Value>) -> Option<Vec<String>> {
    match value {
        Some(Value::Array(arr)) => Some(
            arr.iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .collect(),
        ),
        Some(Value::String(s)) => Some(
            s.split('/')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .collect(),
        ),
        _ => None,
    }
}

pub(crate) fn unique_strings(values: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for value in values {
        let clean = value.trim().to_string();
        if !clean.is_empty() && seen.insert(clean.clone()) {
            out.push(clean);
        }
    }
    out
}

pub(crate) fn append_group(group: &[String], value: String) -> Vec<String> {
    let mut out = group.to_vec();
    out.push(value);
    out
}

pub(crate) fn normalize_status(value: Option<&String>) -> Option<String> {
    value.and_then(|v| normalize_status_value(v))
}

pub(crate) fn normalize_status_value(value: &str) -> Option<String> {
    let raw = value.trim();
    if let Some((_, canonical)) = REQ_STATUS_ALIASES.iter().find(|(alias, _)| *alias == raw) {
        return Some((*canonical).to_string());
    }
    if REQ_STATUSES.contains(&raw) {
        Some(raw.to_string())
    } else {
        None
    }
}

pub(crate) fn canonical_status(value: &str) -> ApiResult<String> {
    normalize_status_value(value)
        .ok_or_else(|| ApiError::bad_request(format!("invalid status: {value}")))
}

pub(crate) fn should_enforce_review_gate_for_status(
    current_status: &str,
    target_status: &str,
) -> bool {
    // Keep the original status-transition gate: only block when entering 测试中.
    // If a requirement is already 测试中 and code changes, review_gate_decision detects
    // stale code-review snapshots by comparing reviewed targetCommit with current HEAD.
    current_status != "测试中" && target_status == "测试中"
}

pub(crate) fn normalize_category(value: Option<&String>) -> Option<String> {
    let raw = value?.trim();
    if REQ_CATEGORIES.contains(&raw) {
        Some(raw.to_string())
    } else {
        None
    }
}

pub(crate) fn ensure_status(value: &str) -> ApiResult<()> {
    canonical_status(value).map(|_| ())
}

pub(crate) fn ensure_category(value: &str) -> ApiResult<()> {
    if REQ_CATEGORIES.contains(&value) {
        Ok(())
    } else {
        Err(ApiError::bad_request(format!("invalid category: {value}")))
    }
}

pub(crate) fn parse_date_ms(value: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(value)
        .map(|d| d.timestamp_millis())
        .ok()
        .or_else(|| {
            chrono::NaiveDate::parse_from_str(&value.replace('/', "-"), "%Y-%m-%d")
                .ok()
                .and_then(|d| d.and_hms_opt(0, 0, 0))
                .map(|d| d.and_utc().timestamp_millis())
        })
}

pub(crate) async fn read_json_if_exists(path: &Path) -> Option<Value> {
    let raw = fs::read_to_string(path).await.ok()?;
    serde_json::from_str(&raw).ok()
}

pub(crate) fn path_if_exists(path: PathBuf) -> Option<String> {
    if path.exists() {
        Some(path.to_string_lossy().to_string())
    } else {
        None
    }
}

pub(crate) fn parse_ones_ref(raw: &str) -> Option<Value> {
    let value = raw.trim();
    if value.is_empty() {
        return None;
    }
    // A pasted value may carry extra text around the ONES link
    // (e.g. "JTYC-1347611 上架策略新增指定库位 https://.../issue/JTYC-1347611"),
    // so search for the first http(s) URL anywhere instead of requiring it at the start.
    if let Some(url) = Regex::new(r#"https?://[^\s<>"']+"#)
        .ok()
        .and_then(|re| re.find(value).map(|m| m.as_str().to_string()))
    {
        let label = Regex::new(r"(?:^|/)issue/([^/?#]+)")
            .ok()
            .and_then(|re| {
                re.captures(&url)
                    .and_then(|c| c.get(1).map(|m| m.as_str().to_string()))
            })
            .unwrap_or_else(|| url.rsplit('/').next().unwrap_or(&url).to_string());
        Some(json!({ "raw": value, "url": url, "label": label }))
    } else {
        Some(json!({ "raw": value, "url": null, "label": value }))
    }
}

pub(crate) async fn atomic_write_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let text = serde_json::to_string_pretty(value)? + "\n";
    atomic_write_text(path, &text).await
}

pub(crate) async fn atomic_write_text(path: &Path, text: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).await?;
    }
    let tmp = path.with_extension(format!("tmp.{}.{}", std::process::id(), now_ms()));
    fs::write(&tmp, text).await?;
    fs::rename(tmp, path).await?;
    Ok(())
}

pub(crate) fn shell_quote(value: &str) -> String {
    if value
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || "-_./:@".contains(c))
    {
        value.to_string()
    } else {
        format!("'{}'", value.replace('\'', "'\\''"))
    }
}

#[allow(dead_code)]
pub(crate) async fn run_command(program: &str, args: &[&str]) -> Result<String> {
    let output = Command::new(program).args(args).output().await?;
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}
