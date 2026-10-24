//! pi --mode rpc 子进程管理与 WebSocket 转发。
//!
//! 设计动机：让「在 Agent Panel 里使用 pi agent」与 SSH 会话解耦——
//! pi RPC 进程挂在 agent-panel（systemd user service）下运行，浏览器只是前端；
//! SSH 断开 / 关闭页面都不影响 pi 继续执行。session JSONL 持久化在
//! `~/.pi/agent/sessions/`，进程退出后用同一 `--session-id` 重新 spawn
//! 即可带完整上下文恢复（pi 原生行为：--session-id 存在则恢复，不存在则创建）。
//!
//! 协议：pi RPC 是 stdin/stdout 上的 JSONL（严格 LF 分帧，见 pi docs/rpc.md）。
//! 后端对协议内容不解释、不改写：浏览器 WS 发来的每条 Text 原样写入 pi stdin，
//! pi stdout 的每行 JSON 原样广播给所有订阅的 WS 客户端；仅附加 `panel_*`
//! 前缀的控制事件（attached / rpc_exit / lagged）用于前端连接状态管理。

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use anyhow::{bail, Context, Result};
use axum::{
    extract::{Query, State, WebSocketUpgrade},
    response::Response,
};
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{ChildStdin, Command};
use tokio::sync::{broadcast, Mutex};
use uuid::Uuid;

use crate::{get_requirement, requirement_project_root, sessions, ApiError, ApiResult, AppState};
use axum::Json;
use serde_json::{json, Value};

/// stderr 尾部保留行数（进程退出时随 panel_rpc_exit 回放，便于排障）。
const STDERR_TAIL_LINES: usize = 40;
/// 事件广播缓冲：pi 流式输出高频时避免订阅慢的客户端拖垮 stdout 读取任务。
const EVENT_BUFFER: usize = 2048;

// ---- 远程 spawn（agent-runner 容器，Phase 2） ----

/// `PANEL_PI_REMOTE_HOST`：如 "root@192.168.6.137"。设置后 pi RPC 经 ssh 远程 spawn；未设置 = 本地。
fn remote_host() -> Option<String> {
    std::env::var("PANEL_PI_REMOTE_HOST")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// `PANEL_PI_MAX_RPC_SESSIONS`：panel 管理的最大并行 pi 会话数（默认 8）。
fn max_rpc_sessions() -> usize {
    std::env::var("PANEL_PI_MAX_RPC_SESSIONS")
        .ok()
        .and_then(|v| v.trim().parse().ok())
        .filter(|&n| n > 0)
        .unwrap_or(8)
}

/// `PANEL_PI_REMOTE_CWD_MAP`："工作站前缀=远程前缀,..."（最长前缀优先）。
/// 例："/home/hevin/Developer=/root/Developer"。
fn remote_cwd_map() -> Vec<(String, String)> {
    std::env::var("PANEL_PI_REMOTE_CWD_MAP")
        .unwrap_or_default()
        .split(',')
        .filter_map(|pair| {
            let (from, to) = pair.split_once('=')?;
            let (from, to) = (from.trim(), to.trim());
            (!from.is_empty() && !to.is_empty()).then(|| (from.to_string(), to.to_string()))
        })
        .collect()
}

/// `PANEL_PI_REMOTE_DEFAULT_CWD`：cwd 无映射匹配时的远程回退目录。
fn remote_default_cwd() -> String {
    std::env::var("PANEL_PI_REMOTE_DEFAULT_CWD")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "/root/Developer".to_string())
}

/// 把工作站 cwd 映射到远程路径（最长前缀替换，无匹配回退默认目录）。
fn map_cwd_to_remote(cwd: &Path) -> String {
    let cwd_str = cwd.to_string_lossy();
    let mut best: Option<(usize, String)> = None;
    for (from, to) in remote_cwd_map() {
        if cwd_str.starts_with(&from) {
            let replaced = format!("{}{}", to, &cwd_str[from.len()..]);
            let hit = best.take();
            best = match hit {
                Some((len, path)) if len >= from.len() => Some((len, path)),
                _ => Some((from.len(), replaced)),
            };
        }
    }
    best.map(|(_, path)| path)
        .unwrap_or_else(remote_default_cwd)
}

fn ssh_common_args() -> Vec<String> {
    [
        "-o",
        "BatchMode=yes",
        "-o",
        "ConnectTimeout=10",
        "-o",
        "ServerAliveInterval=30",
        "-o",
        "ServerAliveCountMax=3",
    ]
    .into_iter()
    .map(String::from)
    .collect()
}

/// 在远程主机执行脚本并返回 stdout（未配置远程时在本地执行同一脚本）。
async fn run_script(script: &str) -> Result<String> {
    let mut cmd = match remote_host() {
        Some(host) => {
            let mut c = Command::new("ssh");
            c.args(ssh_common_args()).arg(&host).arg(script);
            c
        }
        None => {
            let mut c = Command::new("sh");
            c.arg("-c").arg(script);
            c
        }
    };
    let out = cmd
        .stdin(Stdio::null())
        .output()
        .await
        .with_context(|| format!("执行进程扫描失败: {script}"))?;
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

/// 一条 pi --mode rpc 进程的快照。
#[derive(Debug, Clone)]
pub(crate) struct RpcProcInfo {
    pub pid: u32,
    /// spawn 时注入的 PANEL_SESSION_ID（外部进程为 None）。
    pub session_id: Option<String>,
    /// 已运行秒数。
    pub etimes: i64,
    /// 常驻内存 KB。
    pub rss_kb: i64,
}

fn parse_ps_output(out: &str) -> Vec<RpcProcInfo> {
    out.lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|line| {
            let mut it = line.split_whitespace();
            let pid = it.next()?.parse().ok()?;
            let etimes = it.next()?.parse().ok()?;
            let rss_kb = it.next()?.parse().ok()?;
            let session_id = it.next().filter(|s| *s != "-").map(String::from);
            Some(RpcProcInfo {
                pid,
                session_id,
                etimes,
                rss_kb,
            })
        })
        .collect()
}

/// 扫描 pi 进程（远程或本地）。
///
/// pi 会用 node 改写进程标题（comm=pi、args="pi"），按 cmdline 匹配必然落空；
/// 改为匹配 comm + /proc/<pid>/environ 里 spawn 时注入的 PANEL_SESSION_ID。
const PI_PROC_SCAN: &str = "ps -eo pid=,etimes=,rss=,comm= | awk '$4==\"pi\"{print $1, $2, $3}' | while read -r pid etimes rss; do sid=$(tr '\\0' '\\n' < /proc/$pid/environ 2>/dev/null | sed -n 's/^PANEL_SESSION_ID=//p' | head -1); echo \"$pid $etimes $rss ${sid:--}\"; done";

async fn pi_rpc_processes() -> Result<Vec<RpcProcInfo>> {
    let out = run_script(PI_PROC_SCAN).await?;
    Ok(parse_ps_output(&out))
}

/// 宿主内存快照（MB）：返回 (total, used)。
async fn host_memory() -> Option<(i64, i64)> {
    let out = run_script("free -m | awk 'NR==2{print $2, $3}'")
        .await
        .ok()?;
    let nums: Vec<i64> = out
        .split_whitespace()
        .filter_map(|t| t.parse().ok())
        .collect();
    (nums.len() == 2).then(|| (nums[0], nums[1]))
}

fn human_etimes(etimes: i64) -> String {
    let (h, rest) = (etimes / 3600, etimes % 3600);
    if h > 0 {
        format!("{h}h{}m{}s", rest / 60, rest % 60)
    } else {
        let (m, s) = (rest / 60, rest % 60);
        format!("{m}m{s}s")
    }
}

/// GET /api/pi/rpc-sessions：运行中的 pi RPC 会话清单（panel 注册表 + 宿主进程扫描）
/// + 宿主内存快照。远程模式（PANEL_PI_REMOTE_HOST）下列出 agent-runner 上的进程。
pub(crate) async fn api_pi_rpc_sessions(State(state): State<AppState>) -> ApiResult<Json<Value>> {
    let procs = pi_rpc_processes().await?;
    let memory = host_memory().await;
    let registry = state.pi_rpc_sessions.lock().await;
    let registry_list: Vec<Value> = registry
        .iter()
        .map(|(id, s)| {
            json!({
                "sessionId": id,
                "running": s.is_running(),
                "source": "panel",
            })
        })
        .collect();
    let panel_ids: Vec<&String> = registry.keys().collect();
    let processes: Vec<Value> = procs
        .iter()
        .map(|p| {
            let managed = p
                .session_id
                .as_ref()
                .map(|id| panel_ids.contains(&id))
                .unwrap_or(false);
            json!({
                "pid": p.pid,
                "sessionId": p.session_id,
                "managed": managed,
                "rssKb": p.rss_kb,
                "rssMb": p.rss_kb / 1024,
                "etime": human_etimes(p.etimes),
            })
        })
        .collect();
    let total_rss_mb = procs.iter().map(|p| p.rss_kb).sum::<i64>() / 1024;
    Ok(Json(json!({
        "remote": remote_host().is_some(),
        "host": remote_host(),
        "maxSessions": max_rpc_sessions(),
        "registry": registry_list,
        "processes": processes,
        "totalRssMb": total_rss_mb,
        "memory": memory.map(|(total, used)| json!({"totalMb": total, "usedMb": used})),
    })))
}

/// POST /api/pi/rpc-sessions/stop：按 sessionId 停止运行中的 pi RPC 会话。
/// 语义：SIGTERM（1.5s 后仍在则 SIGKILL），并从 panel 注册表移除。
#[derive(Debug, Deserialize)]
pub(crate) struct PiRpcStopPayload {
    #[serde(alias = "sessionId")]
    session_id: Option<String>,
    pid: Option<u32>,
}

async fn kill_pid(sig: &str, pid: &str) {
    let script = format!(
        "kill {} {} 2>/dev/null; true",
        sig,
        crate::util::shell_quote(pid)
    );
    let _ = run_script(&script).await;
}

pub(crate) async fn api_pi_rpc_stop(
    State(state): State<AppState>,
    Json(payload): Json<PiRpcStopPayload>,
) -> ApiResult<Json<Value>> {
    let procs = pi_rpc_processes().await?;
    // 定位目标：sessionId 精确匹配，或 pid 匹配（pid 必须在 pi 进程扫描结果内，防误杀任意进程）
    let target = if let Some(sid) = payload.session_id.as_deref().map(str::trim) {
        if Uuid::parse_str(sid).is_err() {
            return Err(ApiError::bad_request(format!("非法 sessionId：{sid}")));
        }
        procs
            .iter()
            .find(|p| p.session_id.as_deref() == Some(sid))
            .map(|p| (p.pid, Some(sid.to_string())))
    } else if let Some(pid) = payload.pid {
        procs
            .iter()
            .find(|p| p.pid == pid)
            .map(|p| (p.pid, p.session_id.clone()))
    } else {
        None
    };
    let Some((pid, session_id)) = target else {
        // 无活进程：仅清理注册表残留
        if let Some(sid) = payload.session_id.as_deref().map(str::trim) {
            state.pi_rpc_sessions.lock().await.remove(sid);
        }
        return Ok(Json(json!({
            "stopped": false,
            "sessionId": payload.session_id,
            "pid": payload.pid,
            "detail": "未找到运行中的目标 pi 进程",
        })));
    };
    let session_id = session_id.unwrap_or_default();
    let pid_str = pid.to_string();
    kill_pid("-TERM", &pid_str).await;
    tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
    if pi_rpc_processes().await?.iter().any(|p| p.pid == pid) {
        kill_pid("-KILL", &pid_str).await;
    }
    if !session_id.is_empty() {
        state.pi_rpc_sessions.lock().await.remove(&session_id);
    }
    Ok(Json(json!({
        "stopped": true,
        "sessionId": session_id,
        "pid": pid,
    })))
}

/// 一个 pi RPC 子进程的包装；同一 session id 全局至多一个活进程（注册表单例）。
pub(crate) struct PiRpcSession {
    session_id: String,
    /// pi stdin 写句柄；进程退出后置 None。
    stdin: Mutex<Option<ChildStdin>>,
    /// pi stdout JSON 行广播通道。
    events: broadcast::Sender<String>,
    /// stderr 尾部环形缓冲。
    stderr_tail: Mutex<VecDeque<String>>,
    /// 进程是否仍在运行。
    running: AtomicBool,
}

impl PiRpcSession {
    fn broadcast(&self, value: &Value) {
        let _ = self.events.send(value.to_string());
    }

    /// 向 pi stdin 写一行 JSON（RPC 协议：严格 LF 分帧）。
    async fn send_line(&self, line: &str) -> Result<()> {
        let mut guard = self.stdin.lock().await;
        match guard.as_mut() {
            Some(stdin) => {
                stdin
                    .write_all(line.trim_end_matches(['\r', '\n']).as_bytes())
                    .await?;
                stdin.write_all(b"\n").await?;
                stdin.flush().await?;
                Ok(())
            }
            None => bail!("pi rpc 进程已退出，请重新 attach"),
        }
    }

    fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }
}

/// 注册表：session id → 活跃 RPC 进程。
pub(crate) type PiRpcRegistry = Arc<Mutex<HashMap<String, Arc<PiRpcSession>>>>;

/// attach（复用活进程）或 spawn 新的 pi RPC 进程，注册表内去重。
async fn attach_pi_rpc(
    state: &AppState,
    session_id: &str,
    cwd: &Path,
) -> Result<(Arc<PiRpcSession>, bool)> {
    let mut registry = state.pi_rpc_sessions.lock().await;
    if let Some(existing) = registry.get(session_id) {
        if existing.is_running() {
            return Ok((Arc::clone(existing), true));
        }
    }
    let running = registry.values().filter(|s| s.is_running()).count();
    let max = max_rpc_sessions();
    if running >= max {
        bail!("已达最大并行 pi 会话上限（{max}）：请先在会话管理中停止空闲会话");
    }
    let sess = spawn_pi_rpc(session_id, cwd).await?;
    registry.insert(session_id.to_string(), Arc::clone(&sess));
    Ok((sess, false))
}

/// spawn `pi --mode rpc --session-id <id>`：session 已存在则恢复历史，不存在则创建。
/// 配置 `PANEL_PI_REMOTE_HOST` 时经 ssh 远程 spawn（cwd 按 `PANEL_PI_REMOTE_CWD_MAP` 映射）。
async fn spawn_pi_rpc(session_id: &str, cwd: &Path) -> Result<Arc<PiRpcSession>> {
    let mut child = match remote_host() {
        Some(host) => {
            let remote_cwd = map_cwd_to_remote(cwd);
            // cd 失败时显式退场（exit 3 + 标记写 stderr），避免静默起死导致 attach 假成功；
            // PANEL_SESSION_ID 注入环境（进程标题被 node 改写后仍可按 environ 关联会话）
            let script = format!(
                "cd {} 2>/dev/null || {{ echo '__PANEL_CWD_FAIL__ {}' >&2; exit 3; }}\nexec env PANEL_SESSION_ID={} pi --mode rpc --session-id {}",
                crate::util::shell_quote(&remote_cwd),
                crate::util::shell_quote(&remote_cwd),
                crate::util::shell_quote(session_id),
                crate::util::shell_quote(session_id),
            );
            let mut c = Command::new("ssh");
            c.args(ssh_common_args())
                .arg(&host)
                .arg(&script)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());
            c.spawn().with_context(|| {
                format!(
                    "远程 spawn pi --mode rpc (session {session_id}, cwd {})",
                    remote_cwd
                )
            })?
        }
        None => {
            let mut c = Command::new("pi");
            c.current_dir(cwd)
                .env("PANEL_SESSION_ID", session_id)
                .arg("--mode")
                .arg("rpc")
                .arg("--session-id")
                .arg(session_id)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());
            c.spawn().with_context(|| {
                format!(
                    "spawn pi --mode rpc (session {session_id}, cwd {})",
                    cwd.display()
                )
            })?
        }
    };

    let stdin = child.stdin.take().context("pi rpc stdin 未接管")?;
    let stdout = child.stdout.take().context("pi rpc stdout 未接管")?;
    let stderr = child.stderr.take().context("pi rpc stderr 未接管")?;

    let (events_tx, _) = broadcast::channel::<String>(EVENT_BUFFER);
    let sess = Arc::new(PiRpcSession {
        session_id: session_id.to_string(),
        stdin: Mutex::new(Some(stdin)),
        events: events_tx,
        stderr_tail: Mutex::new(VecDeque::new()),
        running: AtomicBool::new(true),
    });

    // stdout 读取：逐行广播。stdout 关闭即标记进程不再运行（wait 任务负责最终广播）。
    {
        let sess = Arc::clone(&sess);
        tokio::spawn(async move {
            let mut lines = BufReader::new(stdout).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let _ = sess.events.send(line);
            }
            sess.running.store(false, Ordering::SeqCst);
        });
    }

    // stderr 采集：仅保留尾部，供退出时排障。
    {
        let sess = Arc::clone(&sess);
        tokio::spawn(async move {
            let mut lines = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let mut tail = sess.stderr_tail.lock().await;
                if tail.len() >= STDERR_TAIL_LINES {
                    tail.pop_front();
                }
                tail.push_back(line);
            }
        });
    }

    // 起动宽限期：进程若立即退出（远程 cd 失败 / ssh 认证失败等），同步报错而非假成功。
    tokio::time::sleep(std::time::Duration::from_millis(600)).await;
    if !child_running(&mut child).await {
        let tail = sess
            .stderr_tail
            .lock()
            .await
            .iter()
            .cloned()
            .collect::<Vec<_>>()
            .join("\n");
        bail!(
            "pi rpc 进程起动后立即退出（cwd={}）：{}",
            cwd.display(),
            if tail.is_empty() { "无 stderr" } else { &tail }
        );
    }

    // wait：进程退出后广播 panel_rpc_exit。
    {
        let sess = Arc::clone(&sess);
        tokio::spawn(async move {
            let code = child.wait().await.ok().and_then(|s| s.code());
            sess.running.store(false, Ordering::SeqCst);
            sess.broadcast(&json!({
                "type": "panel_rpc_exit",
                "sessionId": sess.session_id,
                "exitCode": code,
                "stderrTail": sess.stderr_tail.lock().await.iter().cloned().collect::<Vec<_>>(),
            }));
        });
    }

    Ok(sess)
}

/// 进程是否仍在运行（不消费 Child：先 try_wait，结束后再 wait 回收）。
async fn child_running(child: &mut tokio::process::Child) -> bool {
    matches!(child.try_wait(), Ok(None))
}
/// 直接生成新 session 并绑定需求（对齐「复制命令」的强制换新语义）。
#[derive(Debug, Deserialize)]
pub(crate) struct PiChatOpenQuery {
    #[serde(alias = "reqId")]
    req_id: Option<String>,
    #[serde(alias = "sessionId")]
    session_id: Option<String>,
    force: Option<String>,
}

fn query_flag_true(value: &Option<String>) -> bool {
    matches!(value.as_deref(), Some("true") | Some("1"))
}

/// 解析「在 Panel 中打开 pi 会话」的默认 session 与工作目录。
///
/// - 显式 `sessionId` 优先（仅校验 UUID 格式；`force` 对显式 session 无意义，忽略）；
/// - `reqId`：取该需求关联 session 中最近更新且文件仍存在的一个；
///   无绑定、全部丢失或 `force=true` 时生成新 UUID 并绑定到需求（复用需求 ↔ session 关联体系）；
/// - cwd 与经验总结派发一致：需求所在项目根（无需求上下文时用 panel 启动目录）。
pub(crate) async fn api_pi_chat_open(
    State(state): State<AppState>,
    Query(query): Query<PiChatOpenQuery>,
) -> ApiResult<Json<Value>> {
    let req_id = query.req_id.clone().unwrap_or_default();
    let explicit_session = query.session_id.clone().filter(|s| !s.trim().is_empty());
    let force = query_flag_true(&query.force);

    let (session_id, is_new, title, cwd): (String, bool, Option<String>, PathBuf) =
        if let Some(sid) = explicit_session {
            let sid = sid.trim().to_string();
            if Uuid::parse_str(&sid).is_err() {
                return Err(ApiError::bad_request(format!("非法 sessionId：{sid}")));
            }
            (sid, false, None, state.project_root.as_ref().clone())
        } else if !req_id.trim().is_empty() {
            let req = get_requirement(&state, req_id.trim())
                .await?
                .with_context(|| format!("需求不存在：{}", req_id.trim()))?;
            let cwd = requirement_project_root(&req)
                .unwrap_or_else(|| state.project_root.as_ref().clone());

            // force=true：跳过复用逻辑，直接新建并绑定；
            // 否则取关联 session 中最近更新且文件仍存在的一个（仅 pi harness 的需求走本接口）。
            let mut latest: Option<sessions::SessionInfo> = None;
            if !force {
                let associated = req.session_ids.clone();
                if !associated.is_empty() {
                    let all = sessions::scan_pi_sessions(&state, None).await?;
                    latest = all
                        .into_iter()
                        .filter(|s| associated.iter().any(|a| a == &s.id))
                        .max_by_key(|s| s.updated);
                }
            }
            match latest {
                Some(s) => (s.id, false, Some(s.title), cwd),
                None => {
                    // 无绑定、关联 session 已被清理或 force：新建并绑定。
                    let sid = Uuid::new_v4().to_string();
                    crate::associate_session(&state, req.id.trim(), &sid).await?;
                    (sid, true, None, cwd)
                }
            }
        } else {
            return Err(ApiError::bad_request("缺少 reqId 或 sessionId 参数"));
        };

    Ok(Json(json!({
        "sessionId": session_id,
        "isNew": is_new,
        "title": title,
        "cwd": cwd.to_string_lossy(),
    })))
}

/// WS 连接 query 参数：session 必填，cwd 可选（由 /api/pi-chat/open 返回值回传）。
#[derive(Debug, Deserialize)]
pub(crate) struct PiChatWsQuery {
    #[serde(alias = "sessionId")]
    session: Option<String>,
    cwd: Option<String>,
}

/// GET /ws/pi-chat?session=<uuid>&cwd=<path>：attach pi RPC 进程并双向转发。
pub(crate) async fn ws_pi_chat(
    ws: WebSocketUpgrade,
    Query(query): Query<PiChatWsQuery>,
    State(state): State<AppState>,
) -> Response {
    let session_id = query.session.unwrap_or_default();
    let cwd = query.cwd;
    ws.on_upgrade(move |socket| async move {
        if let Err(e) = pi_chat_socket(state, socket, session_id, cwd).await {
            tracing::warn!("pi chat socket closed with error: {e:#}");
        }
    })
}

async fn pi_chat_socket(
    state: AppState,
    socket: axum::extract::ws::WebSocket,
    session_id: String,
    cwd_arg: Option<String>,
) -> Result<()> {
    let session_id = session_id.trim().to_string();
    if Uuid::parse_str(&session_id).is_err() {
        bail!("非法 session id：{session_id}");
    }
    let cwd = cwd_arg
        .map(PathBuf::from)
        // 远程模式下路径在 agent-runner 上校验（按 CWD_MAP 映射），不做本地 is_dir 过滤
        .filter(|p| remote_host().is_some() || p.is_dir())
        .unwrap_or_else(|| state.project_root.as_ref().clone());

    let (sess, resumed) = match attach_pi_rpc(&state, &session_id, &cwd).await {
        Ok(v) => v,
        Err(e) => {
            // 上限/认证等 spawn 失败：把原因透传给前端再关闭，而不是静默断开。
            let mut ws_tx = socket;
            let _ = ws_tx
                .send(axum::extract::ws::Message::Text(
                    json!({"type": "panel_error", "error": format!("{e:#}")}).to_string(),
                ))
                .await;
            let _ = ws_tx.send(axum::extract::ws::Message::Close(None)).await;
            return Ok(());
        }
    };
    let (mut ws_tx, mut ws_rx) = socket.split();
    let mut events = sess.events.subscribe();

    let _ = ws_tx
        .send(axum::extract::ws::Message::Text(
            json!({"type": "panel_attached", "sessionId": session_id, "resumed": resumed, "cwd": cwd.to_string_lossy()})
                .to_string(),
        ))
        .await;

    // 下行：pi stdout JSON 行 → 浏览器。
    let mut downstream = tokio::spawn(async move {
        loop {
            match events.recv().await {
                Ok(line) => {
                    if ws_tx
                        .send(axum::extract::ws::Message::Text(line))
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    let note = json!({"type": "panel_lagged", "dropped": n}).to_string();
                    if ws_tx
                        .send(axum::extract::ws::Message::Text(note))
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    });

    // 上行：浏览器 Text 消息 → pi stdin（协议内容不解释）。
    let mut upstream = {
        let sess = Arc::clone(&sess);
        let upstream = tokio::spawn(async move {
            while let Some(Ok(msg)) = ws_rx.next().await {
                match msg {
                    axum::extract::ws::Message::Text(text) => {
                        if sess.send_line(&text).await.is_err() {
                            break;
                        }
                    }
                    axum::extract::ws::Message::Close(_) => break,
                    _ => {}
                }
            }
        });
        upstream
    };

    // 任一方向结束即断开本次 WS；不杀 pi 进程——会话留给下次 attach（SSH/页面关闭无感）。
    tokio::select! {
        _ = &mut downstream => upstream.abort(),
        _ = &mut upstream => downstream.abort(),
    }
    Ok(())
}
