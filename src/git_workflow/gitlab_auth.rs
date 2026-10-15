//! GitLab 三步式认证链（生成生产 MR 用）。
//!
//! 解析优先级（命中即返回，全部失败时报出各步原因）：
//! 1. **Chrome 登录态**：读 Chrome cookie（DB 直读免弹窗，失败回退 CDP）取
//!    `_gitlab_session`，用 `GET /api/v4/user` 校验会话有效，再从站点首页 HTML
//!    抓 CSRF token（会话态调 API 的 POST 必带 `X-CSRF-Token`）。
//! 2. **GitLab 账号密码**：`GITLAB_USERNAME`（别名 `GITLAB_USER`）+
//!    `GITLAB_PASSWORD`（别名 `GITLAB_PASS`），环境变量或 panel 运行目录
//!    `.env.agent`；模拟 `/users/sign_in` 表单登录换取会话 cookie。
//! 3. **GITLAB_TOKEN**：`GITLAB_TOKEN` / `GL_TOKEN` / `.env.agent` 的
//!    `GITLAB_TOKEN`，走 `PRIVATE-TOKEN` 直连（原有行为，降级为最后兜底）。
//!
//! 安全约束：cookie/token/密码只在内存流转，不进日志、不落盘、不回显；
//! 登录失败时不带出 GitLab 响应 body（避免回渲染页面里的敏感信息）。

use super::*;
use reqwest::header;

const GITLAB_SESSION_COOKIE: &str = "_gitlab_session";
const GITLAB_WEB_URL_ENV: &str = "GITLAB_WEB_URL";
const GITLAB_AUTH_USER_AGENT: &str = "AgentPanelGitLabAuth/0.1";

/// GitLab API 认证方式（三步式解析的产物）。
#[derive(Debug, Clone)]
pub(crate) enum GitLabAuth {
    /// 会话态认证：Cookie 请求头 + CSRF token（API 非幂等请求必带 `X-CSRF-Token`）。
    Session {
        cookie_header: String,
        csrf_token: String,
    },
    /// 个人访问令牌认证：`PRIVATE-TOKEN` 请求头。
    Token(String),
}

/// 认证链解析结果：认证方式 + 命中的来源（用于响应里的 authSource 便于排障）。
#[derive(Debug, Clone)]
pub(crate) struct GitLabAuthContext {
    pub(crate) auth: GitLabAuth,
    /// "chrome-session" / "password-login" / "env-token"
    pub(crate) source: &'static str,
}

impl GitLabAuth {
    /// 为 GitLab API 请求附加认证头。
    pub(crate) fn apply(&self, req: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        match self {
            GitLabAuth::Session {
                cookie_header,
                csrf_token,
            } => req
                .header(header::COOKIE, cookie_header)
                .header("X-CSRF-Token", csrf_token),
            GitLabAuth::Token(token) => req.header("PRIVATE-TOKEN", token),
        }
    }
}

/// 三步式解析 GitLab 认证。任一步命中即返回；全部失败时返回带各步原因的聚合错误。
pub(crate) async fn resolve_gitlab_auth(
    api_base: &str,
    auth_cfg: &BrowserAuthConfig,
) -> Result<GitLabAuthContext> {
    let web_base = env_gitlab_web_url().unwrap_or_else(|| gitlab_web_base(api_base));
    let client = Client::builder()
        .timeout(Duration::from_secs(30))
        // 关闭自动重定向：登录成功/失败靠 Set-Cookie + 会话校验判断，不跟随 302
        .redirect(reqwest::redirect::Policy::none())
        .user_agent(GITLAB_AUTH_USER_AGENT)
        .build()
        .context("build GitLab auth client")?;
    let mut failures: Vec<String> = Vec::new();

    // Step 1: Chrome 登录态
    match gitlab_session_from_chrome(&client, api_base, &web_base, auth_cfg).await {
        Ok(ctx) => return Ok(ctx),
        Err(err) => failures.push(format!("Chrome 登录态不可用：{err:#}")),
    }

    // Step 2: 账号密码登录
    match gitlab_session_from_password(&client, api_base, &web_base).await {
        Ok(ctx) => return Ok(ctx),
        Err(err) => failures.push(format!("账号密码登录失败：{err:#}")),
    }

    // Step 3: GITLAB_TOKEN / GL_TOKEN / .env.agent
    match gitlab_api_token() {
        Ok(token) if !token.is_empty() => {
            return Ok(GitLabAuthContext {
                auth: GitLabAuth::Token(token),
                source: "env-token",
            });
        }
        Ok(_) => failures.push("GITLAB_TOKEN 配置为空".into()),
        Err(err) => failures.push(format!("GITLAB_TOKEN 未配置：{err}")),
    }

    Err(anyhow!(
        "GitLab 认证失败（三步均未命中）：{}。可在 panel 运行目录建 .env.agent 写入 GITLAB_TOKEN，\
         或配置 GITLAB_USERNAME / GITLAB_PASSWORD 走账号密码登录，或在 Chrome 登录 {} 后重试。",
        failures.join("；"),
        web_base
    ))
}

/// Step 1：从 Chrome 登录态取 GitLab 会话。
async fn gitlab_session_from_chrome(
    client: &Client,
    api_base: &str,
    web_base: &str,
    auth_cfg: &BrowserAuthConfig,
) -> Result<GitLabAuthContext> {
    let domain = gitlab_host(web_base)?;
    let allowlist = vec![domain];
    let (source, cookies) = load_chrome_cookies_with_allowlist(auth_cfg, &allowlist)
        .await
        .context("读取 Chrome cookie 失败")?;
    let cookie_header = session_cookie_header(&cookies);
    if cookie_header.is_empty() {
        return Err(anyhow!(
            "{source} 未读到 GitLab 的 {GITLAB_SESSION_COOKIE}（Chrome 未登录 GitLab 或 cookie 已过期）"
        ));
    }
    finish_session_auth(client, api_base, web_base, &cookie_header, "chrome-session").await
}

/// Step 2：用账号密码模拟 `/users/sign_in` 表单登录换会话。
async fn gitlab_session_from_password(
    client: &Client,
    api_base: &str,
    web_base: &str,
) -> Result<GitLabAuthContext> {
    let username = read_gitlab_credential("GITLAB_USERNAME", "GITLAB_USER");
    let password = read_gitlab_credential("GITLAB_PASSWORD", "GITLAB_PASS");
    if username.is_empty() && password.is_empty() {
        return Err(anyhow!(
            "未配置 GITLAB_USERNAME / GITLAB_PASSWORD（环境变量或 .env.agent）"
        ));
    }
    if username.is_empty() || password.is_empty() {
        return Err(anyhow!(
            "GITLAB_USERNAME / GITLAB_PASSWORD 只配置了一半，请补全"
        ));
    }

    // 1) 取登录页：GitLab 会下发一个全新 _gitlab_session，表单 authenticity_token
    //    与该会话绑定（与页面 meta csrf-token 同值），必须连同 cookie 一起提交。
    let login_url = format!("{}/users/sign_in", web_base.trim_end_matches('/'));
    let page = client
        .get(&login_url)
        .send()
        .await
        .context("请求 GitLab 登录页失败")?;
    if !page.status().is_success() {
        return Err(anyhow!("GitLab 登录页 HTTP {}", page.status().as_u16()));
    }
    let pre_cookies = set_cookie_values(&page);
    let page_html = page.text().await.unwrap_or_default();
    let csrf =
        parse_csrf_token(&page_html).ok_or_else(|| anyhow!("GitLab 登录页缺少 csrf-token meta"))?;
    let cookie_header = merge_cookie_header("", &pre_cookies);
    if cookie_header.is_empty() {
        return Err(anyhow!("GitLab 登录页未下发会话 cookie"));
    }

    // 2) 提交登录表单。不打印响应 body：失败时 GitLab 回渲染登录页，含敏感上下文。
    let form = [
        ("authenticity_token", csrf.as_str()),
        ("user[login]", username.as_str()),
        ("user[password]", password.as_str()),
        ("user[remember_me]", "0"),
    ];
    let resp = client
        .post(&login_url)
        .header(header::COOKIE, &cookie_header)
        .header("X-CSRF-Token", &csrf)
        .form(&form)
        .send()
        .await
        .context("提交 GitLab 登录表单失败")?;
    let status = resp.status();
    let login_cookies = set_cookie_values(&resp);
    let merged = merge_cookie_header(&cookie_header, &login_cookies);
    if merged.is_empty() {
        return Err(anyhow!(
            "GitLab 登录响应未返回会话 cookie（HTTP {}）",
            status.as_u16()
        ));
    }
    finish_session_auth(client, api_base, web_base, &merged, "password-login")
        .await
        .map_err(|err| {
            anyhow!(
                "登录后会话校验未通过（登录响应 HTTP {}，账号或密码错误？）：{err:#}",
                status.as_u16()
            )
        })
}

/// 会话态认证收尾：校验会话有效（GET /api/v4/user）+ 抓 CSRF token。
async fn finish_session_auth(
    client: &Client,
    api_base: &str,
    web_base: &str,
    cookie_header: &str,
    source: &'static str,
) -> Result<GitLabAuthContext> {
    let user_url = format!("{}/user", api_base.trim_end_matches('/'));
    let resp = client
        .get(&user_url)
        .header(header::COOKIE, cookie_header)
        .send()
        .await
        .context("校验 GitLab 会话失败")?;
    if !resp.status().is_success() {
        return Err(anyhow!(
            "会话已失效（GET /user 返回 {}）",
            resp.status().as_u16()
        ));
    }
    let csrf_token = fetch_csrf_token(client, web_base, cookie_header).await?;
    Ok(GitLabAuthContext {
        auth: GitLabAuth::Session {
            cookie_header: cookie_header.to_string(),
            csrf_token,
        },
        source,
    })
}

/// 抓站点首页 HTML 的 csrf-token meta（会话态调 API POST 必需）。
async fn fetch_csrf_token(client: &Client, web_base: &str, cookie_header: &str) -> Result<String> {
    let url = format!("{}/", web_base.trim_end_matches('/'));
    let resp = client
        .get(&url)
        .header(header::COOKIE, cookie_header)
        .send()
        .await
        .context("请求 GitLab 首页失败")?;
    let status = resp.status();
    let html = resp.text().await.unwrap_or_default();
    parse_csrf_token(&html).ok_or_else(|| {
        anyhow!(
            "GitLab 首页 HTTP {} 未包含 csrf-token meta",
            status.as_u16()
        )
    })
}

/// 由 GitLab API base 推导 web 站点根地址（登录页 / CSRF 抓取用）。
/// 纯推导，不做 IO；`GITLAB_WEB_URL` 覆盖逻辑在 [`resolve_gitlab_auth`]。
pub(crate) fn gitlab_web_base(api_base: &str) -> String {
    let trimmed = api_base.trim().trim_end_matches('/');
    trimmed
        .strip_suffix("/api/v4")
        .map(str::to_string)
        .unwrap_or_else(|| trimmed.to_string())
}

/// `GITLAB_WEB_URL` 显式覆盖（环境变量或 .env.agent）。
fn env_gitlab_web_url() -> Option<String> {
    env::var(GITLAB_WEB_URL_ENV)
        .ok()
        .or_else(|| read_agent_panel_env_var(GITLAB_WEB_URL_ENV).ok())
        .map(|v| v.trim().trim_end_matches('/').to_string())
        .filter(|v| !v.is_empty())
}

/// web 站点根地址的 host（cookie 白名单域名）。
fn gitlab_host(web_base: &str) -> Result<String> {
    reqwest::Url::parse(web_base)
        .ok()
        .and_then(|url| url.host_str().map(str::to_string))
        .ok_or_else(|| anyhow!("无法解析 GitLab web 地址 host：{web_base}"))
}

/// 从 cookie 列表中提取未过期的 `_gitlab_session`，拼成 Cookie 请求头。
pub(crate) fn session_cookie_header(cookies: &[ChromeCookie]) -> String {
    let now_secs = now_ms() as f64 / 1000.0;
    cookies
        .iter()
        .filter(|c| c.name == GITLAB_SESSION_COOKIE)
        .filter(|c| {
            c.expires
                .map(|exp| exp <= 0.0 || exp > now_secs)
                .unwrap_or(true)
        })
        .map(|c| format!("{}={}", c.name, c.value))
        .collect::<Vec<_>>()
        .join("; ")
}

/// 把响应的 Set-Cookie 值合并进已有 Cookie 头：同名（如登录后轮换的
/// `_gitlab_session`）覆盖旧值，新名追加，其余保持原有顺序。
pub(crate) fn merge_cookie_header(base: &str, set_cookies: &[String]) -> String {
    let mut pairs: Vec<(String, String)> = base
        .split("; ")
        .filter(|part| !part.is_empty())
        .map(|part| match part.split_once('=') {
            Some((name, value)) => (name.to_string(), value.to_string()),
            None => (part.to_string(), String::new()),
        })
        .collect();
    for raw in set_cookies {
        // 只取 name=value 段，忽略 path/secure/HttpOnly 等属性
        let Some(name_value) = raw.split(';').next() else {
            continue;
        };
        let Some((name, value)) = name_value.split_once('=') else {
            continue;
        };
        let name = name.trim();
        if name.is_empty() {
            continue;
        }
        let value = value.trim().to_string();
        match pairs.iter_mut().find(|(existing, _)| existing == name) {
            Some((_, slot)) => *slot = value,
            None => pairs.push((name.to_string(), value)),
        }
    }
    pairs
        .iter()
        .map(|(name, value)| format!("{name}={value}"))
        .collect::<Vec<_>>()
        .join("; ")
}

/// 从 HTML 中提取 `<meta name="csrf-token" content="...">` 的 token 值
/// （兼容 content 属性在 name 之前的写法）。
pub(crate) fn parse_csrf_token(html: &str) -> Option<String> {
    let name_first = Regex::new(r#"<meta\b[^>]*\bname=["']csrf-token["'][^>]*>"#).ok()?;
    let tag = name_first
        .find(html)
        .map(|m| m.as_str().to_string())
        .or_else(|| {
            let content_first = Regex::new(
                r#"<meta\b[^>]*\bcontent=["'][^"']*["'][^>]*\bname=["']csrf-token["'][^>]*>"#,
            )
            .ok()?;
            content_first.find(html).map(|m| m.as_str().to_string())
        })?;
    let content_re = Regex::new(r#"\bcontent=["']([^"']+)["']"#).ok()?;
    Some(content_re.captures(&tag)?.get(1)?.as_str().to_string())
}

/// 读取 GitLab 登录凭证：环境变量优先，其次 panel 运行目录 `.env.agent`。
/// 未配置时返回空串（由调用方判断）。
fn read_gitlab_credential(primary: &str, alias: &str) -> String {
    env::var(primary)
        .or_else(|_| env::var(alias))
        .or_else(|_| read_agent_panel_env_var(primary))
        .or_else(|_| read_agent_panel_env_var(alias))
        .map(|v| v.trim().to_string())
        .unwrap_or_default()
}

/// 收集响应的全部 Set-Cookie 头值。
fn set_cookie_values(response: &reqwest::Response) -> Vec<String> {
    response
        .headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok().map(str::to_string))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cookie(name: &str, value: &str, expires: Option<f64>) -> ChromeCookie {
        ChromeCookie {
            name: name.into(),
            value: value.into(),
            domain: ".code.jms.com".into(),
            path: "/".into(),
            secure: false,
            expires,
        }
    }

    #[test]
    fn web_base_strips_api_suffix() {
        assert_eq!(
            gitlab_web_base("http://code.jms.com/api/v4"),
            "http://code.jms.com"
        );
        assert_eq!(
            gitlab_web_base("http://code.jms.com/api/v4/"),
            "http://code.jms.com"
        );
        assert_eq!(
            gitlab_web_base("http://code.jms.com"),
            "http://code.jms.com"
        );
        assert_eq!(
            gitlab_web_base("https://git.example.com/api/v4"),
            "https://git.example.com"
        );
    }

    #[test]
    fn csrf_meta_parsing_handles_attribute_orders() {
        let html = r#"<html><head><meta name="csrf-token" content="abc123==" /><meta name="csp-nonce" content="x"></head></html>"#;
        assert_eq!(parse_csrf_token(html).as_deref(), Some("abc123=="));
        assert_eq!(
            parse_csrf_token(r#"<meta content="tok" name="csrf-token">"#).as_deref(),
            Some("tok")
        );
        assert_eq!(parse_csrf_token("<html></html>"), None);
    }

    #[test]
    fn session_cookie_keeps_only_gitlab_session() {
        let cookies = vec![
            cookie("_gitlab_session", "s3cret", Some(9999999999.0)),
            cookie("known_signin", "someuser", Some(9999999999.0)),
        ];
        assert_eq!(session_cookie_header(&cookies), "_gitlab_session=s3cret");
    }

    #[test]
    fn session_cookie_drops_expired() {
        let cookies = vec![cookie("_gitlab_session", "old", Some(1.0))];
        assert_eq!(session_cookie_header(&cookies), "");
        // 无 expires 视为未过期
        let cookies = vec![cookie("_gitlab_session", "live", None)];
        assert_eq!(session_cookie_header(&cookies), "_gitlab_session=live");
    }

    #[test]
    fn merge_overrides_session_and_appends_new() {
        let merged = merge_cookie_header(
            "_gitlab_session=old; known_signin=root",
            &[
                "_gitlab_session=new; path=/; HttpOnly".to_string(),
                "experiment=1; path=/".to_string(),
            ],
        );
        assert_eq!(
            merged,
            "_gitlab_session=new; known_signin=root; experiment=1"
        );
    }

    #[test]
    fn merge_into_empty_base() {
        let merged = merge_cookie_header("", &["_gitlab_session=fresh; path=/".to_string()]);
        assert_eq!(merged, "_gitlab_session=fresh");
    }
}
