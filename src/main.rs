use std::{
    collections::{HashMap, HashSet},
    env,
    net::SocketAddr,
    path::{Path, PathBuf},
    process::Stdio,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use anyhow::{anyhow, Context, Result};
use axum::{
    extract::{Query, State},
    http::{HeaderMap, StatusCode, Uri},
    response::{Html, IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use regex::Regex;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::{
    fs,
    process::Command,
    time::{sleep, timeout},
};
use tower_http::{cors::CorsLayer, services::ServeDir, trace::TraceLayer};
use uuid::Uuid;
use walkdir::WalkDir;

const DEFAULT_PORT: u16 = 7331;
const DEFAULT_PROJECT_NAME: &str = "默认项目";
const DEFAULT_REQ_ID: &str = "__default__";
const STATE_FILE: &str = "state.json";
const ASSOCIATIONS_FILE: &str = "associations.json";
const CONFIG_FILE: &str = "config.json";
const INJECTION_CTX_SUBDIR: &str = "ctx";
const BRANCH_SCOPE_FILE: &str = "branches.json";
const CODE_REVIEW_FILE: &str = "code-review.json";
const DEFAULT_GITLAB_API_URL: &str = "http://code.jms.com/api/v4";
const COMMAND_OUTPUT_LIMIT: usize = 80_000;
const DIFF_OUTPUT_LIMIT: usize = 180_000;

static REQ_STATUSES: &[&str] = &[
    "需求对齐",
    "方案设计",
    "开发中",
    "自测中",
    "测试中",
    "待上线",
    "已完成",
];
static REQ_CATEGORIES: &[&str] = &["需求", "线上问题"];

#[derive(Clone)]
struct AppState {
    project_root: Arc<PathBuf>,
    data_dir: Arc<PathBuf>,
    pi_session_root: Arc<PathBuf>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct AppConfig {
    #[serde(default)]
    harness: String,
    #[serde(default)]
    auto_extract: bool,
    #[serde(default)]
    auto_extract_schedule: bool,
    #[serde(default)]
    extract_model: String,
    #[serde(default)]
    min_change_messages: i64,
    #[serde(default)]
    auto_valuation: bool,
    #[serde(default)]
    valuation_threshold: i64,
    #[serde(default = "default_requirement_scan_roots")]
    requirement_scan_roots: Vec<String>,
    #[serde(default)]
    full_sync_schedule: bool,
    #[serde(default)]
    full_sync_times: Vec<String>,
    #[serde(default)]
    full_sync_github_repos: Vec<String>,
    #[serde(default)]
    code_review_pi_model: String,
    #[serde(default)]
    branch_scope_pi_model: String,
    #[serde(default)]
    effort_estimate_pi_model: String,
    #[serde(default = "default_effort_hours")]
    effort_estimate_base_hours: f64,
    #[serde(default)]
    env_vars: Vec<Value>,
}

fn default_requirement_scan_roots() -> Vec<String> {
    Vec::new()
}

fn default_effort_hours() -> f64 {
    4.0
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            harness: "pi".into(),
            auto_extract: false,
            auto_extract_schedule: false,
            extract_model: "litellm-local/deepseek-v4-flash-auto".into(),
            min_change_messages: 5,
            auto_valuation: false,
            valuation_threshold: 25,
            requirement_scan_roots: Vec::new(),
            full_sync_schedule: true,
            full_sync_times: vec![
                "12:00".into(),
                "18:00".into(),
                "20:30".into(),
                "23:30".into(),
            ],
            full_sync_github_repos: Vec::new(),
            code_review_pi_model: String::new(),
            branch_scope_pi_model: String::new(),
            effort_estimate_pi_model: String::new(),
            effort_estimate_base_hours: 4.0,
            env_vars: Vec::new(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
struct AssociationsStore {
    #[serde(default = "associations_version")]
    version: u8,
    #[serde(default)]
    associations: HashMap<String, Vec<String>>,
}

fn associations_version() -> u8 {
    2
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct Requirement {
    id: String,
    title: String,
    status: String,
    projects: Vec<String>,
    project: String,
    group_path: Vec<String>,
    description: String,
    session_ids: Vec<String>,
    category: Option<String>,
    ones: Option<String>,
    created_at: i64,
    updated_at: i64,
    req_dir: Option<String>,
    meta_path: Option<String>,
    background_path: Option<String>,
    branch_path: Option<String>,
    test_path: Option<String>,
    notes_path: Option<String>,
    config_path: Option<String>,
    impact_path: Option<String>,
    memory_path: Option<String>,
    review_path: Option<String>,
    alignment_path: Option<String>,
    prd_path: Option<String>,
    effort_estimate: Option<Value>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct SessionInfo {
    id: String,
    title: String,
    status: String,
    agent: String,
    source: String,
    path: String,
    directory: String,
    worktree: String,
    created: i64,
    updated: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    model_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    model_provider: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    provider: Option<String>,
    tokens_input: u64,
    tokens_output: u64,
    tokens_reasoning: u64,
    tokens_cache_read: u64,
    tokens_cache_write: u64,
    cost: f64,
    message_count: u64,
    user_message_count: u64,
    assistant_message_count: u64,
    tool_result_count: u64,
    tool_call_count: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    thinking_level: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct StatusCount {
    status: String,
    count: usize,
    percent: f64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct RequirementDuration {
    req: Requirement,
    duration_ms: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DashboardStats {
    total: usize,
    status_counts: Vec<StatusCount>,
    durations: Vec<RequirementDuration>,
    avg_delivery_ms: i64,
    median_delivery_ms: i64,
    max_delivery_ms: i64,
    completed_count: usize,
    in_progress_count: usize,
}

#[derive(Debug, Deserialize)]
struct IdQuery {
    id: Option<String>,
    req_id: Option<String>,
    days: Option<i64>,
    file: Option<String>,
    intent: Option<String>,
    budget: Option<usize>,
    tokens: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StatusForm {
    req_id: String,
    status: String,
    note: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CategoryForm {
    req_id: String,
    category: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct OnesForm {
    req_id: String,
    ones: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AssociateForm {
    req_id: String,
    session_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct NewSessionForm {
    req_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RequirementCreateForm {
    req_id: String,
    title: String,
    #[serde(default)]
    project: Option<String>,
    #[serde(default)]
    projects: Option<Vec<String>>,
    #[serde(default)]
    group_path: Option<Vec<String>>,
    #[serde(default)]
    parent_req_id: Option<String>,
    #[serde(default)]
    root: Option<String>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    category: Option<String>,
    #[serde(default)]
    owner: Option<String>,
    #[serde(default)]
    start_date: Option<String>,
    #[serde(default)]
    plan_release: Option<String>,
    #[serde(default)]
    ones: Option<String>,
    #[serde(default)]
    summary: Option<String>,
    #[serde(default)]
    background: Option<String>,
    #[serde(default)]
    notes: Option<String>,
    #[serde(default)]
    dry_run: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RequirementPatchForm {
    req_id: String,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    project: Option<String>,
    #[serde(default)]
    projects: Option<Vec<String>>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    category: Option<String>,
    #[serde(default)]
    owner: Option<String>,
    #[serde(default)]
    start_date: Option<String>,
    #[serde(default)]
    plan_release: Option<String>,
    #[serde(default)]
    ones: Option<String>,
    #[serde(default)]
    note: Option<String>,
    #[serde(default)]
    dry_run: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RequirementNoteForm {
    req_id: String,
    text: String,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    session_id: Option<String>,
    #[serde(default)]
    dry_run: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RequirementDocForm {
    req_id: String,
    doc_type: String,
    content: String,
    #[serde(default)]
    mode: Option<String>,
    #[serde(default)]
    dry_run: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RequirementValidateForm {
    req_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RequirementEditForm {
    req_id: String,
    operation: String,
    #[serde(default)]
    token: Option<String>,
    #[serde(default)]
    doc_type: Option<String>,
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    heading: Option<String>,
    #[serde(default)]
    mode: Option<String>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    category: Option<String>,
    #[serde(default)]
    note: Option<String>,
    #[serde(default)]
    session_id: Option<String>,
    #[serde(default)]
    fields: Option<HashMap<String, String>>,
    #[serde(default)]
    dry_run: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CodeReviewForm {
    req_id: String,
    #[serde(default)]
    base_ref: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProdMrForm {
    req_id: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
struct BranchScope {
    #[serde(default)]
    version: i64,
    #[serde(default)]
    updated_at: i64,
    #[serde(default)]
    repos: Vec<BranchRepo>,
    #[serde(default)]
    fallback: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
struct BranchRepo {
    #[serde(default)]
    repo_name: String,
    #[serde(default)]
    branches: Vec<String>,
    #[serde(default)]
    role: Option<String>,
    #[serde(default, alias = "projectPath")]
    path: Option<String>,
    #[serde(default)]
    base_ref: Option<String>,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct CodeReviewFileStat {
    path: String,
    status: String,
    additions: i64,
    deletions: i64,
    risk_tags: Vec<String>,
}

#[derive(Debug)]
struct GitCommandResult {
    ok: bool,
    code: Option<i32>,
    command: String,
    stdout: String,
    stderr: String,
    output_truncated: bool,
    timed_out: bool,
}

#[derive(Debug)]
struct BaseRefInfo {
    base_ref: String,
    remote: String,
    remote_branch: String,
    local_branch: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConfigPatch {
    harness: Option<String>,
    auto_extract: Option<bool>,
    auto_extract_schedule: Option<bool>,
    extract_model: Option<String>,
    min_change_messages: Option<i64>,
    auto_valuation: Option<bool>,
    valuation_threshold: Option<i64>,
    requirement_scan_roots: Option<Vec<String>>,
    full_sync_schedule: Option<bool>,
    full_sync_times: Option<Vec<String>>,
    full_sync_github_repos: Option<Vec<String>>,
    code_review_pi_model: Option<String>,
    branch_scope_pi_model: Option<String>,
    effort_estimate_pi_model: Option<String>,
    effort_estimate_base_hours: Option<f64>,
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
    };

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
        .route("/api/requirement/edit-plan", get(api_requirement_edit_plan))
        .route("/api/requirement/edit", post(api_requirement_edit))
        .route("/api/requirement/notes", post(api_requirement_notes))
        .route(
            "/api/requirement/doc",
            post(api_requirement_doc).put(api_requirement_doc),
        )
        .route("/api/requirement/validate", post(api_requirement_validate))
        .route("/api/requirement/status", post(api_requirement_status))
        .route("/api/requirement/category", post(api_requirement_category))
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
            "/api/requirement/master-diff",
            post(api_requirement_master_diff),
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
        .route("/api/config", get(api_config).post(api_config_post))
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

async fn health() -> Json<Value> {
    Json(json!({ "ok": true, "ts": now_ms() }))
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

async fn api_dashboard_stats(State(state): State<AppState>) -> ApiResult<Json<Value>> {
    let reqs = list_requirements(&state).await?;
    let stats = build_dashboard_stats(reqs, now_ms());
    Ok(Json(json!({ "generatedAt": now_ms(), "stats": stats })))
}

async fn api_requirements(State(state): State<AppState>) -> ApiResult<Json<Value>> {
    let requirements = list_requirements(&state).await?;
    Ok(Json(json!({ "requirements": requirements })))
}

async fn api_requirement(
    State(state): State<AppState>,
    Query(query): Query<IdQuery>,
) -> ApiResult<Json<Value>> {
    let id = query.id.or(query.req_id).unwrap_or_default();
    let req = get_requirement(&state, &id).await?;
    Ok(Json(json!({ "requirement": req })))
}

async fn api_requirements_post(
    State(state): State<AppState>,
    form: FormOrJson<RequirementCreateForm>,
) -> ApiResult<Json<Value>> {
    let created = create_requirement(&state, form.0).await?;
    Ok(Json(created))
}

async fn api_requirement_patch(
    State(state): State<AppState>,
    form: FormOrJson<RequirementPatchForm>,
) -> ApiResult<Json<Value>> {
    let updated = update_requirement(&state, form.0).await?;
    Ok(Json(updated))
}

async fn api_requirement_update(
    State(state): State<AppState>,
    form: FormOrJson<RequirementPatchForm>,
) -> ApiResult<Json<Value>> {
    let updated = update_requirement(&state, form.0).await?;
    Ok(Json(updated))
}

async fn api_requirement_notes(
    State(state): State<AppState>,
    form: FormOrJson<RequirementNoteForm>,
) -> ApiResult<Json<Value>> {
    let value = append_requirement_note(&state, form.0).await?;
    Ok(Json(value))
}

async fn api_requirement_doc(
    State(state): State<AppState>,
    form: FormOrJson<RequirementDocForm>,
) -> ApiResult<Json<Value>> {
    let value = write_requirement_doc(&state, form.0).await?;
    Ok(Json(value))
}

async fn api_requirement_validate(
    State(state): State<AppState>,
    form: FormOrJson<RequirementValidateForm>,
) -> ApiResult<Json<Value>> {
    let req = get_real_requirement(&state, &form.0.req_id).await?;
    let value = validate_requirement(&state, &req).await?;
    Ok(Json(value))
}

async fn api_requirement_schema() -> Json<Value> {
    Json(requirement_api_schema())
}

async fn api_requirement_edit_plan(
    State(state): State<AppState>,
    Query(query): Query<IdQuery>,
) -> ApiResult<Json<Value>> {
    let req_id = query.id.or(query.req_id).unwrap_or_default();
    let req = get_real_requirement(&state, &req_id).await?;
    let intent = normalize_requirement_intent(query.intent.as_deref());
    Ok(Json(build_requirement_edit_plan(&req, &intent)))
}

async fn api_requirement_context(
    State(state): State<AppState>,
    Query(query): Query<IdQuery>,
) -> ApiResult<Json<Value>> {
    let req_id = query.id.or(query.req_id).unwrap_or_default();
    let req = get_real_requirement(&state, &req_id).await?;
    let intent = normalize_requirement_intent(query.intent.as_deref());
    let tokens = query
        .tokens
        .as_deref()
        .map(parse_token_list)
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| intent_read_tokens(&intent));
    let budget = query.budget.unwrap_or(2_000).clamp(400, 12_000);
    let value = build_requirement_context(&req, &intent, tokens, budget).await?;
    Ok(Json(value))
}

async fn api_requirement_edit(
    State(state): State<AppState>,
    form: FormOrJson<RequirementEditForm>,
) -> ApiResult<Json<Value>> {
    let value = apply_requirement_edit(&state, form.0).await?;
    Ok(Json(value))
}

async fn api_requirement_status(
    State(state): State<AppState>,
    form: FormOrJson<StatusForm>,
) -> ApiResult<Json<Value>> {
    let body = form.0;
    ensure_status(&body.status)?;
    let req = get_real_requirement(&state, &body.req_id).await?;
    let st = write_requirement_status(
        req.req_dir.as_deref().unwrap_or_default(),
        &body.status,
        body.note.as_deref(),
    )
    .await?;
    Ok(Json(json!({ "ok": true, "state": st })))
}

async fn api_requirement_category(
    State(state): State<AppState>,
    form: FormOrJson<CategoryForm>,
) -> ApiResult<Json<Value>> {
    let body = form.0;
    ensure_category(&body.category)?;
    let req = get_real_requirement(&state, &body.req_id).await?;
    let st = write_requirement_category(req.req_dir.as_deref().unwrap_or_default(), &body.category)
        .await?;
    Ok(Json(json!({ "ok": true, "state": st })))
}

async fn api_requirement_ones(
    State(state): State<AppState>,
    form: FormOrJson<OnesForm>,
) -> ApiResult<Json<Value>> {
    let body = form.0;
    let req = get_real_requirement(&state, &body.req_id).await?;
    let ones = body.ones.unwrap_or_default();
    let stored = write_requirement_ones(req.req_dir.as_deref().unwrap_or_default(), &ones).await?;
    Ok(Json(
        json!({ "ok": true, "ones": stored, "ref": parse_ones_ref(&stored) }),
    ))
}

async fn api_requirement_associate(
    State(state): State<AppState>,
    form: FormOrJson<AssociateForm>,
) -> ApiResult<Json<Value>> {
    let body = form.0;
    associate_session(&state, &body.req_id, &body.session_id).await?;
    Ok(Json(json!({ "ok": true })))
}

async fn api_requirement_dissociate(
    State(state): State<AppState>,
    form: FormOrJson<AssociateForm>,
) -> ApiResult<Json<Value>> {
    let body = form.0;
    dissociate_session(&state, &body.req_id, &body.session_id).await?;
    Ok(Json(json!({ "ok": true })))
}

async fn api_requirement_new_session(
    State(state): State<AppState>,
    form: FormOrJson<NewSessionForm>,
) -> ApiResult<Json<Value>> {
    let body = form.0;
    let req = get_real_requirement(&state, &body.req_id).await?;
    let session_id = Uuid::new_v4().to_string();
    associate_session(&state, &body.req_id, &session_id).await?;
    let ctx_path = write_injection_context(&state, &req, &session_id).await?;
    let title = shell_quote(&req.title);
    let ctx = shell_quote(ctx_path.to_string_lossy().as_ref());
    let pi_command = format!(
        "pi --session-id {} --name {} --append-system-prompt @{}",
        session_id, title, ctx
    );
    let project_root = requirement_project_root(&req).map(|p| p.to_string_lossy().to_string());
    let command = if let Some(root) = &project_root {
        format!("cd {} && {}", shell_quote(root), pi_command)
    } else {
        pi_command
    };
    Ok(Json(
        json!({ "ok": true, "sessionId": session_id, "command": command, "contextPath": ctx_path, "cwd": project_root }),
    ))
}

async fn api_requirement_code_review(
    State(state): State<AppState>,
    Query(query): Query<IdQuery>,
) -> ApiResult<Json<Value>> {
    let id = query.id.or(query.req_id).unwrap_or_default();
    let req = get_real_requirement(&state, &id).await?;
    let req_dir = PathBuf::from(req.req_dir.unwrap_or_default());
    let branch_scope = read_branch_scope(&req_dir).await?;
    let review = read_json_if_exists(&req_dir.join(CODE_REVIEW_FILE)).await;
    Ok(Json(
        json!({ "ok": true, "branchScope": branch_scope, "review": review }),
    ))
}

async fn api_requirement_code_review_post(
    State(state): State<AppState>,
    form: FormOrJson<CodeReviewForm>,
) -> ApiResult<Json<Value>> {
    let req = get_real_requirement(&state, &form.0.req_id).await?;
    let req_dir = PathBuf::from(req.req_dir.unwrap_or_default());
    let branch_scope = read_branch_scope(&req_dir).await?.ok_or_else(|| {
        ApiError::bad_request(format!(
            "missing {BRANCH_SCOPE_FILE}; run req-branches-update first"
        ))
    })?;
    let review = run_code_review_scan(&req_dir, &req.id, &branch_scope).await?;
    Ok(Json(
        json!({ "ok": true, "branchScope": branch_scope, "review": review }),
    ))
}

async fn api_requirement_master_diff(
    State(state): State<AppState>,
    form: FormOrJson<CodeReviewForm>,
) -> ApiResult<Json<Value>> {
    let body = form.0;
    let req = get_real_requirement(&state, &body.req_id).await?;
    let req_dir = PathBuf::from(req.req_dir.unwrap_or_default());
    let branch_scope = read_branch_scope(&req_dir).await?.ok_or_else(|| {
        ApiError::bad_request(format!(
            "missing {BRANCH_SCOPE_FILE}; run req-branches-update first"
        ))
    })?;
    let base_ref = body
        .base_ref
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .unwrap_or("origin/master");
    let review = run_master_diff_scan(&req.id, &branch_scope, base_ref).await?;
    Ok(Json(
        json!({ "ok": true, "branchScope": branch_scope, "review": review }),
    ))
}

async fn api_requirement_prod_mrs(
    State(state): State<AppState>,
    form: FormOrJson<ProdMrForm>,
) -> ApiResult<Json<Value>> {
    let req = get_real_requirement(&state, &form.0.req_id).await?;
    let req_dir = PathBuf::from(req.req_dir.as_deref().unwrap_or_default());
    let branch_scope = read_branch_scope(&req_dir).await?.ok_or_else(|| {
        ApiError::bad_request(format!(
            "missing {BRANCH_SCOPE_FILE}; run req-branches-update first"
        ))
    })?;
    let results = generate_prod_mrs(&req, &branch_scope).await?;
    Ok(Json(json!({
        "ok": true,
        "reqId": req.id,
        "generatedAt": now_ms(),
        "branchScope": branch_scope,
        "results": results,
    })))
}

async fn api_auto_drive() -> Json<Value> {
    Json(
        json!({ "jobs": [], "active": 0, "blocked": 0, "queue": { "active": 0, "queued": 0 }, "message": "auto-drive was removed with the legacy Node backend" }),
    )
}

async fn api_auto_drive_post() -> Json<Value> {
    Json(
        json!({ "jobs": [], "errors": [], "message": "auto-drive is not available in the Rust rewrite yet" }),
    )
}

async fn api_recommendations(
    State(state): State<AppState>,
    Query(query): Query<IdQuery>,
) -> ApiResult<Json<Value>> {
    let req_id = query.id.or(query.req_id).unwrap_or_default();
    let req = get_requirement(&state, &req_id).await?;
    let existing: HashSet<String> = req
        .as_ref()
        .map(|r| r.session_ids.iter().cloned().collect())
        .unwrap_or_default();
    let sessions = scan_pi_sessions(&state, query.days).await?;
    let recommendations: Vec<Value> = sessions
        .into_iter()
        .filter(|s| !existing.contains(&s.id))
        .take(12)
        .map(|session| json!({ "session": session, "score": 25, "reasons": ["recent pi session"] }))
        .collect();
    Ok(Json(json!({ "recommendations": recommendations })))
}

async fn api_attachments(
    State(state): State<AppState>,
    Query(query): Query<IdQuery>,
) -> ApiResult<Json<Value>> {
    let id = query.id.or(query.req_id).unwrap_or_default();
    let req = get_real_requirement(&state, &id).await?;
    let dir = PathBuf::from(req.req_dir.unwrap_or_default()).join("attachments");
    let mut rows = Vec::new();
    if let Ok(mut rd) = fs::read_dir(&dir).await {
        while let Ok(Some(entry)) = rd.next_entry().await {
            if let Ok(meta) = entry.metadata().await {
                if meta.is_file() {
                    rows.push(json!({
                        "filename": entry.file_name().to_string_lossy(),
                        "size": meta.len(),
                        "mtime": system_time_to_ms(meta.modified().unwrap_or(UNIX_EPOCH)),
                    }));
                }
            }
        }
    }
    Ok(Json(json!({ "attachments": rows })))
}

async fn api_effort_estimate(
    State(state): State<AppState>,
    form: FormOrJson<NewSessionForm>,
) -> ApiResult<Json<Value>> {
    let req = get_real_requirement(&state, &form.0.req_id).await?;
    let estimate = json!({
        "version": 1,
        "coefficient": 1.0,
        "baseHours": 4,
        "estimatedHours": 4,
        "factors": [],
        "summary": "Rust rewrite placeholder: AI effort estimation has not been reimplemented yet.",
        "model": "manual-placeholder",
        "updatedAt": now_ms()
    });
    if let Some(dir) = req.req_dir {
        let path = PathBuf::from(dir).join("effort-estimate.json");
        atomic_write_json(&path, &estimate).await?;
    }
    Ok(Json(json!({ "ok": true, "estimate": estimate })))
}

async fn api_sessions(
    State(state): State<AppState>,
    Query(query): Query<IdQuery>,
) -> ApiResult<Json<Value>> {
    let sessions = scan_pi_sessions(&state, query.days).await?;
    let mut summary: HashMap<String, usize> = HashMap::new();
    for s in &sessions {
        *summary.entry(s.status.clone()).or_default() += 1;
    }
    Ok(Json(
        json!({ "summary": summary, "sessions": sessions, "harness": "pi", "days": query.days.unwrap_or(7) }),
    ))
}

async fn api_session(
    State(state): State<AppState>,
    Query(query): Query<IdQuery>,
) -> ApiResult<Json<Value>> {
    let id = query.id.unwrap_or_default();
    let session = scan_pi_sessions(&state, None)
        .await?
        .into_iter()
        .find(|s| s.id == id);
    Ok(Json(json!({ "session": session, "terminalRemoved": true })))
}

async fn api_config(State(state): State<AppState>) -> ApiResult<Json<AppConfig>> {
    Ok(Json(read_config(&state).await?))
}

async fn api_config_post(
    State(state): State<AppState>,
    Json(patch): Json<ConfigPatch>,
) -> ApiResult<Json<AppConfig>> {
    let mut cfg = read_config(&state).await?;
    if let Some(v) = patch.harness {
        cfg.harness = v;
    }
    if let Some(v) = patch.auto_extract {
        cfg.auto_extract = v;
    }
    if let Some(v) = patch.auto_extract_schedule {
        cfg.auto_extract_schedule = v;
    }
    if let Some(v) = patch.extract_model {
        cfg.extract_model = v;
    }
    if let Some(v) = patch.min_change_messages {
        cfg.min_change_messages = v;
    }
    if let Some(v) = patch.auto_valuation {
        cfg.auto_valuation = v;
    }
    if let Some(v) = patch.valuation_threshold {
        cfg.valuation_threshold = v;
    }
    if let Some(v) = patch.requirement_scan_roots {
        cfg.requirement_scan_roots = normalize_scan_roots(v);
    }
    if let Some(v) = patch.full_sync_schedule {
        cfg.full_sync_schedule = v;
    }
    if let Some(v) = patch.full_sync_times {
        cfg.full_sync_times = v;
    }
    if let Some(v) = patch.full_sync_github_repos {
        cfg.full_sync_github_repos = v;
    }
    if let Some(v) = patch.code_review_pi_model {
        cfg.code_review_pi_model = v;
    }
    if let Some(v) = patch.branch_scope_pi_model {
        cfg.branch_scope_pi_model = v;
    }
    if let Some(v) = patch.effort_estimate_pi_model {
        cfg.effort_estimate_pi_model = v;
    }
    if let Some(v) = patch.effort_estimate_base_hours {
        cfg.effort_estimate_base_hours = v.max(0.1);
    }
    write_config(&state, &cfg).await?;
    Ok(Json(cfg))
}

async fn api_pi_config() -> Json<Value> {
    let home = home_dir().unwrap_or_else(|_| PathBuf::from("~"));
    let pi_dir = home.join(".pi/agent");
    let settings_path = pi_dir.join("settings.json");
    let models_path = pi_dir.join("models.json");
    let agents_path = pi_dir.join("agents.json");
    let settings = read_json_if_exists(&settings_path)
        .await
        .unwrap_or_else(|| json!({}));
    let models = read_json_if_exists(&models_path)
        .await
        .unwrap_or_else(|| json!({}));
    let providers_obj = models.get("providers").and_then(Value::as_object);
    let mut providers: Vec<Value> = Vec::new();
    if let Some(providers_map) = providers_obj {
        for (provider_id, provider) in providers_map {
            let provider_models = provider
                .get("models")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            let mut model_rows: Vec<Value> = Vec::new();
            for model in &provider_models {
                let model_id = model
                    .get("id")
                    .or_else(|| model.get("modelId"))
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                if model_id.is_empty() {
                    continue;
                }
                let thinking_levels = model
                    .get("thinkingLevelMap")
                    .and_then(Value::as_object)
                    .map(|m| m.keys().cloned().collect::<Vec<_>>())
                    .unwrap_or_default();
                model_rows.push(json!({
                    "providerId": provider_id,
                    "modelId": model_id,
                    "label": model.get("name").and_then(Value::as_str).unwrap_or(model_id),
                    "name": model.get("name").and_then(Value::as_str).unwrap_or(model_id),
                    "contextWindow": model.get("contextWindow").and_then(Value::as_i64),
                    "reasoning": model.get("reasoning").and_then(Value::as_bool).unwrap_or(false),
                    "thinkingLevels": thinking_levels,
                }));
            }
            providers.push(json!({
                "id": provider_id,
                "api": provider.get("api").and_then(Value::as_str),
                "baseUrl": provider.get("baseUrl").and_then(Value::as_str),
                "modelCount": model_rows.len(),
                "hasApiKey": provider.get("apiKey").and_then(Value::as_str).map(|s| !s.is_empty()).unwrap_or(false),
                "models": model_rows,
            }));
        }
    }
    providers.sort_by_key(|v| {
        v.get("id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string()
    });
    Json(json!({
        "settings": {
            "path": settings_path,
            "exists": settings_path.exists(),
            "defaultProvider": settings.get("defaultProvider").and_then(Value::as_str).unwrap_or(""),
            "defaultModel": settings.get("defaultModel").and_then(Value::as_str).unwrap_or(""),
            "defaultThinkingLevel": settings.get("defaultThinkingLevel").and_then(Value::as_str).unwrap_or("off"),
            "enabledModels": settings.get("enabledModels").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).collect::<Vec<_>>()).unwrap_or_default(),
            "theme": settings.get("theme").and_then(Value::as_str).unwrap_or(""),
        },
        "providers": providers,
        "files": [
            { "file": "settings", "label": "settings.json", "path": settings_path, "sensitive": false, "description": "Pi settings file" },
            { "file": "models", "label": "models.json", "path": models_path, "sensitive": true, "description": "Pi provider/model definitions; API keys are not exposed in the summary" },
            { "file": "agents", "label": "agents.json", "path": agents_path, "sensitive": false, "description": "Pi agent definitions" }
        ],
        "thinkingLevels": ["off", "minimal", "low", "medium", "high", "xhigh", "max"]
    }))
}

fn pi_file_path(file: &str) -> Result<(PathBuf, &'static str, bool, &'static str)> {
    let dir = home_dir()?.join(".pi/agent");
    match file {
        "settings" => Ok((
            dir.join("settings.json"),
            "settings.json",
            false,
            "Pi settings file",
        )),
        "agents" => Ok((
            dir.join("agents.json"),
            "agents.json",
            false,
            "Pi agent definitions",
        )),
        "models" => Ok((
            dir.join("models.json"),
            "models.json",
            true,
            "Pi provider/model definitions",
        )),
        _ => Err(anyhow!("unsupported pi config file: {file}")),
    }
}

async fn api_pi_config_file(Query(query): Query<IdQuery>) -> Json<Value> {
    let file = query.file.unwrap_or_else(|| "settings".into());
    let (path, label, sensitive, description) =
        pi_file_path(&file).unwrap_or_else(|_| (PathBuf::new(), "unknown", false, "unknown"));
    let content = if file == "models" {
        "// models.json contains API keys; edit it directly on disk if needed.
"
        .to_string()
    } else {
        fs::read_to_string(&path).await.unwrap_or_default()
    };
    Json(
        json!({ "file": file, "label": label, "path": path, "sensitive": sensitive, "description": description, "content": content, "updatedAt": now_ms() }),
    )
}

async fn api_pi_config_file_post(Json(payload): Json<Value>) -> ApiResult<Json<Value>> {
    let file = payload
        .get("file")
        .and_then(Value::as_str)
        .unwrap_or("settings");
    if file == "models" {
        return Err(ApiError::bad_request(
            "models.json may contain API keys; edit it directly instead of through the browser",
        ));
    }
    let (path, label, sensitive, description) = pi_file_path(file)?;
    let content = payload
        .get("content")
        .and_then(Value::as_str)
        .unwrap_or_default();
    atomic_write_text(&path, content).await?;
    Ok(Json(
        json!({ "file": file, "label": label, "path": path, "sensitive": sensitive, "description": description, "content": content, "updatedAt": now_ms() }),
    ))
}

async fn api_pi_config_settings(Json(payload): Json<Value>) -> ApiResult<Json<Value>> {
    let path = home_dir()?.join(".pi/agent/settings.json");
    let mut settings = read_json_if_exists(&path)
        .await
        .unwrap_or_else(|| json!({}));
    let Some(obj) = settings.as_object_mut() else {
        return Err(ApiError::bad_request("settings.json is not an object"));
    };
    for key in [
        "defaultProvider",
        "defaultModel",
        "defaultThinkingLevel",
        "theme",
    ] {
        if let Some(value) = payload.get(key).and_then(Value::as_str) {
            obj.insert(key.to_string(), json!(value));
        }
    }
    if let Some(enabled) = payload.get("enabledModels").and_then(Value::as_array) {
        obj.insert(
            "enabledModels".into(),
            Value::Array(
                enabled
                    .iter()
                    .filter_map(Value::as_str)
                    .map(|s| json!(s))
                    .collect(),
            ),
        );
    }
    atomic_write_json(&path, &settings).await?;
    Ok(Json(json!({ "ok": true, "settings": settings })))
}

async fn api_notifications() -> Json<Value> {
    Json(json!({ "notifications": [] }))
}

async fn api_notifications_unread_count() -> Json<Value> {
    Json(json!({ "count": 0 }))
}

async fn api_git_ai_health(State(state): State<AppState>) -> Json<Value> {
    let home = home_dir().unwrap_or_default();
    let store_path = state.data_dir.join("git-ai-suspects.json");
    let cli = read_git_ai_cli_health(&home).await;
    let pi_extension = read_pi_git_ai_extension_health(&home).await;
    Json(json!({
        "generatedAt": now_ms(),
        "storePath": store_path,
        "cli": cli,
        "piExtension": pi_extension,
    }))
}

async fn run_output(cmd: &str, args: &[&str], timeout_ms: u64) -> (Option<i32>, String, String) {
    let fut = Command::new(cmd).args(args).output();
    match timeout(Duration::from_millis(timeout_ms), fut).await {
        Ok(Ok(output)) => (
            output.status.code(),
            String::from_utf8_lossy(&output.stdout).to_string(),
            String::from_utf8_lossy(&output.stderr).to_string(),
        ),
        Ok(Err(err)) => (None, String::new(), err.to_string()),
        Err(_) => (None, String::new(), "timeout".into()),
    }
}

async fn find_git_ai_binary(home: &Path) -> Option<PathBuf> {
    if let Ok(path) = env::var("GIT_AI_BIN") {
        let p = PathBuf::from(path);
        if p.exists() {
            return Some(p);
        }
    }
    let (code, stdout, _) = run_output("bash", &["-lc", "command -v git-ai"], 2_000).await;
    let found = stdout.trim();
    if code == Some(0) && !found.is_empty() {
        let p = PathBuf::from(found);
        if p.exists() {
            return Some(p);
        }
    }
    let default = home.join(".git-ai/bin/git-ai");
    if default.exists() {
        Some(default)
    } else {
        None
    }
}

fn parse_trace2_socket(target: Option<&str>) -> Option<String> {
    let target = target?.trim();
    if target.is_empty() {
        return None;
    }
    let marker = "af_unix:stream:";
    if let Some(idx) = target.find(marker) {
        let socket = target[idx + marker.len()..].trim();
        if socket.is_empty() {
            None
        } else {
            Some(socket.to_string())
        }
    } else {
        Some(target.to_string())
    }
}

async fn read_text_safe(path: &Path) -> String {
    fs::read_to_string(path).await.unwrap_or_default()
}

async fn is_executable(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = fs::metadata(path).await {
            return meta.permissions().mode() & 0o111 != 0;
        }
    }
    #[cfg(not(unix))]
    {
        return path.exists();
    }
    false
}

async fn hook_health(path: Option<PathBuf>, kind: &str) -> Value {
    let Some(path) = path else {
        return json!({ "path": null, "exists": false, "mode": "missing", "recordsToAgentPanel": false, "executable": false });
    };
    let text = read_text_safe(&path).await;
    let exists = path.exists();
    let records = text.contains("record_git_ai_suspect") && text.contains("AGENT_PANEL_STORE");
    let mut mode = if exists && records {
        "record"
    } else if exists {
        "present"
    } else {
        "missing"
    };
    if kind == "pre-push"
        && text.contains("GIT_AI_PUSH_MODE")
        && text.contains("block")
        && !text.contains("record")
    {
        mode = "block";
    }
    if kind == "post-commit" && text.contains("NO_BLOCK") && !text.contains("GIT_AI_BLOCK") {
        mode = "block";
    }
    json!({
        "path": path,
        "exists": exists,
        "mode": mode,
        "recordsToAgentPanel": records,
        "executable": is_executable(&path).await,
    })
}

async fn read_git_ai_cli_health(home: &Path) -> Value {
    let binary = find_git_ai_binary(home).await;
    let (installed, version, daemon_ok, daemon_message) = if let Some(bin) = &binary {
        let (_, version_out, version_err) =
            run_output(bin.to_string_lossy().as_ref(), &["--version"], 3_000).await;
        let version = version_out.trim().to_string();
        let version = if version.is_empty() {
            version_err.trim().to_string()
        } else {
            version
        };
        let (_, bg_out, bg_err) =
            run_output(bin.to_string_lossy().as_ref(), &["bg", "status"], 4_000).await;
        let mut ok = false;
        let mut message = if bg_err.trim().is_empty() {
            bg_out.trim().to_string()
        } else {
            bg_err.trim().to_string()
        };
        if let Ok(parsed) = serde_json::from_str::<Value>(&bg_out) {
            ok = parsed.get("ok").and_then(Value::as_bool).unwrap_or(false)
                && parsed
                    .pointer("/data/last_error")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .is_empty();
            message = parsed
                .pointer("/data/last_error")
                .and_then(Value::as_str)
                .map(str::to_string)
                .unwrap_or_else(|| {
                    if ok {
                        "running".into()
                    } else {
                        "not running".into()
                    }
                });
        }
        (
            true,
            if version.is_empty() {
                None
            } else {
                Some(version)
            },
            ok,
            Some(message),
        )
    } else {
        (false, None, false, Some("git-ai binary missing".into()))
    };
    let (trace_code, trace_out, _) =
        run_output("git", &["config", "--global", "trace2.eventtarget"], 2_000).await;
    let trace2_target = if trace_code == Some(0) {
        Some(trace_out.trim().to_string()).filter(|s| !s.is_empty())
    } else {
        None
    };
    let trace2_socket = parse_trace2_socket(trace2_target.as_deref());
    let trace2_socket_exists = trace2_socket
        .as_ref()
        .map(|s| Path::new(s).exists())
        .unwrap_or(false);
    let (hooks_code, hooks_out, _) =
        run_output("git", &["config", "--global", "core.hooksPath"], 2_000).await;
    let hooks_path = if hooks_code == Some(0) {
        Some(hooks_out.trim().to_string()).filter(|s| !s.is_empty())
    } else {
        None
    };
    let hooks_dir = hooks_path.as_ref().map(PathBuf::from);
    let post_hook = hook_health(
        hooks_dir.as_ref().map(|p| p.join("post-commit")),
        "post-commit",
    )
    .await;
    let pre_hook = hook_health(hooks_dir.as_ref().map(|p| p.join("pre-push")), "pre-push").await;
    json!({
        "binaryPath": binary,
        "installed": installed,
        "version": version,
        "daemonOk": daemon_ok,
        "daemonMessage": daemon_message,
        "trace2Target": trace2_target,
        "trace2Socket": trace2_socket,
        "trace2SocketExists": trace2_socket_exists,
        "hooksPath": hooks_path,
        "postCommitHook": post_hook,
        "prePushHook": pre_hook,
    })
}

fn tracked_tools(text: &str) -> Vec<&'static str> {
    let mut out = Vec::new();
    if text.contains("edit") {
        out.push("edit");
    }
    if text.contains("write") {
        out.push("write");
    }
    if text.contains("tool === \"bash\"") || text.contains("bash") {
        out.push("bash");
    }
    out.sort_unstable();
    out.dedup();
    out
}

async fn read_pi_git_ai_extension_health(home: &Path) -> Value {
    let global = home.join(".pi/agent/extensions/git-ai.ts");
    let source = home.join("Developer/infra/ai-code-config/core/pi/agent/extensions/git-ai.ts");
    let text = read_text_safe(&global).await;
    let global_exists = global.exists();
    let source_exists = source.exists();
    let source_matches = if global_exists && source_exists {
        text == read_text_safe(&source).await
    } else {
        false
    };
    let bin_match = Regex::new(r#"const GIT_AI_BIN = process\.env\.GIT_AI_BIN \|\| \"([^\"]+)\""#)
        .ok()
        .and_then(|re| {
            re.captures(&text)
                .and_then(|c| c.get(1).map(|m| m.as_str().to_string()))
        });
    let binary_path = env::var("GIT_AI_BIN")
        .ok()
        .or(bin_match)
        .unwrap_or_else(|| {
            home.join(".git-ai/bin/git-ai")
                .to_string_lossy()
                .to_string()
        });
    let binary_exists = Path::new(&binary_path).exists();
    let registers_status =
        text.contains("ctx.ui.setStatus(\"git-ai\"") || text.contains("ctx.ui.setStatus('git-ai'");
    let tools = tracked_tools(&text);
    let mut problems = Vec::new();
    if !global_exists {
        problems.push("global extension missing");
    }
    if !binary_exists {
        problems.push("git-ai binary missing for extension");
    }
    if !registers_status {
        problems.push("no git-ai UI status registration");
    }
    if tools.is_empty() {
        problems.push("no tracked tools detected");
    }
    if !source_matches {
        problems.push("runtime extension differs from config source");
    }
    let status = if problems.is_empty() {
        "ok"
    } else if problems.iter().any(|p| p.contains("missing")) {
        "error"
    } else {
        "warn"
    };
    json!({
        "globalPath": global,
        "sourcePath": source,
        "globalExists": global_exists,
        "sourceExists": source_exists,
        "sourceMatchesGlobal": source_matches,
        "autoDiscoveryPath": true,
        "gitAiBinaryExistsForExtension": binary_exists,
        "registersStatus": registers_status,
        "tracksTools": tools,
        "status": status,
        "message": if problems.is_empty() { "Pi auto-discovery path is configured and git-ai extension looks ready".to_string() } else { problems.join("; ") },
    })
}

fn num_or_null(value: Option<&Value>) -> Option<f64> {
    match value {
        Some(Value::Number(n)) => n.as_f64(),
        Some(Value::String(s)) => s.parse::<f64>().ok(),
        _ => None,
    }
}

fn company_has_ai_mark(payload: &Value) -> bool {
    let ai_note = payload.get("ai_note");
    let stats = payload.get("stats");
    num_or_null(ai_note.and_then(|v| v.get("ai_lines_total"))).unwrap_or(0.0) > 0.0
        || num_or_null(ai_note.and_then(|v| v.get("frontmatter_ai_lines"))).unwrap_or(0.0) > 0.0
        || num_or_null(ai_note.and_then(|v| v.get("prompts_count"))).unwrap_or(0.0) > 0.0
        || num_or_null(stats.and_then(|v| v.get("ai_additions"))).unwrap_or(0.0) > 0.0
        || num_or_null(stats.and_then(|v| v.get("ai_rate"))).unwrap_or(0.0) > 0.0
}

async fn check_company_ai_mark(client: &reqwest::Client, record: &Value) -> Value {
    let project_name = record
        .get("projectName")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let commit_sha = record
        .get("commitSha")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if project_name.is_empty() || commit_sha.is_empty() {
        return json!({ "companyStatus": "check_failed", "companyError": "missing projectName or commitSha" });
    }
    let endpoint = env::var("AGENT_PANEL_AI_STATS_CHECK_URL")
        .unwrap_or_else(|_| "http://10.24.12.40/api/ai-stats/check-commit".into());
    let mut query: Vec<(&str, &str)> =
        vec![("project_name", project_name), ("commit_sha", commit_sha)];
    if let Some(gitlab_id) = record
        .get("gitlabProjectId")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
    {
        query.push(("gitlab_project_id", gitlab_id));
    }
    // Retry once on transient send errors (connection reset / timeout) so a
    // brief network blip doesn't permanently mark the record as check_failed.
    let mut resp = None;
    let mut last_err = String::new();
    for attempt in 0..2u8 {
        if attempt == 1 {
            sleep(Duration::from_millis(800)).await;
        }
        let req = client
            .get(endpoint.as_str())
            .query(&query)
            .header("Accept", "application/json")
            .timeout(Duration::from_secs(6));
        match req.send().await {
            Ok(v) => {
                resp = Some(v);
                break;
            }
            Err(err) => last_err = err.to_string(),
        }
    }
    let resp = match resp {
        Some(v) => v,
        None => return json!({ "companyStatus": "check_failed", "companyError": last_err }),
    };
    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    let payload: Value = serde_json::from_str(&text).unwrap_or_else(|_| json!({ "detail": text }));
    if !status.is_success() {
        return json!({ "companyStatus": "check_failed", "companyError": payload.get("detail").and_then(Value::as_str).unwrap_or("company API HTTP error") });
    }
    if payload.get("detail").is_some() && payload.get("commit").is_none() {
        return json!({ "companyStatus": "not_found", "companyError": payload.get("detail").and_then(Value::as_str).unwrap_or("not found") });
    }
    let Some(commit) = payload.get("commit") else {
        return json!({ "companyStatus": "check_failed", "companyError": "公司接口未返回 commit 对象" });
    };
    let stats = payload.get("stats").unwrap_or(&Value::Null);
    let ai_note = payload.get("ai_note").unwrap_or(&Value::Null);
    json!({
        "companyStatus": if company_has_ai_mark(&payload) { "confirmed_ai" } else { "missing_ai" },
        "companyError": Value::Null,
        "commitWebUrl": commit.get("web_url").cloned().unwrap_or(Value::Null),
        "commitTitle": commit.get("title").cloned().unwrap_or(Value::Null),
        "committedAt": commit.get("committed_at").cloned().unwrap_or(Value::Null),
        "originBranch": commit.get("origin_branch").or_else(|| commit.get("branch")).cloned().unwrap_or(Value::Null),
        "additions": commit.get("additions").cloned().unwrap_or(Value::Null),
        "deletions": commit.get("deletions").cloned().unwrap_or(Value::Null),
        "aiRate": stats.get("ai_rate").cloned().unwrap_or(Value::Null),
        "aiLines": stats.get("ai_additions").or_else(|| ai_note.get("ai_lines_total")).cloned().unwrap_or(Value::Null),
        "humanLines": stats.get("human_additions").cloned().unwrap_or(Value::Null),
    })
}

fn apply_company_result(record: &mut Value, result: Value, checked_at: i64) {
    let Some(obj) = record.as_object_mut() else {
        return;
    };
    obj.insert("companyCheckedAt".into(), json!(checked_at));
    for key in [
        "companyStatus",
        "companyError",
        "commitWebUrl",
        "commitTitle",
        "committedAt",
        "originBranch",
        "additions",
        "deletions",
        "aiRate",
        "aiLines",
        "humanLines",
    ] {
        if let Some(value) = result.get(key) {
            if !value.is_null() || key == "companyError" {
                obj.insert(key.into(), value.clone());
            }
        }
    }
}

async fn api_git_ai_suspects_refresh(
    State(state): State<AppState>,
    Json(payload): Json<Value>,
) -> ApiResult<Json<Value>> {
    let limit = payload.get("limit").and_then(Value::as_u64).unwrap_or(200) as usize;
    let force = payload
        .get("force")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let store_path = state.data_dir.join("git-ai-suspects.json");
    let mut store = read_json_if_exists(&store_path)
        .await
        .unwrap_or_else(|| json!({ "version": 1, "records": [] }));
    let mut records = store
        .get("records")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    records.sort_by_key(|r| -(r.get("lastSeenAt").and_then(Value::as_i64).unwrap_or(0)));
    let count = records.len().min(limit);
    // Reuse one HTTP client (pooled connections) for the whole batch. By
    // default skip records already confirmed by the company -- they don't
    // change, and re-checking all of them only amplifies transient failures.
    // Set force=true to re-check every record.
    let client = reqwest::Client::builder()
        .pool_idle_timeout(Duration::from_secs(30))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());
    for record in records.iter_mut().take(count) {
        if !force && record.get("companyStatus").and_then(Value::as_str) == Some("confirmed_ai") {
            continue;
        }
        let result = check_company_ai_mark(&client, record).await;
        apply_company_result(record, result, now_ms());
    }
    store["records"] = Value::Array(records);
    atomic_write_json(&store_path, &store).await?;
    Ok(Json(api_git_ai_suspects_payload(&state).await))
}

async fn api_git_ai_suspects_payload(state: &AppState) -> Value {
    let store_path = state.data_dir.join("git-ai-suspects.json");
    let records = read_json_if_exists(&store_path)
        .await
        .and_then(|v| v.get("records").and_then(Value::as_array).cloned())
        .unwrap_or_default();
    let mut pending = 0;
    let mut confirmed_ai = 0;
    let mut missing_ai = 0;
    let mut not_found = 0;
    let mut check_failed = 0;
    for record in &records {
        match record
            .get("companyStatus")
            .and_then(Value::as_str)
            .unwrap_or("pending")
        {
            "confirmed_ai" => confirmed_ai += 1,
            "missing_ai" => missing_ai += 1,
            "not_found" => not_found += 1,
            "check_failed" => check_failed += 1,
            _ => pending += 1,
        }
    }
    json!({
        "records": records,
        "stats": { "total": pending + not_found + check_failed, "pending": pending, "confirmedAi": confirmed_ai, "missingAi": missing_ai, "notFound": not_found, "checkFailed": check_failed },
        "generatedAt": now_ms()
    })
}

async fn api_git_ai_suspects(State(state): State<AppState>) -> Json<Value> {
    Json(api_git_ai_suspects_payload(&state).await)
}

/// One-click AI-note fix for a single suspect commit.
///
/// Workflow mirrors the user's spec:
///   1. In the commit's repo: `git fetch origin refs/notes/ai`,
///      `git notes --ref=ai merge -s cat_sort_uniq FETCH_HEAD`,
///      `git push origin refs/notes/ai` (with GIT_AI_SKIP=1 to bypass
///      the git-ai pre-push guard).
///   2. Sleep 4s, then re-query the company check-commit API.
///   3. If the company still says missing, spawn a non-interactive pi agent
///      with the `git-ai-fix-note` skill so it generates and pushes a fresh
///      note for that specific commit.
///
/// The pi agent runs detached (no .await on its completion) so the HTTP
/// response returns quickly with a "dispatched" status; the frontend polls
/// the suspects feed afterwards to see the updated company status.
async fn api_git_ai_suspect_fix_note(
    State(state): State<AppState>,
    Json(payload): Json<Value>,
) -> ApiResult<Json<Value>> {
    let id = payload
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    if id.is_empty() {
        return Err(ApiError::bad_request("missing record id"));
    }

    // Resolve the stored suspect record so we know repoPath / projectName / commitSha.
    let store_path = state.data_dir.join("git-ai-suspects.json");
    let store = read_json_if_exists(&store_path)
        .await
        .unwrap_or_else(|| json!({ "version": 1, "records": [] }));
    let record = store
        .get("records")
        .and_then(Value::as_array)
        .and_then(|arr| {
            arr.iter()
                .find(|r| r.get("id").and_then(Value::as_str) == Some(&id))
        })
        .cloned()
        .ok_or_else(|| ApiError::bad_request(format!("suspect record not found: {id}")))?;

    let project_name = record
        .get("projectName")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let commit_sha = record
        .get("commitSha")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let repo_path = resolve_git_ai_repo_path(&record);
    let Some(repo_path) = repo_path else {
        return Err(ApiError::bad_request(
            "record has no repoPath and the repo could not be located under ~/Developer/company/WMS",
        ));
    };
    if !repo_path.is_dir() {
        return Err(ApiError::bad_request(format!(
            "repo path does not exist: {}",
            repo_path.display()
        )));
    }

    // Step 1: re-push local notes to the remote.
    let push_steps = repush_git_ai_notes(&repo_path).await;

    // Step 2: wait 4s then re-check the company API.
    sleep(Duration::from_secs(4)).await;
    let client = reqwest::Client::new();
    let recheck = check_company_ai_mark(&client, &record).await;
    let still_missing = recheck
        .get("companyStatus")
        .and_then(Value::as_str)
        .map(|s| s != "confirmed_ai")
        .unwrap_or(true);

    // Persist the recheck result onto the stored record immediately.
    {
        let mut store = store;
        if let Some(records) = store.get_mut("records").and_then(Value::as_array_mut) {
            if let Some(rec) = records
                .iter_mut()
                .find(|r| r.get("id").and_then(Value::as_str) == Some(&id))
            {
                apply_company_result(rec, recheck.clone(), now_ms());
            }
        }
        atomic_write_json(&store_path, &store).await.ok();
    }

    let mut result = json!({
        "ok": true,
        "recheck": recheck,
        "pushSteps": push_steps,
        "stillMissing": still_missing,
    });

    // Step 3: if still missing, dispatch a non-interactive pi agent with the
    // git-ai-fix-note skill. The skill path resolves to the WMS project-local
    // copy (symlinked into ~/.agents/skills as well).
    if still_missing {
        let skill_path = home_dir()
            .ok()
            .map(|h| h.join("Developer/company/WMS/.agents/skills/git-ai-fix-note/SKILL.md"))
            .filter(|p| p.exists());
        match skill_path {
            Some(path) => {
                let prompt = format!(
                    "为 commit {commit_sha} 补全缺失的 git-ai 作者标注信息（git notes --ref=ai）。\
                     仓库路径：{repo}。项目名：{project}。\
                     先执行 git-ai-fix-note skill 的完整流程，确认目标 commit 缺失 AI note 后再补标；\
                     禁止用 --force 覆盖已有完整 note。",
                    repo = repo_path.display(),
                    project = project_name,
                );
                let session_id = Uuid::new_v4().to_string();
                let status = spawn_pi_fix_note_agent(
                    &repo_path,
                    &session_id,
                    &path.to_string_lossy(),
                    &prompt,
                )
                .await;
                result["piAgent"] = json!({
                    "dispatched": status.ok,
                    "sessionId": session_id,
                    "skillPath": path,
                    "message": if status.ok {
                        "pi agent 已在后台启动，正在用 git-ai-fix-note skill 补标".to_string()
                    } else {
                        status.message
                    },
                });
            }
            None => {
                result["piAgent"] = json!({
                    "dispatched": false,
                    "message": "未找到 git-ai-fix-note skill；请手动运行 pi 并加载该 skill 补标",
                });
            }
        }
    }

    Ok(Json(result))
}

/// Re-push local refs/notes/ai to the remote for the commit's repo.
/// Returns the three step results so the UI can surface failures.
async fn repush_git_ai_notes(repo_path: &Path) -> Vec<Value> {
    let env = vec![("GIT_AI_SKIP", "1")];
    let steps = [
        ("fetch notes", vec!["fetch", "origin", "refs/notes/ai"]),
        (
            "merge notes",
            vec![
                "notes",
                "--ref=ai",
                "merge",
                "-s",
                "cat_sort_uniq",
                "FETCH_HEAD",
            ],
        ),
        (
            "push notes",
            vec!["push", "origin", "refs/notes/ai:refs/notes/ai"],
        ),
    ];
    let mut out = Vec::new();
    for (label, args) in steps {
        let res = git_with_env(repo_path, &args, &env, 30_000, COMMAND_OUTPUT_LIMIT).await;
        out.push(json!({
            "label": label,
            "command": format!("GIT_AI_SKIP=1 git {}", args.join(" ")),
            "ok": res.ok,
            "stdout": compact(&res.stdout, 600),
            "stderr": compact(&res.stderr, 600),
        }));
    }
    out
}

struct SpawnResult {
    ok: bool,
    message: String,
}

/// Spawn a detached `pi -p --skill <skill> <prompt>` process in the repo.
/// The process is detached so the HTTP request returns immediately; the
/// agent writes its own session JSONL which the panel can inspect later.
async fn spawn_pi_fix_note_agent(
    repo_path: &Path,
    session_id: &str,
    skill_path: &str,
    prompt: &str,
) -> SpawnResult {
    let mut cmd = Command::new("pi");
    cmd.current_dir(repo_path)
        .arg("-p")
        .arg("--session-id")
        .arg(session_id)
        .arg("--name")
        .arg(format!("git-ai-fix-note {}", &commit_short(session_id)))
        .arg("--skill")
        .arg(skill_path)
        .arg("--tools")
        .arg("bash,read,write")
        .arg("--approve")
        .arg(prompt)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    match cmd.spawn() {
        Ok(child) => {
            let pid = child.id();
            tracing::info!(
                ?pid,
                %session_id,
                "spawned pi git-ai-fix-note agent"
            );
            SpawnResult {
                ok: true,
                message: format!("pi agent spawned (pid {})", pid.unwrap_or(0)),
            }
        }
        Err(err) => SpawnResult {
            ok: false,
            message: format!("failed to spawn pi agent: {err}"),
        },
    }
}

fn commit_short(s: &str) -> &str {
    s.get(..8).unwrap_or(s)
}

/// Resolve the on-disk repo path for a suspect record. Uses repoPath when
/// present; otherwise looks for `yl-cwhsea-wms-<projectName>` (stripping the
/// `yl-cwhsea-wms-` prefix from projectName for leaf matching) under the
/// WMS backend/frontend/pda/infra areas.
fn resolve_git_ai_repo_path(record: &Value) -> Option<PathBuf> {
    if let Some(path) = record
        .get("repoPath")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
    {
        let p = PathBuf::from(path);
        if p.is_dir() {
            return Some(p);
        }
    }
    let project = record
        .get("projectName")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if project.is_empty() {
        return None;
    }
    let home = home_dir().ok()?;
    let leaf = if let Some(stripped) = project.strip_prefix("yl-cwhsea-wms-") {
        format!("yl-cwhsea-wms-{stripped}")
    } else if project.starts_with("yl-cwhsea-wms") {
        project.to_string()
    } else {
        format!("yl-cwhsea-wms-{project}")
    };
    for area in ["backend", "frontend", "pda", "infra"] {
        let candidate = home.join("Developer/company/WMS").join(area).join(&leaf);
        if candidate.is_dir() {
            return Some(candidate);
        }
    }
    None
}

/// Like `git()` but with extra environment variables (e.g. GIT_AI_SKIP=1).
async fn git_with_env(
    cwd: &Path,
    args: &[&str],
    env: &[(&str, &str)],
    timeout_ms: u64,
    max_output: usize,
) -> GitCommandResult {
    let command = std::iter::once("git".to_string())
        .chain(args.iter().map(|a| shell_quote(a)))
        .collect::<Vec<_>>()
        .join(" ");
    let mut cmd = Command::new("git");
    cmd.args(args).current_dir(cwd);
    for (k, v) in env {
        cmd.env(k, v);
    }
    let fut = cmd.output();
    match timeout(Duration::from_millis(timeout_ms), fut).await {
        Ok(Ok(output)) => {
            let (stdout, stdout_truncated) = limit_output(
                String::from_utf8_lossy(&output.stdout).to_string(),
                max_output,
            );
            let (stderr, stderr_truncated) = limit_output(
                String::from_utf8_lossy(&output.stderr).to_string(),
                max_output,
            );
            GitCommandResult {
                ok: output.status.success(),
                code: output.status.code(),
                command,
                stdout,
                stderr,
                output_truncated: stdout_truncated || stderr_truncated,
                timed_out: false,
            }
        }
        Ok(Err(err)) => GitCommandResult {
            ok: false,
            code: None,
            command,
            stdout: String::new(),
            stderr: err.to_string(),
            output_truncated: false,
            timed_out: false,
        },
        Err(_) => GitCommandResult {
            ok: false,
            code: None,
            command,
            stdout: String::new(),
            stderr: format!("timed out after {timeout_ms}ms"),
            output_truncated: false,
            timed_out: true,
        },
    }
}

async fn ok_json() -> Json<Value> {
    Json(json!({ "ok": true }))
}

type ApiResult<T> = std::result::Result<T, ApiError>;

#[derive(Debug)]
struct ApiError {
    status: StatusCode,
    message: String,
}

impl ApiError {
    fn bad_request(message: impl Into<String>) -> Self {
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
        (self.status, Json(json!({ "error": self.message }))).into_response()
    }
}

struct FormOrJson<T>(T);

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

fn is_json(headers: &HeaderMap) -> bool {
    headers
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.starts_with("application/json"))
        .unwrap_or(false)
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

fn system_time_to_ms(value: SystemTime) -> i64 {
    value
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

fn home_dir() -> Result<PathBuf> {
    dirs::home_dir().ok_or_else(|| anyhow!("cannot resolve home directory"))
}

fn config_path(state: &AppState) -> PathBuf {
    state.data_dir.join(CONFIG_FILE)
}

async fn read_config(state: &AppState) -> Result<AppConfig> {
    let path = config_path(state);
    if !path.exists() {
        return Ok(AppConfig::default());
    }
    let raw = fs::read_to_string(path).await.unwrap_or_default();
    if raw.trim().is_empty() {
        return Ok(AppConfig::default());
    }
    let mut cfg: AppConfig = serde_json::from_str(&raw).unwrap_or_default();
    cfg.requirement_scan_roots = normalize_scan_roots(cfg.requirement_scan_roots);
    Ok(cfg)
}

async fn write_config(state: &AppState, cfg: &AppConfig) -> Result<()> {
    atomic_write_json(&config_path(state), cfg).await
}

fn normalize_scan_roots(values: Vec<String>) -> Vec<String> {
    let developer = home_dir().unwrap_or_default().join("Developer");
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for raw in values {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            continue;
        }
        let path = if trimmed == "~" {
            home_dir().unwrap_or_default()
        } else if let Some(rest) = trimmed.strip_prefix("~/") {
            home_dir().unwrap_or_default().join(rest)
        } else {
            let p = PathBuf::from(trimmed);
            if p.is_absolute() {
                p
            } else {
                developer.join(trimmed)
            }
        };
        let text = path.to_string_lossy().to_string();
        if seen.insert(text.clone()) {
            out.push(text);
        }
    }
    out
}

fn associations_path(state: &AppState) -> PathBuf {
    state.data_dir.join(ASSOCIATIONS_FILE)
}

async fn load_associations(state: &AppState) -> Result<AssociationsStore> {
    let path = associations_path(state);
    if !path.exists() {
        return Ok(AssociationsStore {
            version: 2,
            associations: HashMap::new(),
        });
    }
    let raw = fs::read_to_string(path).await.unwrap_or_default();
    Ok(serde_json::from_str(&raw).unwrap_or(AssociationsStore {
        version: 2,
        associations: HashMap::new(),
    }))
}

async fn save_associations(state: &AppState, store: &AssociationsStore) -> Result<()> {
    atomic_write_json(&associations_path(state), store).await
}

async fn associate_session(state: &AppState, req_id: &str, session_id: &str) -> Result<()> {
    if req_id.trim().is_empty() || session_id.trim().is_empty() {
        return Ok(());
    }
    let mut store = load_associations(state).await?;
    for (k, sids) in store.associations.iter_mut() {
        if k != req_id {
            sids.retain(|s| s != session_id);
        }
    }
    store.associations.retain(|_, sids| !sids.is_empty());
    let entry = store.associations.entry(req_id.to_string()).or_default();
    if !entry.iter().any(|s| s == session_id) {
        entry.push(session_id.to_string());
    }
    save_associations(state, &store).await
}

async fn dissociate_session(state: &AppState, req_id: &str, session_id: &str) -> Result<()> {
    let mut store = load_associations(state).await?;
    if let Some(sids) = store.associations.get_mut(req_id) {
        sids.retain(|s| s != session_id);
        if sids.is_empty() {
            store.associations.remove(req_id);
        }
    }
    save_associations(state, &store).await
}

async fn resolve_req_scan_dirs(state: &AppState) -> Result<Vec<PathBuf>> {
    let cfg = read_config(state).await?;
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for root in cfg.requirement_scan_roots {
        let root_path = PathBuf::from(root);
        for sub in [".agents/req", "req"] {
            let candidate = root_path.join(sub);
            if candidate.is_dir() {
                let key = candidate.to_string_lossy().to_string();
                if seen.insert(key) {
                    out.push(candidate);
                }
            }
        }
    }
    Ok(out)
}

async fn scan_hermes_requirements(state: &AppState) -> Result<Vec<Requirement>> {
    let mut out = Vec::new();
    let dirs = resolve_req_scan_dirs(state).await?;
    let mut seen = HashSet::new();
    for dir in dirs {
        scan_req_dir(&dir, &mut out).await?;
    }
    out.retain(|r| seen.insert(r.id.clone()));
    Ok(out)
}

async fn scan_req_dir(req_dir: &Path, out: &mut Vec<Requirement>) -> Result<()> {
    let mut entries = match fs::read_dir(req_dir).await {
        Ok(v) => v,
        Err(_) => return Ok(()),
    };
    while let Ok(Some(entry)) = entries.next_entry().await {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') || name == "README.md" {
            continue;
        }
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let project = if name == "_default" {
            DEFAULT_PROJECT_NAME.to_string()
        } else {
            name.clone()
        };
        collect_requirements_recursive(&path, vec![project], vec![], out, 0).await?;
    }
    Ok(())
}

async fn collect_requirements_recursive(
    root: &Path,
    projects: Vec<String>,
    group_path: Vec<String>,
    out: &mut Vec<Requirement>,
    depth: usize,
) -> Result<()> {
    if depth > 6 || !root.is_dir() {
        return Ok(());
    }
    let has_meta = root.join("meta.md").is_file();
    let mut child_dirs = Vec::new();
    let mut rd = match fs::read_dir(root).await {
        Ok(v) => v,
        Err(_) => return Ok(()),
    };
    while let Ok(Some(entry)) = rd.next_entry().await {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') || name == "README.md" {
            continue;
        }
        let path = entry.path();
        if path.is_dir() {
            let has_child_meta = path.join("meta.md").is_file();
            child_dirs.push((name, path, has_child_meta));
        }
    }
    let has_nested_req = child_dirs.iter().any(|(_, _, has)| *has);
    let mut current_projects = projects.clone();
    if has_meta && has_nested_req {
        let dir_name = root
            .file_name()
            .and_then(|v| v.to_str())
            .unwrap_or(DEFAULT_PROJECT_NAME)
            .to_string();
        current_projects.extend(read_requirement_project_tags(root, &dir_name).await);
        current_projects = unique_strings(current_projects);
    } else if has_meta {
        let dir_name = root
            .file_name()
            .and_then(|v| v.to_str())
            .unwrap_or("requirement")
            .to_string();
        if let Some(req) =
            load_requirement_from_dir(root, &dir_name, &projects, &group_path).await?
        {
            out.push(req);
        }
    }
    for (name, path, child_has_meta) in child_dirs {
        let next_group = if child_has_meta {
            group_path.clone()
        } else {
            append_group(&group_path, name)
        };
        Box::pin(collect_requirements_recursive(
            &path,
            current_projects.clone(),
            next_group,
            out,
            depth + 1,
        ))
        .await?;
    }
    Ok(())
}

async fn read_requirement_project_tags(dir: &Path, fallback: &str) -> Vec<String> {
    let path = dir.join("meta.md");
    let raw = fs::read_to_string(path).await.unwrap_or_default();
    let fm = parse_frontmatter(&raw);
    let mut values = Vec::new();
    values.extend(split_list(fm.fields.get("projects")));
    values.extend(split_list(fm.fields.get("project")));
    if let Some(title) = fm.fields.get("title") {
        values.push(title.clone());
    }
    if values.is_empty() {
        values.push(fallback.to_string());
    }
    unique_strings(values)
}

async fn load_requirement_from_dir(
    dir: &Path,
    dir_name: &str,
    parent_projects: &[String],
    group_path: &[String],
) -> Result<Option<Requirement>> {
    let meta_path = dir.join("meta.md");
    if !meta_path.is_file() {
        return Ok(None);
    }
    let meta = fs::metadata(dir).await?;
    let raw = fs::read_to_string(&meta_path).await.unwrap_or_default();
    let fm = parse_frontmatter(&raw);
    let mut id = fm
        .fields
        .get("req-id")
        .cloned()
        .unwrap_or_else(|| dir_name.to_string());
    if id.trim().is_empty() {
        id = dir_name.to_string();
    }
    let mut title = fm
        .fields
        .get("title")
        .cloned()
        .unwrap_or_else(|| dir_name.to_string());
    if title == dir_name {
        if let Some(caps) = Regex::new(r"(?im)^\s*-\s*Title\s*:\s*(.+?)\s*$")
            .unwrap()
            .captures(&raw)
        {
            title = caps
                .get(1)
                .map(|m| m.as_str().trim().to_string())
                .unwrap_or(title);
        }
    }
    let mut status = normalize_status(fm.fields.get("status")).unwrap_or_else(|| "开发中".into());
    let mut category =
        normalize_category(fm.fields.get("category")).unwrap_or_else(|| "需求".into());
    let ones = fm
        .fields
        .get("ones")
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty());
    let mut explicit_projects = Vec::new();
    explicit_projects.extend(split_list(fm.fields.get("project")));
    explicit_projects.extend(split_list(fm.fields.get("projects")));
    let (project_file_projects, project_file_group) = read_project_json(dir).await;
    explicit_projects.extend(project_file_projects);
    let projects = if explicit_projects.is_empty() {
        unique_strings(parent_projects.to_vec())
    } else {
        unique_strings(explicit_projects)
    };
    let projects = if projects.is_empty() {
        vec![DEFAULT_PROJECT_NAME.to_string()]
    } else {
        projects
    };
    let effective_group_path = project_file_group.unwrap_or_else(|| group_path.to_vec());
    let mut created_at = fm
        .fields
        .get("start-date")
        .and_then(|v| parse_date_ms(v))
        .unwrap_or_else(|| system_time_to_ms(meta.modified().unwrap_or(UNIX_EPOCH)));
    let mut updated_at = system_time_to_ms(meta.modified().unwrap_or(UNIX_EPOCH));
    let description = first_paragraph(&fm.body);
    if let Some(state) = read_requirement_state(dir).await? {
        if let Some(s) = state
            .get("status")
            .and_then(Value::as_str)
            .and_then(|s| normalize_status(Some(&s.to_string())))
        {
            status = s;
        }
        if let Some(c) = state
            .get("category")
            .and_then(Value::as_str)
            .and_then(|s| normalize_category(Some(&s.to_string())))
        {
            category = c;
        }
        if let Some(ts) = state.get("updatedAt").and_then(Value::as_i64) {
            updated_at = updated_at.max(ts);
        }
    }
    if created_at <= 0 {
        created_at = updated_at;
    }
    let effort = read_json_if_exists(&dir.join("effort-estimate.json")).await;
    let project = projects
        .first()
        .cloned()
        .unwrap_or_else(|| DEFAULT_PROJECT_NAME.into());
    Ok(Some(Requirement {
        id,
        title,
        status,
        projects,
        project,
        group_path: effective_group_path,
        description,
        session_ids: Vec::new(),
        category: Some(category),
        ones,
        created_at,
        updated_at,
        req_dir: Some(dir.to_string_lossy().to_string()),
        meta_path: Some(meta_path.to_string_lossy().to_string()),
        background_path: path_if_exists(dir.join("background.md")),
        branch_path: path_if_exists(dir.join("branch.md")),
        test_path: path_if_exists(dir.join("test.md")),
        notes_path: path_if_exists(dir.join("notes.md")),
        config_path: path_if_exists(dir.join("config-changes.md")),
        impact_path: path_if_exists(dir.join("impact.md")),
        memory_path: path_if_exists(dir.join("memory.md")),
        review_path: path_if_exists(dir.join("review.md")),
        alignment_path: path_if_exists(dir.join("alignment.md")),
        prd_path: path_if_exists(dir.join("prd.md")),
        effort_estimate: effort,
    }))
}

async fn read_project_json(dir: &Path) -> (Vec<String>, Option<Vec<String>>) {
    let path = dir.join("project.json");
    let Some(v) = read_json_if_exists(&path).await else {
        return (Vec::new(), None);
    };
    let mut projects = Vec::new();
    projects.extend(value_to_list(v.get("project")));
    projects.extend(value_to_list(v.get("projects")));
    let group = value_to_path(
        v.get("groupPath")
            .or_else(|| v.get("subproject"))
            .or_else(|| v.get("path")),
    );
    (unique_strings(projects), group)
}

async fn list_requirements(state: &AppState) -> Result<Vec<Requirement>> {
    let mut reqs = scan_hermes_requirements(state).await?;
    let store = load_associations(state).await?;
    for req in &mut reqs {
        req.session_ids = store.associations.get(&req.id).cloned().unwrap_or_default();
    }
    reqs.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    Ok(reqs)
}

async fn get_requirement(state: &AppState, id: &str) -> Result<Option<Requirement>> {
    if id == DEFAULT_REQ_ID {
        let store = load_associations(state).await?;
        let sessions = store
            .associations
            .get(DEFAULT_REQ_ID)
            .cloned()
            .unwrap_or_default();
        return Ok(Some(default_requirement(sessions)));
    }
    Ok(list_requirements(state)
        .await?
        .into_iter()
        .find(|r| r.id == id))
}

async fn get_real_requirement(state: &AppState, id: &str) -> Result<Requirement> {
    get_requirement(state, id)
        .await?
        .filter(|r| r.id != DEFAULT_REQ_ID)
        .ok_or_else(|| anyhow!("requirement not found: {id}"))
}

fn default_requirement(session_ids: Vec<String>) -> Requirement {
    let now = now_ms();
    Requirement {
        id: DEFAULT_REQ_ID.into(),
        title: "默认需求".into(),
        status: "开发中".into(),
        projects: vec![DEFAULT_PROJECT_NAME.into()],
        project: DEFAULT_PROJECT_NAME.into(),
        group_path: Vec::new(),
        description: "未关联到具体需求的 session 归属到此默认需求。".into(),
        session_ids,
        category: Some("需求".into()),
        ones: None,
        created_at: now,
        updated_at: now,
        req_dir: None,
        meta_path: None,
        background_path: None,
        branch_path: None,
        test_path: None,
        notes_path: None,
        config_path: None,
        impact_path: None,
        memory_path: None,
        review_path: None,
        alignment_path: None,
        prd_path: None,
        effort_estimate: None,
    }
}

fn build_dashboard_stats(requirements: Vec<Requirement>, now: i64) -> DashboardStats {
    let real: Vec<Requirement> = requirements
        .into_iter()
        .filter(|r| r.id != DEFAULT_REQ_ID)
        .collect();
    let total = real.len();
    let status_counts = REQ_STATUSES
        .iter()
        .map(|status| {
            let count = real.iter().filter(|r| r.status == *status).count();
            let percent = if total > 0 {
                ((count as f64 / total as f64) * 1000.0).round() / 10.0
            } else {
                0.0
            };
            StatusCount {
                status: status.to_string(),
                count,
                percent,
            }
        })
        .collect();
    let mut durations: Vec<RequirementDuration> = real
        .into_iter()
        .map(|req| {
            let end = if req.status == "已完成" {
                req.updated_at
            } else {
                now
            };
            RequirementDuration {
                duration_ms: (end - req.created_at).max(0),
                req,
            }
        })
        .collect();
    durations.sort_by(|a, b| b.duration_ms.cmp(&a.duration_ms));
    let mut completed: Vec<i64> = durations
        .iter()
        .filter(|d| d.req.status == "已完成")
        .map(|d| d.duration_ms)
        .collect();
    completed.sort_unstable();
    let completed_count = completed.len();
    let avg = if completed.is_empty() {
        0
    } else {
        completed.iter().sum::<i64>() / completed.len() as i64
    };
    let median = if completed.is_empty() {
        0
    } else if completed.len() % 2 == 0 {
        (completed[completed.len() / 2 - 1] + completed[completed.len() / 2]) / 2
    } else {
        completed[completed.len() / 2]
    };
    let max = completed.last().copied().unwrap_or(0);
    DashboardStats {
        total,
        status_counts,
        durations,
        avg_delivery_ms: avg,
        median_delivery_ms: median,
        max_delivery_ms: max,
        completed_count,
        in_progress_count: total.saturating_sub(completed_count),
    }
}

async fn read_branch_scope(req_dir: &Path) -> Result<Option<BranchScope>> {
    let Some(raw) = read_json_if_exists(&req_dir.join(BRANCH_SCOPE_FILE)).await else {
        return Ok(None);
    };
    let mut scope: BranchScope = serde_json::from_value(raw).unwrap_or_default();
    scope.repos.retain(|repo| !repo.repo_name.trim().is_empty());
    for repo in &mut scope.repos {
        repo.repo_name = repo.repo_name.trim().to_string();
        repo.branches = repo
            .branches
            .iter()
            .map(|b| b.trim().to_string())
            .filter(|b| !b.is_empty())
            .collect();
    }
    if scope.repos.is_empty() {
        return Ok(None);
    }
    if scope.version <= 0 {
        scope.version = 1;
    }
    if scope.updated_at <= 0 {
        scope.updated_at = now_ms();
    }
    Ok(Some(scope))
}

async fn run_code_review_scan(req_dir: &Path, req_id: &str, scope: &BranchScope) -> Result<Value> {
    let mut repos = Vec::new();
    for repo in &scope.repos {
        let branches = if repo.branches.is_empty() {
            vec![String::new()]
        } else {
            repo.branches.clone()
        };
        for branch in branches {
            repos.push(scan_repo_branch(repo, &branch).await);
        }
    }
    let review = json!({
        "version": 1,
        "reqId": req_id,
        "updatedAt": now_ms(),
        "baseRef": "origin/master",
        "frontendBaseRef": "origin/production",
        "backendBaseRef": "origin/master",
        "sourceFallback": scope.fallback,
        "repos": repos,
    });
    atomic_write_json(&req_dir.join(CODE_REVIEW_FILE), &review).await?;
    Ok(review)
}

async fn run_master_diff_scan(req_id: &str, scope: &BranchScope, base_ref: &str) -> Result<Value> {
    let mut repos = Vec::new();
    for repo in &scope.repos {
        let branches = if repo.branches.is_empty() {
            vec![String::new()]
        } else {
            repo.branches.clone()
        };
        let repo_base = if repo.role.as_deref() == Some("前端")
            || repo.path.as_deref().unwrap_or("").contains("/frontend/")
        {
            "origin/production"
        } else {
            base_ref
        };
        for branch in branches {
            repos.push(scan_repo_branch_with_base(repo, &branch, Some(repo_base)).await);
        }
    }
    Ok(json!({
        "version": 1,
        "reqId": req_id,
        "updatedAt": now_ms(),
        "baseRef": base_ref,
        "frontendBaseRef": "origin/production",
        "backendBaseRef": "origin/master",
        "sourceFallback": scope.fallback,
        "repos": repos,
    }))
}

async fn generate_prod_mrs(req: &Requirement, scope: &BranchScope) -> Result<Vec<Value>> {
    let token = gitlab_api_token()?;
    let api_base = env::var("GITLAB_API_URL").unwrap_or_else(|_| DEFAULT_GITLAB_API_URL.into());
    let client = Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .context("build GitLab API client")?;
    let mut results = Vec::new();

    for repo in &scope.repos {
        let target_branch = detect_prod_target_branch(repo);
        let branches = if repo.branches.is_empty() {
            vec![String::new()]
        } else {
            repo.branches.clone()
        };
        let Some(project_path) =
            resolve_code_review_project_path(repo.path.as_deref(), &repo.repo_name)
        else {
            for branch in branches {
                results.push(prod_mr_result(
                    repo,
                    &branch,
                    &target_branch,
                    "failed",
                    None,
                    Some("branches.json 缺少 path"),
                    None,
                ));
            }
            continue;
        };
        if !project_path.exists() {
            for branch in branches {
                results.push(prod_mr_result(
                    repo,
                    &branch,
                    &target_branch,
                    "failed",
                    None,
                    Some(&format!(
                        "仓库路径不存在：{}",
                        project_path.to_string_lossy()
                    )),
                    None,
                ));
            }
            continue;
        }
        let remote = git(
            &project_path,
            &["config", "--get", "remote.origin.url"],
            30_000,
            COMMAND_OUTPUT_LIMIT,
        )
        .await;
        let project_namespace = if remote.ok {
            gitlab_project_path_from_remote(remote.stdout.trim())
        } else {
            None
        };
        let Some(project_namespace) = project_namespace else {
            for branch in branches {
                results.push(prod_mr_result(
                    repo,
                    &branch,
                    &target_branch,
                    "failed",
                    None,
                    Some(&format!("无法解析 GitLab 项目路径：{}", short_err(&remote))),
                    Some(project_path.to_string_lossy().as_ref()),
                ));
            }
            continue;
        };
        for branch in branches {
            results.push(
                create_or_reuse_gitlab_mr(
                    &client,
                    &api_base,
                    &token,
                    req,
                    repo,
                    &project_namespace,
                    &project_path,
                    &branch,
                    &target_branch,
                )
                .await,
            );
        }
    }
    Ok(results)
}

async fn create_or_reuse_gitlab_mr(
    client: &Client,
    api_base: &str,
    token: &str,
    req: &Requirement,
    repo: &BranchRepo,
    project_namespace: &str,
    project_path: &Path,
    source_branch: &str,
    target_branch: &str,
) -> Value {
    let source_branch = source_branch.trim();
    if source_branch.is_empty() {
        return prod_mr_result(
            repo,
            "(未指定分支)",
            target_branch,
            "skipped",
            None,
            Some("branches.json 缺少需求分支"),
            Some(project_path.to_string_lossy().as_ref()),
        );
    }
    let project_key = percent_encode(project_namespace);
    let url = format!(
        "{}/projects/{}/merge_requests",
        api_base.trim_end_matches('/'),
        project_key
    );
    match find_existing_gitlab_mr(client, token, &url, source_branch, target_branch).await {
        Ok(Some(mr)) => {
            return prod_mr_result(
                repo,
                source_branch,
                target_branch,
                "reused",
                Some(&mr),
                None,
                Some(project_path.to_string_lossy().as_ref()),
            );
        }
        Ok(None) => {}
        Err(err) => {
            return prod_mr_result(
                repo,
                source_branch,
                target_branch,
                "failed",
                None,
                Some(&err),
                Some(project_path.to_string_lossy().as_ref()),
            );
        }
    }

    let title = format!(
        "{} 生产发布：{} {} -> {}",
        req.id, repo.repo_name, source_branch, target_branch
    );
    let description = format!(
        "Agent Panel 自动创建生产 MR。\n\n- Req: `{}` {}\n- Repo: `{}`\n- Source: `{}`\n- Target: `{}`\n\n请组长审批后合入生产分支。",
        req.id, req.title, repo.repo_name, source_branch, target_branch
    );
    let form = [
        ("source_branch", source_branch),
        ("target_branch", target_branch),
        ("title", title.as_str()),
        ("description", description.as_str()),
        ("remove_source_branch", "false"),
    ];
    let response = client
        .post(&url)
        .header("PRIVATE-TOKEN", token)
        .form(&form)
        .send()
        .await;
    let response = match response {
        Ok(response) => response,
        Err(err) => {
            return prod_mr_result(
                repo,
                source_branch,
                target_branch,
                "failed",
                None,
                Some(&format!("GitLab API request failed: {err}")),
                Some(project_path.to_string_lossy().as_ref()),
            );
        }
    };
    let status = response.status();
    let text = response.text().await.unwrap_or_default();
    if !status.is_success() {
        if let Ok(Some(mr)) =
            find_existing_gitlab_mr(client, token, &url, source_branch, target_branch).await
        {
            return prod_mr_result(
                repo,
                source_branch,
                target_branch,
                "reused",
                Some(&mr),
                Some("创建返回非成功状态，但已找到可复用 open MR"),
                Some(project_path.to_string_lossy().as_ref()),
            );
        }
        return prod_mr_result(
            repo,
            source_branch,
            target_branch,
            "failed",
            None,
            Some(&format!(
                "GitLab API HTTP {}: {}",
                status.as_u16(),
                compact_http_body(&text)
            )),
            Some(project_path.to_string_lossy().as_ref()),
        );
    }
    let mr: Value =
        serde_json::from_str(&text).unwrap_or_else(|_| json!({ "raw": compact_http_body(&text) }));
    prod_mr_result(
        repo,
        source_branch,
        target_branch,
        "created",
        Some(&mr),
        None,
        Some(project_path.to_string_lossy().as_ref()),
    )
}

async fn find_existing_gitlab_mr(
    client: &Client,
    token: &str,
    url: &str,
    source_branch: &str,
    target_branch: &str,
) -> std::result::Result<Option<Value>, String> {
    let response = client
        .get(url)
        .header("PRIVATE-TOKEN", token)
        .query(&[
            ("state", "opened"),
            ("source_branch", source_branch),
            ("target_branch", target_branch),
            ("per_page", "20"),
        ])
        .send()
        .await
        .map_err(|err| format!("GitLab API request failed: {err}"))?;
    let status = response.status();
    let text = response.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(format!(
            "GitLab API HTTP {}: {}",
            status.as_u16(),
            compact_http_body(&text)
        ));
    }
    let value: Value = serde_json::from_str(&text)
        .map_err(|err| format!("GitLab API response is not JSON: {err}"))?;
    Ok(value
        .as_array()
        .and_then(|arr| arr.iter().find(|item| item.is_object()).cloned()))
}

fn prod_mr_result(
    repo: &BranchRepo,
    source_branch: &str,
    target_branch: &str,
    status: &str,
    mr: Option<&Value>,
    error: Option<&str>,
    project_path: Option<&str>,
) -> Value {
    json!({
        "repoName": repo.repo_name,
        "role": repo.role,
        "projectPath": project_path,
        "sourceBranch": source_branch,
        "targetBranch": target_branch,
        "status": status,
        "iid": mr.and_then(|m| m.get("iid")).cloned().unwrap_or(Value::Null),
        "webUrl": mr.and_then(|m| m.get("web_url")).cloned().unwrap_or(Value::Null),
        "title": mr.and_then(|m| m.get("title")).cloned().unwrap_or(Value::Null),
        "error": error,
    })
}

fn detect_prod_target_branch(repo: &BranchRepo) -> String {
    let role = repo.role.as_deref().unwrap_or_default();
    let path = repo.path.as_deref().unwrap_or_default();
    if role.contains("前端") || path.contains("/frontend/") || path.contains("\\frontend\\") {
        "production".into()
    } else {
        "master".into()
    }
}

fn gitlab_api_token() -> Result<String> {
    let token = env::var("GITLAB_TOKEN")
        .or_else(|_| env::var("GL_TOKEN"))
        .or_else(|_| read_agent_panel_env_var("GITLAB_TOKEN"))
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .ok_or_else(|| anyhow!("missing GitLab token: set GITLAB_TOKEN / GL_TOKEN, or create .env.agent with GITLAB_TOKEN"))?;
    Ok(token)
}

fn read_agent_panel_env_var(key: &str) -> std::result::Result<String, env::VarError> {
    let path = env::current_dir()
        .map(|cwd| cwd.join(".env.agent"))
        .unwrap_or_else(|_| PathBuf::from(".env.agent"));
    let text = std::fs::read_to_string(path).map_err(|_| env::VarError::NotPresent)?;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let trimmed = trimmed
            .strip_prefix("export ")
            .unwrap_or(trimmed)
            .trim_start();
        let Some((name, value)) = trimmed.split_once('=') else {
            continue;
        };
        if name.trim() == key {
            return Ok(value
                .trim()
                .trim_matches('"')
                .trim_matches('\'')
                .to_string());
        }
    }
    Err(env::VarError::NotPresent)
}

fn gitlab_project_path_from_remote(remote: &str) -> Option<String> {
    let mut value = remote.trim().trim_end_matches(".git").to_string();
    if value.is_empty() {
        return None;
    }
    if let Some(rest) = value.strip_prefix("ssh://") {
        return rest
            .split_once('/')
            .map(|(_, path)| path.trim_matches('/').to_string())
            .filter(|path| !path.is_empty());
    }
    if let Some(rest) = value
        .strip_prefix("http://")
        .or_else(|| value.strip_prefix("https://"))
    {
        return rest
            .split_once('/')
            .map(|(_, path)| path.trim_matches('/').to_string())
            .filter(|path| !path.is_empty());
    }
    if let Some((prefix, path)) = value.split_once(':') {
        if prefix.contains('@') {
            let path = path.trim_matches('/').to_string();
            if !path.is_empty() {
                return Some(path);
            }
        }
    }
    if value.starts_with('/') {
        value = value.trim_matches('/').to_string();
    }
    (!value.is_empty()).then_some(value)
}

fn percent_encode(value: &str) -> String {
    let mut out = String::new();
    for b in value.as_bytes() {
        match *b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*b as char)
            }
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

fn compact_http_body(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(600)
        .collect()
}

async fn scan_repo_branch(repo: &BranchRepo, branch: &str) -> Value {
    scan_repo_branch_with_base(repo, branch, None).await
}

async fn scan_repo_branch_with_base(
    repo: &BranchRepo,
    branch: &str,
    forced_base_ref: Option<&str>,
) -> Value {
    let mut warnings = Vec::<String>::new();
    let base_ref = forced_base_ref
        .filter(|v| !v.trim().is_empty())
        .map(str::trim)
        .map(str::to_string)
        .or_else(|| {
            repo.base_ref
                .as_deref()
                .filter(|v| !v.trim().is_empty())
                .map(str::trim)
                .map(str::to_string)
        })
        .unwrap_or_else(|| detect_default_base_ref(repo));
    let base_info = parse_base_ref(&base_ref);
    let branch = branch.trim();
    let project_path = resolve_code_review_project_path(repo.path.as_deref(), &repo.repo_name);
    let Some(project_path) = project_path else {
        return empty_repo_snapshot(
            repo,
            branch,
            &base_info,
            warnings,
            "branches.json 缺少 path",
        );
    };
    if !project_path.exists() {
        return empty_repo_snapshot(
            repo,
            branch,
            &base_info,
            warnings,
            &format!("仓库路径不存在：{}", project_path.to_string_lossy()),
        );
    }
    if branch.is_empty() {
        return empty_repo_snapshot(
            repo,
            "(未指定分支)",
            &base_info,
            warnings,
            "branches.json 缺少需求分支",
        );
    }
    let git_root = git(
        &project_path,
        &["rev-parse", "--show-toplevel"],
        30_000,
        COMMAND_OUTPUT_LIMIT,
    )
    .await;
    if !git_root.ok {
        return empty_repo_snapshot(
            repo,
            branch,
            &base_info,
            warnings,
            "projectPath 不是 Git 仓库",
        );
    }

    let current_branch = git(
        &project_path,
        &["rev-parse", "--abbrev-ref", "HEAD"],
        30_000,
        COMMAND_OUTPUT_LIMIT,
    )
    .await;
    let dirty_state = git(
        &project_path,
        &["status", "--porcelain"],
        30_000,
        COMMAND_OUTPUT_LIMIT,
    )
    .await;
    let (target_ref, target_warning) = resolve_target_ref(&project_path, branch).await;
    if let Some(warning) = target_warning {
        warnings.push(warning);
    }

    let commit_range = format!("{}..{}", base_info.base_ref, target_ref);
    let diff_range = format!("{}...{}", base_info.base_ref, target_ref);
    let commits = git(
        &project_path,
        &[
            "log",
            "--oneline",
            "--decorate=short",
            "--max-count=80",
            &commit_range,
        ],
        30_000,
        COMMAND_OUTPUT_LIMIT,
    )
    .await;
    if !commits.ok {
        warnings.push(format!("提交列表读取失败：{}", short_err(&commits)));
    }
    let name_status = git(
        &project_path,
        &["diff", "--name-status", "--find-renames", &diff_range, "--"],
        30_000,
        COMMAND_OUTPUT_LIMIT,
    )
    .await;
    if !name_status.ok {
        warnings.push(format!("文件列表读取失败：{}", short_err(&name_status)));
    }
    let numstat = git(
        &project_path,
        &["diff", "--numstat", "--find-renames", &diff_range, "--"],
        30_000,
        COMMAND_OUTPUT_LIMIT,
    )
    .await;
    if !numstat.ok {
        warnings.push(format!("增删行统计读取失败：{}", short_err(&numstat)));
    }
    let diff = git(
        &project_path,
        &[
            "diff",
            "--no-ext-diff",
            "--no-color",
            "--find-renames",
            "--unified=80",
            &diff_range,
            "--",
        ],
        60_000,
        DIFF_OUTPUT_LIMIT,
    )
    .await;
    if !diff.ok {
        warnings.push(format!("Diff 读取失败：{}", short_err(&diff)));
    }

    let files = merge_file_stats(&name_status.stdout, &numstat.stdout);
    let additions: i64 = files.iter().map(|f| f.additions).sum();
    let deletions: i64 = files.iter().map(|f| f.deletions).sum();
    json!({
        "repoName": repo.repo_name,
        "projectPath": project_path.to_string_lossy(),
        "branch": branch,
        "resolvedTargetRef": target_ref,
        "baseRef": base_info.base_ref,
        "currentBranch": current_branch.ok.then(|| current_branch.stdout.trim().to_string()),
        "dirty": dirty_state.ok && !dirty_state.stdout.trim().is_empty(),
        "baseUpdate": read_only_base_update(&base_info),
        "commits": if commits.ok { commits.stdout.lines().filter(|l| !l.trim().is_empty()).map(str::to_string).collect::<Vec<_>>() } else { Vec::<String>::new() },
        "files": files,
        "additions": additions,
        "deletions": deletions,
        "diff": if diff.ok { diff.stdout.clone() } else { String::new() },
        "diffTruncated": diff.output_truncated,
        "warnings": warnings,
        "error": if diff.ok || additions + deletions > 0 { Value::Null } else { Value::String(short_err(&diff)) },
    })
}

fn empty_repo_snapshot(
    repo: &BranchRepo,
    branch: &str,
    base_info: &BaseRefInfo,
    warnings: Vec<String>,
    error: &str,
) -> Value {
    json!({
        "repoName": repo.repo_name,
        "projectPath": resolve_code_review_project_path(repo.path.as_deref(), &repo.repo_name).or_else(|| repo.path.as_ref().map(PathBuf::from)).map(|p| p.to_string_lossy().to_string()),
        "branch": branch,
        "resolvedTargetRef": branch,
        "baseRef": base_info.base_ref,
        "dirty": false,
        "baseUpdate": read_only_base_update(base_info),
        "commits": Vec::<String>::new(),
        "files": Vec::<CodeReviewFileStat>::new(),
        "additions": 0,
        "deletions": 0,
        "diff": "",
        "diffTruncated": false,
        "warnings": warnings,
        "error": error,
    })
}

fn detect_default_base_ref(repo: &BranchRepo) -> String {
    let role = repo.role.as_deref().unwrap_or_default();
    let path = repo.path.as_deref().unwrap_or_default();
    if role == "前端" || path.contains("/frontend/") {
        "origin/production".to_string()
    } else {
        "origin/master".to_string()
    }
}

fn parse_base_ref(input: &str) -> BaseRefInfo {
    let base_ref = if input.trim().is_empty() {
        "origin/master"
    } else {
        input.trim()
    }
    .to_string();
    if base_ref.contains('/') && !base_ref.starts_with("refs/") {
        let mut parts = base_ref.split('/');
        let remote = parts.next().unwrap_or("origin").to_string();
        let remote_branch = parts.collect::<Vec<_>>().join("/");
        let remote_branch = if remote_branch.is_empty() {
            "master".to_string()
        } else {
            remote_branch
        };
        BaseRefInfo {
            base_ref,
            remote,
            local_branch: remote_branch.clone(),
            remote_branch,
        }
    } else {
        BaseRefInfo {
            base_ref: base_ref.clone(),
            remote: "origin".to_string(),
            remote_branch: base_ref.clone(),
            local_branch: base_ref,
        }
    }
}

fn read_only_base_update(info: &BaseRefInfo) -> Value {
    json!({
        "ok": true,
        "remote": info.remote,
        "remoteBranch": info.remote_branch,
        "localBranch": info.local_branch,
        "steps": [{
            "label": "read local git refs",
            "command": "fetch/pull skipped by Rust panel read-only scan",
            "ok": true,
        }],
    })
}

async fn resolve_target_ref(repo_path: &Path, branch: &str) -> (String, Option<String>) {
    let local_ref = format!("{}^{{commit}}", branch);
    let local = git(
        repo_path,
        &["rev-parse", "--verify", &local_ref],
        30_000,
        COMMAND_OUTPUT_LIMIT,
    )
    .await;
    if local.ok {
        return (branch.to_string(), None);
    }
    let remote_branch = format!("origin/{}", branch);
    let remote_ref = format!("{}^{{commit}}", remote_branch);
    let remote = git(
        repo_path,
        &["rev-parse", "--verify", &remote_ref],
        30_000,
        COMMAND_OUTPUT_LIMIT,
    )
    .await;
    if remote.ok {
        return (
            remote_branch.clone(),
            Some(format!("本地分支 {branch} 不存在，已使用 {remote_branch}")),
        );
    }
    (
        branch.to_string(),
        Some(format!("无法验证需求分支 {branch}，diff 可能失败")),
    )
}

fn resolve_code_review_project_path(
    project_path: Option<&str>,
    repo_name: &str,
) -> Option<PathBuf> {
    let raw = project_path?.trim();
    if raw.is_empty() {
        return None;
    }
    let expanded = if raw == "~" {
        home_dir().ok()?
    } else if let Some(rest) = raw.strip_prefix("~/") {
        home_dir().ok()?.join(rest)
    } else {
        PathBuf::from(raw)
    };
    let resolved = if expanded.is_absolute() {
        expanded
    } else {
        env::current_dir().ok()?.join(expanded)
    };
    if resolved.exists() {
        return Some(resolved);
    }
    let leaf = if repo_name.trim().is_empty() {
        resolved.file_name()?.to_string_lossy().to_string()
    } else {
        repo_name.trim().to_string()
    };
    let mut roots = Vec::new();
    if let Some(parent) = resolved.parent() {
        roots.push(parent.to_path_buf());
    }
    if let Ok(home) = home_dir() {
        roots.push(home.join("Developer/company/WMS"));
    }
    for root in roots {
        for area in ["backend", "frontend", "pda", "infra"] {
            let candidate = root.join(area).join(&leaf);
            if candidate.exists() {
                return Some(candidate);
            }
        }
    }
    Some(resolved)
}

fn merge_file_stats(name_status_out: &str, numstat_out: &str) -> Vec<CodeReviewFileStat> {
    let mut by_path: HashMap<String, CodeReviewFileStat> = HashMap::new();
    for line in name_status_out.lines().filter(|l| !l.trim().is_empty()) {
        let cols: Vec<&str> = line.split('\t').collect();
        let status = cols.first().copied().unwrap_or("M").to_string();
        let path = if cols.len() >= 3 && (status.starts_with('R') || status.starts_with('C')) {
            cols[2]
        } else {
            cols.get(1).copied().unwrap_or_default()
        };
        if !path.is_empty() {
            by_path.insert(
                path.to_string(),
                CodeReviewFileStat {
                    path: path.to_string(),
                    status,
                    additions: 0,
                    deletions: 0,
                    risk_tags: Vec::new(),
                },
            );
        }
    }
    for line in numstat_out.lines().filter(|l| !l.trim().is_empty()) {
        let cols: Vec<&str> = line.split('\t').collect();
        if cols.len() < 3 {
            continue;
        }
        let path = normalize_numstat_path(&cols[2..].join("\t"));
        let entry = by_path
            .entry(path.clone())
            .or_insert_with(|| CodeReviewFileStat {
                path: path.clone(),
                status: "M".to_string(),
                additions: 0,
                deletions: 0,
                risk_tags: Vec::new(),
            });
        entry.additions = cols[0].parse::<i64>().unwrap_or(0);
        entry.deletions = cols[1].parse::<i64>().unwrap_or(0);
    }
    let mut files: Vec<CodeReviewFileStat> = by_path
        .into_values()
        .map(|mut f| {
            f.risk_tags = classify_code_review_risk_tags(&f);
            f
        })
        .collect();
    files.sort_by(|a, b| a.path.cmp(&b.path));
    files
}

fn normalize_numstat_path(raw: &str) -> String {
    Regex::new(r"=>\s*(.*)$")
        .ok()
        .and_then(|re| {
            re.captures(raw)
                .and_then(|c| c.get(1).map(|m| m.as_str().to_string()))
        })
        .unwrap_or_else(|| raw.to_string())
        .replace(['{', '}'], "")
        .trim()
        .to_string()
}

fn classify_code_review_risk_tags(file: &CodeReviewFileStat) -> Vec<String> {
    let p = file.path.to_lowercase();
    let mut tags = Vec::new();
    if p.contains("/test/") || p.contains("src/test") {
        tags.push("测试".to_string());
    }
    if p.contains("controller") || p.contains("resource") || p.contains("/api/") {
        tags.push("API".to_string());
    }
    if p.contains("service") || p.contains("manager") {
        tags.push("Service".to_string());
    }
    if p.contains("mapper")
        || p.ends_with(".xml")
        || p.contains("dao")
        || p.ends_with(".sql")
        || p.ends_with("pom.xml")
    {
        tags.push("DB".to_string());
    }
    if p.contains("listener")
        || p.contains("consumer")
        || p.contains("kafka")
        || p.contains("rocket")
        || p.contains("rabbit")
        || p.contains("mq")
    {
        tags.push("MQ".to_string());
    }
    if p.contains("config")
        || p.ends_with(".yml")
        || p.ends_with(".yaml")
        || p.ends_with(".properties")
    {
        tags.push("配置".to_string());
    }
    if file.additions + file.deletions >= 500 {
        tags.push("大改动".to_string());
    }
    tags
}

async fn git(cwd: &Path, args: &[&str], timeout_ms: u64, max_output: usize) -> GitCommandResult {
    let command = std::iter::once("git".to_string())
        .chain(args.iter().map(|a| shell_quote(a)))
        .collect::<Vec<_>>()
        .join(" ");
    let fut = Command::new("git").args(args).current_dir(cwd).output();
    match timeout(Duration::from_millis(timeout_ms), fut).await {
        Ok(Ok(output)) => {
            let (stdout, stdout_truncated) = limit_output(
                String::from_utf8_lossy(&output.stdout).to_string(),
                max_output,
            );
            let (stderr, stderr_truncated) = limit_output(
                String::from_utf8_lossy(&output.stderr).to_string(),
                max_output,
            );
            GitCommandResult {
                ok: output.status.success(),
                code: output.status.code(),
                command,
                stdout,
                stderr,
                output_truncated: stdout_truncated || stderr_truncated,
                timed_out: false,
            }
        }
        Ok(Err(err)) => GitCommandResult {
            ok: false,
            code: None,
            command,
            stdout: String::new(),
            stderr: err.to_string(),
            output_truncated: false,
            timed_out: false,
        },
        Err(_) => GitCommandResult {
            ok: false,
            code: None,
            command,
            stdout: String::new(),
            stderr: format!("timed out after {timeout_ms}ms"),
            output_truncated: false,
            timed_out: true,
        },
    }
}

fn limit_output(value: String, max: usize) -> (String, bool) {
    if value.len() <= max {
        return (value, false);
    }
    (value.chars().take(max).collect::<String>(), true)
}

fn compact(value: &str, max: usize) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.len() > max {
        Some(format!(
            "{}…",
            trimmed.chars().take(max).collect::<String>()
        ))
    } else {
        Some(trimmed.to_string())
    }
}

fn short_err(result: &GitCommandResult) -> String {
    compact(&result.stderr, 600)
        .or_else(|| compact(&result.stdout, 600))
        .unwrap_or_else(|| match result.code {
            Some(code) => format!("{} exited {code}", result.command),
            None if result.timed_out => format!("{} timed out", result.command),
            None => format!("{} failed", result.command),
        })
}

async fn create_requirement(state: &AppState, form: RequirementCreateForm) -> ApiResult<Value> {
    let title = clean_required(&form.title, "title")?;
    let status = form
        .status
        .as_deref()
        .unwrap_or("需求对齐")
        .trim()
        .to_string();
    ensure_status(&status)?;
    let category = form
        .category
        .as_deref()
        .unwrap_or("需求")
        .trim()
        .to_string();
    ensure_category(&category)?;
    let dry_run = form.dry_run.unwrap_or(false);
    let base = resolve_create_req_root(state, form.root.as_deref()).await?;
    let (req_id, target_dir) =
        resolve_req_id_and_target_dir(state, &base, form.req_id.trim(), &form, dry_run).await?;

    let projects = normalize_projects(form.project.as_deref(), form.projects.as_deref());
    let project = projects
        .first()
        .cloned()
        .unwrap_or_else(|| DEFAULT_PROJECT_NAME.to_string());
    let owner = clean_optional(form.owner.as_deref()).unwrap_or_else(|| "unknown".to_string());
    let start_date = clean_optional(form.start_date.as_deref()).unwrap_or_else(today_ymd);
    ensure_date_or_unknown(&start_date, "startDate")?;
    let plan_release =
        clean_optional(form.plan_release.as_deref()).unwrap_or_else(|| "unknown".to_string());
    ensure_date_or_unknown(&plan_release, "planRelease")?;
    let ones = clean_optional(form.ones.as_deref()).unwrap_or_default();
    let summary = clean_optional(form.summary.as_deref()).unwrap_or_else(|| "待补充".to_string());
    let background = clean_optional(form.background.as_deref());
    let notes = clean_optional(form.notes.as_deref());

    let files = requirement_create_files(
        &req_id,
        &title,
        &status,
        &project,
        &projects,
        &category,
        &owner,
        &start_date,
        &plan_release,
        &ones,
        &summary,
        background.as_deref(),
        notes.as_deref(),
    );
    let planned: Vec<String> = files
        .iter()
        .map(|(name, _)| target_dir.join(name).to_string_lossy().to_string())
        .collect();
    if !dry_run {
        fs::create_dir_all(&target_dir).await?;
        for (name, body) in &files {
            atomic_write_text(&target_dir.join(name), body).await?;
        }
    }
    let validation = if dry_run {
        json!({ "ok": true, "dryRun": true, "problems": [], "warnings": [] })
    } else {
        let req = load_requirement_from_dir(&target_dir, &req_id, &[project.clone()], &[])
            .await?
            .ok_or_else(|| anyhow!("created requirement cannot be loaded"))?;
        validate_requirement(state, &req).await?
    };
    Ok(json!({
        "ok": true,
        "dryRun": dry_run,
        "reqId": req_id,
        "title": title,
        "status": status,
        "category": category,
        "project": project,
        "projects": projects,
        "reqDir": target_dir.to_string_lossy(),
        "files": planned,
        "validation": validation,
    }))
}

async fn update_requirement(state: &AppState, form: RequirementPatchForm) -> ApiResult<Value> {
    let req = get_real_requirement(state, &form.req_id).await?;
    let dir = req_dir_path(&req)?;
    ensure_requirement_dir_writable(state, &dir).await?;
    let dry_run = form.dry_run.unwrap_or(false);
    let mut changes = Vec::<String>::new();
    let mut planned_files = Vec::<String>::new();

    let meta_path = dir.join("meta.md");
    let mut meta_next = fs::read_to_string(&meta_path)
        .await
        .unwrap_or_default()
        .replace("\r\n", "\n");
    if let Some(title) = clean_optional(form.title.as_deref()) {
        meta_next = set_frontmatter_field(&meta_next, "title", &title);
        meta_next = update_meta_summary_line(&meta_next, "Title", &title);
        changes.push("meta.title".into());
    }
    if let Some(project) = clean_optional(form.project.as_deref()) {
        meta_next = set_frontmatter_field(&meta_next, "project", &project);
        changes.push("meta.project".into());
    }
    if let Some(projects) = form.projects.as_deref() {
        let value =
            unique_strings(projects.iter().map(|s| s.trim().to_string()).collect()).join(", ");
        meta_next = set_frontmatter_field(&meta_next, "projects", &value);
        changes.push("meta.projects".into());
    }
    if let Some(owner) = clean_optional(form.owner.as_deref()) {
        meta_next = set_frontmatter_field(&meta_next, "owner", &owner);
        meta_next = update_meta_summary_line(&meta_next, "Owner", &owner);
        changes.push("meta.owner".into());
    }
    if let Some(start_date) = clean_optional(form.start_date.as_deref()) {
        ensure_date_or_unknown(&start_date, "startDate")?;
        meta_next = set_frontmatter_field(&meta_next, "start-date", &start_date);
        meta_next = update_meta_summary_line(&meta_next, "Start date", &start_date);
        changes.push("meta.startDate".into());
    }
    if let Some(plan_release) = clean_optional(form.plan_release.as_deref()) {
        ensure_date_or_unknown(&plan_release, "planRelease")?;
        meta_next = set_frontmatter_field(&meta_next, "plan-release", &plan_release);
        meta_next = update_meta_summary_line(&meta_next, "Planned release", &plan_release);
        changes.push("meta.planRelease".into());
    }
    if let Some(ones) = form.ones.as_deref() {
        let value = ones.trim().to_string();
        meta_next = set_frontmatter_field(&meta_next, "ones", &value);
        changes.push("meta.ones".into());
    }
    if let Some(category) = form.category.as_deref() {
        ensure_category(category)?;
        changes.push("state.category".into());
        if !dry_run {
            write_requirement_category(dir.to_string_lossy().as_ref(), category).await?;
        }
        planned_files.push(dir.join(STATE_FILE).to_string_lossy().to_string());
    }
    if let Some(status) = form.status.as_deref() {
        ensure_status(status)?;
        changes.push("state.status".into());
        if !dry_run {
            write_requirement_status(dir.to_string_lossy().as_ref(), status, form.note.as_deref())
                .await?;
        }
        planned_files.push(dir.join(STATE_FILE).to_string_lossy().to_string());
    }
    let old_meta = fs::read_to_string(&meta_path)
        .await
        .unwrap_or_default()
        .replace("\r\n", "\n");
    if meta_next != old_meta {
        planned_files.push(meta_path.to_string_lossy().to_string());
        if !dry_run {
            atomic_write_text(&meta_path, &meta_next).await?;
        }
    }

    let validation = if dry_run {
        json!({ "ok": true, "dryRun": true, "problems": [], "warnings": [] })
    } else {
        let refreshed = get_real_requirement(state, &form.req_id).await?;
        validate_requirement(state, &refreshed).await?
    };
    Ok(json!({
        "ok": true,
        "dryRun": dry_run,
        "reqId": req.id,
        "changes": unique_strings(changes),
        "files": unique_strings(planned_files),
        "validation": validation,
    }))
}

async fn append_requirement_note(state: &AppState, form: RequirementNoteForm) -> ApiResult<Value> {
    let req = get_real_requirement(state, &form.req_id).await?;
    let dir = req_dir_path(&req)?;
    ensure_requirement_dir_writable(state, &dir).await?;
    let text = clean_required(&form.text, "text")?;
    ensure_text_size(&text, "text")?;
    let dry_run = form.dry_run.unwrap_or(false);
    let path = dir.join("notes.md");
    let raw = fs::read_to_string(&path)
        .await
        .unwrap_or_else(|_| format!("# {} Notes\n", req.id));
    let title = clean_optional(form.title.as_deref()).unwrap_or_else(|| "Agent Update".to_string());
    let session = clean_optional(form.session_id.as_deref()).unwrap_or_default();
    let block = format!(
        "\n\n## {} - {}\n{}{}\n",
        today_ymd(),
        title,
        if session.is_empty() {
            String::new()
        } else {
            format!("- Session: `{}`\n", session)
        },
        text.trim()
    );
    let next = format!("{}{}", raw.trim_end(), block);
    if !dry_run {
        atomic_write_text(&path, &next).await?;
    }
    Ok(json!({
        "ok": true,
        "dryRun": dry_run,
        "reqId": req.id,
        "file": path.to_string_lossy(),
        "appendedBytes": block.len(),
    }))
}

async fn write_requirement_doc(state: &AppState, form: RequirementDocForm) -> ApiResult<Value> {
    let req = get_real_requirement(state, &form.req_id).await?;
    let dir = req_dir_path(&req)?;
    ensure_requirement_dir_writable(state, &dir).await?;
    let content = form.content.replace("\r\n", "\n");
    ensure_text_size(&content, "content")?;
    let doc_file = requirement_doc_file(&form.doc_type)?;
    let mode = form.mode.as_deref().unwrap_or("replace").trim();
    if !matches!(mode, "replace" | "append") {
        return Err(ApiError::bad_request(format!("invalid mode: {mode}")));
    }
    let dry_run = form.dry_run.unwrap_or(false);
    let path = dir.join(doc_file);
    let next = if mode == "append" {
        let raw = fs::read_to_string(&path)
            .await
            .unwrap_or_else(|_| format!("# {} {}\n", req.id, doc_file));
        format!("{}\n\n{}\n", raw.trim_end(), content.trim())
    } else {
        ensure_doc_heading(&req.id, doc_file, &content)
    };
    if !dry_run {
        atomic_write_text(&path, &next).await?;
    }
    Ok(json!({
        "ok": true,
        "dryRun": dry_run,
        "reqId": req.id,
        "docType": form.doc_type,
        "mode": mode,
        "file": path.to_string_lossy(),
        "bytes": next.len(),
    }))
}

fn requirement_api_schema() -> Value {
    json!({
        "version": 1,
        "flow": ["edit-plan", "context", "edit", "validate"],
        "statusValues": REQ_STATUSES,
        "categoryValues": REQ_CATEGORIES,
        "tokens": requirement_token_specs_json(),
        "intents": [
            {"intent": "overview", "readTokens": intent_read_tokens("overview"), "writeTokens": ["req.memory", "req.notes"]},
            {"intent": "status", "readTokens": intent_read_tokens("status"), "writeTokens": ["req.state", "req.notes"]},
            {"intent": "progress", "readTokens": intent_read_tokens("progress"), "writeTokens": ["req.notes", "req.memory"]},
            {"intent": "branch", "readTokens": intent_read_tokens("branch"), "writeTokens": ["req.branchScope", "req.branch"]},
            {"intent": "self-test", "readTokens": intent_read_tokens("self-test"), "writeTokens": ["req.test", "req.notes", "req.memory"]},
            {"intent": "release-check", "readTokens": intent_read_tokens("release-check"), "writeTokens": ["req.configChanges", "req.test", "req.impact", "req.review", "req.notes"]},
            {"intent": "design", "readTokens": intent_read_tokens("design"), "writeTokens": ["req.background", "req.impact", "req.memory"]},
            {"intent": "config", "readTokens": intent_read_tokens("config"), "writeTokens": ["req.configChanges", "req.impact"]},
            {"intent": "review", "readTokens": intent_read_tokens("review"), "writeTokens": ["req.review", "req.notes"]}
        ],
        "operations": [
            {"operation": "setStatus", "writes": ["req.state"], "required": ["reqId", "status"], "optional": ["note", "dryRun"]},
            {"operation": "setCategory", "writes": ["req.state"], "required": ["reqId", "category"], "optional": ["dryRun"]},
            {"operation": "patchMeta", "writes": ["req.meta"], "required": ["reqId", "fields"], "allowedFields": ["title", "project", "owner", "startDate", "planRelease", "ones"]},
            {"operation": "appendNote", "writes": ["req.notes"], "required": ["reqId", "text"], "optional": ["title", "sessionId", "dryRun"]},
            {"operation": "writeDoc", "writes": ["token/docType"], "required": ["reqId", "token or docType", "content"], "optional": ["mode=replace|append", "dryRun"]},
            {"operation": "upsertSection", "writes": ["token/docType"], "required": ["reqId", "token or docType", "heading", "content"], "optional": ["dryRun"]}
        ],
        "rules": [
            "Agent should call edit-plan before selecting files for non-trivial requirement edits.",
            "Agent should call context with an intent and token budget instead of reading large files directly.",
            "state.json is the source of truth for status/category; do not direct-edit it.",
            "Use appendNote for progress logs; avoid replacing notes.md.",
            "Use branches.json as machine-readable branch scope; branch.md is human-readable narrative."
        ]
    })
}

fn requirement_token_specs_json() -> Vec<Value> {
    vec![
        token_spec(
            "req.meta",
            "meta.md",
            None,
            "stable identity fields and human summary",
            true,
            vec!["patchMeta"],
        ),
        token_spec(
            "req.state",
            STATE_FILE,
            None,
            "status/category and transition history; state source of truth",
            true,
            vec!["setStatus", "setCategory"],
        ),
        token_spec(
            "req.background",
            "background.md",
            Some("background"),
            "goal, background, scope, key decisions",
            true,
            vec!["writeDoc", "upsertSection"],
        ),
        token_spec(
            "req.memory",
            "memory.md",
            Some("memory"),
            "short agent-facing lifecycle memory and compressed context",
            true,
            vec!["writeDoc", "upsertSection"],
        ),
        token_spec(
            "req.branch",
            "branch.md",
            Some("branch"),
            "human-readable branch and merge narrative",
            true,
            vec!["writeDoc", "upsertSection"],
        ),
        token_spec(
            "req.branchScope",
            BRANCH_SCOPE_FILE,
            None,
            "machine-readable repo/branch scope for diff/deploy automation",
            true,
            vec![],
        ),
        token_spec(
            "req.configChanges",
            "config-changes.md",
            Some("config-changes"),
            "DB/Apollo/Nacos/RocketMQ/config changes",
            true,
            vec!["writeDoc", "upsertSection"],
        ),
        token_spec(
            "req.impact",
            "impact.md",
            Some("impact"),
            "impact assessment, core-link risk, rollback plan",
            true,
            vec!["writeDoc", "upsertSection"],
        ),
        token_spec(
            "req.test",
            "test.md",
            Some("test"),
            "test scenarios, self-test/UAT evidence and confidence",
            true,
            vec!["writeDoc", "upsertSection"],
        ),
        token_spec(
            "req.notes",
            "notes.md",
            Some("notes"),
            "append-only progress, decisions and pitfalls",
            false,
            vec!["appendNote", "writeDoc", "upsertSection"],
        ),
        token_spec(
            "req.review",
            "review.md",
            Some("review"),
            "pre-release review summary",
            false,
            vec!["writeDoc", "upsertSection"],
        ),
        token_spec(
            "req.alignment",
            "alignment.md",
            Some("alignment"),
            "requirement alignment notes and PRD interpretation",
            false,
            vec!["writeDoc", "upsertSection"],
        ),
        token_spec(
            "req.prd",
            "prd.md",
            Some("prd"),
            "source PRD or product notes",
            false,
            vec!["writeDoc", "upsertSection"],
        ),
        token_spec(
            "req.codeReview",
            CODE_REVIEW_FILE,
            None,
            "large generated code-review artifact; read only by explicit intent",
            false,
            vec![],
        ),
    ]
}

fn token_spec(
    token: &str,
    file: &str,
    doc_type: Option<&str>,
    role: &str,
    standard: bool,
    write_ops: Vec<&str>,
) -> Value {
    json!({
        "token": token,
        "file": file,
        "docType": doc_type,
        "role": role,
        "standard": standard,
        "writeOps": write_ops,
    })
}

fn normalize_requirement_intent(raw: Option<&str>) -> String {
    let s = raw.unwrap_or("overview").trim().to_lowercase();
    if s.is_empty() || s == "default" {
        "overview".into()
    } else if s.contains("状态") || s.contains("status") || s.contains("state") {
        "status".into()
    } else if s.contains("自测")
        || s.contains("测试")
        || s.contains("test")
        || s.contains("evidence")
    {
        "self-test".into()
    } else if s.contains("上线") || s.contains("release") || s.contains("发布") {
        "release-check".into()
    } else if s.contains("分支") || s.contains("branch") || s.contains("diff") {
        "branch".into()
    } else if s.contains("配置")
        || s.contains("config")
        || s.contains("apollo")
        || s.contains("nacos")
        || s.contains("db")
    {
        "config".into()
    } else if s.contains("影响")
        || s.contains("方案")
        || s.contains("design")
        || s.contains("impact")
    {
        "design".into()
    } else if s.contains("进展") || s.contains("note") || s.contains("progress") {
        "progress".into()
    } else if s.contains("review") || s.contains("cr") || s.contains("代码审查") {
        "review".into()
    } else {
        s
    }
}

fn intent_read_tokens(intent: &str) -> Vec<&'static str> {
    match intent {
        "status" => vec!["req.meta", "req.state", "req.notes"],
        "progress" => vec!["req.meta", "req.memory", "req.notes"],
        "branch" => vec!["req.meta", "req.branchScope", "req.branch"],
        "self-test" => vec![
            "req.meta",
            "req.memory",
            "req.branch",
            "req.test",
            "req.notes",
        ],
        "release-check" => vec![
            "req.meta",
            "req.state",
            "req.branchScope",
            "req.branch",
            "req.configChanges",
            "req.test",
            "req.impact",
            "req.review",
        ],
        "config" => vec!["req.meta", "req.configChanges", "req.impact", "req.notes"],
        "design" => vec!["req.meta", "req.background", "req.impact", "req.memory"],
        "review" => vec![
            "req.meta",
            "req.branchScope",
            "req.review",
            "req.codeReview",
        ],
        _ => vec![
            "req.meta",
            "req.state",
            "req.memory",
            "req.background",
            "req.branch",
            "req.test",
            "req.notes",
        ],
    }
}

fn intent_write_tokens(intent: &str) -> Vec<&'static str> {
    match intent {
        "status" => vec!["req.state", "req.notes"],
        "progress" => vec!["req.notes", "req.memory"],
        "branch" => vec!["req.branchScope", "req.branch"],
        "self-test" => vec!["req.test", "req.notes", "req.memory"],
        "release-check" => vec![
            "req.configChanges",
            "req.test",
            "req.impact",
            "req.review",
            "req.notes",
        ],
        "config" => vec!["req.configChanges", "req.impact"],
        "design" => vec!["req.background", "req.impact", "req.memory"],
        "review" => vec!["req.review", "req.notes"],
        _ => vec!["req.memory", "req.notes"],
    }
}

fn parse_token_list(raw: &str) -> Vec<&'static str> {
    raw.split([',', '，', ' '])
        .filter_map(canonical_requirement_token)
        .collect()
}

fn canonical_requirement_token(raw: &str) -> Option<&'static str> {
    let s = raw
        .trim()
        .trim_start_matches("req.")
        .trim_end_matches(".md")
        .replace(['-', '_'], "")
        .to_lowercase();
    match s.as_str() {
        "meta" => Some("req.meta"),
        "state" | "statejson" => Some("req.state"),
        "background" => Some("req.background"),
        "memory" => Some("req.memory"),
        "branch" => Some("req.branch"),
        "branchscope" | "branches" | "branchesjson" => Some("req.branchScope"),
        "config" | "configchanges" => Some("req.configChanges"),
        "impact" => Some("req.impact"),
        "test" => Some("req.test"),
        "notes" | "note" => Some("req.notes"),
        "review" => Some("req.review"),
        "alignment" => Some("req.alignment"),
        "prd" => Some("req.prd"),
        "codereview" | "codereviewjson" => Some("req.codeReview"),
        _ => None,
    }
}

fn requirement_token_file(token: &str) -> Option<&'static str> {
    match canonical_requirement_token(token)? {
        "req.meta" => Some("meta.md"),
        "req.state" => Some(STATE_FILE),
        "req.background" => Some("background.md"),
        "req.memory" => Some("memory.md"),
        "req.branch" => Some("branch.md"),
        "req.branchScope" => Some(BRANCH_SCOPE_FILE),
        "req.configChanges" => Some("config-changes.md"),
        "req.impact" => Some("impact.md"),
        "req.test" => Some("test.md"),
        "req.notes" => Some("notes.md"),
        "req.review" => Some("review.md"),
        "req.alignment" => Some("alignment.md"),
        "req.prd" => Some("prd.md"),
        "req.codeReview" => Some(CODE_REVIEW_FILE),
        _ => None,
    }
}

fn requirement_doc_type_for_token(token: &str) -> Option<&'static str> {
    match canonical_requirement_token(token)? {
        "req.background" => Some("background"),
        "req.memory" => Some("memory"),
        "req.branch" => Some("branch"),
        "req.configChanges" => Some("config-changes"),
        "req.impact" => Some("impact"),
        "req.test" => Some("test"),
        "req.notes" => Some("notes"),
        "req.review" => Some("review"),
        "req.alignment" => Some("alignment"),
        "req.prd" => Some("prd"),
        _ => None,
    }
}

fn requirement_token_info(req: &Requirement, token: &str) -> ApiResult<Value> {
    let canonical = canonical_requirement_token(token)
        .ok_or_else(|| ApiError::bad_request(format!("unknown requirement token: {token}")))?;
    let file = requirement_token_file(canonical).unwrap_or_default();
    let dir = req_dir_path(req)?;
    let path = dir.join(file);
    let bytes = path.metadata().map(|m| m.len()).unwrap_or(0);
    Ok(json!({
        "token": canonical,
        "file": file,
        "docType": requirement_doc_type_for_token(canonical),
        "path": path.to_string_lossy(),
        "exists": path.is_file(),
        "bytes": bytes,
    }))
}

fn build_requirement_edit_plan(req: &Requirement, intent: &str) -> Value {
    let read: Vec<Value> = intent_read_tokens(intent)
        .into_iter()
        .filter_map(|t| requirement_token_info(req, t).ok())
        .collect();
    let write: Vec<Value> = intent_write_tokens(intent)
        .into_iter()
        .filter_map(|t| requirement_token_info(req, t).ok())
        .collect();
    json!({
        "ok": true,
        "reqId": req.id,
        "title": req.title,
        "status": req.status,
        "intent": intent,
        "read": read,
        "write": write,
        "preferredFlow": [
            format!("GET /api/requirement/context?id={}&intent={}&budget=2000", req.id, intent),
            "POST /api/requirement/edit",
            "POST /api/requirement/validate"
        ],
        "writeExamples": {
            "appendNote": {"operation": "appendNote", "reqId": req.id, "title": "进展", "text": "..."},
            "upsertTestSection": {"operation": "upsertSection", "reqId": req.id, "token": "req.test", "heading": "自测证据", "content": "- ..."},
            "setStatus": {"operation": "setStatus", "reqId": req.id, "status": "自测中", "note": "..."}
        },
        "rules": [
            "Read only the tokens listed here unless the task explicitly needs more.",
            "Prefer /api/requirement/edit over direct file edits.",
            "Use appendNote for progress; use upsertSection for targeted document updates.",
            "Run validate after writes."
        ]
    })
}

async fn build_requirement_context(
    req: &Requirement,
    intent: &str,
    tokens: Vec<&'static str>,
    budget: usize,
) -> ApiResult<Value> {
    let dir = req_dir_path(req)?;
    let mut rows = Vec::new();
    let mut remaining = budget;
    let total = tokens.len().max(1);
    for (idx, token) in tokens.into_iter().enumerate() {
        let canonical = canonical_requirement_token(token).unwrap_or(token);
        let Some(file) = requirement_token_file(canonical) else {
            continue;
        };
        let path = dir.join(file);
        let exists = path.is_file();
        let bytes = path.metadata().map(|m| m.len()).unwrap_or(0);
        let slots_left = total.saturating_sub(idx).max(1);
        let per_file_budget = (remaining / slots_left).clamp(300, 3_000);
        let (content, truncated, chars) = if exists && remaining > 0 {
            let raw = fs::read_to_string(&path).await.unwrap_or_default();
            let (excerpt, truncated) = truncate_chars(&raw, per_file_budget);
            let chars = excerpt.chars().count();
            remaining = remaining.saturating_sub(chars);
            (excerpt, truncated, chars)
        } else {
            (String::new(), false, 0)
        };
        rows.push(json!({
            "token": canonical,
            "file": file,
            "docType": requirement_doc_type_for_token(canonical),
            "path": path.to_string_lossy(),
            "exists": exists,
            "bytes": bytes,
            "contentChars": chars,
            "truncated": truncated,
            "content": content,
        }));
    }
    Ok(json!({
        "ok": true,
        "reqId": req.id,
        "title": req.title,
        "status": req.status,
        "project": req.project,
        "intent": intent,
        "budget": budget,
        "remainingBudget": remaining,
        "tokens": rows,
        "editPlanUrl": format!("/api/requirement/edit-plan?id={}&intent={}", req.id, intent),
    }))
}

fn truncate_chars(raw: &str, max_chars: usize) -> (String, bool) {
    let count = raw.chars().count();
    if count <= max_chars {
        return (raw.to_string(), false);
    }
    let excerpt = raw.chars().take(max_chars).collect::<String>();
    (
        format!(
            "{}\n…[truncated; {} chars total]",
            excerpt.trim_end(),
            count
        ),
        true,
    )
}

async fn apply_requirement_edit(state: &AppState, form: RequirementEditForm) -> ApiResult<Value> {
    let op = form.operation.trim();
    match op {
        "setStatus" | "status" => {
            let status = clean_required_opt(form.status.as_deref(), "status")?;
            update_requirement(
                state,
                RequirementPatchForm {
                    req_id: form.req_id,
                    title: None,
                    project: None,
                    projects: None,
                    status: Some(status),
                    category: None,
                    owner: None,
                    start_date: None,
                    plan_release: None,
                    ones: None,
                    note: form.note,
                    dry_run: form.dry_run,
                },
            )
            .await
        }
        "setCategory" | "category" => {
            let category = clean_required_opt(form.category.as_deref(), "category")?;
            update_requirement(
                state,
                RequirementPatchForm {
                    req_id: form.req_id,
                    title: None,
                    project: None,
                    projects: None,
                    status: None,
                    category: Some(category),
                    owner: None,
                    start_date: None,
                    plan_release: None,
                    ones: None,
                    note: None,
                    dry_run: form.dry_run,
                },
            )
            .await
        }
        "patchMeta" | "meta" => {
            let fields = form.fields.unwrap_or_default();
            let patch = RequirementPatchForm {
                req_id: form.req_id,
                title: field_value(&fields, &["title"]),
                project: field_value(&fields, &["project"]),
                projects: None,
                status: None,
                category: None,
                owner: field_value(&fields, &["owner"]),
                start_date: field_value(&fields, &["startDate", "start-date"]),
                plan_release: field_value(&fields, &["planRelease", "plan-release"]),
                ones: field_value(&fields, &["ones"]),
                note: None,
                dry_run: form.dry_run,
            };
            if patch.title.is_none()
                && patch.project.is_none()
                && patch.owner.is_none()
                && patch.start_date.is_none()
                && patch.plan_release.is_none()
                && patch.ones.is_none()
            {
                return Err(ApiError::bad_request("patchMeta has no supported fields"));
            }
            update_requirement(state, patch).await
        }
        "appendNote" | "appendNotes" | "note" => {
            let text = form.text.or(form.content).unwrap_or_default();
            append_requirement_note(
                state,
                RequirementNoteForm {
                    req_id: form.req_id,
                    text,
                    title: form.title,
                    session_id: form.session_id,
                    dry_run: form.dry_run,
                },
            )
            .await
        }
        "writeDoc" | "replaceDoc" | "appendDoc" | "doc" => {
            let doc_type = resolve_doc_type(form.doc_type.as_deref(), form.token.as_deref())?;
            let mode = if op == "appendDoc" {
                Some("append".to_string())
            } else if op == "replaceDoc" {
                Some("replace".to_string())
            } else {
                form.mode
            };
            write_requirement_doc(
                state,
                RequirementDocForm {
                    req_id: form.req_id,
                    doc_type,
                    content: form.content.or(form.text).unwrap_or_default(),
                    mode,
                    dry_run: form.dry_run,
                },
            )
            .await
        }
        "upsertSection" | "section" => upsert_requirement_section(state, form).await,
        other => Err(ApiError::bad_request(format!(
            "unsupported requirement edit operation: {other}"
        ))),
    }
}

fn field_value(fields: &HashMap<String, String>, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| fields.get(*key))
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

fn resolve_doc_type(doc_type: Option<&str>, token: Option<&str>) -> ApiResult<String> {
    if let Some(doc_type) = clean_optional(doc_type) {
        requirement_doc_file(&doc_type)?;
        return Ok(doc_type);
    }
    let token = token.ok_or_else(|| ApiError::bad_request("missing token or docType"))?;
    requirement_doc_type_for_token(token)
        .map(str::to_string)
        .ok_or_else(|| {
            ApiError::bad_request(format!("token is not a writable markdown doc: {token}"))
        })
}

async fn upsert_requirement_section(
    state: &AppState,
    form: RequirementEditForm,
) -> ApiResult<Value> {
    let req = get_real_requirement(state, &form.req_id).await?;
    let dir = req_dir_path(&req)?;
    ensure_requirement_dir_writable(state, &dir).await?;
    let doc_type = resolve_doc_type(form.doc_type.as_deref(), form.token.as_deref())?;
    let doc_file = requirement_doc_file(&doc_type)?;
    let heading = clean_required_opt(form.heading.as_deref(), "heading")?;
    let merged_content = form.content.or(form.text);
    let content = clean_required_opt(merged_content.as_deref(), "content")?;
    ensure_text_size(&content, "content")?;
    let dry_run = form.dry_run.unwrap_or(false);
    let path = dir.join(doc_file);
    let raw = fs::read_to_string(&path)
        .await
        .unwrap_or_else(|_| format!("# {} {}\n", req.id, doc_file));
    let next = upsert_markdown_section(&raw, &heading, &content);
    if !dry_run {
        atomic_write_text(&path, &next).await?;
    }
    let validation = if dry_run {
        json!({ "ok": true, "dryRun": true, "problems": [], "warnings": [] })
    } else {
        let refreshed = get_real_requirement(state, &req.id).await?;
        validate_requirement(state, &refreshed).await?
    };
    Ok(json!({
        "ok": true,
        "dryRun": dry_run,
        "reqId": req.id,
        "operation": "upsertSection",
        "docType": doc_type,
        "heading": heading,
        "file": path.to_string_lossy(),
        "bytes": next.len(),
        "validation": validation,
    }))
}

fn upsert_markdown_section(raw: &str, heading: &str, content: &str) -> String {
    let target = normalize_heading_text(heading);
    let heading_line = if heading.trim_start().starts_with('#') {
        heading.trim().to_string()
    } else {
        format!("## {}", heading.trim())
    };
    let lines: Vec<&str> = raw.lines().collect();
    let mut start: Option<(usize, usize)> = None;
    for (idx, line) in lines.iter().enumerate() {
        if let Some((level, text)) = parse_markdown_heading(line) {
            if normalize_heading_text(&text) == target {
                start = Some((idx, level));
                break;
            }
        }
    }
    let new_block = format!("{}\n{}", heading_line, content.trim());
    if let Some((start_idx, level)) = start {
        let mut end_idx = lines.len();
        for (idx, line) in lines.iter().enumerate().skip(start_idx + 1) {
            if let Some((next_level, _)) = parse_markdown_heading(line) {
                if next_level <= level {
                    end_idx = idx;
                    break;
                }
            }
        }
        let mut out = Vec::new();
        out.extend(lines[..start_idx].iter().map(|s| s.to_string()));
        out.push(new_block);
        out.extend(lines[end_idx..].iter().map(|s| s.to_string()));
        format!("{}\n", out.join("\n").trim_end())
    } else {
        format!("{}\n\n{}\n", raw.trim_end(), new_block)
    }
}

fn parse_markdown_heading(line: &str) -> Option<(usize, String)> {
    let trimmed = line.trim_start();
    let level = trimmed.chars().take_while(|c| *c == '#').count();
    if level == 0
        || level > 6
        || !trimmed
            .chars()
            .nth(level)
            .map(|c| c.is_whitespace())
            .unwrap_or(false)
    {
        return None;
    }
    Some((
        level,
        trimmed[level..].trim().trim_matches('#').trim().to_string(),
    ))
}

fn normalize_heading_text(value: &str) -> String {
    value
        .trim()
        .trim_start_matches('#')
        .trim()
        .trim_matches('#')
        .trim()
        .to_lowercase()
}

async fn validate_requirement(state: &AppState, req: &Requirement) -> ApiResult<Value> {
    let dir = req_dir_path(req)?;
    ensure_requirement_dir_writable(state, &dir).await?;
    let mut problems = Vec::<String>::new();
    let mut warnings = Vec::<String>::new();
    let mut files = HashMap::<String, bool>::new();
    for file in [
        "meta.md",
        "background.md",
        "memory.md",
        "branch.md",
        "config-changes.md",
        "impact.md",
        "test.md",
        "notes.md",
    ] {
        let exists = dir.join(file).is_file();
        files.insert(file.to_string(), exists);
        if file == "meta.md" && !exists {
            problems.push("missing meta.md".into());
        } else if file != "meta.md" && !exists {
            warnings.push(format!("missing {file}"));
        }
    }
    let meta_path = dir.join("meta.md");
    let raw = fs::read_to_string(&meta_path).await.unwrap_or_default();
    let fm = parse_frontmatter(&raw);
    match fm.fields.get("req-id") {
        Some(id) if id == &req.id => {}
        Some(id) => problems.push(format!("meta req-id mismatch: {id} != {}", req.id)),
        None => problems.push("meta missing req-id".into()),
    }
    if fm
        .fields
        .get("title")
        .map(|v| v.trim().is_empty())
        .unwrap_or(true)
    {
        problems.push("meta missing title".into());
    }
    if let Some(status) = fm.fields.get("status") {
        if normalize_status(Some(status)).is_none() {
            problems.push(format!("invalid meta status: {status}"));
        }
    } else if req.status.trim().is_empty() {
        problems.push("missing status".into());
    }
    if let Some(category) = fm.fields.get("category") {
        if normalize_category(Some(category)).is_none() {
            problems.push(format!("invalid meta category: {category}"));
        }
    }
    if let Some(state_json) = read_requirement_state(&dir).await? {
        if let Some(status) = state_json.get("status").and_then(Value::as_str) {
            if normalize_status(Some(&status.to_string())).is_none() {
                problems.push(format!("invalid state status: {status}"));
            }
        }
        if let Some(category) = state_json.get("category").and_then(Value::as_str) {
            if normalize_category(Some(&category.to_string())).is_none() {
                problems.push(format!("invalid state category: {category}"));
            }
        }
    }
    let branches_path = dir.join(BRANCH_SCOPE_FILE);
    if branches_path.is_file() && read_branch_scope(&dir).await?.is_none() {
        warnings.push("branches.json exists but has no valid repos".into());
    }
    Ok(json!({
        "ok": problems.is_empty(),
        "reqId": req.id,
        "reqDir": dir.to_string_lossy(),
        "problems": problems,
        "warnings": warnings,
        "files": files,
    }))
}

fn req_dir_path(req: &Requirement) -> ApiResult<PathBuf> {
    req.req_dir
        .as_deref()
        .filter(|v| !v.trim().is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| {
            ApiError::bad_request(format!("requirement has no writable dir: {}", req.id))
        })
}

async fn resolve_create_req_root(state: &AppState, requested: Option<&str>) -> ApiResult<PathBuf> {
    let roots = writable_req_roots(state).await?;
    if roots.is_empty() {
        return Err(ApiError::bad_request(
            "no requirementScanRoots configured; set /settings requirement scan roots first",
        ));
    }
    if let Some(raw) = clean_optional(requested) {
        let requested_path = normalize_user_path(&raw);
        for root in &roots {
            if same_or_child_path(&requested_path, root) {
                return Ok(root.clone());
            }
            if let Some(parent) = root.parent().and_then(|p| p.parent()) {
                if path_eq(&requested_path, parent) {
                    return Ok(root.clone());
                }
            }
        }
        return Err(ApiError::bad_request(format!(
            "root is not under configured requirementScanRoots: {raw}"
        )));
    }
    Ok(roots[0].clone())
}

async fn writable_req_roots(state: &AppState) -> ApiResult<Vec<PathBuf>> {
    let cfg = read_config(state).await?;
    let roots = normalize_scan_roots(cfg.requirement_scan_roots);
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for root in roots {
        let root_path = PathBuf::from(root);
        let mut candidates = Vec::new();
        if root_path.file_name().and_then(|v| v.to_str()) == Some("req") {
            candidates.push(root_path.clone());
        } else {
            candidates.push(root_path.join(".agents/req"));
            candidates.push(root_path.join("req"));
        }
        for candidate in candidates {
            let key = normalize_path_string(&candidate);
            if seen.insert(key) {
                out.push(candidate);
            }
        }
    }
    Ok(out)
}

async fn ensure_requirement_dir_writable(state: &AppState, dir: &Path) -> ApiResult<()> {
    ensure_path_inside_req_roots(state, dir).await
}

async fn ensure_path_inside_req_roots(state: &AppState, path: &Path) -> ApiResult<()> {
    let roots = writable_req_roots(state).await?;
    if roots.iter().any(|root| same_or_child_path(path, root)) {
        Ok(())
    } else {
        Err(ApiError::bad_request(format!(
            "path is outside configured requirement roots: {}",
            path.to_string_lossy()
        )))
    }
}

fn same_or_child_path(path: &Path, root: &Path) -> bool {
    normalize_path_string(path) == normalize_path_string(root)
        || normalize_path_string(path).starts_with(&(normalize_path_string(root) + "/"))
}

fn path_eq(a: &Path, b: &Path) -> bool {
    normalize_path_string(a) == normalize_path_string(b)
}

fn normalize_path_string(path: &Path) -> String {
    let abs = if path.is_absolute() {
        path.to_path_buf()
    } else {
        env::current_dir().unwrap_or_default().join(path)
    };
    let mut parts = Vec::new();
    for comp in abs.components() {
        match comp {
            std::path::Component::ParentDir => {
                parts.pop();
            }
            std::path::Component::CurDir => {}
            _ => parts.push(comp.as_os_str().to_string_lossy().to_string()),
        }
    }
    if parts.is_empty() {
        "/".into()
    } else if parts.first().map(|s| s.as_str()) == Some("/") {
        format!("/{}", parts[1..].join("/"))
    } else {
        parts.join("/")
    }
}

fn normalize_user_path(raw: &str) -> PathBuf {
    let trimmed = raw.trim();
    if trimmed == "~" {
        home_dir().unwrap_or_default()
    } else if let Some(rest) = trimmed.strip_prefix("~/") {
        home_dir().unwrap_or_default().join(rest)
    } else {
        let path = PathBuf::from(trimmed);
        if path.is_absolute() {
            path
        } else {
            env::current_dir().unwrap_or_default().join(path)
        }
    }
}

fn ensure_req_id(value: &str) -> ApiResult<String> {
    let v = value.trim();
    if v == DEFAULT_REQ_ID || v.len() < 2 || v.len() > 128 {
        return Err(ApiError::bad_request("invalid reqId length"));
    }
    if !v.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        || v.starts_with('-')
        || v.ends_with('-')
        || v.contains("--")
    {
        return Err(ApiError::bad_request(
            "reqId must use ASCII letters, numbers and single hyphens only",
        ));
    }
    Ok(v.to_string())
}

/// Parse a reqId template containing a single `{seq}` placeholder into
/// `(prefix, suffix)`. `WMS-{seq}-demo` -> `("WMS", "-demo")`.
fn split_seq_template(template: &str) -> ApiResult<(String, String)> {
    let count = template.matches("{seq}").count();
    if count == 0 {
        return Err(ApiError::bad_request("reqId template must contain {seq}"));
    }
    if count > 1 {
        return Err(ApiError::bad_request(
            "reqId template may contain at most one {seq}",
        ));
    }
    let idx = template.find("{seq}").expect("checked count above");
    let prefix = template[..idx].trim_end_matches('-').to_string();
    let suffix = template[idx + 5..].to_string();
    if prefix.is_empty() {
        return Err(ApiError::bad_request(
            "reqId template must have a non-empty prefix before {seq}",
        ));
    }
    if !prefix
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-')
    {
        return Err(ApiError::bad_request(
            "reqId prefix must be ASCII alphanumeric or hyphen",
        ));
    }
    Ok((prefix, suffix))
}

/// Format the final reqId from prefix, sequence (zero-padded to 3 digits) and
/// suffix. `("WMS", 43, "-demo")` -> `WMS-043-demo`.
fn format_seq_id(prefix: &str, seq: u64, suffix: &str) -> String {
    format!("{prefix}-{:03}{suffix}", seq)
}

/// Pure helper: compute the next sequence number for `prefix` from a list of
/// existing requirement ids. Sub-requirements sharing a number (e.g.
/// `WMS-003-*`) and gaps are handled by taking `max + 1`. `floor` forces a
/// minimum (used when retrying after a collision).
fn compute_next_seq_from_ids(ids: &[String], prefix: &str, floor: Option<u64>) -> u64 {
    let re = Regex::new(&format!("^{}-(\\d+)(-|$)", regex::escape(prefix))).expect("valid regex");
    let mut max_seq: u64 = 0;
    for id in ids {
        if let Some(caps) = re.captures(id) {
            if let Ok(n) = caps[1].parse::<u64>() {
                if n > max_seq {
                    max_seq = n;
                }
            }
        }
    }
    let mut next = max_seq + 1;
    if let Some(f) = floor {
        next = next.max(f);
    }
    next
}

/// Allocate the next sequence number for `prefix` by scanning existing
/// requirements on disk.
async fn allocate_next_seq(state: &AppState, prefix: &str, floor: Option<u64>) -> ApiResult<u64> {
    let reqs = scan_hermes_requirements(state).await?;
    let ids: Vec<String> = reqs.iter().map(|r| r.id.clone()).collect();
    Ok(compute_next_seq_from_ids(&ids, prefix, floor))
}

/// Compute the target directory for a fully-resolved reqId, resolving parent
/// requirement or group path from the create form.
async fn compute_create_target_dir(
    state: &AppState,
    base: &Path,
    req_id: &str,
    form: &RequirementCreateForm,
) -> ApiResult<PathBuf> {
    if let Some(parent_id) = clean_optional(form.parent_req_id.as_deref()) {
        let parent = get_real_requirement(state, &parent_id).await?;
        let parent_dir = req_dir_path(&parent)?;
        ensure_requirement_dir_writable(state, &parent_dir).await?;
        Ok(parent_dir.join(req_id))
    } else {
        let mut dir = base.to_path_buf();
        for segment in form.group_path.as_deref().unwrap_or_default() {
            dir = dir.join(ensure_safe_segment(segment, "groupPath")?);
        }
        Ok(dir.join(req_id))
    }
}

/// Resolve the final reqId and target directory for a create request.
///
/// When `template` contains `{seq}`, the next sequence number is allocated
/// from existing requirements and (for non-dry-run) the target directory is
/// atomically reserved with `fs::create_dir`, retrying on collision. When
/// `template` has no `{seq}`, it is validated as-is and the target directory
/// is checked for prior existence.
async fn resolve_req_id_and_target_dir(
    state: &AppState,
    base: &Path,
    template: &str,
    form: &RequirementCreateForm,
    dry_run: bool,
) -> ApiResult<(String, PathBuf)> {
    if !template.contains("{seq}") {
        let req_id = ensure_req_id(template)?;
        let target_dir = compute_create_target_dir(state, base, &req_id, form).await?;
        ensure_path_inside_req_roots(state, &target_dir).await?;
        if target_dir.exists() {
            return Err(ApiError::bad_request(format!(
                "requirement directory already exists: {}",
                target_dir.to_string_lossy()
            )));
        }
        return Ok((req_id, target_dir));
    }

    let (prefix, suffix) = split_seq_template(template)?;
    let max_retries: u32 = 5;
    let mut floor: Option<u64> = None;
    for attempt in 0..=max_retries {
        let seq = allocate_next_seq(state, &prefix, floor).await?;
        let req_id = ensure_req_id(&format_seq_id(&prefix, seq, &suffix))?;
        let target_dir = compute_create_target_dir(state, base, &req_id, form).await?;
        ensure_path_inside_req_roots(state, &target_dir).await?;
        if dry_run {
            return Ok((req_id, target_dir));
        }
        if let Some(parent) = target_dir.parent() {
            fs::create_dir_all(parent).await?;
        }
        match fs::create_dir(&target_dir).await {
            Ok(()) => return Ok((req_id, target_dir)),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                if attempt == max_retries {
                    return Err(ApiError::bad_request(format!(
                        "requirement directory already exists after {max_retries} retries: {}",
                        target_dir.to_string_lossy()
                    )));
                }
                floor = Some(seq + 1);
                continue;
            }
            Err(e) => return Err(anyhow!("create requirement dir failed: {e}").into()),
        }
    }
    unreachable!("retry loop exhausted without returning")
}

fn ensure_safe_segment(value: &str, field: &str) -> ApiResult<String> {
    let v = value.trim();
    if v.is_empty() || v == "." || v == ".." || v.len() > 128 {
        return Err(ApiError::bad_request(format!("invalid {field} segment")));
    }
    if !v
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        || v.contains('/')
        || v.contains('\\')
    {
        return Err(ApiError::bad_request(format!(
            "{field} segment must be ASCII and path-safe"
        )));
    }
    Ok(v.to_string())
}

fn clean_required(value: &str, field: &str) -> ApiResult<String> {
    clean_optional(Some(value)).ok_or_else(|| ApiError::bad_request(format!("missing {field}")))
}

fn clean_required_opt(value: Option<&str>, field: &str) -> ApiResult<String> {
    clean_optional(value).ok_or_else(|| ApiError::bad_request(format!("missing {field}")))
}

fn clean_optional(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_string)
}

fn ensure_date_or_unknown(value: &str, field: &str) -> ApiResult<()> {
    if value == "unknown" || chrono::NaiveDate::parse_from_str(value, "%Y-%m-%d").is_ok() {
        Ok(())
    } else {
        Err(ApiError::bad_request(format!(
            "{field} must be YYYY-MM-DD or unknown"
        )))
    }
}

fn ensure_text_size(value: &str, field: &str) -> ApiResult<()> {
    if value.len() > 300_000 {
        Err(ApiError::bad_request(format!("{field} is too large")))
    } else {
        Ok(())
    }
}

fn today_ymd() -> String {
    chrono::Utc::now()
        .date_naive()
        .format("%Y-%m-%d")
        .to_string()
}

fn normalize_projects(project: Option<&str>, projects: Option<&[String]>) -> Vec<String> {
    let mut values = Vec::new();
    if let Some(project) = clean_optional(project) {
        values.push(project);
    }
    if let Some(projects) = projects {
        values.extend(
            projects
                .iter()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty()),
        );
    }
    if values.is_empty() {
        values.push(DEFAULT_PROJECT_NAME.to_string());
    }
    unique_strings(values)
}

#[allow(clippy::too_many_arguments)]
fn requirement_create_files(
    req_id: &str,
    title: &str,
    status: &str,
    project: &str,
    projects: &[String],
    category: &str,
    owner: &str,
    start_date: &str,
    plan_release: &str,
    ones: &str,
    summary: &str,
    background: Option<&str>,
    notes: Option<&str>,
) -> Vec<(&'static str, String)> {
    let meta = build_meta_doc(
        req_id,
        title,
        status,
        project,
        projects,
        category,
        owner,
        start_date,
        plan_release,
        ones,
        summary,
    );
    vec![
        ("meta.md", meta),
        (
            "background.md",
            background
                .map(str::to_string)
                .unwrap_or_else(|| template_background(req_id)),
        ),
        ("memory.md", template_memory(req_id, title)),
        ("branch.md", template_branch(req_id)),
        ("config-changes.md", template_config_changes(req_id)),
        ("impact.md", template_impact(req_id)),
        ("test.md", template_test(req_id)),
        (
            "notes.md",
            notes
                .map(str::to_string)
                .unwrap_or_else(|| template_notes(req_id)),
        ),
    ]
}

#[allow(clippy::too_many_arguments)]
fn build_meta_doc(
    req_id: &str,
    title: &str,
    status: &str,
    project: &str,
    projects: &[String],
    category: &str,
    owner: &str,
    start_date: &str,
    plan_release: &str,
    ones: &str,
    summary: &str,
) -> String {
    let mut fm = vec![
        format!("req-id: {}", yaml_quote(req_id)),
        format!("title: {}", yaml_quote(title)),
        format!("status: {}", yaml_quote(status)),
        format!("project: {}", yaml_quote(project)),
    ];
    if projects.len() > 1 {
        fm.push(format!("projects: {}", yaml_quote(&projects.join(", "))));
    }
    fm.push(format!("category: {}", yaml_quote(category)));
    fm.push(format!("owner: {}", yaml_quote(owner)));
    fm.push(format!("start-date: {}", yaml_quote(start_date)));
    fm.push(format!("plan-release: {}", yaml_quote(plan_release)));
    if !ones.trim().is_empty() {
        fm.push(format!("ones: {}", yaml_quote(ones)));
    }
    format!(
        "---\n{}\n---\n\n# {} {}\n\n## Summary\n- Title: {}\n- Status: {}\n- Owner: {}\n- Start date: {}\n- Planned release: {}\n- Project: {}\n\n{}\n\n## Scope\n- Include:\n  - 待补充\n- Exclude:\n  - 待补充\n\n## Open Questions\n- 待补充\n",
        fm.join("\n"),
        req_id,
        title,
        title,
        status,
        owner,
        start_date,
        plan_release,
        projects.join(" / "),
        summary.trim()
    )
}

fn template_background(req_id: &str) -> String {
    format!("# {req_id} 需求背景\n\n## 目标\n- 待补充\n\n## 背景\n- 待补充\n\n## 范围\n- 仓库：待补充\n- 分支：待补充\n- 关键改动文件：待补充\n\n## 关键决策\n- 待补充\n")
}

fn template_memory(req_id: &str, title: &str) -> String {
    format!("# {req_id} Memory\n\n## 当前目标\n- {title}\n\n## 当前进展\n- 已创建需求，待补充进展。\n\n## 关键决策\n- 待补充\n\n## 待办 / 风险\n- [ ] 待补充\n")
}

fn template_branch(req_id: &str) -> String {
    format!("# {req_id} Branches\n\n| Item | Value |\n| --- | --- |\n| Source branch | unknown |\n| Target branch | unknown |\n| Project path | unknown |\n| Merge status | 开发中 |\n\n## Commit / Diff Notes\n- 待补充\n")
}

fn template_config_changes(req_id: &str) -> String {
    format!("# {req_id} Config Changes\n\n## DB 变更\n- 暂无\n\n## Apollo / Nacos 变更\n- 暂无\n\n## RocketMQ / Console 变更\n- 暂无\n")
}

fn template_impact(req_id: &str) -> String {
    format!("# {req_id} Impact\n\n## 风险等级\n- 待评估\n\n## 核心链路影响\n- 待补充\n\n## 回滚方案\n- 待补充\n")
}

fn template_test(req_id: &str) -> String {
    format!("# {req_id} Test\n\n## 测试场景清单\n\n| ID | 场景描述 | 触发方式 | 前置条件 | 预期结果 | 证据标准 |\n| --- | --- | --- | --- | --- | --- |\n| S1 | 待补充 | 待补充 | 待补充 | 待补充 | 日志 + DB + 副作用 + 反向检查 |\n\n## 自测记录\n- ⬜ 待执行\n\n## UAT 回归记录\n- ⬜ 待执行\n")
}

fn template_notes(req_id: &str) -> String {
    format!("# {req_id} Notes\n\n## 当前状态\n- 需求已创建。\n\n## 待跟进\n- [ ] 补充需求背景、影响面、分支和测试证据。\n")
}

fn update_meta_summary_line(raw: &str, label: &str, value: &str) -> String {
    let prefix = format!("- {label}:");
    let mut changed = false;
    let lines: Vec<String> = raw
        .split('\n')
        .map(|line| {
            if line.trim_start().starts_with(&prefix) {
                changed = true;
                format!("- {label}: {value}")
            } else {
                line.to_string()
            }
        })
        .collect();
    if changed {
        lines.join("\n")
    } else {
        raw.to_string()
    }
}

fn requirement_doc_file(doc_type: &str) -> ApiResult<&'static str> {
    match doc_type.trim() {
        "background" | "background.md" => Ok("background.md"),
        "memory" | "memory.md" => Ok("memory.md"),
        "branch" | "branch.md" => Ok("branch.md"),
        "config" | "config-changes" | "config-changes.md" => Ok("config-changes.md"),
        "impact" | "impact.md" => Ok("impact.md"),
        "test" | "test.md" => Ok("test.md"),
        "notes" | "notes.md" => Ok("notes.md"),
        "review" | "review.md" => Ok("review.md"),
        "alignment" | "alignment.md" => Ok("alignment.md"),
        "prd" | "prd.md" => Ok("prd.md"),
        other => Err(ApiError::bad_request(format!(
            "unsupported docType: {other}"
        ))),
    }
}

fn ensure_doc_heading(req_id: &str, doc_file: &str, content: &str) -> String {
    let clean = content.trim_start_matches('\u{feff}').trim_start();
    if clean.starts_with('#') {
        format!("{}\n", content.trim_end())
    } else {
        format!("# {} {}\n\n{}\n", req_id, doc_file, content.trim_end())
    }
}

async fn read_requirement_state(dir: &Path) -> Result<Option<Value>> {
    let path = dir.join(STATE_FILE);
    if path.is_file() {
        return Ok(read_json_if_exists(&path).await);
    }
    Ok(None)
}

async fn write_requirement_status(
    req_dir: &str,
    new_status: &str,
    note: Option<&str>,
) -> Result<Value> {
    let dir = PathBuf::from(req_dir);
    let path = dir.join(STATE_FILE);
    let previous = read_requirement_state(&dir)
        .await?
        .unwrap_or_else(|| json!({ "version": 1, "history": [] }));
    let from = previous
        .get("status")
        .and_then(Value::as_str)
        .map(|s| s.to_string());
    let mut history = previous
        .get("history")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if from.as_deref() != Some(new_status) {
        history.push(json!({ "status": new_status, "from": from, "at": now_ms(), "note": note.unwrap_or("") }));
    }
    if history.len() > 50 {
        history = history[history.len() - 50..].to_vec();
    }
    let state = json!({ "version": 1, "status": new_status, "category": previous.get("category").cloned().unwrap_or(Value::Null), "updatedAt": now_ms(), "history": history });
    atomic_write_json(&path, &state).await?;
    Ok(state)
}

async fn write_requirement_category(req_dir: &str, new_category: &str) -> Result<Value> {
    let dir = PathBuf::from(req_dir);
    let path = dir.join(STATE_FILE);
    let previous = read_requirement_state(&dir)
        .await?
        .unwrap_or_else(|| json!({ "version": 1, "status": "开发中", "history": [] }));
    let state = json!({
        "version": 1,
        "status": previous.get("status").and_then(Value::as_str).unwrap_or("开发中"),
        "category": new_category,
        "updatedAt": now_ms(),
        "history": previous.get("history").cloned().unwrap_or_else(|| json!([]))
    });
    atomic_write_json(&path, &state).await?;
    Ok(state)
}

async fn write_requirement_ones(req_dir: &str, ones: &str) -> Result<String> {
    let path = PathBuf::from(req_dir).join("meta.md");
    let raw = fs::read_to_string(&path).await.unwrap_or_default();
    let normalized = raw.replace("\r\n", "\n");
    let value = ones.trim().to_string();
    let next = set_frontmatter_field(&normalized, "ones", &value);
    atomic_write_text(&path, &next).await?;
    Ok(value)
}

fn set_frontmatter_field(raw: &str, key: &str, value: &str) -> String {
    let mut lines: Vec<String> = raw.split('\n').map(|s| s.to_string()).collect();
    if lines.first().map(|s| s.as_str()) != Some("---") {
        let body = raw.trim_start_matches('\n');
        if value.is_empty() {
            return body.to_string();
        }
        return format!("---\n{}: {}\n---\n{}", key, yaml_quote(value), body);
    }
    let end = lines
        .iter()
        .enumerate()
        .skip(1)
        .find(|(_, l)| l.as_str() == "---")
        .map(|(i, _)| i);
    let Some(end) = end else {
        return raw.to_string();
    };
    let mut found = None;
    for i in 1..end {
        if lines[i]
            .split_once(':')
            .map(|(k, _)| k.trim() == key)
            .unwrap_or(false)
        {
            found = Some(i);
            break;
        }
    }
    if let Some(i) = found {
        if value.is_empty() {
            lines.remove(i);
        } else {
            lines[i] = format!("{}: {}", key, yaml_quote(value));
        }
    } else if !value.is_empty() {
        lines.insert(end, format!("{}: {}", key, yaml_quote(value)));
    }
    lines.join("\n")
}

fn yaml_quote(value: &str) -> String {
    if value
        .chars()
        .all(|c| c.is_alphanumeric() || "-_./:#?=&%".contains(c))
    {
        value.to_string()
    } else {
        serde_json::to_string(value).unwrap_or_else(|_| value.to_string())
    }
}

fn requirement_project_root(req: &Requirement) -> Option<PathBuf> {
    let req_dir = PathBuf::from(req.req_dir.as_ref()?);
    for ancestor in req_dir.ancestors() {
        if ancestor.file_name().and_then(|v| v.to_str()) == Some("req") {
            if ancestor
                .parent()
                .and_then(|p| p.file_name())
                .and_then(|v| v.to_str())
                == Some(".agents")
            {
                return ancestor
                    .parent()
                    .and_then(|p| p.parent())
                    .map(Path::to_path_buf);
            }
            return ancestor.parent().map(Path::to_path_buf);
        }
    }
    None
}

async fn write_injection_context(
    state: &AppState,
    req: &Requirement,
    session_id: &str,
) -> Result<PathBuf> {
    let dir = state.data_dir.join(INJECTION_CTX_SUBDIR);
    fs::create_dir_all(&dir).await?;
    let path = dir.join(format!("{}.md", session_id));
    let body = format!(
        "# Agent Panel Requirement Context\n\n- Req ID: {}\n- Title: {}\n- Status: {}\n- Directory: {}\n\n{}\n\nAgent workflow:\n1. For requirement edits, call `GET /api/requirement/edit-plan?id={}&intent=<intent>` before choosing files.\n2. For context, call `GET /api/requirement/context?id={}&intent=<intent>&budget=2000` instead of reading large markdown files directly.\n3. For writes, prefer `POST /api/requirement/edit`, then validate with `POST /api/requirement/validate`.\n4. Only read files directly when the API is unavailable or the task explicitly requires raw content.\n",
        req.id,
        req.title,
        req.status,
        req.req_dir.clone().unwrap_or_default(),
        req.description,
        req.id,
        req.id
    );
    atomic_write_text(&path, &body).await?;
    Ok(path)
}

async fn scan_pi_sessions(state: &AppState, days: Option<i64>) -> Result<Vec<SessionInfo>> {
    let root = state.pi_session_root.as_ref();
    if !root.is_dir() {
        return Ok(Vec::new());
    }
    let cutoff = days.filter(|d| *d > 0).map(|d| now_ms() - d * 86_400_000);
    let mut out = Vec::new();
    for entry in WalkDir::new(root)
        .min_depth(2)
        .max_depth(2)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if !path.is_file() || path.extension().and_then(|s| s.to_str()) != Some("jsonl") {
            continue;
        }
        if let Some(session) = read_pi_session_file(path).await {
            if cutoff
                .map(|c| session.updated >= c || session.created >= c)
                .unwrap_or(true)
            {
                out.push(session);
            }
        }
    }
    out.sort_by(|a, b| b.updated.cmp(&a.updated));
    out.truncate(200);
    Ok(out)
}

async fn read_pi_session_file(path: &Path) -> Option<SessionInfo> {
    let meta = fs::metadata(path).await.ok()?;
    let raw = fs::read_to_string(path).await.ok()?;
    let mut lines = raw.lines().filter(|l| !l.trim().is_empty());
    let header: Value = serde_json::from_str(lines.next()?).ok()?;
    if header.get("type")?.as_str()? != "session" {
        return None;
    }
    let id = header.get("id")?.as_str()?.to_string();
    if Uuid::parse_str(&id).is_err() {
        return None;
    }
    let cwd = header
        .get("cwd")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let created = header
        .get("timestamp")
        .and_then(Value::as_str)
        .and_then(parse_date_ms)
        .unwrap_or_else(|| system_time_to_ms(meta.created().unwrap_or(UNIX_EPOCH)));
    let updated = system_time_to_ms(meta.modified().unwrap_or(UNIX_EPOCH));
    let mut title = String::new();
    let mut model_id: Option<String> = None;
    let mut provider: Option<String> = None;
    let mut thinking_level: Option<String> = None;
    let mut message_count = 0;
    let mut user_message_count = 0;
    let mut assistant_message_count = 0;
    let mut tool_result_count = 0;
    let mut tool_call_count = 0;
    let mut tokens_input = 0;
    let mut tokens_output = 0;
    let mut tokens_reasoning = 0;
    let mut tokens_cache_read = 0;
    let mut tokens_cache_write = 0;
    let mut cost = 0.0;
    for line in lines {
        let entry: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => continue,
        };
        match entry
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default()
        {
            "model_change" => {
                model_id = entry
                    .get("modelId")
                    .and_then(Value::as_str)
                    .map(|s| s.to_string())
                    .or(model_id);
                provider = entry
                    .get("provider")
                    .and_then(Value::as_str)
                    .map(|s| s.to_string())
                    .or(provider);
            }
            "thinking_level_change" => {
                thinking_level = entry
                    .get("thinkingLevel")
                    .and_then(Value::as_str)
                    .map(|s| s.to_string())
                    .or(thinking_level)
            }
            "session_info" => {
                if title.is_empty() {
                    title = entry
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .trim()
                        .chars()
                        .take(200)
                        .collect();
                }
            }
            "message" => {
                message_count += 1;
                let Some(msg) = entry.get("message") else {
                    continue;
                };
                if title.is_empty() {
                    title = text_from_user_message(msg);
                }
                match msg.get("role").and_then(Value::as_str).unwrap_or_default() {
                    "user" => user_message_count += 1,
                    "assistant" => assistant_message_count += 1,
                    "toolResult" => tool_result_count += 1,
                    _ => {}
                }
                provider = msg
                    .get("provider")
                    .and_then(Value::as_str)
                    .map(|s| s.to_string())
                    .or(provider);
                model_id = msg
                    .get("model")
                    .and_then(Value::as_str)
                    .map(|s| s.to_string())
                    .or(model_id);
                if let Some(parts) = msg.get("content").and_then(Value::as_array) {
                    tool_call_count += parts
                        .iter()
                        .filter(|p| p.get("type").and_then(Value::as_str) == Some("toolCall"))
                        .count() as u64;
                }
                if let Some(usage) = msg.get("usage") {
                    tokens_input += usage.get("input").and_then(Value::as_u64).unwrap_or(0);
                    tokens_output += usage.get("output").and_then(Value::as_u64).unwrap_or(0);
                    tokens_reasoning += usage.get("reasoning").and_then(Value::as_u64).unwrap_or(0);
                    tokens_cache_read +=
                        usage.get("cacheRead").and_then(Value::as_u64).unwrap_or(0);
                    tokens_cache_write +=
                        usage.get("cacheWrite").and_then(Value::as_u64).unwrap_or(0);
                    cost += usage
                        .get("cost")
                        .and_then(|c| c.get("total"))
                        .and_then(Value::as_f64)
                        .unwrap_or(0.0);
                }
            }
            _ => {}
        }
    }
    if title.is_empty() {
        title = format!("pi {}", &id[..8]);
    }
    let model = model_id.clone();
    let model_provider = provider.clone();
    Some(SessionInfo {
        id,
        title,
        status: status_from_updated(updated),
        agent: "pi".into(),
        source: "fs".into(),
        path: path.to_string_lossy().to_string(),
        directory: cwd.clone(),
        worktree: derive_worktree(&cwd),
        created,
        updated,
        model_id,
        model_provider,
        model,
        provider,
        tokens_input,
        tokens_output,
        tokens_reasoning,
        tokens_cache_read,
        tokens_cache_write,
        cost,
        message_count,
        user_message_count,
        assistant_message_count,
        tool_result_count,
        tool_call_count,
        thinking_level,
    })
}

fn text_from_user_message(msg: &Value) -> String {
    if msg.get("role").and_then(Value::as_str) != Some("user") {
        return String::new();
    }
    let mut out = String::new();
    if let Some(parts) = msg.get("content").and_then(Value::as_array) {
        for part in parts {
            if part.get("type").and_then(Value::as_str) == Some("text") {
                if let Some(text) = part.get("text").and_then(Value::as_str) {
                    if !out.is_empty() {
                        out.push(' ');
                    }
                    out.push_str(text.trim());
                }
            }
        }
    }
    let compact = out.split_whitespace().collect::<Vec<_>>().join(" ");
    compact.chars().take(200).collect()
}

fn status_from_updated(updated: i64) -> String {
    let age = now_ms() - updated;
    if age < 5 * 60_000 {
        "running".into()
    } else if age < 24 * 60 * 60_000 {
        "idle".into()
    } else {
        "stale".into()
    }
}

fn derive_worktree(cwd: &str) -> String {
    Path::new(cwd)
        .file_name()
        .and_then(|v| v.to_str())
        .unwrap_or(cwd)
        .to_string()
}

#[derive(Debug, Clone)]
struct Frontmatter {
    fields: HashMap<String, String>,
    body: String,
}

fn parse_frontmatter(text: &str) -> Frontmatter {
    let normalized = text.replace("\r\n", "\n");
    if !normalized.starts_with("---\n") && normalized.trim() != "---" {
        return Frontmatter {
            fields: HashMap::new(),
            body: normalized,
        };
    }
    let lines: Vec<&str> = normalized.split('\n').collect();
    let Some(end) = lines
        .iter()
        .enumerate()
        .skip(1)
        .find(|(_, l)| **l == "---")
        .map(|(i, _)| i)
    else {
        return Frontmatter {
            fields: HashMap::new(),
            body: normalized,
        };
    };
    let mut fields = HashMap::new();
    for line in &lines[1..end] {
        if line.trim().is_empty() || line.trim_start().starts_with('#') {
            continue;
        }
        if let Some((k, v)) = line.split_once(':') {
            let value = v.trim().trim_matches('"').trim_matches('\'').to_string();
            fields.insert(k.trim().to_string(), value);
        }
    }
    Frontmatter {
        fields,
        body: lines[end + 1..].join("\n"),
    }
}

fn first_paragraph(body: &str) -> String {
    for paragraph in body.trim_start().split("\n\n") {
        let cleaned = paragraph
            .lines()
            .filter(|l| !l.trim_start().starts_with('#'))
            .collect::<Vec<_>>()
            .join("\n")
            .trim()
            .to_string();
        if !cleaned.is_empty() {
            return cleaned;
        }
    }
    String::new()
}

fn split_list(value: Option<&String>) -> Vec<String> {
    value
        .map(|v| {
            v.split([',', '，'])
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

fn value_to_list(value: Option<&Value>) -> Vec<String> {
    match value {
        Some(Value::Array(arr)) => arr
            .iter()
            .filter_map(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect(),
        Some(Value::String(s)) => s
            .split([',', '，'])
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect(),
        _ => Vec::new(),
    }
}

fn value_to_path(value: Option<&Value>) -> Option<Vec<String>> {
    match value {
        Some(Value::Array(arr)) => Some(
            arr.iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .collect(),
        ),
        Some(Value::String(s)) => Some(
            s.split('/')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .collect(),
        ),
        _ => None,
    }
}

fn unique_strings(values: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for value in values {
        let clean = value.trim().to_string();
        if !clean.is_empty() && seen.insert(clean.clone()) {
            out.push(clean);
        }
    }
    out
}

fn append_group(group: &[String], value: String) -> Vec<String> {
    let mut out = group.to_vec();
    out.push(value);
    out
}

fn normalize_status(value: Option<&String>) -> Option<String> {
    let raw = value?.trim();
    if raw == "待开发" {
        return Some("方案设计".into());
    }
    if REQ_STATUSES.contains(&raw) {
        Some(raw.to_string())
    } else {
        None
    }
}

fn normalize_category(value: Option<&String>) -> Option<String> {
    let raw = value?.trim();
    if REQ_CATEGORIES.contains(&raw) {
        Some(raw.to_string())
    } else {
        None
    }
}

fn ensure_status(value: &str) -> ApiResult<()> {
    if REQ_STATUSES.contains(&value) {
        Ok(())
    } else {
        Err(ApiError::bad_request(format!("invalid status: {value}")))
    }
}

fn ensure_category(value: &str) -> ApiResult<()> {
    if REQ_CATEGORIES.contains(&value) {
        Ok(())
    } else {
        Err(ApiError::bad_request(format!("invalid category: {value}")))
    }
}

fn parse_date_ms(value: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(value)
        .map(|d| d.timestamp_millis())
        .ok()
        .or_else(|| {
            chrono::NaiveDate::parse_from_str(&value.replace('/', "-"), "%Y-%m-%d")
                .ok()
                .and_then(|d| d.and_hms_opt(0, 0, 0))
                .map(|d| d.and_utc().timestamp_millis())
        })
}

async fn read_json_if_exists(path: &Path) -> Option<Value> {
    let raw = fs::read_to_string(path).await.ok()?;
    serde_json::from_str(&raw).ok()
}

fn path_if_exists(path: PathBuf) -> Option<String> {
    if path.exists() {
        Some(path.to_string_lossy().to_string())
    } else {
        None
    }
}

fn parse_ones_ref(raw: &str) -> Option<Value> {
    let value = raw.trim();
    if value.is_empty() {
        return None;
    }
    if value.starts_with("http://") || value.starts_with("https://") {
        let label = Regex::new(r"(?:^|/)issue/([^/?#]+)")
            .ok()
            .and_then(|re| {
                re.captures(value)
                    .and_then(|c| c.get(1).map(|m| m.as_str().to_string()))
            })
            .unwrap_or_else(|| value.rsplit('/').next().unwrap_or(value).to_string());
        Some(json!({ "raw": value, "url": value, "label": label }))
    } else {
        Some(json!({ "raw": value, "url": null, "label": value }))
    }
}

async fn atomic_write_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let text = serde_json::to_string_pretty(value)? + "\n";
    atomic_write_text(path, &text).await
}

async fn atomic_write_text(path: &Path, text: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).await?;
    }
    let tmp = path.with_extension(format!("tmp.{}.{}", std::process::id(), now_ms()));
    fs::write(&tmp, text).await?;
    fs::rename(tmp, path).await?;
    Ok(())
}

fn shell_quote(value: &str) -> String {
    if value
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || "-_./:@".contains(c))
    {
        value.to_string()
    } else {
        format!("'{}'", value.replace('\'', "'\\''"))
    }
}

#[allow(dead_code)]
async fn run_command(program: &str, args: &[&str]) -> Result<String> {
    let output = Command::new(program).args(args).output().await?;
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_seq_template_with_suffix() {
        let (prefix, suffix) = split_seq_template("WMS-{seq}-demo").unwrap();
        assert_eq!(prefix, "WMS");
        assert_eq!(suffix, "-demo");
    }

    #[test]
    fn split_seq_template_trailing() {
        let (prefix, suffix) = split_seq_template("WMS-{seq}").unwrap();
        assert_eq!(prefix, "WMS");
        assert_eq!(suffix, "");
    }

    #[test]
    fn split_seq_template_strips_trailing_hyphen_in_prefix() {
        // "WMS--{seq}" -> prefix trimmed to "WMS"
        let (prefix, _) = split_seq_template("WMS--{seq}").unwrap();
        assert_eq!(prefix, "WMS");
    }

    #[test]
    fn split_seq_template_rejects_missing_placeholder() {
        assert!(split_seq_template("WMS-043").is_err());
    }

    #[test]
    fn split_seq_template_rejects_multiple_placeholders() {
        assert!(split_seq_template("WMS-{seq}-{seq}").is_err());
    }

    #[test]
    fn split_seq_template_rejects_empty_prefix() {
        assert!(split_seq_template("{seq}-demo").is_err());
    }

    #[test]
    fn split_seq_template_rejects_non_ascii_prefix() {
        assert!(split_seq_template("WMS_测试-{seq}").is_err());
    }

    #[test]
    fn format_seq_id_pads_to_three_digits() {
        assert_eq!(format_seq_id("WMS", 43, "-demo"), "WMS-043-demo");
        assert_eq!(format_seq_id("WMS", 1, ""), "WMS-001");
        // 4-digit numbers are not truncated by {:03}
        assert_eq!(format_seq_id("WMS", 1000, "-x"), "WMS-1000-x");
    }

    #[test]
    fn compute_next_seq_ignores_subrequirements_and_gaps() {
        // Existing WMS data: WMS-003-* sub-requirements share 003, 004 is a gap,
        // max is 042 -> next is 043.
        let ids = vec![
            "WMS-001-log".to_string(),
            "WMS-003-a".to_string(),
            "WMS-003-b".to_string(),
            "WMS-003-c".to_string(),
            "WMS-005-x".to_string(),
            "WMS-042-y".to_string(),
            "OTHER-099-z".to_string(), // different prefix, ignored
        ];
        assert_eq!(compute_next_seq_from_ids(&ids, "WMS", None), 43);
    }

    #[test]
    fn compute_next_seq_respects_floor() {
        let ids = vec!["WMS-010-a".to_string()];
        // max + 1 = 11, but floor = 50
        assert_eq!(compute_next_seq_from_ids(&ids, "WMS", Some(50)), 50);
    }

    #[test]
    fn compute_next_seq_starts_at_one_when_no_match() {
        let ids = vec!["OTHER-099".to_string()];
        assert_eq!(compute_next_seq_from_ids(&ids, "WMS", None), 1);
    }

    #[test]
    fn compute_next_seq_ignores_non_numeric_segments() {
        let ids = vec!["WMS-abc".to_string(), "WMS-005".to_string()];
        // WMS-abc does not match \d+, max numeric = 5 -> next 6
        assert_eq!(compute_next_seq_from_ids(&ids, "WMS", None), 6);
    }

    #[test]
    fn compute_next_seq_matches_subrequirement_numbers() {
        // Ensure the regex captures the number even when followed by a hyphen
        // (sub-requirement case like WMS-003-after-picking-batch).
        let ids = vec!["WMS-003-after-picking-batch".to_string()];
        assert_eq!(compute_next_seq_from_ids(&ids, "WMS", None), 4);
    }
}
