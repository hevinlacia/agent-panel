use super::*;

pub(crate) fn phase_status_index(status: &str) -> Option<usize> {
    REQ_STATUSES.iter().position(|s| *s == status)
}

pub(crate) fn skipped_statuses(from: Option<&str>, to: &str) -> Vec<String> {
    let Some(from_idx) = from.and_then(phase_status_index) else {
        return Vec::new();
    };
    let Some(to_idx) = phase_status_index(to) else {
        return Vec::new();
    };
    if to_idx <= from_idx + 1 {
        return Vec::new();
    }
    REQ_STATUSES[from_idx + 1..to_idx]
        .iter()
        .map(|s| s.to_string())
        .collect()
}

pub(crate) async fn build_phase_runtime_context(
    state: &AppState,
    req: &Requirement,
    intent: &str,
    dir: &Path,
) -> Value {
    let fixed_phase_prompt = load_fixed_phase_prompt(state).await;
    let state_phase_prompt = load_phase_prompt(state, &req.status).await;
    let current_phase_prompt = [fixed_phase_prompt.trim(), state_phase_prompt.trim()]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n---\n\n");
    let state_json = read_requirement_state(dir)
        .await
        .ok()
        .flatten()
        .unwrap_or_else(|| json!({ "version": 1, "status": req.status, "history": [] }));
    let history = state_json
        .get("history")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let transition_path: Vec<Value> = if history.is_empty() {
        vec![json!({ "status": req.status, "source": "current" })]
    } else {
        history
            .iter()
            .filter_map(|item| {
                item.get("status").and_then(Value::as_str).map(|status| {
                    json!({
                        "from": item.get("from").cloned().unwrap_or(Value::Null),
                        "status": status,
                        "at": item.get("at").cloned().unwrap_or(Value::Null),
                        "note": item.get("note").cloned().unwrap_or(Value::Null),
                        "skippedStatuses": item.get("skippedStatuses").cloned().unwrap_or_else(|| json!([]))
                    })
                })
            })
            .collect()
    };
    let skipped_transitions: Vec<Value> = history
        .iter()
        .filter_map(|item| {
            let skipped = item.get("skippedStatuses").and_then(Value::as_array)?;
            if skipped.is_empty() {
                return None;
            }
            Some(json!({
                "from": item.get("from").cloned().unwrap_or(Value::Null),
                "status": item.get("status").cloned().unwrap_or(Value::Null),
                "skippedStatuses": skipped,
                "at": item.get("at").cloned().unwrap_or(Value::Null),
                "note": item.get("note").cloned().unwrap_or(Value::Null),
            }))
        })
        .collect();
    let entry_checks = phase_entry_checks(&req.status, dir);
    let entry_checks = if req.source == "开发推动" && req.status == "测试中" {
        let mut checks = entry_checks;
        checks.push(file_check(
            dir,
            "test-scenario.md",
            "开发推动：测试场景文档（需求说明 + 开发评估的测试范围 + 测试覆盖场景）",
            true,
        ));
        checks
    } else {
        entry_checks
    };
    let missing_required: Vec<Value> = entry_checks
        .iter()
        .filter(|item| {
            item.get("required")
                .and_then(Value::as_bool)
                .unwrap_or(false)
                && !item.get("ok").and_then(Value::as_bool).unwrap_or(false)
        })
        .cloned()
        .collect();
    let phase_gaps = json!({
        "skippedTransitions": skipped_transitions,
        "missingRequiredEntryChecks": missing_required,
        "policy": "状态跳转允许继续；缺口会作为风险提示注入当前阶段上下文，除安全门禁外不自动回退状态。"
    });
    json!({
        "currentStatus": req.status,
        "intent": intent,
        "recommendedIntent": default_intent_for_status(&req.status),
        "fixedPhasePromptFile": PHASE_COMMON_PROMPT_FILE,
        "fixedPhasePrompt": fixed_phase_prompt.trim(),
        "statePhasePromptFile": phase_prompt_file(&req.status),
        "statePhasePrompt": state_phase_prompt.trim(),
        "currentPhasePromptFile": phase_prompt_file(&req.status),
        "currentPhasePrompt": current_phase_prompt.trim(),
        "entryChecks": entry_checks,
        "phaseGaps": phase_gaps,
        "transitionMemory": {
            "source": "state.json.history",
            "path": transition_path,
            "lastTransition": state_json.get("lastTransition").cloned().unwrap_or_else(|| history.last().cloned().unwrap_or(Value::Null)),
            "principle": "当前阶段提示词按最新 status 实时生成；历史阶段只作为摘要和风险，不覆盖当前阶段行为。"
        }
    })
}

pub(crate) fn default_intent_for_status(status: &str) -> &'static str {
    match status {
        "需求澄清" => "clarification",
        "开发中" => "overview",
        "自测中" => "self-test",
        "测试中" => "self-test",
        "经验总结" => "experience-summary",
        "排查中" => "progress",
        "已定位" => "overview",
        "已修复" => "experience-summary",
        "已复盘" | "已关闭" => "overview",
        "已完成" => "overview",
        _ => "overview",
    }
}

pub(crate) fn phase_entry_checks(status: &str, dir: &Path) -> Vec<Value> {
    match status {
        "需求澄清" => vec![
            file_check(dir, "background.md", "业务背景、范围和验收口径", true),
            file_check(
                dir,
                "technical-plan.md",
                "技术方案可供人工先判断实现方向",
                true,
            ),
            file_check(dir, "notes.md", "关键沟通和待确认项可追溯", true),
        ],
        "开发中" => vec![
            file_check(
                dir,
                "background.md",
                "已明确做什么、不做什么和验收标准",
                true,
            ),
            file_check(
                dir,
                "technical-plan.md",
                "实现方案、影响范围、风险和验证计划持续维护",
                true,
            ),
            file_check(dir, BRANCH_SCOPE_FILE, "repo/branch 机器可读映射", false),
            file_check(dir, "release-manifest.md", "有上线资产时按需维护", false),
            file_check(dir, "test.md", "进入自测前按需创建验证场景", false),
        ],
        "自测中" => vec![
            file_check(dir, BRANCH_SCOPE_FILE, "可计算 diff / 部署影响", false),
            file_check(dir, "test.md", "自测场景、tid、DB/副作用和反向证据", true),
            file_check(
                dir,
                "technical-plan.md",
                "实际实现与方案一致或已同步修正",
                true,
            ),
            file_check(dir, "release-manifest.md", "有上线资产时完成自检", false),
            any_file_check(
                dir,
                &["review.md", "code-review-ai.md", CODE_REVIEW_FILE],
                "代码审查门禁结论",
                false,
            ),
        ],
        "测试中" => vec![
            file_check(dir, "test.md", "测试反馈、复现证据和回归结果", true),
            any_file_check(
                dir,
                &["review.md", "code-review-ai.md", CODE_REVIEW_FILE],
                "代码审查门禁已通过或豁免",
                true,
            ),
            file_check(
                dir,
                "technical-plan.md",
                "测试修复后的实现方案仍可审查",
                true,
            ),
            file_check(
                dir,
                "release-manifest.md",
                "有上线资产时待测版本清单完整",
                false,
            ),
            file_check(dir, BRANCH_SCOPE_FILE, "test/UAT 合并目标可计算", false),
        ],
        "经验总结" => vec![
            file_check(
                dir,
                "experience-summary.md",
                "业务知识、经验、skill 和流程改进闭环",
                true,
            ),
            file_check(dir, "test.md", "验证结果和证据可复用", true),
            file_check(dir, "technical-plan.md", "最终实现方案可追溯", true),
            file_check(dir, "release-manifest.md", "有上线资产时变更无遗漏", false),
            file_check(dir, "notes.md", "关键决策和坑点可追溯", false),
        ],
        "排查中" => vec![
            file_check(dir, "notes.md", "线上问题排查过程", true),
            file_check(dir, "incident.md", "问题现象、影响范围和触发条件", true),
            file_check(dir, "root-cause.md", "排查假设与证据链草稿", false),
        ],
        "已定位" => vec![
            file_check(dir, "notes.md", "线上问题排查过程和根因结论", true),
            file_check(dir, "incident.md", "问题现象、影响范围和触发条件", true),
            any_file_check(
                dir,
                &["root-cause.md", "technical-plan.md"],
                "根因、可复核证据链和修复路径决策（存量问题可用 technical-plan.md 兼容）",
                true,
            ),
        ],
        "已修复" => vec![
            file_check(dir, "notes.md", "排查与修复过程可追溯", true),
            any_file_check(
                dir,
                &["root-cause.md", "technical-plan.md"],
                "根因与修复决策（存量问题可用 technical-plan.md 兼容）",
                true,
            ),
            file_check(
                dir,
                "troubleshooting.md",
                "排查经验草稿：怎么排查 + 怎么修复",
                false,
            ),
        ],
        "已复盘" => vec![
            file_check(
                dir,
                "troubleshooting.md",
                "排查经验已沉淀：怎么排查 + 怎么修复 + 复用清单",
                true,
            ),
            any_file_check(
                dir,
                &["root-cause.md", "technical-plan.md"],
                "根因与修复决策（存量问题可用 technical-plan.md 兼容）",
                true,
            ),
            file_check(dir, "notes.md", "排查过程与经验库落地记录", true),
        ],
        "已关闭" => vec![
            file_check(
                dir,
                "notes.md",
                "关闭原因可追溯（误报/重复/环境问题等）",
                true,
            ),
            file_check(
                dir,
                "incident.md",
                "问题现象档案（关闭前建议补齐现象记录）",
                false,
            ),
        ],
        "已完成" => vec![
            file_check(
                dir,
                "experience-summary.md",
                "经验总结已完成或明确无需沉淀",
                false,
            ),
            file_check(dir, "release-check.md", "发布/完成前检查记录", false),
            file_check(dir, "test.md", "最终验证证据", true),
            file_check(dir, "technical-plan.md", "最终技术方案", true),
            file_check(
                dir,
                "release-manifest.md",
                "有上线资产时最终上线清单",
                false,
            ),
        ],
        _ => vec![file_check(dir, "technical-plan.md", "需求技术方案", false)],
    }
}
pub(crate) fn file_check(dir: &Path, file: &str, label: &str, required: bool) -> Value {
    let path = dir.join(file);
    let bytes = path.metadata().map(|m| m.len()).unwrap_or(0);
    let ok = bytes > 0;
    json!({
        "id": file.replace(['.', '/'], "-"),
        "label": label,
        "required": required,
        "ok": ok,
        "status": if ok { "ok" } else if required { "missing" } else { "optionalMissing" },
        "source": file,
        "bytes": bytes,
        "path": path.to_string_lossy()
    })
}

pub(crate) fn any_file_check(dir: &Path, files: &[&str], label: &str, required: bool) -> Value {
    let candidates: Vec<Value> = files
        .iter()
        .map(|file| {
            let path = dir.join(file);
            let bytes = path.metadata().map(|m| m.len()).unwrap_or(0);
            json!({ "file": file, "exists": bytes > 0, "bytes": bytes, "path": path.to_string_lossy() })
        })
        .collect();
    let ok = candidates
        .iter()
        .any(|item| item.get("exists").and_then(Value::as_bool).unwrap_or(false));
    json!({
        "id": files.join("-or-").replace(['.', '/'], "-"),
        "label": label,
        "required": required,
        "ok": ok,
        "status": if ok { "ok" } else if required { "missing" } else { "optionalMissing" },
        "source": files.join(" | "),
        "candidates": candidates
    })
}

pub(crate) fn agent_context_tokens(intent: &str, is_online_issue: bool) -> Vec<&'static str> {
    if is_online_issue {
        // 线上问题专用文档集：不注入需求开发文档（technical-plan/config-changes 等）；
        // 复现/验证代码允许登记分支，注入 req.branchScope 供 diff/merge 使用。
        return match intent {
            "progress" | "status" => vec![
                "req.rootCause",
                "req.branchScope",
                "req.memory",
                "req.notes",
            ],
            _ => vec![
                "req.incident",
                "req.rootCause",
                "req.branchScope",
                "req.memory",
                "req.notes",
            ],
        };
    }
    match intent {
        "self-test" => vec![
            "req.technicalPlan",
            "req.test",
            "req.releaseManifest",
            "req.memory",
            "req.notes",
        ],
        "release-check" => vec![
            "req.releaseManifest",
            "req.attachments",
            "req.technicalPlan",
            "req.test",
            "req.review",
            "req.releaseCheck",
            "req.memory",
        ],
        "config" => vec![
            "req.releaseManifest",
            "req.attachments",
            "req.technicalPlan",
            "req.memory",
            "req.notes",
        ],
        "review" => vec![
            "req.technicalPlan",
            "req.review",
            "req.codeReview",
            "req.memory",
        ],
        "progress" | "status" => vec!["req.technicalPlan", "req.memory", "req.notes"],
        "clarification" | "design" => vec![
            "req.background",
            "req.technicalPlan",
            "req.memory",
            "req.notes",
        ],
        _ => vec![
            "req.background",
            "req.technicalPlan",
            "req.memory",
            "req.notes",
        ],
    }
}
pub(crate) fn summarize_requirement_doc(raw: &str, max_chars: usize) -> (Value, bool) {
    if raw.trim().is_empty() {
        return (json!({ "headings": [], "excerpt": "" }), false);
    }
    let headings: Vec<Value> = raw
        .lines()
        .filter_map(parse_markdown_heading)
        .take(12)
        .map(|(level, text)| json!({ "level": level, "text": text }))
        .collect();
    let candidate_lines: Vec<&str> = raw
        .lines()
        .filter(|line| {
            let t = line.trim();
            t.starts_with("- ")
                || t.starts_with("* ")
                || t.starts_with("##")
                || t.starts_with("###")
        })
        .take(80)
        .collect();
    let base = if candidate_lines.is_empty() {
        raw.trim().to_string()
    } else {
        candidate_lines.join("\n")
    };
    let (excerpt, truncated) = truncate_chars(&base, max_chars);
    (
        json!({ "headings": headings, "excerpt": excerpt }),
        truncated,
    )
}

pub(crate) async fn read_recent_requirement_events(path: &Path, limit: usize) -> Vec<Value> {
    let raw = fs::read_to_string(path).await.unwrap_or_default();
    let mut events: Vec<Value> = raw
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .collect();
    if events.len() > limit {
        events = events.split_off(events.len() - limit);
    }
    events
}

pub(crate) fn recommended_requirement_writes(intent: &str) -> Vec<Value> {
    match intent {
        "self-test" => vec![
            json!({"method":"POST","path":"/api/requirement/events","body":{"operation":"implicit","type":"testResult","reqId":"<req-id>","summary":"...","testCases":[{"name":"...","result":"pass|fail","evidence":"..."}]}}),
            json!({"method":"POST","path":"/api/requirement/events","body":{"type":"learningCandidate","reqId":"<req-id>","summary":"可复用验证/排查方法","candidateType":"experience","triggerTerms":["..."],"evidence":["..."],"dedupeKey":"wms.<topic>.<point>","confidence":"confirmed","target":"experiences"}}),
            json!({"method":"POST","path":"/api/requirement/sections/test","body":{"reqId":"<req-id>","heading":"测试场景","content":"..."}}),
        ],
        "clarification" | "design" => vec![
            json!({"method":"POST","path":"/api/requirement/events","body":{"type":"knowledgeReference","reqId":"<req-id>","summary":"已参考相关知识/经验","relatedKnowledgeIds":["..."],"triggerTerms":["..."]}}),
            json!({"method":"POST","path":"/api/requirement/events","body":{"type":"decision","reqId":"<req-id>","summary":"...","decisions":["..."]}}),
            json!({"method":"POST","path":"/api/requirement/sections/background","body":{"reqId":"<req-id>","heading":"范围与验收口径","content":"..."}}),
            json!({"method":"POST","path":"/api/requirement/sections/technical-plan","body":{"reqId":"<req-id>","heading":"总体实现方案","content":"..."}}),
        ],
        "experience-summary" => vec![
            json!({"method":"GET","path":"/api/requirement/experience-summary-context?id=<req-id>&limit=200"}),
            json!({"method":"POST","path":"/api/knowledge","body":{"kind":"experience|businessKnowledge","title":"...","summary":"...","details":"..."}}),
            json!({"method":"POST","path":"/api/requirement/doc","body":{"reqId":"<req-id>","docType":"experience-summary","mode":"replace","content":"# ..."}}),
        ],
        _ => vec![
            json!({"method":"POST","path":"/api/requirement/events","body":{"type":"progress","reqId":"<req-id>","summary":"..."}}),
            json!({"method":"POST","path":"/api/requirement/events","body":{"type":"learningCandidate","reqId":"<req-id>","summary":"可复用经验/业务知识候选","candidateType":"business-knowledge|experience","triggerTerms":["..."],"evidence":["..."],"dedupeKey":"wms.<topic>.<point>","confidence":"confirmed|inferred|needs-confirmation","target":"business-knowledge|experiences"}}),
            json!({"method":"POST","path":"/api/requirement/events","body":{"type":"skillImprovementCandidate","reqId":"<req-id>","summary":"skill 改进候选","candidateType":"skill-improvement","triggerTerms":["..."],"evidence":["..."],"target":"skill"}}),
            json!({"method":"POST","path":"/api/requirement/sections/technical-plan","body":{"reqId":"<req-id>","heading":"方案摘要","content":"..."}}),
            json!({"method":"POST","path":"/api/requirement/edit","body":{"operation":"appendNote","reqId":"<req-id>","title":"进展","text":"..."}}),
        ],
    }
}

pub(crate) fn truncate_chars(raw: &str, max_chars: usize) -> (String, bool) {
    let count = raw.chars().count();
    if count <= max_chars {
        return (raw.to_string(), false);
    }
    let excerpt = raw.chars().take(max_chars).collect::<String>();
    (
        format!(
            "{}\n…[truncated; {} chars total]",
            excerpt.trim_end(),
            count
        ),
        true,
    )
}
