//! Worker 注册表与代理（分离式架构：hub 转发执行类请求到本机 worker）。
//!
//! worker（`--mode worker`）启动后向 hub `POST /api/worker/register` 注册并每 30s 续期
//! （幂等 upsert，兼做心跳）；hub 收到 worker 类端点请求时，若注册表中有在线 worker
//! 则把请求原样转发到 worker 的 `callbackBase`，无在线 worker 时落回路由上的 503 stub。
//!
//! 数据流约束：worker 与 hub 同机时共享同一数据目录（worker 直接写 SSOT 无分叉）；
//! 远端 hub 场景 worker 产物的写回（hub file API）为阶段 2.5 工作，当前 worker 只写本地。

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::{
    body::Body,
    extract::{Request, State},
    http::{header, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::AppState;

/// worker 心跳间隔；超过 3 个周期未续期判 offline。
pub(crate) const WORKER_HEARTBEAT_INTERVAL_SECS: u64 = 30;
const WORKER_OFFLINE_AFTER_SECS: u64 = WORKER_HEARTBEAT_INTERVAL_SECS * 3;
const WORKERS_FILE: &str = "workers.json";

/// hub → worker 转发的路径白名单：docs_router 上挂 worker_only_stub 的全部 path
/// （/ws/pi-chat 与 /api/pi-chat/open 的 WebSocket 代理暂不收录，保持 stub）。
pub(crate) const WORKER_PROXY_PATHS: &[&str] = &[
    // 审查/差异生成（git 执行类）
    "/api/requirement/code-review",
    "/api/requirement/code-review/incremental",
    "/api/requirement/review-materials",
    "/api/requirement/master-diff",
    "/api/git/file-lines",
    "/api/git/blame",
    // branch-rounds（GET/POST 纯文件轮次登记）不在此列：hub 直挂读写 SSOT，
    // worker 缺登记时经 fetch_branch_scope_from_hub 按需物化（2026-10-23 迁回 hub）。
    "/api/requirement/sync-base",
    // 分支合并/发布分支执行
    "/api/requirement/merge-options",
    "/api/requirement/merge-branch",
    "/api/requirement/merge-status",
    "/api/requirement/prod-mrs",
    "/api/requirement/sub/init-branches",
    "/api/requirement/sub/sync-parent",
    "/api/requirement/sub/merge-to-parent",
    "/api/requirement/release-branch/merge-sub",
    "/api/requirement/release-branch/sync-prod",
    "/api/requirement/release-branch/prod-mr",
    // pi 会话扫描
    "/api/sessions",
    "/api/session",
    "/api/session/log",
    "/api/sessions/resolve",
    // testdata/接口测试
    "/api/capability",
    "/api/capability/schema",
    "/api/capability/sources",
    "/api/capabilities",
    "/api/testdata/capabilities",
    "/api/testdata/capability",
    "/api/testdata/run",
    "/api/apitest/apis",
    "/api/apitest/api",
    "/api/apitest/trigger",
    "/api/agent/capabilities/query",
    // 经验总结 agent spawn（hub 决策后下发 worker 本机执行 pi）
    "/api/worker/experience-summary/spawn",
    // 本机资源：Chrome 登录态/ONES/cainiao mock/pi 配置/harness
    "/api/config/skills",
    "/api/auth-sites",
    "/api/ones/tasks",
    "/api/ones/manhour",
    "/api/ones/manhour/save",
    "/api/ones/manhour/log",
    "/api/cainiao-mock/status",
    "/api/harness/list",
    "/api/harness/current",
    "/api/harness/switch",
    "/api/harness/models",
    "/api/pi-config",
    "/api/pi-config/file",
    "/api/pi-config/settings",
];

/// 带路径参数的 worker 路由用前缀匹配（其余精确匹配）。
const WORKER_PROXY_PATH_PREFIXES: &[&str] = &["/api/auth-sites/"];

pub(crate) fn is_worker_path(path: &str) -> bool {
    WORKER_PROXY_PATHS.contains(&path)
        || WORKER_PROXY_PATH_PREFIXES
            .iter()
            .any(|p| path.starts_with(p))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct WorkerEntry {
    #[serde(rename = "machineId")]
    pub machine_id: String,
    #[serde(rename = "callbackBase")]
    pub callback_base: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(rename = "registeredAt")]
    pub registered_at: u64,
    #[serde(rename = "lastHeartbeatAt")]
    pub last_heartbeat_at: u64,
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn workers_file(data_dir: &std::path::Path) -> PathBuf {
    data_dir.join(WORKERS_FILE)
}

fn load_registry(data_dir: &std::path::Path) -> BTreeMap<String, WorkerEntry> {
    let Ok(text) = std::fs::read_to_string(workers_file(data_dir)) else {
        return BTreeMap::new();
    };
    #[derive(Deserialize)]
    struct File {
        #[serde(default)]
        workers: Vec<WorkerEntry>,
    }
    serde_json::from_str::<File>(&text)
        .map(|f| {
            f.workers
                .into_iter()
                .map(|w| (w.machine_id.clone(), w))
                .collect()
        })
        .unwrap_or_default()
}

fn save_registry(data_dir: &std::path::Path, registry: &BTreeMap<String, WorkerEntry>) {
    let file = FilePayload {
        version: 1,
        workers: registry.values().cloned().collect(),
    };
    if let Ok(text) = serde_json::to_string_pretty(&file) {
        let tmp = workers_file(data_dir).with_extension("json.tmp");
        if std::fs::write(&tmp, text).is_ok() {
            let _ = std::fs::rename(&tmp, workers_file(data_dir));
        }
    }
}

#[derive(Serialize)]
struct FilePayload {
    version: u8,
    workers: Vec<WorkerEntry>,
}

fn is_online(entry: &WorkerEntry) -> bool {
    now_secs().saturating_sub(entry.last_heartbeat_at) < WORKER_OFFLINE_AFTER_SECS
}

fn normalize_callback_base(raw: &str) -> String {
    raw.trim().trim_end_matches('/').to_string()
}

/// POST /api/worker/register：注册/心跳幂等 upsert（machineId 为键）。
pub(crate) async fn api_worker_register(
    State(state): State<AppState>,
    Json(form): Json<RegisterForm>,
) -> Response {
    if form.machine_id.trim().is_empty() || normalize_callback_base(&form.callback_base).is_empty()
    {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "machineId 与 callbackBase 必填"})),
        )
            .into_response();
    }
    let mut registry = load_registry(&state.data_dir);
    let entry = registry
        .entry(form.machine_id.trim().to_string())
        .or_insert_with(|| WorkerEntry {
            machine_id: form.machine_id.trim().to_string(),
            callback_base: normalize_callback_base(&form.callback_base),
            tags: form.tags.clone().unwrap_or_default(),
            registered_at: now_secs(),
            last_heartbeat_at: now_secs(),
        });
    entry.callback_base = normalize_callback_base(&form.callback_base);
    entry.last_heartbeat_at = now_secs();
    if let Some(tags) = &form.tags {
        entry.tags = tags.clone();
    }
    save_registry(&state.data_dir, &registry);
    Json(json!({"ok": true, "online": registry.values().filter(|w| is_online(w)).count()}))
        .into_response()
}

#[derive(Deserialize)]
pub(crate) struct RegisterForm {
    #[serde(rename = "machineId")]
    machine_id: String,
    #[serde(rename = "callbackBase")]
    callback_base: String,
    #[serde(default)]
    tags: Option<Vec<String>>,
}

/// GET /api/workers：注册表快照（含 online 状态），观测/调试用。
pub(crate) async fn api_workers(State(state): State<AppState>) -> Response {
    let registry = load_registry(&state.data_dir);
    let workers: Vec<serde_json::Value> = registry
        .values()
        .map(|w| {
            json!({
                "machineId": w.machine_id,
                "callbackBase": w.callback_base,
                "tags": w.tags,
                "registeredAt": w.registered_at,
                "lastHeartbeatAt": w.last_heartbeat_at,
                "online": is_online(w),
            })
        })
        .collect();
    Json(json!({ "workers": workers })).into_response()
}

/// 选一个在线 worker（当前只有一个本机 worker；多 worker 时按最近心跳）。
pub(crate) fn pick_online_worker(data_dir: &std::path::Path) -> Option<WorkerEntry> {
    load_registry(data_dir)
        .values()
        .filter(|w| is_online(w))
        .max_by_key(|w| w.last_heartbeat_at)
        .cloned()
}

/// hub → worker 转发用的 token：PANEL_WORKER_TOKEN 优先，缺省回落 PANEL_AUTH_TOKEN。
static WORKER_TOKEN: OnceLock<Option<String>> = OnceLock::new();

pub(crate) fn init_worker_token() {
    let token = std::env::var("PANEL_WORKER_TOKEN")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .or_else(|| {
            std::env::var("PANEL_AUTH_TOKEN")
                .ok()
                .filter(|v| !v.trim().is_empty())
        });
    let _ = WORKER_TOKEN.set(token);
}

pub(crate) fn proxy_client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(Duration::from_secs(300))
            .build()
            .expect("build proxy client")
    })
}

/// hub 内部直调 worker 时带的 token（同代理转发头语义）。
pub(crate) fn worker_forward_token() -> Option<String> {
    WORKER_TOKEN.get().and_then(|t| t.as_ref()).cloned()
}

/// worker 代理中间件：路径命中 WORKER_PROXY_PATHS 且有在线 worker 时转发，
/// 否则透传给后续路由（落回 503 stub / 正常 handler）。
pub(crate) async fn worker_proxy_middleware(
    State(state): State<AppState>,
    req: Request<Body>,
    next: Next,
) -> Response {
    let path = req.uri().path().to_string();
    if !is_worker_path(&path) {
        return next.run(req).await;
    }
    let Some(worker) = pick_online_worker(&state.data_dir) else {
        return next.run(req).await;
    };
    let method = req.method().clone();
    let query = req.uri().query().map(|q| q.to_string());
    let content_type = req
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.to_string());
    let body = match axum::body::to_bytes(req.into_body(), usize::MAX).await {
        Ok(bytes) => bytes,
        Err(err) => {
            return (
                StatusCode::BAD_GATEWAY,
                Json(json!({"error": "proxy read body failed", "message": err.to_string()})),
            )
                .into_response()
        }
    };

    let mut url = format!("{}{}", worker.callback_base, path);
    if let Some(q) = &query {
        url.push('?');
        url.push_str(q);
    }
    let mut request = proxy_client().request(method, &url);
    if let Some(ct) = &content_type {
        request = request.header(header::CONTENT_TYPE, ct);
    }
    if let Some(token) = WORKER_TOKEN.get().and_then(|t| t.as_ref()) {
        request = request.header("X-Worker-Token", token);
    }
    match request.body(body).send().await {
        Ok(resp) => {
            let status =
                StatusCode::from_u16(resp.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
            let content_type = resp
                .headers()
                .get(header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .map(|v| v.to_string());
            match resp.bytes().await {
                Ok(bytes) => {
                    let mut response = (status, bytes).into_response();
                    if let Some(ct) = content_type {
                        response
                            .headers_mut()
                            .insert(header::CONTENT_TYPE, ct.parse().unwrap());
                    }
                    response
                }
                Err(err) => (
                    StatusCode::BAD_GATEWAY,
                    Json(
                        json!({"error": "proxy read response failed", "message": err.to_string()}),
                    ),
                )
                    .into_response(),
            }
        }
        Err(err) => (
            StatusCode::BAD_GATEWAY,
            Json(json!({
                "error": "worker unreachable",
                "message": format!("worker {} 不可达：{}", worker.machine_id, err),
                "workerCallbackBase": worker.callback_base,
            })),
        )
            .into_response(),
    }
}

/// worker 侧：向 hub 周期注册（幂等，兼做心跳）。hub 不可达时仅告警，worker 本地服务继续——
/// hub 恢复后下一轮心跳自动重新注册（兜底语义）。
pub(crate) async fn spawn_worker_heartbeat(
    hub_url: String,
    callback_base: String,
    machine_id: String,
) {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .expect("build heartbeat client");
    tokio::spawn(async move {
        let url = format!("{}/api/worker/register", hub_url.trim_end_matches('/'));
        loop {
            match client
                .post(&url)
                .json(&json!({
                    "machineId": machine_id,
                    "callbackBase": callback_base,
                    "tags": ["local-execution"],
                }))
                .send()
                .await
            {
                Ok(resp) if resp.status().is_success() => {}
                Ok(resp) => tracing::warn!(status = %resp.status(), "worker 注册/心跳被 hub 拒绝"),
                Err(err) => {
                    tracing::warn!(error = %err, hub = %url, "worker 心跳失败（hub 不可达，本地服务继续）")
                }
            }
            tokio::time::sleep(Duration::from_secs(WORKER_HEARTBEAT_INTERVAL_SECS)).await;
        }
    });
}

/// worker 的 machineId：hostname 兜底。
pub(crate) fn worker_machine_id() -> String {
    std::fs::read_to_string("/etc/hostname")
        .map(|s| s.trim().to_string())
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(|| hostname_from_env())
        .unwrap_or_else(|| "local-worker".to_string())
}

fn hostname_from_env() -> Option<String> {
    std::env::var("HOSTNAME").ok().filter(|s| !s.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(machine: &str, callback: &str, heartbeat_age_secs: u64) -> WorkerEntry {
        WorkerEntry {
            machine_id: machine.to_string(),
            callback_base: callback.to_string(),
            tags: vec!["local-execution".to_string()],
            registered_at: now_secs() - 120,
            last_heartbeat_at: now_secs() - heartbeat_age_secs,
        }
    }

    #[test]
    fn register_upserts_and_heartbeat_renews() {
        let tmp = tempfile::tempdir().unwrap();
        let data_dir = tmp.path();
        let registry = load_registry(data_dir);
        assert!(registry.is_empty());

        // 模拟 register 幂等 upsert：首注册 + 二次心跳更新时间戳。
        let mut registry = load_registry(data_dir);
        let e = entry("box", "http://127.0.0.1:7332", 0);
        registry.insert(e.machine_id.clone(), e);
        save_registry(data_dir, &registry);

        let reloaded = load_registry(data_dir);
        assert_eq!(reloaded.len(), 1);
        assert_eq!(reloaded["box"].callback_base, "http://127.0.0.1:7332");
        assert!(is_online(&reloaded["box"]));
    }

    #[test]
    fn stale_heartbeat_is_offline() {
        let tmp = tempfile::tempdir().unwrap();
        let mut registry = load_registry(tmp.path());
        registry.insert(
            "box".to_string(),
            entry(
                "box",
                "http://127.0.0.1:7332",
                WORKER_OFFLINE_AFTER_SECS + 1,
            ),
        );
        assert!(pick_online_worker(tmp.path()).is_none());
    }

    #[test]
    fn pick_prefers_most_recent_heartbeat() {
        let tmp = tempfile::tempdir().unwrap();
        let mut registry = load_registry(tmp.path());
        registry.insert("a".to_string(), entry("a", "http://10.0.0.1:7332", 10));
        registry.insert("b".to_string(), entry("b", "http://10.0.0.2:7332", 1));
        save_registry(tmp.path(), &registry);
        assert_eq!(pick_online_worker(tmp.path()).unwrap().machine_id, "b");
    }

    #[test]
    fn worker_path_matching_exact_and_prefix() {
        assert!(is_worker_path("/api/sessions"));
        assert!(is_worker_path("/api/requirement/review-materials"));
        assert!(is_worker_path("/api/auth-sites/ones/check"));
        // branch-rounds GET/POST 纯文件轮次登记，hub 直挂不经 worker 代理（2026-10-23 迁回）。
        assert!(!is_worker_path("/api/requirement/branch-rounds"));
        assert!(!is_worker_path("/api/requirements"));
        assert!(!is_worker_path("/api/requirement/edit"));
        assert!(!is_worker_path("/ws/pi-chat"));
    }
}
