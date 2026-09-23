use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result};
use axum::{extract::State, Json};
use futures_util::{SinkExt, StreamExt};
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::ServerConfig;
use serde_json::{json, Value};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpListener;
use tokio_rustls::TlsAcceptor;
use tokio_tungstenite::accept_async;

use crate::{read_config, ApiResult, AppState, CAINIAO_MOCK_PRINTERS};

/// 菜鸟前端在 https 页面固定连接 wss://localhost:13529，与 http 页面的 ws://localhost:13528 成对。
pub(crate) const CAINIAO_MOCK_TLS_PORT: u16 = 13529;

/// Start or stop the cainiao print mock server to match the persisted config.
/// Safe to call on boot and after every config change.
pub(crate) async fn sync_cainiao_mock(state: &AppState) {
    let cfg = read_config(state).await.unwrap_or_default();
    let mut guard = state.cainiao_mock.lock().await;
    let running = guard.as_ref().map(|h| !h.is_finished()).unwrap_or(false);
    if cfg.cainiao_mock_enabled && !running {
        let port = cfg.cainiao_mock_port;
        let cert_dir = state.data_dir.join("certs");
        let handle = tokio::spawn(async move {
            if let Err(e) = run_cainiao_mock(port, &cert_dir).await {
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
    let tls_ready = tls_cert_paths(&state.data_dir.join("certs")).is_some();
    Ok(Json(json!({
        "enabled": cfg.cainiao_mock_enabled,
        "running": running,
        "port": cfg.cainiao_mock_port,
        "tlsPort": CAINIAO_MOCK_TLS_PORT,
        "tlsCertReady": tls_ready,
    })))
}

/// TLS 证书约定路径：{data_dir}/certs/{localhost.crt, localhost.key}（ca.crt 供脚本安装信任库）。
/// 证书 + 私钥同时存在才启用 wss 监听。
pub(crate) fn tls_cert_paths(cert_dir: &Path) -> Option<(PathBuf, PathBuf, PathBuf)> {
    let ca = cert_dir.join("ca.crt");
    let cert = cert_dir.join("localhost.crt");
    let key = cert_dir.join("localhost.key");
    if cert.is_file() && key.is_file() {
        Some((ca, cert, key))
    } else {
        None
    }
}

/// Mock 菜鸟云打印客户端 WebSocket 服务，让前端以为打印成功。
/// 明文 ws://127.0.0.1:{port}（http 页面）+ 可选 wss://localhost:13529（https 页面）。
async fn run_cainiao_mock(port: u16, cert_dir: &Path) -> Result<()> {
    let listener = TcpListener::bind(("127.0.0.1", port))
        .await
        .with_context(|| format!("cainiao mock: bind 127.0.0.1:{port}"))?;
    tracing::info!("Mock 菜鸟打印客户端 listening on ws://127.0.0.1:{port}");

    let tls_listener = match tls_cert_paths(cert_dir) {
        Some((_ca, cert, key)) => match bind_tls(CAINIAO_MOCK_TLS_PORT, &cert, &key).await {
            Ok(v) => {
                tracing::info!(
                    "Mock 菜鸟打印客户端 TLS listening on wss://localhost:{}（https 页面可用）",
                    CAINIAO_MOCK_TLS_PORT
                );
                Some(v)
            }
            Err(e) => {
                tracing::warn!("cainiao mock: wss 启动失败（明文 ws 不受影响）: {e:#}");
                None
            }
        },
        None => {
            tracing::warn!(
                "cainiao mock: 未找到 TLS 证书（{}），wss:{CAINIAO_MOCK_TLS_PORT} 未启动，https 页面无法使用 mock",
                cert_dir.display()
            );
            None
        }
    };

    match tls_listener {
        Some((acceptor, tls)) => {
            tokio::select! {
                r = plain_accept_loop(listener) => r,
                r = tls_accept_loop(acceptor, tls) => r,
            }
        }
        None => plain_accept_loop(listener).await,
    }
}

async fn plain_accept_loop(listener: TcpListener) -> Result<()> {
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

async fn tls_accept_loop(acceptor: TlsAcceptor, listener: TcpListener) -> Result<()> {
    loop {
        let (stream, peer) = listener.accept().await?;
        let peer = peer.to_string();
        let acceptor = acceptor.clone();
        tokio::spawn(async move {
            match acceptor.accept(stream).await {
                Ok(tls) => {
                    if let Err(e) = cainiao_mock_conn(tls, &peer).await {
                        tracing::debug!("cainiao mock tls conn {peer} ended: {e:#}");
                    }
                }
                Err(e) => tracing::debug!("cainiao mock tls handshake {peer} failed: {e}"),
            }
        });
    }
}

async fn bind_tls(port: u16, cert: &Path, key: &Path) -> Result<(TlsAcceptor, TcpListener)> {
    let certs = load_certs(cert)?;
    let key_der = load_key(key)?;
    let config = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key_der)
        .context("cainiao mock tls: invalid cert/key")?;
    let acceptor = TlsAcceptor::from(Arc::new(config));
    let listener = TcpListener::bind(("127.0.0.1", port))
        .await
        .with_context(|| format!("cainiao mock tls: bind 127.0.0.1:{port}"))?;
    Ok((acceptor, listener))
}

fn load_certs(path: &Path) -> Result<Vec<CertificateDer<'static>>> {
    let file = File::open(path).with_context(|| format!("open cert {}", path.display()))?;
    rustls_pemfile::certs(&mut BufReader::new(file))
        .collect::<std::result::Result<Vec<_>, _>>()
        .context("parse tls certs")
}

fn load_key(path: &Path) -> Result<PrivateKeyDer<'static>> {
    let file = File::open(path).with_context(|| format!("open key {}", path.display()))?;
    rustls_pemfile::private_key(&mut BufReader::new(file))?
        .context("tls key file contains no private key")
}

async fn cainiao_mock_conn<S>(stream: S, peer: &str) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
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
