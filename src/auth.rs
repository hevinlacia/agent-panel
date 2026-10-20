//! Token 鉴权中间件（所有模式共用）。
//!
//! 通过 `PANEL_AUTH_TOKEN` 环境变量配置访问 token：
//! - 未配置：鉴权层不启用（本机个人使用保持历史行为，完全开放）。
//! - 已配置：`/api/*` 与 `/ws/*` 全部要求 token；`/health`、`/api/mode`、静态资源与 SPA fallback 豁免
//!   ——SPA 产物不含秘密，且前端「连接」页需要用 `/health` 做连通性与延迟测试。
//!
//! token 来源（任一匹配即可）：
//! - `Authorization: Bearer <token>`
//! - `X-Panel-Token: <token>`
//! - `?token=<token>`（WebSocket/EventSource 等无法设 header 的场景；支持 percent-encoding）
//!
//! 挂载顺序：main.rs 中本中间件 add 在 CorsLayer 之前（内层），跨域 preflight
//! OPTIONS 由 CorsLayer 直接响应，不会到达本层。

use axum::{
    body::Body,
    extract::{Request, State},
    http::{header, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;

/// 豁免鉴权的路径前缀/精确路径：健康检查、模式探测、静态资源、SPA 入口。
fn is_public_path(path: &str) -> bool {
    if path == "/health" || path == "/api/mode" {
        return true;
    }
    // SPA 静态资源与页面入口；API/WS 之外的一切都视为前端资源（含无尾斜杠的精确路径）。
    !(path == "/api" || path.starts_with("/api/") || path == "/ws" || path.starts_with("/ws/"))
}

fn extract_token(req: &Request<Body>) -> Option<String> {
    if let Some(v) = req.headers().get(header::AUTHORIZATION) {
        if let Ok(s) = v.to_str() {
            if let Some(token) = s.strip_prefix("Bearer ") {
                return Some(token.trim().to_string());
            }
        }
    }
    if let Some(v) = req.headers().get("X-Panel-Token") {
        if let Ok(s) = v.to_str() {
            return Some(s.trim().to_string());
        }
    }
    // hub → worker 代理转发专用头（worker 节点用 PANEL_WORKER_TOKEN/PANEL_AUTH_TOKEN 校验）。
    if let Some(v) = req.headers().get("X-Worker-Token") {
        if let Ok(s) = v.to_str() {
            return Some(s.trim().to_string());
        }
    }
    // 查询参数兜底：?token=xxx（供 WebSocket/EventSource 等场景）。
    req.uri().query().and_then(|q| {
        q.split('&').find_map(|pair| {
            let (k, v) = pair.split_once('=')?;
            (k == "token").then(|| percent_decode(v))
        })
    })
}

/// 常量时间比较，避免响应时间侧信道泄露 token 前缀。
fn token_matches(provided: &str, expected: &str) -> bool {
    if provided.len() != expected.len() {
        return false;
    }
    provided
        .bytes()
        .zip(expected.bytes())
        .fold(0u8, |acc, (a, b)| acc | (a ^ b))
        == 0
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

fn unauthorized() -> Response {
    (
        StatusCode::UNAUTHORIZED,
        Json(json!({
            "error": "unauthorized",
            "message": "缺少或错误的访问 token。请求头携带 Authorization: Bearer <token> 或 X-Panel-Token: <token>。"
        })),
    )
        .into_response()
}

/// axum 中间件：`State<Option<String>>` 为期望 token（None = 不启用鉴权）。
pub(crate) async fn require_token_middleware(
    State(expected): State<Option<String>>,
    req: Request<Body>,
    next: Next,
) -> Response {
    match expected {
        None => next.run(req).await,
        Some(expected) => {
            if is_public_path(req.uri().path()) {
                return next.run(req).await;
            }
            match extract_token(&req) {
                Some(token) if token_matches(&token, &expected) => next.run(req).await,
                _ => {
                    tracing::warn!(path = %req.uri().path(), "unauthorized request (invalid or missing token)");
                    unauthorized()
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_api_and_ws_paths_are_protected() {
        for path in [
            "/health",
            "/api/mode",
            "/assets/x.js",
            "/static/a",
            "/dashboard",
            "/",
        ] {
            assert!(is_public_path(path), "path {path} 应豁免");
        }
        for path in ["/api", "/api/dashboard/stats", "/ws", "/ws/pi-chat"] {
            assert!(!is_public_path(path), "path {path} 应受保护");
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

    #[test]
    fn token_matches_is_constant_time_and_exact() {
        assert!(token_matches("secret", "secret"));
        assert!(!token_matches("secret", "secreT"));
        assert!(!token_matches("secret", "secretx"));
        assert!(!token_matches("", "secret"));
    }
}
