//! Hub 模式（`--mode docs`）token 鉴权。
//!
//! 通过 `PANEL_AUTH_TOKEN` 环境变量配置访问 token：
//! - 未配置：鉴权层不启用（本机 local 模式不受影响）。
//! - 已配置：`/api/*` 与 `/ws/*` 全部要求 token；`/health`、`/api/mode`、静态资源与 SPA fallback 豁免。
//!
//! token 来源（任一匹配即可）：
//! - `Authorization: Bearer <token>`
//! - `X-Panel-Token: <token>`
//! - `?token=<token>`（EventSource/浏览器直接打开等无法设 header 的场景）

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
    // SPA 静态资源与页面入口；API/WS 之外的一切都视为前端资源。
    !path.starts_with("/api/") && !path.starts_with("/ws/")
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
    // 查询参数兜底：?token=xxx（供 EventSource 等场景）。
    req.uri().query().and_then(|q| {
        q.split('&').find_map(|pair| {
            let (k, v) = pair.split_once('=')?;
            (k == "token").then(|| v.to_string())
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
                _ => unauthorized(),
            }
        }
    }
}
