use std::{env, path::PathBuf};

use axum::{
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde_json::{json, Value};

use crate::*;

pub(crate) async fn health() -> Json<Value> {
    Json(json!({ "ok": true, "ts": now_ms() }))
}

pub(crate) async fn api_notifications() -> Json<Value> {
    Json(json!({ "notifications": [] }))
}

pub(crate) async fn api_notifications_unread_count() -> Json<Value> {
    Json(json!({ "count": 0 }))
}

pub(crate) async fn ok_json() -> Json<Value> {
    Json(json!({ "ok": true }))
}

pub(crate) type ApiResult<T> = std::result::Result<T, ApiError>;

#[derive(Debug)]
pub(crate) struct ApiError {
    pub(crate) status: StatusCode,
    pub(crate) message: String,
}

impl ApiError {
    pub(crate) fn bad_request(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: message.into(),
        }
    }
}

impl<E> From<E> for ApiError
where
    E: Into<anyhow::Error>,
{
    fn from(err: E) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: err.into().to_string(),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let help = api_error_help(&self.message);
        let body = if let Some(help) = help {
            json!({ "error": self.message, "help": help })
        } else {
            json!({ "error": self.message })
        };
        (self.status, Json(body)).into_response()
    }
}

pub(crate) struct FormOrJson<T>(pub(crate) T);

#[axum::async_trait]
impl<S, T> axum::extract::FromRequest<S> for FormOrJson<T>
where
    S: Send + Sync,
    T: serde::de::DeserializeOwned,
{
    type Rejection = ApiError;

    async fn from_request(
        req: axum::extract::Request,
        state: &S,
    ) -> std::result::Result<Self, Self::Rejection> {
        let headers = req.headers().clone();
        if is_json(&headers) {
            let Json(value) = Json::<T>::from_request(req, state)
                .await
                .map_err(|e| ApiError::bad_request(e.to_string()))?;
            return Ok(Self(value));
        }
        let axum::extract::Form(value) = axum::extract::Form::<T>::from_request(req, state)
            .await
            .map_err(|e| ApiError::bad_request(e.to_string()))?;
        Ok(Self(value))
    }
}

pub(crate) fn is_json(headers: &HeaderMap) -> bool {
    headers
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.starts_with("application/json"))
        .unwrap_or(false)
}

pub(crate) fn agent_panel_skill_path(skill_name: &str) -> String {
    let local = env::current_dir()
        .ok()
        .map(|root| {
            root.join(".agents/skills")
                .join(skill_name)
                .join("SKILL.md")
        })
        .filter(|path| path.is_file());
    local
        .unwrap_or_else(|| {
            PathBuf::from("/home/hevin/Developer/company/WMS/.agents/skills")
                .join(skill_name)
                .join("SKILL.md")
        })
        .to_string_lossy()
        .to_string()
}

pub(crate) fn api_error_help(message: &str) -> Option<Value> {
    let lower = message.to_lowercase();
    if lower.contains("invalid intent") {
        return Some(json!({
            "skillName": "req-tracker",
            "skillPath": agent_panel_skill_path("req-tracker"),
            "why": "intent 传错或不符合阶段语义",
            "correctExamples": [
                "GET /api/requirement/context?id=<reqId>&for=agent&intent=clarification&budget=2000",
                "GET /api/requirement/context?id=<reqId>&for=agent&intent=self-test&budget=2000"
            ],
            "relatedDocs": [
                "/api/requirement/schema",
                "/api/requirement/edit-plan?id=<reqId>&intent=<intent>"
            ]
        }));
    }
    if lower.contains("missing token or doctype")
        || lower.contains("token is not a writable markdown doc")
        || lower.contains("unsupported requirement edit operation")
    {
        return Some(json!({
            "skillName": "req-create",
            "skillPath": agent_panel_skill_path("req-create"),
            "why": "文档 token / docType / edit operation 选错",
            "correctExamples": [
                "POST /api/requirement/edit {\"reqId\":\"<reqId>\",\"operation\":\"upsertSection\",\"token\":\"req.test\",\"heading\":\"自测证据\",\"content\":\"- ...\"}",
                "POST /api/requirement/edit {\"reqId\":\"<reqId>\",\"operation\":\"writeDoc\",\"docType\":\"test\",\"mode\":\"replace\",\"content\":\"# ...\"}"
            ],
            "relatedDocs": [
                "/api/requirement/edit-plan?id=<reqId>&intent=<intent>",
                "/api/requirement/schema"
            ]
        }));
    }
    if lower.contains("invalid status") {
        return Some(json!({
            "skillName": "req-tracker",
            "skillPath": agent_panel_skill_path("req-tracker"),
            "why": "状态值不在当前需求状态流转集合中",
            "correctExamples": [
                "POST /api/requirement/status {\"reqId\":\"<reqId>\",\"status\":\"开发中\",\"note\":\"...\"}",
                "POST /api/requirement/status {\"reqId\":\"<reqId>\",\"status\":\"自测中\",\"note\":\"...\"}"
            ],
            "relatedDocs": [
                "/api/requirement/review-gate?id=<reqId>",
                "/api/requirement/context?id=<reqId>&for=agent&intent=status&budget=2000"
            ]
        }));
    }
    if lower.contains("missing branches.json") || lower.contains("run req-branches-update first") {
        return Some(json!({
            "skillName": "req-branches-update",
            "skillPath": agent_panel_skill_path("req-branches-update"),
            "why": "分支范围文件缺失或未刷新",
            "correctExamples": [
                "python3 ~/.agents/scripts/req-branches-scan.py <req-id>",
                "GET /api/requirement/merge-options?id=<reqId>"
            ],
            "relatedDocs": [
                "/api/requirement/merge-status?id=<reqId>&target=test",
                "/api/requirement/master-diff"
            ]
        }));
    }
    if lower.contains("code review gate") || lower.contains("review gate") {
        return Some(json!({
            "skillName": "req-tracker",
            "skillPath": agent_panel_skill_path("req-tracker"),
            "why": "测试前代码审查门禁未通过或未明确豁免",
            "correctExamples": [
                "GET /api/requirement/review-gate?id=<reqId>",
                "POST /api/requirement/code-review {\"reqId\":\"<reqId>\"}"
            ],
            "relatedDocs": [
                "review.md",
                "code-review-ai.md"
            ]
        }));
    }
    None
}
