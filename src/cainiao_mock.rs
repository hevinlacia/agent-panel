use anyhow::{Context, Result};
use axum::{extract::State, Json};
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::accept_async;

use crate::{read_config, ApiResult, AppState, CAINIAO_MOCK_PRINTERS};

/// Start or stop the cainiao print mock server to match the persisted config.
/// Safe to call on boot and after every config change.
pub(crate) async fn sync_cainiao_mock(state: &AppState) {
    let cfg = read_config(state).await.unwrap_or_default();
    let mut guard = state.cainiao_mock.lock().await;
    let running = guard.as_ref().map(|h| !h.is_finished()).unwrap_or(false);
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

pub(crate) async fn api_cainiao_mock_status(
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
