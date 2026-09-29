//! Role: WMS 接口测试联动 — 解析 testdata pack 的 api/catalog.yaml + .bru 请求模板，
//! 提供接口目录查询（含造数/测试标记与关键字过滤）与直接触发（login.mjs 会话注入 +
//! reqwest 代理发送，临时入参修改不写回模板）。
//! Public surface: api_apitest_apis（GET /api/apitest/apis）、api_apitest_api
//! （GET /api/apitest/api）、api_apitest_trigger（POST /api/apitest/trigger）、
//! parse_bru_template / derive_api_kind（含单测）。
//! Constraints: 只面向 testdata pack 内已登记模板；登录态经 wms-auth-auto-login
//! login.mjs 子进程获取（Chrome 登录态优先，全自动），token/client 只存在于后端
//! 发出的请求头，不回显、不落盘、不进日志；响应体截断保护（256KB）。
//! Read-this-with: src/capability.rs（testdata pack 路径）、src/paths.rs、
//! wms-test-api-call SKILL.md（网关域名与 header 规则）、wms-auth-auto-login/login.mjs
//! （session JSON 结构与 ENV_PROFILES）。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::anyhow;
use axum::{extract::Query, Json};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::time::timeout;

use crate::*;

/// 触发响应体截断上限。
const MAX_RESPONSE_BODY: usize = 256 * 1024;
/// 登录会话获取超时（Chrome 读取 + 可能的账密登录）。
const LOGIN_TIMEOUT_SECS: u64 = 90;
/// 接口触发请求超时。
const TRIGGER_TIMEOUT_SECS: u64 = 60;
/// 默认仓库/货主（与造数脚本 DEFAULT_WAREHOUSE/DEFAULT_COMPANY 口径一致）。
const DEFAULT_WAREHOUSE: &str = "SH.001";
const DEFAULT_COMPANY: &str = "JMJ001";

// ---------------------------------------------------------------------------
// 环境 profile（契约优先：api/profiles.yaml；读不到时用内置 fallback。
// 契约规则来源 wms-test-api-call SKILL.md：业务接口一律打网关域名）
// ---------------------------------------------------------------------------

/// x-params 构造规则（对应 profiles.yaml 每环境 x_params 节）。
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct XParamsSpec {
    #[serde(default = "default_warehouse")]
    pub(crate) warehouse_default: String,
    #[serde(default)]
    pub(crate) fields_from_session: Vec<String>,
    #[serde(default)]
    pub(crate) org_code: Option<String>,
    #[serde(default = "default_true")]
    pub(crate) include_user: bool,
}

fn default_warehouse() -> String {
    DEFAULT_WAREHOUSE.into()
}

fn default_true() -> bool {
    true
}

/// 环境静态配置（profiles.yaml 单环境 或 内置 fallback）。
#[derive(Debug, Clone)]
struct ProfileSpec {
    gateway: String,
    pda_gateway: Option<String>,
    origin: String,
    referer: String,
    x_env: Option<String>,
    /// 固定头（值可为字面量或 ${session.*} 表达式），不含登录态隐式头。
    required_headers: Vec<(String, String)>,
    x_params: XParamsSpec,
}

/// profiles.yaml 契约文档（serde 映射）。
#[derive(Debug, Deserialize)]
struct ProfilesDoc {
    pub(crate) environments: BTreeMap<String, ProfilesEnv>,
}

#[derive(Debug, Deserialize)]
struct ProfilesEnv {
    pub(crate) gateway: String,
    #[serde(default)]
    pub(crate) pda_gateway: Option<String>,
    pub(crate) origin: String,
    pub(crate) referer: String,
    #[serde(default)]
    pub(crate) x_env: Option<String>,
    #[serde(default)]
    pub(crate) x_params: Option<XParamsSpec>,
    #[serde(default)]
    pub(crate) required_headers: BTreeMap<String, String>,
}

/// 默认 x-params 构造规则（与 profiles.yaml test 环境一致）。
fn default_x_params_spec() -> XParamsSpec {
    XParamsSpec {
        warehouse_default: DEFAULT_WAREHOUSE.into(),
        fields_from_session: vec![
            "level".into(),
            "username".into(),
            "cooperationType".into(),
            "userAttr".into(),
        ],
        org_code: None,
        include_user: true,
    }
}

/// 内置 fallback（profiles.yaml 缺失/损坏时兑底；与契约内容保持同步）。
fn fallback_profile_spec(env: &str) -> ApiResult<ProfileSpec> {
    let (gateway, pda_gateway, origin, referer, x_env, org_code) = match env {
        "test" => (
            "https://test-cwhsea-wms-gw.jnt-express.com.cn",
            None,
            "http://test-wms.jms.com",
            "http://test-wms.jms.com/",
            Some("test"),
            None,
        ),
        "uat-cn" => (
            "https://demo-cwhcn-wmsgw.jtfulfillment.cn",
            None,
            "https://demo-jtwms.jtfulfillment.cn",
            "https://demo-jtwms.jtfulfillment.cn/",
            None,
            None,
        ),
        "uat-sea" => (
            "https://demo-wms-api.jtfulfillment.com",
            Some("https://demo-wms-gw.jtfulfillment.com"),
            "https://demo-wms.jtfulfillment.com",
            "https://demo-wms.jtfulfillment.com/",
            Some("uat"),
            Some("JTExpress"),
        ),
        other => {
            return Err(ApiError::bad_request(format!(
                "未知环境 {other}（支持 test / uat-cn / uat-sea）"
            )))
        }
    };
    let mut required_headers: Vec<(String, String)> = vec![
        ("x-area".into(), "hnzy".into()),
        ("X-DB".into(), "${session.db}".into()),
        ("X-User".into(), "${session.user}".into()),
        ("X-Locale".into(), "ZH".into()),
        ("langType".into(), "ZH".into()),
        ("timezone".into(), "GMT+0800".into()),
    ];
    if let Some(x_env) = x_env {
        required_headers.push(("x-env".into(), x_env.into()));
    }
    if env == "uat-cn" {
        required_headers.push(("X-Authentication-Switch".into(), "true".into()));
    }
    Ok(ProfileSpec {
        gateway: gateway.into(),
        pda_gateway: pda_gateway.map(str::to_string),
        origin: origin.into(),
        referer: referer.into(),
        x_env: x_env.map(str::to_string),
        required_headers,
        x_params: XParamsSpec {
            org_code: org_code.map(str::to_string),
            ..default_x_params_spec()
        },
    })
}

/// 从 testdata pack 读取 api/profiles.yaml（契约优先）；缺失/损坏返回 None。
fn load_profiles_doc(root: &Path) -> Option<ProfilesDoc> {
    let path = root.join("api").join("profiles.yaml");
    let text = std::fs::read_to_string(path).ok()?;
    serde_yaml::from_str(&text).ok()
}

/// 解析环境静态配置：契约优先，fallback 内置。
fn load_profile_spec(root: &Path, env: &str) -> ApiResult<ProfileSpec> {
    if let Some(doc) = load_profiles_doc(root) {
        if let Some(e) = doc.environments.get(env) {
            return Ok(ProfileSpec {
                gateway: e.gateway.clone(),
                pda_gateway: e.pda_gateway.clone(),
                origin: e.origin.clone(),
                referer: e.referer.clone(),
                x_env: e.x_env.clone(),
                required_headers: e
                    .required_headers
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect(),
                x_params: e.x_params.clone().unwrap_or_else(default_x_params_spec),
            });
        }
        return Err(ApiError::bad_request(format!(
            "profiles.yaml 中不存在环境 {env}（支持 test / uat-cn / uat-sea）"
        )));
    }
    fallback_profile_spec(env)
}

/// 运行时已求值环境：网关信息 + 基础头（含登录态隐式头，模板头由调用方去重合并）。
#[derive(Debug, Clone)]
struct ResolvedEnv {
    gateway: String,
    pda_gateway: Option<String>,
    base_headers: Vec<(String, String)>,
}

/// 求值 ${session.*} 表达式；x-params 由调用方预先构造后传入。
fn eval_header_value(expr: &str, session: &LoginSession, x_params: &str) -> String {
    expr.replace("${session.token}", &session.token)
        .replace("${session.client}", &session.client)
        .replace("${session.user}", &session.user)
        .replace("${session.db}", &session.db)
        .replace("${x_params}", x_params)
}

/// 组装默认 x-params 头（JSON 字符串；username 保持 session 内的 URL 编码形态）。
fn build_x_params(session: &LoginSession, spec: &XParamsSpec, warehouse: &str) -> String {
    let mut obj = serde_json::Map::new();
    for key in &spec.fields_from_session {
        if let Some(v) = session.params.get(key) {
            if !v.is_null() {
                obj.insert(key.clone(), v.clone());
            }
        }
    }
    obj.insert("warehouse".into(), Value::String(warehouse.to_string()));
    obj.insert(
        "orgCode".into(),
        spec.org_code
            .clone()
            .map(Value::String)
            .unwrap_or(Value::Null),
    );
    if spec.include_user && !session.user.is_empty() {
        obj.insert("user".into(), Value::String(session.user.clone()));
    }
    Value::Object(obj).to_string()
}

// ---------------------------------------------------------------------------
// catalog + .bru 解析
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) struct CatalogApi {
    pub(crate) id: String,
    pub(crate) domain: String,
    #[serde(default)]
    pub(crate) name: String,
    #[serde(default)]
    pub(crate) purpose: String,
    pub(crate) method: String,
    pub(crate) url: String,
    #[serde(default)]
    pub(crate) template: String,
    #[serde(default)]
    pub(crate) request_schema: Option<String>,
    #[serde(default)]
    pub(crate) response_schema: Option<String>,
    #[serde(default)]
    pub(crate) status: String,
    #[serde(default)]
    pub(crate) env_support: BTreeMap<String, String>,
    #[serde(default)]
    pub(crate) notes: Option<String>,
    #[serde(default)]
    pub(crate) tags: Vec<String>,
    /// 可选显式标记：create（造数）/ test（测试）；缺省时按名称启发式判定。
    #[serde(default)]
    pub(crate) kind: Option<String>,
}

/// .bru 模板解析结果：请求事实源（method/url/headers/body）+ vars 映射 + tests 断言。
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BruTemplate {
    /// vars 块：bru 变量名 → process.env 名（如 xToken → WMS_X_TOKEN）。
    pub(crate) vars_map: BTreeMap<String, String>,
    pub(crate) method: String,
    pub(crate) url: String,
    /// (头名, 值) 保持模板顺序。
    pub(crate) headers: Vec<(String, String)>,
    /// body:json 原文（保留 {{占位符}}）。
    pub(crate) body: Option<String>,
    /// tests 块中的 expect 断言行。
    pub(crate) tests: Vec<String>,
}

/// 读取单个块（`tag {` 起始，花括号配平，忽略双引号字符串内的花括号）。
fn extract_block(text: &str, tag: &str) -> Option<String> {
    let marker = format!("{tag} {{");
    let start = text.find(&marker)? + marker.len();
    let bytes = text.as_bytes();
    let mut depth = 1_i32;
    let mut in_string = false;
    let mut escaped = false;
    let mut i = start;
    while i < bytes.len() {
        let c = bytes[i] as char;
        if in_string {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_string = false;
            }
        } else {
            match c {
                '"' => in_string = true,
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(text[start..i].to_string());
                    }
                }
                _ => {}
            }
        }
        i += 1;
    }
    None
}

/// 解析 .bru 模板。实际格式：`post { url: ..., body: json }` 块标签即 method，
/// url 缩进在块内；兼容旧式顶层 `method POST` / `url xxx` 形态。
pub(crate) fn parse_bru_template(text: &str) -> BruTemplate {
    let mut t = BruTemplate {
        method: "POST".into(),
        ..Default::default()
    };
    if let Some(vars) = extract_block(text, "vars") {
        for line in vars.lines() {
            let line = line.trim();
            if let Some((name, value)) = line.split_once(':') {
                let name = name.trim().to_string();
                let value = value.trim();
                // 形如 {{process.env.WMS_X_TOKEN}}
                if let Some(env) = value
                    .trim_start_matches("{{")
                    .trim_end_matches("}}")
                    .trim()
                    .strip_prefix("process.env.")
                {
                    t.vars_map.insert(name, env.trim().to_string());
                }
            }
        }
    }
    // 方法块：post/get/put/delete/patch { url: ..., ... }（块标签即 method）。
    let method_re = Regex::new(r"(?m)^\s*(get|post|put|delete|patch)\s*\{").expect("method regex");
    if let Some(cap) = method_re.captures(text) {
        t.method = cap.get(1).unwrap().as_str().to_uppercase();
        if let Some(block) = extract_block(text, cap.get(1).unwrap().as_str()) {
            for line in block.lines() {
                let line = line.trim();
                if let Some(u) = line.strip_prefix("url:") {
                    t.url = u.trim().to_string();
                }
            }
        }
    }
    // 兼容旧式顶层形态：method POST / url xxx / url: xxx（缩进）。
    for line in text.lines() {
        let line = line.trim();
        if let Some(m) = line.strip_prefix("method ") {
            t.method = m.trim().to_uppercase();
        } else if let Some(u) = line.strip_prefix("url ") {
            t.url = u.trim().to_string();
        } else if t.url.is_empty() {
            if let Some(u) = line.strip_prefix("url:") {
                t.url = u.trim().to_string();
            }
        }
    }
    if let Some(headers) = extract_block(text, "headers") {
        for line in headers.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some((name, value)) = line.split_once(':') {
                t.headers
                    .push((name.trim().to_string(), value.trim().to_string()));
            }
        }
    }
    t.body = extract_block(text, "body:json").map(|b| b.trim().to_string());
    if let Some(tests) = extract_block(text, "tests") {
        t.tests = tests
            .lines()
            .map(str::trim)
            .filter(|l| l.contains("expect("))
            .map(str::to_string)
            .collect();
    }
    t
}

/// 占位符归一化：canonical 名 → process.env 名。
/// `{{xToken}}`（vars 块有映射）→ (xToken, WMS_X_TOKEN)；
/// `{{process.env.WMS_X_USER}}` → (WMS_X_USER, WMS_X_USER)；
/// `{{naturalReceiptCode}}`（无映射，请求特有值）→ (naturalReceiptCode, "")。
fn canonical_placeholders<'a>(
    text: &'a str,
    vars_map: &BTreeMap<String, String>,
) -> Vec<(String, String)> {
    let re = Regex::new(r"\{\{\s*([A-Za-z0-9_.]+)\s*\}\}").expect("placeholder regex");
    let mut seen: Vec<(String, String)> = Vec::new();
    for cap in re.captures_iter(text) {
        let raw = cap.get(1).map(|m| m.as_str()).unwrap_or_default();
        let (canonical, env) = if let Some(env) = raw.strip_prefix("process.env.") {
            (env.trim().to_string(), env.trim().to_string())
        } else {
            let env = vars_map
                .get(raw)
                .cloned()
                .unwrap_or_else(|| heuristic_env_name(raw));
            (raw.to_string(), env)
        };
        if canonical.is_empty() || seen.iter().any(|(c, _)| *c == canonical) {
            continue;
        }
        seen.push((canonical, env));
    }
    seen
}

/// vars 映射缺失时的启发式 env 名：xPdaVersion → WMS_X_PDA_VERSION；
/// warehouseCode → WMS_WAREHOUSE_CODE。仅用于会话/默认值派生匹配，
/// 无派生的名字仍由用户填写，行为不变。
fn heuristic_env_name(raw: &str) -> String {
    let mut s = String::new();
    for (i, c) in raw.chars().enumerate() {
        if c.is_uppercase() && i > 0 {
            s.push('_');
        }
        s.extend(c.to_uppercase());
    }
    if s.starts_with("WMS_") {
        s
    } else {
        format!("WMS_{s}")
    }
}

/// 造数/测试标记启发式：catalog `kind` 字段优先；否则按 id/purpose 词表判定，
/// 查询词先命中判测试，创建词命中判造数，默认造数（WMS catalog 以写操作为主）。
pub(crate) fn derive_api_kind(api: &CatalogApi) -> &'static str {
    if let Some(k) = api.kind.as_deref() {
        return match k {
            "test" => "test",
            _ => "create",
        };
    }
    let hay = format!("{} {} {}", api.id, api.name, api.purpose).to_lowercase();
    const TEST_WORDS: &[&str] = &["query", "list", "page", "detail", "search", "get-", "check"];
    const CREATE_WORDS: &[&str] = &[
        "create", "close", "cancel", "claim", "confirm", "adjust", "release", "putaway", "add",
        "bind", "freeze", "unfreeze", "review", "delivery", "ship", "receipt",
    ];
    if TEST_WORDS.iter().any(|w| hay.contains(w)) {
        "test"
    } else if CREATE_WORDS.iter().any(|w| hay.contains(w)) {
        "create"
    } else {
        "create"
    }
}

fn testdata_root() -> PathBuf {
    paths::wms_testdata_pack_root()
}

fn load_catalog(root: &Path) -> ApiResult<Vec<CatalogApi>> {
    let path = root.join("api").join("catalog.yaml");
    let text = std::fs::read_to_string(&path)
        .map_err(|e| ApiError::bad_request(format!("catalog.yaml 读取失败: {e}")))?;
    let doc: Value = serde_yaml::from_str(&text)
        .map_err(|e| ApiError::bad_request(format!("catalog.yaml 解析失败: {e}")))?;
    let apis = doc
        .get("apis")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    Ok(apis
        .into_iter()
        .filter_map(|v| serde_json::from_value(v).ok())
        .collect())
}

fn load_bru(root: &Path, api: &CatalogApi) -> ApiResult<BruTemplate> {
    let path = root.join(&api.template);
    let text = std::fs::read_to_string(&path)
        .map_err(|e| ApiError::bad_request(format!("模板 {} 读取失败: {e}", api.template)))?;
    let mut t = parse_bru_template(&text);
    // Bruno 机制：变量→process.env 映射统一在 environments/local.bru（请求模板自身
    // 无 vars 块），与运行时行为一致，此处合并（模板内 vars 块优先）。
    let env_path = root
        .join("api")
        .join("bruno")
        .join("environments")
        .join("local.bru");
    if let Ok(env_text) = std::fs::read_to_string(&env_path) {
        let global = parse_bru_template(&env_text);
        for (k, v) in global.vars_map {
            t.vars_map.entry(k).or_insert(v);
        }
    }
    Ok(t)
}

// ---------------------------------------------------------------------------
// API handlers
// ---------------------------------------------------------------------------

/// apitest 目录当前只挂载 WMS testdata pack；显式传其它 project 时明确拒绝，
/// 避免参数被静默忽略造成契约误导（与 capability 端点同风格）。
fn ensure_apitest_project(project: Option<&str>) -> ApiResult<()> {
    if let Some(p) = project.map(str::trim).filter(|v| !v.is_empty()) {
        if !p.eq_ignore_ascii_case("WMS") {
            return Err(ApiError::bad_request(format!(
                "unknown project for apitest catalog: {p} (only `WMS` pack is mounted)"
            )));
        }
    }
    Ok(())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ApitestListQuery {
    #[serde(default)]
    pub(crate) project: Option<String>,
    #[serde(default)]
    pub(crate) q: Option<String>,
    #[serde(default)]
    pub(crate) domain: Option<String>,
    /// create / test / all（默认 all）。
    #[serde(default)]
    pub(crate) kind: Option<String>,
    #[serde(default)]
    pub(crate) status: Option<String>,
}

/// GET /api/apitest/apis — 接口目录（catalog + 模板校验 + 造数/测试标记）。
pub(crate) async fn api_apitest_apis(Query(q): Query<ApitestListQuery>) -> ApiResult<Json<Value>> {
    ensure_apitest_project(q.project.as_deref())?;
    let root = testdata_root();
    let mut apis = load_catalog(&root)?;
    if let Some(d) = q.domain.as_deref().filter(|s| !s.is_empty()) {
        apis.retain(|a| a.domain == d);
    }
    if let Some(s) = q.status.as_deref().filter(|s| !s.is_empty()) {
        apis.retain(|a| a.status == s);
    }
    if let Some(kw) = q.q.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        let kw = kw.to_lowercase();
        apis.retain(|a| {
            format!(
                "{} {} {} {} {}",
                a.id,
                a.name,
                a.purpose,
                a.url,
                a.tags.join(" ")
            )
            .to_lowercase()
            .contains(&kw)
        });
    }
    let kind_filter = q.kind.as_deref().unwrap_or("all");
    let mut items: Vec<Value> = Vec::new();
    for api in &apis {
        let kind = derive_api_kind(api);
        if kind_filter != "all" && kind_filter != kind {
            continue;
        }
        let bru = load_bru(&root, api).ok();
        let placeholder_count = bru
            .as_ref()
            .map(|t| {
                let mut text = format!(
                    "{} {}",
                    t.url,
                    t.headers
                        .iter()
                        .map(|(_, v)| v.as_str())
                        .collect::<Vec<_>>()
                        .join(" ")
                );
                if let Some(b) = &t.body {
                    text.push(' ');
                    text.push_str(b);
                }
                canonical_placeholders(&text, &t.vars_map).len()
            })
            .unwrap_or(0);
        items.push(json!({
            "id": api.id,
            "domain": api.domain,
            "purpose": api.purpose,
            "method": bru.as_ref().map(|t| t.method.clone()).unwrap_or_else(|| api.method.clone()),
            "url": bru.as_ref().map(|t| t.url.clone()).unwrap_or_else(|| api.url.clone()),
            "status": api.status,
            "kind": kind,
            "tags": api.tags,
            "envSupport": api.env_support,
            "notes": api.notes,
            "hasRequestSchema": api.request_schema.is_some(),
            "placeholderCount": placeholder_count,
            "templateOk": bru.is_some(),
        }));
    }
    Ok(Json(json!({
        "ok": true,
        "count": items.len(),
        "apis": items,
        "packRoot": root.display().to_string(),
    })))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ApitestDetailQuery {
    #[serde(default)]
    pub(crate) project: Option<String>,
    #[serde(default)]
    pub(crate) id: Option<String>,
    #[serde(alias = "reqId")]
    pub(crate) api_id: Option<String>,
}

/// GET /api/apitest/api?id=<api-id> — 接口详情：模板、占位符、schema、tests。
pub(crate) async fn api_apitest_api(Query(q): Query<ApitestDetailQuery>) -> ApiResult<Json<Value>> {
    ensure_apitest_project(q.project.as_deref())?;
    let id = q.id.or(q.api_id).unwrap_or_default();
    let root = testdata_root();
    let api = load_catalog(&root)?
        .into_iter()
        .find(|a| a.id == id)
        .ok_or_else(|| ApiError::bad_request(format!("接口不存在: {id}")))?;
    let bru = load_bru(&root, &api)?;
    let mut text = format!(
        "{} {}",
        bru.url,
        bru.headers
            .iter()
            .map(|(_, v)| v.as_str())
            .collect::<Vec<_>>()
            .join(" ")
    );
    if let Some(b) = &bru.body {
        text.push(' ');
        text.push_str(b);
    }
    let placeholders: Vec<Value> = canonical_placeholders(&text, &bru.vars_map)
        .into_iter()
        .map(|(canonical, env)| {
            json!({
                "name": canonical,
                "envVar": env,
                "derived": matches!(env.as_str(), "WMS_X_TOKEN" | "WMS_X_CLIENT" | "WMS_X_PARAMS" | "WMS_X_USER" | "WMS_X_DB" | "WMS_BASE_URL"),
            })
        })
        .collect();
    let request_schema = api
        .request_schema
        .as_ref()
        .and_then(|rel| std::fs::read_to_string(root.join(rel)).ok());
    Ok(Json(json!({
        "ok": true,
        "api": {
            "id": api.id, "domain": api.domain, "name": api.name, "purpose": api.purpose,
            "method": bru.method, "url": bru.url, "status": api.status, "kind": derive_api_kind(&api),
            "tags": api.tags, "envSupport": api.env_support, "notes": api.notes,
        },
        "template": {
            "headers": bru.headers,
            "body": bru.body,
            "varsMap": bru.vars_map,
        },
        "tests": bru.tests,
        "placeholders": placeholders,
        "requestSchema": request_schema,
        "requestSchemaPath": api.request_schema,
        "responseSchemaPath": api.response_schema,
    })))
}

// ---------------------------------------------------------------------------
// 触发
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ApitestTriggerForm {
    pub(crate) id: String,
    /// test / uat-cn / uat-sea。
    #[serde(default = "default_env")]
    pub(crate) env: String,
    /// 占位符取值（canonical 名 → 值），覆盖默认与会话派生值。
    #[serde(default)]
    pub(crate) vars: BTreeMap<String, String>,
    /// body 覆盖（临时修改入参；保留 {{占位符}} 也会被替换）。
    #[serde(default)]
    pub(crate) body_override: Option<String>,
    /// true 时只返回构造后的请求（凭证头脱敏），不发送。
    #[serde(default)]
    pub(crate) dry_run: Option<bool>,
}

fn default_env() -> String {
    "test".into()
}

#[derive(Debug, Deserialize)]
struct LoginSession {
    token: String,
    client: String,
    #[serde(default)]
    user: String,
    #[serde(default)]
    db: String,
    #[serde(default)]
    params: Value,
}

#[derive(Debug, Deserialize)]
struct LoginResult {
    #[serde(default)]
    code: i64,
    #[serde(default)]
    token: String,
    #[serde(default)]
    client: String,
    #[serde(default)]
    from: String,
    #[serde(default)]
    session: Option<LoginSession>,
}

/// 调 login.mjs 获取会话（Chrome 登录态优先 → /tmp 缓存 → 账密自动登录）。
/// 输出 JSON 只在进程内解析，token 不落盘不回显。
async fn fetch_login_session(env: &str) -> ApiResult<LoginSession> {
    let script = paths::wms_root()
        .join(".agents")
        .join("skills")
        .join("wms-auth-auto-login")
        .join("login.mjs");
    if !script.is_file() {
        return Err(ApiError::bad_request(format!(
            "login.mjs 不存在: {}",
            script.display()
        )));
    }
    let output = tokio::process::Command::new("node")
        .arg(&script)
        .arg("--env")
        .arg(env)
        .current_dir(script.parent().unwrap_or(Path::new(".")))
        .output();
    let result = timeout(Duration::from_secs(LOGIN_TIMEOUT_SECS), output)
        .await
        .map_err(|_| ApiError::from(anyhow!("login.mjs 超时（{LOGIN_TIMEOUT_SECS}s）")))?;
    let result = result.map_err(|e| ApiError::from(anyhow!("login.mjs 启动失败: {e}")))?;
    let stdout = String::from_utf8_lossy(&result.stdout);
    let parsed: LoginResult = serde_json::from_str(stdout.trim()).map_err(|e| {
        let stderr = String::from_utf8_lossy(&result.stderr);
        ApiError::from(anyhow!(
            "login.mjs 输出解析失败: {e}；stderr: {}",
            stderr.chars().take(300).collect::<String>()
        ))
    })?;
    if let Some(s) = parsed.session {
        if !s.token.is_empty() {
            return Ok(s);
        }
    }
    if !parsed.token.is_empty() {
        return Ok(LoginSession {
            token: parsed.token,
            client: parsed.client,
            user: String::new(),
            db: String::new(),
            params: Value::Null,
        });
    }
    Err(ApiError::from(anyhow!(
        "登录态获取失败（from={}），请在 Chrome 登录 WMS 后重试",
        parsed.from
    )))
}

/// 解析单个占位符：用户 vars → 会话/环境派生 → 内置默认 → 缺失。
fn resolve_placeholder(
    canonical: &str,
    env_name: &str,
    user_vars: &BTreeMap<String, String>,
    session: &LoginSession,
    spec: &ProfileSpec,
    x_params: &str,
    warehouse: &str,
) -> Option<String> {
    let clean = |v: &String| v.trim().to_string();
    if let Some(v) = user_vars
        .get(canonical)
        .map(clean)
        .filter(|v| !v.is_empty())
    {
        return Some(v);
    }
    if let Some(v) = user_vars.get(env_name).map(clean).filter(|v| !v.is_empty()) {
        return Some(v);
    }
    let derived = match env_name {
        "WMS_BASE_URL" => Some(spec.gateway.clone()),
        "WMS_X_TOKEN" => Some(session.token.clone()),
        "WMS_X_CLIENT" => Some(session.client.clone()),
        "WMS_X_USER" => (!session.user.is_empty()).then(|| session.user.clone()),
        "WMS_X_DB" => (!session.db.is_empty()).then(|| session.db.clone()),
        "WMS_X_PARAMS" => Some(x_params.to_string()),
        "WMS_WAREHOUSE_CODE" => Some(warehouse.to_string()),
        "WMS_OWNER_CODE" | "WMS_COMPANY_CODE" => Some(DEFAULT_COMPANY.to_string()),
        "WMS_OPERATOR_CODE" => (!session.user.is_empty()).then(|| session.user.clone()),
        // PDA App 固定版本头（catalog PDA 模板 {{xPdaVersion}} 占位符）。
        "WMS_X_PDA_VERSION" => Some("V1.0.0".to_string()),
        _ => None,
    };
    derived.filter(|v| !v.is_empty())
}

/// 求值运行时环境：契约/静态 spec + 会话 + 仓库值 → 网关 + 基础头。
fn resolve_env(spec: &ProfileSpec, session: &LoginSession, warehouse: &str) -> ResolvedEnv {
    let x_params_value = build_x_params(session, &spec.x_params, warehouse);
    let x_params = x_params_value.clone();
    let mut base_headers: Vec<(String, String)> = vec![
        ("Accept".into(), "application/json, text/plain, */*".into()),
        ("User-Agent".into(), "Mozilla/5.0".into()),
        ("Origin".into(), spec.origin.clone()),
        ("Referer".into(), spec.referer.clone()),
        // 登录态隐式头：模板头同名时会被去重跳过。
        ("X-Token".into(), session.token.clone()),
        ("X-Client".into(), session.client.clone()),
        ("x-params".into(), x_params_value),
    ];
    for (name, expr) in &spec.required_headers {
        base_headers.push((name.clone(), eval_header_value(expr, session, &x_params)));
    }
    ResolvedEnv {
        gateway: spec.gateway.clone(),
        pda_gateway: spec.pda_gateway.clone(),
        base_headers,
    }
}

/// SEA UAT 按路径前缀选域名：/wms-web 前缀（PDA 网关）与业务路径分开。
fn resolve_base_url<'a>(env: &'a ResolvedEnv, path: &str) -> &'a str {
    if let Some(pda) = &env.pda_gateway {
        if path.trim_start().starts_with("/wms-web") {
            return pda;
        }
    }
    &env.gateway
}

/// 响应头值脱敏：凭证类头只回显长度（dry-run 展示用）。
fn redact_header(name: &str, value: &str) -> String {
    let n = name.to_lowercase();
    if n == "x-token" || n == "x-client" {
        format!("***redacted(len={})", value.len())
    } else {
        value.to_string()
    }
}

/// POST /api/apitest/trigger — 解析模板 → 会话注入 → 代理发送 → 返回响应。
pub(crate) async fn api_apitest_trigger(
    form: FormOrJson<ApitestTriggerForm>,
) -> ApiResult<Json<Value>> {
    let body = form.0;
    let dry_run = body.dry_run.unwrap_or(false);
    let root = testdata_root();
    let spec = load_profile_spec(&root, &body.env)?;
    let api = load_catalog(&root)?
        .into_iter()
        .find(|a| a.id == body.id)
        .ok_or_else(|| ApiError::bad_request(format!("接口不存在: {}", body.id)))?;
    let bru = load_bru(&root, &api)?;
    let session = fetch_login_session(&body.env).await?;

    let warehouse = body
        .vars
        .get("warehouseCode")
        .or_else(|| body.vars.get("WMS_WAREHOUSE_CODE"))
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| spec.x_params.warehouse_default.clone());
    let env = resolve_env(&spec, &session, &warehouse);

    // 收集全部占位符（url + headers + body）。
    let mut text = format!(
        "{} {}",
        bru.url,
        bru.headers
            .iter()
            .map(|(_, v)| v.as_str())
            .collect::<Vec<_>>()
            .join(" ")
    );
    let effective_body = body.body_override.clone().or_else(|| bru.body.clone());
    if let Some(b) = &effective_body {
        text.push(' ');
        text.push_str(b);
    }
    let placeholders = canonical_placeholders(&text, &bru.vars_map);

    // 解析占位符，缺失清单返回。
    let mut resolved: BTreeMap<String, String> = BTreeMap::new();
    let mut missing: Vec<String> = Vec::new();
    for (canonical, env_name) in &placeholders {
        match resolve_placeholder(
            canonical,
            env_name,
            &body.vars,
            &session,
            &spec,
            &env.base_headers
                .iter()
                .find(|(n, _)| n == "x-params")
                .map(|(_, v)| v.clone())
                .unwrap_or_default(),
            &warehouse,
        ) {
            Some(v) => {
                resolved.insert(canonical.clone(), v);
            }
            None => missing.push(canonical.clone()),
        }
    }
    if !missing.is_empty() {
        return Ok(Json(json!({
            "ok": false,
            "missing": missing,
            "message": "以下占位符缺少取值（会话可派生的已自动注入）；在入参表单填写后重试。",
        })));
    }

    // URL：先替换 {{baseUrl}}，再按路径前缀选 SEA 双域名。
    // 占位符两种原文形式都要替换：{{canonical}} 与 {{process.env.CANONICAL}}。
    let substitute = |text: &str| -> String {
        let mut out = text.to_string();
        for (k, v) in &resolved {
            let plain = String::from("{{") + k + "}}";
            let env_form = String::from("{{process.env.") + k + "}}";
            out = out.replace(&plain, v);
            out = out.replace(&env_form, v);
        }
        out
    };
    let raw_url = substitute(&bru.url);
    // 剥离已替换的 {{baseUrl}}（gateway），按路径前缀重选域名（SEA UAT 双域名）。
    let path = raw_url
        .trim_start_matches(&env.gateway)
        .trim_start_matches(&env.pda_gateway.clone().unwrap_or_default())
        .to_string();
    let url = if path.starts_with("http") {
        path // 模板本身就是完整 URL（罕见），不重写
    } else {
        format!("{}{}", resolve_base_url(&env, &path), path)
    };

    // Headers：运行时基础头 + 模板头（模板优先，按头名小写去重）。
    let mut headers: Vec<(String, String)> = env.base_headers.clone();
    let template_names: Vec<String> = bru.headers.iter().map(|(n, _)| n.to_lowercase()).collect();
    headers.retain(|(n, _)| !template_names.contains(&n.to_lowercase()));
    for (name, value) in &bru.headers {
        headers.push((name.clone(), substitute(value)));
    }

    // dry-run：返回构造后的请求（凭证头脱敏），不发送。
    if dry_run {
        return Ok(Json(json!({
            "ok": true,
            "dryRun": true,
            "env": body.env,
            "apiId": api.id,
            "method": bru.method,
            "url": url,
            "headers": headers.iter().map(|(n, v)| (n.clone(), redact_header(n, v))).collect::<Vec<_>>(),
            "body": effective_body.as_ref().map(|b| substitute(b)),
            "tests": bru.tests,
            "note": "dry-run 构造预览：凭证头已脱敏；确认无误后去掉 dryRun 真正发送。",
        })));
    }

    // 发送。
    let client = reqwest::Client::builder()
        .danger_accept_invalid_certs(true)
        .build()
        .map_err(|e| ApiError::from(anyhow!("HTTP client 构建失败: {e}")))?;
    let method = reqwest::Method::from_bytes(bru.method.as_bytes())
        .map_err(|e| ApiError::from(anyhow!("method 非法: {e}")))?;
    let mut req = client
        .request(method, &url)
        .timeout(Duration::from_secs(TRIGGER_TIMEOUT_SECS));
    for (name, value) in &headers {
        if let Ok(hn) = reqwest::header::HeaderName::from_bytes(name.as_bytes()) {
            if let Ok(hv) = reqwest::header::HeaderValue::from_str(value) {
                req = req.header(hn, hv);
            }
        }
    }
    if let Some(b) = &effective_body {
        req = req.body(substitute(b));
    }
    let started = std::time::Instant::now();
    let resp = timeout(Duration::from_secs(TRIGGER_TIMEOUT_SECS + 5), req.send())
        .await
        .map_err(|_| ApiError::from(anyhow!("接口触发超时（{}s）", TRIGGER_TIMEOUT_SECS)))?
        .map_err(|e| ApiError::from(anyhow!("请求发送失败: {e}")))?;
    let status = resp.status().as_u16();
    let content_type = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    let body_text = resp.text().await.unwrap_or_default();
    let duration_ms = started.elapsed().as_millis() as u64;
    let truncated = body_text.len() > MAX_RESPONSE_BODY;
    let body_out: String = if truncated {
        body_text.chars().take(MAX_RESPONSE_BODY / 4).collect()
    } else {
        body_text
    };
    let body_json: Value = serde_json::from_str(body_out.trim()).unwrap_or(Value::Null);

    Ok(Json(json!({
        "ok": (200..400).contains(&status),
        "status": status,
        "durationMs": duration_ms,
        "contentType": content_type,
        "url": url,
        "bodyText": body_out,
        "bodyJson": body_json,
        "truncated": truncated,
        "env": body.env,
        "apiId": api.id,
        "kind": derive_api_kind(&api),
        "tests": bru.tests,
        "note": "会话由 login.mjs 自动获取（Chrome 优先）；响应不含请求头，token 不回显。",
    })))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_BRU: &str = r#"meta {
  name: close-receipt
  type: http
  seq: 1
}

vars {
  baseUrl: {{process.env.WMS_BASE_URL}}
  xToken: {{process.env.WMS_X_TOKEN}}
  warehouseCode: {{process.env.WMS_WAREHOUSE_CODE}}
}

post {
  url: {{baseUrl}}/wms-web/inbound/receipt/close
  body: json
  auth: none
}

headers {
  Content-Type: application/json
  X-Token: {{xToken}}
  X-User: {{process.env.WMS_X_USER}}
}

body:json {
  {
    "referCode": "{{referCode}}",
    "warehouseCode": "{{warehouseCode}}",
    "note": "brace } inside \"str}\" ok"
  }
}

tests {
  const body = res.getBody();
  expect(res.getStatus()).to.be.oneOf([200, 201]);
  expect(body).to.have.property('data');
}"#;

    #[test]
    fn parse_bru_extracts_all_sections() {
        let t = parse_bru_template(SAMPLE_BRU);
        assert_eq!(t.method, "POST");
        assert_eq!(t.url, "{{baseUrl}}/wms-web/inbound/receipt/close");
        assert_eq!(
            t.vars_map.get("xToken").map(String::as_str),
            Some("WMS_X_TOKEN")
        );
        assert_eq!(t.headers.len(), 3);
        assert_eq!(t.headers[1].0, "X-Token");
        assert!(t
            .body
            .as_deref()
            .unwrap_or("")
            .contains("\"referCode\": \"{{referCode}}\""));
        // 字符串内花括号不破坏配平
        assert!(t.body.as_deref().unwrap_or("").contains("str}"));
        assert_eq!(t.tests.len(), 2);
    }

    #[test]
    fn parse_bru_legacy_top_level_format() {
        let legacy = "method GET\n\nurl {{baseUrl}}/api/x\n\nheaders {\n  X-Token: {{xToken}}\n}";
        let t = parse_bru_template(legacy);
        assert_eq!(t.method, "GET");
        assert_eq!(t.url, "{{baseUrl}}/api/x");
    }

    #[test]
    fn placeholders_canonicalize_with_vars_map() {
        let t = parse_bru_template(SAMPLE_BRU);
        let mut text = format!(
            "{} {}",
            t.url,
            t.headers
                .iter()
                .map(|(_, v)| v.as_str())
                .collect::<Vec<_>>()
                .join(" ")
        );
        text.push(' ');
        text.push_str(t.body.as_deref().unwrap_or(""));
        let ph = canonical_placeholders(&text, &t.vars_map);
        let names: Vec<&str> = ph.iter().map(|(c, _)| c.as_str()).collect();
        assert!(names.contains(&"baseUrl"));
        assert!(names.contains(&"xToken"));
        assert!(names.contains(&"WMS_X_USER"));
        assert!(names.contains(&"referCode"));
        assert!(names.contains(&"warehouseCode"));
        // 无重复
        assert_eq!(
            names.len(),
            names.iter().collect::<std::collections::HashSet<_>>().len()
        );
    }

    fn api(id: &str, purpose: &str) -> CatalogApi {
        CatalogApi {
            id: id.into(),
            domain: "outbound".into(),
            name: id.into(),
            purpose: purpose.into(),
            method: "POST".into(),
            url: "".into(),
            template: "".into(),
            request_schema: None,
            response_schema: None,
            status: "template_verified".into(),
            env_support: BTreeMap::new(),
            notes: None,
            tags: vec![],
            kind: None,
        }
    }

    #[test]
    fn derive_kind_heuristic() {
        assert_eq!(
            derive_api_kind(&api("inbound/close-receipt", "Close an inbound receipt")),
            "create"
        );
        assert_eq!(
            derive_api_kind(&api(
                "pda/replenish-stock-query-get-location-item-qty",
                "查询库位库存"
            )),
            "test"
        );
        assert_eq!(
            derive_api_kind(&api("pda/cycle-count-task-claim", "盘点任务领取")),
            "create"
        );
        let mut explicit = api("x/whatever", "anything");
        explicit.kind = Some("test".into());
        assert_eq!(derive_api_kind(&explicit), "test");
    }

    #[test]
    fn x_params_builds_json_with_warehouse() {
        let session = LoginSession {
            token: "t".into(),
            client: "c".into(),
            user: "01668709".into(),
            db: "hnzy".into(),
            params: json!({"level": 10, "username": "%E6%9D%A8", "cooperationType": "K", "userAttr": 2}),
        };
        let spec = fallback_profile_spec("test").unwrap();
        let x = build_x_params(&session, &spec.x_params, "SH.001");
        let v: Value = serde_json::from_str(&x).unwrap();
        assert_eq!(v["level"], 10);
        assert_eq!(v["warehouse"], "SH.001");
        assert_eq!(v["user"], "01668709");
        assert!(v.get("orgCode").unwrap().is_null());
        // fallback 内置 spec 的必需头含 X-User（401 根因修复）。
        assert!(spec
            .required_headers
            .iter()
            .any(|(n, v)| n == "X-User" && v == "${session.user}"));
    }

    #[test]
    fn sea_uat_selects_pda_gateway_for_wms_web() {
        let session = LoginSession {
            token: "t".into(),
            client: "c".into(),
            user: "u".into(),
            db: "hnzy".into(),
            params: Value::Null,
        };
        let spec = fallback_profile_spec("uat-sea").unwrap();
        let env = resolve_env(&spec, &session, "SH.001");
        assert_eq!(
            resolve_base_url(&env, "/wms-web/pda/x"),
            spec.pda_gateway.as_deref().unwrap()
        );
        assert_eq!(resolve_base_url(&env, "/rest/cbt/x"), spec.gateway);
        // 求值后的基础头包含 X-User 且凭证头就位。
        assert!(env
            .base_headers
            .iter()
            .any(|(n, v)| n == "X-User" && v == "u"));
        assert!(env.base_headers.iter().any(|(n, _)| n == "X-Token"));
        let test = resolve_env(&fallback_profile_spec("test").unwrap(), &session, "SH.001");
        assert_eq!(resolve_base_url(&test, "/wms-web/x"), test.gateway);
    }

    #[test]
    fn fallback_profiles_cover_three_envs() {
        assert!(fallback_profile_spec("test").is_ok());
        assert!(fallback_profile_spec("uat-cn").is_ok());
        assert!(fallback_profile_spec("uat-sea").is_ok());
        assert!(fallback_profile_spec("prod").is_err());
        // uat-cn 特有：X-Authentication-Switch + 无 x-env。
        let cn = fallback_profile_spec("uat-cn").unwrap();
        assert!(cn
            .required_headers
            .iter()
            .any(|(n, v)| n == "X-Authentication-Switch" && v == "true"));
        assert!(cn.x_env.is_none());
    }

    #[test]
    fn profiles_doc_contract_parses_and_overrides() {
        let yaml = r#"
environments:
  test:
    gateway: https://gw.example.com
    origin: https://fe.example.com
    referer: https://fe.example.com/
    x_env: test
    x_params:
      warehouse_default: WH.1
      fields_from_session: [level]
      org_code: null
      include_user: false
    required_headers:
      x-area: hnzy
      X-User: "${session.user}"
"#;
        let doc: ProfilesDoc = serde_yaml::from_str(yaml).unwrap();
        assert!(doc.environments.contains_key("test"));
        let e = &doc.environments["test"];
        assert_eq!(e.gateway, "https://gw.example.com");
        assert_eq!(
            e.required_headers.get("X-User").map(String::as_str),
            Some("${session.user}")
        );
        let spec = ProfileSpec {
            gateway: e.gateway.clone(),
            pda_gateway: e.pda_gateway.clone(),
            origin: e.origin.clone(),
            referer: e.referer.clone(),
            x_env: e.x_env.clone(),
            required_headers: e
                .required_headers
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
            x_params: e.x_params.clone().unwrap_or_else(default_x_params_spec),
        };
        let session = LoginSession {
            token: "t".into(),
            client: "c".into(),
            user: "U1".into(),
            db: "db1".into(),
            params: json!({"level": 5}),
        };
        let env = resolve_env(&spec, &session, "WH.1");
        let get = |n: &str| {
            env.base_headers
                .iter()
                .find(|(k, _)| k == n)
                .map(|(_, v)| v.clone())
                .unwrap_or_default()
        };
        assert_eq!(get("X-User"), "U1");
        assert_eq!(get("x-area"), "hnzy");
        let xp: Value = serde_json::from_str(&get("x-params")).unwrap();
        assert_eq!(xp["warehouse"], "WH.1");
        assert_eq!(xp["level"], 5);
        assert!(xp.get("user").is_none()); // include_user: false
    }

    #[test]
    fn redact_header_masks_credentials() {
        assert_eq!(redact_header("X-Token", "abcdef"), "***redacted(len=6)");
        assert_eq!(redact_header("x-client", "ab"), "***redacted(len=2)");
        assert_eq!(redact_header("X-User", "u1"), "u1");
    }

    #[test]
    fn substitute_replaces_both_placeholder_forms() {
        let mut resolved = BTreeMap::new();
        resolved.insert("WMS_X_USER".to_string(), "U1".to_string());
        resolved.insert("referCode".to_string(), "R1".to_string());
        let mut out = "X-User: {{process.env.WMS_X_USER}}; code={{referCode}}".to_string();
        for (k, v) in &resolved {
            let plain = String::from("{{") + k + "}}";
            let env_form = String::from("{{process.env.") + k + "}}";
            out = out.replace(&plain, v);
            out = out.replace(&env_form, v);
        }
        assert_eq!(out, "X-User: U1; code=R1");
    }
}
