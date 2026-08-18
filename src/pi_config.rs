use std::path::PathBuf;

use anyhow::{anyhow, Result};
use axum::{extract::Query, Json};
use serde_json::{json, Value};
use tokio::fs;

use crate::{
    atomic_write_json, atomic_write_text, home_dir, now_ms, read_json_if_exists, ApiError,
    ApiResult, IdQuery,
};

pub(crate) async fn api_pi_config() -> Json<Value> {
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

pub(crate) async fn api_pi_config_file(Query(query): Query<IdQuery>) -> Json<Value> {
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

pub(crate) async fn api_pi_config_file_post(Json(payload): Json<Value>) -> ApiResult<Json<Value>> {
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

pub(crate) async fn api_pi_config_settings(Json(payload): Json<Value>) -> ApiResult<Json<Value>> {
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
