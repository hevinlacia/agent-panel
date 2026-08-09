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
    extract::{Path as AxumPath, Query, State},
    http::{HeaderMap, StatusCode, Uri},
    response::{Html, IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use futures_util::{SinkExt, StreamExt};
use regex::Regex;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::{
    fs,
    net::{TcpListener, TcpStream},
    process::Command,
    sync::Mutex,
    task::JoinHandle,
    time::{sleep, timeout},
};
use tokio_tungstenite::accept_async;
use tower_http::{cors::CorsLayer, services::ServeDir, trace::TraceLayer};
use uuid::Uuid;
use walkdir::WalkDir;

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
const REQUIREMENT_EVENTS_FILE: &str = "events.jsonl";
const DEFAULT_WMS_PROJECT_ROOT: &str = "/home/hevin/Developer/company/WMS";
const DEFAULT_WMS_TESTDATA_PACK_ROOT: &str = "/home/hevin/Developer/company/WMS/.agents/testdata";
const DEFAULT_GITLAB_API_URL: &str = "http://code.jms.com/api/v4";
const DEFAULT_CAINIAO_MOCK_PORT: u16 = 13528;
/// 经验总结状态停留超过该时长后自动推进为已完成（48 小时）。
const EXPERIENCE_SUMMARY_GRACE_MS: i64 = 48 * 60 * 60 * 1000;
/// 自动推进扫描周期（每 10 分钟）。
const EXPERIENCE_AUTO_COMPLETE_INTERVAL_SECS: u64 = 600;
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
];
static REQ_STATUS_ALIASES: &[(&str, &str)] = &[
    ("需求对齐", "需求澄清"),
    ("方案设计", "需求澄清"),
    ("待设计", "需求澄清"),
    ("待开发", "需求澄清"),
    ("待上线", "经验总结"),
];
static REQ_CATEGORIES: &[&str] = &["需求", "线上问题"];

#[derive(Clone)]
struct AppState {
    project_root: Arc<PathBuf>,
    data_dir: Arc<PathBuf>,
    pi_session_root: Arc<PathBuf>,
    /// JoinHandle of the running cainiao print mock server task (None = not running).
    cainiao_mock: Arc<Mutex<Option<JoinHandle<()>>>>,
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
    #[serde(default)]
    cainiao_mock_enabled: bool,
    #[serde(default = "default_cainiao_mock_port")]
    cainiao_mock_port: u16,
}

fn default_cainiao_mock_port() -> u16 {
    DEFAULT_CAINIAO_MOCK_PORT
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
            cainiao_mock_enabled: false,
            cainiao_mock_port: DEFAULT_CAINIAO_MOCK_PORT,
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
    completed_at: Option<i64>,
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
    release_manifest_path: Option<String>,
    release_check_path: Option<String>,
    experience_summary_path: Option<String>,
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
    #[serde(alias = "reqId")]
    req_id: Option<String>,
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
    include_full: Option<bool>,
    section: Option<String>,
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

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct KnowledgeAgentQuery {
    #[serde(default)]
    kind: Option<String>,
    #[serde(default)]
    intent: Option<String>,
    #[serde(default)]
    query: Option<String>,
    #[serde(default)]
    domain: Option<String>,
    #[serde(default)]
    project: Option<String>,
    #[serde(default)]
    scope: Option<String>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    limit: Option<usize>,
    #[serde(default)]
    tokens_budget: Option<usize>,
    #[serde(default)]
    budget: Option<usize>,
    #[serde(default)]
    include_outline: Option<bool>,
    #[serde(default)]
    outline: Option<bool>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct KnowledgeWriteForm {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    kind: Option<String>,
    title: String,
    #[serde(default)]
    domain: Option<String>,
    #[serde(default)]
    project: Option<String>,
    #[serde(default)]
    scope: Option<String>,
    #[serde(default)]
    root: Option<String>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    category: Option<String>,
    #[serde(default)]
    confidence: Option<String>,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    trigger_terms: Vec<String>,
    #[serde(default)]
    related_skills: Vec<String>,
    #[serde(default)]
    related_repos: Vec<String>,
    #[serde(default)]
    related_tables: Vec<String>,
    #[serde(default)]
    related_apis: Vec<String>,
    #[serde(default)]
    source: Option<String>,
    #[serde(default)]
    origin_path: Option<String>,
    #[serde(default)]
    summary: Option<String>,
    #[serde(default)]
    details: Option<String>,
    #[serde(default)]
    last_verified_at: Option<String>,
    #[serde(default)]
    valid_until: Option<String>,
    #[serde(default)]
    dry_run: Option<bool>,
}

#[derive(Debug, Default)]
struct KnowledgeQueryFilter {
    kind: Option<String>,
    query: Option<String>,
    domain: Option<String>,
    project: Option<String>,
    scope: Option<String>,
    status: Option<String>,
    limit: Option<usize>,
    include_full: bool,
    include_outline: bool,
    budget: Option<usize>,
}

#[derive(Debug, Clone)]
struct KnowledgePath {
    path: PathBuf,
    kind: String,
    scope: String,
    root: PathBuf,
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
struct RequirementSectionForm {
    req_id: String,
    content: String,
    #[serde(default)]
    token: Option<String>,
    #[serde(default)]
    doc_type: Option<String>,
    #[serde(default)]
    heading: Option<String>,
    #[serde(default)]
    dry_run: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RequirementEventForm {
    req_id: String,
    #[serde(default, alias = "type")]
    event_type: Option<String>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    summary: Option<String>,
    #[serde(default)]
    details: Option<String>,
    #[serde(default)]
    evidence: Vec<String>,
    #[serde(default)]
    decisions: Vec<String>,
    #[serde(default)]
    todos: Vec<String>,
    #[serde(default)]
    related_files: Vec<String>,
    #[serde(default)]
    test_cases: Vec<RequirementEventTestCase>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    risk_level: Option<String>,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    session_id: Option<String>,
    #[serde(default)]
    idempotency_key: Option<String>,
    #[serde(default)]
    append_note: Option<bool>,
    #[serde(default)]
    dry_run: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
struct RequirementEventTestCase {
    #[serde(default)]
    name: String,
    #[serde(default)]
    result: String,
    #[serde(default)]
    evidence: Option<String>,
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
struct SyncBaseForm {
    req_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProdMrForm {
    req_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MergeBranchForm {
    req_id: String,
    #[serde(default)]
    target: Option<String>,
    #[serde(default)]
    target_branch: Option<String>,
    #[serde(default)]
    repo_kind: Option<String>,
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
    #[serde(default)]
    test_target_branch: Option<String>,
    #[serde(default)]
    uat_target_branch: Option<String>,
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
    cainiao_mock_enabled: Option<bool>,
    cainiao_mock_port: Option<u16>,
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
    };

    // Start the cainiao print mock server on boot if enabled in config.
    sync_cainiao_mock(&state).await;

    // Background: auto-advance requirements stuck in 经验总结 for >48h to 已完成.
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

async fn api_requirement_events(
    State(state): State<AppState>,
    form: FormOrJson<RequirementEventForm>,
) -> ApiResult<Json<Value>> {
    let value = record_requirement_event(&state, form.0).await?;
    Ok(Json(value))
}

async fn api_requirement_section(
    State(state): State<AppState>,
    AxumPath(section): AxumPath<String>,
    form: FormOrJson<RequirementSectionForm>,
) -> ApiResult<Json<Value>> {
    let edit = requirement_section_form_to_edit(section, form.0)?;
    let value = upsert_requirement_section(&state, edit).await?;
    Ok(Json(value))
}

async fn api_requirement_doc(
    State(state): State<AppState>,
    form: FormOrJson<RequirementDocForm>,
) -> ApiResult<Json<Value>> {
    let value = write_requirement_doc(&state, form.0).await?;
    Ok(Json(value))
}

async fn api_requirement_doc_get(
    State(state): State<AppState>,
    Query(query): Query<IdQuery>,
) -> ApiResult<Json<Value>> {
    let req_id = query.id.or(query.req_id).unwrap_or_default();
    let req = get_real_requirement(&state, &req_id).await?;
    let doc_type = query
        .file
        .as_deref()
        .or(query.kind.as_deref())
        .unwrap_or("background");
    let doc_file = requirement_doc_file(doc_type)?;
    let dir = req_dir_path(&req)?;
    let path = dir.join(doc_file);
    let exists = path.is_file();
    let content = if exists {
        fs::read_to_string(&path).await.unwrap_or_default()
    } else {
        String::new()
    };
    Ok(Json(json!({
        "ok": true,
        "reqId": req.id,
        "docType": doc_type,
        "file": doc_file,
        "path": path.to_string_lossy(),
        "exists": exists,
        "content": content,
    })))
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
    ensure_requirement_intent(&intent)?;
    Ok(Json(build_requirement_edit_plan(&req, &intent)))
}

async fn api_requirement_context(
    State(state): State<AppState>,
    Query(query): Query<IdQuery>,
) -> ApiResult<Json<Value>> {
    let req_id = query.id.or(query.req_id).unwrap_or_default();
    let req = get_real_requirement(&state, &req_id).await?;
    let intent = normalize_requirement_intent(query.intent.as_deref());
    ensure_requirement_intent(&intent)?;
    let budget = query.budget.unwrap_or(2_000).clamp(400, 12_000);
    let agent_context = query
        .for_agent
        .as_deref()
        .map(|v| v.eq_ignore_ascii_case("agent"))
        .unwrap_or(false)
        || query
            .kind
            .as_deref()
            .map(|v| v.eq_ignore_ascii_case("agent"))
            .unwrap_or(false);
    if agent_context {
        let limit = query.limit.unwrap_or(8).clamp(1, 30);
        let value = build_requirement_agent_context(&state, &req, &intent, budget, limit).await?;
        return Ok(Json(value));
    }
    let tokens = query
        .tokens
        .as_deref()
        .map(parse_token_list)
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| intent_read_tokens(&intent));
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
    let status = canonical_status(&body.status)?;
    let req = get_real_requirement(&state, &body.req_id).await?;
    if req.status != "测试中" && status == "测试中" {
        ensure_review_gate_allows_testing(&req).await?;
    }
    let st = write_requirement_status(
        req.req_dir.as_deref().unwrap_or_default(),
        &status,
        body.note.as_deref(),
    )
    .await?;
    if !matches!(st.get("changed").and_then(Value::as_bool), Some(false)) {
        record_status_transition_event(&state, &req, &st, body.note.as_deref()).await?;
    }
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
    // 先刷新本地生产基线分支到最新远端(fetch + reset/更新本地 base 分支),
    // 确保 diff 基线(origin/master / origin/production)与本地 base 分支都是最新,
    // 否则扫描是纯只读的,会用陈旧的 remote-tracking ref 计算 diff(看起来像没刷新)。
    let mut sync_results = Vec::new();
    for repo in &branch_scope.repos {
        sync_results.push(sync_repo_base_branch(repo).await);
    }
    let review = run_code_review_scan(&req_dir, &req.id, &branch_scope).await?;
    Ok(Json(
        json!({
            "ok": true,
            "branchScope": branch_scope,
            "sync": json!({
                "generatedAt": now_ms(),
                "results": sync_results,
            }),
            "review": review,
        }),
    ))
}

async fn api_requirement_review_gate(
    State(state): State<AppState>,
    Query(query): Query<IdQuery>,
) -> ApiResult<Json<Value>> {
    let id = query.id.or(query.req_id).unwrap_or_default();
    let req = get_real_requirement(&state, &id).await?;
    Ok(Json(review_gate_json(&req).await?))
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

async fn api_requirement_sync_base(
    State(state): State<AppState>,
    form: FormOrJson<SyncBaseForm>,
) -> ApiResult<Json<Value>> {
    let req = get_real_requirement(&state, &form.0.req_id).await?;
    let branch_scope = read_branch_scope(&PathBuf::from(req.req_dir.unwrap_or_default()))
        .await?
        .ok_or_else(|| {
            ApiError::bad_request(format!(
                "missing {BRANCH_SCOPE_FILE}; run req-branches-update first"
            ))
        })?;
    let mut results = Vec::new();
    for repo in &branch_scope.repos {
        results.push(sync_repo_base_branch(repo).await);
    }
    Ok(Json(json!({
        "ok": true,
        "generatedAt": now_ms(),
        "results": results,
    })))
}

async fn api_requirement_merge_options(
    State(state): State<AppState>,
    Query(query): Query<IdQuery>,
) -> ApiResult<Json<Value>> {
    let id = query.id.or(query.req_id).unwrap_or_default();
    let req = get_real_requirement(&state, &id).await?;
    let req_dir = PathBuf::from(req.req_dir.as_deref().unwrap_or_default());
    let branch_scope = read_branch_scope(&req_dir).await?.ok_or_else(|| {
        ApiError::bad_request(format!(
            "missing {BRANCH_SCOPE_FILE}; run req-branches-update first"
        ))
    })?;
    let options = build_merge_options(&branch_scope, &req.status).await;
    Ok(Json(json!({
        "ok": true,
        "reqId": req.id,
        "status": req.status,
        "generatedAt": now_ms(),
        "branchScope": branch_scope,
        "options": options,
    })))
}

async fn api_requirement_merge_branch(
    State(state): State<AppState>,
    form: FormOrJson<MergeBranchForm>,
) -> ApiResult<Json<Value>> {
    let body = form.0;
    let req = get_real_requirement(&state, &body.req_id).await?;
    let req_dir = PathBuf::from(req.req_dir.as_deref().unwrap_or_default());
    let branch_scope = read_branch_scope(&req_dir).await?.ok_or_else(|| {
        ApiError::bad_request(format!(
            "missing {BRANCH_SCOPE_FILE}; run req-branches-update first"
        ))
    })?;
    let merge_request = normalize_merge_request(&body)?;
    let results = merge_requirement_branches(&branch_scope, &merge_request).await;
    let status = merge_overall_status(&results);
    Ok(Json(json!({
        "ok": matches!(status, "merged" | "skipped" | "empty"),
        "reqId": req.id,
        "target": merge_request.target,
        "targetBranch": merge_request.target_branch,
        "repoKind": merge_request.repo_kind,
        "status": status,
        "generatedAt": now_ms(),
        "branchScope": branch_scope,
        "results": results,
    })))
}

async fn api_requirement_merge_status(
    State(state): State<AppState>,
    Query(query): Query<IdQuery>,
) -> ApiResult<Json<Value>> {
    let id = query.id.or(query.req_id).unwrap_or_default();
    let req = get_real_requirement(&state, &id).await?;
    let req_dir = PathBuf::from(req.req_dir.as_deref().unwrap_or_default());
    let branch_scope = read_branch_scope(&req_dir).await?.ok_or_else(|| {
        ApiError::bad_request(format!(
            "missing {BRANCH_SCOPE_FILE}; run req-branches-update first"
        ))
    })?;
    let target = match query
        .target
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
    {
        Some(raw) => Some(normalize_merge_target(raw)?),
        None => None,
    };
    let results = inspect_requirement_merge_status(&branch_scope, target.clone()).await;
    let status = merge_overall_status(&results);
    Ok(Json(json!({
        "ok": true,
        "reqId": req.id,
        "target": target,
        "status": status,
        "generatedAt": now_ms(),
        "branchScope": branch_scope,
        "results": results,
    })))
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

async fn api_knowledge_list(
    State(state): State<AppState>,
    Query(query): Query<IdQuery>,
) -> ApiResult<Json<Value>> {
    let filter = KnowledgeQueryFilter {
        kind: normalize_knowledge_kind(query.kind.as_deref()).ok(),
        query: clean_optional(query.q.as_deref())
            .or_else(|| clean_optional(query.intent.as_deref())),
        domain: clean_optional(query.domain.as_deref()),
        project: clean_optional(query.project.as_deref()),
        scope: clean_optional(query.scope.as_deref()),
        status: clean_optional(query.status.as_deref()),
        limit: query.limit,
        include_full: query.include_full.unwrap_or(false),
        include_outline: true,
        budget: query.budget,
    };
    let items = query_knowledge_items(&state, &filter).await?;
    Ok(Json(json!({ "items": items, "generatedAt": now_ms() })))
}

async fn api_knowledge_item(
    State(state): State<AppState>,
    Query(query): Query<IdQuery>,
) -> ApiResult<Json<Value>> {
    let id = clean_required_opt(query.id.as_deref(), "id")?;
    let item = get_knowledge_item(&state, &id, true, query.budget, query.section.as_deref())
        .await?
        .ok_or_else(|| ApiError::bad_request(format!("knowledge item not found: {id}")))?;
    Ok(Json(json!({ "item": item })))
}

async fn api_agent_knowledge_query(
    State(state): State<AppState>,
    Json(payload): Json<KnowledgeAgentQuery>,
) -> ApiResult<Json<Value>> {
    let budget = payload.tokens_budget.or(payload.budget).or(Some(2200));
    let filter = KnowledgeQueryFilter {
        kind: normalize_knowledge_kind(payload.kind.as_deref()).ok(),
        query: clean_optional(payload.intent.as_deref())
            .or_else(|| clean_optional(payload.query.as_deref())),
        domain: clean_optional(payload.domain.as_deref()),
        project: clean_optional(payload.project.as_deref()),
        scope: clean_optional(payload.scope.as_deref()),
        status: clean_optional(payload.status.as_deref()),
        limit: payload
            .limit
            .or_else(|| default_knowledge_limit_for_budget(budget)),
        include_full: false,
        include_outline: payload.include_outline.or(payload.outline).unwrap_or(true),
        budget,
    };
    let items = query_knowledge_items(&state, &filter).await?;
    Ok(Json(json!({
        "results": items,
        "generatedAt": now_ms(),
        "usage": {
            "summary": "Query returns budgeted summaries, outline, score, whyMatched, matchedFields, and matches. Use ids with GET /api/agent/items/full?id=<id> only when more detail is required; use section=<heading> to fetch one Markdown section.",
            "fullItemEndpoint": "/api/agent/items/full?id=<id>",
            "sectionEndpoint": "/api/agent/items/full?id=<id>&section=<heading>",
            "summaryEndpoint": "/api/agent/items/summary?id=<id>",
            "tokensBudget": filter.budget
        }
    })))
}

async fn api_agent_item_summary(
    State(state): State<AppState>,
    Query(query): Query<IdQuery>,
) -> ApiResult<Json<Value>> {
    let id = clean_required_opt(query.id.as_deref(), "id")?;
    let item = get_knowledge_item(&state, &id, false, query.budget, None)
        .await?
        .ok_or_else(|| ApiError::bad_request(format!("knowledge item not found: {id}")))?;
    Ok(Json(json!({ "item": item })))
}

async fn api_agent_item_full(
    State(state): State<AppState>,
    Query(query): Query<IdQuery>,
) -> ApiResult<Json<Value>> {
    let id = clean_required_opt(query.id.as_deref(), "id")?;
    let item = get_knowledge_item(&state, &id, true, query.budget, query.section.as_deref())
        .await?
        .ok_or_else(|| ApiError::bad_request(format!("knowledge item not found: {id}")))?;
    Ok(Json(json!({ "item": item })))
}

async fn api_knowledge_save(
    State(state): State<AppState>,
    Json(payload): Json<KnowledgeWriteForm>,
) -> ApiResult<Json<Value>> {
    let item = save_knowledge_item(&state, payload).await?;
    Ok(Json(json!({ "ok": true, "item": item })))
}

async fn api_capability_sources(State(state): State<AppState>) -> ApiResult<Json<Value>> {
    let sources = capability_sources(&state);
    Ok(Json(json!({
        "ok": true,
        "format": "agentPanel.capabilitySources.v1",
        "version": 1,
        "sources": sources,
        "schemaUrl": "/api/capability/schema",
        "rules": [
            "Agent Panel indexes project-owned capability packs and normalizes legacy adapters into a common schema.",
            "Project business data stays in the project root; Agent Panel does not store business assets in its own repo.",
            "Current APIs are read-only; run execution is intentionally out of scope for this phase."
        ]
    })))
}

async fn api_capability_schema() -> Json<Value> {
    Json(capability_pack_schema())
}

async fn api_testdata_capabilities(
    State(state): State<AppState>,
    Query(query): Query<IdQuery>,
) -> ApiResult<Json<Value>> {
    let project = query.project.as_deref().unwrap_or("WMS");
    let Some(source) = capability_sources(&state).into_iter().find(|source| {
        source
            .get("project")
            .and_then(Value::as_str)
            .map(|p| p.eq_ignore_ascii_case(project))
            .unwrap_or(false)
    }) else {
        return Err(ApiError::bad_request(format!(
            "capability source not found for project: {project}"
        )));
    };
    let source_path = PathBuf::from(
        source
            .get("sourcePath")
            .and_then(Value::as_str)
            .unwrap_or(""),
    );
    let pack = read_testdata_capability_pack(&source_path).await?;
    let raw_caps = pack
        .get("capabilities")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let target = query
        .target
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty());
    let domain = query
        .domain
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty());
    let q = query.q.as_deref().map(str::trim).filter(|v| !v.is_empty());
    let capabilities: Vec<Value> = raw_caps
        .into_iter()
        .filter(|cap| capability_matches(cap, target, domain, q))
        .map(|cap| capability_summary(&source_path, &cap))
        .collect();
    Ok(Json(json!({
        "ok": true,
        "format": "agentPanel.capabilities.v1",
        "project": project,
        "source": source,
        "filters": { "target": target, "domain": domain, "q": q },
        "count": capabilities.len(),
        "capabilities": capabilities,
        "apis": {
            "schema": "/api/capability/schema",
            "detail": "/api/capability?id=<capability-id>&project=<project>",
            "legacyDetail": "/api/testdata/capability?id=<capability-id>&project=<project>",
            "sources": "/api/capability/sources"
        },
        "help": {
            "skillName": "wms-test-data-creation",
            "skillPath": agent_panel_skill_path("wms-test-data-creation"),
            "phase": "read-only-index",
            "note": "第一阶段只读索引；如需执行，按 detail.runnerHint 中的命令到 sourcePath 项目运行。"
        }
    })))
}

async fn api_testdata_capability(
    State(state): State<AppState>,
    Query(query): Query<IdQuery>,
) -> ApiResult<Json<Value>> {
    let id = query.id.or(query.req_id).unwrap_or_default();
    if id.trim().is_empty() {
        return Err(ApiError::bad_request("missing capability id"));
    }
    let project = query.project.as_deref().unwrap_or("WMS");
    let Some(source) = capability_sources(&state).into_iter().find(|source| {
        source
            .get("project")
            .and_then(Value::as_str)
            .map(|p| p.eq_ignore_ascii_case(project))
            .unwrap_or(false)
    }) else {
        return Err(ApiError::bad_request(format!(
            "capability source not found for project: {project}"
        )));
    };
    let source_path = PathBuf::from(
        source
            .get("sourcePath")
            .and_then(Value::as_str)
            .unwrap_or(""),
    );
    let pack = read_testdata_capability_pack(&source_path).await?;
    let capabilities = pack
        .get("capabilities")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let Some(capability) = capabilities
        .into_iter()
        .find(|cap| cap.get("id").and_then(Value::as_str) == Some(id.as_str()))
    else {
        return Err(ApiError::bad_request(format!(
            "testdata capability not found: {id}"
        )));
    };
    Ok(Json(json!({
        "ok": true,
        "format": "agentPanel.capability.v1",
        "project": project,
        "source": source,
        "capability": capability_detail(&source_path, &capability),
        "help": {
            "skillName": "wms-test-data-creation",
            "skillPath": agent_panel_skill_path("wms-test-data-creation"),
            "phase": "read-only-index",
            "note": "第一阶段只读索引；Agent Panel 不执行造数脚本。"
        }
    })))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TestdataRunForm {
    project: String,
    capability_id: String,
    target: String,
    #[serde(default)]
    env: Option<String>,
    #[serde(default)]
    params: HashMap<String, String>,
    #[serde(default)]
    dry_run: Option<bool>,
    #[serde(default)]
    execute: Option<bool>,
}

async fn api_testdata_run(
    State(state): State<AppState>,
    form: FormOrJson<TestdataRunForm>,
) -> ApiResult<Json<Value>> {
    let body = form.0;
    let project = body.project.as_str();
    let Some(source) = capability_sources(&state).into_iter().find(|source| {
        source
            .get("project")
            .and_then(Value::as_str)
            .map(|p| p.eq_ignore_ascii_case(project))
            .unwrap_or(false)
    }) else {
        return Err(ApiError::bad_request(format!(
            "capability source not found for project: {project}"
        )));
    };
    let source_path = PathBuf::from(
        source
            .get("sourcePath")
            .and_then(Value::as_str)
            .unwrap_or(""),
    );
    if !source_path.is_dir() {
        return Err(ApiError::bad_request(format!(
            "capability pack missing: {}",
            source_path.display()
        )));
    }
    let pack = read_testdata_capability_pack(&source_path).await?;
    let capabilities = pack
        .get("capabilities")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let Some(capability) = capabilities
        .into_iter()
        .find(|cap| cap.get("id").and_then(Value::as_str) == Some(body.capability_id.as_str()))
    else {
        return Err(ApiError::bad_request(format!(
            "testdata capability not found: {}",
            body.capability_id
        )));
    };
    let script = capability
        .get("script")
        .and_then(Value::as_str)
        .unwrap_or("");
    if script.trim().is_empty() {
        return Err(ApiError::bad_request(format!(
            "capability {} has no runnable script",
            body.capability_id
        )));
    }
    let execution = capability
        .get("execution")
        .and_then(Value::as_str)
        .unwrap_or("script");
    if execution != "script" {
        return Err(ApiError::bad_request(format!(
            "capability {} execution is `{}`, Agent Panel only runs `script` capabilities",
            body.capability_id, execution
        )));
    }
    if body.target.trim().is_empty() {
        return Err(ApiError::bad_request("missing target"));
    }
    let target_valid = capability
        .get("targets")
        .and_then(Value::as_array)
        .map(|targets| {
            targets
                .iter()
                .any(|item| item.get("name").and_then(Value::as_str) == Some(body.target.as_str()))
        })
        .unwrap_or(false);
    if !target_valid {
        return Err(ApiError::bad_request(format!(
            "target `{}` is not supported by capability {}",
            body.target, body.capability_id
        )));
    }
    let mut args: Vec<String> = vec![
        "run".into(),
        "python".into(),
        script.into(),
        "--target".into(),
        body.target.clone(),
    ];
    if let Some(cli) = capability.get("cli").and_then(Value::as_object) {
        for (key, _spec) in cli {
            if key == "target" || key == "env" {
                continue;
            }
            if let Some(val) = body.params.get(key) {
                let trimmed = val.trim();
                if !trimmed.is_empty() {
                    args.push(format!("--{}", key));
                    args.push(trimmed.to_string());
                }
            }
        }
    }
    if let Some(env) = body.env.as_deref().map(str::trim).filter(|v| !v.is_empty()) {
        args.push("--env".into());
        args.push(env.to_string());
    }
    let command_preview = format!("uv {}", args.join(" "));
    let execute_flag = body.execute.unwrap_or(false);
    let dry_run = body.dry_run.unwrap_or(true);
    if !execute_flag || dry_run {
        return Ok(Json(json!({
            "ok": true,
            "dryRun": true,
            "executed": false,
            "project": project,
            "capabilityId": body.capability_id,
            "target": body.target,
            "env": body.env.clone(),
            "cwd": source_path.to_string_lossy(),
            "command": command_preview,
            "args": args,
            "safety": {
                "agentPanelExecutes": false,
                "env": "test",
                "note": "dry-run preview. Pass execute=true (and dryRun=false) to create real test data in the WMS test environment."
            }
        })));
    }
    let runner = Command::new("uv")
        .args(&args)
        .current_dir(&source_path)
        .output();
    let result = timeout(Duration::from_secs(180), runner).await;
    match result {
        Ok(Ok(output)) => {
            let stdout = String::from_utf8_lossy(&output.stdout).to_string();
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            Ok(Json(json!({
                "ok": output.status.success(),
                "dryRun": false,
                "executed": true,
                "project": project,
                "capabilityId": body.capability_id,
                "target": body.target,
                "env": body.env.clone(),
                "cwd": source_path.to_string_lossy(),
                "command": command_preview,
                "exitCode": output.status.code(),
                "stdout": stdout,
                "stderr": stderr,
                "safety": {
                    "agentPanelExecutes": true,
                    "env": body.env.clone().unwrap_or_else(|| "test".to_string()),
                    "note": "Real test data was created in the WMS test environment."
                }
            })))
        }
        Ok(Err(e)) => Err(ApiError::bad_request(format!(
            "failed to spawn runner for capability {}: {e}",
            body.capability_id
        ))),
        Err(_) => Err(ApiError::bad_request(format!(
            "runner timed out after 180s for capability {}",
            body.capability_id
        ))),
    }
}

async fn api_config(State(state): State<AppState>) -> ApiResult<Json<AppConfig>> {
    Ok(Json(read_config(&state).await?))
}

fn capability_pack_schema() -> Value {
    json!({
        "format": "agentPanel.capabilityPackSchema.v1",
        "version": 1,
        "packFile": "capabilities.yaml",
        "pack": {
            "required": ["version", "capabilities"],
            "recommended": ["id", "title", "project", "kind", "description", "updated"],
            "fields": {
                "version": "integer schema version",
                "id": "stable pack id, e.g. wms-testdata",
                "project": "owning project id/name",
                "kind": "capability kind, e.g. testdata/api/debug/deploy/docs",
                "capabilities": "array of capability items"
            }
        },
        "capability": {
            "required": ["id", "kind", "title", "runner"],
            "recommended": ["domain", "object", "description", "inputs", "outputs", "environments", "safety", "verification", "relatedArtifacts"],
            "fields": {
                "id": "stable id within pack",
                "kind": "testdata | api | debug | deploy | docs | custom",
                "title": "human-readable title",
                "description": "what this capability does",
                "domain": "business or technical domain",
                "object": "main target object/entity",
                "inputs": "parameter schema map",
                "outputs": "declared output schema map",
                "environments": "supported environments and policy",
                "runner": "how to run: command/script/recipe/api/dry-run; read-only APIs do not execute it",
                "safety": "side effects, DB write policy, env allowlist",
                "verification": "how to verify outputs or side effects",
                "relatedArtifacts": "recipes, schemas, state graphs, pitfalls, API templates, docs"
            }
        },
        "legacyAdapters": {
            "wms-testdata-recipes": {
                "id": "id",
                "kind": "testdata",
                "title": "purpose or id",
                "description": "purpose",
                "runner.command": "invocation/script/execution",
                "inputs": "cli",
                "verification.targets": "targets",
                "relatedArtifacts.recipe": "recipe",
                "relatedArtifacts.stateGraph": "state_graph",
                "relatedArtifacts.pitfalls": "pitfalls"
            }
        }
    })
}

fn capability_sources(_state: &AppState) -> Vec<Value> {
    let pack = Path::new(DEFAULT_WMS_TESTDATA_PACK_ROOT);
    vec![json!({
        "id": "wms-testdata",
        "kind": "testdata",
        "project": "WMS",
        "title": "WMS Test Data Capability Pack",
        "adapter": "wms-testdata-recipes",
        "projectRoot": DEFAULT_WMS_PROJECT_ROOT,
        "sourcePath": DEFAULT_WMS_TESTDATA_PACK_ROOT,
        "capabilityFile": "capabilities.yaml",
        "knowledgeRoot": format!("{}/.agents/knowledge", DEFAULT_WMS_PROJECT_ROOT),
        "capabilityApi": "/api/capabilities?project=WMS",
        "detailApi": "/api/capability?id=<capability-id>&project=WMS",
        "status": if pack.is_dir() { "ok" } else { "missing" },
        "exists": pack.is_dir(),
        "projectExists": Path::new(DEFAULT_WMS_PROJECT_ROOT).is_dir(),
        "readOnly": true,
        "notes": [
            "WMS-owned test-data assets live under the WMS project root.",
            "These APIs are read-only; Agent Panel does not execute runners."
        ]
    })]
}

async fn read_testdata_capability_pack(path: &Path) -> ApiResult<Value> {
    let file = path.join("capabilities.yaml");
    let raw = fs::read_to_string(&file).await.map_err(|e| {
        ApiError::bad_request(format!(
            "failed to read capability pack {}: {e}",
            file.display()
        ))
    })?;
    let value: serde_yaml::Value = serde_yaml::from_str(&raw).map_err(|e| {
        ApiError::bad_request(format!("invalid capability pack {}: {e}", file.display()))
    })?;
    let json = serde_json::to_value(value).map_err(|e| {
        ApiError::bad_request(format!(
            "failed to convert capability pack {}: {e}",
            file.display()
        ))
    })?;
    Ok(json)
}

fn capability_matches(
    cap: &Value,
    target: Option<&str>,
    domain: Option<&str>,
    q: Option<&str>,
) -> bool {
    if let Some(domain) = domain {
        let cap_domain = cap.get("domain").and_then(Value::as_str).unwrap_or("");
        if !cap_domain.eq_ignore_ascii_case(domain) {
            return false;
        }
    }
    if let Some(target) = target {
        let matches_target = cap
            .get("targets")
            .and_then(Value::as_array)
            .map(|targets| {
                targets.iter().any(|item| {
                    item.get("name").and_then(Value::as_str) == Some(target)
                        || item
                            .get("label")
                            .and_then(Value::as_str)
                            .map(|v| v.eq_ignore_ascii_case(target))
                            .unwrap_or(false)
                        || item
                            .get("status_code")
                            .and_then(Value::as_i64)
                            .map(|code| code.to_string() == target)
                            .unwrap_or(false)
                })
            })
            .unwrap_or(false);
        if !matches_target {
            return false;
        }
    }
    if let Some(q) = q {
        let q = q.to_lowercase();
        let mut hay = vec![
            cap.get("id")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            cap.get("domain")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            cap.get("purpose")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            cap.get("execution")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            cap.get("script")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            cap.get("recipe")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            cap.get("invocation")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
        ];
        if let Some(notes) = cap.get("notes").and_then(Value::as_array) {
            hay.push(
                notes
                    .iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join(" "),
            );
        }
        if !hay.iter().any(|v| v.to_lowercase().contains(&q)) {
            return false;
        }
    }
    true
}

fn capability_summary(source_path: &Path, cap: &Value) -> Value {
    let targets = cap
        .get("targets")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let verified_targets = targets
        .iter()
        .filter(|item| {
            item.get("verified")
                .and_then(Value::as_bool)
                .unwrap_or(false)
        })
        .count();
    let normalized = normalize_capability(source_path, cap);
    json!({
        "id": cap.get("id").cloned().unwrap_or(Value::Null),
        "domain": cap.get("domain").cloned().unwrap_or(Value::Null),
        "object": cap.get("object").cloned().unwrap_or(Value::Null),
        "execution": cap.get("execution").cloned().unwrap_or(Value::Null),
        "purpose": cap.get("purpose").cloned().unwrap_or(Value::Null),
        "script": cap.get("script").cloned().unwrap_or(Value::Null),
        "recipe": cap.get("recipe").cloned().unwrap_or(Value::Null),
        "verifiedEnv": cap.get("verified_env").cloned().unwrap_or(Value::Null),
        "verifiedDate": cap.get("verified_date").cloned().unwrap_or(Value::Null),
        "stdoutJson": cap.get("stdout_json").cloned().unwrap_or(Value::Null),
        "exitCode": cap.get("exit_code").cloned().unwrap_or(Value::Null),
        "targetCount": targets.len(),
        "verifiedTargetCount": verified_targets,
        "runHint": capability_run_hint(source_path, cap),
        "sourcePath": source_path.to_string_lossy(),
        "status": capability_status(cap),
        "normalized": normalized,
    })
}

fn capability_detail(source_path: &Path, cap: &Value) -> Value {
    let mut summary = capability_summary(source_path, cap);
    if let Some(map) = summary.as_object_mut() {
        map.insert("capability".to_string(), cap.clone());
        map.insert(
            "targets".to_string(),
            cap.get("targets").cloned().unwrap_or_else(|| json!([])),
        );
        map.insert(
            "cli".to_string(),
            cap.get("cli").cloned().unwrap_or_else(|| json!({})),
        );
        map.insert(
            "pitfalls".to_string(),
            cap.get("pitfalls").cloned().unwrap_or_else(|| json!([])),
        );
        map.insert(
            "notes".to_string(),
            cap.get("notes").cloned().unwrap_or_else(|| json!([])),
        );
        map.insert(
            "migration".to_string(),
            json!({
                "sourcePath": DEFAULT_WMS_TESTDATA_PACK_ROOT,
                "phase": "phase-3",
                "note": "Assets are owned by the WMS project; the legacy tools repo has been removed."
            })
        );
    }
    summary
}

fn normalize_capability(source_path: &Path, cap: &Value) -> Value {
    let id = cap.get("id").and_then(Value::as_str).unwrap_or_default();
    let purpose = cap.get("purpose").and_then(Value::as_str).unwrap_or(id);
    let runner_type = cap
        .get("execution")
        .and_then(Value::as_str)
        .unwrap_or("custom");
    json!({
        "schemaVersion": 1,
        "id": id,
        "kind": "testdata",
        "title": purpose,
        "description": purpose,
        "domain": cap.get("domain").cloned().unwrap_or(Value::Null),
        "object": cap.get("object").cloned().unwrap_or(Value::Null),
        "inputs": cap.get("cli").cloned().unwrap_or_else(|| json!({})),
        "outputs": {
            "stdoutJson": cap.get("stdout_json").cloned().unwrap_or(Value::Null),
            "exitCode": cap.get("exit_code").cloned().unwrap_or(Value::Null)
        },
        "environments": {
            "verified": cap.get("verified_env").cloned().unwrap_or(Value::Null),
            "verifiedDate": cap.get("verified_date").cloned().unwrap_or(Value::Null)
        },
        "runner": {
            "type": runner_type,
            "script": cap.get("script").cloned().unwrap_or(Value::Null),
            "command": cap.get("invocation").cloned().unwrap_or(Value::Null),
            "cwd": source_path.to_string_lossy(),
            "readOnlyInAgentPanel": true
        },
        "safety": {
            "agentPanelExecutes": false,
            "writesDatabase": false,
            "uatDbWritesAllowed": false,
            "policy": "Capability pack may define executable runners, but Agent Panel phase 3 only indexes and normalizes them."
        },
        "verification": {
            "targets": cap.get("targets").cloned().unwrap_or_else(|| json!([])),
            "stateGraph": cap.get("state_graph").cloned().unwrap_or(Value::Null)
        },
        "relatedArtifacts": {
            "recipe": cap.get("recipe").cloned().unwrap_or(Value::Null),
            "stateGraph": cap.get("state_graph").cloned().unwrap_or(Value::Null),
            "pitfalls": cap.get("pitfalls").cloned().unwrap_or_else(|| json!([])),
            "notes": cap.get("notes").cloned().unwrap_or_else(|| json!([]))
        },
        "legacy": cap
    })
}

fn capability_run_hint(source_path: &Path, cap: &Value) -> Value {
    json!({
        "sourcePath": source_path.to_string_lossy(),
        "dryRun": cap.get("invocation").cloned().unwrap_or(Value::Null),
        "execute": cap.get("invocation").and_then(Value::as_str).unwrap_or_default(),
        "normalizedRunner": normalize_capability(source_path, cap).get("runner").cloned().unwrap_or(Value::Null),
        "notes": [
            "Phase 3 keeps Agent Panel read-only.",
            "Use normalized.runner for generic capability-pack consumers.",
            "To execute manually, run the command from sourcePath in the project owner repo or legacy fallback path."
        ]
    })
}

fn capability_status(cap: &Value) -> Value {
    json!({
        "verifiedEnv": cap.get("verified_env").cloned().unwrap_or(Value::Null),
        "verifiedDate": cap.get("verified_date").cloned().unwrap_or(Value::Null),
        "hasScript": cap.get("script").and_then(Value::as_str).map(|s| !s.trim().is_empty()).unwrap_or(false),
        "hasRecipe": cap.get("recipe").and_then(Value::as_str).map(|s| !s.trim().is_empty()).unwrap_or(false)
    })
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
    if let Some(v) = patch.cainiao_mock_enabled {
        cfg.cainiao_mock_enabled = v;
    }
    if let Some(v) = patch.cainiao_mock_port {
        cfg.cainiao_mock_port = v;
    }
    write_config(&state, &cfg).await?;
    sync_cainiao_mock(&state).await;
    Ok(Json(cfg))
}

/// Start or stop the cainiao print mock server to match the persisted config.
/// Safe to call on boot and after every config change.
async fn sync_cainiao_mock(state: &AppState) {
    let cfg = read_config(state).await.unwrap_or_default();
    let mut guard = state.cainiao_mock.lock().await;
    let running = guard
        .as_ref()
        .map(|h| !h.is_finished())
        .unwrap_or(false);
    if cfg.cainiao_mock_enabled && !running {
        let port = cfg.cainiao_mock_port;
        let handle = tokio::spawn(async move {
            if let Err(e) = cainiao_mock_server(port).await {
                tracing::error!("cainiao mock server exited: {e:#}");
            }
        });
        *guard = Some(handle);
    } else if !cfg.cainiao_mock_enabled && running {
        if let Some(h) = guard.take() {
            h.abort();
            tracing::info!("cainiao mock server stopped");
        }
    }
}

async fn api_cainiao_mock_status(
    State(state): State<AppState>,
) -> ApiResult<Json<Value>> {
    let cfg = read_config(&state).await?;
    let running = {
        let guard = state.cainiao_mock.lock().await;
        guard.as_ref().map(|h| !h.is_finished()).unwrap_or(false)
    };
    Ok(Json(json!({
        "enabled": cfg.cainiao_mock_enabled,
        "running": running,
        "port": cfg.cainiao_mock_port,
    })))
}

/// Mock 菜鸟云打印客户端 WebSocket 服务，让前端以为打印成功。
/// 监听 ws://127.0.0.1:{port}（与 WMS 前端默认端口一致）。
async fn cainiao_mock_server(port: u16) -> Result<()> {
    let listener = TcpListener::bind(("127.0.0.1", port))
        .await
        .with_context(|| format!("cainiao mock: bind 127.0.0.1:{port}"))?;
    tracing::info!("Mock 菜鸟打印客户端 listening on ws://127.0.0.1:{port}");
    loop {
        let (stream, peer) = listener.accept().await?;
        let peer = peer.to_string();
        tokio::spawn(async move {
            if let Err(e) = cainiao_mock_conn(stream, &peer).await {
                tracing::debug!("cainiao mock conn {peer} ended: {e:#}");
            }
        });
    }
}

async fn cainiao_mock_conn(stream: TcpStream, peer: &str) -> Result<()> {
    let ws = accept_async(stream)
        .await
        .with_context(|| format!("cainiao mock websocket handshake from {peer}"))?;
    tracing::info!("[+] 前端已连接 {peer}");
    let (mut tx, mut rx) = ws.split();
    while let Some(msg) = rx.next().await {
        let msg = msg.context("read ws message")?;
        let text = match msg {
            tokio_tungstenite::tungstenite::Message::Text(t) => t.to_string(),
            tokio_tungstenite::tungstenite::Message::Binary(b) => {
                String::from_utf8_lossy(&b).into_owned()
            }
            tokio_tungstenite::tungstenite::Message::Close(_) => break,
            _ => continue,
        };
        let req: Value = match serde_json::from_str(&text) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let cmd = req.get("cmd").and_then(Value::as_str).unwrap_or("");
        let request_id = req.get("requestID").cloned().unwrap_or(Value::Null);
        // 关键:status='success' 是 React 端必查项(socket.ts:147)
        let reply = if cmd == "getPrinters" {
            json!({
                "requestID": request_id,
                "version": "1.0",
                "cmd": "getPrinters",
                "status": "success",
                "printers": CAINIAO_MOCK_PRINTERS
                    .iter()
                    .map(|(name, display)| json!({
                        "name": name,
                        "displayName": display,
                        "status": 0,
                    }))
                    .collect::<Vec<_>>(),
                "defaultPrinter": CAINIAO_MOCK_PRINTERS[0].0,
            })
        } else if cmd == "print" {
            let task = req.get("task").cloned().unwrap_or(Value::Null);
            json!({
                "requestID": request_id,
                "version": "1.0",
                "cmd": "print",
                "status": "success",
                "taskID": task.get("taskID").cloned().unwrap_or(Value::Null),
                "previewURL": "",
            })
        } else {
            continue;
        };
        tracing::info!("[<-] cmd={cmd} requestID={request_id}");
        tx.send(tokio_tungstenite::tungstenite::Message::Text(
            reply.to_string().into(),
        ))
        .await?;
    }
    tracing::info!("[-] 前端已断开 {peer}");
    Ok(())
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
        let help = api_error_help(&self.message);
        let body = if let Some(help) = help {
            json!({ "error": self.message, "help": help })
        } else {
            json!({ "error": self.message })
        };
        (self.status, Json(body)).into_response()
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

fn agent_panel_skill_path(skill_name: &str) -> String {
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

fn api_error_help(message: &str) -> Option<Value> {
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

fn all_knowledge_kinds() -> Vec<String> {
    vec!["businessKnowledge".to_string(), "experience".to_string()]
}

fn normalize_knowledge_kind(raw: Option<&str>) -> ApiResult<String> {
    let value = raw.unwrap_or("").trim().to_ascii_lowercase();
    match value.as_str() {
        "businessknowledge" | "business-knowledge" | "business_knowledge" | "business" | "biz"
        | "knowledge" | "业务知识" => Ok("businessKnowledge".to_string()),
        "experience" | "experiences" | "exp" | "经验" => Ok("experience".to_string()),
        "" => Err(ApiError::bad_request("missing knowledge kind")),
        _ => Err(ApiError::bad_request(format!(
            "unknown knowledge kind: {value}"
        ))),
    }
}

fn knowledge_kind_dir(kind: &str) -> &'static str {
    match kind {
        "businessKnowledge" => BUSINESS_KNOWLEDGE_DIR,
        "experience" => EXPERIENCES_DIR,
        _ => EXPERIENCES_DIR,
    }
}

fn knowledge_type_name(kind: &str) -> &'static str {
    match kind {
        "businessKnowledge" => "business_knowledge",
        "experience" => "experience",
        _ => "experience",
    }
}

async fn knowledge_storage_roots(
    state: &AppState,
    kind_filter: Option<&str>,
) -> Result<Vec<KnowledgePath>> {
    let home = home_dir()?;
    let mut bases: Vec<(String, PathBuf)> = vec![("global".to_string(), home.join(".agents"))];
    bases.push(("project".to_string(), state.project_root.as_ref().clone()));
    let cfg = read_config(state).await.unwrap_or_default();
    for root in normalize_scan_roots(cfg.requirement_scan_roots) {
        bases.push((
            "project".to_string(),
            normalize_knowledge_base(PathBuf::from(root)),
        ));
    }

    let kinds = kind_filter
        .map(|k| vec![k.to_string()])
        .unwrap_or_else(all_knowledge_kinds);
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for (scope, base) in bases {
        for kind in &kinds {
            let dir_name = knowledge_kind_dir(kind);
            let dir = if base.file_name().and_then(|v| v.to_str()) == Some(dir_name) {
                base.clone()
            } else if base.file_name().and_then(|v| v.to_str()) == Some(".agents") {
                base.join(dir_name)
            } else {
                base.join(".agents").join(dir_name)
            };
            let key = normalize_path_string(&dir);
            if seen.insert(format!("{kind}:{key}")) {
                out.push(KnowledgePath {
                    path: dir,
                    kind: kind.clone(),
                    scope: scope.clone(),
                    root: base.clone(),
                });
            }
        }
    }
    Ok(out)
}

fn normalize_knowledge_base(path: PathBuf) -> PathBuf {
    if path.file_name().and_then(|v| v.to_str()) == Some("req") {
        if path
            .parent()
            .and_then(|p| p.file_name())
            .and_then(|v| v.to_str())
            == Some(".agents")
        {
            return path
                .parent()
                .and_then(|p| p.parent())
                .map(Path::to_path_buf)
                .unwrap_or(path);
        }
    }
    if matches!(
        path.file_name().and_then(|v| v.to_str()),
        Some(BUSINESS_KNOWLEDGE_DIR) | Some(EXPERIENCES_DIR)
    ) {
        if let Some(parent) = path.parent().and_then(|p| p.parent()) {
            return parent.to_path_buf();
        }
    }
    path
}

async fn query_knowledge_items(
    state: &AppState,
    filter: &KnowledgeQueryFilter,
) -> ApiResult<Vec<Value>> {
    let roots = knowledge_storage_roots(state, filter.kind.as_deref()).await?;
    let budget = filter.budget.unwrap_or(2200).clamp(600, 60_000);
    let query_has_text = filter
        .query
        .as_deref()
        .map(|q| !q.trim().is_empty())
        .unwrap_or(false);
    let requested_limit = filter
        .limit
        .unwrap_or_else(|| default_knowledge_limit_for_budget(Some(budget)).unwrap_or(5));
    let effective_limit = requested_limit.clamp(1, max_knowledge_limit_for_budget(budget));
    let mut rows: Vec<(i64, String, Value)> = Vec::new();
    for root in roots {
        let meta_dir = root.path.join("meta");
        if !meta_dir.is_dir() {
            continue;
        }
        for entry in WalkDir::new(&meta_dir)
            .min_depth(1)
            .max_depth(1)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let path = entry.path();
            if !is_knowledge_meta_file(path) {
                continue;
            }
            let mut item = read_knowledge_item_file(
                &root,
                path,
                filter.include_full,
                filter.budget,
                None,
                filter.include_outline,
            )
            .await?;
            if !knowledge_item_matches(&item, filter) {
                continue;
            }
            let search = knowledge_search_score(&item, filter.query.as_deref());
            if query_has_text && search.score == 0 {
                continue;
            }
            apply_query_budget_to_item(&mut item, budget, effective_limit, filter.include_outline);
            if let Some(obj) = item.as_object_mut() {
                obj.insert("score".to_string(), json!(search.score));
                obj.insert("whyMatched".to_string(), json!(search.why_matched));
                obj.insert("matchedFields".to_string(), json!(search.matched_fields));
                obj.insert("matches".to_string(), json!(search.matches));
                obj.insert("queryTokens".to_string(), json!(search.query_tokens));
            }
            let updated = item
                .get("updatedAt")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            rows.push((search.score, updated, item));
        }
    }
    rows.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| b.1.cmp(&a.1)));
    Ok(rows
        .into_iter()
        .take(effective_limit)
        .map(|(_, _, item)| item)
        .collect())
}

fn knowledge_item_matches(item: &Value, filter: &KnowledgeQueryFilter) -> bool {
    value_filter_matches(item, "domain", filter.domain.as_deref())
        && value_filter_matches(item, "project", filter.project.as_deref())
        && value_filter_matches(item, "scope", filter.scope.as_deref())
        && value_filter_matches(item, "status", filter.status.as_deref())
}

fn is_knowledge_meta_file(path: &Path) -> bool {
    path.is_file()
        && path_has_component(path, "meta")
        && matches!(
            path.extension().and_then(|v| v.to_str()),
            Some("yaml") | Some("yml")
        )
}

fn path_has_component(path: &Path, name: &str) -> bool {
    path.components()
        .any(|c| c.as_os_str().to_string_lossy() == name)
}

fn value_filter_matches(item: &Value, key: &str, filter: Option<&str>) -> bool {
    let Some(filter) = filter.map(str::trim).filter(|v| !v.is_empty()) else {
        return true;
    };
    let value = item.get(key).and_then(Value::as_str).unwrap_or_default();
    value
        .to_ascii_lowercase()
        .contains(&filter.to_ascii_lowercase())
}

async fn get_knowledge_item(
    state: &AppState,
    id: &str,
    include_full: bool,
    budget: Option<usize>,
    section: Option<&str>,
) -> ApiResult<Option<Value>> {
    let Some((root, path, _)) = find_knowledge_file_by_id(state, id).await? else {
        return Ok(None);
    };
    read_knowledge_item_file(
        &root,
        &path,
        include_full,
        budget.or(Some(20_000)),
        section,
        true,
    )
    .await
    .map(Some)
}

async fn find_knowledge_file_by_id(
    state: &AppState,
    id: &str,
) -> ApiResult<Option<(KnowledgePath, PathBuf, String)>> {
    let target = id.trim();
    if target.is_empty() {
        return Ok(None);
    }
    for root in knowledge_storage_roots(state, None).await? {
        let meta_dir = root.path.join("meta");
        if !meta_dir.is_dir() {
            continue;
        }
        for entry in WalkDir::new(&meta_dir)
            .min_depth(1)
            .max_depth(1)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let path = entry.path();
            if !is_knowledge_meta_file(path) {
                continue;
            }
            let raw = fs::read_to_string(path).await.unwrap_or_default();
            let fm = parse_knowledge_meta_yaml(&raw, path)?;
            let item_id = fm.fields.get("id").cloned().unwrap_or_else(|| {
                path.file_stem()
                    .and_then(|v| v.to_str())
                    .unwrap_or_default()
                    .to_string()
            });
            if item_id == target {
                return Ok(Some((root.clone(), path.to_path_buf(), raw)));
            }
        }
    }
    Ok(None)
}

async fn read_knowledge_item_file(
    root: &KnowledgePath,
    path: &Path,
    include_full: bool,
    budget: Option<usize>,
    section: Option<&str>,
    include_outline: bool,
) -> ApiResult<Value> {
    let raw = fs::read_to_string(path).await.unwrap_or_default();
    let fm = parse_knowledge_meta_yaml(&raw, path)?;
    let source_path = resolve_knowledge_source_path(path, &fm);
    let source_body = if let Some(source) = source_path.as_ref().filter(|p| p.is_file()) {
        fs::read_to_string(source).await.unwrap_or_default()
    } else {
        String::new()
    };
    let meta = fs::metadata(path).await.ok();
    let file_stem = path.file_stem().and_then(|v| v.to_str()).unwrap_or("item");
    let kind = fm
        .fields
        .get("kind")
        .or_else(|| fm.fields.get("type"))
        .and_then(|v| normalize_knowledge_kind(Some(v)).ok())
        .unwrap_or_else(|| root.kind.clone());
    let title = fm
        .fields
        .get("title")
        .cloned()
        .unwrap_or_else(|| file_stem.to_string());
    let summary = clean_optional(fm.fields.get("summary").map(String::as_str))
        .unwrap_or_else(|| first_paragraph(&source_body));
    let summary_limit = summary_limit_for_budget(budget, include_full);
    let (summary, summary_truncated) = truncate_chars(&summary, summary_limit);
    let outline = if include_outline {
        markdown_outline(&source_body)
    } else {
        Vec::new()
    };
    let section_title = clean_optional(section);
    let section_body = section_title
        .as_deref()
        .map(|heading| markdown_section(&source_body, heading).unwrap_or_default())
        .unwrap_or_default();
    let detail_source = if section_title.is_some() {
        &section_body
    } else {
        &source_body
    };
    let (details, details_truncated) = if include_full {
        truncate_chars(detail_source, budget.unwrap_or(20_000).clamp(1_000, 60_000))
    } else {
        (String::new(), false)
    };
    let created_at = fm
        .fields
        .get("created_at")
        .or_else(|| fm.fields.get("createdAt"))
        .cloned()
        .or_else(|| {
            meta.as_ref()
                .and_then(|m| m.created().ok())
                .map(system_time_to_rfc3339)
        })
        .unwrap_or_else(rfc3339_now);
    let updated_at = fm
        .fields
        .get("updated_at")
        .or_else(|| fm.fields.get("updatedAt"))
        .cloned()
        .or_else(|| {
            meta.as_ref()
                .and_then(|m| m.modified().ok())
                .map(system_time_to_rfc3339)
        })
        .unwrap_or_else(rfc3339_now);
    Ok(json!({
        "id": fm.fields.get("id").cloned().unwrap_or_else(|| file_stem.to_string()),
        "title": title,
        "kind": kind,
        "type": knowledge_type_name(&kind),
        "category": fm.fields.get("category").cloned().unwrap_or_default(),
        "domain": fm.fields.get("domain").cloned().unwrap_or_else(|| "general".to_string()),
        "project": fm.fields.get("project").cloned().unwrap_or_default(),
        "scope": fm.fields.get("scope").cloned().unwrap_or_else(|| root.scope.clone()),
        "status": fm.fields.get("status").cloned().unwrap_or_else(|| KNOWLEDGE_DEFAULT_STATUS.to_string()),
        "confidence": fm.fields.get("confidence").cloned().unwrap_or_else(|| KNOWLEDGE_DEFAULT_CONFIDENCE.to_string()),
        "tags": frontmatter_list(fm.fields.get("tags")),
        "triggerTerms": frontmatter_list(fm.fields.get("trigger_terms").or_else(|| fm.fields.get("triggerTerms"))),
        "relatedSkills": frontmatter_list(fm.fields.get("related_skills").or_else(|| fm.fields.get("relatedSkills"))),
        "relatedRepos": frontmatter_list(fm.fields.get("related_repos").or_else(|| fm.fields.get("relatedRepos"))),
        "relatedTables": frontmatter_list(fm.fields.get("related_tables").or_else(|| fm.fields.get("relatedTables"))),
        "relatedApis": frontmatter_list(fm.fields.get("related_apis").or_else(|| fm.fields.get("relatedApis"))),
        "source": fm.fields.get("source").cloned().unwrap_or_default(),
        "originPath": fm.fields.get("origin_path").or_else(|| fm.fields.get("originPath")).cloned().unwrap_or_default(),
        "createdAt": created_at,
        "updatedAt": updated_at,
        "lastVerifiedAt": fm.fields.get("last_verified_at").or_else(|| fm.fields.get("lastVerifiedAt")).cloned().unwrap_or_default(),
        "validUntil": fm.fields.get("valid_until").or_else(|| fm.fields.get("validUntil")).cloned().unwrap_or_default(),
        "summary": summary,
        "summaryTruncated": summary_truncated,
        "outline": outline,
        "section": section_title.unwrap_or_default(),
        "sectionFound": if section.is_some() { !section_body.is_empty() } else { true },
        "details": if include_full { Value::String(details) } else { Value::Null },
        "detailsTruncated": details_truncated,
        "contentChars": source_body.chars().count(),
        "path": source_path.as_ref().map_or(path, |p| p.as_path()).to_string_lossy(),
        "sourcePath": source_path.as_ref().map(|p| p.to_string_lossy().to_string()).unwrap_or_default(),
        "metaPath": path.to_string_lossy(),
        "root": root.root.to_string_lossy(),
    }))
}

fn resolve_knowledge_source_path(meta_path: &Path, fm: &Frontmatter) -> Option<PathBuf> {
    let value = fm
        .fields
        .get("source_path")
        .or_else(|| fm.fields.get("sourcePath"))
        .map(String::as_str)
        .and_then(|v| clean_optional(Some(v)))?;
    let path = PathBuf::from(&value);
    if path.is_absolute() {
        Some(path)
    } else {
        Some(
            meta_path
                .parent()
                .unwrap_or_else(|| Path::new(""))
                .join(path),
        )
    }
}

#[derive(Debug, Default)]
struct KnowledgeSearchScore {
    score: i64,
    why_matched: Vec<String>,
    matched_fields: Vec<String>,
    matches: Vec<Value>,
    query_tokens: Vec<String>,
}

fn knowledge_search_score(item: &Value, query: Option<&str>) -> KnowledgeSearchScore {
    let Some(query) = query.map(str::trim).filter(|v| !v.is_empty()) else {
        return KnowledgeSearchScore {
            score: 1,
            ..Default::default()
        };
    };
    let tokens = knowledge_query_tokens(query);
    let fields = [
        (
            "title",
            12,
            item.get("title")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_ascii_lowercase(),
        ),
        (
            "triggerTerms",
            10,
            json_array_text(item.get("triggerTerms")),
        ),
        ("relatedApis", 10, json_array_text(item.get("relatedApis"))),
        (
            "relatedTables",
            9,
            json_array_text(item.get("relatedTables")),
        ),
        ("tags", 8, json_array_text(item.get("tags"))),
        ("relatedRepos", 7, json_array_text(item.get("relatedRepos"))),
        (
            "relatedSkills",
            7,
            json_array_text(item.get("relatedSkills")),
        ),
        (
            "summary",
            6,
            item.get("summary")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_ascii_lowercase(),
        ),
        ("outline", 5, outline_text(item.get("outline"))),
        (
            "id",
            4,
            item.get("id")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_ascii_lowercase(),
        ),
        (
            "domain",
            4,
            item.get("domain")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_ascii_lowercase(),
        ),
        (
            "project",
            4,
            item.get("project")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_ascii_lowercase(),
        ),
        (
            "category",
            3,
            item.get("category")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_ascii_lowercase(),
        ),
    ];
    let mut score = 0;
    let mut why = Vec::new();
    let mut matched_fields = Vec::new();
    let mut matches = Vec::new();
    for token in &tokens {
        let mut matched = false;
        for (field, weight, value) in &fields {
            if !token.is_empty() && value.contains(token) {
                score += *weight;
                matched = true;
                if !matched_fields.iter().any(|v| v == field) {
                    matched_fields.push((*field).to_string());
                }
                matches.push(json!({
                    "token": token,
                    "field": field,
                    "weight": weight,
                }));
            }
        }
        if matched && !why.iter().any(|v| v == token) {
            why.push(token.clone());
        }
    }
    KnowledgeSearchScore {
        score,
        why_matched: why,
        matched_fields,
        matches,
        query_tokens: tokens,
    }
}

fn default_knowledge_limit_for_budget(budget: Option<usize>) -> Option<usize> {
    let budget = budget.unwrap_or(2200);
    Some(match budget {
        0..=899 => 3,
        900..=1799 => 4,
        1800..=3999 => 5,
        4000..=7999 => 8,
        _ => 12,
    })
}

fn max_knowledge_limit_for_budget(budget: usize) -> usize {
    match budget {
        0..=899 => 3,
        900..=1799 => 5,
        1800..=3999 => 8,
        4000..=7999 => 12,
        _ => 20,
    }
}

fn summary_limit_for_budget(budget: Option<usize>, include_full: bool) -> usize {
    if include_full {
        return 700;
    }
    match budget.unwrap_or(2200) {
        0..=899 => 220,
        900..=1799 => 320,
        1800..=3999 => 420,
        4000..=7999 => 520,
        _ => 700,
    }
}

fn apply_query_budget_to_item(
    item: &mut Value,
    budget: usize,
    limit: usize,
    include_outline: bool,
) {
    let per_item_summary = (budget.saturating_mul(2) / limit.max(1)).clamp(140, 700);
    if let Some(obj) = item.as_object_mut() {
        if let Some(summary) = obj.get("summary").and_then(Value::as_str) {
            let already_truncated = obj
                .get("summaryTruncated")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let (next, truncated) = truncate_chars(summary, per_item_summary);
            obj.insert("summary".to_string(), Value::String(next));
            obj.insert(
                "summaryTruncated".to_string(),
                json!(already_truncated || truncated),
            );
        }
        if include_outline {
            let max_outline = match budget {
                0..=899 => 4,
                900..=1799 => 6,
                1800..=3999 => 10,
                _ => 16,
            };
            if let Some(outline) = obj.get_mut("outline").and_then(Value::as_array_mut) {
                if outline.len() > max_outline {
                    outline.truncate(max_outline);
                }
            }
        } else {
            obj.remove("outline");
        }
    }
}

fn knowledge_query_tokens(query: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut ascii = String::new();
    let mut cjk = String::new();
    for ch in query.chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
            flush_cjk_tokens(&mut tokens, &cjk);
            cjk.clear();
            ascii.push(ch.to_ascii_lowercase());
        } else if is_cjk(ch) {
            flush_ascii_token(&mut tokens, &ascii);
            ascii.clear();
            cjk.push(ch);
        } else {
            flush_ascii_token(&mut tokens, &ascii);
            flush_cjk_tokens(&mut tokens, &cjk);
            ascii.clear();
            cjk.clear();
        }
    }
    flush_ascii_token(&mut tokens, &ascii);
    flush_cjk_tokens(&mut tokens, &cjk);
    unique_strings(tokens).into_iter().take(80).collect()
}

fn flush_ascii_token(tokens: &mut Vec<String>, raw: &str) {
    let value = raw.trim().to_ascii_lowercase();
    if value.len() >= 2 || value.chars().any(|c| c.is_ascii_digit()) {
        tokens.push(value);
    }
}

fn flush_cjk_tokens(tokens: &mut Vec<String>, raw: &str) {
    let chars: Vec<char> = raw.chars().collect();
    if chars.is_empty() {
        return;
    }
    if chars.len() <= 8 {
        tokens.push(chars.iter().collect::<String>());
    }
    for n in [2usize, 3, 4, 5, 6] {
        if chars.len() < n {
            continue;
        }
        for window in chars.windows(n) {
            tokens.push(window.iter().collect());
        }
    }
}

fn is_cjk(ch: char) -> bool {
    matches!(ch as u32, 0x4E00..=0x9FFF | 0x3400..=0x4DBF | 0x20000..=0x2A6DF | 0x2A700..=0x2B73F | 0x2B740..=0x2B81F | 0x2B820..=0x2CEAF | 0xF900..=0xFAFF)
}

fn outline_text(value: Option<&Value>) -> String {
    value
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.get("title").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join(" ")
        })
        .unwrap_or_default()
        .to_ascii_lowercase()
}

fn markdown_outline(body: &str) -> Vec<Value> {
    body.lines()
        .filter_map(markdown_heading)
        .take(80)
        .map(|(level, title)| {
            json!({
                "level": level,
                "title": title,
                "anchor": markdown_anchor(&title),
            })
        })
        .collect()
}

fn markdown_section(body: &str, heading: &str) -> Option<String> {
    let target = heading.trim().to_ascii_lowercase();
    if target.is_empty() {
        return None;
    }
    let lines: Vec<&str> = body.lines().collect();
    let mut start = None;
    let mut start_level = 0usize;
    for (idx, line) in lines.iter().enumerate() {
        let Some((level, title)) = markdown_heading(line) else {
            continue;
        };
        let title_lower = title.to_ascii_lowercase();
        if title_lower == target
            || title_lower.contains(&target)
            || markdown_anchor(&title) == target
        {
            start = Some(idx);
            start_level = level;
            break;
        }
    }
    let start = start?;
    let mut end = lines.len();
    for (idx, line) in lines.iter().enumerate().skip(start + 1) {
        if let Some((level, _)) = markdown_heading(line) {
            if level <= start_level {
                end = idx;
                break;
            }
        }
    }
    Some(lines[start..end].join("\n"))
}

fn markdown_heading(line: &str) -> Option<(usize, String)> {
    let trimmed = line.trim_start();
    let hashes = trimmed.chars().take_while(|c| *c == '#').count();
    if !(1..=6).contains(&hashes) {
        return None;
    }
    let rest = trimmed[hashes..].trim();
    if rest.is_empty() {
        return None;
    }
    Some((hashes, rest.trim_matches('#').trim().to_string()))
}

fn markdown_anchor(title: &str) -> String {
    title
        .trim()
        .to_ascii_lowercase()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || is_cjk(c) {
                c
            } else {
                '-'
            }
        })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

fn json_array_text(value: Option<&Value>) -> String {
    value
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(" ")
        })
        .unwrap_or_default()
        .to_ascii_lowercase()
}

async fn save_knowledge_item(state: &AppState, form: KnowledgeWriteForm) -> ApiResult<Value> {
    let title = clean_required(&form.title, "title")?;
    ensure_text_size(&title, "title")?;
    let now = rfc3339_now();
    let requested_kind =
        normalize_knowledge_kind(form.kind.as_deref()).unwrap_or_else(|_| "experience".to_string());
    let requested_id = clean_optional(form.id.as_deref());
    let existing = if let Some(id) = requested_id.as_deref() {
        find_knowledge_file_by_id(state, id).await?
    } else {
        None
    };
    let existing_fm = existing
        .as_ref()
        .map(|(_, path, raw)| parse_knowledge_meta_yaml(raw, path))
        .transpose()?
        .unwrap_or(Frontmatter {
            fields: HashMap::new(),
            body: String::new(),
        });
    let existing_body = if let Some((_, path, raw)) = existing.as_ref() {
        let fm = parse_knowledge_meta_yaml(raw, path)?;
        if let Some(source) = resolve_knowledge_source_path(path, &fm) {
            if source.is_file() {
                fs::read_to_string(source).await.unwrap_or_default()
            } else {
                String::new()
            }
        } else {
            String::new()
        }
    } else {
        String::new()
    };
    let kind = if form.kind.is_some() {
        requested_kind
    } else {
        existing_fm
            .fields
            .get("kind")
            .or_else(|| existing_fm.fields.get("type"))
            .and_then(|v| normalize_knowledge_kind(Some(v)).ok())
            .unwrap_or(requested_kind)
    };
    let id = if let Some(id) = requested_id {
        ensure_knowledge_id(&id)?
    } else {
        unique_knowledge_id(state, &kind, form.domain.as_deref(), &title).await?
    };
    let scope = clean_optional(form.scope.as_deref())
        .or_else(|| existing_fm.fields.get("scope").cloned())
        .unwrap_or_else(|| "global".to_string());
    let domain = clean_optional(form.domain.as_deref())
        .or_else(|| existing_fm.fields.get("domain").cloned())
        .unwrap_or_else(|| "general".to_string());
    let project = clean_optional(form.project.as_deref())
        .or_else(|| existing_fm.fields.get("project").cloned())
        .unwrap_or_default();
    let status = clean_optional(form.status.as_deref())
        .or_else(|| existing_fm.fields.get("status").cloned())
        .unwrap_or_else(|| KNOWLEDGE_DEFAULT_STATUS.to_string());
    let category = clean_optional(form.category.as_deref())
        .or_else(|| existing_fm.fields.get("category").cloned())
        .unwrap_or_else(|| category_from_knowledge_id(&id));
    let confidence = clean_optional(form.confidence.as_deref())
        .or_else(|| existing_fm.fields.get("confidence").cloned())
        .unwrap_or_else(|| KNOWLEDGE_DEFAULT_CONFIDENCE.to_string());
    let summary = clean_optional(form.summary.as_deref())
        .or_else(|| existing_fm.fields.get("summary").cloned())
        .unwrap_or_default();
    ensure_text_size(&summary, "summary")?;
    let details = clean_optional(form.details.as_deref())
        .or_else(|| clean_optional(Some(existing_body.as_str())))
        .unwrap_or_else(|| template_knowledge_body(&kind));
    ensure_text_size(&details, "details")?;
    let created_at = existing_fm
        .fields
        .get("created_at")
        .or_else(|| existing_fm.fields.get("createdAt"))
        .cloned()
        .unwrap_or_else(|| now.clone());
    let last_verified_at = clean_optional(form.last_verified_at.as_deref())
        .or_else(|| existing_fm.fields.get("last_verified_at").cloned())
        .unwrap_or_default();
    let valid_until = clean_optional(form.valid_until.as_deref())
        .or_else(|| existing_fm.fields.get("valid_until").cloned())
        .unwrap_or_default();
    let tags = if form.tags.is_empty() {
        frontmatter_list(existing_fm.fields.get("tags"))
    } else {
        unique_strings(form.tags)
    };
    let trigger_terms = if form.trigger_terms.is_empty() {
        frontmatter_list(existing_fm.fields.get("trigger_terms"))
    } else {
        unique_strings(form.trigger_terms)
    };
    let related_skills = if form.related_skills.is_empty() {
        frontmatter_list(existing_fm.fields.get("related_skills"))
    } else {
        unique_strings(form.related_skills)
    };
    let related_repos = if form.related_repos.is_empty() {
        frontmatter_list(existing_fm.fields.get("related_repos"))
    } else {
        unique_strings(form.related_repos)
    };
    let related_tables = if form.related_tables.is_empty() {
        frontmatter_list(existing_fm.fields.get("related_tables"))
    } else {
        unique_strings(form.related_tables)
    };
    let related_apis = if form.related_apis.is_empty() {
        frontmatter_list(existing_fm.fields.get("related_apis"))
    } else {
        unique_strings(form.related_apis)
    };
    let source = clean_optional(form.source.as_deref())
        .or_else(|| existing_fm.fields.get("source").cloned())
        .unwrap_or_default();
    let origin_path = clean_optional(form.origin_path.as_deref())
        .or_else(|| existing_fm.fields.get("origin_path").cloned())
        .or_else(|| existing_fm.fields.get("originPath").cloned())
        .unwrap_or_default();

    let (meta_path, source_path) = if let Some((_, path, raw)) = existing {
        let fm = parse_knowledge_meta_yaml(&raw, &path)?;
        let source = resolve_knowledge_source_path(&path, &fm).unwrap_or_else(|| {
            path.parent()
                .unwrap_or_else(|| Path::new(""))
                .join("../items")
                .join(format!("{id}.md"))
        });
        (path, source)
    } else {
        knowledge_paths_for_new_item(state, &kind, &scope, form.root.as_deref(), &domain, &id)
            .await?
    };
    if form.dry_run.unwrap_or(false) {
        return Ok(
            json!({ "id": id, "path": source_path, "sourcePath": source_path, "metaPath": meta_path, "dryRun": true }),
        );
    }
    let meta_source_path = relative_knowledge_source_path(&meta_path, &source_path);
    let meta_text = render_knowledge_meta_yaml(KnowledgeRenderInput {
        id: &id,
        kind: &kind,
        title: &title,
        category: &category,
        domain: &domain,
        project: &project,
        scope: &scope,
        status: &status,
        confidence: &confidence,
        tags: &tags,
        trigger_terms: &trigger_terms,
        related_skills: &related_skills,
        related_repos: &related_repos,
        related_tables: &related_tables,
        related_apis: &related_apis,
        source: &source,
        origin_path: &origin_path,
        source_path: &meta_source_path,
        summary: &summary,
        created_at: &created_at,
        updated_at: &now,
        last_verified_at: &last_verified_at,
        valid_until: &valid_until,
    });
    atomic_write_text(&source_path, &(details.trim().to_string() + "\n")).await?;
    atomic_write_text(&meta_path, &meta_text).await?;
    let root = KnowledgePath {
        path: meta_path
            .parent()
            .unwrap_or_else(|| Path::new(""))
            .to_path_buf(),
        kind,
        scope,
        root: meta_path
            .parent()
            .unwrap_or_else(|| Path::new(""))
            .to_path_buf(),
    };
    read_knowledge_item_file(&root, &meta_path, true, Some(20_000), None, true).await
}

async fn unique_knowledge_id(
    state: &AppState,
    kind: &str,
    domain: Option<&str>,
    title: &str,
) -> ApiResult<String> {
    let mut id = knowledge_id_from_title(kind, domain, title);
    if find_knowledge_file_by_id(state, &id).await?.is_none() {
        return Ok(id);
    }
    id = format!("{}-{}", id, chrono::Utc::now().format("%H%M%S"));
    ensure_knowledge_id(&id)
}

fn knowledge_id_from_title(kind: &str, domain: Option<&str>, title: &str) -> String {
    let prefix = if kind == "businessKnowledge" {
        "biz"
    } else {
        "exp"
    };
    let domain = slug_segment(domain.unwrap_or("general"), "general");
    let title_slug = slug_segment(
        title,
        &chrono::Utc::now().format("%Y%m%d%H%M%S").to_string(),
    );
    format!("{prefix}-{domain}-{title_slug}")
}

fn ensure_knowledge_id(value: &str) -> ApiResult<String> {
    let v = value.trim();
    if v.len() < 3 || v.len() > 160 {
        return Err(ApiError::bad_request("invalid knowledge id length"));
    }
    if !v
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
    {
        return Err(ApiError::bad_request(
            "knowledge id must be ASCII alphanumeric, dash, underscore, or dot",
        ));
    }
    Ok(v.to_string())
}

async fn resolve_knowledge_write_dir(
    state: &AppState,
    kind: &str,
    scope: &str,
    requested_root: Option<&str>,
) -> ApiResult<PathBuf> {
    let dir_name = knowledge_kind_dir(kind);
    if let Some(raw) = clean_optional(requested_root) {
        let base = normalize_user_path(&raw);
        let dir = if base.file_name().and_then(|v| v.to_str()) == Some(dir_name) {
            base
        } else if base.file_name().and_then(|v| v.to_str()) == Some(".agents") {
            base.join(dir_name)
        } else {
            base.join(".agents").join(dir_name)
        };
        let home = home_dir()?;
        if !same_or_child_path(&dir, &home) {
            return Err(ApiError::bad_request(format!(
                "knowledge root is outside home: {raw}"
            )));
        }
        return Ok(dir);
    }
    if scope == "project" {
        if let Some(root) = knowledge_storage_roots(state, Some(kind))
            .await?
            .into_iter()
            .find(|r| r.scope == "project")
        {
            return Ok(root.path);
        }
    }
    Ok(home_dir()?.join(".agents").join(dir_name))
}

struct KnowledgeRenderInput<'a> {
    id: &'a str,
    kind: &'a str,
    title: &'a str,
    category: &'a str,
    domain: &'a str,
    project: &'a str,
    scope: &'a str,
    status: &'a str,
    confidence: &'a str,
    tags: &'a [String],
    trigger_terms: &'a [String],
    related_skills: &'a [String],
    related_repos: &'a [String],
    related_tables: &'a [String],
    related_apis: &'a [String],
    source: &'a str,
    origin_path: &'a str,
    source_path: &'a str,
    summary: &'a str,
    created_at: &'a str,
    updated_at: &'a str,
    last_verified_at: &'a str,
    valid_until: &'a str,
}

fn render_knowledge_meta_yaml(input: KnowledgeRenderInput<'_>) -> String {
    let mut lines = vec![
        format!("id: {}", yaml_quote(input.id)),
        format!("title: {}", yaml_quote(input.title)),
        format!("kind: {}", yaml_quote(input.kind)),
        format!("type: {}", yaml_quote(knowledge_type_name(input.kind))),
        format!("category: {}", yaml_quote(input.category)),
        format!("domain: {}", yaml_quote(input.domain)),
        format!("project: {}", yaml_quote(input.project)),
        format!("scope: {}", yaml_quote(input.scope)),
        format!("status: {}", yaml_quote(input.status)),
        format!("confidence: {}", yaml_quote(input.confidence)),
        format!("created_at: {}", yaml_quote(input.created_at)),
        format!("updated_at: {}", yaml_quote(input.updated_at)),
        format!("source_path: {}", yaml_quote(input.source_path)),
    ];
    if !input.last_verified_at.is_empty() {
        lines.push(format!(
            "last_verified_at: {}",
            yaml_quote(input.last_verified_at)
        ));
    }
    if !input.valid_until.is_empty() {
        lines.push(format!("valid_until: {}", yaml_quote(input.valid_until)));
    }
    if !input.source.is_empty() {
        lines.push(format!("source: {}", yaml_quote(input.source)));
    }
    if !input.origin_path.is_empty() {
        lines.push(format!("origin_path: {}", yaml_quote(input.origin_path)));
    }
    if !input.summary.is_empty() {
        lines.push(format!("summary: {}", yaml_quote(input.summary)));
    }
    lines.push(format!("tags: {}", yaml_inline_list(input.tags)));
    lines.push(format!(
        "trigger_terms: {}",
        yaml_inline_list(input.trigger_terms)
    ));
    if !input.related_skills.is_empty() {
        lines.push(format!(
            "related_skills: {}",
            yaml_inline_list(input.related_skills)
        ));
    }
    if !input.related_repos.is_empty() {
        lines.push(format!(
            "related_repos: {}",
            yaml_inline_list(input.related_repos)
        ));
    }
    if !input.related_tables.is_empty() {
        lines.push(format!(
            "related_tables: {}",
            yaml_inline_list(input.related_tables)
        ));
    }
    if !input.related_apis.is_empty() {
        lines.push(format!(
            "related_apis: {}",
            yaml_inline_list(input.related_apis)
        ));
    }
    format!("{}\n", lines.join("\n"))
}

fn yaml_inline_list(values: &[String]) -> String {
    if values.is_empty() {
        "[]".to_string()
    } else {
        format!(
            "[{}]",
            values
                .iter()
                .map(|v| yaml_quote(v))
                .collect::<Vec<_>>()
                .join(", ")
        )
    }
}

async fn knowledge_paths_for_new_item(
    state: &AppState,
    kind: &str,
    scope: &str,
    requested_root: Option<&str>,
    _domain: &str,
    id: &str,
) -> ApiResult<(PathBuf, PathBuf)> {
    let dir = resolve_knowledge_write_dir(state, kind, scope, requested_root).await?;
    let base = dir;
    Ok((
        base.join("meta").join(format!("{id}.yaml")),
        base.join("items").join(format!("{id}.md")),
    ))
}

fn relative_knowledge_source_path(meta_path: &Path, source_path: &Path) -> String {
    if let Some(meta_dir) = meta_path.parent() {
        if let Some(base) = meta_dir.parent() {
            if let Ok(rel) = source_path.strip_prefix(base) {
                return format!("../{}", rel.to_string_lossy());
            }
        }
        if let Ok(rel) = source_path.strip_prefix(meta_dir) {
            return rel.to_string_lossy().to_string();
        }
    }
    source_path.to_string_lossy().to_string()
}

fn category_from_knowledge_id(id: &str) -> String {
    let first = id.split('-').next().unwrap_or_default();
    match first {
        "api" | "biz" | "conventions" | "link" | "pitfall" | "profile" | "ref" => first.to_string(),
        "exp" => "experience".to_string(),
        _ => "general".to_string(),
    }
}

fn frontmatter_list(value: Option<&String>) -> Vec<String> {
    value
        .map(|v| {
            v.trim()
                .trim_matches(|c| matches!(c, '[' | ']' | ' '))
                .split([',', '，'])
                .map(|s| s.trim().trim_matches('"').trim_matches('\'').to_string())
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

fn slug_segment(value: &str, fallback: &str) -> String {
    let mut out = String::new();
    let mut prev_dash = false;
    for ch in value.to_ascii_lowercase().chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch);
            prev_dash = false;
        } else if matches!(ch, '-' | '_' | '.') {
            out.push(ch);
            prev_dash = false;
        } else if !prev_dash {
            out.push('-');
            prev_dash = true;
        }
    }
    let compact = out.trim_matches('-').to_string();
    if compact.is_empty() {
        fallback.to_string()
    } else {
        compact
    }
}

fn rfc3339_now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

fn system_time_to_rfc3339(value: SystemTime) -> String {
    let dt: chrono::DateTime<chrono::Utc> = value.into();
    dt.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

fn template_knowledge_body(kind: &str) -> String {
    if kind == "businessKnowledge" {
        "## 概述\n\n## 规则\n\n## 相关接口\n\n## 相关表\n\n## 注意事项".to_string()
    } else {
        "## 现象\n\n## 根因\n\n## 处理方式\n\n## 验证方式\n\n## 过期风险".to_string()
    }
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
    let req_state = read_requirement_state(dir).await?;
    if let Some(state) = &req_state {
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
    let completed_at = req_state.as_ref().and_then(extract_completed_at);
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
        completed_at,
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
        release_manifest_path: path_if_exists(dir.join("release-manifest.md")),
        release_check_path: path_if_exists(dir.join("release-check.md")),
        experience_summary_path: path_if_exists(dir.join("experience-summary.md")),
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
        completed_at: None,
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
        release_manifest_path: None,
        release_check_path: None,
        experience_summary_path: None,
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

/// 同步单个仓库的本地生产基线分支到最新远端:
/// 1. git fetch <remote> <base-branch>(更新 remote-tracking ref,diff 基线即从此读取)
/// 2. 把本地 <base-branch> 分支指向 <remote>/<base-branch>:
///    - 当前 HEAD == base-branch 且工作区干净 -> git reset --hard
///    - 当前 HEAD == base-branch 但工作区脏 -> 跳过 reset,仅 fetch(保护未提交改动)
///    - 当前 HEAD != base-branch -> git branch -f(不动工作区)
async fn sync_repo_base_branch(repo: &BranchRepo) -> Value {
    let repo_name = repo.repo_name.clone();
    let project_path = match resolve_code_review_project_path(repo.path.as_deref(), &repo.repo_name)
    {
        Some(p) => p,
        None => {
            return json!({
                "repoName": repo_name,
                "ok": false,
                "status": "skipped",
                "message": "branches.json 缺少 path",
            })
        }
    };
    if !project_path.exists() {
        return json!({
            "repoName": repo_name,
            "ok": false,
            "status": "skipped",
            "message": format!("仓库路径不存在：{}", project_path.to_string_lossy()),
        });
    }
    let git_root = git(
        &project_path,
        &["rev-parse", "--show-toplevel"],
        30_000,
        COMMAND_OUTPUT_LIMIT,
    )
    .await;
    if !git_root.ok {
        return json!({
            "repoName": repo_name,
            "ok": false,
            "status": "skipped",
            "message": "projectPath 不是 Git 仓库",
        });
    }

    // base ref 的确定与 code review scan 保持一致
    let base_ref = repo
        .base_ref
        .as_deref()
        .filter(|v| !v.trim().is_empty())
        .map(str::trim)
        .map(str::to_string)
        .unwrap_or_else(|| detect_default_base_ref(repo));
    let base_info = parse_base_ref(&base_ref);
    let local_branch = base_info.local_branch.clone();
    let remote_ref = format!("{}/{}", base_info.remote, base_info.remote_branch);
    let mut warnings = Vec::<String>::new();

    // 1. fetch 远端分支,更新 remote-tracking ref(diff 基线即从此读取)
    let fetch = git(
        &project_path,
        &[
            "fetch",
            base_info.remote.as_str(),
            base_info.remote_branch.as_str(),
        ],
        60_000,
        COMMAND_OUTPUT_LIMIT,
    )
    .await;
    if !fetch.ok {
        return json!({
            "repoName": repo_name,
            "ok": false,
            "status": "fetch_failed",
            "baseRef": base_info.base_ref,
            "remoteRef": remote_ref,
            "message": format!("fetch {remote_ref} 失败：{}", short_err(&fetch)),
            "warnings": warnings,
        });
    }

    // fetch 后 remote-tracking ref 的最新 commit
    let after = git(
        &project_path,
        &["rev-parse", "--short", &remote_ref],
        30_000,
        COMMAND_OUTPUT_LIMIT,
    )
    .await;
    let after_commit = if after.ok {
        after.stdout.trim().to_string()
    } else {
        String::new()
    };

    // 2. 检查本地 base 分支是否存在
    let local_ref = format!("refs/heads/{local_branch}");
    let verify_local = git(
        &project_path,
        &["rev-parse", "--verify", "--quiet", &local_ref],
        30_000,
        COMMAND_OUTPUT_LIMIT,
    )
    .await;
    if !verify_local.ok {
        // 本地分支不存在,只 fetch 不 reset(不主动创建本地分支)
        return json!({
            "repoName": repo_name,
            "ok": true,
            "status": "fetched_no_local",
            "baseRef": base_info.base_ref,
            "remoteRef": remote_ref,
            "localBranch": local_branch,
            "afterCommit": after_commit,
            "message": format!("本地分支 {local_branch} 不存在,已 fetch {remote_ref},未 reset"),
            "warnings": warnings,
        });
    }

    // 本地分支 reset 前的 commit
    let before = git(
        &project_path,
        &["rev-parse", "--short", &local_ref],
        30_000,
        COMMAND_OUTPUT_LIMIT,
    )
    .await;
    let before_commit = if before.ok {
        before.stdout.trim().to_string()
    } else {
        String::new()
    };

    // 当前 HEAD(判断是否 checkout 在 base 分支)
    let head = git(
        &project_path,
        &["rev-parse", "--abbrev-ref", "HEAD"],
        30_000,
        COMMAND_OUTPUT_LIMIT,
    )
    .await;
    let current_branch = if head.ok {
        head.stdout.trim().to_string()
    } else {
        String::new()
    };

    if current_branch == local_branch {
        // 当前 checkout 在 base 分支:reset --hard 前必须确认工作区干净
        let dirty = git(
            &project_path,
            &["status", "--porcelain"],
            30_000,
            COMMAND_OUTPUT_LIMIT,
        )
        .await;
        if dirty.ok && !dirty.stdout.trim().is_empty() {
            warnings.push(format!(
                "当前在 {local_branch} 但工作区有未提交改动,已跳过 reset 以免丢弃"
            ));
            return json!({
                "repoName": repo_name,
                "ok": true,
                "status": "dirty_skipped",
                "baseRef": base_info.base_ref,
                "remoteRef": remote_ref,
                "localBranch": local_branch,
                "currentBranch": current_branch,
                "beforeCommit": before_commit,
                "afterCommit": after_commit,
                "message": format!("工作区不干净,已 fetch {remote_ref} 但跳过 reset"),
                "warnings": warnings,
            });
        }
        let reset = git(
            &project_path,
            &["reset", "--hard", &remote_ref],
            60_000,
            COMMAND_OUTPUT_LIMIT,
        )
        .await;
        if !reset.ok {
            warnings.push(format!(
                "reset --hard {remote_ref} 失败：{}",
                short_err(&reset)
            ));
            return json!({
                "repoName": repo_name,
                "ok": false,
                "status": "reset_failed",
                "baseRef": base_info.base_ref,
                "remoteRef": remote_ref,
                "localBranch": local_branch,
                "currentBranch": current_branch,
                "beforeCommit": before_commit,
                "afterCommit": after_commit,
                "message": format!("reset 失败：{}", short_err(&reset)),
                "warnings": warnings,
            });
        }
        return json!({
            "repoName": repo_name,
            "ok": true,
            "status": "reset",
            "baseRef": base_info.base_ref,
            "remoteRef": remote_ref,
            "localBranch": local_branch,
            "currentBranch": current_branch,
            "beforeCommit": before_commit,
            "afterCommit": after_commit,
            "message": format!("{local_branch} 已 reset 到 {remote_ref}"),
            "warnings": warnings,
        });
    }

    // 当前不在 base 分支:用 git branch -f 把本地 base 分支指向远端(不动工作区)
    let branch_f = git(
        &project_path,
        &["branch", "-f", local_branch.as_str(), &remote_ref],
        30_000,
        COMMAND_OUTPUT_LIMIT,
    )
    .await;
    if !branch_f.ok {
        warnings.push(format!(
            "git branch -f {local_branch} {remote_ref} 失败：{}",
            short_err(&branch_f)
        ));
        return json!({
            "repoName": repo_name,
            "ok": false,
            "status": "update_ref_failed",
            "baseRef": base_info.base_ref,
            "remoteRef": remote_ref,
            "localBranch": local_branch,
            "currentBranch": current_branch,
            "beforeCommit": before_commit,
            "afterCommit": after_commit,
            "message": format!("更新本地分支指向失败：{}", short_err(&branch_f)),
            "warnings": warnings,
        });
    }
    json!({
        "repoName": repo_name,
        "ok": true,
        "status": "updated",
        "baseRef": base_info.base_ref,
        "remoteRef": remote_ref,
        "localBranch": local_branch,
        "currentBranch": current_branch,
        "beforeCommit": before_commit,
        "afterCommit": after_commit,
        "message": format!("{local_branch} 已更新到 {remote_ref}(当前 checkout 在 {current_branch},工作区未动)"),
        "warnings": warnings,
    })
}

#[derive(Debug, Clone)]
struct ProdDiffStat {
    files: i64,
    additions: i64,
    deletions: i64,
    no_diff: bool,
}

/// 计算需求分支相较于生产分支(target)的差异统计。
/// fetch 远端 target 后用三点 diff 比对;返回 None 表示无法判断(分支缺失/命令失败)。
async fn compute_prod_diff_stat(
    project_path: &Path,
    target_branch: &str,
    source_branch: &str,
) -> Option<ProdDiffStat> {
    // fetch 远端生产分支,确保 diff 基线最新
    let _ = git(
        project_path,
        &["fetch", "origin", target_branch],
        60_000,
        COMMAND_OUTPUT_LIMIT,
    )
    .await;
    let base_ref = format!("origin/{target_branch}");
    // 解析需求分支 ref:先本地,再 origin/<source>
    let local_ref = format!("{source_branch}^{{commit}}");
    let local = git(
        project_path,
        &["rev-parse", "--verify", &local_ref],
        30_000,
        COMMAND_OUTPUT_LIMIT,
    )
    .await;
    let source_ref = if local.ok {
        source_branch.to_string()
    } else {
        let remote = format!("origin/{source_branch}");
        let remote_ref = format!("{remote}^{{commit}}");
        let r = git(
            project_path,
            &["rev-parse", "--verify", &remote_ref],
            30_000,
            COMMAND_OUTPUT_LIMIT,
        )
        .await;
        if r.ok {
            remote
        } else {
            return None;
        }
    };
    let range = format!("{base_ref}...{source_ref}");
    let numstat = git(
        project_path,
        &["diff", "--numstat", "--find-renames", &range, "--"],
        30_000,
        COMMAND_OUTPUT_LIMIT,
    )
    .await;
    if !numstat.ok {
        return None;
    }
    let mut files = 0i64;
    let mut additions = 0i64;
    let mut deletions = 0i64;
    for line in numstat.stdout.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        files += 1;
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 2 {
            if let Ok(a) = parts[0].parse::<i64>() {
                additions += a;
            }
            if let Ok(d) = parts[1].parse::<i64>() {
                deletions += d;
            }
        }
    }
    Some(ProdDiffStat {
        files,
        additions,
        deletions,
        no_diff: files == 0,
    })
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
            let branch_trim = branch.trim();
            // 先计算需求分支相较于生产分支的差异,无差异则跳过 MR 生成
            let stat = if branch_trim.is_empty() {
                None
            } else {
                compute_prod_diff_stat(&project_path, &target_branch, branch_trim).await
            };
            if let Some(s) = &stat {
                if s.no_diff {
                    let mut v = prod_mr_result(
                        repo,
                        branch_trim,
                        &target_branch,
                        "no_diff",
                        None,
                        Some(&format!("相较于生产分支 {target_branch} 无差异,未生成 MR")),
                        Some(project_path.to_string_lossy().as_ref()),
                    );
                    if let Some(obj) = v.as_object_mut() {
                        obj.insert("diffFiles".into(), json!(s.files));
                        obj.insert("diffAdditions".into(), json!(s.additions));
                        obj.insert("diffDeletions".into(), json!(s.deletions));
                    }
                    results.push(v);
                    continue;
                }
            }
            let mut v = create_or_reuse_gitlab_mr(
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
            .await;
            if let (Some(s), Some(obj)) = (&stat, v.as_object_mut()) {
                obj.insert("diffFiles".into(), json!(s.files));
                obj.insert("diffAdditions".into(), json!(s.additions));
                obj.insert("diffDeletions".into(), json!(s.deletions));
            }
            results.push(v);
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

fn normalize_merge_target(raw: &str) -> ApiResult<String> {
    let target = raw.trim().to_lowercase();
    match target.as_str() {
        "test" | "uat" => Ok(target),
        _ => Err(ApiError::bad_request("target must be test or uat")),
    }
}

#[derive(Debug, Clone)]
struct MergeRequest {
    target: String,
    target_branch: String,
    repo_kind: Option<String>,
}

fn normalize_merge_request(form: &MergeBranchForm) -> ApiResult<MergeRequest> {
    let explicit_branch = form
        .target_branch
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_string);
    let target = if let Some(target) = form
        .target
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
    {
        normalize_merge_target(target)?
    } else if let Some(branch) = explicit_branch.as_deref() {
        target_from_branch(branch)?
    } else {
        return Err(ApiError::bad_request("targetBranch is required"));
    };
    let target_branch = explicit_branch
        .unwrap_or_else(|| if target == "test" { "test" } else { "uat" }.to_string());
    let repo_kind = form
        .repo_kind
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(normalize_repo_kind)
        .transpose()?;
    Ok(MergeRequest {
        target,
        target_branch,
        repo_kind,
    })
}

fn target_from_branch(branch: &str) -> ApiResult<String> {
    if branch == "test" {
        Ok("test".to_string())
    } else if branch == "master" || branch.starts_with("UAT-") {
        Ok("uat".to_string())
    } else {
        Err(ApiError::bad_request(
            "targetBranch must be test, master, or UAT-*",
        ))
    }
}

fn normalize_repo_kind(raw: &str) -> ApiResult<String> {
    let kind = raw.trim().to_lowercase();
    match kind.as_str() {
        "frontend" | "front" | "web" | "前端" => Ok("frontend".to_string()),
        "backend" | "back" | "server" | "后端" => Ok("backend".to_string()),
        _ => Err(ApiError::bad_request(
            "repoKind must be frontend or backend",
        )),
    }
}

fn merge_option_target(branch: &str) -> &'static str {
    if branch == "test" {
        "test"
    } else {
        "uat"
    }
}

fn merge_option_label(kind: &str, branch: &str) -> String {
    match (kind, branch) {
        ("frontend", "test") => "前端 test".to_string(),
        ("frontend", "master") => "前端 UAT (master)".to_string(),
        ("backend", "test") => "后端 test".to_string(),
        ("backend", branch) if branch.starts_with("UAT-") => format!("后端 UAT ({branch})"),
        _ => branch.to_string(),
    }
}

fn default_merge_selection(status: &str, kind: &str, options: &[String]) -> Option<String> {
    match status {
        "自测中" => options.iter().find(|v| v.as_str() == "test").cloned(),
        "测试中" if kind == "frontend" => {
            options.iter().find(|v| v.as_str() == "master").cloned()
        }
        "测试中" if kind == "backend" => options.iter().find(|v| v.starts_with("UAT-")).cloned(),
        _ => None,
    }
}

async fn build_merge_options(scope: &BranchScope, req_status: &str) -> Value {
    let mut has_frontend = false;
    let mut has_backend = false;
    let mut backend_uat: Option<String> = None;
    for repo in &scope.repos {
        if is_pda_client_repo(repo) {
            continue;
        }
        let Some(project_path) =
            resolve_code_review_project_path(repo.path.as_deref(), &repo.repo_name)
        else {
            continue;
        };
        if is_frontend_repo(repo) {
            has_frontend = true;
        } else {
            has_backend = true;
            if backend_uat.is_none() && project_path.exists() {
                backend_uat = detect_latest_uat_branch(&project_path).await;
            }
        }
    }
    let frontend_branches = if has_frontend {
        vec!["test".to_string(), "master".to_string()]
    } else {
        Vec::new()
    };
    let mut backend_branches = if has_backend {
        vec!["test".to_string()]
    } else {
        Vec::new()
    };
    if let Some(branch) = backend_uat {
        backend_branches.push(branch);
    }
    json!({
        "frontend": merge_options_for_kind("frontend", &frontend_branches, req_status),
        "backend": merge_options_for_kind("backend", &backend_branches, req_status),
    })
}

fn merge_options_for_kind(kind: &str, branches: &[String], req_status: &str) -> Value {
    let values = branches
        .iter()
        .map(|branch| {
            json!({
                "value": branch,
                "label": merge_option_label(kind, branch),
                "target": merge_option_target(branch),
            })
        })
        .collect::<Vec<_>>();
    json!({
        "repoKind": kind,
        "options": values,
        "defaultValue": default_merge_selection(req_status, kind, branches),
    })
}

fn merge_overall_status(results: &[Value]) -> &'static str {
    let mut has_conflict = false;
    let mut has_failed = false;
    let mut has_merged = false;
    let mut has_skipped = false;
    let mut has_pending = false;
    let mut has_idle = false;
    for item in results {
        match item
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or_default()
        {
            "conflict" => has_conflict = true,
            "failed" => has_failed = true,
            "merged" | "upToDate" => has_merged = true,
            "skipped" => has_skipped = true,
            "pending" => has_pending = true,
            "idle" => has_idle = true,
            _ => {}
        }
    }
    if has_conflict {
        "conflict"
    } else if has_merged && (has_failed || has_pending) {
        "partial"
    } else if has_failed {
        "failed"
    } else if has_pending {
        "pending"
    } else if has_merged {
        "merged"
    } else if has_skipped && !has_idle {
        "skipped"
    } else if has_idle {
        "idle"
    } else {
        "empty"
    }
}

async fn merge_requirement_branches(scope: &BranchScope, request: &MergeRequest) -> Vec<Value> {
    let mut results = Vec::new();
    for repo in &scope.repos {
        if let Some(kind) = request.repo_kind.as_deref() {
            if repo_kind(repo) != kind {
                continue;
            }
        }
        let branches = if repo.branches.is_empty() {
            vec![String::new()]
        } else {
            repo.branches.clone()
        };
        for branch in branches {
            results.push(merge_repo_branch(repo, &branch, request).await);
        }
    }
    results
}

async fn inspect_requirement_merge_status(
    scope: &BranchScope,
    target: Option<String>,
) -> Vec<Value> {
    let targets = match target.as_deref() {
        Some("test") | Some("uat") => vec![target.unwrap()],
        _ => vec!["test".to_string(), "uat".to_string()],
    };
    let mut results = Vec::new();
    for repo in &scope.repos {
        let branches = if repo.branches.is_empty() {
            vec![String::new()]
        } else {
            repo.branches.clone()
        };
        for target in &targets {
            for branch in &branches {
                results.push(inspect_repo_merge_status(repo, branch, target).await);
            }
        }
    }
    results
}

async fn merge_repo_branch(
    repo: &BranchRepo,
    source_branch: &str,
    request: &MergeRequest,
) -> Value {
    let target = request.target.as_str();
    let source_branch = source_branch.trim();
    let Some(project_path) =
        resolve_code_review_project_path(repo.path.as_deref(), &repo.repo_name)
    else {
        return merge_result(
            repo,
            source_branch,
            target,
            None,
            "failed",
            "branches.json 缺少 path",
            None,
            Vec::new(),
            Vec::new(),
            Vec::new(),
        );
    };
    if !project_path.exists() {
        return merge_result(
            repo,
            source_branch,
            target,
            None,
            "failed",
            &format!("仓库路径不存在：{}", project_path.to_string_lossy()),
            Some(&project_path),
            Vec::new(),
            Vec::new(),
            Vec::new(),
        );
    }
    if source_branch.is_empty() {
        return merge_result(
            repo,
            "(未指定分支)",
            target,
            None,
            "skipped",
            "branches.json 缺少需求分支",
            Some(&project_path),
            Vec::new(),
            Vec::new(),
            Vec::new(),
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
        return merge_result(
            repo,
            source_branch,
            target,
            None,
            "failed",
            "projectPath 不是 Git 仓库",
            Some(&project_path),
            Vec::new(),
            Vec::new(),
            vec![git_root.command],
        );
    }

    let target_branch = if request.target_branch == "uat" {
        let Some(resolved) = merge_target_branch_for_repo(repo, target, &project_path).await else {
            return merge_result(
                repo,
                source_branch,
                target,
                None,
                "skipped",
                "当前仓库不适用该环境分支合并；如需启用，请在下拉框选择具体分支",
                Some(&project_path),
                Vec::new(),
                Vec::new(),
                Vec::new(),
            );
        };
        resolved
    } else {
        request.target_branch.clone()
    };
    if !target_branch_matches_repo(repo, &target_branch) {
        return merge_result(
            repo,
            source_branch,
            target,
            None,
            "skipped",
            "所选目标分支不适用于当前仓库类型",
            Some(&project_path),
            Vec::new(),
            Vec::new(),
            Vec::new(),
        );
    }
    let worktree_path = merge_worktree_path(&project_path, target, source_branch, &target_branch);
    if worktree_path.exists() {
        let existing = inspect_merge_worktree(
            repo,
            source_branch,
            target,
            &target_branch,
            &project_path,
            &worktree_path,
        )
        .await;
        let existing_status = existing
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if matches!(existing_status, "conflict" | "pending") {
            return existing;
        }
        let remove = git(
            &project_path,
            &[
                "worktree",
                "remove",
                "--force",
                worktree_path.to_string_lossy().as_ref(),
            ],
            30_000,
            COMMAND_OUTPUT_LIMIT,
        )
        .await;
        if !remove.ok {
            return merge_result(
                repo,
                source_branch,
                target,
                Some(&target_branch),
                "failed",
                &format!("旧 merge worktree 清理失败：{}", short_err(&remove)),
                Some(&project_path),
                Vec::new(),
                vec![worktree_path.to_string_lossy().to_string()],
                vec![remove.command],
            );
        }
    }

    let mut warnings = Vec::new();
    for branch_to_fetch in [&target_branch, source_branch] {
        let fetch = git(
            &project_path,
            &["fetch", "origin", branch_to_fetch],
            60_000,
            COMMAND_OUTPUT_LIMIT,
        )
        .await;
        if !fetch.ok {
            warnings.push(format!(
                "fetch {branch_to_fetch} 失败：{}",
                short_err(&fetch)
            ));
        }
    }
    let Some(target_ref) = resolve_branch_ref(&project_path, &target_branch).await else {
        return merge_result(
            repo,
            source_branch,
            target,
            Some(&target_branch),
            "failed",
            &format!("无法解析目标分支 {target_branch}"),
            Some(&project_path),
            Vec::new(),
            warnings,
            Vec::new(),
        );
    };
    let Some(source_ref) = resolve_branch_ref(&project_path, source_branch).await else {
        return merge_result(
            repo,
            source_branch,
            target,
            Some(&target_branch),
            "failed",
            &format!("无法解析需求分支 {source_branch}"),
            Some(&project_path),
            Vec::new(),
            warnings,
            Vec::new(),
        );
    };

    if let Err(err) = fs::create_dir_all(worktree_path.parent().unwrap_or(&project_path)).await {
        return merge_result(
            repo,
            source_branch,
            target,
            Some(&target_branch),
            "failed",
            &format!("创建 merge worktree 目录失败：{err}"),
            Some(&project_path),
            Vec::new(),
            warnings,
            Vec::new(),
        );
    }
    let temp_branch = merge_temp_branch(target, source_branch, &target_branch);
    let add = git(
        &project_path,
        &[
            "worktree",
            "add",
            "-B",
            &temp_branch,
            worktree_path.to_string_lossy().as_ref(),
            &target_ref,
        ],
        60_000,
        COMMAND_OUTPUT_LIMIT,
    )
    .await;
    if !add.ok {
        return merge_result(
            repo,
            source_branch,
            target,
            Some(&target_branch),
            "failed",
            &format!("创建 merge worktree 失败：{}", short_err(&add)),
            Some(&project_path),
            Vec::new(),
            warnings,
            vec![add.command],
        );
    }
    let merge = git(
        &worktree_path,
        &["merge", "--no-ff", "--no-edit", &source_ref],
        120_000,
        COMMAND_OUTPUT_LIMIT,
    )
    .await;
    if !merge.ok {
        let conflicts = conflicted_files(&worktree_path).await;
        let mut commands = vec![add.command.clone(), merge.command.clone()];
        let status = if conflicts.is_empty() {
            "failed"
        } else {
            "conflict"
        };
        let message = if conflicts.is_empty() {
            format!("合并失败：{}", short_err(&merge))
        } else {
            format!("合并冲突：{} 个文件需要处理", conflicts.len())
        };
        if conflicts.is_empty() {
            commands
                .extend(cleanup_merge_worktree(&project_path, &worktree_path, &temp_branch).await);
        }
        return merge_result(
            repo,
            source_branch,
            target,
            Some(&target_branch),
            status,
            &message,
            Some(&project_path),
            conflicts,
            warnings,
            commands,
        );
    }
    let push_ref = format!("HEAD:refs/heads/{target_branch}");
    let push = git(
        &worktree_path,
        &["push", "origin", &push_ref],
        120_000,
        COMMAND_OUTPUT_LIMIT,
    )
    .await;
    if !push.ok {
        let mut commands = vec![
            add.command.clone(),
            merge.command.clone(),
            push.command.clone(),
        ];
        commands.extend(cleanup_merge_worktree(&project_path, &worktree_path, &temp_branch).await);
        return merge_result(
            repo,
            source_branch,
            target,
            Some(&target_branch),
            "failed",
            &format!("推送目标分支失败：{}", short_err(&push)),
            Some(&project_path),
            Vec::new(),
            warnings,
            commands,
        );
    }
    let cleanup_commands =
        cleanup_merge_worktree(&project_path, &worktree_path, &temp_branch).await;
    if cleanup_commands.iter().any(|cmd| cmd.contains(" failed:")) {
        warnings.push("合并已推送，但临时 worktree/分支清理不完整".to_string());
    }
    let status = if merge.stdout.contains("Already up to date")
        || merge.stdout.contains("Already up-to-date")
    {
        "upToDate"
    } else {
        "merged"
    };
    merge_result(
        repo,
        source_branch,
        target,
        Some(&target_branch),
        status,
        "合并并推送完成",
        Some(&project_path),
        Vec::new(),
        warnings,
        vec![
            add.command,
            merge.command,
            push.command,
            cleanup_commands.join(" && "),
        ],
    )
}

async fn inspect_repo_merge_status(repo: &BranchRepo, source_branch: &str, target: &str) -> Value {
    let Some(project_path) =
        resolve_code_review_project_path(repo.path.as_deref(), &repo.repo_name)
    else {
        return merge_result(
            repo,
            source_branch,
            target,
            None,
            "failed",
            "branches.json 缺少 path",
            None,
            Vec::new(),
            Vec::new(),
            Vec::new(),
        );
    };
    if !project_path.exists() {
        return merge_result(
            repo,
            source_branch,
            target,
            None,
            "failed",
            &format!("仓库路径不存在：{}", project_path.to_string_lossy()),
            Some(&project_path),
            Vec::new(),
            Vec::new(),
            Vec::new(),
        );
    }
    if source_branch.trim().is_empty() {
        return merge_result(
            repo,
            "(未指定分支)",
            target,
            None,
            "skipped",
            "branches.json 缺少需求分支",
            Some(&project_path),
            Vec::new(),
            Vec::new(),
            Vec::new(),
        );
    }
    let Some(target_branch) = merge_target_branch_for_repo(repo, target, &project_path).await
    else {
        return merge_result(
            repo,
            source_branch,
            target,
            None,
            "skipped",
            "当前仓库不适用该环境分支合并",
            Some(&project_path),
            Vec::new(),
            Vec::new(),
            Vec::new(),
        );
    };
    let worktree_path = merge_worktree_path(&project_path, target, source_branch, &target_branch);
    inspect_merge_worktree(
        repo,
        source_branch,
        target,
        &target_branch,
        &project_path,
        &worktree_path,
    )
    .await
}

async fn inspect_merge_worktree(
    repo: &BranchRepo,
    source_branch: &str,
    target: &str,
    target_branch: &str,
    project_path: &Path,
    worktree_path: &Path,
) -> Value {
    if !worktree_path.exists() {
        return merge_result(
            repo,
            source_branch,
            target,
            Some(target_branch),
            "idle",
            "暂无未完成合并",
            Some(project_path),
            Vec::new(),
            Vec::new(),
            Vec::new(),
        );
    }
    let conflicts = conflicted_files(worktree_path).await;
    if conflicts.is_empty() {
        let status = git(
            worktree_path,
            &["status", "--porcelain"],
            30_000,
            COMMAND_OUTPUT_LIMIT,
        )
        .await;
        let message = if status.ok && status.stdout.trim().is_empty() {
            "merge worktree 存在但无未提交变更"
        } else {
            "merge worktree 存在，待人工检查"
        };
        return merge_result(
            repo,
            source_branch,
            target,
            Some(target_branch),
            "pending",
            message,
            Some(project_path),
            Vec::new(),
            Vec::new(),
            vec![status.command],
        );
    }
    merge_result(
        repo,
        source_branch,
        target,
        Some(target_branch),
        "conflict",
        &format!("合并冲突：{} 个文件需要处理", conflicts.len()),
        Some(project_path),
        conflicts,
        Vec::new(),
        Vec::new(),
    )
}

fn merge_result(
    repo: &BranchRepo,
    source_branch: &str,
    target: &str,
    target_branch: Option<&str>,
    status: &str,
    message: &str,
    project_path: Option<&Path>,
    conflict_files: Vec<String>,
    warnings: Vec<String>,
    commands: Vec<String>,
) -> Value {
    let worktree_path = project_path
        .and_then(|path| {
            target_branch.map(|branch| merge_worktree_path(path, target, source_branch, branch))
        })
        .map(|path| path.to_string_lossy().to_string());
    json!({
        "repoName": repo.repo_name,
        "role": repo.role,
        "projectPath": project_path.map(|p| p.to_string_lossy().to_string()),
        "sourceBranch": source_branch,
        "target": target,
        "targetBranch": target_branch,
        "status": status,
        "message": message,
        "conflictFiles": conflict_files,
        "worktreePath": worktree_path,
        "warnings": warnings,
        "commands": commands,
    })
}

async fn merge_target_branch_for_repo(
    repo: &BranchRepo,
    target: &str,
    project_path: &Path,
) -> Option<String> {
    let explicit = if target == "test" {
        repo.test_target_branch.as_deref()
    } else if target == "uat" {
        repo.uat_target_branch.as_deref()
    } else {
        None
    }
    .map(str::trim)
    .filter(|v| !v.is_empty())
    .map(str::to_string);
    if explicit.is_some() {
        return explicit;
    }
    if is_pda_client_repo(repo) {
        return None;
    }
    if target == "test" {
        return Some("test".to_string());
    }
    if target != "uat" {
        return None;
    }
    if is_frontend_repo(repo) {
        return Some("master".to_string());
    }
    detect_latest_uat_branch(project_path).await
}

fn is_frontend_repo(repo: &BranchRepo) -> bool {
    let role = repo.role.as_deref().unwrap_or_default();
    let path = repo.path.as_deref().unwrap_or_default();
    role.contains("前端") || path.contains("/frontend/") || path.contains("\\frontend\\")
}

fn is_pda_client_repo(repo: &BranchRepo) -> bool {
    let role = repo.role.as_deref().unwrap_or_default();
    let path = repo.path.as_deref().unwrap_or_default();
    role == "PDA" || path.contains("/pda/") || path.contains("\\pda\\")
}

fn repo_kind(repo: &BranchRepo) -> &'static str {
    if is_pda_client_repo(repo) {
        "pda"
    } else if is_frontend_repo(repo) {
        "frontend"
    } else {
        "backend"
    }
}

fn target_branch_matches_repo(repo: &BranchRepo, target_branch: &str) -> bool {
    if is_pda_client_repo(repo) {
        return false;
    }
    if is_frontend_repo(repo) {
        matches!(target_branch, "test" | "master")
    } else {
        target_branch == "test" || target_branch.starts_with("UAT-")
    }
}

async fn detect_latest_uat_branch(project_path: &Path) -> Option<String> {
    let _ = git(
        project_path,
        &[
            "fetch",
            "origin",
            "+refs/heads/UAT-*:refs/remotes/origin/UAT-*",
        ],
        60_000,
        COMMAND_OUTPUT_LIMIT,
    )
    .await;
    let result = git(
        project_path,
        &[
            "for-each-ref",
            "--sort=-committerdate",
            "--format=%(refname:short)",
            "refs/remotes/origin/UAT-*",
        ],
        30_000,
        COMMAND_OUTPUT_LIMIT,
    )
    .await;
    if !result.ok {
        return None;
    }
    result
        .stdout
        .lines()
        .map(str::trim)
        .find(|line| line.starts_with("origin/UAT-"))
        .map(|line| line.trim_start_matches("origin/").to_string())
}

async fn resolve_branch_ref(project_path: &Path, branch: &str) -> Option<String> {
    let remote_branch = format!("origin/{branch}");
    let remote_ref = format!("{}^{{commit}}", remote_branch);
    let remote = git(
        project_path,
        &["rev-parse", "--verify", &remote_ref],
        30_000,
        COMMAND_OUTPUT_LIMIT,
    )
    .await;
    if remote.ok {
        return Some(remote_branch);
    }
    let local_ref = format!("{}^{{commit}}", branch);
    let local = git(
        project_path,
        &["rev-parse", "--verify", &local_ref],
        30_000,
        COMMAND_OUTPUT_LIMIT,
    )
    .await;
    local.ok.then_some(branch.to_string())
}

async fn conflicted_files(worktree_path: &Path) -> Vec<String> {
    let result = git(
        worktree_path,
        &["diff", "--name-only", "--diff-filter=U"],
        30_000,
        COMMAND_OUTPUT_LIMIT,
    )
    .await;
    if !result.ok {
        return Vec::new();
    }
    result
        .stdout
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect()
}

async fn cleanup_merge_worktree(
    project_path: &Path,
    worktree_path: &Path,
    temp_branch: &str,
) -> Vec<String> {
    let remove = git(
        &project_path,
        &[
            "worktree",
            "remove",
            "--force",
            worktree_path.to_string_lossy().as_ref(),
        ],
        30_000,
        COMMAND_OUTPUT_LIMIT,
    )
    .await;
    let delete_branch = git(
        &project_path,
        &["branch", "-D", temp_branch],
        30_000,
        COMMAND_OUTPUT_LIMIT,
    )
    .await;
    vec![
        if remove.ok {
            remove.command
        } else {
            format!("{} failed: {}", remove.command, short_err(&remove))
        },
        if delete_branch.ok {
            delete_branch.command
        } else {
            format!(
                "{} failed: {}",
                delete_branch.command,
                short_err(&delete_branch)
            )
        },
    ]
}

fn merge_worktree_path(
    project_path: &Path,
    target: &str,
    source_branch: &str,
    target_branch: &str,
) -> PathBuf {
    let repo_leaf = project_path
        .file_name()
        .map(|v| v.to_string_lossy().to_string())
        .unwrap_or_else(|| "repo".to_string());
    project_path
        .parent()
        .unwrap_or(project_path)
        .join(".agent-panel-merge-worktrees")
        .join(repo_leaf)
        .join(target)
        .join(format!(
            "{}__{}",
            sanitize_ref_segment(target_branch),
            sanitize_ref_segment(source_branch)
        ))
}

fn merge_temp_branch(target: &str, source_branch: &str, target_branch: &str) -> String {
    format!(
        "agent-panel/merge/{}/{}/{}",
        sanitize_ref_segment(target),
        sanitize_ref_segment(target_branch),
        sanitize_ref_segment(source_branch)
    )
}

fn sanitize_ref_segment(value: &str) -> String {
    let mut out = String::new();
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
            out.push(ch);
        } else {
            out.push('-');
        }
    }
    let compact = out.trim_matches('-').to_string();
    if compact.is_empty() {
        "branch".to_string()
    } else {
        compact
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
    // fetch 远端需求分支,确保 origin/<branch> 作为回退 ref 时是最新的
    // (本地分支存在时扫描优先用本地分支,此 fetch 无副作用)
    let _ = git(
        &project_path,
        &["fetch", "origin", branch],
        60_000,
        COMMAND_OUTPUT_LIMIT,
    )
    .await;

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
        .and_then(normalize_status_value)
        .unwrap_or_else(|| "需求澄清".to_string());
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
        let status = canonical_status(status)?;
        if req.status != "测试中" && status == "测试中" {
            ensure_review_gate_allows_testing(&req).await?;
        }
        changes.push("state.status".into());
        if !dry_run {
            let st = write_requirement_status(
                dir.to_string_lossy().as_ref(),
                &status,
                form.note.as_deref(),
            )
            .await?;
            if !matches!(st.get("changed").and_then(Value::as_bool), Some(false)) {
                record_status_transition_event(state, &req, &st, form.note.as_deref()).await?;
                planned_files.push(
                    dir.join(REQUIREMENT_EVENTS_FILE)
                        .to_string_lossy()
                        .to_string(),
                );
            }
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

async fn record_requirement_event(
    state: &AppState,
    form: RequirementEventForm,
) -> ApiResult<Value> {
    let req = get_real_requirement(state, &form.req_id).await?;
    let dir = req_dir_path(&req)?;
    ensure_requirement_dir_writable(state, &dir).await?;
    let event_type = normalize_requirement_event_type(form.event_type.as_deref());
    let title = clean_optional(form.title.as_deref())
        .or_else(|| clean_optional(form.summary.as_deref()))
        .unwrap_or_else(|| requirement_event_label(&event_type).to_string());
    let summary = clean_optional(form.summary.as_deref()).unwrap_or_else(|| title.clone());
    ensure_text_size(&summary, "summary")?;
    let details = clean_optional(form.details.as_deref()).unwrap_or_default();
    ensure_text_size(&details, "details")?;
    let dry_run = form.dry_run.unwrap_or(false);
    let append_note = form.append_note.unwrap_or(true);
    let event_id = clean_optional(form.idempotency_key.as_deref())
        .unwrap_or_else(|| format!("{}-{}", now_ms(), Uuid::new_v4()));
    let event = json!({
        "id": event_id,
        "reqId": req.id,
        "type": event_type,
        "title": title,
        "summary": summary,
        "details": details,
        "evidence": clean_string_vec(form.evidence),
        "decisions": clean_string_vec(form.decisions),
        "todos": clean_string_vec(form.todos),
        "relatedFiles": clean_string_vec(form.related_files),
        "testCases": form.test_cases,
        "status": clean_optional(form.status.as_deref()),
        "riskLevel": clean_optional(form.risk_level.as_deref()),
        "tags": clean_string_vec(form.tags),
        "sessionId": clean_optional(form.session_id.as_deref()),
        "createdAt": now_ms(),
    });
    let events_path = dir.join(REQUIREMENT_EVENTS_FILE);
    let note_text = render_requirement_event_note(&event);
    let mut files = vec![events_path.to_string_lossy().to_string()];
    if append_note {
        files.push(dir.join("notes.md").to_string_lossy().to_string());
    }
    if !dry_run {
        let existing = fs::read_to_string(&events_path).await.unwrap_or_default();
        let event_already_exists = requirement_event_exists(
            &existing,
            event.get("id").and_then(Value::as_str).unwrap_or_default(),
        );
        if !event_already_exists {
            let line = serde_json::to_string(&event)?;
            let next = if existing.trim().is_empty() {
                format!("{}\n", line)
            } else {
                format!("{}\n{}\n", existing.trim_end(), line)
            };
            atomic_write_text(&events_path, &next).await?;
            if append_note {
                append_requirement_note(
                    state,
                    RequirementNoteForm {
                        req_id: req.id.clone(),
                        text: note_text.clone(),
                        title: Some(format!("事件：{}", title)),
                        session_id: clean_optional(form.session_id.as_deref()),
                        dry_run: Some(false),
                    },
                )
                .await?;
            }
        }
    }
    Ok(json!({
        "ok": true,
        "dryRun": dry_run,
        "reqId": req.id,
        "operation": "recordEvent",
        "event": event,
        "notePreview": note_text,
        "files": unique_strings(files),
    }))
}

fn requirement_section_form_to_edit(
    section: String,
    form: RequirementSectionForm,
) -> ApiResult<RequirementEditForm> {
    let doc_type = form
        .doc_type
        .or_else(|| {
            form.token
                .as_deref()
                .and_then(requirement_doc_type_for_token)
                .map(str::to_string)
        })
        .or_else(|| requirement_section_default_doc_type(&section).map(str::to_string));
    let heading = form
        .heading
        .or_else(|| Some(requirement_section_default_heading(&section).to_string()));
    Ok(RequirementEditForm {
        req_id: form.req_id,
        operation: "upsertSection".to_string(),
        token: None,
        doc_type,
        content: Some(form.content),
        text: None,
        title: None,
        heading,
        mode: None,
        status: None,
        category: None,
        note: None,
        session_id: None,
        fields: None,
        dry_run: form.dry_run,
    })
}

fn requirement_section_default_doc_type(section: &str) -> Option<&'static str> {
    let s = normalize_section_key(section);
    if matches!(
        s.as_str(),
        "test" | "tests" | "selftest" | "uat" | "testcase" | "testcases"
    ) {
        Some("test")
    } else if matches!(
        s.as_str(),
        "impact" | "risk" | "risks" | "rootcause" | "boxcodeissue" | "issue"
    ) {
        Some("impact")
    } else if matches!(
        s.as_str(),
        "background" | "design" | "scope" | "decision" | "decisions"
    ) {
        Some("background")
    } else if matches!(s.as_str(), "memory" | "summary" | "agentcontext") {
        Some("memory")
    } else if matches!(s.as_str(), "config" | "configchanges") {
        Some("config-changes")
    } else if matches!(s.as_str(), "release" | "manifest" | "releasemanifest") {
        Some("release-manifest")
    } else if matches!(s.as_str(), "review" | "codereview") {
        Some("review")
    } else if matches!(s.as_str(), "notes" | "note" | "progress") {
        Some("notes")
    } else {
        None
    }
}

fn requirement_section_default_heading(section: &str) -> &str {
    let s = normalize_section_key(section);
    match s.as_str() {
        "boxcodeissue" => "boxCode 问题",
        "rootcause" => "根因分析",
        "test" | "tests" | "testcase" | "testcases" => "测试场景",
        "selftest" => "自测证据",
        "uat" => "UAT 验证",
        "impact" => "影响面评估",
        "risk" | "risks" => "风险与回滚",
        "decision" | "decisions" => "关键决策",
        "summary" | "agentcontext" => "Agent 摘要",
        "config" | "configchanges" => "配置变更",
        "release" | "manifest" | "releasemanifest" => "上线清单",
        "review" | "codereview" => "代码审查结论",
        "progress" => "进展记录",
        _ => section.trim(),
    }
}

fn normalize_section_key(value: &str) -> String {
    value
        .trim()
        .trim_start_matches("req.")
        .trim_end_matches(".md")
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect::<String>()
        .to_lowercase()
}

fn normalize_requirement_event_type(raw: Option<&str>) -> String {
    let s = raw.unwrap_or("note").trim().to_lowercase();
    match s.as_str() {
        "issue" | "issue_found" | "bug" | "problem" | "问题" => "issueFound".to_string(),
        "root_cause" | "rootcause" | "cause" | "根因" => "rootCause".to_string(),
        "workaround" | "mitigation" | "临时方案" | "治标" => "workaround".to_string(),
        "fix" | "solution" | "方案" | "修复" => "solution".to_string(),
        "test" | "test_result" | "验证" | "自测" => "testResult".to_string(),
        "decision" | "决策" => "decision".to_string(),
        "todo" | "next" | "后续" => "todo".to_string(),
        "risk" | "风险" => "risk".to_string(),
        "status_transition" | "statustransition" | "phase_transition" | "phasetransition"
        | "状态切换" | "阶段切换" => "statusTransition".to_string(),
        "progress" | "进展" => "progress".to_string(),
        _ => s.replace(['-', '_'], ""),
    }
}

fn requirement_event_label(event_type: &str) -> &str {
    match event_type {
        "issueFound" => "发现问题",
        "rootCause" => "根因确认",
        "workaround" => "治标方案",
        "solution" => "方案落地",
        "testResult" => "测试验证",
        "decision" => "关键决策",
        "todo" => "后续事项",
        "risk" => "风险记录",
        "statusTransition" => "状态切换",
        "progress" => "进展记录",
        _ => "需求事件",
    }
}

fn clean_string_vec(values: Vec<String>) -> Vec<String> {
    values
        .into_iter()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .collect()
}

async fn record_status_transition_event(
    state: &AppState,
    req: &Requirement,
    status_state: &Value,
    note: Option<&str>,
) -> ApiResult<()> {
    let to = status_state
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or(req.status.as_str());
    let from = status_state
        .get("previousStatus")
        .and_then(Value::as_str)
        .unwrap_or("未记录");
    let skipped: Vec<String> = status_state
        .get("lastTransition")
        .and_then(|v| v.get("skippedStatuses"))
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    let at = status_state
        .get("lastTransition")
        .and_then(|v| v.get("at"))
        .and_then(Value::as_i64)
        .unwrap_or_else(now_ms);
    let mut details = vec![
        format!("状态从 `{from}` 切换到 `{to}`。"),
        "Agent 后续应刷新 `/api/requirement/context?for=agent`，使用最新 `phaseRuntime.currentPhasePrompt`，不要继续沿用 session 创建时的阶段提示词。".to_string(),
    ];
    if !skipped.is_empty() {
        details.push(format!(
            "本次跳过阶段：{}。这些阶段的 entry checks 会作为风险提示进入 phaseRuntime.phaseGaps。",
            skipped.join("、")
        ));
    }
    if let Some(note) = note.map(str::trim).filter(|v| !v.is_empty()) {
        details.push(format!("备注：{note}"));
    }
    record_requirement_event(
        state,
        RequirementEventForm {
            req_id: req.id.clone(),
            event_type: Some("statusTransition".to_string()),
            title: Some(format!("状态切换：{from} → {to}")),
            summary: Some(format!("状态切换：{from} → {to}")),
            details: Some(details.join("\n")),
            evidence: Vec::new(),
            decisions: Vec::new(),
            todos: if skipped.is_empty() {
                Vec::new()
            } else {
                vec!["补查被跳过阶段的 entry checks，缺失项作为风险或待办记录。".to_string()]
            },
            related_files: vec![STATE_FILE.to_string()],
            test_cases: Vec::new(),
            status: Some(to.to_string()),
            risk_level: if skipped.is_empty() {
                None
            } else {
                Some("medium".to_string())
            },
            tags: vec!["phase".to_string(), "status-transition".to_string()],
            session_id: None,
            idempotency_key: Some(format!("{}-status-{}", req.id, at)),
            append_note: Some(true),
            dry_run: Some(false),
        },
    )
    .await?;
    Ok(())
}

fn requirement_event_exists(raw: &str, id: &str) -> bool {
    if id.is_empty() {
        return false;
    }
    raw.lines().any(|line| {
        serde_json::from_str::<Value>(line)
            .ok()
            .and_then(|v| v.get("id").and_then(Value::as_str).map(str::to_string))
            .map(|existing| existing == id)
            .unwrap_or(false)
    })
}

fn render_requirement_event_note(event: &Value) -> String {
    let mut lines = Vec::new();
    if let Some(summary) = event
        .get("summary")
        .and_then(Value::as_str)
        .filter(|v| !v.trim().is_empty())
    {
        lines.push(format!("- Summary: {}", summary.trim()));
    }
    if let Some(kind) = event.get("type").and_then(Value::as_str) {
        lines.push(format!("- Type: `{}`", kind));
    }
    if let Some(status) = event
        .get("status")
        .and_then(Value::as_str)
        .filter(|v| !v.trim().is_empty())
    {
        lines.push(format!("- Status: {}", status.trim()));
    }
    if let Some(risk) = event
        .get("riskLevel")
        .and_then(Value::as_str)
        .filter(|v| !v.trim().is_empty())
    {
        lines.push(format!("- Risk: {}", risk.trim()));
    }
    push_event_array(&mut lines, event, "evidence", "Evidence");
    push_event_array(&mut lines, event, "decisions", "Decisions");
    push_event_array(&mut lines, event, "todos", "TODO");
    push_event_array(&mut lines, event, "relatedFiles", "Related files");
    if let Some(test_cases) = event
        .get("testCases")
        .and_then(Value::as_array)
        .filter(|v| !v.is_empty())
    {
        lines.push("- Test cases:".to_string());
        for case in test_cases {
            let name = case
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("Unnamed");
            let result = case
                .get("result")
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            let evidence = case.get("evidence").and_then(Value::as_str).unwrap_or("");
            lines.push(if evidence.trim().is_empty() {
                format!("  - {}: {}", name, result)
            } else {
                format!("  - {}: {} ({})", name, result, evidence)
            });
        }
    }
    if let Some(details) = event
        .get("details")
        .and_then(Value::as_str)
        .filter(|v| !v.trim().is_empty())
    {
        lines.push(String::new());
        lines.push(details.trim().to_string());
    }
    lines.join("\n")
}

fn push_event_array(lines: &mut Vec<String>, event: &Value, key: &str, label: &str) {
    if let Some(values) = event
        .get(key)
        .and_then(Value::as_array)
        .filter(|v| !v.is_empty())
    {
        lines.push(format!("- {}:", label));
        for value in values {
            if let Some(text) = value.as_str().map(str::trim).filter(|v| !v.is_empty()) {
                lines.push(format!("  - {}", text));
            }
        }
    }
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
        "version": 3,
        "flow": ["需求澄清", "开发中", "自测中", "测试中", "经验总结", "已完成"],
        "statusValues": REQ_STATUSES,
        "statusAliases": REQ_STATUS_ALIASES,
        "categoryValues": REQ_CATEGORIES,
        "tokens": requirement_token_specs_json(),
        "intents": [
            {"intent": "overview", "readTokens": intent_read_tokens("overview"), "writeTokens": ["req.memory", "req.notes"]},
            {"intent": "clarification", "readTokens": intent_read_tokens("clarification"), "writeTokens": ["req.alignment", "req.background", "req.releaseManifest", "req.impact", "req.memory", "req.notes"]},
            {"intent": "status", "readTokens": intent_read_tokens("status"), "writeTokens": ["req.state", "req.notes"]},
            {"intent": "progress", "readTokens": intent_read_tokens("progress"), "writeTokens": ["req.notes", "req.memory"]},
            {"intent": "branch", "readTokens": intent_read_tokens("branch"), "writeTokens": ["req.branchScope", "req.branch"]},
            {"intent": "self-test", "readTokens": intent_read_tokens("self-test"), "writeTokens": ["req.test", "req.releaseManifest", "req.notes", "req.memory"]},
            {"intent": "release-check", "readTokens": intent_read_tokens("release-check"), "writeTokens": ["req.releaseCheck", "req.releaseManifest", "req.configChanges", "req.test", "req.impact", "req.review", "req.notes"]},
            {"intent": "design", "readTokens": intent_read_tokens("design"), "writeTokens": intent_write_tokens("design")},
            {"intent": "config", "readTokens": intent_read_tokens("config"), "writeTokens": ["req.configChanges", "req.releaseManifest", "req.impact"]},
            {"intent": "review", "readTokens": intent_read_tokens("review"), "writeTokens": ["req.review", "req.notes"]},
            {"intent": "experience-summary", "readTokens": intent_read_tokens("experience-summary"), "writeTokens": ["req.experienceSummary", "req.releaseManifest", "req.memory", "req.notes"]}
        ],
        "operations": [
            {"operation": "setStatus", "writes": ["req.state"], "required": ["reqId", "status"], "optional": ["note", "dryRun"]},
            {"operation": "setCategory", "writes": ["req.state"], "required": ["reqId", "category"], "optional": ["dryRun"]},
            {"operation": "patchMeta", "writes": ["req.meta"], "required": ["reqId", "fields"], "allowedFields": ["title", "project", "owner", "startDate", "planRelease", "ones"]},
            {"operation": "appendNote", "writes": ["req.notes"], "required": ["reqId", "text"], "optional": ["title", "sessionId", "dryRun"]},
            {"operation": "recordEvent", "endpoint": "POST /api/requirement/events", "writes": ["events.jsonl", "req.notes"], "required": ["reqId", "type", "summary"], "optional": ["details", "evidence", "decisions", "todos", "relatedFiles", "testCases", "idempotencyKey", "appendNote", "dryRun"]},
            {"operation": "writeDoc", "writes": ["token/docType"], "required": ["reqId", "token or docType", "content"], "optional": ["mode=replace|append", "dryRun"]},
            {"operation": "upsertSection", "writes": ["token/docType"], "required": ["reqId", "token or docType", "heading", "content"], "optional": ["dryRun"]},
            {"operation": "upsertNamedSection", "endpoint": "POST /api/requirement/sections/{section}", "writes": ["mapped doc section"], "required": ["reqId", "content"], "optional": ["heading", "docType", "token", "dryRun"]}
        ],
        "agentContext": {
            "endpoint": "GET /api/requirement/context?id=<reqId>&for=agent&intent=<intent>&budget=2000",
            "description": "returns compressed summary docs, recent structured events and recommended write APIs"
        },
        "rules": [
            "需求澄清阶段合并旧的需求对齐和方案设计：先读业务知识/经验，再初步调查代码，输出 alignment.md、background.md、impact.md 的最小闭环。",
            "经验总结阶段替代旧待上线状态：识别本次需求暴露的 skill、业务知识、经验和流程改进，并把已落地/待落地区分记录到 experience-summary.md。",
            "Agent should call context with for=agent for most work; use token context only when the compressed summary is insufficient.",
            "Use recordEvent for facts, evidence, test results, decisions and todos; it stores events.jsonl and can append notes.md.",
            "Use sections/{section} or upsertSection for targeted document updates instead of replacing full markdown files.",
            "Agent should call edit-plan before selecting files for non-trivial requirement edits.",
            "state.json is the source of truth for status/category; do not direct-edit it.",
            "Use appendNote for free-form progress logs; avoid replacing notes.md.",
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
            "developer-facing business background, goal, scope, current behavior, key rules and decisions",
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
            "req.releaseManifest",
            "release-manifest.md",
            Some("release-manifest"),
            "always-visible release change manifest: DB tables, configs, topics, groups, jobs, APIs and manual release actions",
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
            "code review and pre-release review summary",
            false,
            vec!["writeDoc", "upsertSection"],
        ),
        token_spec(
            "req.releaseCheck",
            "release-check.md",
            Some("release-check"),
            "release readiness checklist and blocking items",
            false,
            vec!["writeDoc", "upsertSection"],
        ),
        token_spec(
            "req.experienceSummary",
            "experience-summary.md",
            Some("experience-summary"),
            "post-requirement capability evolution summary for knowledge, experience and skill improvements",
            false,
            vec!["writeDoc", "upsertSection"],
        ),
        token_spec(
            "req.alignment",
            "alignment.md",
            Some("alignment"),
            "requirement clarification notes, PRD interpretation and open questions",
            true,
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
    } else if s.contains("经验")
        || s.contains("总结")
        || s.contains("复盘")
        || s.contains("knowledge")
        || s.contains("skill")
        || s.contains("evolve")
    {
        "experience-summary".into()
    } else if s.contains("澄清")
        || s.contains("对齐")
        || s.contains("背景")
        || s.contains("业务")
        || s.contains("prd")
        || s.contains("alignment")
        || s.contains("clarif")
    {
        "clarification".into()
    } else if s.contains("自测")
        || s.contains("测试")
        || s.contains("test")
        || s.contains("evidence")
    {
        "self-test".into()
    } else if s.contains("上线")
        || s.contains("release")
        || s.contains("发布")
        || s.contains("上线清单")
        || s.contains("manifest")
        || s.contains("发布清单")
    {
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
        "clarification".into()
    } else if s.contains("进展") || s.contains("note") || s.contains("progress") {
        "progress".into()
    } else if s.contains("review") || s.contains("cr") || s.contains("代码审查") {
        "review".into()
    } else {
        s
    }
}

fn is_supported_requirement_intent(intent: &str) -> bool {
    matches!(
        intent,
        "overview"
            | "status"
            | "progress"
            | "clarification"
            | "design"
            | "branch"
            | "self-test"
            | "release-check"
            | "config"
            | "experience-summary"
            | "review"
    )
}

fn ensure_requirement_intent(intent: &str) -> ApiResult<()> {
    if is_supported_requirement_intent(intent) {
        Ok(())
    } else {
        Err(ApiError::bad_request(format!("invalid intent: {intent}")))
    }
}

fn intent_read_tokens(intent: &str) -> Vec<&'static str> {
    match intent {
        "status" => vec!["req.meta", "req.state", "req.notes"],
        "progress" => vec!["req.meta", "req.memory", "req.notes"],
        "clarification" | "design" => vec![
            "req.meta",
            "req.prd",
            "req.alignment",
            "req.background",
            "req.memory",
            "req.releaseManifest",
            "req.impact",
            "req.notes",
        ],
        "branch" => vec!["req.meta", "req.branchScope", "req.branch"],
        "self-test" => vec![
            "req.meta",
            "req.memory",
            "req.branch",
            "req.releaseManifest",
            "req.test",
            "req.notes",
        ],
        "release-check" => vec![
            "req.meta",
            "req.state",
            "req.branchScope",
            "req.branch",
            "req.configChanges",
            "req.releaseManifest",
            "req.test",
            "req.impact",
            "req.review",
            "req.releaseCheck",
        ],
        "config" => vec![
            "req.meta",
            "req.configChanges",
            "req.releaseManifest",
            "req.impact",
            "req.notes",
        ],
        "experience-summary" => vec![
            "req.meta",
            "req.background",
            "req.memory",
            "req.notes",
            "req.test",
            "req.impact",
            "req.review",
            "req.releaseCheck",
            "req.releaseManifest",
            "req.experienceSummary",
        ],
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
            "req.alignment",
            "req.releaseManifest",
            "req.impact",
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
        "self-test" => vec!["req.test", "req.releaseManifest", "req.notes", "req.memory"],
        "release-check" => vec![
            "req.releaseCheck",
            "req.releaseManifest",
            "req.configChanges",
            "req.test",
            "req.impact",
            "req.review",
            "req.notes",
        ],
        "config" => vec!["req.configChanges", "req.releaseManifest", "req.impact"],
        "clarification" | "design" => vec![
            "req.alignment",
            "req.background",
            "req.releaseManifest",
            "req.impact",
            "req.memory",
            "req.notes",
        ],
        "review" => vec!["req.review", "req.notes"],
        "experience-summary" => vec![
            "req.experienceSummary",
            "req.releaseManifest",
            "req.memory",
            "req.notes",
        ],
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
        "releasemanifest" | "manifest" | "deploymanifest" => Some("req.releaseManifest"),
        "impact" => Some("req.impact"),
        "test" => Some("req.test"),
        "notes" | "note" => Some("req.notes"),
        "review" => Some("req.review"),
        "releasecheck" => Some("req.releaseCheck"),
        "experiencesummary" | "summary" | "retrospective" => Some("req.experienceSummary"),
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
        "req.releaseManifest" => Some("release-manifest.md"),
        "req.impact" => Some("impact.md"),
        "req.test" => Some("test.md"),
        "req.notes" => Some("notes.md"),
        "req.review" => Some("review.md"),
        "req.releaseCheck" => Some("release-check.md"),
        "req.experienceSummary" => Some("experience-summary.md"),
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
        "req.releaseManifest" => Some("release-manifest"),
        "req.impact" => Some("impact"),
        "req.test" => Some("test"),
        "req.notes" => Some("notes"),
        "req.review" => Some("review"),
        "req.releaseCheck" => Some("release-check"),
        "req.experienceSummary" => Some("experience-summary"),
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

async fn build_requirement_agent_context(
    state: &AppState,
    req: &Requirement,
    intent: &str,
    budget: usize,
    event_limit: usize,
) -> ApiResult<Value> {
    let dir = req_dir_path(req)?;
    let phase_runtime = build_phase_runtime_context(state, req, intent, &dir).await;
    let context_tokens = agent_context_tokens(intent);
    let mut docs = Vec::new();
    let per_doc_budget = (budget / context_tokens.len().max(1)).clamp(300, 2_000);
    for token in context_tokens {
        let Some(file) = requirement_token_file(token) else {
            continue;
        };
        let path = dir.join(file);
        let raw = fs::read_to_string(&path).await.unwrap_or_default();
        let (summary, truncated) = summarize_requirement_doc(&raw, per_doc_budget);
        docs.push(json!({
            "token": token,
            "file": file,
            "docType": requirement_doc_type_for_token(token),
            "exists": path.is_file(),
            "bytes": path.metadata().map(|m| m.len()).unwrap_or(0),
            "truncated": truncated,
            "summary": summary,
        }));
    }
    let events_path = dir.join(REQUIREMENT_EVENTS_FILE);
    let events = read_recent_requirement_events(&events_path, event_limit).await;
    Ok(json!({
        "ok": true,
        "format": "agentRequirementContext.v2",
        "reqId": req.id,
        "title": req.title,
        "status": req.status,
        "project": req.project,
        "projects": req.projects,
        "category": req.category,
        "ones": req.ones,
        "intent": intent,
        "budget": budget,
        "phaseRuntime": phase_runtime,
        "summaryDocs": docs,
        "recentEvents": events,
        "recommendedWrites": recommended_requirement_writes(intent),
        "apis": {
            "recordEvent": "/api/requirement/events",
            "upsertSection": "/api/requirement/sections/{section}",
            "edit": "/api/requirement/edit",
            "validate": "/api/requirement/validate",
            "refreshAgentContext": format!("/api/requirement/context?id={}&for=agent&intent={}&budget={}", req.id, intent, budget)
        },
        "rules": [
            "Treat phaseRuntime as current navigation: it is rebuilt from the requirement's latest state on every context call.",
            "If the session started in an earlier phase, do not keep following the startup prompt; refresh context with for=agent and follow phaseRuntime.currentPhasePrompt.",
            "Skipped phase gaps are risk flags, not hard blockers: record them and continue the user's current task unless a safety gate blocks it.",
            "Prefer recordEvent for facts/status/evidence/decisions; it stores events.jsonl and can append notes.md.",
            "Prefer sections/{section} or upsertSection for targeted impact/test/background updates.",
            "Read full docs only when this compressed context is insufficient."
        ]
    }))
}

fn phase_status_index(status: &str) -> Option<usize> {
    REQ_STATUSES.iter().position(|s| *s == status)
}

fn skipped_statuses(from: Option<&str>, to: &str) -> Vec<String> {
    let Some(from_idx) = from.and_then(phase_status_index) else {
        return Vec::new();
    };
    let Some(to_idx) = phase_status_index(to) else {
        return Vec::new();
    };
    if to_idx <= from_idx + 1 {
        return Vec::new();
    }
    REQ_STATUSES[from_idx + 1..to_idx]
        .iter()
        .map(|s| s.to_string())
        .collect()
}

async fn build_phase_runtime_context(
    state: &AppState,
    req: &Requirement,
    intent: &str,
    dir: &Path,
) -> Value {
    let phase_prompt = load_phase_prompt(state, &req.status).await;
    let state_json = read_requirement_state(dir)
        .await
        .ok()
        .flatten()
        .unwrap_or_else(|| json!({ "version": 1, "status": req.status, "history": [] }));
    let history = state_json
        .get("history")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let transition_path: Vec<Value> = if history.is_empty() {
        vec![json!({ "status": req.status, "source": "current" })]
    } else {
        history
            .iter()
            .filter_map(|item| {
                item.get("status").and_then(Value::as_str).map(|status| {
                    json!({
                        "from": item.get("from").cloned().unwrap_or(Value::Null),
                        "status": status,
                        "at": item.get("at").cloned().unwrap_or(Value::Null),
                        "note": item.get("note").cloned().unwrap_or(Value::Null),
                        "skippedStatuses": item.get("skippedStatuses").cloned().unwrap_or_else(|| json!([]))
                    })
                })
            })
            .collect()
    };
    let skipped_transitions: Vec<Value> = history
        .iter()
        .filter_map(|item| {
            let skipped = item.get("skippedStatuses").and_then(Value::as_array)?;
            if skipped.is_empty() {
                return None;
            }
            Some(json!({
                "from": item.get("from").cloned().unwrap_or(Value::Null),
                "status": item.get("status").cloned().unwrap_or(Value::Null),
                "skippedStatuses": skipped,
                "at": item.get("at").cloned().unwrap_or(Value::Null),
                "note": item.get("note").cloned().unwrap_or(Value::Null),
            }))
        })
        .collect();
    let entry_checks = phase_entry_checks(&req.status, dir);
    let missing_required: Vec<Value> = entry_checks
        .iter()
        .filter(|item| {
            item.get("required")
                .and_then(Value::as_bool)
                .unwrap_or(false)
                && !item.get("ok").and_then(Value::as_bool).unwrap_or(false)
        })
        .cloned()
        .collect();
    let phase_gaps = json!({
        "skippedTransitions": skipped_transitions,
        "missingRequiredEntryChecks": missing_required,
        "policy": "状态跳转允许继续；缺口会作为风险提示注入当前阶段上下文，除安全门禁外不自动回退状态。"
    });
    json!({
        "currentStatus": req.status,
        "intent": intent,
        "recommendedIntent": default_intent_for_status(&req.status),
        "currentPhasePromptFile": phase_prompt_file(&req.status),
        "currentPhasePrompt": phase_prompt.trim(),
        "entryChecks": entry_checks,
        "phaseGaps": phase_gaps,
        "transitionMemory": {
            "source": "state.json.history",
            "path": transition_path,
            "lastTransition": state_json.get("lastTransition").cloned().unwrap_or_else(|| history.last().cloned().unwrap_or(Value::Null)),
            "principle": "当前阶段提示词按最新 status 实时生成；历史阶段只作为摘要和风险，不覆盖当前阶段行为。"
        }
    })
}

fn default_intent_for_status(status: &str) -> &'static str {
    match status {
        "需求澄清" => "clarification",
        "开发中" => "overview",
        "自测中" => "self-test",
        "测试中" => "self-test",
        "经验总结" => "experience-summary",
        "已完成" => "overview",
        _ => "overview",
    }
}

fn phase_entry_checks(status: &str, dir: &Path) -> Vec<Value> {
    match status {
        "需求澄清" => vec![
            file_check(
                dir,
                "alignment.md",
                "澄清目标、范围、验收口径和待确认问题",
                true,
            ),
            file_check(dir, "background.md", "业务背景可被开发/测试继承", true),
            file_check(dir, "impact.md", "初步影响面、核心链路和验证方向", true),
            file_check(dir, "memory.md", "压缩阶段结论和下一步入口", true),
        ],
        "开发中" => vec![
            file_check(
                dir,
                "alignment.md",
                "已明确做什么、不做什么和验收标准",
                true,
            ),
            file_check(dir, "impact.md", "开发前复核核心链路风险与回退策略", true),
            file_check(dir, "branch.md", "需求分支和提交记录可追踪", true),
            file_check(dir, BRANCH_SCOPE_FILE, "repo/branch 机器可读映射", false),
            file_check(dir, "release-manifest.md", "上线清单持续维护", true),
            file_check(dir, "config-changes.md", "配置/DB/MQ 变更集中记录", false),
        ],
        "自测中" => vec![
            file_check(dir, "branch.md", "需求分支已提交并同步", true),
            file_check(dir, BRANCH_SCOPE_FILE, "可计算 diff / 部署影响", false),
            file_check(dir, "test.md", "自测场景、tid、DB/副作用和反向证据", true),
            file_check(dir, "release-manifest.md", "上线清单完成自检", true),
            any_file_check(
                dir,
                &["review.md", "code-review-ai.md", CODE_REVIEW_FILE],
                "代码审查门禁结论",
                false,
            ),
        ],
        "测试中" => vec![
            file_check(dir, "test.md", "测试反馈、复现证据和回归结果", true),
            any_file_check(
                dir,
                &["review.md", "code-review-ai.md", CODE_REVIEW_FILE],
                "代码审查门禁已通过或豁免",
                true,
            ),
            file_check(dir, "release-manifest.md", "待测版本的上线清单完整", true),
            file_check(dir, BRANCH_SCOPE_FILE, "test/UAT 合并目标可计算", false),
        ],
        "经验总结" => vec![
            file_check(
                dir,
                "experience-summary.md",
                "业务知识、经验、skill 和流程改进闭环",
                true,
            ),
            file_check(dir, "memory.md", "本需求最终摘要", true),
            file_check(dir, "test.md", "验证结果和证据可复用", true),
            file_check(dir, "release-manifest.md", "发布相关变更无遗漏", true),
            file_check(dir, "notes.md", "关键决策和坑点可追溯", false),
        ],
        "已完成" => vec![
            file_check(
                dir,
                "experience-summary.md",
                "经验总结已完成或明确无需沉淀",
                false,
            ),
            file_check(dir, "release-check.md", "发布/完成前检查记录", false),
            file_check(dir, "test.md", "最终验证证据", true),
            file_check(dir, "release-manifest.md", "最终上线清单", true),
        ],
        _ => vec![file_check(dir, "memory.md", "需求摘要", false)],
    }
}

fn file_check(dir: &Path, file: &str, label: &str, required: bool) -> Value {
    let path = dir.join(file);
    let bytes = path.metadata().map(|m| m.len()).unwrap_or(0);
    let ok = bytes > 0;
    json!({
        "id": file.replace(['.', '/'], "-"),
        "label": label,
        "required": required,
        "ok": ok,
        "status": if ok { "ok" } else if required { "missing" } else { "optionalMissing" },
        "source": file,
        "bytes": bytes,
        "path": path.to_string_lossy()
    })
}

fn any_file_check(dir: &Path, files: &[&str], label: &str, required: bool) -> Value {
    let candidates: Vec<Value> = files
        .iter()
        .map(|file| {
            let path = dir.join(file);
            let bytes = path.metadata().map(|m| m.len()).unwrap_or(0);
            json!({ "file": file, "exists": bytes > 0, "bytes": bytes, "path": path.to_string_lossy() })
        })
        .collect();
    let ok = candidates
        .iter()
        .any(|item| item.get("exists").and_then(Value::as_bool).unwrap_or(false));
    json!({
        "id": files.join("-or-").replace(['.', '/'], "-"),
        "label": label,
        "required": required,
        "ok": ok,
        "status": if ok { "ok" } else if required { "missing" } else { "optionalMissing" },
        "source": files.join(" | "),
        "candidates": candidates
    })
}

fn agent_context_tokens(intent: &str) -> Vec<&'static str> {
    match intent {
        "self-test" => vec!["req.memory", "req.test", "req.impact", "req.notes"],
        "release-check" => vec![
            "req.memory",
            "req.releaseManifest",
            "req.test",
            "req.impact",
            "req.releaseCheck",
        ],
        "config" => vec![
            "req.memory",
            "req.configChanges",
            "req.releaseManifest",
            "req.impact",
        ],
        "review" => vec!["req.memory", "req.review", "req.impact"],
        "progress" | "status" => vec!["req.memory", "req.notes"],
        _ => vec![
            "req.memory",
            "req.background",
            "req.impact",
            "req.test",
            "req.notes",
        ],
    }
}

fn summarize_requirement_doc(raw: &str, max_chars: usize) -> (Value, bool) {
    if raw.trim().is_empty() {
        return (json!({ "headings": [], "excerpt": "" }), false);
    }
    let headings: Vec<Value> = raw
        .lines()
        .filter_map(parse_markdown_heading)
        .take(12)
        .map(|(level, text)| json!({ "level": level, "text": text }))
        .collect();
    let candidate_lines: Vec<&str> = raw
        .lines()
        .filter(|line| {
            let t = line.trim();
            t.starts_with("- ")
                || t.starts_with("* ")
                || t.starts_with("##")
                || t.starts_with("###")
        })
        .take(80)
        .collect();
    let base = if candidate_lines.is_empty() {
        raw.trim().to_string()
    } else {
        candidate_lines.join("\n")
    };
    let (excerpt, truncated) = truncate_chars(&base, max_chars);
    (
        json!({ "headings": headings, "excerpt": excerpt }),
        truncated,
    )
}

async fn read_recent_requirement_events(path: &Path, limit: usize) -> Vec<Value> {
    let raw = fs::read_to_string(path).await.unwrap_or_default();
    let mut events: Vec<Value> = raw
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .collect();
    if events.len() > limit {
        events = events.split_off(events.len() - limit);
    }
    events
}

fn recommended_requirement_writes(intent: &str) -> Vec<Value> {
    match intent {
        "self-test" => vec![
            json!({"method":"POST","path":"/api/requirement/events","body":{"operation":"implicit","type":"testResult","reqId":"<req-id>","summary":"...","testCases":[{"name":"...","result":"pass|fail","evidence":"..."}]}}),
            json!({"method":"POST","path":"/api/requirement/sections/test","body":{"reqId":"<req-id>","heading":"测试场景","content":"..."}}),
        ],
        "clarification" | "design" => vec![
            json!({"method":"POST","path":"/api/requirement/events","body":{"type":"decision","reqId":"<req-id>","summary":"...","decisions":["..."]}}),
            json!({"method":"POST","path":"/api/requirement/sections/impact","body":{"reqId":"<req-id>","heading":"影响面评估","content":"..."}}),
        ],
        _ => vec![
            json!({"method":"POST","path":"/api/requirement/events","body":{"type":"progress","reqId":"<req-id>","summary":"..."}}),
            json!({"method":"POST","path":"/api/requirement/edit","body":{"operation":"appendNote","reqId":"<req-id>","title":"进展","text":"..."}}),
        ],
    }
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
            let status = canonical_status(&status)?;
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

#[derive(Debug)]
struct ReviewGateDecision {
    status: String,
    label: String,
    allows_testing: bool,
    reason: String,
    source: Option<String>,
    review_path: PathBuf,
    ai_review_path: PathBuf,
    actions: Vec<String>,
}

async fn review_gate_json(req: &Requirement) -> ApiResult<Value> {
    let gate = review_gate_decision(req).await?;
    Ok(json!({
        "ok": true,
        "reqId": req.id,
        "gate": {
            "status": gate.status,
            "label": gate.label,
            "allowsTesting": gate.allows_testing,
            "reason": gate.reason,
            "source": gate.source,
            "reviewPath": gate.review_path.to_string_lossy(),
            "aiReviewPath": gate.ai_review_path.to_string_lossy(),
            "actions": gate.actions,
            "checkedAt": now_ms(),
        }
    }))
}

async fn ensure_review_gate_allows_testing(req: &Requirement) -> ApiResult<()> {
    let gate = review_gate_decision(req).await?;
    if gate.allows_testing {
        return Ok(());
    }
    Err(ApiError::bad_request(format!(
        "Code Review Gate 未通过（{}）：{}。请先补充 review.md / code-review-ai.md 并明确 `Review Gate: PASS`，或在 review.md 记录 `Review Gate: WAIVED` + 豁免原因。",
        gate.label, gate.reason
    )))
}

async fn review_gate_decision(req: &Requirement) -> ApiResult<ReviewGateDecision> {
    let dir = req_dir_path(req)?;
    let review_path = dir.join("review.md");
    let ai_review_path = dir.join("code-review-ai.md");
    let mut docs = Vec::<(String, String)>::new();
    for (label, path) in [
        ("review.md".to_string(), review_path.clone()),
        ("code-review-ai.md".to_string(), ai_review_path.clone()),
    ] {
        if let Ok(raw) = fs::read_to_string(&path).await {
            if !raw.trim().is_empty() {
                docs.push((label, raw));
            }
        }
    }
    if docs.is_empty() {
        return Ok(ReviewGateDecision {
            status: "missing".into(),
            label: "未执行".into(),
            allows_testing: false,
            reason: "未找到 review.md 或 code-review-ai.md 的代码审查结论".into(),
            source: None,
            review_path,
            ai_review_path,
            actions: vec![
                "生成 code-review.json / code-review-ai.md，或手工补充 review.md".into(),
                "在审查结论中写明 `Review Gate: PASS`、`Review Gate: BLOCKED` 或 `Review Gate: WAIVED`".into(),
            ],
        });
    }
    for (source, raw) in &docs {
        if review_gate_waived(raw) {
            return Ok(ReviewGateDecision {
                status: "waived".into(),
                label: "用户豁免".into(),
                allows_testing: true,
                reason: "review 文档记录了豁免结论".into(),
                source: Some(source.clone()),
                review_path,
                ai_review_path,
                actions: vec!["保留豁免原因，测试阶段重点覆盖高风险改动".into()],
            });
        }
    }
    for (source, raw) in &docs {
        if review_gate_blocked(raw) {
            return Ok(ReviewGateDecision {
                status: "blocked".into(),
                label: "有阻塞项".into(),
                allows_testing: false,
                reason: "review 文档存在严重问题、阻塞项或明确 BLOCKED 结论".into(),
                source: Some(source.clone()),
                review_path,
                ai_review_path,
                actions: vec![
                    "修复严重问题后重新审查".into(),
                    "若业务确认可带风险提测，在 review.md 明确 `Review Gate: WAIVED` 和豁免原因"
                        .into(),
                ],
            });
        }
    }
    for (source, raw) in &docs {
        if review_gate_passed(raw) {
            return Ok(ReviewGateDecision {
                status: "passed".into(),
                label: "审查通过".into(),
                allows_testing: true,
                reason: "review 文档记录了通过结论，或严重问题小节为空/无".into(),
                source: Some(source.clone()),
                review_path,
                ai_review_path,
                actions: vec!["可以推进到测试中；测试阶段按 review 的验收要点回归".into()],
            });
        }
    }
    Ok(ReviewGateDecision {
        status: "pending".into(),
        label: "待确认".into(),
        allows_testing: false,
        reason: "已找到 review 文档，但缺少明确 PASS / BLOCKED / WAIVED 结论".into(),
        source: docs.first().map(|(source, _)| source.clone()),
        review_path,
        ai_review_path,
        actions: vec![
            "在 review.md 顶部补充 `Review Gate: PASS` / `BLOCKED` / `WAIVED`".into(),
            "若使用 AI 审查，确认 code-review-ai.md 后同步结论到 review.md".into(),
        ],
    })
}

fn review_gate_waived(raw: &str) -> bool {
    let lower = raw.to_lowercase();
    lower.contains("review gate: waived")
        || raw.contains("用户豁免")
        || raw.contains("代码审查豁免")
}

fn review_gate_blocked(raw: &str) -> bool {
    let lower = raw.to_lowercase();
    if lower.contains("review gate: blocked")
        || raw.contains("不可提测")
        || raw.contains("审查不通过")
    {
        return true;
    }
    if raw.contains('❌')
        && (raw.contains("阻塞") || raw.contains("必须修复") || raw.contains("严重问题"))
    {
        return true;
    }
    review_gate_section(raw, &["严重问题", "必须修复", "Blocking Items", "阻塞项"])
        .map(|section| !review_section_is_empty(&section))
        .unwrap_or(false)
}

fn review_gate_passed(raw: &str) -> bool {
    let lower = raw.to_lowercase();
    if lower.contains("review gate: pass")
        || lower.contains("result: pass")
        || raw.contains("代码审查通过")
        || raw.contains("审查通过")
        || raw.contains("可提测")
        || raw.contains("无阻塞")
    {
        return true;
    }
    review_gate_section(raw, &["严重问题", "必须修复"])
        .map(|section| review_section_is_empty(&section))
        .unwrap_or(false)
}

fn review_gate_section(raw: &str, heading_keywords: &[&str]) -> Option<String> {
    let mut start: Option<(usize, usize)> = None;
    let lines: Vec<&str> = raw.lines().collect();
    for (idx, line) in lines.iter().enumerate() {
        if let Some((level, text)) = parse_markdown_heading(line) {
            let normalized = text.to_lowercase();
            if heading_keywords
                .iter()
                .any(|keyword| normalized.contains(&keyword.to_lowercase()))
            {
                start = Some((idx, level));
                break;
            }
        }
    }
    let (start_idx, start_level) = start?;
    let mut end_idx = lines.len();
    for (idx, line) in lines.iter().enumerate().skip(start_idx + 1) {
        if let Some((level, _)) = parse_markdown_heading(line) {
            if level <= start_level {
                end_idx = idx;
                break;
            }
        }
    }
    Some(lines[start_idx + 1..end_idx].join("\n"))
}

fn review_section_is_empty(section: &str) -> bool {
    !section.lines().any(|line| {
        let clean = line
            .trim()
            .trim_start_matches(['-', '*', ' ', '\t'])
            .trim_start_matches("[ ]")
            .trim_start_matches("[x]")
            .trim_start_matches("[X]")
            .trim()
            .trim_matches('。')
            .trim_matches('.')
            .trim();
        if clean.is_empty() {
            return false;
        }
        let lower = clean.to_lowercase();
        !matches!(
            lower.as_str(),
            "无" | "none" | "n/a" | "na" | "暂无" | "-" | "无。"
        ) && !clean.starts_with("无，")
            && !clean.starts_with("无；")
            && !clean.starts_with("无 ")
    })
}

async fn validate_requirement(state: &AppState, req: &Requirement) -> ApiResult<Value> {
    let dir = req_dir_path(req)?;
    ensure_requirement_dir_writable(state, &dir).await?;
    let mut problems = Vec::<String>::new();
    let mut warnings = Vec::<String>::new();
    let mut files = HashMap::<String, bool>::new();
    for file in [
        "meta.md",
        "alignment.md",
        "background.md",
        "memory.md",
        "branch.md",
        "config-changes.md",
        "release-manifest.md",
        "impact.md",
        "test.md",
        "notes.md",
        "experience-summary.md",
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
        ("alignment.md", template_alignment(req_id)),
        (
            "background.md",
            background
                .map(str::to_string)
                .unwrap_or_else(|| template_background(req_id)),
        ),
        ("memory.md", template_memory(req_id, title)),
        ("branch.md", template_branch(req_id)),
        ("config-changes.md", template_config_changes(req_id)),
        ("release-manifest.md", template_release_manifest(req_id)),
        ("impact.md", template_impact(req_id)),
        ("test.md", template_test(req_id)),
        ("experience-summary.md", template_experience_summary(req_id)),
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

fn template_alignment(req_id: &str) -> String {
    format!("# {req_id} 需求澄清\n\n## 业务目标\n- 待补充：这次需求要解决的业务问题和成功标准。\n\n## 场景与角色\n- 待补充：涉及的业务角色、对象、入口和主流程。\n\n## PRD 解读\n- 来源：待补充\n- 已确认：待补充\n- 不确定：待补充\n\n## 初步代码调查\n- 相关仓库/模块：待补充\n- 现有系统行为：待补充\n- 初步实现方向：待补充\n\n## 范围与非目标\n- Include：待补充\n- Exclude：待补充\n\n## 待确认问题\n- [ ] 待补充\n")
}

fn template_background(req_id: &str) -> String {
    format!("# {req_id} 业务背景文档\n\n> 面向不熟悉业务的开发、测试和后续经验总结使用；尽量用业务语言说明为什么做、当前怎么运转、这次改变什么。\n\n## 一句话背景\n- 待补充\n\n## 业务目标\n- 待补充\n\n## 业务对象与角色\n- 对象：待补充\n- 角色：待补充\n- 入口：待补充\n\n## 当前系统行为\n- 待补充\n\n## 本次需求改变\n- 待补充\n\n## 关键业务规则\n- 待补充\n\n## 沟通口径\n- 产品/业务确认点：待补充\n- 测试重点：待补充\n\n## 关联知识与经验\n- 业务知识：待补充\n- 历史经验：待补充\n")
}

fn template_memory(req_id: &str, title: &str) -> String {
    format!("# {req_id} Memory\n\n## 当前目标\n- {title}\n\n## 当前进展\n- 已创建需求，待补充进展。\n\n## 关键决策\n- 待补充\n\n## 待办 / 风险\n- [ ] 待补充\n")
}

fn template_branch(req_id: &str) -> String {
    format!("# {req_id} Branches\n\n| Item | Value |\n| --- | --- |\n| Source branch | unknown |\n| Target branch | unknown |\n| Project path | unknown |\n| Merge status | 开发中 |\n\n## Commit / Diff Notes\n- 待补充\n")
}

fn template_config_changes(req_id: &str) -> String {
    format!("# {req_id} Config Changes\n\n> 低层配置明细；上线总览请同步维护 release-manifest.md。\n\n## DB 变更\n- 暂无\n\n## Apollo / Nacos 变更\n- 暂无\n\n## RocketMQ / Console 变更\n- 暂无\n")
}

fn template_release_manifest(req_id: &str) -> String {
    format!("# {req_id} 上线清单\n\n> 贯穿需求全流程维护；用于上线前快速确认本次改了哪些配置、表、Topic、Group、Job、接口和人工动作，避免发布遗漏。\n\n## Summary\n- 结论：暂无上线资产变更 / 待补充\n- 最后更新：待补充\n- 负责人：待补充\n\n## DB / 表变更\n| 类型 | 表/库 | 变更内容 | 环境 | 是否需上线执行 | 回滚/备注 |\n| --- | --- | --- | --- | --- | --- |\n| 无 | - | - | - | 否 | - |\n\n## 配置变更\n| 类型 | Namespace/配置源 | Key/名称 | 变更内容 | 环境 | 是否已发布 | 备注 |\n| --- | --- | --- | --- | --- | --- | --- |\n| 无 | - | - | - | - | 否 | - |\n\n## MQ / Topic / Group\n| 类型 | Topic | Group/Tag | 生产者 | 消费者 | 控制台动作 | 备注 |\n| --- | --- | --- | --- | --- | --- | --- |\n| 无 | - | - | - | - | 否 | - |\n\n## Job / 定时任务 / 开关\n| 类型 | 名称 | 动作 | 环境 | 是否需人工处理 | 备注 |\n| --- | --- | --- | --- | --- | --- |\n| 无 | - | - | - | 否 | - |\n\n## API / 外部依赖\n| 类型 | 接口/系统 | 变更 | 是否需通知 | 备注 |\n| --- | --- | --- | --- | --- |\n| 无 | - | - | 否 | - |\n\n## 上线人工动作\n- [ ] 暂无\n\n## 风险与回滚提醒\n- 待补充\n")
}

fn template_impact(req_id: &str) -> String {
    format!("# {req_id} Impact\n\n## 风险等级\n- 待评估\n\n## 核心链路影响\n- 待补充\n\n## 回滚方案\n- 待补充\n")
}

fn template_test(req_id: &str) -> String {
    format!("# {req_id} Test\n\n## 测试场景清单\n\n| ID | 场景描述 | 触发方式 | 前置条件 | 预期结果 | 证据标准 |\n| --- | --- | --- | --- | --- | --- |\n| S1 | 待补充 | 待补充 | 待补充 | 待补充 | 日志 + DB + 副作用 + 反向检查 |\n\n## 自测记录\n- ⬜ 待执行\n\n## UAT 回归记录\n- ⬜ 待执行\n")
}

fn template_experience_summary(req_id: &str) -> String {
    format!("# {req_id} 经验总结\n\n## 本次需求结论\n- 待补充\n\n## 新发现的业务知识\n| 发现 | 是否已落地 | 目标位置 | 备注 |\n| --- | --- | --- | --- |\n| 待补充 | 否 | .agents/business-knowledge/ | - |\n\n## 新发现的经验 / 踩坑\n| 经验 | 是否已落地 | 目标位置 | 备注 |\n| --- | --- | --- | --- |\n| 待补充 | 否 | .agents/experiences/ | - |\n\n## Skill 改进机会\n| Skill | 问题 / 机会 | 动作 | 状态 |\n| --- | --- | --- | --- |\n| 待补充 | 待补充 | 新增/优化/不处理 | 待落地 |\n\n## 流程改进\n- 待补充\n\n## 已落地清单\n- [ ] 待补充\n\n## 待落地清单\n- [ ] 待补充\n")
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
        "release-manifest" | "releasemanifest" | "manifest" | "release-manifest.md" => {
            Ok("release-manifest.md")
        }
        "impact" | "impact.md" => Ok("impact.md"),
        "test" | "test.md" => Ok("test.md"),
        "notes" | "notes.md" => Ok("notes.md"),
        "review" | "review.md" => Ok("review.md"),
        "release-check" | "releasecheck" | "release-check.md" => Ok("release-check.md"),
        "experience-summary" | "experiencesummary" | "experience-summary.md" => {
            Ok("experience-summary.md")
        }
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

fn extract_completed_at(state: &Value) -> Option<i64> {
    state
        .get("history")
        .and_then(Value::as_array)?
        .iter()
        .rev()
        .find(|h| h.get("status").and_then(Value::as_str) == Some("已完成"))
        .and_then(|h| h.get("at").and_then(Value::as_i64))
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
    let changed = from.as_deref() != Some(new_status);
    let mut history = previous
        .get("history")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let transition = if changed {
        json!({
            "status": new_status,
            "from": from,
            "at": now_ms(),
            "note": note.unwrap_or(""),
            "skippedStatuses": skipped_statuses(from.as_deref(), new_status)
        })
    } else {
        Value::Null
    };
    if changed {
        history.push(transition.clone());
    }
    if history.len() > 50 {
        history = history[history.len() - 50..].to_vec();
    }
    let state = json!({
        "version": 1,
        "status": new_status,
        "previousStatus": from,
        "changed": changed,
        "lastTransition": transition,
        "category": previous.get("category").cloned().unwrap_or(Value::Null),
        "updatedAt": now_ms(),
        "history": history
    });
    atomic_write_json(&path, &state).await?;
    Ok(state)
}

/// 从 state.json 提取该需求进入「经验总结」状态的时间戳（毫秒）。
/// 取 history 中最后一次 status == 经验总结 的 at；历史缺失时回退到 updated_at。
fn experience_summary_entered_at_from_state(state: &Value, fallback_updated_at: i64) -> i64 {
    if let Some(history) = state.get("history").and_then(Value::as_array) {
        for entry in history.iter().rev() {
            let is_experience = entry
                .get("status")
                .and_then(Value::as_str)
                .map(|s| s == "经验总结")
                .unwrap_or(false);
            if is_experience {
                return entry.get("at").and_then(Value::as_i64).unwrap_or(fallback_updated_at);
            }
        }
    }
    fallback_updated_at
}

/// 判定：进入经验总结的时间早于 (now - grace_ms) 即视为超期，应自动推进为已完成。
fn experience_summary_overdue(entered_at: i64, now: i64, grace_ms: i64) -> bool {
    entered_at > 0 && now - entered_at >= grace_ms
}

/// 判定：该需求当前是否应被自动推进为已完成。
/// 要求 state.json 真实状态为「经验总结」且进入该状态超过 grace_ms；
/// 「待上线」等历史遗留状态不参与自动推进。
fn should_auto_complete_experience_summary(
    state: &Value,
    fallback_updated_at: i64,
    now: i64,
    grace_ms: i64,
) -> bool {
    let real_status = state
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if real_status != "经验总结" {
        return false;
    }
    let entered_at = experience_summary_entered_at_from_state(state, fallback_updated_at);
    experience_summary_overdue(entered_at, now, grace_ms)
}

/// 扫描所有需求，将停留在「经验总结」状态超过阈值的需求自动改为「已完成」。
/// 返回本次实际推进的数量。
async fn expire_stale_experience_summary(state: &AppState) -> Result<usize> {
    let reqs = list_requirements(state).await?;
    let now = now_ms();
    let mut changed = 0;
    for req in reqs {
        if req.status != "经验总结" {
            continue;
        }
        let dir = match req_dir_path(&req) {
            Ok(d) => d,
            Err(_) => continue,
        };
        let Some(state_value) = read_requirement_state(&dir).await? else {
            continue;
        };
        if !should_auto_complete_experience_summary(
            &state_value,
            req.updated_at,
            now,
            EXPERIENCE_SUMMARY_GRACE_MS,
        ) {
            continue;
        }
        let st = write_requirement_status(
            dir.to_string_lossy().as_ref(),
            "已完成",
            Some(EXPERIENCE_AUTO_COMPLETE_NOTE),
        )
        .await?;
        // 事件记录失败不阻断推进，仅告警（避免单个需求写失败阻塞整个扫描）。
        if let Err(e) = record_status_transition_event(state, &req, &st, Some(EXPERIENCE_AUTO_COMPLETE_NOTE)).await {
            tracing::warn!(req_id = %req.id, "auto-complete event record failed: {e:?}");
        } else {
            tracing::info!(req_id = %req.id, "auto-completed requirement after staying in 经验总结 >48h");
        }
        changed += 1;
    }
    Ok(changed)
}

/// 常驻后台任务：每隔固定周期扫描一次，自动推进超期停留在经验总结状态的需求。
async fn expire_stale_experience_summary_loop(state: AppState) {
    let mut ticker = tokio::time::interval(Duration::from_secs(EXPERIENCE_AUTO_COMPLETE_INTERVAL_SECS));
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        ticker.tick().await;
        match expire_stale_experience_summary(&state).await {
            Ok(n) => tracing::info!("experience summary auto-complete scan: {n} requirement(s) advanced to 已完成"),
            Err(e) => tracing::warn!("experience summary auto-complete scan failed: {e:#}"),
        }
    }
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
        "# Agent Panel Requirement Startup Context\n\n- Req ID: {}\n- Title: {}\n- Startup Status: {}\n- Directory: {}\n\n{}\n\n## 重要：阶段提示词实时刷新\n\n这个文件只在 session 创建时注入一次，不能作为长期阶段真相。每轮开始、状态切换后、或任务意图变化时，优先调用：\n\n`GET /api/requirement/context?id={}&for=agent&intent=<intent>&budget=2000`\n\n返回值里的 `phaseRuntime.currentPhasePrompt` 才是当前状态独有提示词；`phaseRuntime.transitionMemory` 会给出状态历史、跳状态风险和缺失前置项。\n\n## Agent workflow\n1. 非简单需求编辑先调用 `GET /api/requirement/edit-plan?id={}&intent=<intent>`，再选择读写范围。\n2. 读取上下文优先调用 `GET /api/requirement/context?id={}&for=agent&intent=<intent>&budget=2000`，不要默认通读大 Markdown。\n3. 状态可以跳转；进入新状态后执行 `phaseRuntime.entryChecks`，缺失项作为风险记录，不自动回退状态。\n4. 需求澄清阶段使用 `intent=clarification`，产出/更新 `alignment.md`、`background.md`、`impact.md`、`memory.md`。\n5. 经验总结阶段使用 `intent=experience-summary`，产出/更新 `experience-summary.md`，并把已确认的新业务知识、经验或 skill 改进落地。\n6. 写入优先调用 `POST /api/requirement/edit`，完成后调用 `POST /api/requirement/validate`。\n7. 只有 API 不可用或任务明确要求原文时，才直接读取需求目录文件。\n",
        req.id,
        req.title,
        req.status,
        req.req_dir.clone().unwrap_or_default(),
        req.description,
        req.id,
        req.id,
        req.id
    );
    atomic_write_text(&path, &body).await?;
    Ok(path)
}

async fn load_phase_prompt(state: &AppState, status: &str) -> String {
    let prompt_file = phase_prompt_file(status);
    let path = state.project_root.join(prompt_file);
    fs::read_to_string(path)
        .await
        .unwrap_or_else(|_| format!("本阶段状态：{status}。请遵循 Agent Panel 需求文件协议推进。"))
}

fn phase_prompt_file(status: &str) -> &'static str {
    match status {
        "需求澄清" | "需求对齐" | "方案设计" => "prompts/phase-clarify.md",
        "开发中" => "prompts/phase-dev.md",
        "自测中" => "prompts/phase-selftest.md",
        "测试中" => "prompts/phase-testing.md",
        "经验总结" | "待上线" => "prompts/phase-experience-summary.md",
        "已完成" => "prompts/phase-done.md",
        _ => "prompts/phase-dev.md",
    }
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

fn parse_knowledge_meta_yaml(text: &str, path: &Path) -> ApiResult<Frontmatter> {
    let value: serde_yaml::Value = serde_yaml::from_str(text).map_err(|e| {
        ApiError::bad_request(format!(
            "invalid knowledge meta yaml {}: {e}",
            path.display()
        ))
    })?;
    let mapping = value.as_mapping().ok_or_else(|| {
        ApiError::bad_request(format!(
            "knowledge meta yaml must be a mapping: {}",
            path.display()
        ))
    })?;
    let mut fields = HashMap::new();
    for (key, value) in mapping {
        let Some(key) = key.as_str() else {
            continue;
        };
        fields.insert(key.to_string(), yaml_value_to_field(value));
    }
    Ok(Frontmatter {
        fields,
        body: String::new(),
    })
}

fn yaml_value_to_field(value: &serde_yaml::Value) -> String {
    match value {
        serde_yaml::Value::Null => String::new(),
        serde_yaml::Value::Bool(v) => v.to_string(),
        serde_yaml::Value::Number(v) => v.to_string(),
        serde_yaml::Value::String(v) => v.clone(),
        serde_yaml::Value::Sequence(values) => values
            .iter()
            .map(yaml_value_to_field)
            .filter(|v| !v.is_empty())
            .collect::<Vec<_>>()
            .join(", "),
        serde_yaml::Value::Mapping(_) | serde_yaml::Value::Tagged(_) => {
            serde_yaml::to_string(value)
                .unwrap_or_default()
                .trim()
                .to_string()
        }
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
    value.and_then(|v| normalize_status_value(v))
}

fn normalize_status_value(value: &str) -> Option<String> {
    let raw = value.trim();
    if let Some((_, canonical)) = REQ_STATUS_ALIASES.iter().find(|(alias, _)| *alias == raw) {
        return Some((*canonical).to_string());
    }
    if REQ_STATUSES.contains(&raw) {
        Some(raw.to_string())
    } else {
        None
    }
}

fn canonical_status(value: &str) -> ApiResult<String> {
    normalize_status_value(value)
        .ok_or_else(|| ApiError::bad_request(format!("invalid status: {value}")))
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
    canonical_status(value).map(|_| ())
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
    fn normalize_capability_maps_legacy_wms_fields_to_common_schema() {
        let cap = json!({
            "id": "outbound-any-status",
            "domain": "outbound",
            "object": "shipment_header",
            "execution": "script",
            "purpose": "创建任意状态出库单",
            "script": "scripts/wms_create_outbound.py",
            "invocation": "uv run python scripts/wms_create_outbound.py --target <state>",
            "verified_env": "test",
            "verified_date": "2026-07-05",
            "stdout_json": true,
            "exit_code": "success_in_json",
            "targets": [{"name": "shipped", "verified": true}],
            "state_graph": "state-graph/outbound.yaml",
            "recipe": "recipes/outbound/create-any-status-shipment.yaml",
            "pitfalls": ["pitfalls/outbound/stock-not-available.md"],
            "notes": ["autoStatus may advance to 900"]
        });
        let normalized = normalize_capability(Path::new("/tmp/wms-testdata"), &cap);
        assert_eq!(normalized["kind"], "testdata");
        assert_eq!(normalized["id"], "outbound-any-status");
        assert_eq!(normalized["title"], "创建任意状态出库单");
        assert_eq!(normalized["runner"]["type"], "script");
        assert_eq!(
            normalized["runner"]["script"],
            "scripts/wms_create_outbound.py"
        );
        assert_eq!(normalized["runner"]["cwd"], "/tmp/wms-testdata");
        assert_eq!(normalized["safety"]["agentPanelExecutes"], false);
        assert_eq!(normalized["verification"]["targets"][0]["name"], "shipped");
        assert_eq!(
            normalized["relatedArtifacts"]["recipe"],
            "recipes/outbound/create-any-status-shipment.yaml"
        );
        assert_eq!(normalized["legacy"]["domain"], "outbound");
    }

    #[test]
    fn skipped_statuses_reports_forward_phase_gaps() {
        assert_eq!(
            skipped_statuses(Some("需求澄清"), "测试中"),
            vec!["开发中".to_string(), "自测中".to_string()]
        );
    }

    #[test]
    fn skipped_statuses_ignores_adjacent_and_backward_moves() {
        assert!(skipped_statuses(Some("需求澄清"), "开发中").is_empty());
        assert!(skipped_statuses(Some("测试中"), "开发中").is_empty());
        assert!(skipped_statuses(None, "开发中").is_empty());
    }

    #[test]
    fn status_transition_alias_normalizes_event_type() {
        assert_eq!(
            normalize_requirement_event_type(Some("phase_transition")),
            "statusTransition"
        );
        assert_eq!(requirement_event_label("statusTransition"), "状态切换");
    }

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

    #[test]
    fn experience_summary_entered_at_picks_last_history_entry() {
        // 多次进入经验总结时取最后一次进入时间。
        let state = json!({
            "status": "经验总结",
            "history": [
                {"status": "测试中", "from": "自测中", "at": 1000},
                {"status": "经验总结", "from": "测试中", "at": 2000},
                {"status": "已完成", "from": "经验总结", "at": 3000},
                {"status": "经验总结", "from": "已完成", "at": 4000},
            ]
        });
        assert_eq!(experience_summary_entered_at_from_state(&state, 9999), 4000);
    }

    #[test]
    fn experience_summary_entered_at_falls_back_when_no_history() {
        // 历史缺失或没有经验总结记录时回退到 updated_at。
        let empty = json!({ "status": "经验总结", "history": [] });
        assert_eq!(experience_summary_entered_at_from_state(&empty, 5555), 5555);
        let no_exp = json!({
            "status": "经验总结",
            "history": [{"status": "开发中", "from": null, "at": 1000}]
        });
        assert_eq!(experience_summary_entered_at_from_state(&no_exp, 5555), 5555);
    }

    #[test]
    fn experience_summary_overdue_respects_grace_window() {
        let now = 1_800_000_000_000i64; // ~2027，真实毫秒时间戳量级
        let day_ms = 24 * 3600 * 1000i64;
        // 恰好 48h 之前进入 -> 视为超期（>= 阈值）。
        assert!(experience_summary_overdue(now - 2 * day_ms, now, 2 * day_ms));
        // 超期更久 -> 超期。
        assert!(experience_summary_overdue(now - 3 * day_ms, now, 2 * day_ms));
        // 48h 内 -> 未超期。
        assert!(!experience_summary_overdue(now - day_ms, now, 2 * day_ms));
        // 未来时间戳（时钟异常）-> 不超期。
        assert!(!experience_summary_overdue(now + 1000, now, 2 * day_ms));
        // entered_at 无效（<=0）-> 不推进。
        assert!(!experience_summary_overdue(0, now, 2 * day_ms));
    }

    #[test]
    fn should_auto_complete_only_for_real_experience_summary_status() {
        let now = 1_800_000_000_000i64;
        let day_ms = 24 * 3600 * 1000i64;
        let stale = json!({
            "status": "经验总结",
            "history": [{"status": "经验总结", "from": "测试中", "at": now - 3 * day_ms}]
        });
        // 真实状态为经验总结且超期 -> 推进。
        assert!(should_auto_complete_experience_summary(&stale, now, now, 2 * day_ms));
        // 真实状态为经验总结但未超期 -> 不推进。
        let fresh = json!({
            "status": "经验总结",
            "history": [{"status": "经验总结", "from": "测试中", "at": now - day_ms}]
        });
        assert!(!should_auto_complete_experience_summary(&fresh, now, now, 2 * day_ms));
        // 真实状态为待上线（历史遗留，normalize 为经验总结）即使超期也不推进。
        let waiting = json!({
            "status": "待上线",
            "history": [{"status": "待上线", "from": "测试中", "at": now - 10 * day_ms}]
        });
        assert!(!should_auto_complete_experience_summary(&waiting, now, now, 2 * day_ms));
        // 真实状态为已完成 -> 不推进。
        let done = json!({
            "status": "已完成",
            "history": [{"status": "已完成", "from": "经验总结", "at": now - 3 * day_ms}]
        });
        assert!(!should_auto_complete_experience_summary(&done, now, now, 2 * day_ms));
    }
}
