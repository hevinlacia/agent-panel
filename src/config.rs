use std::{collections::HashSet, path::PathBuf};

use anyhow::Result;
use axum::{extract::State, Json};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::fs;

use crate::*;

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AppConfig {
    #[serde(default)]
    pub(crate) harness: String,
    #[serde(default)]
    pub(crate) auto_extract: bool,
    #[serde(default)]
    pub(crate) auto_extract_schedule: bool,
    #[serde(default)]
    pub(crate) extract_model: String,
    #[serde(default)]
    pub(crate) min_change_messages: i64,
    #[serde(default)]
    pub(crate) auto_valuation: bool,
    #[serde(default)]
    pub(crate) valuation_threshold: i64,
    #[serde(default = "default_requirement_scan_roots")]
    pub(crate) requirement_scan_roots: Vec<String>,
    #[serde(default)]
    pub(crate) full_sync_schedule: bool,
    #[serde(default)]
    pub(crate) full_sync_times: Vec<String>,
    #[serde(default)]
    pub(crate) full_sync_github_repos: Vec<String>,
    #[serde(default)]
    pub(crate) code_review_pi_model: String,
    #[serde(default)]
    pub(crate) branch_scope_pi_model: String,
    #[serde(default)]
    pub(crate) effort_estimate_pi_model: String,
    #[serde(default = "default_effort_hours")]
    pub(crate) effort_estimate_base_hours: f64,
    #[serde(default)]
    pub(crate) auto_experience_summary: bool,
    #[serde(default)]
    pub(crate) experience_summary_pi_model: String,
    #[serde(default = "default_experience_summary_max_agents")]
    pub(crate) experience_summary_max_agents: usize,
    #[serde(default)]
    pub(crate) env_vars: Vec<Value>,
    #[serde(default)]
    pub(crate) cainiao_mock_enabled: bool,
    #[serde(default = "default_cainiao_mock_port")]
    pub(crate) cainiao_mock_port: u16,
}

pub(crate) fn default_cainiao_mock_port() -> u16 {
    DEFAULT_CAINIAO_MOCK_PORT
}

pub(crate) fn default_requirement_scan_roots() -> Vec<String> {
    Vec::new()
}

pub(crate) fn default_effort_hours() -> f64 {
    4.0
}

pub(crate) fn default_experience_summary_max_agents() -> usize {
    3
}

pub(crate) fn clamp_experience_summary_max_agents(value: usize) -> usize {
    value.clamp(1, 8)
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
            auto_experience_summary: false,
            experience_summary_pi_model: String::new(),
            experience_summary_max_agents: default_experience_summary_max_agents(),
            env_vars: Vec::new(),
            cainiao_mock_enabled: false,
            cainiao_mock_port: DEFAULT_CAINIAO_MOCK_PORT,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ConfigPatch {
    pub(crate) harness: Option<String>,
    pub(crate) auto_extract: Option<bool>,
    pub(crate) auto_extract_schedule: Option<bool>,
    pub(crate) extract_model: Option<String>,
    pub(crate) min_change_messages: Option<i64>,
    pub(crate) auto_valuation: Option<bool>,
    pub(crate) valuation_threshold: Option<i64>,
    pub(crate) requirement_scan_roots: Option<Vec<String>>,
    pub(crate) full_sync_schedule: Option<bool>,
    pub(crate) full_sync_times: Option<Vec<String>>,
    pub(crate) full_sync_github_repos: Option<Vec<String>>,
    pub(crate) code_review_pi_model: Option<String>,
    pub(crate) branch_scope_pi_model: Option<String>,
    pub(crate) effort_estimate_pi_model: Option<String>,
    pub(crate) effort_estimate_base_hours: Option<f64>,
    pub(crate) auto_experience_summary: Option<bool>,
    pub(crate) experience_summary_pi_model: Option<String>,
    pub(crate) experience_summary_max_agents: Option<usize>,
    pub(crate) cainiao_mock_enabled: Option<bool>,
    pub(crate) cainiao_mock_port: Option<u16>,
}

pub(crate) async fn api_config(State(state): State<AppState>) -> ApiResult<Json<AppConfig>> {
    Ok(Json(read_config(&state).await?))
}

pub(crate) async fn api_config_post(
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
    if let Some(v) = patch.auto_experience_summary {
        cfg.auto_experience_summary = v;
    }
    if let Some(v) = patch.experience_summary_pi_model {
        cfg.experience_summary_pi_model = v;
    }
    if let Some(v) = patch.experience_summary_max_agents {
        cfg.experience_summary_max_agents = clamp_experience_summary_max_agents(v);
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

pub(crate) fn config_path(state: &AppState) -> PathBuf {
    state.data_dir.join(CONFIG_FILE)
}

pub(crate) async fn read_config(state: &AppState) -> Result<AppConfig> {
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
    cfg.experience_summary_max_agents =
        clamp_experience_summary_max_agents(cfg.experience_summary_max_agents);
    Ok(cfg)
}

pub(crate) async fn write_config(state: &AppState, cfg: &AppConfig) -> Result<()> {
    atomic_write_json(&config_path(state), cfg).await
}

pub(crate) fn normalize_scan_roots(values: Vec<String>) -> Vec<String> {
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
