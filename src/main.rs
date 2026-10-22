use std::{collections::HashMap, env, net::SocketAddr, path::PathBuf, sync::Arc, time::UNIX_EPOCH};

use anyhow::{Context, Result};
use axum::{
    extract::{Query, Request, State},
    http::{header, HeaderMap, HeaderValue, StatusCode, Uri},
    middleware::{self, Next},
    response::{Html, IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::{fs, sync::Mutex, task::JoinHandle};
use tower::Layer;
use tower_http::{cors::CorsLayer, services::ServeDir, trace::TraceLayer};

mod api_catalog;
mod attachments;
mod auth;
mod browser_auth;
mod cainiao_mock;
mod capability;
mod chrome_cookies;
mod config;
mod dsh_client;
mod experience_summary;
mod git_blame;
mod git_file_lines;
mod git_workflow;
mod harness;
mod http;
mod knowledge;
mod markdown;
mod ones;
mod ones_manhour;
mod paths;
mod pi_config;
mod pi_rpc;
mod requirement_api;
mod requirement_context;
mod requirement_index;
mod requirement_release;
mod requirement_service;
mod requirement_sub;
mod sessions;
mod util;
mod workers;

use api_catalog::*;
use attachments::*;
use browser_auth::*;
use cainiao_mock::*;
use capability::*;
use chrome_cookies::*;
use config::*;
use experience_summary::*;
use git_blame::*;
use git_file_lines::*;
pub(crate) use git_workflow::*;
use harness::*;
use http::*;
use knowledge::*;
use markdown::*;
use ones::*;
use ones_manhour::*;
use pi_config::*;
use pi_rpc::*;
use requirement_api::*;
use requirement_context::*;
use requirement_index::*;
use requirement_release::*;
use requirement_service::*;
use requirement_sub::*;
use sessions::*;
use util::*;

const DEFAULT_PORT: u16 = 7331;
/// worker 模式默认端口（本机执行节点；PORT 可覆盖）。
const WORKER_DEFAULT_PORT: u16 = 7332;
/// docs 模式（容器 hub）默认端口。
const HUB_DEFAULT_PORT: u16 = 8788;
const ENV_PANEL_MODE: &str = "PANEL_MODE";
const ENV_PANEL_AUTH_TOKEN: &str = "PANEL_AUTH_TOKEN";
const ENV_PANEL_DATA_DIR: &str = "PANEL_DATA_DIR";
const ENV_PANEL_PORT: &str = "PANEL_PORT";
/// 绑定地址覆盖：docs 默认 0.0.0.0、local 默认 127.0.0.1；手机/局域网/Tailscale 远程访问时设 0.0.0.0。
const ENV_PANEL_BIND_HOST: &str = "PANEL_BIND_HOST";
/// worker → hub 注册地址（worker 模式用）。
const ENV_PANEL_HUB_URL: &str = "PANEL_HUB_URL";
/// worker 对外回调地址（hub 代理转发的目标，worker 模式用）。
const ENV_PANEL_CALLBACK_BASE: &str = "PANEL_CALLBACK_BASE";
const DEFAULT_PROJECT_NAME: &str = "默认项目";
const DEFAULT_REQ_ID: &str = "__default__";
const STATE_FILE: &str = "state.json";
const ASSOCIATIONS_FILE: &str = "associations.json";
/// 引用式需求组定义文件：需求目录下的 group.json，members 引用其他 req id。
const GROUP_FILE: &str = "group.json";
const CONFIG_FILE: &str = "config.json";
const BUSINESS_KNOWLEDGE_DIR: &str = "business-knowledge";
const EXPERIENCES_DIR: &str = "experiences";
const KNOWLEDGE_DEFAULT_STATUS: &str = "active";
const KNOWLEDGE_DEFAULT_CONFIDENCE: &str = "medium";
const INJECTION_CTX_SUBDIR: &str = "ctx";
const BRANCH_SCOPE_FILE: &str = "branches.json";
/// 整合需求发布分支登记文件：父需求（整合需求）目录下的 release-branches.json。
const RELEASE_BRANCHES_FILE: &str = "release-branches.json";
const CODE_REVIEW_FILE: &str = "code-review.json";
const CODE_REVIEW_INCREMENTAL_FILE: &str = "code-review-incremental.json";
/// 快照 JSON 里 diff 是单行转义串（read 会被单行截断），同时导出多行 patch 伴随文件供 reviewer read。
const CODE_REVIEW_PATCH_FILE: &str = "code-review-diff.patch";
const CODE_REVIEW_INCREMENTAL_PATCH_FILE: &str = "code-review-incremental-diff.patch";
const CODE_ANNOTATIONS_FILE: &str = "code-annotations.json";
/// 审查清单（机器可判定的逐项结论）：审查门禁 PASS 路径的硬校验数据源。
const REVIEW_CHECKLIST_FILE: &str = "review-checklist.json";
const CODE_DIFF_SNAPSHOTS_FILE: &str = "code-diff-snapshots.json";
const REQUIREMENT_EVENTS_FILE: &str = "events.jsonl";
const EXPERIENCE_SUMMARY_JOB_FILE: &str = "experience-summary-job.json";
const PHASE_COMMON_PROMPT_FILE: &str = "prompts/phase-common.md";
/// 开发中返工轮次变体 prompt：需求被打回（人工核查/测试中 → 开发中）后加载，
/// 语境是「小修小补 + 增量验证」，不是首轮全新开发。
const PHASE_DEV_REWORK_PROMPT_FILE: &str = "prompts/phase-dev-rework.md";
const PHASE_DEV_RELEASE_READY_PROMPT_FILE: &str = "prompts/phase-dev-release-ready.md";
const DEFAULT_GITLAB_API_URL: &str = "http://code.jms.com/api/v4";
const DEFAULT_CAINIAO_MOCK_PORT: u16 = 13528;
/// 经验总结状态停留超过该时长后自动推进为已完成（48 小时）。
const EXPERIENCE_SUMMARY_GRACE_MS: i64 = 48 * 60 * 60 * 1000;
/// 自动推进扫描周期（每 10 分钟）。
const EXPERIENCE_AUTO_COMPLETE_INTERVAL_SECS: u64 = 600;
/// 自动经验总结派发扫描周期（每 1 分钟）。
const EXPERIENCE_AUTO_SUMMARY_INTERVAL_SECS: u64 = 60;
/// 自动总结 agent 超过该时长仍未回写则标记为 failed，避免长期占用并发槽位（12 小时）。
const EXPERIENCE_SUMMARY_JOB_STALE_MS: i64 = 2 * 60 * 60 * 1000;
const EXPERIENCE_SUMMARY_MAX_ATTEMPTS: i64 = 2;
/// 自动推进时写入的备注/事件说明。
const EXPERIENCE_AUTO_COMPLETE_NOTE: &str =
    "自动推进：经验总结状态停留超过 48 小时，自动标记为已完成";
/// issue 家族（线上问题/测试问题/性能问题）总结完成后的自动关闭备注/事件说明。
const EXPERIENCE_ISSUE_AUTO_CLOSE_NOTE: &str =
    "自动经验总结完成，问题自动关闭（issue 状态机无「已完成」，完成即关闭）";
const CAINIAO_MOCK_PRINTERS: &[(&str, &str)] = &[
    ("Mock-A4", "Mock A4 打印机"),
    ("Mock-Label-100", "Mock 标签机 100x100"),
    ("Mock-Label-76", "Mock 标签机 76x130"),
    ("Mock-Receipt", "Mock 小票打印机"),
    ("Mock-Express", "Mock 面单打印机"),
];
const COMMAND_OUTPUT_LIMIT: usize = 80_000;
const DIFF_OUTPUT_LIMIT: usize = 2_000_000;

static REQ_STATUSES: &[&str] = &[
    "需求澄清",
    "开发中",
    "自测中",
    "测试中",
    // 人工核查：agent 测试完成后，人工负责视觉验收、主流程复测和人工代码审查；
    // agent 在该阶段的职责是准备 UAT 测试数据/复测材料（manual-check.md），方便人工复测。
    // 人工核查通过后由人工在 UI 推进发布就绪 = 人工确权，保证发布就绪的需求真的随时可发。
    // 发布就绪：agent 测试过 + 代码审查过 + 人工复测过，随时可发布。
    // 经验总结：发布检查通过后沉淀知识/经验/skill 改进；停留超期自动推进已完成。
    "人工核查",
    "发布就绪",
    "经验总结",
    "已完成",
    // Lightweight statuses for category=线上问题; no strict requirement lifecycle gate.
    // 流转：排查中 → 已定位 → 已修复 → 经验总结（有价值路径，与需求流经验总结统一：进入即触发自动总结任务队列）；
    // 任意阶段可直接已关闭（无沉淀价值：误报/重复/环境问题等）。
    "排查中",
    "已定位",
    "已修复",
    "经验总结",
    "已关闭",
];
/// 需求流状态（REQ_STATUSES 前 7 个，有序）：需求组聚合状态取成员状态的最小序数。
static REQ_FLOW_STATUSES: &[&str] = &[
    "需求澄清",
    "开发中",
    "自测中",
    "测试中",
    "人工核查",
    "发布就绪",
    "经验总结",
    "已完成",
];
static ISSUE_STATUSES: &[&str] = &["排查中", "已定位", "已修复", "经验总结", "已关闭"];
/// 子需求独立状态机：从大需求拆出的并行执行单元。
/// 两种路径：① 父集成模型（legacy）：需求创建 → 开发中 → 已合入，环境集成/发布在父需求做；
/// ② 整合发布模型：需求创建 → 开发中 →（自测中/测试中/发布就绪可跳级）→ 已合入（合入整合发布分支）
/// → 已发布（随发布分支生产 MR 合入封版）；发布进度由子需求独立跟踪。
static SUB_REQ_STATUSES: &[&str] = &[
    "需求创建",
    "开发中",
    "自测中",
    "测试中",
    "发布就绪",
    "已合入",
    "已发布",
    "已取消",
];
/// 仅子需求可用的状态（“开发中/自测中/测试中/发布就绪”与需求流共用）；普通需求/issue 设置这些状态会被拒。
static SUB_ONLY_STATUSES: &[&str] = &["需求创建", "已合入", "已发布", "已取消"];
static REQ_STATUS_ALIASES: &[(&str, &str)] = &[
    ("需求对齐", "需求澄清"),
    ("方案设计", "需求澄清"),
    ("待设计", "需求澄清"),
    ("待开发", "需求澄清"),
    ("待上线", "经验总结"),
    ("人工复测", "人工核查"),
    ("线上排查", "排查中"),
    ("问题排查", "排查中"),
    ("已确认", "已定位"),
    ("问题确认", "已定位"),
    // 存量兼容：问题家族旧终态「已复盘」并入统一经验总结环节（读路径自动升级）。
    ("已复盘", "经验总结"),
];
static REQ_CATEGORIES: &[&str] = &["需求", "线上问题", "测试问题", "性能问题"];
/// 需求推动方：产品推动（默认）可问产品要测试范围；开发推动必须先沉淀测试场景文档才能进入测试中。
static REQ_SOURCES: &[&str] = &["产品推动", "开发推动"];
/// 流水外标记状态：不进 REQ_STATUSES/REQ_FLOW_STATUSES 流水序列（不影响组聚合序数、
/// skipped_statuses 跳过计算、状态门禁规则），任何状态都可置为挂起，恢复时回到挂起前状态。
/// 子需求/需求组不支持挂起（子需求独立状态机拒绝，组状态为派生值被锁）。
static REQ_MARKER_STATUSES: &[&str] = &["挂起"];

#[derive(Clone)]
struct AppState {
    project_root: Arc<PathBuf>,
    data_dir: Arc<PathBuf>,
    pi_session_root: Arc<PathBuf>,
    dsh_session_root: Arc<PathBuf>,
    /// JoinHandle of the running cainiao print mock server task (None = not running).
    cainiao_mock: Arc<Mutex<Option<JoinHandle<()>>>>,
    /// Serializes auto experience-summary dispatch scans so two triggers do not start duplicate agents.
    experience_summary_dispatch: Arc<Mutex<()>>,
    /// Serializes requirement creation: {seq} allocation scans existing requirements, so
    /// scan -> reserve dir -> write meta.md must be atomic per process or two parallel
    /// creates observe the same max seq and both succeed with duplicate numbers.
    requirement_create_lock: Arc<Mutex<()>>,
    /// ONES 候选任务内存缓存（GET /api/ones/tasks 默认命中，refresh=true 回源）。
    ones_cache: Arc<Mutex<Option<OnesCache>>>,
    /// pi RPC 会话注册表：session id → 活跃 `pi --mode rpc` 子进程（全局单例）。
    pi_rpc_sessions: PiRpcRegistry,
}

#[derive(Debug, Default, Deserialize)]
struct IdQuery {
    id: Option<String>,
    #[serde(alias = "reqId")]
    req_id: Option<String>,
    #[serde(alias = "sessionId")]
    session_id: Option<String>,
    ids: Option<String>,
    days: Option<i64>,
    file: Option<String>,
    intent: Option<String>,
    budget: Option<usize>,
    /// 附件接口专用：full=1 时返回附件全文（上限 200KB）而非 600 字符摘要。
    full: Option<String>,
    tokens: Option<String>,
    #[serde(rename = "for")]
    for_agent: Option<String>,
    target: Option<String>,
    kind: Option<String>,
    q: Option<String>,
    domain: Option<String>,
    /// 知识列表/索引：按 category 过滤（大小写不敏感 contains）。
    category: Option<String>,
    /// 知识列表/agent 知识查询：按 tags 精确过滤（trim + 大小写不敏感匹配任一 tag，如「线上问题修复」）。
    tag: Option<String>,
    project: Option<String>,
    scope: Option<String>,
    /// 分支登记轮次（diff-snapshots / branch-rounds 等）：1 = 原始 branches.json。
    round: Option<u32>,
    /// 合并冲突页：仓库名（branches.json repoName）
    repo: Option<String>,
    /// 合并冲突页：需求分支（缺省取 branches.json 第一条）
    branch: Option<String>,
    status: Option<String>,
    /// 知识查询：信息观察时间下界（YYYY-MM-DD，含当天）。
    #[serde(alias = "observedAfter")]
    observed_after: Option<String>,
    /// 知识查询：信息观察时间上界（YYYY-MM-DD，含当天）。
    #[serde(alias = "observedBefore")]
    observed_before: Option<String>,
    limit: Option<usize>,
    cursor: Option<usize>,
    include_full: Option<bool>,
    section: Option<String>,
    format: Option<String>,
    /// 需求列表专用：默认不展示子需求，传 true 时包含（camelCase includeSubs 亦可）。
    #[serde(alias = "includeSubs")]
    include_subs: Option<bool>,
}

/// 运行模式：docs = hub（文档/状态 SSOT + agent 文档 API，默认，调用方唯一入口）；
/// worker = 本机执行节点（git/会话/造数/Chrome 登录态）+ 向 hub 注册心跳。
/// 分离式架构：worker 类请求由 hub 代理转发，调用方不直接感知 worker 地址。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum PanelMode {
    Docs,
    Worker,
}

impl PanelMode {
    fn resolve() -> Self {
        let args: Vec<String> = env::args().skip(1).collect();
        let env_mode = env::var(ENV_PANEL_MODE)
            .ok()
            .map(|v| v.trim().to_ascii_lowercase())
            .unwrap_or_default();
        let flag_mode = args
            .windows(2)
            .find(|w| w[0] == "--mode")
            .map(|w| w[1].to_ascii_lowercase())
            .or_else(|| {
                args.iter()
                    .find_map(|a| a.strip_prefix("--mode=").map(|v| v.to_ascii_lowercase()))
            });
        match flag_mode.or(Some(env_mode)).as_deref() {
            Some("worker") => PanelMode::Worker,
            _ => PanelMode::Docs,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            PanelMode::Docs => "docs",
            PanelMode::Worker => "worker",
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let mode = PanelMode::resolve();
    let project_root = env::current_dir().context("resolve project root")?;
    let home = home_dir()?;
    let data_dir = match mode {
        // 容器部署用 PANEL_DATA_DIR 指向数据卷。
        PanelMode::Docs => env::var(ENV_PANEL_DATA_DIR).ok().map(PathBuf::from),
        PanelMode::Worker => None,
    }
    .unwrap_or_else(|| home.join(".local/share/agent-panel"));
    let pi_session_root = home.join(".pi/agent/sessions");
    let dsh_session_root = home.join(".dsh/sessions");
    fs::create_dir_all(&data_dir).await.ok();

    let auth_token = env::var(ENV_PANEL_AUTH_TOKEN)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty());
    if mode == PanelMode::Docs {
        workers::init_worker_token();
    }
    if mode == PanelMode::Docs && auth_token.is_none() {
        // token 可选（内网/tailnet 场景用户可选择不启用鉴权）；配置后则全部 API 强制校验。
        tracing::warn!(
            "{ENV_PANEL_AUTH_TOKEN} 未配置：docs 模式不启用鉴权，任何能访问该地址的设备均可读写需求数据"
        );
    }

    let state = AppState {
        project_root: Arc::new(project_root.clone()),
        data_dir: Arc::new(data_dir),
        pi_session_root: Arc::new(pi_session_root),
        dsh_session_root: Arc::new(dsh_session_root),
        cainiao_mock: Arc::new(Mutex::new(None)),
        experience_summary_dispatch: Arc::new(Mutex::new(())),
        requirement_create_lock: Arc::new(Mutex::new(())),
        ones_cache: Arc::new(Mutex::new(None)),
        pi_rpc_sessions: Arc::new(Mutex::new(HashMap::new())),
    };

    match mode {
        PanelMode::Worker => {
            // Start the cainiao print mock server on boot if enabled in config.
            sync_cainiao_mock(&state).await;

            // 向 hub 注册并周期心跳（失败仅告警，本地服务继续——hub 恢复后自动重注册）。
            let hub_url = env::var(ENV_PANEL_HUB_URL)
                .ok()
                .filter(|v| !v.trim().is_empty())
                .unwrap_or_else(|| format!("http://127.0.0.1:{HUB_DEFAULT_PORT}"));
            let callback_base = env::var(ENV_PANEL_CALLBACK_BASE)
                .ok()
                .filter(|v| !v.trim().is_empty())
                .unwrap_or_else(|| format!("http://127.0.0.1:{WORKER_DEFAULT_PORT}"));
            workers::spawn_worker_heartbeat(hub_url, callback_base, workers::worker_machine_id())
                .await;
        }
        // 经验总结派发循环跑在 hub（2026-10-21）：开关/窗口/额度等决策配置的 SSOT 在 hub；
        // spawn 已 HTTP 下发在线 worker，容器内无 pi 也能派发。早期「循环只在 worker 跑」
        // 是本地架构遗留——分离后 worker 循环读本机配置/本机镜像需求列表，与 hub 配置
        // 和容器 job 状态分裂（实测：开 hub 开关后循环无响应）。
        PanelMode::Docs => {
            // Background: dispatch automatic experience-summary agents, then auto-advance stale summaries as a fallback.
            tokio::spawn(experience_summary_dispatch_loop(state.clone()));
            tokio::spawn(expire_stale_experience_summary_loop(state.clone()));
        }
    }

    let app = build_router(state, mode, auth_token);

    let port = match mode {
        PanelMode::Docs => env::var(ENV_PANEL_PORT)
            .ok()
            .and_then(|v| v.parse::<u16>().ok())
            .unwrap_or(HUB_DEFAULT_PORT),
        PanelMode::Worker => env::var("PORT")
            .ok()
            .and_then(|v| v.parse::<u16>().ok())
            .unwrap_or(WORKER_DEFAULT_PORT),
    };
    // 绑定地址：docs 模式默认 0.0.0.0（容器/tailnet 场景）；local 模式默认仅回环（本地个人使用
    // 安全默认）；两种模式都可用 PANEL_BIND_HOST 覆盖（远程访问局域网/Tailscale 时设 0.0.0.0）。
    let host = env::var(ENV_PANEL_BIND_HOST)
        .ok()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| match mode {
            PanelMode::Docs => "0.0.0.0".to_string(),
            PanelMode::Worker => "127.0.0.1".to_string(),
        });
    let addr: SocketAddr = host
        .parse::<std::net::IpAddr>()
        .map(|ip| SocketAddr::new(ip, port))
        .with_context(|| format!("非法 {ENV_PANEL_BIND_HOST}：{host}"))?;
    tracing::info!(%addr, mode = mode.as_str(), "Agent Panel running");
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

/// 组装路由：docs = hub 子集 + worker 代理 + worker 类端点 503 stub 兼底；
/// worker = 本机执行节点全量路由（原 local 模式路由，local 已退役）。
/// 两种模式统一挂 token 鉴权（未配置 PANEL_AUTH_TOKEN 时不拦截，行为与历史一致）。
fn build_router(state: AppState, mode: PanelMode, auth_token: Option<String>) -> Router {
    let public_dir = state.project_root.join("public");
    let router = match mode {
        PanelMode::Worker => worker_router(public_dir),
        PanelMode::Docs => docs_router(public_dir),
    };
    let router = match mode {
        // hub：worker 类请求代理转发到在线 worker（无在线 worker 时落回 503 stub）。
        PanelMode::Docs => router.layer(middleware::from_fn_with_state(
            state.clone(),
            workers::worker_proxy_middleware,
        )),
        PanelMode::Worker => router,
    };
    router
        .layer(middleware::from_fn_with_state(
            auth_token,
            auth::require_token_middleware,
        ))
        .with_state(state)
}

/// 本机执行节点全量路由（原 local 模式路由；同机部署下数据与 hub 同源，worker 直接写 SSOT）。
fn worker_router(public_dir: PathBuf) -> Router<AppState> {
    Router::new()
        .route("/health", get(health))
        .route("/api/mode", get(api_mode))
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
        .route(
            "/api/worker/experience-summary/spawn",
            post(api_worker_experience_summary_spawn),
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
        .route(
            "/api/requirement/doc-part",
            post(api_requirement_doc_part_post),
        )
        .route(
            "/api/requirement/doc-parts",
            get(api_requirement_doc_parts_get),
        )
        .route("/api/requirement/validate", post(api_requirement_validate))
        .route("/api/requirement/status", post(api_requirement_status))
        .route(
            "/api/requirement/compliance",
            get(api_requirement_compliance),
        )
        .route(
            "/api/requirement/status-flow",
            get(api_requirement_status_flow),
        )
        .route(
            "/api/requirement/review-checklist",
            get(api_requirement_review_checklist_get).put(api_requirement_review_checklist_put),
        )
        .route(
            "/api/requirement/branch-registration",
            get(api_requirement_branch_registration_get)
                .put(api_requirement_branch_registration_put),
        )
        .route(
            "/api/requirement/status-gate-detail",
            get(api_requirement_status_gate_detail),
        )
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
            "/api/requirement/pending-session",
            get(api_requirement_pending_session),
        )
        .route(
            "/api/requirement/session-candidates",
            get(api_requirement_session_candidates),
        )
        .route(
            "/api/requirement/by-session",
            get(api_requirement_by_session),
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
            "/api/requirement/review-materials",
            post(api_requirement_review_materials_post),
        )
        .route(
            "/api/requirement/review-snapshot",
            post(api_requirement_review_snapshot_post),
        )
        .route(
            "/api/requirement/dir-bundle",
            get(api_requirement_dir_bundle_get),
        )
        .route(
            "/api/requirement/master-diff",
            post(api_requirement_master_diff),
        )
        .route(
            "/api/requirement/diff-snapshots",
            get(api_requirement_diff_snapshots_get),
        )
        .route("/api/git/file-lines", get(api_git_file_lines))
        .route("/api/git/blame", get(api_git_blame))
        .route(
            "/api/requirement/branch-rounds",
            get(api_requirement_branch_rounds_get).post(api_requirement_branch_rounds_post),
        )
        .route(
            "/api/requirement/annotations",
            get(api_requirement_annotations_get).put(api_requirement_annotations_put),
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
        .route(
            "/api/requirement/merge-conflict",
            get(api_requirement_merge_conflict_detail),
        )
        .route(
            "/api/requirement/merge-conflict/resolve-file",
            post(api_requirement_merge_conflict_resolve),
        )
        .route(
            "/api/requirement/merge-conflict/complete",
            post(api_requirement_merge_conflict_complete),
        )
        .route(
            "/api/requirement/merge-conflict/abort",
            post(api_requirement_merge_conflict_abort),
        )
        .route("/api/requirement/prod-mrs", post(api_requirement_prod_mrs))
        .route(
            "/api/requirement/auto-drive",
            get(api_auto_drive).post(api_auto_drive_post),
        )
        .route("/api/requirement/recommendations", get(api_recommendations))
        .route("/api/requirement/attachments", get(api_attachments))
        .route(
            "/api/requirement/attachment",
            get(api_attachment_get).put(api_attachment_put),
        )
        .route(
            "/api/requirement/effort-estimate",
            post(api_effort_estimate),
        )
        .route(
            "/api/requirement/create-sub",
            post(api_requirement_create_sub),
        )
        .route(
            "/api/requirement/sub/init-branches",
            post(api_requirement_sub_init_branches),
        )
        .route(
            "/api/requirement/sub/sync-parent",
            post(api_requirement_sub_sync_parent),
        )
        .route(
            "/api/requirement/sub/merge-to-parent",
            post(api_requirement_sub_merge_to_parent),
        )
        .route(
            "/api/requirement/release-branch/create",
            post(api_release_branch_create),
        )
        .route(
            "/api/requirement/release-branch/list",
            get(api_release_branch_list),
        )
        .route(
            "/api/requirement/release-branch/merge-sub",
            post(api_release_branch_merge_sub),
        )
        .route(
            "/api/requirement/release-branch/sync-prod",
            post(api_release_branch_sync_prod),
        )
        .route(
            "/api/requirement/release-branch/prod-mr",
            post(api_release_branch_prod_mr),
        )
        .route(
            "/api/requirement/release-branch/mark-released",
            post(api_release_branch_mark_released),
        )
        .route("/api/sessions", get(api_sessions))
        .route("/api/session", get(api_session))
        .route("/api/session/log", get(api_session_log))
        .route("/api/sessions/resolve", get(api_sessions_resolve))
        .route(
            "/api/knowledge",
            get(api_knowledge_list)
                .post(api_knowledge_save)
                .delete(api_knowledge_delete),
        )
        .route(
            "/api/knowledge/item",
            get(api_knowledge_item).patch(api_knowledge_save),
        )
        .route("/api/knowledge/index", get(crate::api_knowledge_index))
        .route(
            "/api/agent/knowledge/index",
            get(crate::api_knowledge_index),
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
        .route("/api/apitest/apis", get(api_apitest_apis))
        .route("/api/apitest/api", get(api_apitest_api))
        .route("/api/apitest/trigger", post(api_apitest_trigger))
        .route("/api/apitest/curl-import", post(api_apitest_curl_import))
        .route(
            "/api/agent/capabilities/query",
            get(api_testdata_capabilities),
        )
        .route("/api/config", get(api_config).post(api_config_post))
        .route("/api/config/skills", get(api_config_skills))
        .route("/api/pi-chat/open", get(api_pi_chat_open))
        .route("/ws/pi-chat", get(ws_pi_chat))
        .route("/api/auth-sites", get(api_auth_sites))
        .route("/api/ones/tasks", get(api_ones_tasks))
        .route("/api/ones/manhour", get(api_ones_manhour))
        .route("/api/ones/manhour/save", post(api_ones_manhour_save))
        .route("/api/ones/manhour/log", post(api_ones_manhour_log))
        .route(
            "/api/auth-sites/:site/check",
            get(api_auth_site_check).post(api_auth_site_check),
        )
        .route("/api/auth-sites/:site/request", post(api_auth_site_request))
        .route("/api/cainiao-mock/status", get(api_cainiao_mock_status))
        .route("/api/pi-config", get(api_pi_config))
        .route("/api/harness/list", get(api_harness_list))
        .route("/api/harness/current", get(api_harness_current))
        .route("/api/harness/switch", post(api_harness_switch))
        .route("/api/harness/models", get(api_harness_models))
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
        .nest_service(
            "/assets",
            middleware::from_fn_with_state(ASSET_CACHE, cache_on_success)
                .layer(ServeDir::new(public_dir.join("dashboard-react/assets"))),
        )
        .nest_service(
            "/icons",
            middleware::from_fn_with_state(ICON_CACHE, cache_on_success)
                .layer(ServeDir::new(public_dir.join("dashboard-react/icons"))),
        )
        .route("/manifest.webmanifest", get(serve_pwa_root_file))
        .route("/sw.js", get(serve_pwa_root_file))
        .nest_service("/static", ServeDir::new(public_dir.clone()))
        .fallback(get(spa_fallback))
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
}

/// 容器 hub：WORKER 类端点统一 503，阶段二替换为 worker 代理。
async fn worker_only_stub() -> Response {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(json!({
            "error": "requires local worker",
            "message": "该端点属于本机执行类能力，docs 模式（容器 hub）不直接提供，等待阶段二 worker 代理。"
        })),
    )
        .into_response()
}

/// 免 token 的模式探测：前端据此裁剪菜单，agent 据此判断调用面。
async fn api_mode() -> Json<Value> {
    Json(json!({ "mode": PanelMode::resolve().as_str() }))
}

/// 容器 hub（docs 模式）路由：文档/状态 SSOT + agent 文档 API；WORKER 类端点挂 503 stub。
fn docs_router(public_dir: PathBuf) -> Router<AppState> {
    Router::new()
        .route("/health", get(health))
        .route("/api/mode", get(api_mode))
        .route("/api/workers", get(workers::api_workers))
        .route("/api/worker/register", post(workers::api_worker_register))
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
        .route(
            "/api/worker/experience-summary/spawn",
            post(worker_only_stub),
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
        .route(
            "/api/requirement/doc-part",
            post(api_requirement_doc_part_post),
        )
        .route(
            "/api/requirement/doc-parts",
            get(api_requirement_doc_parts_get),
        )
        .route("/api/requirement/validate", post(api_requirement_validate))
        .route("/api/requirement/status", post(api_requirement_status))
        .route(
            "/api/requirement/compliance",
            get(api_requirement_compliance),
        )
        .route(
            "/api/requirement/status-flow",
            get(api_requirement_status_flow),
        )
        .route(
            "/api/requirement/review-checklist",
            get(api_requirement_review_checklist_get).put(api_requirement_review_checklist_put),
        )
        .route(
            "/api/requirement/branch-registration",
            get(api_requirement_branch_registration_get)
                .put(api_requirement_branch_registration_put),
        )
        .route(
            "/api/requirement/status-gate-detail",
            get(api_requirement_status_gate_detail),
        )
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
            "/api/requirement/pending-session",
            get(api_requirement_pending_session),
        )
        .route(
            "/api/requirement/session-candidates",
            get(api_requirement_session_candidates),
        )
        .route(
            "/api/requirement/by-session",
            get(api_requirement_by_session),
        )
        // 审查产物读取在 hub（生成在 worker）：GET 读快照，POST 走 worker。
        .route(
            "/api/requirement/code-review",
            get(api_requirement_code_review).post(worker_only_stub),
        )
        .route(
            "/api/requirement/code-review/incremental",
            post(worker_only_stub),
        )
        .route(
            "/api/requirement/review-gate",
            get(api_requirement_review_gate),
        )
        .route("/api/requirement/review-materials", post(worker_only_stub))
        // 快照直传接收端点：hub 本地执行（worker 生成快照后 POST 过来，不经过 git 备份链）
        .route(
            "/api/requirement/review-snapshot",
            post(api_requirement_review_snapshot_post),
        )
        // 需求目录包：worker 本地副本缺失时按需物化的活链路读端点
        .route(
            "/api/requirement/dir-bundle",
            get(api_requirement_dir_bundle_get),
        )
        .route("/api/requirement/master-diff", post(worker_only_stub))
        .route(
            "/api/requirement/diff-snapshots",
            get(api_requirement_diff_snapshots_get),
        )
        .route("/api/git/file-lines", get(worker_only_stub))
        .route("/api/git/blame", get(worker_only_stub))
        .route(
            "/api/requirement/branch-rounds",
            get(api_requirement_branch_rounds_get).post(worker_only_stub),
        )
        .route(
            "/api/requirement/annotations",
            get(api_requirement_annotations_get).put(api_requirement_annotations_put),
        )
        .route("/api/requirement/sync-base", post(worker_only_stub))
        .route("/api/requirement/merge-options", get(worker_only_stub))
        .route("/api/requirement/merge-branch", post(worker_only_stub))
        .route("/api/requirement/merge-status", get(worker_only_stub))
        .route("/api/requirement/prod-mrs", post(worker_only_stub))
        .route("/api/requirement/recommendations", get(api_recommendations))
        .route("/api/requirement/attachments", get(api_attachments))
        .route(
            "/api/requirement/attachment",
            get(api_attachment_get).put(api_attachment_put),
        )
        .route(
            "/api/requirement/effort-estimate",
            post(api_effort_estimate),
        )
        .route(
            "/api/requirement/file",
            get(api_requirement_file_get).put(api_requirement_file_put),
        )
        .route(
            "/api/requirement/create-sub",
            post(api_requirement_create_sub),
        )
        .route("/api/requirement/sub/init-branches", post(worker_only_stub))
        .route("/api/requirement/sub/sync-parent", post(worker_only_stub))
        .route(
            "/api/requirement/sub/merge-to-parent",
            post(worker_only_stub),
        )
        .route(
            "/api/requirement/release-branch/create",
            post(api_release_branch_create),
        )
        .route(
            "/api/requirement/release-branch/list",
            get(api_release_branch_list),
        )
        .route(
            "/api/requirement/release-branch/merge-sub",
            post(worker_only_stub),
        )
        .route(
            "/api/requirement/release-branch/sync-prod",
            post(worker_only_stub),
        )
        .route(
            "/api/requirement/release-branch/prod-mr",
            post(worker_only_stub),
        )
        .route(
            "/api/requirement/release-branch/mark-released",
            post(api_release_branch_mark_released),
        )
        .route("/api/sessions", get(worker_only_stub))
        .route("/api/session", get(worker_only_stub))
        .route("/api/session/log", get(worker_only_stub))
        .route("/api/sessions/resolve", get(worker_only_stub))
        .route(
            "/api/knowledge",
            get(api_knowledge_list)
                .post(api_knowledge_save)
                .delete(api_knowledge_delete),
        )
        .route(
            "/api/knowledge/item",
            get(api_knowledge_item).patch(api_knowledge_save),
        )
        .route("/api/knowledge/index", get(crate::api_knowledge_index))
        .route(
            "/api/agent/knowledge/index",
            get(crate::api_knowledge_index),
        )
        .route(
            "/api/agent/knowledge/query",
            post(api_agent_knowledge_query),
        )
        .route("/api/agent/items", post(api_knowledge_save))
        .route("/api/agent/items/summary", get(api_agent_item_summary))
        .route("/api/agent/items/full", get(api_agent_item_full))
        .route("/api/agent/items/section", get(api_agent_item_full))
        .route("/api/capability/sources", get(worker_only_stub))
        .route("/api/capability/schema", get(worker_only_stub))
        .route("/api/capabilities", get(worker_only_stub))
        .route("/api/capability", get(worker_only_stub))
        .route("/api/testdata/capabilities", get(worker_only_stub))
        .route("/api/testdata/capability", get(worker_only_stub))
        .route("/api/testdata/run", post(worker_only_stub))
        .route("/api/apitest/apis", get(worker_only_stub))
        .route("/api/apitest/api", get(worker_only_stub))
        .route("/api/apitest/trigger", post(worker_only_stub))
        .route("/api/agent/capabilities/query", get(worker_only_stub))
        .route("/api/config", get(api_config).post(api_config_post))
        .route("/api/config/skills", get(worker_only_stub))
        .route("/api/pi-chat/open", get(worker_only_stub))
        .route("/ws/pi-chat", get(worker_only_stub))
        .route("/api/auth-sites", get(worker_only_stub))
        .route("/api/ones/tasks", get(worker_only_stub))
        .route("/api/ones/manhour", get(worker_only_stub))
        .route("/api/ones/manhour/save", post(worker_only_stub))
        .route("/api/ones/manhour/log", post(worker_only_stub))
        .route(
            "/api/auth-sites/:site/check",
            get(worker_only_stub).post(worker_only_stub),
        )
        .route("/api/auth-sites/:site/request", post(worker_only_stub))
        .route("/api/cainiao-mock/status", get(worker_only_stub))
        .route("/api/harness/list", get(worker_only_stub))
        .route("/api/harness/current", get(worker_only_stub))
        .route("/api/harness/switch", post(worker_only_stub))
        .route("/api/harness/models", get(worker_only_stub))
        .route("/api/pi-config", get(worker_only_stub))
        .route(
            "/api/pi-config/file",
            get(worker_only_stub).post(worker_only_stub),
        )
        .route("/api/pi-config/settings", post(worker_only_stub))
        .route("/api/notifications", get(api_notifications))
        .route(
            "/api/notifications/unread-count",
            get(api_notifications_unread_count),
        )
        .route("/api/notifications/dismiss", post(ok_json))
        .route("/api/notifications/mark-read", post(ok_json))
        .nest_service(
            "/assets",
            middleware::from_fn_with_state(ASSET_CACHE, cache_on_success)
                .layer(ServeDir::new(public_dir.join("dashboard-react/assets"))),
        )
        .nest_service("/static", ServeDir::new(public_dir.clone()))
        .nest_service(
            "/icons",
            middleware::from_fn_with_state(ICON_CACHE, cache_on_success)
                .layer(ServeDir::new(public_dir.join("dashboard-react/icons"))),
        )
        .route("/manifest.webmanifest", get(serve_pwa_root_file))
        .route("/sw.js", get(serve_pwa_root_file))
        .fallback(get(spa_fallback))
        .layer(TraceLayer::new_for_http())
}

/// /assets/* 为 Vite 内容哈希产物：文件名变即逻辑失效，可安全 immutable 一年。
const ASSET_CACHE: &str = "public, max-age=31536000, immutable";
/// /icons/* 非哈希命名（icon-192.png 等）：长缓存但保留到期 Last-Modified 重验。
const ICON_CACHE: &str = "public, max-age=604800";

/// 静态资源 Cache-Control 注入：仅对成功响应生效（404/304 不带缓存头，避免负缓存）。
/// /static 混挂整个 public（含未哈希文件），维持 ServeDir 默认重验行为，不注入长缓存。
async fn cache_on_success(State(value): State<&'static str>, req: Request, next: Next) -> Response {
    let mut res = next.run(req).await;
    if res.status().is_success() {
        res.headers_mut()
            .insert(header::CACHE_CONTROL, HeaderValue::from_static(value));
    }
    res
}

/// PWA 根路径静态文件（manifest / sw.js）：白名单文件名防路径穿越，从 vite 构建产物读取。
/// no-cache 强制每次重验，保证发版后 SW 更新与 manifest 引用不被陈旧缓存拖住。
/// 路径基于 AppState.project_root（与 spa_fallback 同源），不依赖进程 cwd。
async fn serve_pwa_root_file(State(state): State<AppState>, uri: Uri) -> Response {
    let name = uri.path().trim_start_matches('/');
    let (allowed, mime) = match name {
        "manifest.webmanifest" => (true, "application/manifest+json"),
        "sw.js" => (true, "application/javascript"),
        _ => (false, ""),
    };
    if !allowed {
        return StatusCode::NOT_FOUND.into_response();
    }
    let path = state
        .project_root
        .join(format!("public/dashboard-react/{name}"));
    match tokio::fs::read(&path).await {
        Ok(bytes) => (
            [
                (header::CONTENT_TYPE, mime.to_string()),
                (header::CACHE_CONTROL, "no-cache".to_string()),
                // sw.js 需要根 scope；Service-Worker-Allowed 放宽默认目录 scope。
                (
                    header::HeaderName::from_static("service-worker-allowed"),
                    "/".to_string(),
                ),
            ],
            bytes,
        )
            .into_response(),
        Err(_) => StatusCode::NOT_FOUND.into_response(),
    }
}

/// SPA fallback：index.html 带 `Cache-Control: no-cache` + 弱 ETag（mtime+size）。
/// 浏览器每次导航需重验，If-None-Match 命中时 304，未命中才重拉整页；
/// 部署后文件变化 → ETag 变化 → 必然拿到引用新哈希资源的新 HTML。
async fn spa_fallback(State(state): State<AppState>, uri: Uri, headers: HeaderMap) -> Response {
    let path = uri.path();
    if path.starts_with("/api/") || path.starts_with("/ws/") {
        return (StatusCode::NOT_FOUND, Json(json!({ "error": "not found" }))).into_response();
    }
    let index = state.project_root.join("public/dashboard-react/index.html");
    let meta = match fs::metadata(&index).await {
        Ok(meta) => meta,
        Err(_) => return spa_not_built_response(),
    };
    let etag = weak_index_etag(&meta);
    if if_none_match_hits(&headers, &etag) {
        return (StatusCode::NOT_MODIFIED, [(header::ETAG, etag.as_str())]).into_response();
    }
    match fs::read_to_string(index).await {
        Ok(html) => (
            [
                (header::CACHE_CONTROL, "no-cache"),
                (header::ETAG, etag.as_str()),
            ],
            Html(html),
        )
            .into_response(),
        Err(_) => spa_not_built_response(),
    }
}

fn spa_not_built_response() -> Response {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Html(String::from("<h1>Agent Panel frontend is not built</h1><p>Run <code>bun run build:dashboard</code>.</p>")),
    )
        .into_response()
}

/// index.html 的弱 ETag：mtime 秒 + 字节长度，与 ServeDir 的 Last-Modified 语义一致。
fn weak_index_etag(meta: &std::fs::Metadata) -> String {
    let mtime_secs = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("W:\"{:x}-{:x}\"", mtime_secs, meta.len())
}

/// If-None-Match 按 RFC 9110 弱比较：逗号分隔逐项精确匹配（`*` 视为命中）。
fn if_none_match_hits(headers: &HeaderMap, etag: &str) -> bool {
    let Some(if_none_match) = headers
        .get(header::IF_NONE_MATCH)
        .and_then(|value| value.to_str().ok())
    else {
        return false;
    };
    if_none_match
        .split(',')
        .map(str::trim)
        .any(|candidate| candidate == "*" || candidate == etag)
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
    // 子需求是父需求的并行执行单元，不独立计入面板统计（进度在父需求详情展示）。
    reqs.retain(|r| !r.is_sub_req);
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
