use std::{collections::BTreeMap, env, sync::{OnceLock, RwLock}};

use axum::{
    extract::State,
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

    pub(crate) fn not_found(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
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

/// 全局 skill 位置映射快照：config 读取/保存时同步（IntoResponse / 无 state 的同步路径
/// 也要能拿到 override，所以不走 AppState）。
static SKILL_PATH_OVERRIDES: OnceLock<RwLock<BTreeMap<String, String>>> = OnceLock::new();

fn skill_overrides_cell() -> &'static RwLock<BTreeMap<String, String>> {
    SKILL_PATH_OVERRIDES.get_or_init(|| RwLock::new(BTreeMap::new()))
}

/// 同步 skill 位置映射到全局快照（内容不变时无写入开销）。
pub(crate) fn set_skill_path_overrides(map: BTreeMap<String, String>) {
    if let Ok(mut guard) = skill_overrides_cell().write() {
        if *guard != map {
            *guard = map;
        }
    }
}

fn skill_overrides_snapshot() -> BTreeMap<String, String> {
    skill_overrides_cell().read().map(|g| g.clone()).unwrap_or_default()
}

/// 解析 skill 的 SKILL.md 路径：override（支持 skills 根目录 / 具体 skill 目录 / .md 文件三种写法）
/// → panel cwd/.agents/skills → WMS 工作区 .agents/skills。override 命中后不再回退默认链，
/// 路径是否存在由 /api/config/skills 检查暴露，避免 skill 搬家后功能静默失效。
pub(crate) fn resolve_skill_path_with(
    overrides: &BTreeMap<String, String>,
    skill_name: &str,
) -> String {
    if let Some(value) = overrides.get(skill_name) {
        let base = std::path::PathBuf::from(value);
        let candidate = if base.is_file() {
            base
        } else if base.join(skill_name).is_dir() {
            base.join(skill_name).join("SKILL.md")
        } else {
            base.join("SKILL.md")
        };
        return candidate.to_string_lossy().to_string();
    }
    default_skill_path(skill_name)
}

fn default_skill_path(skill_name: &str) -> String {
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
            crate::paths::wms_skills_dir()
                .join(skill_name)
                .join("SKILL.md")
        })
        .to_string_lossy()
        .to_string()
}

pub(crate) fn agent_panel_skill_path(skill_name: &str) -> String {
    let snapshot = skill_overrides_snapshot();
    resolve_skill_path_with(&snapshot, skill_name)
}

/// panel 依赖的 skill 注册表：设置页「Skill 路径映射」逐个展示解析结果与生效状态。
pub(crate) struct SkillDependDef {
    pub(crate) name: &'static str,
    pub(crate) label: &'static str,
    pub(crate) used_by: &'static str,
}

pub(crate) static SKILL_DEPENDS: &[SkillDependDef] = &[
    SkillDependDef {
        name: "req-tracker",
        label: "需求跟踪与上下文 intent 指引",
        used_by: "API 错误帮助（intent / status 类错误自动附 skill 指引）",
    },
    SkillDependDef {
        name: "req-create",
        label: "需求文档写入规范",
        used_by: "API 错误帮助（token / docType / edit 类错误）",
    },
    SkillDependDef {
        name: "req-branches-update",
        label: "分支范围登记与刷新",
        used_by: "API 错误帮助（branches.json 缺失类错误）",
    },
    SkillDependDef {
        name: "agent-panel-code-review",
        label: "代码审查门禁流程",
        used_by: "API 错误帮助 + 状态门禁 review-gate 推进指引",
    },
    SkillDependDef {
        name: "wms-test-data-creation",
        label: "WMS 测试造数能力入口",
        used_by: "能力索引 / 详情 API 的 help 指引",
    },
];

/// GET /api/config/skills：逐个检查 panel 依赖的 skill 解析结果与生效状态。
pub(crate) async fn api_config_skills(State(state): State<AppState>) -> ApiResult<Json<Value>> {
    let cfg = read_config(&state).await?;
    let skills: Vec<Value> = SKILL_DEPENDS
        .iter()
        .map(|d| {
            let path = agent_panel_skill_path(d.name);
            let meta = std::fs::metadata(&path).ok();
            let exists = meta.as_ref().map(|m| m.is_file()).unwrap_or(false);
            let bytes = meta.map(|m| m.len()).unwrap_or(0);
            let override_path = cfg.skill_path_overrides.get(d.name).cloned();
            json!({
                "name": d.name,
                "label": d.label,
                "usedBy": d.used_by,
                "source": if override_path.is_some() { "override" } else { "default" },
                "overridePath": override_path,
                "path": path,
                "exists": exists,
                "bytes": bytes,
            })
        })
        .collect();
    let all_ok = skills
        .iter()
        .all(|s| s.get("exists").and_then(Value::as_bool).unwrap_or(false));
    Ok(Json(json!({ "ok": true, "allOk": all_ok, "skills": skills })))
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
            "skillName": "agent-panel-code-review",
            "skillPath": agent_panel_skill_path("agent-panel-code-review"),
            "why": "代码审查门禁未通过：审查必须按该 skill 流程执行（备料 → 深度审查 → 结论落盘 → 门禁自检）",
            "correctExamples": [
                "GET /api/requirement/review-gate?id=<reqId>",
                "POST /api/requirement/review-materials {\"reqId\":\"<reqId>\"}（mode 可选 full/incremental，缺省按需求状态：发布就绪前全量、发布就绪起增量）",
                "PUT /api/requirement/annotations {\"reqId\":\"<reqId>\",\"annotations\":{\"reviewedCommit\":{\"<repo>\":\"<targetCommit>\"},\"files\":[...]}}"
            ],
            "relatedDocs": [
                "review.md",
                "code-review-ai.md（顶部需含 `Source: agent-panel-code-review skill` 标记）"
            ]
        }));
    }
    None
}
