use std::{collections::HashMap, path::PathBuf};

use anyhow::{anyhow, Context, Result};
use axum::{
    extract::{Path, State},
    Json,
};
use futures_util::{SinkExt, StreamExt};
use reqwest::{header, Method, Url};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::{fs, net::TcpStream};
use tokio_tungstenite::{
    connect_async,
    tungstenite::Message,
    MaybeTlsStream, WebSocketStream,
};

use crate::*;

const AUTH_AUDIT_FILE: &str = "auth-audit.jsonl";
const AUTH_RESPONSE_LIMIT_BYTES: usize = 2_000_000;

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BrowserAuthConfig {
    #[serde(default)]
    pub(crate) cdp_url: String,
    /// Cookie 域名白名单：只有这些域名的 cookie 会被解密/读取。
    /// 为空时回退为所有启用站点的 cookieDomains 并集（安全默认：无站点则读取 0 条）。
    #[serde(default)]
    pub(crate) cookie_allowlist: Vec<String>,
    #[serde(default)]
    pub(crate) sites: Vec<BrowserAuthSiteConfig>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BrowserAuthSiteConfig {
    pub(crate) id: String,
    #[serde(default)]
    pub(crate) label: String,
    #[serde(default)]
    pub(crate) enabled: bool,
    pub(crate) base_url: String,
    #[serde(default)]
    pub(crate) cookie_domains: Vec<String>,
    #[serde(default)]
    pub(crate) allowed_hosts: Vec<String>,
    #[serde(default)]
    pub(crate) allowed_path_prefixes: Vec<String>,
    #[serde(default)]
    pub(crate) default_headers: HashMap<String, String>,
    #[serde(default)]
    pub(crate) login_check: Option<BrowserAuthLoginCheck>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BrowserAuthLoginCheck {
    #[serde(default = "default_login_check_method")]
    pub(crate) method: String,
    #[serde(default)]
    pub(crate) path: String,
    #[serde(default = "default_login_check_expect")]
    pub(crate) expect: u16,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BrowserAuthRequestForm {
    #[serde(default = "default_request_method")]
    method: String,
    path: String,
    #[serde(default)]
    headers: HashMap<String, String>,
    #[serde(default)]
    json: Option<Value>,
    #[serde(default)]
    body: Option<String>,
}

pub(crate) fn default_login_check_method() -> String {
    "GET".into()
}

pub(crate) fn default_login_check_expect() -> u16 {
    200
}

fn default_request_method() -> String {
    "GET".into()
}

pub(crate) fn normalize_browser_auth_config(mut cfg: BrowserAuthConfig) -> BrowserAuthConfig {
    cfg.cdp_url = cfg.cdp_url.trim().to_string();
    cfg.cookie_allowlist = unique_strings(cfg.cookie_allowlist);
    let mut seen = std::collections::HashSet::new();
    cfg.sites = cfg
        .sites
        .into_iter()
        .filter_map(|mut site| {
            site.id = site.id.trim().to_string();
            site.label = site.label.trim().to_string();
            site.base_url = site.base_url.trim().trim_end_matches('/').to_string();
            site.cookie_domains = unique_strings(site.cookie_domains);
            site.allowed_hosts = unique_strings(site.allowed_hosts);
            site.allowed_path_prefixes = unique_strings(site.allowed_path_prefixes);
            site.default_headers = site
                .default_headers
                .into_iter()
                .filter_map(|(k, v)| {
                    let key = k.trim().to_string();
                    let value = v.trim().to_string();
                    if key.is_empty() || value.is_empty() {
                        None
                    } else {
                        Some((key, value))
                    }
                })
                .collect();
            if let Some(mut check) = site.login_check.take() {
                check.method = check.method.trim().to_uppercase();
                check.path = clean_path(&check.path).unwrap_or_else(|| "/".into());
                if check.expect == 0 {
                    check.expect = 200;
                }
                site.login_check = Some(check);
            }
            if site.id.is_empty() || site.base_url.is_empty() || !seen.insert(site.id.clone()) {
                None
            } else {
                Some(site)
            }
        })
        .collect();
    cfg
}

pub(crate) async fn api_auth_sites(State(state): State<AppState>) -> ApiResult<Json<Value>> {
    let cfg = read_config(&state).await?;
    let auth = normalize_browser_auth_config(cfg.browser_auth);
    let cookie_result = load_chrome_cookies(&auth).await;
    let (_source, cdp_status, cookies) = match cookie_result {
        Ok((source, cookies)) => {
            let message = format!("{} cookies available via {}", cookies.len(), source);
            (
                source.clone(),
                json!({ "connected": true, "source": source, "message": message }),
                cookies,
            )
        }
        Err(err) => (
            String::new(),
            json!({ "connected": false, "source": "none", "message": err.to_string() }),
            Vec::new(),
        ),
    };
    let effective_allowlist = effective_cookie_allowlist(&auth);
    let sites = auth
        .sites
        .iter()
        .map(|site| {
            let status = site_cookie_status(site, &cookies);
            json!({
                "id": site.id,
                "label": display_label(site),
                "enabled": site.enabled,
                "baseUrl": site.base_url,
                "allowedHosts": resolved_allowed_hosts(site),
                "allowedPathPrefixes": site.allowed_path_prefixes,
                "cookieDomains": resolved_cookie_domains(site),
                "loginCheck": site.login_check,
                "status": status,
            })
        })
        .collect::<Vec<_>>();
    Ok(Json(json!({
        "generatedAt": now_ms(),
        "config": auth,
        "cdp": cdp_status,
        "sites": sites,
        "security": {
            "returnsSecrets": false,
            "tokenPersistence": "none",
            "auditFile": state.data_dir.join(AUTH_AUDIT_FILE).to_string_lossy(),
            "allowlistEnforced": true,
            "cookieAllowlist": auth.cookie_allowlist,
            "effectiveAllowlist": effective_allowlist,
            "heldCookieCount": cookies.len(),
        }
    })))
}

pub(crate) async fn api_auth_site_check(
    State(state): State<AppState>,
    Path(site_id): Path<String>,
) -> ApiResult<Json<Value>> {
    let cfg = read_config(&state).await?;
    let auth = normalize_browser_auth_config(cfg.browser_auth);
    let site = find_site(&auth, &site_id)?;
    ensure_site_enabled(site)?;
    let cookies = load_chrome_cookies(&auth).await?.1;
    let cookie_status = site_cookie_status(site, &cookies);
    let login = if let Some(check) = &site.login_check {
        match send_site_request(site, check.method.as_str(), &check.path, HashMap::new(), None, None, &cookies).await {
            Ok(resp) => json!({
                "ok": resp.status == check.expect,
                "status": resp.status,
                "expected": check.expect,
                "contentType": resp.content_type,
                "bodyPreview": resp.body_text.chars().take(240).collect::<String>(),
            }),
            Err(err) => json!({ "ok": false, "error": err.to_string(), "expected": check.expect }),
        }
    } else {
        json!({ "ok": cookie_status.get("cookieCount").and_then(Value::as_u64).unwrap_or(0) > 0, "skipped": true, "reason": "loginCheck not configured" })
    };
    Ok(Json(json!({
        "ok": login.get("ok").and_then(Value::as_bool).unwrap_or(false),
        "generatedAt": now_ms(),
        "site": site.id,
        "status": cookie_status,
        "login": login,
    })))
}

pub(crate) async fn api_auth_site_request(
    State(state): State<AppState>,
    Path(site_id): Path<String>,
    FormOrJson(form): FormOrJson<BrowserAuthRequestForm>,
) -> ApiResult<Json<Value>> {
    let cfg = read_config(&state).await?;
    let auth = normalize_browser_auth_config(cfg.browser_auth);
    let site = find_site(&auth, &site_id)?;
    ensure_site_enabled(site)?;
    let method = form.method.trim().to_uppercase();
    let path = clean_path(&form.path).ok_or_else(|| ApiError::bad_request("path must start with /"))?;
    ensure_allowed_method(&method)?;
    ensure_allowed_path(site, &path)?;
    let cookies = load_chrome_cookies(&auth).await?.1;
    let result = send_site_request(
        site,
        &method,
        &path,
        form.headers,
        form.json,
        form.body,
        &cookies,
    )
    .await;
    match &result {
        Ok(resp) => {
            append_auth_audit(&state, site, &method, &path, Some(resp.status), None).await.ok();
            Ok(Json(resp.to_json()))
        }
        Err(err) => {
            append_auth_audit(&state, site, &method, &path, None, Some(&err.to_string()))
                .await
                .ok();
            Err(ApiError::from(anyhow!(err.to_string())))
        }
    }
}

fn find_site<'a>(auth: &'a BrowserAuthConfig, site_id: &str) -> ApiResult<&'a BrowserAuthSiteConfig> {
    auth.sites
        .iter()
        .find(|site| site.id == site_id)
        .ok_or_else(|| ApiError::bad_request(format!("unknown auth site: {site_id}")))
}

fn ensure_site_enabled(site: &BrowserAuthSiteConfig) -> ApiResult<()> {
    if site.enabled {
        Ok(())
    } else {
        Err(ApiError::bad_request(format!("auth site disabled: {}", site.id)))
    }
}

fn ensure_allowed_method(method: &str) -> ApiResult<()> {
    match method {
        "GET" | "POST" | "PUT" | "PATCH" | "DELETE" => Ok(()),
        _ => Err(ApiError::bad_request(format!("unsupported method: {method}"))),
    }
}

fn ensure_allowed_path(site: &BrowserAuthSiteConfig, path: &str) -> ApiResult<()> {
    if site.allowed_path_prefixes.is_empty() {
        return Err(ApiError::bad_request(format!(
            "auth site {} has no allowedPathPrefixes; refusing request",
            site.id
        )));
    }
    if site
        .allowed_path_prefixes
        .iter()
        .any(|prefix| path.starts_with(prefix))
    {
        Ok(())
    } else {
        Err(ApiError::bad_request(format!(
            "path {path} is not allowed for auth site {}",
            site.id
        )))
    }
}

fn clean_path(path: &str) -> Option<String> {
    let trimmed = path.trim();
    if trimmed.starts_with('/') && !trimmed.starts_with("//") {
        Some(trimmed.to_string())
    } else {
        None
    }
}

fn display_label(site: &BrowserAuthSiteConfig) -> String {
    if site.label.trim().is_empty() {
        site.id.clone()
    } else {
        site.label.clone()
    }
}

fn site_cookie_status(site: &BrowserAuthSiteConfig, cookies: &[ChromeCookie]) -> Value {
    let domains = resolved_cookie_domains(site);
    let matching = cookies
        .iter()
        .filter(|cookie| domains.iter().any(|domain| cookie_matches_domain(cookie, domain)))
        .collect::<Vec<_>>();
    let http_only = matching.iter().filter(|cookie| cookie.value.len() > 0).count();
    let domains = matching
        .iter()
        .map(|cookie| cookie.domain.clone())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    json!({
        "ok": !matching.is_empty(),
        "cookieCount": matching.len(),
        "matchedDomains": domains,
        "hasCookieValue": http_only > 0,
    })
}

fn resolved_allowed_hosts(site: &BrowserAuthSiteConfig) -> Vec<String> {
    if !site.allowed_hosts.is_empty() {
        return site.allowed_hosts.clone();
    }
    Url::parse(&site.base_url)
        .ok()
        .and_then(|url| url.host_str().map(str::to_string))
        .into_iter()
        .collect()
}

fn resolved_cookie_domains(site: &BrowserAuthSiteConfig) -> Vec<String> {
    if !site.cookie_domains.is_empty() {
        return site.cookie_domains.clone();
    }
    resolved_allowed_hosts(site)
}

/// 计算生效的 cookie 白名单：显式配置的 `cookie_allowlist` 优先；
/// 为空时回退为所有启用站点 cookieDomains 的并集（没有启用站点则为空 → 读取 0 条）。
fn effective_cookie_allowlist(auth: &BrowserAuthConfig) -> Vec<String> {
    if !auth.cookie_allowlist.is_empty() {
        return auth.cookie_allowlist.clone();
    }
    let mut domains = Vec::new();
    for site in &auth.sites {
        if site.enabled {
            domains.extend(resolved_cookie_domains(site));
        }
    }
    unique_strings(domains)
}

/// 读取 Chrome 登录 cookie：优先直读 cookie 数据库（无 CDP、无弹窗），
/// 失败时回退到 CDP（可能触发 Chrome “允许远程调试”确认）。
/// 只返回白名单域名内的 cookie。返回 (读取方式, cookies)。
async fn load_chrome_cookies(auth: &BrowserAuthConfig) -> Result<(String, Vec<ChromeCookie>)> {
    let allowlist = effective_cookie_allowlist(auth);
    match load_chrome_cookies_db(&allowlist).await {
        Ok(cookies) => Ok(("db".into(), cookies)),
        Err(db_err) => {
            tracing::warn!(error = %db_err, "Chrome cookie DB read failed; falling back to CDP");
            match load_chrome_cookies_cdp(auth, &allowlist).await {
                Ok(cookies) => Ok(("cdp".into(), cookies)),
                Err(cdp_err) => Err(anyhow!(
                    "cookie DB: {db_err:#}; CDP fallback: {cdp_err:#}"
                )),
            }
        }
    }
}

async fn load_chrome_cookies_cdp(
    auth: &BrowserAuthConfig,
    allowlist: &[String],
) -> Result<Vec<ChromeCookie>> {
    let ws = resolve_cdp_ws(auth).await?;
    let (mut stream, _) = connect_async(&ws)
        .await
        .with_context(|| format!("connect chrome devtools websocket {ws}"))?;
    let mut id = 1_i64;
    match cdp_call(&mut stream, &mut id, "Network.getAllCookies", None, None).await {
        Ok(result) => filter_cookies_by_allowlist(parse_cookies(result)?, allowlist),
        Err(first_err) => {
            let targets = cdp_call(&mut stream, &mut id, "Target.getTargets", None, None).await?;
            let target_id = targets
                .get("targetInfos")
                .and_then(Value::as_array)
                .and_then(|items| {
                    items.iter().find_map(|item| {
                        let ty = item.get("type")?.as_str()?;
                        let url = item.get("url").and_then(Value::as_str).unwrap_or("");
                        if ty == "page" && !url.starts_with("chrome://") && !url.starts_with("devtools://") {
                            item.get("targetId")?.as_str().map(str::to_string)
                        } else {
                            None
                        }
                    })
                })
                .ok_or_else(|| anyhow!("Network.getAllCookies failed ({first_err}); no page target available for fallback"))?;
            let attach = cdp_call(
                &mut stream,
                &mut id,
                "Target.attachToTarget",
                Some(json!({ "targetId": target_id, "flatten": true })),
                None,
            )
            .await?;
            let session_id = attach
                .get("sessionId")
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow!("Target.attachToTarget did not return sessionId"))?;
            let result = cdp_call(
                &mut stream,
                &mut id,
                "Network.getAllCookies",
                None,
                Some(session_id),
            )
            .await?;
            filter_cookies_by_allowlist(parse_cookies(result)?, allowlist)
        }
    }
}

/// 只保留白名单域名内的 cookie（域名匹配，支持子域）。
fn filter_cookies_by_allowlist(
    cookies: Vec<ChromeCookie>,
    allowlist: &[String],
) -> Result<Vec<ChromeCookie>> {
    if allowlist.is_empty() {
        return Ok(Vec::new());
    }
    Ok(cookies
        .into_iter()
        .filter(|cookie| {
            allowlist
                .iter()
                .any(|domain| cookie_matches_domain(cookie, domain))
        })
        .collect())
}

async fn resolve_cdp_ws(auth: &BrowserAuthConfig) -> Result<String> {
    if let Ok(ws) = std::env::var("BU_CDP_WS") {
        if !ws.trim().is_empty() {
            return Ok(ws);
        }
    }
    let base = if !auth.cdp_url.trim().is_empty() {
        Some(auth.cdp_url.trim().trim_end_matches('/').to_string())
    } else if let Ok(url) = std::env::var("BU_CDP_URL") {
        let clean = url.trim().trim_end_matches('/').to_string();
        (!clean.is_empty()).then_some(clean)
    } else {
        None
    };
    if let Some(base) = base {
        if let Ok(ws) = resolve_ws_from_version(&base).await {
            return Ok(ws);
        }
        if let Some(ws) = resolve_ws_from_devtools_active_port().await? {
            return Ok(ws);
        }
        return Err(anyhow!("cannot resolve Chrome websocket from {base}"));
    }
    if let Some(ws) = resolve_ws_from_devtools_active_port().await? {
        return Ok(ws);
    }
    for port in [9222_u16, 9223] {
        let base = format!("http://127.0.0.1:{port}");
        if let Ok(ws) = resolve_ws_from_version(&base).await {
            return Ok(ws);
        }
    }
    Err(anyhow!(
        "Chrome DevTools endpoint not found; enable chrome://inspect/#remote-debugging"
    ))
}

async fn resolve_ws_from_version(base: &str) -> Result<String> {
    let url = format!("{}/json/version", base.trim_end_matches('/'));
    let value: Value = reqwest::get(url).await?.json().await?;
    value
        .get("webSocketDebuggerUrl")
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| anyhow!("webSocketDebuggerUrl missing"))
}

async fn resolve_ws_from_devtools_active_port() -> Result<Option<String>> {
    let home = home_dir()?;
    let candidates: [PathBuf; 2] = [
        home.join(".config/google-chrome/DevToolsActivePort"),
        home.join(".config/chromium/DevToolsActivePort"),
    ];
    for path in candidates {
        let raw = match fs::read_to_string(&path).await {
            Ok(raw) => raw,
            Err(_) => continue,
        };
        let mut lines = raw.lines();
        let port = lines.next().unwrap_or_default().trim();
        let ws_path = lines.next().unwrap_or_default().trim();
        if !port.is_empty() && !ws_path.is_empty() {
            return Ok(Some(format!("ws://127.0.0.1:{port}{ws_path}")));
        }
    }
    Ok(None)
}

async fn cdp_call(
    stream: &mut WebSocketStream<MaybeTlsStream<TcpStream>>,
    id: &mut i64,
    method: &str,
    params: Option<Value>,
    session_id: Option<&str>,
) -> Result<Value> {
    let call_id = *id;
    *id += 1;
    let mut req = serde_json::Map::new();
    req.insert("id".into(), json!(call_id));
    req.insert("method".into(), json!(method));
    if let Some(params) = params {
        req.insert("params".into(), params);
    }
    if let Some(session_id) = session_id {
        req.insert("sessionId".into(), json!(session_id));
    }
    stream.send(Message::Text(Value::Object(req).to_string().into())).await?;
    while let Some(msg) = stream.next().await {
        let msg = msg?;
        let text = match msg {
            Message::Text(text) => text.to_string(),
            Message::Binary(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
            Message::Close(_) => break,
            _ => continue,
        };
        let value: Value = serde_json::from_str(&text)?;
        if value.get("id").and_then(Value::as_i64) != Some(call_id) {
            continue;
        }
        if let Some(error) = value.get("error") {
            return Err(anyhow!("CDP {method} failed: {error}"));
        }
        return Ok(value.get("result").cloned().unwrap_or(Value::Null));
    }
    Err(anyhow!("CDP {method} closed before response"))
}

fn parse_cookies(result: Value) -> Result<Vec<ChromeCookie>> {
    let raw = result
        .get("cookies")
        .cloned()
        .ok_or_else(|| anyhow!("CDP cookie response missing cookies"))?;
    Ok(serde_json::from_value(raw)?)
}

async fn send_site_request(
    site: &BrowserAuthSiteConfig,
    method: &str,
    path: &str,
    headers: HashMap<String, String>,
    json_body: Option<Value>,
    text_body: Option<String>,
    cookies: &[ChromeCookie],
) -> Result<AuthProxyResponse> {
    let url = site_url(site, path)?;
    ensure_allowed_host(site, &url)?;
    let cookie_header = cookie_header_for(site, &url, cookies)?;
    let client = reqwest::Client::builder()
        .user_agent("AgentPanelAuthBroker/0.1")
        .redirect(reqwest::redirect::Policy::limited(5))
        .build()?;
    let method = Method::from_bytes(method.as_bytes())?;
    let mut req = client.request(method, url.clone());
    if !cookie_header.is_empty() {
        req = req.header(header::COOKIE, cookie_header);
    }
    for (name, value) in site.default_headers.iter().chain(headers.iter()) {
        if is_forbidden_request_header(name) {
            continue;
        }
        req = req.header(name.as_str(), value.as_str());
    }
    if let Some(body) = json_body {
        req = req.json(&body);
    } else if let Some(body) = text_body {
        req = req.body(body);
    }
    let response = req.send().await?;
    let status = response.status().as_u16();
    let content_type = response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    let safe_headers = response
        .headers()
        .iter()
        .filter_map(|(name, value)| {
            if is_sensitive_response_header(name.as_str()) {
                None
            } else {
                value
                    .to_str()
                    .ok()
                    .map(|v| (name.as_str().to_string(), v.to_string()))
            }
        })
        .collect::<HashMap<_, _>>();
    let bytes = response.bytes().await?;
    let truncated = bytes.len() > AUTH_RESPONSE_LIMIT_BYTES;
    let slice = if truncated {
        &bytes[..AUTH_RESPONSE_LIMIT_BYTES]
    } else {
        &bytes[..]
    };
    let body_text = String::from_utf8_lossy(slice).into_owned();
    let body_json = serde_json::from_str::<Value>(&body_text).ok();
    Ok(AuthProxyResponse {
        status,
        url: format!("{} {}", status, safe_url_for_log(&url)),
        content_type,
        headers: safe_headers,
        body_text,
        body_json,
        truncated,
    })
}

fn site_url(site: &BrowserAuthSiteConfig, path: &str) -> Result<Url> {
    let base = Url::parse(&site.base_url).with_context(|| format!("invalid baseUrl for {}", site.id))?;
    base.join(path)
        .with_context(|| format!("invalid request path for {}: {path}", site.id))
}

fn ensure_allowed_host(site: &BrowserAuthSiteConfig, url: &Url) -> Result<()> {
    let host = url
        .host_str()
        .ok_or_else(|| anyhow!("request URL has no host"))?;
    if resolved_allowed_hosts(site).iter().any(|allowed| host == allowed) {
        Ok(())
    } else {
        Err(anyhow!("host {host} is not allowed for auth site {}", site.id))
    }
}

fn cookie_header_for(
    site: &BrowserAuthSiteConfig,
    url: &Url,
    cookies: &[ChromeCookie],
) -> Result<String> {
    let host = url.host_str().ok_or_else(|| anyhow!("URL has no host"))?;
    let path = url.path();
    let secure = url.scheme() == "https";
    let domains = resolved_cookie_domains(site);
    let mut parts = Vec::new();
    for cookie in cookies {
        if cookie.secure && !secure {
            continue;
        }
        if is_expired(cookie) {
            continue;
        }
        if !cookie_path_matches(cookie, path) {
            continue;
        }
        if !domains.iter().any(|domain| cookie_matches_domain(cookie, domain)) {
            continue;
        }
        if !cookie_domain_matches_host(cookie, host) {
            continue;
        }
        parts.push(format!("{}={}", cookie.name, cookie.value));
    }
    Ok(parts.join("; "))
}

fn cookie_matches_domain(cookie: &ChromeCookie, configured_domain: &str) -> bool {
    let domain = normalize_domain(&cookie.domain);
    let configured = normalize_domain(configured_domain);
    domain == configured || domain.ends_with(&format!(".{configured}")) || configured.ends_with(&format!(".{domain}"))
}

fn cookie_domain_matches_host(cookie: &ChromeCookie, host: &str) -> bool {
    let domain = normalize_domain(&cookie.domain);
    let host = normalize_domain(host);
    host == domain || host.ends_with(&format!(".{domain}"))
}

fn cookie_path_matches(cookie: &ChromeCookie, path: &str) -> bool {
    let cookie_path = if cookie.path.is_empty() { "/" } else { &cookie.path };
    path.starts_with(cookie_path)
}

fn normalize_domain(domain: &str) -> String {
    domain.trim().trim_start_matches('.').to_lowercase()
}

fn is_expired(cookie: &ChromeCookie) -> bool {
    cookie
        .expires
        .filter(|expires| *expires > 0.0)
        .map(|expires| expires < (now_ms() as f64 / 1000.0))
        .unwrap_or(false)
}

fn is_forbidden_request_header(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "cookie" | "authorization" | "proxy-authorization" | "host" | "connection" | "content-length"
    )
}

fn is_sensitive_response_header(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "set-cookie" | "cookie" | "authorization" | "proxy-authorization"
    )
}

fn safe_url_for_log(url: &Url) -> String {
    let mut clone = url.clone();
    clone.set_query(None);
    clone.to_string()
}

struct AuthProxyResponse {
    status: u16,
    url: String,
    content_type: Option<String>,
    headers: HashMap<String, String>,
    body_text: String,
    body_json: Option<Value>,
    truncated: bool,
}

impl AuthProxyResponse {
    fn to_json(&self) -> Value {
        json!({
            "ok": (200..300).contains(&self.status),
            "status": self.status,
            "url": self.url,
            "contentType": self.content_type,
            "headers": self.headers,
            "bodyText": self.body_text,
            "bodyJson": self.body_json,
            "truncated": self.truncated,
            "secretsReturned": false,
        })
    }
}

async fn append_auth_audit(
    state: &AppState,
    site: &BrowserAuthSiteConfig,
    method: &str,
    path: &str,
    status: Option<u16>,
    error: Option<&str>,
) -> Result<()> {
    let audit = json!({
        "ts": now_ms(),
        "site": site.id,
        "method": method,
        "path": path,
        "status": status,
        "ok": status.map(|s| (200..300).contains(&s)).unwrap_or(false),
        "error": error.map(|e| e.chars().take(280).collect::<String>()),
    });
    let path = state.data_dir.join(AUTH_AUDIT_FILE);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).await.ok();
    }
    let line = serde_json::to_string(&audit)? + "\n";
    let mut options = tokio::fs::OpenOptions::new();
    options.create(true).append(true);
    use tokio::io::AsyncWriteExt;
    let mut file = options.open(path).await?;
    file.write_all(line.as_bytes()).await?;
    Ok(())
}

#[cfg(test)]
mod browser_auth_tests {
    use super::*;

    fn cookie(domain: &str, path: &str, secure: bool) -> ChromeCookie {
        ChromeCookie {
            name: "sid".into(),
            value: "secret".into(),
            domain: domain.into(),
            path: path.into(),
            secure,
            expires: None,
        }
    }

    #[test]
    fn cookie_domain_matching_handles_parent_domains() {
        let c = cookie(".example.com", "/", true);
        assert!(cookie_domain_matches_host(&c, "kibana.example.com"));
        assert!(cookie_matches_domain(&c, "example.com"));
        assert!(!cookie_domain_matches_host(&c, "evil-example.com"));
    }

    #[test]
    fn path_must_be_whitelisted() {
        let site = BrowserAuthSiteConfig {
            id: "demo".into(),
            allowed_path_prefixes: vec!["/api/".into()],
            ..Default::default()
        };
        assert!(ensure_allowed_path(&site, "/api/status").is_ok());
        assert!(ensure_allowed_path(&site, "/admin").is_err());
    }
}
