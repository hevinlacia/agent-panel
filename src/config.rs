use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;

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
    /// 发版冻结开关：ylops_deploy.py 直读 config.json 的 deployFreeze 字段拦截 UAT 构建/部署。
    #[serde(default)]
    pub(crate) deploy_freeze: DeployFreeze,
    /// 自动总结派发时间窗口：仅在窗口内允许自动派发（手动派发不受限）；None = 不限。
    #[serde(default)]
    pub(crate) experience_summary_dispatch_window: Option<DispatchWindow>,
    #[serde(default)]
    pub(crate) browser_auth: BrowserAuthConfig,
    /// 状态流转门禁规则：from → to 流转上要依次通过的门禁；None（配置文件未写）时使用内置默认规则。
    /// Agent 通过 API 推进状态时强校验，人在 Panel UI 上修改状态直接跳过。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) status_gates: Option<Vec<StatusGateRule>>,
    /// skill 位置映射：skill 名 → skill 目录（含 SKILL.md）或 SKILL.md 文件路径（支持 ~/）。
    /// 设置页维护；未配置的 skill 走默认解析（panel cwd/.agents/skills → WMS 工作区 .agents/skills）。
    /// skill 目录迁移后在这里改映射，避免依赖该 skill 的 panel 功能静默失效。
    #[serde(default)]
    pub(crate) skill_path_overrides: BTreeMap<String, String>,
}

/// 自动任务派发时间窗口：仅在窗口内允许自动派发（如 GLM 晚间低价，把自动总结排在夜间）。
/// start > end 表示跨午夜（22:00-08:00）；窗口内已触发的任务执行多久都不受限；
/// 手动派发（显式指定 reqId）不受窗口限制。
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DispatchWindow {
    #[serde(default)]
    pub(crate) enabled: bool,
    /// 窗口起点 HH:MM（本地时间，含）。
    pub(crate) start: String,
    /// 窗口终点 HH:MM（本地时间，排他；早于 start 表示跨午夜）。
    pub(crate) end: String,
}

pub(crate) fn parse_hh_mm(s: &str) -> Result<chrono::NaiveTime, String> {
    chrono::NaiveTime::parse_from_str(s.trim(), "%H:%M").map_err(|_| format!("invalid HH:MM time: {s:?}"))
}

/// 判定 now 是否在派发窗口内（start 含、end 排他；start > end = 跨午夜；start == end = 全天允许）。
/// 窗口时间非法时返回 Err（调用方 fail-open 视为不限，避免配置错误停摆自动总结）。
pub(crate) fn dispatch_window_allows(win: &DispatchWindow, now: chrono::NaiveTime) -> Result<bool, String> {
    let start = parse_hh_mm(&win.start)?;
    let end = parse_hh_mm(&win.end)?;
    if start == end {
        return Ok(true);
    }
    if start < end {
        Ok(now >= start && now < end)
    } else {
        Ok(now >= start || now < end)
    }
}

/// serde 双层 Option 反序列化：区分「字段缺失(None，不动)」与「显式 null(Some(None)，清除)」。
/// 直接用 Option<Option<T>> 时 serde 会把 JSON null 折叠成外层 None，导致无法显式清除。
fn deserialize_some<'de, T, D>(de: D) -> Result<Option<T>, D::Error>
where
    T: serde::Deserialize<'de>,
    D: serde::Deserializer<'de>,
{
    serde::Deserialize::deserialize(de).map(Some)
}

/// 保存路径严格校验：enabled 时 start/end 必须是合法 HH:MM 且不相等（相等语义歧义）。
pub(crate) fn normalize_dispatch_window(win: DispatchWindow) -> ApiResult<DispatchWindow> {
    if !win.enabled {
        return Ok(win);
    }
    let start = parse_hh_mm(&win.start).map_err(ApiError::bad_request)?;
    let end = parse_hh_mm(&win.end).map_err(ApiError::bad_request)?;
    if start == end {
        return Err(ApiError::bad_request(
            "dispatchWindow start == end 语义歧义（全天或禁用），请调整时间",
        ));
    }
    Ok(win)
}

/// 发版冻结状态：开启后部署脚本（ylops_deploy.py）在触发 UAT（uat-sg/uat-cn）构建/部署前
/// 会直接读本字段并拒绝执行；面板 UI 是唯一开关入口。
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DeployFreeze {
    #[serde(default)]
    pub(crate) enabled: bool,
    /// 冻结原因（如需求号 / 发版窗口说明），展示用。
    #[serde(default)]
    pub(crate) reason: String,
    /// 最近一次开启时间（RFC3339）；关闭时保留供展示。
    #[serde(default)]
    pub(crate) since: String,
}

fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

/// 归一化发版冻结：trim 原因；开启且缺时间戳时补当前时间（关闭时保留 reason/since 供展示）。
pub(crate) fn normalize_deploy_freeze(mut freeze: DeployFreeze) -> DeployFreeze {
    freeze.reason = freeze.reason.trim().to_string();
    if freeze.enabled && freeze.since.trim().is_empty() {
        freeze.since = now_rfc3339();
    } else {
        freeze.since = freeze.since.trim().to_string();
    }
    freeze
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
        id: "uat-regression",
        label: "UAT 回归门禁",
        description: "test.md「## UAT 回归」必须逐项给出回归结果与证据（| # | 场景 | 结果 | 证据 |）；证据须是已发生的可核对事实（tid/日志关键字/接口返回/DB 前后值），计划与状态流转动作不算；全通过放行，「无法测试」（有原因）放行但警示，失败/未执行/缺结果/通过但证据不可采不放行；整节不适用写 `不适用：<原因>`",
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
    StatusGateDef {
        id: "effort-estimate",
        label: "工时预估门禁",
        description: "需求澄清/创建 → 开发中：必须先完成工时预估（effort-estimate.json 非 placeholder），供需求排期页消费；绑线上问题的抢修需求自动豁免",
    },
];

pub(crate) fn status_gate_known_ids() -> Vec<&'static str> {
    STATUS_GATE_DEFS.iter().map(|d| d.id).collect()
}

/// 内置默认门禁规则：与历史硬编码门禁行为一致，外加新增的自测清单门禁。
/// 人工核查（2026-08 新增）：测试中 → 人工核查原无硬门禁（agent 测完即推进）；
/// 2026-10 新增 uat-regression 门禁（WMS-136 实测：agent 把「用户推进状态」当 UAT 回归证据，
/// 薄证据一路畅通到发布就绪）：测试中 → 人工核查、及进入发布就绪的两条流转都挂 UAT 回归门禁，
/// agent API 推进时要求 test.md「## UAT 回归」逐项有结果与证据；人工在 UI 推进跳过门禁 = 人工确权
/// （状态流转卡片会实时评估门禁材料并展示三态，人工跳过是知情跳过）。
/// 进入发布就绪的路径（人工核查→发布就绪 / 测试中→发布就绪直达 / 开发中→发布就绪小循环直通）都挂
/// review 门禁，agent API 推进时兜底要求审查通过；人工在 UI 推进跳过门禁 = 人工确权（复测 + 人工审码）。
/// 开发中→发布就绪（2026-09 新增）服务发布就绪小循环：上线前小改动改完必须增量审查刷新快照后才能回发布就绪。
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
            gates: vec![
                "review".into(),
                "selftest-checklist".into(),
                "test-scenario".into(),
            ],
        },
        StatusGateRule {
            from: "测试中".into(),
            to: "人工核查".into(),
            gates: vec!["uat-regression".into()],
        },
        StatusGateRule {
            from: "人工核查".into(),
            to: "发布就绪".into(),
            gates: vec!["review".into(), "uat-regression".into()],
        },
        StatusGateRule {
            from: "测试中".into(),
            to: "发布就绪".into(),
            gates: vec!["review".into(), "uat-regression".into()],
        },
        // 发布就绪小循环：开发中 → 发布就绪 直通（跳过自测中/测试中/人工核查），
        // 挂 review 门禁保证小改动经增量审查刷新快照后才能回发布就绪；不挂 selftest-checklist（快）。
        StatusGateRule {
            from: "开发中".into(),
            to: "发布就绪".into(),
            gates: vec!["review".into()],
        },
        StatusGateRule {
            from: "排查中".into(),
            to: "已定位".into(),
            gates: vec!["issue-root-cause".into()],
        },
        // 工时预估门禁（2026-10 新增）：需求澄清完成进入开发前，agent 必须先按 4h 单位（真实工时口径）
        // 评估工作量并写入 effort-estimate.json，供需求排期页消费；绑线上问题的抢修需求自动豁免。
        StatusGateRule {
            from: "需求澄清".into(),
            to: "开发中".into(),
            gates: vec!["effort-estimate".into()],
        },
        StatusGateRule {
            from: "需求创建".into(),
            to: "开发中".into(),
            gates: vec!["effort-estimate".into()],
        },
        StatusGateRule {
            from: "已修复".into(),
            to: "经验总结".into(),
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
pub(crate) fn normalize_status_gate_rules_lenient(
    rules: Vec<StatusGateRule>,
) -> Vec<StatusGateRule> {
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
            experience_summary_dispatch_window: None,
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
            skill_path_overrides: BTreeMap::new(),
            env_vars: Vec::new(),
            merge_excluded_repos: Vec::new(),
            cainiao_mock_enabled: false,
            cainiao_mock_port: DEFAULT_CAINIAO_MOCK_PORT,
            deploy_freeze: DeployFreeze::default(),
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
    pub(crate) deploy_freeze: Option<DeployFreeze>,
    /// 外层 Some = 本次要改；内层 None = 清除窗口（不限）。enabled=true 时严格校验时间。
    #[serde(default, deserialize_with = "deserialize_some")]
    pub(crate) experience_summary_dispatch_window: Option<Option<DispatchWindow>>,
    pub(crate) merge_excluded_repos: Option<Vec<String>>,
    pub(crate) browser_auth: Option<BrowserAuthConfig>,
    pub(crate) status_gates: Option<Vec<StatusGateRule>>,
    pub(crate) skill_path_overrides: Option<BTreeMap<String, String>>,
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
    if let Some(v) = patch.deploy_freeze {
        cfg.deploy_freeze = normalize_deploy_freeze(v);
    }
    if let Some(v) = patch.experience_summary_dispatch_window {
        cfg.experience_summary_dispatch_window = match v {
            Some(w) => Some(normalize_dispatch_window(w)?),
            None => None,
        };
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
    if let Some(v) = patch.skill_path_overrides {
        cfg.skill_path_overrides = normalize_skill_path_overrides(v);
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
    cfg.deploy_freeze = normalize_deploy_freeze(cfg.deploy_freeze);
    cfg.experience_summary_max_agents =
        clamp_experience_summary_max_agents(cfg.experience_summary_max_agents);
    cfg.status_gates = cfg.status_gates.map(normalize_status_gate_rules_lenient);
    cfg.skill_path_overrides = normalize_skill_path_overrides(cfg.skill_path_overrides);
    crate::set_skill_path_overrides(cfg.skill_path_overrides.clone());
    Ok(cfg)
}

pub(crate) async fn write_config(state: &AppState, cfg: &AppConfig) -> Result<()> {
    atomic_write_json(&config_path(state), cfg).await?;
    crate::set_skill_path_overrides(cfg.skill_path_overrides.clone());
    Ok(())
}

/// 归一化 skill 位置映射：key trim 去首尾斜杠；value 展开 ~/ 前缀、trim、去尾斜杠，空 key/value 剔除。
pub(crate) fn normalize_skill_path_overrides(
    map: BTreeMap<String, String>,
) -> BTreeMap<String, String> {
    map.into_iter()
        .filter_map(|(k, v)| {
            let key = k.trim().trim_matches('/').to_string();
            let trimmed = v.trim();
            let value = if let Some(rest) = trimmed.strip_prefix("~/") {
                match crate::util::home_dir() {
                    Ok(home) => home.join(rest).to_string_lossy().to_string(),
                    Err(_) => trimmed.to_string(),
                }
            } else {
                trimmed.to_string()
            };
            let value = value.trim_end_matches('/').to_string();
            if key.is_empty() || value.is_empty() {
                None
            } else {
                Some((key, value))
            }
        })
        .collect()
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
