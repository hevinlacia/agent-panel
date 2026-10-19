//! 认证接缝（auth seam）：`PANEL_AUTH_TOKEN` 未配置时保持历史行为（完全开放，
//! 面向 localhost 个人使用）；配置后对全部 `/api/*` 与 `/ws/*` 请求强制校验，
//! 其余路径（`/health`、静态资源、SPA fallback）保持豁免——SPA 产物不含秘密，
//! 且前端「连接」页需要用 `/health` 做连通性与延迟测试。
//!
//! 凭证形态：
//! - `Authorization: Bearer <token>`（普通 API 请求）；
//! - `?token=<token>` query（浏览器 WebSocket 无法携带自定义 header，统一走 query）。
//!
//! 挂载顺序：main.rs 中本中间件 add 在 CorsLayer 之前（内层），跨域 preflight
//! OPTIONS 由 CorsLayer 直接响应，不会到达本层。

use std::sync::OnceLock;

use axum::{
    extract::Request,
    http::{header, HeaderMap, StatusCode, Uri},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;

/// 读取一次并缓存 `PANEL_AUTH_TOKEN`；None = 未配置（认证关闭）。
fn configured_token() -> Option<&'static str> {
    static TOKEN: OnceLock<Option<String>> = OnceLock::new();
    TOKEN
        .get_or_init(|| {
            std::env::var("PANEL_AUTH_TOKEN")
                .ok()
                .map(|t| t.trim().to_string())
                .filter(|t| !t.is_empty())
        })
        .as_deref()
}

/// 只有 `/api` 与 `/ws` 前缀受保护；`/health`、`/assets`、`/static`、SPA fallback 全部豁免。
fn is_protected(path: &str) -> bool {
    path == "/api" || path.starts_with("/api/") || path == "/ws" || path.starts_with("/ws/")
}

/// 检查 Bearer header 或 `?token=` query 是否匹配。
fn request_authorized(uri: &Uri, headers: &HeaderMap) -> bool {
    let Some(expected) = configured_token() else {
        return true;
    };
    if let Some(value) = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
    {
        if value
            .trim()
            .eq_ignore_ascii_case(&format!("Bearer {expected}"))
        {
            return true;
        }
    }
    if let Some(query) = uri.query() {
        for pair in query.split('&') {
            let mut parts = pair.splitn(2, '=');
            if parts.next() != Some("token") {
                continue;
            }
            if let Some(raw) = parts.next() {
                if percent_decode(raw) == expected {
                    return true;
                }
            }
        }
    }
    false
}

/// 最小 percent-decode（token 常见字符集无需完整 URL 解码；`+` 不转空格）。
fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            if let Some(hex) = bytes.get(i + 1..i + 3) {
                if let Ok(v) = u8::from_str_radix(std::str::from_utf8(hex).unwrap_or(""), 16) {
                    out.push(v);
                    i += 3;
                    continue;
                }
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// axum middleware：未配置 token 时直通；配置后仅拦截未授权的 `/api`、`/ws` 请求，
/// 其余路径（health/静态/SPA）始终放行。
pub(crate) async fn require_auth(req: Request, next: Next) -> Response {
    let (parts, body) = req.into_parts();
    let authorized =
        !is_protected(parts.uri.path()) || request_authorized(&parts.uri, &parts.headers);
    let req = Request::from_parts(parts, body);
    if authorized {
        return next.run(req).await;
    }
    tracing::warn!(path = %req.uri().path(), "unauthorized request (invalid or missing token)");
    (
        StatusCode::UNAUTHORIZED,
        Json(json!({
            "code": "unauthorized",
            "message": "认证失败：token 缺失或不匹配（PANEL_AUTH_TOKEN）",
        })),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn uri(path: &str) -> Uri {
        Uri::builder().path_and_query(path).build().unwrap()
    }

    #[test]
    fn only_api_and_ws_paths_are_protected() {
        for path in ["/health", "/assets/x.js", "/static/a", "/dashboard", "/"] {
            assert!(!is_protected(path), "path {path} 应豁免");
        }
        for path in ["/api", "/api/dashboard/stats", "/ws", "/ws/pi-chat"] {
            assert!(is_protected(path), "path {path} 应受保护");
        }
    }

    #[test]
    fn percent_decode_basic() {
        assert_eq!(percent_decode("abc"), "abc");
        assert_eq!(percent_decode("a%20b"), "a b");
        assert_eq!(percent_decode("a%2Fb"), "a/b");
        assert_eq!(percent_decode("bad%zz"), "bad%zz");
        assert_eq!(percent_decode("trail%2"), "trail%2");
    }
}
