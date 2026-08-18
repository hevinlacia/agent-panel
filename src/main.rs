use std::{env, net::SocketAddr, path::PathBuf, sync::Arc};

use anyhow::{Context, Result};
use axum::{
    extract::{Query, State},
    http::{StatusCode, Uri},
    response::{Html, IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::{fs, sync::Mutex, task::JoinHandle};
use tower_http::{cors::CorsLayer, services::ServeDir, trace::TraceLayer};

mod attachments;
mod cainiao_mock;
mod capability;
mod config;
mod experience_summary;
mod git_ai;
mod git_workflow;
mod http;
mod knowledge;
mod markdown;
mod pi_config;
mod requirement_api;
mod requirement_context;
mod requirement_index;
mod requirement_service;
mod sessions;
mod util;

use attachments::*;
use cainiao_mock::*;
use capability::*;
use config::*;
use experience_summary::*;
use git_ai::*;
pub(crate) use git_workflow::*;
use http::*;
use knowledge::*;
use markdown::*;
use pi_config::*;
use requirement_api::*;
use requirement_context::*;
use requirement_index::*;
use requirement_service::*;
use sessions::*;
use util::*;

const DEFAULT_PORT: u16 = 7331;
const DEFAULT_PROJECT_NAME: &str = "默认项目";
const DEFAULT_REQ_ID: &str = "__default__";
const STATE_FILE: &str = "state.json";
const ASSOCIATIONS_FILE: &str = "associations.json";
const CONFIG_FILE: &str = "config.json";
const BUSINESS_KNOWLEDGE_DIR: &str = "business-knowledge";
const EXPERIENCES_DIR: &str = "experiences";
const KNOWLEDGE_DEFAULT_STATUS: &str = "active";
const KNOWLEDGE_DEFAULT_CONFIDENCE: &str = "medium";
const INJECTION_CTX_SUBDIR: &str = "ctx";
const BRANCH_SCOPE_FILE: &str = "branches.json";
const CODE_REVIEW_FILE: &str = "code-review.json";
const CODE_REVIEW_INCREMENTAL_FILE: &str = "code-review-incremental.json";
const REQUIREMENT_EVENTS_FILE: &str = "events.jsonl";
const EXPERIENCE_SUMMARY_JOB_FILE: &str = "experience-summary-job.json";
const PHASE_COMMON_PROMPT_FILE: &str = "prompts/phase-common.md";
const DEFAULT_WMS_PROJECT_ROOT: &str = "/home/hevin/Developer/company/WMS";
const DEFAULT_WMS_TESTDATA_PACK_ROOT: &str = "/home/hevin/Developer/company/WMS/.agents/testdata";
const DEFAULT_GITLAB_API_URL: &str = "http://code.jms.com/api/v4";
const DEFAULT_CAINIAO_MOCK_PORT: u16 = 13528;
/// 经验总结状态停留超过该时长后自动推进为已完成（48 小时）。
const EXPERIENCE_SUMMARY_GRACE_MS: i64 = 48 * 60 * 60 * 1000;
/// 自动推进扫描周期（每 10 分钟）。
const EXPERIENCE_AUTO_COMPLETE_INTERVAL_SECS: u64 = 600;
/// 自动经验总结派发扫描周期（每 1 分钟）。
const EXPERIENCE_AUTO_SUMMARY_INTERVAL_SECS: u64 = 60;
/// 自动总结 agent 超过该时长仍未回写则标记为 failed，避免长期占用并发槽位（12 小时）。
const EXPERIENCE_SUMMARY_JOB_STALE_MS: i64 = 12 * 60 * 60 * 1000;
/// 自动推进时写入的备注/事件说明。
const EXPERIENCE_AUTO_COMPLETE_NOTE: &str =
    "自动推进：经验总结状态停留超过 48 小时，自动标记为已完成";
const CAINIAO_MOCK_PRINTERS: &[(&str, &str)] = &[
    ("Mock-A4", "Mock A4 打印机"),
    ("Mock-Label-100", "Mock 标签机 100x100"),
    ("Mock-Label-76", "Mock 标签机 76x130"),
    ("Mock-Receipt", "Mock 小票打印机"),
    ("Mock-Express", "Mock 面单打印机"),
];
const COMMAND_OUTPUT_LIMIT: usize = 80_000;
const DIFF_OUTPUT_LIMIT: usize = 180_000;

static REQ_STATUSES: &[&str] = &[
    "需求澄清",
    "开发中",
    "自测中",
    "测试中",
    "经验总结",
    "已完成",
    // Lightweight statuses for category=线上问题; no strict requirement lifecycle gate.
    "排查中",
    "已确认",
];
static ISSUE_STATUSES: &[&str] = &["排查中", "已确认"];
static REQ_STATUS_ALIASES: &[(&str, &str)] = &[
    ("需求对齐", "需求澄清"),
    ("方案设计", "需求澄清"),
    ("待设计", "需求澄清"),
    ("待开发", "需求澄清"),
    ("待上线", "经验总结"),
    ("线上排查", "排查中"),
    ("问题排查", "排查中"),
    ("问题确认", "已确认"),
];
static REQ_CATEGORIES: &[&str] = &["需求", "线上问题"];

#[derive(Clone)]
struct AppState {
    project_root: Arc<PathBuf>,
    data_dir: Arc<PathBuf>,
    pi_session_root: Arc<PathBuf>,
    /// JoinHandle of the running cainiao print mock server task (None = not running).
    cainiao_mock: Arc<Mutex<Option<JoinHandle<()>>>>,
    /// Serializes auto experience-summary dispatch scans so two triggers do not start duplicate agents.
    experience_summary_dispatch: Arc<Mutex<()>>,
}

#[derive(Debug, Deserialize)]
struct IdQuery {
    id: Option<String>,
    #[serde(alias = "reqId")]
    req_id: Option<String>,
    ids: Option<String>,
    days: Option<i64>,
    file: Option<String>,
    intent: Option<String>,
    budget: Option<usize>,
    tokens: Option<String>,
    #[serde(rename = "for")]
    for_agent: Option<String>,
    target: Option<String>,
    kind: Option<String>,
    q: Option<String>,
    domain: Option<String>,
    project: Option<String>,
    scope: Option<String>,
    status: Option<String>,
    limit: Option<usize>,
    cursor: Option<usize>,
    include_full: Option<bool>,
    section: Option<String>,
    format: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let project_root = env::current_dir().context("resolve project root")?;
    let home = home_dir()?;
    let data_dir = home.join(".local/share/agent-panel");
    let pi_session_root = home.join(".pi/agent/sessions");
    fs::create_dir_all(&data_dir).await.ok();

    let state = AppState {
        project_root: Arc::new(project_root.clone()),
        data_dir: Arc::new(data_dir),
        pi_session_root: Arc::new(pi_session_root),
        cainiao_mock: Arc::new(Mutex::new(None)),
        experience_summary_dispatch: Arc::new(Mutex::new(())),
    };

    // Start the cainiao print mock server on boot if enabled in config.
    sync_cainiao_mock(&state).await;

    // Background: dispatch automatic experience-summary agents, then auto-advance stale summaries as a fallback.
    tokio::spawn(experience_summary_dispatch_loop(state.clone()));
    tokio::spawn(expire_stale_experience_summary_loop(state.clone()));

    let public_dir = project_root.join("public");
    let app = Router::new()
        .route("/health", get(health))
        .route("/api/dashboard/stats", get(api_dashboard_stats))
        .route(
            "/api/requirements",
            get(api_requirements).post(api_requirements_post),
        )
        .route(
            "/api/requirement",
            get(api_requirement).patch(api_requirement_patch),
        )
        .route("/api/requirement/update", post(api_requirement_update))
        .route("/api/requirement/schema", get(api_requirement_schema))
        .route("/api/requirement/context", get(api_requirement_context))
        .route(
            "/api/requirement/experience-summary-context",
            get(api_requirement_experience_summary_context),
        )
        .route(
            "/api/requirement/experience-summary-report",
            get(api_requirement_experience_summary_report),
        )
        .route(
            "/api/experience-summary/jobs",
            get(api_experience_summary_jobs),
        )
        .route(
            "/api/experience-summary/jobs/dispatch",
            post(api_experience_summary_jobs_dispatch),
        )
        .route(
            "/api/experience-summary/jobs/retry",
            post(api_experience_summary_jobs_retry),
        )
        .route(
            "/api/experience-summary/jobs/complete",
            post(api_experience_summary_jobs_complete),
        )
        .route("/api/requirement/edit-plan", get(api_requirement_edit_plan))
        .route("/api/requirement/edit", post(api_requirement_edit))
        .route("/api/requirement/events", post(api_requirement_events))
        .route(
            "/api/requirement/sections/:section",
            post(api_requirement_section).patch(api_requirement_section),
        )
        .route("/api/requirement/notes", post(api_requirement_notes))
        .route(
            "/api/requirement/doc",
            get(api_requirement_doc_get)
                .post(api_requirement_doc)
                .put(api_requirement_doc),
        )
        .route("/api/requirement/validate", post(api_requirement_validate))
        .route("/api/requirement/status", post(api_requirement_status))
        .route("/api/requirement/category", post(api_requirement_category))
        .route(
            "/api/requirement/convert-issue",
            post(api_requirement_convert_issue),
        )
        .route("/api/requirement/ones", post(api_requirement_ones))
        .route(
            "/api/requirement/associate",
            post(api_requirement_associate),
        )
        .route(
            "/api/requirement/dissociate",
            post(api_requirement_dissociate),
        )
        .route(
            "/api/requirement/new-session",
            post(api_requirement_new_session),
        )
        .route(
            "/api/requirement/code-review",
            get(api_requirement_code_review).post(api_requirement_code_review_post),
        )
        .route(
            "/api/requirement/code-review/incremental",
            get(api_requirement_code_review).post(api_requirement_code_review_incremental_post),
        )
        .route(
            "/api/requirement/review-gate",
            get(api_requirement_review_gate),
        )
        .route(
            "/api/requirement/master-diff",
            post(api_requirement_master_diff),
        )
        .route(
            "/api/requirement/sync-base",
            post(api_requirement_sync_base),
        )
        .route(
            "/api/requirement/merge-options",
            get(api_requirement_merge_options),
        )
        .route(
            "/api/requirement/merge-branch",
            post(api_requirement_merge_branch),
        )
        .route(
            "/api/requirement/merge-status",
            get(api_requirement_merge_status),
        )
        .route("/api/requirement/prod-mrs", post(api_requirement_prod_mrs))
        .route(
            "/api/requirement/auto-drive",
            get(api_auto_drive).post(api_auto_drive_post),
        )
        .route("/api/requirement/recommendations", get(api_recommendations))
        .route("/api/requirement/attachments", get(api_attachments))
        .route(
            "/api/requirement/effort-estimate",
            post(api_effort_estimate),
        )
        .route("/api/sessions", get(api_sessions))
        .route("/api/session", get(api_session))
        .route("/api/session/log", get(api_session_log))
        .route("/api/sessions/resolve", get(api_sessions_resolve))
        .route(
            "/api/knowledge",
            get(api_knowledge_list).post(api_knowledge_save),
        )
        .route(
            "/api/knowledge/item",
            get(api_knowledge_item).patch(api_knowledge_save),
        )
        .route(
            "/api/agent/knowledge/query",
            post(api_agent_knowledge_query),
        )
        .route("/api/agent/items", post(api_knowledge_save))
        .route("/api/agent/items/summary", get(api_agent_item_summary))
        .route("/api/agent/items/full", get(api_agent_item_full))
        .route("/api/agent/items/section", get(api_agent_item_full))
        .route("/api/capability/sources", get(api_capability_sources))
        .route("/api/capability/schema", get(api_capability_schema))
        .route("/api/capabilities", get(api_testdata_capabilities))
        .route("/api/capability", get(api_testdata_capability))
        .route("/api/testdata/capabilities", get(api_testdata_capabilities))
        .route("/api/testdata/capability", get(api_testdata_capability))
        .route("/api/testdata/run", post(api_testdata_run))
        .route(
            "/api/agent/capabilities/query",
            get(api_testdata_capabilities),
        )
        .route("/api/config", get(api_config).post(api_config_post))
        .route("/api/cainiao-mock/status", get(api_cainiao_mock_status))
        .route("/api/pi-config", get(api_pi_config))
        .route(
            "/api/pi-config/file",
            get(api_pi_config_file).post(api_pi_config_file_post),
        )
        .route("/api/pi-config/settings", post(api_pi_config_settings))
        .route("/api/notifications", get(api_notifications))
        .route(
            "/api/notifications/unread-count",
            get(api_notifications_unread_count),
        )
        .route("/api/notifications/dismiss", post(ok_json))
        .route("/api/notifications/mark-read", post(ok_json))
        .route(
            "/api/git-ai/suspects",
            get(api_git_ai_suspects).post(ok_json),
        )
        .route(
            "/api/git-ai/suspects/refresh",
            post(api_git_ai_suspects_refresh),
        )
        .route(
            "/api/git-ai/suspects/fix-note",
            post(api_git_ai_suspect_fix_note),
        )
        .route("/api/git-ai/health", get(api_git_ai_health))
        .nest_service(
            "/assets",
            ServeDir::new(public_dir.join("dashboard-react/assets")),
        )
        .nest_service("/static", ServeDir::new(public_dir.clone()))
        .fallback(get(spa_fallback))
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    let port = env::var("PORT")
        .ok()
        .and_then(|v| v.parse::<u16>().ok())
        .unwrap_or(DEFAULT_PORT);
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    tracing::info!(%addr, "Agent Panel running");
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

async fn spa_fallback(State(state): State<AppState>, uri: Uri) -> Response {
    let path = uri.path();
    if path.starts_with("/api/") || path.starts_with("/ws/") {
        return (StatusCode::NOT_FOUND, Json(json!({ "error": "not found" }))).into_response();
    }
    let index = state.project_root.join("public/dashboard-react/index.html");
    match fs::read_to_string(index).await {
        Ok(html) => Html(html).into_response(),
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Html(String::from("<h1>Agent Panel frontend is not built</h1><p>Run <code>bun run build:dashboard</code>.</p>")),
        ).into_response(),
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DashboardStatsQuery {
    project: Option<String>,
}

async fn api_dashboard_stats(
    State(state): State<AppState>,
    Query(query): Query<DashboardStatsQuery>,
) -> ApiResult<Json<Value>> {
    let mut reqs = list_requirements(&state).await?;
    if let Some(project) = query
        .project
        .as_deref()
        .map(str::trim)
        .filter(|p| !p.is_empty())
    {
        reqs.retain(|r| r.projects.iter().any(|p| p == project) || r.project == project);
    }
    let stats = build_dashboard_stats(reqs, now_ms());
    Ok(Json(json!({ "generatedAt": now_ms(), "stats": stats })))
}

#[cfg(test)]
mod tests;
