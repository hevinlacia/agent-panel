use std::path::PathBuf;

use axum::{extract::State, Json};
use serde_json::{json, Value};

use crate::{
    atomic_write_json, atomic_write_text, home_dir, read_json_if_exists, ApiError, ApiResult,
    AppState,
};

/// Harness identity — pi keeps running as default, dsh is additive.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum HarnessKind {
    Pi,
    Dsh,
}

impl HarnessKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Pi => "pi",
            Self::Dsh => "dsh",
        }
    }
    fn label(self) -> &'static str {
        match self {
            Self::Pi => "Pi",
            Self::Dsh => "DSH",
        }
    }
    fn from_str(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "pi" => Some(Self::Pi),
            "dsh" | "deepseek-harness" | "deepseek_harness" => Some(Self::Dsh),
            _ => None,
        }
    }
}

fn pi_settings_path() -> Result<PathBuf, ApiError> {
    Ok(home_dir()
        .map_err(|e| ApiError::from(anyhow::anyhow!(e.to_string())))?
        .join(".pi/agent/settings.json"))
}
fn pi_models_path() -> Result<PathBuf, ApiError> {
    Ok(home_dir()
        .map_err(|e| ApiError::from(anyhow::anyhow!(e.to_string())))?
        .join(".pi/agent/models.json"))
}
fn dsh_settings_path() -> Result<PathBuf, ApiError> {
    Ok(home_dir()
        .map_err(|e| ApiError::from(anyhow::anyhow!(e.to_string())))?
        .join(".dsh/settings.yaml"))
}
fn dsh_home() -> Result<PathBuf, ApiError> {
    Ok(home_dir()
        .map_err(|e| ApiError::from(anyhow::anyhow!(e.to_string())))?
        .join(".dsh"))
}

async fn pi_harness_payload() -> Value {
    let settings = read_json_if_exists(&pi_settings_path().unwrap_or_default())
        .await
        .unwrap_or_else(|| json!({}));
    let models = read_json_if_exists(&pi_models_path().unwrap_or_default())
        .await
        .unwrap_or_else(|| json!({}));
    let providers = models
        .get("providers")
        .and_then(Value::as_object)
        .map(|m| {
            m.iter()
                .map(|(id, v)| {
                    let count = v
                        .get("models")
                        .and_then(Value::as_array)
                        .map(|a| a.len())
                        .unwrap_or(0);
                    json!({
                        "id": id,
                        "label": id,
                        "modelCount": count,
                        "hasApiKey": v.get("apiKey").and_then(Value::as_str).map(|s| !s.is_empty()).unwrap_or(false),
                        "models": v.get("models").and_then(Value::as_array).cloned().unwrap_or_default().iter().filter_map(|m| {
                            let mid = m.get("id").or_else(|| m.get("modelId")).and_then(Value::as_str)?;
                            if mid.is_empty() { return None; }
                            Some(json!({
                                "providerId": id,
                                "modelId": mid,
                                "label": m.get("name").and_then(Value::as_str).unwrap_or(mid),
                                "name": m.get("name").and_then(Value::as_str).unwrap_or(mid),
                                "contextWindow": m.get("contextWindow").and_then(Value::as_i64),
                                "reasoning": m.get("reasoning").and_then(Value::as_bool).unwrap_or(false),
                            }))
                        }).collect::<Vec<_>>()
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let flat_models: Vec<Value> = providers
        .iter()
        .flat_map(|p| {
            let pid = p.get("id").and_then(Value::as_str).unwrap_or("");
            p.get("models")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .map(|m| {
                    let mid = m.get("modelId").and_then(Value::as_str).unwrap_or("");
                    json!({
                        "id": format!("{pid}/{mid}"),
                        "providerId": pid,
                        "modelId": mid,
                        "label": m.get("label").and_then(Value::as_str).unwrap_or(mid),
                        "contextWindow": m.get("contextWindow"),
                        "reasoning": m.get("reasoning"),
                    })
                })
                .collect::<Vec<_>>()
        })
        .collect();
    // Also include enabledModels cross-product for quick lookup
    let enabled = settings
        .get("enabledModels")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).map(|s| s.to_string()).collect::<Vec<_>>())
        .unwrap_or_default();
    json!({
        "harness": "pi",
        "label": "Pi",
        "kind": "pi",
        "settingsPath": pi_settings_path().ok().map(|p| p.to_string_lossy().to_string()).unwrap_or_default(),
        "exists": pi_settings_path().ok().map(|p| p.exists()).unwrap_or(false),
        "defaultProvider": settings.get("defaultProvider").and_then(Value::as_str).unwrap_or(""),
        "defaultModel": settings.get("defaultModel").and_then(Value::as_str).unwrap_or(""),
        "defaultThinkingLevel": settings.get("defaultThinkingLevel").and_then(Value::as_str).unwrap_or("off"),
        "enabledModels": enabled,
        "providers": providers,
        "models": flat_models,
        "modelCount": flat_models.len(),
    })
}

async fn dsh_harness_payload() -> Value {
    // dsh settings.yaml is YAML, not JSON — read via read_json_if_exists fallback to manual YAML
    let path = dsh_settings_path().unwrap_or_default();
    let raw = tokio::fs::read_to_string(&path).await.unwrap_or_default();
    let yaml_val: serde_json::Value = if raw.trim().is_empty() {
        json!({})
    } else {
        serde_yaml::from_str::<serde_yaml::Value>(&raw)
            .ok()
            .and_then(|v| serde_json::to_value(v).ok())
            .unwrap_or_else(|| json!({}))
    };
    // llm-pi-ai.providers.*.models[]
    // llm-deepseek is a single-provider adapter (deepseek-official)
    let mut providers: Vec<Value> = Vec::new();
    let mut flat: Vec<Value> = Vec::new();
    if let Some(pi_ai) = yaml_val.get("llm-pi-ai").and_then(|v| v.get("providers")).and_then(Value::as_object) {
        for (pid, pv) in pi_ai {
            let models = pv.get("models").and_then(Value::as_array).cloned().unwrap_or_default();
            let rows: Vec<Value> = models
                .iter()
                .filter_map(|m| {
                    let mid = m.get("id").and_then(Value::as_str)?;
                    if mid.is_empty() { return None; }
                    Some(json!({
                        "providerId": pid,
                        "modelId": mid,
                        "label": m.get("name").and_then(Value::as_str).unwrap_or(mid),
                        "name": m.get("name").and_then(Value::as_str).unwrap_or(mid),
                        "contextWindow": m.get("contextWindow").and_then(Value::as_i64),
                        "reasoning": m.get("reasoning").and_then(Value::as_bool).unwrap_or(false),
                    }))
                })
                .collect();
            for r in &rows {
                let mid = r.get("modelId").and_then(Value::as_str).unwrap_or("");
                flat.push(json!({
                    "id": format!("{pid}/{mid}"),
                    "providerId": pid,
                    "modelId": mid,
                    "label": r.get("label").and_then(Value::as_str).unwrap_or(mid),
                    "contextWindow": r.get("contextWindow"),
                    "reasoning": r.get("reasoning"),
                }));
            }
            providers.push(json!({
                "id": pid,
                "label": pid,
                "modelCount": rows.len(),
                "hasApiKey": pv.get("apiKeyEnv").and_then(Value::as_str).map(|s| !s.is_empty()).unwrap_or(false),
                "models": rows,
            }));
        }
    }
    // deepseek-official singletons if present in settings
    if yaml_val.get("llm-deepseek").is_some() {
        // adapter is mounted via bundle, model catalog is in adapter defaults when not overridden
        // Expose as one provider entry so UI can select deepseek-official/* as in pi
        let already_has_ds = providers.iter().any(|p| p.get("id").and_then(Value::as_str) == Some("deepseek-official"));
        if !already_has_ds {
            // Add stub entry — models come from default catalog when settings empty
            providers.push(json!({
                "id": "deepseek-official",
                "label": "deepseek-official",
                "modelCount": 0,
                "hasApiKey": false,
                "models": [],
            }));
        }
    }
    let agent_default = yaml_val.get("agent-default-model");
    let dp = agent_default.and_then(|v| v.get("provider")).and_then(Value::as_str).unwrap_or("");
    let dm = agent_default.and_then(|v| v.get("model")).and_then(Value::as_str).unwrap_or("");
    json!({
        "harness": "dsh",
        "label": "DSH",
        "kind": "dsh",
        "settingsPath": path.to_string_lossy().to_string(),
        "exists": path.exists(),
        "homePath": dsh_home().ok().map(|p| p.to_string_lossy().to_string()).unwrap_or_default(),
        "defaultProvider": dp,
        "defaultModel": dm,
        "defaultThinkingLevel": "off",
        "enabledModels": [],
        "providers": providers,
        "models": flat,
        "modelCount": flat.len(),
        "hasSettings": !raw.trim().is_empty(),
    })
}

pub(crate) async fn api_harness_list(State(_state): State<AppState>) -> Json<Value> {
    let pi = pi_harness_payload().await;
    let dsh = dsh_harness_payload().await;
    // Current harness: the one whose settings most recently selected the default model?
    // For now: prefer pi when both exist, surface both; frontend tracks selected harness separately.
    let harnesses = vec![pi, dsh];
    Json(json!({
        "harnesses": harnesses,
        "defaultHarness": "pi",
    }))
}

pub(crate) async fn api_harness_current(State(state): State<AppState>) -> Json<Value> {
    // Source of truth for "which harness is currently wired for one-click dispatches":
    // agent-panel config harness field if present, else pi.
    let cfg_raw = crate::config::read_config(&state).await.ok();
    let harness_str = cfg_raw
        .as_ref()
        .map(|c| c.harness.as_str())
        .unwrap_or("pi");
    let kind = HarnessKind::from_str(harness_str).unwrap_or(HarnessKind::Pi);
    let payload = if kind == HarnessKind::Dsh {
        dsh_harness_payload().await
    } else {
        pi_harness_payload().await
    };
    Json(json!({
        "harness": kind.as_str(),
        "label": kind.label(),
        "payload": payload,
    }))
}

pub(crate) async fn api_harness_switch(
    State(state): State<AppState>,
    Json(body): Json<Value>,
) -> ApiResult<Json<Value>> {
    let harness_raw = body.get("harness").and_then(Value::as_str).unwrap_or("pi");
    let kind = HarnessKind::from_str(harness_raw)
        .ok_or_else(|| ApiError::bad_request(format!("unknown harness: {harness_raw} (use pi or dsh)")))?;
    // Optional: also switch default model in same call
    let provider = body.get("provider").and_then(Value::as_str);
    let model = body.get("model").and_then(Value::as_str);
    let thinking = body.get("thinkingLevel").and_then(Value::as_str);

    // Persist harness selection in agent-panel config
    {
        let mut cfg = crate::config::read_config(&state).await.unwrap_or_default();
        cfg.harness = kind.as_str().to_string();
        crate::config::write_config(&state, &cfg)
            .await
            .map_err(|e| ApiError::from(anyhow::anyhow!(e.to_string())))?;
    }

    // If provider/model supplied, persist there too (single call UX from header dropdown)
    if let (Some(p), Some(m)) = (provider, model) {
        if !p.is_empty() && !m.is_empty() {
            if kind == HarnessKind::Pi {
                let path = pi_settings_path()?;
                let mut settings = read_json_if_exists(&path).await.unwrap_or_else(|| json!({}));
                if let Some(obj) = settings.as_object_mut() {
                    obj.insert("defaultProvider".into(), json!(p));
                    obj.insert("defaultModel".into(), json!(m));
                    if let Some(t) = thinking {
                        obj.insert("defaultThinkingLevel".into(), json!(t));
                    }
                }
                atomic_write_json(&path, &settings)
                    .await
                    .map_err(|e| ApiError::from(anyhow::anyhow!(e.to_string())))?;
            } else {
                let path = dsh_settings_path()?;
                let raw = tokio::fs::read_to_string(&path).await.unwrap_or_default();
                let mut doc: serde_yaml::Value = if raw.trim().is_empty() {
                    serde_yaml::Value::Mapping(serde_yaml::Mapping::new())
                } else {
                    serde_yaml::from_str(&raw).unwrap_or_else(|_| serde_yaml::Value::Mapping(serde_yaml::Mapping::new()))
                };
                let map = doc.as_mapping_mut().ok_or_else(|| ApiError::from(anyhow::anyhow!("dsh settings is not a mapping")))?;
                let mut adm = serde_yaml::Mapping::new();
                adm.insert(
                    serde_yaml::Value::String("provider".into()),
                    serde_yaml::Value::String(p.to_string()),
                );
                adm.insert(
                    serde_yaml::Value::String("model".into()),
                    serde_yaml::Value::String(m.to_string()),
                );
                map.insert(
                    serde_yaml::Value::String("agent-default-model".into()),
                    serde_yaml::Value::Mapping(adm),
                );
                let out = serde_yaml::to_string(&doc).map_err(|e| ApiError::from(anyhow::anyhow!(e.to_string())))?;
                atomic_write_text(&path, &out)
                    .await
                    .map_err(|e| ApiError::from(anyhow::anyhow!(e.to_string())))?;
            }
        }
    }

    let payload = if kind == HarnessKind::Dsh {
        dsh_harness_payload().await
    } else {
        pi_harness_payload().await
    };
    Ok(Json(json!({
        "ok": true,
        "harness": kind.as_str(),
        "label": kind.label(),
        "payload": payload,
    })))
}

pub(crate) async fn api_harness_models(State(_state): State<AppState>) -> Json<Value> {
    let pi = pi_harness_payload().await;
    let dsh = dsh_harness_payload().await;
    Json(json!({
        "pi": pi,
        "dsh": dsh,
    }))
}
