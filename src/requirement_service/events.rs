use super::*;

pub(crate) fn normalize_requirement_event_type(raw: Option<&str>) -> String {
    let s = raw.unwrap_or("note").trim().to_lowercase();
    match s.as_str() {
        "issue" | "issue_found" | "bug" | "problem" | "问题" => "issueFound".to_string(),
        "root_cause" | "rootcause" | "cause" | "根因" => "rootCause".to_string(),
        "workaround" | "mitigation" | "临时方案" | "治标" => "workaround".to_string(),
        "fix" | "solution" | "方案" | "修复" => "solution".to_string(),
        "test" | "test_result" | "验证" | "自测" => "testResult".to_string(),
        "decision" | "决策" => "decision".to_string(),
        "knowledge_reference" | "knowledgereference" | "知识引用" | "已有知识" => {
            "knowledgeReference".to_string()
        }
        "learning_candidate" | "learningcandidate" | "经验候选" | "知识候选" | "沉淀候选" => {
            "learningCandidate".to_string()
        }
        "skill_improvement"
        | "skillimprovement"
        | "skill_improvement_candidate"
        | "skillimprovementcandidate"
        | "skill改进" => "skillImprovementCandidate".to_string(),
        "todo" | "next" | "后续" => "todo".to_string(),
        "risk" | "风险" => "risk".to_string(),
        "status_transition" | "statustransition" | "phase_transition" | "phasetransition"
        | "状态切换" | "阶段切换" => "statusTransition".to_string(),
        "progress" | "进展" => "progress".to_string(),
        _ => s.replace(['-', '_'], ""),
    }
}

pub(crate) fn requirement_event_label(event_type: &str) -> &str {
    match event_type {
        "issueFound" => "发现问题",
        "rootCause" => "根因确认",
        "workaround" => "治标方案",
        "solution" => "方案落地",
        "testResult" => "测试验证",
        "decision" => "关键决策",
        "knowledgeReference" => "已参考知识/经验",
        "learningCandidate" => "知识/经验沉淀候选",
        "skillImprovementCandidate" => "Skill 改进候选",
        "todo" => "后续事项",
        "risk" => "风险记录",
        "statusTransition" => "状态切换",
        "progress" => "进展记录",
        _ => "需求事件",
    }
}

pub(crate) fn clean_string_vec(values: Vec<String>) -> Vec<String> {
    values
        .into_iter()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .collect()
}

pub(crate) async fn record_status_transition_event(
    state: &AppState,
    req: &Requirement,
    status_state: &Value,
    note: Option<&str>,
) -> ApiResult<()> {
    let to = status_state
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or(req.status.as_str());
    let from = status_state
        .get("previousStatus")
        .and_then(Value::as_str)
        .unwrap_or("未记录");
    let skipped: Vec<String> = status_state
        .get("lastTransition")
        .and_then(|v| v.get("skippedStatuses"))
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    let at = status_state
        .get("lastTransition")
        .and_then(|v| v.get("at"))
        .and_then(Value::as_i64)
        .unwrap_or_else(now_ms);
    let mut details = vec![
        format!("状态从 `{from}` 切换到 `{to}`。"),
        "Agent 后续应刷新 `/api/requirement/context?for=agent`，同时遵循 `phaseRuntime.fixedPhasePrompt` 和 `phaseRuntime.statePhasePrompt`，不要继续沿用 session 创建时的阶段提示词。".to_string(),
    ];
    if !skipped.is_empty() {
        details.push(format!(
            "本次跳过阶段：{}。这些阶段的 entry checks 会作为风险提示进入 phaseRuntime.phaseGaps。",
            skipped.join("、")
        ));
    }
    if let Some(note) = note.map(str::trim).filter(|v| !v.is_empty()) {
        details.push(format!("备注：{note}"));
    }
    record_requirement_event(
        state,
        RequirementEventForm {
            req_id: req.id.clone(),
            event_type: Some("statusTransition".to_string()),
            title: Some(format!("状态切换：{from} → {to}")),
            summary: Some(format!("状态切换：{from} → {to}")),
            details: Some(details.join("\n")),
            evidence: Vec::new(),
            decisions: Vec::new(),
            todos: if skipped.is_empty() {
                Vec::new()
            } else {
                vec!["补查被跳过阶段的 entry checks，缺失项作为风险或待办记录。".to_string()]
            },
            related_files: vec![STATE_FILE.to_string()],
            related_knowledge_ids: Vec::new(),
            trigger_terms: Vec::new(),
            related_repos: Vec::new(),
            related_tables: Vec::new(),
            related_apis: Vec::new(),
            candidate_type: None,
            dedupe_key: None,
            confidence: None,
            target: None,
            test_cases: Vec::new(),
            status: Some(to.to_string()),
            risk_level: if skipped.is_empty() {
                None
            } else {
                Some("medium".to_string())
            },
            tags: vec!["phase".to_string(), "status-transition".to_string()],
            session_id: None,
            idempotency_key: Some(format!("{}-status-{}", req.id, at)),
            append_note: Some(true),
            dry_run: Some(false),
        },
    )
    .await?;
    Ok(())
}

pub(crate) fn requirement_event_exists(raw: &str, id: &str) -> bool {
    if id.is_empty() {
        return false;
    }
    raw.lines().any(|line| {
        serde_json::from_str::<Value>(line)
            .ok()
            .and_then(|v| v.get("id").and_then(Value::as_str).map(str::to_string))
            .map(|existing| existing == id)
            .unwrap_or(false)
    })
}

pub(crate) fn render_requirement_event_note(event: &Value) -> String {
    let mut lines = Vec::new();
    if let Some(summary) = event
        .get("summary")
        .and_then(Value::as_str)
        .filter(|v| !v.trim().is_empty())
    {
        lines.push(format!("- Summary: {}", summary.trim()));
    }
    if let Some(kind) = event.get("type").and_then(Value::as_str) {
        lines.push(format!("- Type: `{}`", kind));
    }
    if let Some(status) = event
        .get("status")
        .and_then(Value::as_str)
        .filter(|v| !v.trim().is_empty())
    {
        lines.push(format!("- Status: {}", status.trim()));
    }
    if let Some(risk) = event
        .get("riskLevel")
        .and_then(Value::as_str)
        .filter(|v| !v.trim().is_empty())
    {
        lines.push(format!("- Risk: {}", risk.trim()));
    }
    push_event_array(&mut lines, event, "evidence", "Evidence");
    push_event_array(&mut lines, event, "decisions", "Decisions");
    push_event_array(&mut lines, event, "todos", "TODO");
    push_event_array(&mut lines, event, "relatedFiles", "Related files");
    push_event_array(
        &mut lines,
        event,
        "relatedKnowledgeIds",
        "Related knowledge/experience IDs",
    );
    push_event_array(&mut lines, event, "triggerTerms", "Trigger terms");
    push_event_array(&mut lines, event, "relatedRepos", "Related repos");
    push_event_array(&mut lines, event, "relatedTables", "Related tables");
    push_event_array(&mut lines, event, "relatedApis", "Related APIs");
    for (key, label) in [
        ("candidateType", "Candidate type"),
        ("dedupeKey", "Dedupe key"),
        ("confidence", "Confidence"),
        ("target", "Target"),
    ] {
        if let Some(value) = event
            .get(key)
            .and_then(Value::as_str)
            .filter(|v| !v.trim().is_empty())
        {
            lines.push(format!("- {}: {}", label, value.trim()));
        }
    }
    if let Some(test_cases) = event
        .get("testCases")
        .and_then(Value::as_array)
        .filter(|v| !v.is_empty())
    {
        lines.push("- Test cases:".to_string());
        for case in test_cases {
            let name = case
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("Unnamed");
            let result = case
                .get("result")
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            let evidence = case.get("evidence").and_then(Value::as_str).unwrap_or("");
            lines.push(if evidence.trim().is_empty() {
                format!("  - {}: {}", name, result)
            } else {
                format!("  - {}: {} ({})", name, result, evidence)
            });
        }
    }
    if let Some(details) = event
        .get("details")
        .and_then(Value::as_str)
        .filter(|v| !v.trim().is_empty())
    {
        lines.push(String::new());
        lines.push(details.trim().to_string());
    }
    lines.join("\n")
}

pub(crate) fn push_event_array(lines: &mut Vec<String>, event: &Value, key: &str, label: &str) {
    if let Some(values) = event
        .get(key)
        .and_then(Value::as_array)
        .filter(|v| !v.is_empty())
    {
        lines.push(format!("- {}:", label));
        for value in values {
            if let Some(text) = value.as_str().map(str::trim).filter(|v| !v.is_empty()) {
                lines.push(format!("  - {}", text));
            }
        }
    }
}
