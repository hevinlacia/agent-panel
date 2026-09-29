//! Role: ONES 工时联动 — 需求工时档案（ones-manhour.json）读写 + ONES 已登记工时
//! 匹配（复用工时报表缓存）+ 工时录入（写入接口未验证前为准备模式）。
//! Public surface: api_ones_manhour（GET /api/ones/manhour）、api_ones_manhour_save
//! （POST /api/ones/manhour/save）、api_ones_manhour_log（POST /api/ones/manhour/log）、
//! extract_display_id / read_ones_manhour_file（含单测）。
//! Constraints: 只写需求目录内 ones-manhour.json；ONES 侧仅只读（复用
//! ensure_ones_candidates 的工时报表缓存，窗口 120 天）；工时写入单函数接缝
//! push_manhour_to_ones 未验证前返回明确错误，不产生半写状态；不落盘、
//! 不返回任何 cookie/token。
//! Read-this-with: src/ones.rs（候选与工时报表缓存）、src/browser_auth.rs、
//! src/requirement_index.rs（get_real_requirement / Requirement.ones_manhour）、
//! src/util.rs（parse_ones_ref / atomic_write_json）。

use std::path::{Path, PathBuf};

use anyhow::{anyhow, Result};
use axum::{extract::{Query, State}, Json};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::*;

/// 需求工时档案文件名（需求目录内，agent 可直接读写）。
pub(crate) const MANHOUR_FILE: &str = "ones-manhour.json";
/// 登记历史上限：防止文件无限增长。
const MAX_LOG_HISTORY: usize = 50;
/// 工时字段合法上限（小时），防误输入。
const MAX_HOURS: f64 = 24.0;

/// 需求工时档案：人工预估 / agent 实际 / 录入历史。
/// ONES 侧「已登记工时」为实时读取值，不落盘。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct OnesManhourFile {
    pub(crate) version: u8,
    /// 人工预估工时（用户自己觉得要多久）。
    pub(crate) manual_hours: Option<f64>,
    /// agent 实际完成工时（开发+测试+UAT 回归；MVP 手动维护，后续可按 session 时间线自动估算）。
    pub(crate) agent_hours: Option<f64>,
    pub(crate) agent_hours_note: Option<String>,
    /// 通过面板发起的登记记录（prepared/logged/failed）。
    pub(crate) log_history: Vec<OnesManhourLogEntry>,
    pub(crate) updated_at: i64,
}

impl Default for OnesManhourFile {
    fn default() -> Self {
        Self {
            version: 1,
            manual_hours: None,
            agent_hours: None,
            agent_hours_note: None,
            log_history: Vec::new(),
            updated_at: 0,
        }
    }
}

/// 一条通过面板发起的工时登记记录。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct OnesManhourLogEntry {
    pub(crate) date: String,
    pub(crate) hours: f64,
    #[serde(default)]
    pub(crate) remark: String,
    /// prepared = 已生成登记计划（写入接口未验证）；logged = 已写入 ONES；failed = 写入失败。
    pub(crate) status: String,
    pub(crate) created_at: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) task_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) message: Option<String>,
}

/// 从需求 meta 的 ones 字段提取任务编号（如 JTYC-1347611）。
/// 兼容纯编号 / 纯 URL / “编号 标题 URL”整段文本；URL 内编号同样能命中。
pub(crate) fn extract_display_id(ones: Option<&str>) -> Option<String> {
    let raw = ones?;
    if raw.trim().is_empty() {
        return None;
    }
    let label = parse_ones_ref(raw)
        .and_then(|v| {
            v.get("label")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .unwrap_or_default();
    let hay = format!("{label} {raw}");
    let re = Regex::new(r"(?i)([A-Z][A-Z0-9_]*-\d+)").ok()?;
    re.captures(&hay)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_uppercase())
}

/// 小时数保留两位小数。
fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

/// JSON null → Some(None)（清除），字段缺失 → None（不修改），值 → Some(Some(v))。
/// 标准 double_option 模式；无它时 serde_json 会把 null 与缺失都反序列化为外层 None。
fn double_option<'de, T, D>(de: D) -> Result<Option<Option<T>>, D::Error>
where
    T: Deserialize<'de>,
    D: serde::Deserializer<'de>,
{
    Deserialize::deserialize(de).map(Some)
}

/// 读取需求工时档案；缺失或损坏时返回默认值（不抛错，避免旧需求阻塞页面）。
pub(crate) async fn read_ones_manhour_file(dir: &str) -> OnesManhourFile {
    let path = Path::new(dir).join(MANHOUR_FILE);
    match fs::read_to_string(&path).await {
        Ok(text) => serde_json::from_str(&text).unwrap_or_default(),
        Err(_) => OnesManhourFile::default(),
    }
}

async fn write_ones_manhour_file(dir: &str, file: &OnesManhourFile) -> Result<()> {
    let path = PathBuf::from(dir).join(MANHOUR_FILE);
    atomic_write_json(&path, file).await
}

/// 从 Value（Requirement.ones_manhour / effort_estimate）取数字字段。
fn f64_field(v: Option<&Value>, key: &str) -> Option<f64> {
    v?.get(key).and_then(Value::as_f64).filter(|n| *n > 0.0)
}

fn validate_hours(hours: f64, field: &str) -> ApiResult<()> {
    if !hours.is_finite() || hours < 0.0 || hours > MAX_HOURS {
        return Err(ApiError::bad_request(format!(
            "{field} 非法：{hours}（需 0~{MAX_HOURS} 小时）"
        )));
    }
    Ok(())
}

fn validate_date(date: &str) -> ApiResult<()> {
    let ok = Regex::new(r"^\d{4}-\d{2}-\d{2}$")
        .map(|re| re.is_match(date))
        .unwrap_or(false);
    if ok {
        Ok(())
    } else {
        Err(ApiError::bad_request(format!(
            "日期格式非法：{date}（需 YYYY-MM-DD）"
        )))
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct OnesManhourQuery {
    pub(crate) id: Option<String>,
    #[serde(alias = "reqId")]
    pub(crate) req_id: Option<String>,
    pub(crate) team: Option<String>,
    pub(crate) refresh: Option<bool>,
}

/// GET /api/ones/manhour?reqId=<id>
/// 汇总需求工时四联：人工预估 / 自动预估（effort-estimate）/ ONES 已登记（实时）/
/// agent 实际，以及剩余工时与录入历史。
pub(crate) async fn api_ones_manhour(
    State(state): State<AppState>,
    Query(q): Query<OnesManhourQuery>,
) -> ApiResult<Json<Value>> {
    let req_id = q.id.or(q.req_id).unwrap_or_default();
    let req = get_real_requirement(&state, &req_id).await?;
    let team = q
        .team
        .filter(|t| !t.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_ONES_TEAM.to_string());
    let file = read_ones_manhour_file(req.req_dir.as_deref().unwrap_or_default()).await;
    let display_id = extract_display_id(req.ones.as_deref());
    let ones_ref = req.ones.as_deref().and_then(parse_ones_ref);

    let mut logged_hours: Option<f64> = None;
    let mut task_uuid: Option<String> = None;
    let mut task_name: Option<String> = None;
    let mut task_url: Option<String> = None;
    let mut warnings: Vec<String> = Vec::new();
    let mut fetched_at = 0_i64;
    let mut cache_hit = false;
    if let Some(did) = display_id.clone() {
        match ensure_ones_candidates(&state, &team, q.refresh.unwrap_or(false)).await {
            Ok(snap) => {
                fetched_at = snap.fetched_at;
                cache_hit = snap.cache_hit;
                warnings.extend(snap.warnings);
                if let Some(c) = snap
                    .candidates
                    .iter()
                    .find(|c| c.display_id.to_uppercase() == did)
                {
                    logged_hours = Some(round2(c.actual_hours_raw as f64 / 100_000.0));
                    task_uuid = (!c.task_uuid.is_empty()).then(|| c.task_uuid.clone());
                    task_name = (!c.name.is_empty()).then(|| c.name.clone());
                    task_url = Some(c.issue_url(&team));
                } else {
                    warnings.push(format!(
                        "ONES 工时报表窗口内未找到任务 {did}（可能无登记记录或已超出统计窗口）"
                    ));
                }
            }
            Err(err) => warnings.push(format!("ONES 候选获取失败: {:?}", err.message)),
        }
    }

    let manual_hours = file.manual_hours;
    let remaining_hours = match (manual_hours, logged_hours) {
        (Some(m), Some(l)) if m > 0.0 => Some(round2((m - l).max(0.0))),
        _ => None,
    };
    Ok(Json(json!({
        "reqId": req.id,
        "title": req.title,
        "status": req.status,
        "ones": ones_ref,
        "displayId": display_id,
        "taskUuid": task_uuid,
        "taskName": task_name,
        "taskUrl": task_url,
        "manualHours": manual_hours,
        "autoHours": f64_field(req.effort_estimate.as_ref(), "estimatedHours"),
        "autoHoursUpdatedAt": req.effort_estimate.as_ref().and_then(|v| v.get("updatedAt")).and_then(Value::as_i64),
        "loggedHours": logged_hours,
        "remainingHours": remaining_hours,
        "agentHours": file.agent_hours,
        "agentHoursNote": file.agent_hours_note,
        "logHistory": file.log_history,
        "manhourWindowDays": MANHOUR_WINDOW_DAYS,
        "onesFetchedAt": fetched_at,
        "cacheHit": cache_hit,
        "warnings": warnings,
    })))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct OnesManhourSaveForm {
    pub(crate) req_id: String,
    /// null = 清除；缺失 = 不修改；数值 = 设置。
    #[serde(default, deserialize_with = "double_option")]
    pub(crate) manual_hours: Option<Option<f64>>,
    #[serde(default, deserialize_with = "double_option")]
    pub(crate) agent_hours: Option<Option<f64>>,
    #[serde(default, deserialize_with = "double_option")]
    pub(crate) agent_hours_note: Option<Option<String>>,
}

/// POST /api/ones/manhour/save — 更新人工预估 / agent 实际工时（写 ones-manhour.json）。
pub(crate) async fn api_ones_manhour_save(
    State(state): State<AppState>,
    form: FormOrJson<OnesManhourSaveForm>,
) -> ApiResult<Json<Value>> {
    let body = form.0;
    let req = get_real_requirement(&state, &body.req_id).await?;
    let dir = req.req_dir.clone().ok_or_else(|| {
        ApiError::bad_request(format!("需求 {} 无目录，无法保存工时档案", req.id))
    })?;
    if let Some(Some(h)) = body.manual_hours {
        validate_hours(h, "manualHours")?;
    }
    if let Some(Some(h)) = body.agent_hours {
        validate_hours(h, "agentHours")?;
    }
    let mut file = read_ones_manhour_file(&dir).await;
    if let Some(v) = body.manual_hours {
        file.manual_hours = v.filter(|h| *h > 0.0);
    }
    if let Some(v) = body.agent_hours {
        file.agent_hours = v.filter(|h| *h > 0.0);
    }
    if let Some(v) = body.agent_hours_note {
        file.agent_hours_note = v.map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    }
    file.updated_at = now_ms();
    write_ones_manhour_file(&dir, &file).await?;
    Ok(Json(json!({ "ok": true, "manhour": file })))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct OnesManhourLogInput {
    pub(crate) date: String,
    pub(crate) hours: f64,
    #[serde(default)]
    pub(crate) remark: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct OnesManhourLogForm {
    pub(crate) req_id: String,
    pub(crate) entries: Vec<OnesManhourLogInput>,
    /// 准备模式（默认 false）：只生成登记清单与任务链接；true 时尝试写 ONES（当前必然失败）。
    pub(crate) execute: Option<bool>,
}

/// POST /api/ones/manhour/log — 工时录入。
/// 雏形为准备模式：校验 + 匹配 ONES 任务 + 生成登记清单（含任务链接），
/// 追加到 logHistory（prepared）；ONES 写入接口验证后 execute=true 走真实写入。
pub(crate) async fn api_ones_manhour_log(
    State(state): State<AppState>,
    form: FormOrJson<OnesManhourLogForm>,
) -> ApiResult<Json<Value>> {
    let body = form.0;
    if body.entries.is_empty() {
        return Err(ApiError::bad_request("entries 为空"));
    }
    for e in &body.entries {
        validate_hours(e.hours, "hours")?;
        if e.hours <= 0.0 {
            return Err(ApiError::bad_request("hours 需大于 0"));
        }
        validate_date(&e.date)?;
    }
    let req = get_real_requirement(&state, &body.req_id).await?;
    let dir = req.req_dir.clone().ok_or_else(|| {
        ApiError::bad_request(format!("需求 {} 无目录，无法记录工时登记", req.id))
    })?;
    let team = DEFAULT_ONES_TEAM.to_string();
    let display_id = extract_display_id(req.ones.as_deref())
        .ok_or_else(|| ApiError::bad_request(format!("需求 {} 未关联 ONES 任务", req.id)))?;

    // 登记执行需要 ONES 任务与登录态；准备模式只读缓存（无 Chrome 登录态也能生成清单）。
    let mut task_url: Option<String> = None;
    let mut task_uuid: Option<String> = None;
    let mut warnings: Vec<String> = Vec::new();
    match ensure_ones_candidates(&state, &team, false).await {
        Ok(snap) => {
            warnings.extend(snap.warnings);
            if let Some(c) = snap
                .candidates
                .iter()
                .find(|c| c.display_id.to_uppercase() == display_id)
            {
                task_url = Some(c.issue_url(&team));
                task_uuid = (!c.task_uuid.is_empty()).then(|| c.task_uuid.clone());
            }
        }
        Err(err) => warnings.push(format!(
            "ONES 候选获取失败（不影响生成登记清单）: {:?}",
            err.message
        )),
    }
    if task_url.is_none() {
        // 缓存未命中时仍给出可打开的任务页链接（编号即路由）。
        task_url = Some(format!(
            "https://ones.jtexpress.com.cn/project/#/team/{team}/issue/{display_id}"
        ));
    }

    let execute = body.execute.unwrap_or(false);
    let mut results: Vec<Value> = Vec::new();
    let mut history_entries: Vec<OnesManhourLogEntry> = Vec::new();
    for e in &body.entries {
        let (status, message) = if execute {
            let auth_cfg = read_config(&state).await?;
            let auth = normalize_browser_auth_config(auth_cfg.browser_auth);
            let site = auth
                .sites
                .iter()
                .find(|s| s.id == ONES_SITE_ID)
                .cloned();
            let outcome: Result<()> = async {
                let site = site.ok_or_else(|| {
                    anyhow!("browserAuth.sites 未配置 id=ones 的站点")
                })?;
                let cookies = load_chrome_cookies(&auth).await?.1;
                let uuid = task_uuid.clone().ok_or_else(|| {
                    anyhow!("未获取到任务 uuid（工时报表缓存中无 {display_id}），无法写入")
                })?;
                push_manhour_to_ones(&site, &cookies, &team, &uuid, e).await
            }
            .await;
            match outcome {
                Ok(()) => ("logged".to_string(), None),
                Err(err) => ("failed".to_string(), Some(format!("{err:#}"))),
            }
        } else {
            (
                "prepared".to_string(),
                Some("准备模式：ONES 写入接口待验证，请按清单在 ONES 任务页登记".to_string()),
            )
        };
        let entry = OnesManhourLogEntry {
            date: e.date.clone(),
            hours: e.hours,
            remark: e.remark.trim().to_string(),
            status: status.clone(),
            created_at: now_ms(),
            task_url: task_url.clone(),
            message: message.clone(),
        };
        results.push(json!({
            "date": entry.date,
            "hours": entry.hours,
            "remark": entry.remark,
            "status": status,
            "taskUrl": task_url,
            "message": message,
        }));
        history_entries.push(entry);
    }

    let mut file = read_ones_manhour_file(&dir).await;
    file.log_history.extend(history_entries);
    if file.log_history.len() > MAX_LOG_HISTORY {
        let drop = file.log_history.len() - MAX_LOG_HISTORY;
        file.log_history.drain(0..drop);
    }
    file.updated_at = now_ms();
    write_ones_manhour_file(&dir, &file).await?;

    Ok(Json(json!({
        "ok": true,
        "mode": if execute { "execute" } else { "prepared" },
        "reqId": req.id,
        "displayId": display_id,
        "results": results,
        "warnings": warnings,
        "note": "雏形：ONES 工时写入接口尚未抓包验证，execute=true 会返回失败原因；准备模式生成清单与任务链接。",
    })))
}

/// ONES 工时写入接缝：待抓包验证「登记工时」内部接口后实现（参照 ones-task-transit.md
/// 的 new_transit 模式走 browser_auth 代理）。未实现前返回 Err，调用方把条目标记为
/// failed，不产生半写状态。
async fn push_manhour_to_ones(
    _site: &BrowserAuthSiteConfig,
    _cookies: &[ChromeCookie],
    _team: &str,
    _task_uuid: &str,
    _entry: &OnesManhourLogInput,
) -> Result<()> {
    anyhow::bail!(
        "ONES 登记工时写入接口尚未验证：请在 ONES 任务页手动登记一条工时并抓包 \
         （DevTools → Copy as cURL），将请求样例补进 ones-api-call skill 后实现本函数"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_display_id_from_plain_id() {
        assert_eq!(
            extract_display_id(Some("JTYC-1347611")).as_deref(),
            Some("JTYC-1347611")
        );
    }

    #[test]
    fn extract_display_id_from_pasted_text_with_url() {
        let raw = "JTYC-1347611 上架策略新增指定库位 https://ones.jtexpress.com.cn/project/#/team/5BXYuw3B/issue/JTYC-1347611";
        assert_eq!(extract_display_id(Some(raw)).as_deref(), Some("JTYC-1347611"));
    }

    #[test]
    fn extract_display_id_from_url_only() {
        let raw = "https://ones.jtexpress.com.cn/project/#/team/5BXYuw3B/issue/JTYC-99";
        assert_eq!(extract_display_id(Some(raw)).as_deref(), Some("JTYC-99"));
    }

    #[test]
    fn extract_display_id_none_for_empty_or_missing() {
        assert!(extract_display_id(None).is_none());
        assert!(extract_display_id(Some("")).is_none());
        assert!(extract_display_id(Some("  ")).is_none());
        assert!(extract_display_id(Some("无编号文本")).is_none());
    }

    #[test]
    fn manhour_file_default_and_roundtrip() {
        let f = OnesManhourFile::default();
        assert_eq!(f.version, 1);
        assert!(f.manual_hours.is_none());
        let text = serde_json::to_string(&f).unwrap();
        let back: OnesManhourFile = serde_json::from_str(&text).unwrap();
        assert_eq!(back.version, 1);
    }

    #[test]
    fn log_entry_serializes_camel_case() {
        let e = OnesManhourLogEntry {
            date: "2026-02-09".into(),
            hours: 3.5,
            remark: "联调".into(),
            status: "prepared".into(),
            created_at: 1,
            task_url: Some("https://x".into()),
            message: None,
        };
        let v = serde_json::to_value(&e).unwrap();
        assert_eq!(v["date"], "2026-02-09");
        assert_eq!(v["taskUrl"], "https://x");
        assert!(v.get("message").is_none());
    }

    #[tokio::test]
    async fn read_missing_file_returns_default() {
        let tmp = tempfile::tempdir().unwrap();
        let f = read_ones_manhour_file(tmp.path().to_str().unwrap()).await;
        assert_eq!(f.version, 1);
        assert!(f.log_history.is_empty());
    }

    #[tokio::test]
    async fn save_and_reread_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().to_str().unwrap().to_string();
        let mut f = read_ones_manhour_file(&dir).await;
        f.manual_hours = Some(8.0);
        f.agent_hours = Some(6.5);
        write_ones_manhour_file(&dir, &f).await.unwrap();
        let back = read_ones_manhour_file(&dir).await;
        assert_eq!(back.manual_hours, Some(8.0));
        assert_eq!(back.agent_hours, Some(6.5));
    }
}
