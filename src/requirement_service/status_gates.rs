use super::*;

/// 状态流转门禁统一入口：按配置找到 from → to 命中的规则并依次校验。
///
/// - 只在真实流转（from != to）时校验；未配置的流转不校验。
/// - 配置文件未写 statusGates 时使用内置默认规则（与历史硬编码门禁行为一致 + 自测清单门禁）。
/// - 调用方决定是否跳过：agent 走 API 推进必须校验；人在 Panel UI 上改状态直接跳过。
pub(crate) async fn ensure_status_transition_gates(
    state: &AppState,
    req: &Requirement,
    target_status: &str,
) -> ApiResult<()> {
    let from = req.status.as_str();
    if from == target_status {
        return Ok(());
    }
    let cfg = read_config(state).await.unwrap_or_default();
    let rules = effective_status_gates(&cfg);
    let Some(rule) = rules.iter().find(|r| r.from == from && r.to == target_status) else {
        return Ok(());
    };
    for gate in &rule.gates {
        dispatch_status_gate(gate, req).await?;
    }
    Ok(())
}

async fn dispatch_status_gate(gate: &str, req: &Requirement) -> ApiResult<()> {
    match gate {
        "review" => ensure_review_gate_allows_testing(req).await,
        "selftest-checklist" => ensure_selftest_checklist_allows_testing(req).await,
        "test-scenario" => ensure_test_scenario_allows_testing(req).await,
        "issue-root-cause" => ensure_issue_root_cause_allows_transition(req).await,
        "issue-troubleshooting" => ensure_issue_troubleshooting_allows_transition(req).await,
        other => {
            // 配置归一化已拦未知 id；这里兜底放行并记日志，避免脏配置卡死状态流转。
            tracing::warn!(gate = %other, "unknown status gate id, skipping");
            Ok(())
        }
    }
}

/// 单个门禁当前通过情况的只读评估结果。
pub(crate) struct StatusGateEval {
    pub(crate) passed: bool,
    pub(crate) reason: String,
    /// 放行但需用户注意的警示（如 P1 严重问题、无法测试项），前端渲染 ⚠。
    pub(crate) warnings: Vec<String>,
}

/// 只读评估单个门禁的当前通过情况（不抛错），供状态流转卡片展示。
pub(crate) async fn evaluate_status_gate(gate: &str, req: &Requirement) -> StatusGateEval {
    match gate {
        "review" => {
            let decision = review_gate_decision(req).await;
            match decision {
                Ok(d) if d.allows_testing => StatusGateEval {
                    passed: true,
                    reason: "当前满足通过条件".into(),
                    warnings: d.warnings,
                },
                Ok(d) => StatusGateEval {
                    passed: false,
                    reason: format!("{}：{}", d.label, d.reason),
                    warnings: Vec::new(),
                },
                Err(e) => StatusGateEval {
                    passed: false,
                    reason: e.message,
                    warnings: Vec::new(),
                },
            }
        }
        "selftest-checklist" => {
            let eval = selftest_checklist_eval(req).await;
            if eval.problems.is_empty() {
                StatusGateEval {
                    passed: true,
                    reason: "当前满足通过条件".into(),
                    warnings: eval.warnings,
                }
            } else {
                StatusGateEval {
                    passed: false,
                    reason: eval.problems.join("；"),
                    warnings: Vec::new(),
                }
            }
        }
        "test-scenario" => {
            let outcome = ensure_test_scenario_allows_testing(req).await;
            gate_eval_from_outcome(outcome)
        }
        "issue-root-cause" => {
            let outcome = ensure_issue_root_cause_allows_transition(req).await;
            gate_eval_from_outcome(outcome)
        }
        "issue-troubleshooting" => {
            let outcome = ensure_issue_troubleshooting_allows_transition(req).await;
            gate_eval_from_outcome(outcome)
        }
        _ => StatusGateEval {
            passed: true,
            reason: "当前满足通过条件".into(),
            warnings: Vec::new(),
        },
    }
}

fn gate_eval_from_outcome(outcome: ApiResult<()>) -> StatusGateEval {
    match outcome {
        Ok(()) => StatusGateEval {
            passed: true,
            reason: "当前满足通过条件".into(),
            warnings: Vec::new(),
        },
        Err(e) => StatusGateEval {
            passed: false,
            reason: e.message,
            warnings: Vec::new(),
        },
    }
}

/// 门禁展示名；未知 id 原样返回。
pub(crate) fn status_gate_label(gate_id: &str) -> String {
    STATUS_GATE_DEFS
        .iter()
        .find(|d| d.id == gate_id)
        .map(|d| d.label.to_string())
        .unwrap_or_else(|| gate_id.to_string())
}

/// 测试场景文档门禁：开发推动的需求流转前必须先完成测试场景文档
/// test-scenario.md（需求说明 + 开发评估的测试范围 + 测试覆盖场景）。
pub(crate) async fn ensure_test_scenario_allows_testing(req: &Requirement) -> ApiResult<()> {
    if req.source != "开发推动" {
        return Ok(());
    }
    let Some(dir) = req.req_dir.as_deref() else {
        return Ok(());
    };
    let body = tokio::fs::read_to_string(PathBuf::from(dir).join("test-scenario.md"))
        .await
        .unwrap_or_default();
    if doc_has_filled_items(&body) {
        return Ok(());
    }
    Err(ApiError::bad_request(
        "开发推动的需求流转前必须先完成测试场景文档 test-scenario.md（需求说明 + 开发评估的测试范围 + 测试覆盖场景）；请在需求详情页「测试场景」面板生成并填写，或让 agent 通过 doc API 更新",
    ))
}

/// 线上问题定位门禁：流转到「已定位」前必须填完根因与修复决策
/// （root-cause.md：根因 + 可复核证据链 + 修复路径决策），
/// 存量兼容：历史问题已写 technical-plan.md 且内容成型时同样放行。仅对 category=线上问题 生效。
pub(crate) async fn ensure_issue_root_cause_allows_transition(req: &Requirement) -> ApiResult<()> {
    if req.category.as_deref() != Some("线上问题") {
        return Ok(());
    }
    let dir = req_dir_path(req)?;
    let root_cause_filled = doc_has_filled_items(
        &tokio::fs::read_to_string(dir.join("root-cause.md"))
            .await
            .unwrap_or_default(),
    );
    let legacy_plan_filled = doc_has_filled_items(
        &tokio::fs::read_to_string(dir.join("technical-plan.md"))
            .await
            .unwrap_or_default(),
    );
    if root_cause_filled || legacy_plan_filled {
        return Ok(());
    }
    Err(ApiError::bad_request(
        "进入「已定位」前必须先完成 root-cause.md（根因 + 可复核证据链 + 修复路径决策，至少一条非「待补充」记录）；每条证据要附用户可独立复核的线索：日志=时间范围+tid/关键字，DB=验证 SQL，代码=应用+文件+可搜关键字；存量问题已填 technical-plan.md 的可直接推进",
    ))
}

/// 线上问题复盘点禁：流转到「已复盘」前必须已沉淀排查经验
/// （troubleshooting.md 含怎么排查/怎么修复），无沉淀价值的问题应直接「已关闭」而不是复盘。
/// 仅对 category=线上问题 生效。
pub(crate) async fn ensure_issue_troubleshooting_allows_transition(
    req: &Requirement,
) -> ApiResult<()> {
    if req.category.as_deref() != Some("线上问题") {
        return Ok(());
    }
    let dir = req_dir_path(req)?;
    if doc_has_filled_items(
        &tokio::fs::read_to_string(dir.join("troubleshooting.md"))
            .await
            .unwrap_or_default(),
    ) {
        return Ok(());
    }
    Err(ApiError::bad_request(
        "进入「已复盘」前必须先沉淀排查经验：请填写 troubleshooting.md（含 怎么排查 + 怎么修复，至少一条非「待补充」记录）；无沉淀价值请改用「已关闭」",
    ))
}
