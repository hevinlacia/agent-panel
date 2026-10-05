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

use crate::{get_requirement, requirement_project_root, sessions, ApiError, ApiResult, AppState, IdQuery};
use axum::Json;
use serde_json::{json, Value};

/// stderr 尾部保留行数（进程退出时随 panel_rpc_exit 回放，便于排障）。
const STDERR_TAIL_LINES: usize = 40;
/// 事件广播缓冲：pi 流式输出高频时避免订阅慢的客户端拖垮 stdout 读取任务。
const EVENT_BUFFER: usize = 2048;

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
                stdin.write_all(line.trim_end_matches(['\r', '\n']).as_bytes()).await?;
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
    let sess = spawn_pi_rpc(session_id, cwd).await?;
    registry.insert(session_id.to_string(), Arc::clone(&sess));
    Ok((sess, false))
}

/// spawn `pi --mode rpc --session-id <id>`：session 已存在则恢复历史，不存在则创建。
async fn spawn_pi_rpc(session_id: &str, cwd: &Path) -> Result<Arc<PiRpcSession>> {
    let mut cmd = Command::new("pi");
    cmd.current_dir(cwd)
        .arg("--mode")
        .arg("rpc")
        .arg("--session-id")
        .arg(session_id)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd
        .spawn()
        .with_context(|| format!("spawn pi --mode rpc (session {session_id}, cwd {})", cwd.display()))?;

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

/// 解析「在 Panel 中打开 pi 会话」的默认 session 与工作目录。
///
/// - 显式 `sessionId` 优先（仅校验 UUID 格式）；
/// - `reqId`：取该需求关联 session 中最近更新且文件仍存在的一个；
///   无绑定或全部丢失时生成新 UUID 并绑定到需求（复用需求 ↔ session 关联体系）；
/// - cwd 与经验总结派发一致：需求所在项目根（无需求上下文时用 panel 启动目录）。
pub(crate) async fn api_pi_chat_open(
    State(state): State<AppState>,
    Query(query): Query<IdQuery>,
) -> ApiResult<Json<Value>> {
    let req_id = query.req_id.clone().unwrap_or_default();
    let explicit_session = query.session_id.clone().filter(|s| !s.trim().is_empty());

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
            let cwd = requirement_project_root(&req).unwrap_or_else(|| state.project_root.as_ref().clone());

            // 取关联 session 中最近更新且文件仍存在的一个（仅 pi harness 的需求走本接口）。
            let associated = req.session_ids.clone();
            let mut latest: Option<sessions::SessionInfo> = None;
            if !associated.is_empty() {
                let all = sessions::scan_pi_sessions(&state, None).await?;
                latest = all
                    .into_iter()
                    .filter(|s| associated.iter().any(|a| a == &s.id))
                    .max_by_key(|s| s.updated);
            }
            match latest {
                Some(s) => (s.id, false, Some(s.title), cwd),
                None => {
                    // 无绑定或关联 session 已被清理：新建并绑定。
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
        .filter(|p| p.is_dir())
        .unwrap_or_else(|| state.project_root.as_ref().clone());

    let (sess, resumed) = attach_pi_rpc(&state, &session_id, &cwd).await?;
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
                    if ws_tx.send(axum::extract::ws::Message::Text(line)).await.is_err() {
                        break;
                    }
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    let note = json!({"type": "panel_lagged", "dropped": n}).to_string();
                    if ws_tx.send(axum::extract::ws::Message::Text(note)).await.is_err() {
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
