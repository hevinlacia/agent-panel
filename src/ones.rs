//! Role: ONES 任务候选与推荐 — 复用 browser_auth 站点代理（Chrome 登录态）聚合
//! 「消息通知(notices) + 工时报表(manhour GraphQL)」两类信号源，为需求关联
//! ONES 任务提供候选列表与按标题匹配的推荐。
//! Public surface: api_ones_tasks（GET /api/ones/tasks?reqId=&team=）、
//! recommend_ones_tasks / merge_notices / merge_buckets（含单测）。
//! Constraints: 只读代理请求；不落盘、不返回任何 cookie/token；站点必须在
//! browserAuth.sites 白名单（id=ones）；推荐只做标题相似度 + 编号直引加权。
//! Read-this-with: src/browser_auth.rs（load_chrome_cookies/send_site_request）、
//! src/requirement_index.rs（get_real_requirement）。

use std::cmp::Ordering;
use std::collections::HashMap;

use axum::{
    extract::{Query, State},
    Json,
};
use anyhow::anyhow;
use chrono::Duration;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::*;

pub(crate) const ONES_SITE_ID: &str = "ones";
pub(crate) const DEFAULT_ONES_TEAM: &str = "5BXYuw3B";
const NOTICE_LIMIT: usize = 200;
/// 工时报表回看窗口：足够覆盖近期活跃任务，又不至于拖慢响应。
const MANHOUR_WINDOW_DAYS: i64 = 120;
const MAX_RECOMMENDATIONS: usize = 8;
/// 低于该分数的候选不进入推荐列表（纯噪声过滤）。
const MIN_RECOMMEND_SCORE: f64 = 0.10;
/// 需求文本（meta/notes/背景）里已引用任务编号时的加权：最强直接信号。
const DISPLAY_ID_BOOST: f64 = 0.35;
/// 单个需求文本文件的读取上限（字符），防止超大文档拖慢推荐。
const REQ_TEXT_FILE_CAP: usize = 100_000;

/// 候选任务：去重后的一条 ONES 工作项信号。
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct OnesTaskCandidate {
    pub(crate) display_id: String,
    pub(crate) name: String,
    pub(crate) project: String,
    pub(crate) task_uuid: String,
    /// 信号来源：notice（消息通知）/ manhour（登记过工时）。
    pub(crate) sources: Vec<String>,
    /// 最近活动时间（毫秒）；manhour 来源无时间则为 0。
    pub(crate) last_activity_at: i64,
}

impl OnesTaskCandidate {
    pub(crate) fn issue_url(&self, team: &str) -> String {
        format!(
            "https://ones.jtexpress.com.cn/project/#/team/{team}/issue/{}",
            self.display_id
        )
    }

    /// 生成可直接写入 meta.md ones 字段的文本："编号 标题 URL"。
    /// 与 parse_ones_ref 的识别规则（/issue/ 提取 label）兼容。
    pub(crate) fn ref_text(&self, team: &str) -> String {
        format!("{} {} {}", self.display_id, self.name, self.issue_url(team))
    }

    pub(crate) fn to_json(&self, team: &str) -> Value {
        json!({
            "displayId": self.display_id,
            "name": self.name,
            "project": self.project,
            "taskUuid": self.task_uuid,
            "sources": self.sources,
            "lastActivityAt": self.last_activity_at,
            "url": self.issue_url(team),
            "refText": self.ref_text(team),
        })
    }
}

#[derive(Deserialize)]
pub(crate) struct OnesTasksQuery {
    #[serde(rename = "reqId")]
    pub(crate) req_id: Option<String>,
    pub(crate) team: Option<String>,
}

/// GET /api/ones/tasks?reqId=<id>&team=<teamId>
/// 无 reqId 时返回候选列表；带 reqId 时按需求标题推荐（recommendations）。
pub(crate) async fn api_ones_tasks(
    State(state): State<AppState>,
    Query(q): Query<OnesTasksQuery>,
) -> ApiResult<Json<Value>> {
    let cfg = read_config(&state).await?;
    let auth = normalize_browser_auth_config(cfg.browser_auth);
    let site = auth
        .sites
        .iter()
        .find(|s| s.id == ONES_SITE_ID)
        .ok_or_else(|| {
            ApiError::bad_request(format!(
                "browserAuth.sites 未配置 id={ONES_SITE_ID} 的站点；请在配置中添加（baseUrl=https://ones.jtexpress.com.cn，cookieDomains/allowedHosts=ones.jtexpress.com.cn）"
            ))
        })?;
    if !site.enabled {
        return Err(ApiError::bad_request(format!(
            "auth site disabled: {ONES_SITE_ID}"
        )));
    }
    let team = q
        .team
        .filter(|t| !t.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_ONES_TEAM.to_string());
    let cookies = load_chrome_cookies(&auth)
        .await
        .map_err(|e| ApiError::from(anyhow!("Chrome 登录态读取失败: {e:#}")))?
        .1;

    let (mut candidates, warnings) = fetch_ones_candidates(site, &cookies, &team).await;
    candidates.sort_by(sort_candidates);

    let mut recommendations: Vec<Value> = Vec::new();
    let mut requirement_title = String::new();
    let mut warnings = warnings;
    if let Some(req_id) = q.req_id.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        match get_real_requirement(&state, req_id).await {
            Ok(req) => {
                requirement_title = req.title.clone();
                let haystack = build_requirement_haystack(&req).await;
                recommendations =
                    recommend_ones_tasks(&haystack, &req.title, &candidates, &team);
            }
            Err(err) => warnings.push(format!("需求 {req_id} 读取失败: {err:#}")),
        }
    }

    Ok(Json(json!({
        "generatedAt": now_ms(),
        "team": team,
        "count": candidates.len(),
        "candidates": candidates.iter().map(|c| c.to_json(&team)).collect::<Vec<_>>(),
        "recommendations": recommendations,
        "requirementTitle": requirement_title,
        "warnings": warnings,
    })))
}

/// 从 notices + manhour GraphQL 拉取候选；单一信号源失败降级为 warning，
/// 不影响另一信号源（任何一源可用即返回结果）。
async fn fetch_ones_candidates(
    site: &BrowserAuthSiteConfig,
    cookies: &[ChromeCookie],
    team: &str,
) -> (Vec<OnesTaskCandidate>, Vec<String>) {
    let mut map: HashMap<String, OnesTaskCandidate> = HashMap::new();
    let mut warnings: Vec<String> = Vec::new();

    let notices_path = format!("/project/api/project/team/{team}/notices?type=1&limit={NOTICE_LIMIT}");
    match send_site_request(site, "GET", &notices_path, HashMap::new(), None, None, cookies).await {
        Ok(resp) if resp.status == 200 => {
            if let Some(data) = resp.body_json {
                merge_notices(&mut map, &data);
            }
        }
        Ok(resp) if resp.status == 401 || resp.status == 403 => {
            warnings.push("ONES 登录态已过期（通知接口 401），请在 Chrome 中刷新 ONES 页面后重试".into());
        }
        Ok(resp) => warnings.push(format!("notices 请求返回 {}", resp.status)),
        Err(err) => warnings.push(format!("notices 请求失败: {err:#}")),
    }

    match send_site_request(
        site,
        "POST",
        &graphql_path(team),
        graphql_headers(),
        None,
        Some(graphql_body(team)),
        cookies,
    )
    .await
    {
        Ok(resp) if resp.status == 200 => {
            if let Some(data) = resp.body_json {
                if let Some(errors) = data.get("errors") {
                    warnings.push(format!(
                        "manhour GraphQL 返回错误: {}",
                        serde_json::to_string(errors).unwrap_or_default()
                    ));
                } else {
                    merge_buckets(&mut map, &data);
                }
            }
        }
        Ok(resp) if resp.status == 401 || resp.status == 403 => {
            warnings.push("ONES 登录态已过期（工时报表 401），请在 Chrome 中刷新 ONES 页面后重试".into());
        }
        Ok(resp) => warnings.push(format!("manhour GraphQL 请求返回 {}", resp.status)),
        Err(err) => warnings.push(format!("manhour GraphQL 请求失败: {err:#}")),
    }

    (map.into_values().collect(), warnings)
}

fn graphql_path(team: &str) -> String {
    format!("/project/api/project/team/{team}/items/graphql?t=report-data__workspace_manhour-{team}")
}

fn graphql_headers() -> HashMap<String, String> {
    HashMap::from([
        ("content-type".into(), "application/json;charset=UTF-8".into()),
        ("accept".into(), "application/json, text/plain, */*".into()),
    ])
}

/// ONES GraphQL 工时报表查询：owner=$currentUser 服务端解析，
/// timeSeries 按天分布（聚合阶段只用 bucket 的任务信息，不用数值）。
const MANHOUR_GQL_QUERY: &str = r#"query QUERY_MANHOURS($groupBy: GroupBy, $orderBy: OrderBy, $timeSeries: TimeSeriesArgs, $actualHoursSum: String, $filter: Filter, $columnSource: Source) {
  buckets(groupBy: $groupBy, orderBy: $orderBy, filter: $filter) {
    ...ColumnBucketFragment
  }
}

fragment TaskSimple on Task {
  key
  uuid
  name
  displayId
  number
  project {
    uuid
  }
}

fragment ColumnBucketFragment on Bucket {
  key
  columnField: aggregateTask(source: $columnSource) {
    ...TaskSimple
  }
  actualHours(sum: $actualHoursSum)
  actualHoursSeries(timeSeries: $timeSeries) {
    times
    values
  }
}"#;

fn graphql_body(team: &str) -> String {
    let today = chrono::Local::now().date_naive();
    let from = (today - Duration::days(MANHOUR_WINDOW_DAYS)).format("%Y-%m-%d");
    let to = today.format("%Y-%m-%d");
    let variables = json!({
        "groupBy": { "manhours": { "task": {} } },
        "orderBy": { "aggregateTask": { "createTime": "DESC" } },
        "filter": {
            "manhours": {
                "task_notIn": [null],
                "startTime_range": { "unit": "day", "from": from.to_string(), "to": to.to_string() },
                "owner_in": ["$currentUser"],
            },
            "actualHours_notIn": [0],
        },
        "actualHoursSum": "manhours.recordedHour",
        "timeSeries": {
            "timeField": "manhours.startTime",
            "valueField": "manhours.recordedHour",
            "unit": "day",
            "from": from.to_string(),
            "to": to.to_string(),
        },
        "columnSource": "task",
        "_team": team,
    });
    let _ = team;
    json!({ "query": MANHOUR_GQL_QUERY, "variables": variables }).to_string()
}

/// notices 是消息流：同一任务可能多条，去重保留最近一条活动时间。
fn merge_notices(map: &mut HashMap<String, OnesTaskCandidate>, data: &Value) {
    let Some(notices) = data.get("notices").and_then(Value::as_array) else {
        return;
    };
    for n in notices {
        let Some(display_id) = n.get("display_id").and_then(Value::as_str) else {
            continue;
        };
        let task_uuid = n
            .get("task_uuid")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let msg = n.get("message").cloned().unwrap_or(Value::Null);
        let name = msg
            .get("object_name")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let project = msg
            .get("ref_name")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        // ONES send_time 为微秒，统一成毫秒。
        let ts_ms = msg
            .get("send_time")
            .and_then(Value::as_i64)
            .map(|us| us / 1_000)
            .unwrap_or(0);
        merge_candidate(map, display_id, &name, &project, task_uuid, "notice", ts_ms);
    }
}

/// 工时报表 bucket：columnField 即任务对象（displayId/name/uuid/project.uuid）。
fn merge_buckets(map: &mut HashMap<String, OnesTaskCandidate>, data: &Value) {
    let Some(buckets) = data.pointer("/data/buckets").and_then(Value::as_array) else {
        return;
    };
    for b in buckets {
        let Some(col) = b.get("columnField") else { continue };
        let Some(display_id) = col.get("displayId").and_then(Value::as_str) else {
            continue;
        };
        let name = col
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let task_uuid = col
            .get("uuid")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        merge_candidate(map, display_id, &name, "", &task_uuid, "manhour", 0);
    }
}

fn merge_candidate(
    map: &mut HashMap<String, OnesTaskCandidate>,
    display_id: &str,
    name: &str,
    project: &str,
    task_uuid: &str,
    source: &str,
    last_activity_at: i64,
) {
    let key = display_id.trim().to_uppercase();
    if key.is_empty() {
        return;
    }
    let entry = map
        .entry(key)
        .or_insert_with(|| OnesTaskCandidate {
            display_id: display_id.trim().to_string(),
            name: String::new(),
            project: String::new(),
            task_uuid: String::new(),
            sources: Vec::new(),
            last_activity_at: 0,
        });
    if entry.name.is_empty() {
        entry.name = name.trim().to_string();
    }
    if entry.project.is_empty() {
        entry.project = project.trim().to_string();
    }
    if entry.task_uuid.is_empty() {
        entry.task_uuid = task_uuid.trim().to_string();
    }
    if !entry.sources.contains(&source.to_string()) {
        entry.sources.push(source.to_string());
        entry.sources.sort();
    }
    entry.last_activity_at = entry.last_activity_at.max(last_activity_at);
}

fn sort_candidates(a: &OnesTaskCandidate, b: &OnesTaskCandidate) -> Ordering {
    b.last_activity_at
        .cmp(&a.last_activity_at)
        .then_with(|| natural_key(&b.display_id).cmp(&natural_key(&a.display_id)))
}

/// displayId 尾部数字按数值比较（JTYC-1348129 > JTYC-999999）。
fn natural_key(display_id: &str) -> (String, u64) {
    match display_id.rsplit_once('-') {
        Some((prefix, num)) => (
            prefix.to_string(),
            num.parse::<u64>().unwrap_or(0),
        ),
        None => (display_id.to_string(), 0),
    }
}

/// 组装参与匹配的需求文本：id + 标题 + 描述 + meta/notes/背景 文档（截断）。
/// 文本只用于"编号已被引用"检测，不做全文相似度。
async fn build_requirement_haystack(req: &Requirement) -> String {
    let mut haystack = format!("{} {}", req.id, req.title);
    if !req.description.is_empty() {
        haystack.push(' ');
        haystack.push_str(&req.description);
    }
    for path in [&req.meta_path, &req.notes_path, &req.background_path] {
        let Some(path) = path else { continue };
        if let Ok(text) = fs::read_to_string(path).await {
            let capped: String = text.chars().take(REQ_TEXT_FILE_CAP).collect();
            haystack.push(' ');
            haystack.push_str(&capped);
        }
    }
    haystack
}

/// 推荐：标题字符 bigram Dice 相似度 + 编号直引加权；过滤低分后取 Top N。
pub(crate) fn recommend_ones_tasks(
    haystack: &str,
    title: &str,
    candidates: &[OnesTaskCandidate],
    team: &str,
) -> Vec<Value> {
    let title_norm = normalize_for_match(title);
    let title_bigrams = bigrams(&title_norm);
    let mut scored: Vec<(f64, bool, &OnesTaskCandidate)> = candidates
        .iter()
        .filter_map(|c| {
            let mut score = dice(&title_bigrams, &bigrams(&normalize_for_match(&c.name)));
            let mut direct = false;
            if !c.display_id.is_empty() && haystack.contains(&c.display_id) {
                score += DISPLAY_ID_BOOST;
                direct = true;
            }
            (score >= MIN_RECOMMEND_SCORE).then_some((score, direct, c))
        })
        .collect();
    scored.sort_by(|a, b| {
        b.0.partial_cmp(&a.0)
            .unwrap_or(Ordering::Equal)
            .then_with(|| sort_candidates(a.2, b.2))
    });
    scored
        .into_iter()
        .take(MAX_RECOMMENDATIONS)
        .map(|(score, direct, c)| {
            json!({
                "displayId": c.display_id,
                "name": c.name,
                "project": c.project,
                "url": c.issue_url(team),
                "refText": c.ref_text(team),
                "sources": c.sources,
                "score": (score * 100.0).round() / 100.0,
                "displayIdBoost": direct,
            })
        })
        .collect()
}

/// 匹配归一化：小写 + 仅保留字母/数字/CJK（is_alphanumeric 覆盖汉字）。
fn normalize_for_match(text: &str) -> String {
    text.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(|c| c.to_lowercase())
        .collect()
}

/// 相邻字符 bigram 集合（中文友好，无需分词）。
fn bigrams(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() < 2 {
        return Vec::new();
    }
    chars.windows(2).map(|w| w.iter().collect()).collect()
}

/// Dice 系数：2×|A∩B| / (|A|+|B|)；任一侧空集返回 0。
fn dice(a: &[String], b: &[String]) -> f64 {
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    let mut b_counts: HashMap<&String, usize> = HashMap::new();
    for g in b {
        *b_counts.entry(g).or_default() += 1;
    }
    let mut inter = 0_usize;
    for g in a {
        if let Some(count) = b_counts.get_mut(g) {
            if *count > 0 {
                *count -= 1;
                inter += 1;
            }
        }
    }
    2.0 * inter as f64 / (a.len() + b.len()) as f64
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn candidate(display_id: &str, name: &str) -> OnesTaskCandidate {
        OnesTaskCandidate {
            display_id: display_id.into(),
            name: name.into(),
            project: String::new(),
            task_uuid: String::new(),
            sources: vec!["notice".into()],
            last_activity_at: 0,
        }
    }

    #[test]
    fn normalize_keeps_cjk_and_digits() {
        assert_eq!(
            normalize_for_match("【重构迭代】订单查询（增加条件）#12"),
            "重构迭代订单查询增加条件12"
        );
    }

    #[test]
    fn dice_identical_is_one() {
        let g = bigrams(&normalize_for_match("上架策略新增指定库位"));
        assert!((dice(&g, &g) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn dice_disjoint_is_zero() {
        let a = bigrams(&normalize_for_match("库存快照"));
        let b = bigrams(&normalize_for_match("波次导出"));
        assert!(dice(&a, &b) < 1e-9);
    }

    #[test]
    fn recommend_boosts_referenced_display_id() {
        let cands = vec![
            candidate("JTYC-111", "完全无关的任务"),
            candidate("JTYC-1347475", "新版波次分析增加特征值展示列"),
        ];
        let req_title = "WMS-123-波次分析特征值展示列";
        // haystack 里已引用编号：应排第一且 displayIdBoost=true
        let recs = recommend_ones_tasks(
            "WMS-123-波次分析特征值展示列 refs JTYC-1347475",
            req_title,
            &cands,
            "5BXYuw3B",
        );
        assert_eq!(recs[0]["displayId"], "JTYC-1347475");
        assert_eq!(recs[0]["displayIdBoost"], true);
        assert!(recs[0]["refText"]
            .as_str()
            .unwrap()
            .contains("/project/#/team/5BXYuw3B/issue/JTYC-1347475"));
    }

    #[test]
    fn recommend_filters_low_score() {
        let cands = vec![candidate("JTYC-222", "库存快照修复")];
        let recs = recommend_ones_tasks("x", "波次导出", &cands, "5BXYuw3B");
        assert!(recs.is_empty());
    }

    #[test]
    fn merge_notices_dedup_keeps_latest() {
        let mut map = HashMap::new();
        let data = json!({ "notices": [
            { "task_uuid": "T1", "display_id": "JTYC-1", "message": { "object_name": "任务A", "ref_name": "项目P", "send_time": 1_000_000 } },
            { "task_uuid": "T1", "display_id": "JTYC-1", "message": { "object_name": "任务A", "ref_name": "项目P", "send_time": 3_000_000 } }
        ] });
        merge_notices(&mut map, &data);
        assert_eq!(map.len(), 1);
        let c = map.values().next().unwrap();
        assert_eq!(c.last_activity_at, 3_000);
        assert_eq!(c.sources, vec!["notice"]);
    }

    #[test]
    fn merge_buckets_unifies_with_notice() {
        let mut map = HashMap::new();
        merge_notices(
            &mut map,
            &json!({ "notices": [ { "task_uuid": "T1", "display_id": "JTYC-1", "message": { "object_name": "任务A", "ref_name": "项目P", "send_time": 1_000_000 } } ] }),
        );
        let data = json!({ "data": { "buckets": [ { "columnField": { "displayId": "JTYC-1", "name": "任务A", "uuid": "T1", "project": { "uuid": "P1" } } } ] } });
        merge_buckets(&mut map, &data);
        let c = map.values().next().unwrap();
        assert_eq!(c.sources, vec!["manhour", "notice"]);
        assert_eq!(c.project, "项目P");
    }

    #[test]
    fn sort_candidates_natural_desc() {
        let mut v = vec![
            candidate("JTYC-999", "a"),
            candidate("JTYC-1348129", "b"),
        ];
        v[0].last_activity_at = 0;
        v[1].last_activity_at = 0;
        v.sort_by(sort_candidates);
        assert_eq!(v[0].display_id, "JTYC-1348129");
    }
}
