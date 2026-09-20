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
    /// dsh profile name used when generating "new session" / resume commands (default dsh-tui).
    #[serde(default = "default_dsh_profile")]
    pub(crate) dsh_profile: String,
    /// Base URL of the running dsh `/api` (default web profile daemon).
    #[serde(default = "default_dsh_api_base_url")]
    pub(crate) dsh_api_base_url: String,
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
    /// 需求分支合并（test/UAT）时跳过的仓库清单（按 repoName 精确匹配，设置页维护）。
    /// 适合 jar 组件库等不走环境分支的仓库。
    #[serde(default)]
    pub(crate) merge_excluded_repos: Vec<String>,
    #[serde(default)]
    pub(crate) cainiao_mock_enabled: bool,
    #[serde(default = "default_cainiao_mock_port")]
    pub(crate) cainiao_mock_port: u16,
    #[serde(default)]
    pub(crate) browser_auth: BrowserAuthConfig,
    /// 状态流转门禁规则：from → to 流转上要依次通过的门禁；None（配置文件未写）时使用内置默认规则。
    /// Agent 通过 API 推进状态时强校验，人在 Panel UI 上修改状态直接跳过。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) status_gates: Option<Vec<StatusGateRule>>,
}

/// 单条状态门禁规则：同一 (from, to) 只保留一条，gates 有序，全部通过才允许流转。
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StatusGateRule {
    pub(crate) from: String,
    pub(crate) to: String,
    #[serde(default)]
    pub(crate) gates: Vec<String>,
}

/// 可选门禁定义（id, 展示名, 说明）；前端设置页从 /api/config 读取渲染。
pub(crate) struct StatusGateDef {
    pub(crate) id: &'static str,
    pub(crate) label: &'static str,
    pub(crate) description: &'static str,
}

pub(crate) static STATUS_GATE_DEFS: &[StatusGateDef] = &[
    StatusGateDef {
        id: "review",
        label: "代码审查门禁",
        description: "review.md / code-review-ai.md 必须给出 Review Gate: PASS 或 WAIVED（含豁免原因），否则拦截流转",
    },
    StatusGateDef {
        id: "selftest-checklist",
        label: "自测清单门禁",
        description: "test.md「自测清单」必须列出测试项目且每项有结果（通过/失败/无法测试），失败或无法测试的项必须写明具体原因",
    },
    StatusGateDef {
        id: "test-scenario",
        label: "测试场景文档门禁",
        description: "仅 source=开发推动 生效：流转前必须完成 test-scenario.md（需求说明 + 开发评估测试范围 + 覆盖场景）",
    },
    StatusGateDef {
        id: "issue-root-cause",
        label: "线上问题定位门禁",
        description: "仅 category=线上问题 生效：流转前必须完成 root-cause.md（根因 + 可复核证据链 + 修复决策；存量已填 technical-plan.md 可放行）",
    },
    StatusGateDef {
        id: "issue-troubleshooting",
        label: "线上问题复盘点禁",
        description: "仅 category=线上问题 生效：流转前必须沉淀 troubleshooting.md（怎么排查 + 怎么修复）",
    },
];

pub(crate) fn status_gate_known_ids() -> Vec<&'static str> {
    STATUS_GATE_DEFS.iter().map(|d| d.id).collect()
}

/// 内置默认门禁规则：与历史硬编码门禁行为一致，外加新增的自测清单门禁。
pub(crate) fn default_status_gate_rules() -> Vec<StatusGateRule> {
    vec![
        StatusGateRule {
            from: "开发中".into(),
            to: "测试中".into(),
            gates: vec!["review".into(), "test-scenario".into()],
        },
        StatusGateRule {
            from: "自测中".into(),
            to: "测试中".into(),
            gates: vec!["review".into(), "selftest-checklist".into(), "test-scenario".into()],
        },
        StatusGateRule {
            from: "排查中".into(),
            to: "已定位".into(),
            gates: vec!["issue-root-cause".into()],
        },
        StatusGateRule {
            from: "已修复".into(),
            to: "已复盘".into(),
            gates: vec!["issue-troubleshooting".into()],
        },
    ]
}

/// 配置未写 statusGates 时用默认规则；写了则完全以配置为准（可为空 = 关闭所有门禁）。
pub(crate) fn effective_status_gates(cfg: &AppConfig) -> Vec<StatusGateRule> {
    cfg.status_gates
        .clone()
        .unwrap_or_else(default_status_gate_rules)
}

/// 保存时严格校验：状态名必须是合法状态、gate id 必须已定义、from != to、(from,to) 去重。
pub(crate) fn normalize_status_gate_rules_strict(
    rules: Vec<StatusGateRule>,
) -> ApiResult<Vec<StatusGateRule>> {
    normalize_status_gate_rules(rules, true).map_err(ApiError::bad_request)
}

/// 读取配置时宽容归一化：非法条目静默丢弃，避免脏配置阻塞整个面板。
pub(crate) fn normalize_status_gate_rules_lenient(rules: Vec<StatusGateRule>) -> Vec<StatusGateRule> {
    normalize_status_gate_rules(rules, false).unwrap_or_default()
}

fn normalize_status_gate_rules(
    rules: Vec<StatusGateRule>,
    strict: bool,
) -> std::result::Result<Vec<StatusGateRule>, String> {
    let known_gates = status_gate_known_ids();
    let mut out: Vec<StatusGateRule> = Vec::new();
    for rule in rules {
        let from = rule.from.trim().to_string();
        let to = rule.to.trim().to_string();
        if from.is_empty() && to.is_empty() && rule.gates.is_empty() {
            continue;
        }
        for (label, status) in [("from", &from), ("to", &to)] {
            if canonical_status(status).is_err() {
                let msg = format!("statusGates 规则的 {label} 状态非法：{status}");
                if strict {
                    return Err(msg);
                }
                continue;
            }
        }
        if from == to {
            let msg = format!("statusGates 规则 from/to 不能相同：{from}");
            if strict {
                return Err(msg);
            }
            continue;
        }
        let mut gates = Vec::new();
        for gate in rule.gates {
            let gate = gate.trim().to_string();
            if gate.is_empty() || gates.contains(&gate) {
                continue;
            }
            if !known_gates.contains(&gate.as_str()) {
                let msg = format!("statusGates 规则包含未知门禁 id：{gate}");
                if strict {
                    return Err(msg);
                }
                continue;
            }
            gates.push(gate);
        }
        if let Some(existing) = out.iter_mut().find(|r| r.from == from && r.to == to) {
            existing.gates = gates;
        } else {
            out.push(StatusGateRule { from, to, gates });
        }
    }
    Ok(out)
}

pub(crate) fn default_cainiao_mock_port() -> u16 {
    DEFAULT_CAINIAO_MOCK_PORT
}

pub(crate) fn default_dsh_profile() -> String {
    "dsh-tui".into()
}

pub(crate) fn default_dsh_api_base_url() -> String {
    "http://127.0.0.1:3080".into()
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
            dsh_profile: default_dsh_profile(),
            dsh_api_base_url: default_dsh_api_base_url(),
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
            merge_excluded_repos: Vec::new(),
            cainiao_mock_enabled: false,
            cainiao_mock_port: DEFAULT_CAINIAO_MOCK_PORT,
            browser_auth: BrowserAuthConfig::default(),
            status_gates: None,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ConfigPatch {
    pub(crate) harness: Option<String>,
    pub(crate) dsh_profile: Option<String>,
    pub(crate) dsh_api_base_url: Option<String>,
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
    pub(crate) merge_excluded_repos: Option<Vec<String>>,
    pub(crate) browser_auth: Option<BrowserAuthConfig>,
    pub(crate) status_gates: Option<Vec<StatusGateRule>>,
}

pub(crate) async fn api_config(State(state): State<AppState>) -> ApiResult<Json<Value>> {
    let cfg = read_config(&state).await?;
    let mut value = serde_json::to_value(&cfg)?;
    if let Some(obj) = value.as_object_mut() {
        // 门禁定义与当前生效规则给设置页渲染；不入盘。
        let defs: Vec<Value> = STATUS_GATE_DEFS
            .iter()
            .map(|d| {
                json!({
                    "id": d.id,
                    "label": d.label,
                    "description": d.description,
                })
            })
            .collect();
        obj.insert("availableStatusGates".into(), Value::Array(defs));
        obj.insert(
            "effectiveStatusGates".into(),
            serde_json::to_value(effective_status_gates(&cfg))?,
        );
    }
    Ok(Json(value))
}

pub(crate) async fn api_config_post(
    State(state): State<AppState>,
    Json(patch): Json<ConfigPatch>,
) -> ApiResult<Json<AppConfig>> {
    let mut cfg = read_config(&state).await?;
    if let Some(v) = patch.harness {
        cfg.harness = v;
    }
    if let Some(v) = patch.dsh_profile {
        cfg.dsh_profile = v.trim().to_string();
    }
    if let Some(v) = patch.dsh_api_base_url {
        cfg.dsh_api_base_url = v.trim().trim_end_matches('/').to_string();
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
    if let Some(v) = patch.merge_excluded_repos {
        cfg.merge_excluded_repos = normalize_repo_name_list(v);
    }
    if let Some(v) = patch.browser_auth {
        cfg.browser_auth = normalize_browser_auth_config(v);
    }
    if let Some(v) = patch.status_gates {
        cfg.status_gates = Some(normalize_status_gate_rules_strict(v)?);
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
    cfg.merge_excluded_repos = normalize_repo_name_list(cfg.merge_excluded_repos);
    cfg.experience_summary_max_agents =
        clamp_experience_summary_max_agents(cfg.experience_summary_max_agents);
    cfg.status_gates = cfg
        .status_gates
        .map(normalize_status_gate_rules_lenient);
    Ok(cfg)
}

pub(crate) async fn write_config(state: &AppState, cfg: &AppConfig) -> Result<()> {
    atomic_write_json(&config_path(state), cfg).await
}

/// 归一化仓库名清单：去空白/首尾斜杠、去空项、去重，保序。
pub(crate) fn normalize_repo_name_list(values: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for raw in values {
        let trimmed = raw.trim().trim_matches('/').to_string();
        if trimmed.is_empty() || !seen.insert(trimmed.clone()) {
            continue;
        }
        out.push(trimmed);
    }
    out
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
