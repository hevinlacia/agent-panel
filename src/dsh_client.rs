use anyhow::{anyhow, Context, Result};
use serde_json::{json, Value};
use uuid::Uuid;

/// Minimal dsh `/api` RPC client (the same wire the dsh web browser client uses:
/// HTTP POST unary + `clientRequestSchema` envelope, loopback-trusted).
///
/// Envelope:
/// ```json
/// { "type": "client-request", "rpcId": "<uuid>", "method": "session.create", "payload": {...} }
/// ```
/// Response:
/// ```json
/// { "type": "server-response", "rpcId": "<uuid>", "result": { "ok": true, "value": {...} } }
/// ```
#[derive(Clone)]
pub(crate) struct DshClient {
    base_url: String,
    http: reqwest::Client,
}

impl DshClient {
    pub(crate) fn new(base_url: &str) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            http: reqwest::Client::builder()
                .connect_timeout(std::time::Duration::from_secs(3))
                .timeout(std::time::Duration::from_secs(15))
                .build()
                .unwrap_or_default(),
        }
    }

    /// One unary RPC call. Returns the `result.value` when the server reports
    /// `ok: true`, else an `ApiError`-shaped anyhow error with the RPC message.
    pub(crate) async fn call(&self, method: &str, payload: Value) -> Result<Value> {
        let url = format!("{}/api/{}", self.base_url, method);
        let body = json!({
            "type": "client-request",
            "rpcId": Uuid::new_v4().to_string(),
            "method": method,
            "payload": payload,
        });
        let response = self
            .http
            .post(&url)
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .with_context(|| format!("dsh ({url}) unreachable"))?;
        let status = response.status();
        let text = response
            .text()
            .await
            .unwrap_or_else(|_| String::new());
        if !status.is_success() {
            return Err(anyhow!("dsh {method} HTTP {status}: {text}"));
        }
        let value: Value = serde_json::from_str(&text)
            .with_context(|| format!("dsh {method}: non-JSON response {text}"))?;
        let result = value
            .get("result")
            .ok_or_else(|| anyhow!("dsh {method}: missing result in {value}"))?;
        if result.get("ok").and_then(Value::as_bool) != Some(true) {
            let message = result
                .get("error")
                .and_then(|e| e.get("message"))
                .and_then(Value::as_str)
                .unwrap_or("unknown error");
            return Err(anyhow!("dsh {method}: {message}"));
        }
        // A successful Remote whose value was `undefined` (e.g. an unregistered
        // command) carries no `value` key; surface that as JSON null.
        Ok(result.get("value").cloned().unwrap_or(Value::Null))
    }

    /// Whether the dsh daemon answers the `/api` host probe.
    pub(crate) async fn healthy(&self) -> bool {
        self.call("host.describe", json!({})).await.is_ok()
    }

    /// Preallocate a session with the given id. `cwd` is optional: an absent or
    /// empty value omits it so the host falls back to its own working directory.
    /// Returns the created session id (echoed by the host).
    pub(crate) async fn create_session(&self, session_id: &str, cwd: Option<&str>) -> Result<String> {
        let mut payload = json!({ "sessionId": session_id });
        if let Some(c) = cwd {
            if !c.is_empty() {
                payload["cwd"] = json!(c);
            }
        }
        let value = self.call("session.create", payload).await?;
        value
            .get("sessionId")
            .and_then(Value::as_str)
            .map(|s| s.to_string())
            .ok_or_else(|| anyhow!("dsh session.create: missing sessionId in {value}"))
    }

    /// Execute a slash command on the session through the `commands/execute`
    /// Remote (the same path the web composer uses; commands dispatch without a
    /// model turn). An unregistered command resolves to an empty ok value —
    /// the caller decides whether that is fatal.
    pub(crate) async fn run_command(&self, session_id: &str, command_line: &str) -> Result<Value> {
        self.call(
            "commands/execute",
            json!({
                "args": { "agentId": session_id, "line": command_line, "images": [] }
            }),
        )
        .await
    }
}
