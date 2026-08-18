use std::{
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};

use serde_json::{json, Value};
use tokio::fs;

use crate::*;

pub(crate) fn requirement_api_schema() -> Value {
    json!({
        "version": 3,
        "flow": ["需求澄清", "开发中", "自测中", "测试中", "经验总结", "已完成"],
        "statusValues": REQ_STATUSES,
        "statusAliases": REQ_STATUS_ALIASES,
        "categoryValues": REQ_CATEGORIES,
        "tokens": requirement_token_specs_json(),
        "intents": [
            {"intent": "overview", "readTokens": intent_read_tokens("overview"), "writeTokens": intent_write_tokens("overview")},
            {"intent": "clarification", "readTokens": intent_read_tokens("clarification"), "writeTokens": intent_write_tokens("clarification")},
            {"intent": "status", "readTokens": intent_read_tokens("status"), "writeTokens": intent_write_tokens("status")},
            {"intent": "progress", "readTokens": intent_read_tokens("progress"), "writeTokens": intent_write_tokens("progress")},
            {"intent": "branch", "readTokens": intent_read_tokens("branch"), "writeTokens": intent_write_tokens("branch")},
            {"intent": "self-test", "readTokens": intent_read_tokens("self-test"), "writeTokens": intent_write_tokens("self-test")},
            {"intent": "release-check", "readTokens": intent_read_tokens("release-check"), "writeTokens": intent_write_tokens("release-check")},
            {"intent": "design", "readTokens": intent_read_tokens("design"), "writeTokens": intent_write_tokens("design")},
            {"intent": "config", "readTokens": intent_read_tokens("config"), "writeTokens": intent_write_tokens("config")},
            {"intent": "review", "readTokens": intent_read_tokens("review"), "writeTokens": intent_write_tokens("review")},
            {"intent": "experience-summary", "readTokens": intent_read_tokens("experience-summary"), "writeTokens": intent_write_tokens("experience-summary")}
        ],
        "operations": [
            {"operation": "setStatus", "writes": ["req.state"], "required": ["reqId", "status"], "optional": ["note", "dryRun"]},
            {"operation": "setCategory", "writes": ["req.state"], "required": ["reqId", "category"], "optional": ["dryRun"]},
            {"operation": "patchMeta", "writes": ["req.meta"], "required": ["reqId", "fields"], "allowedFields": ["title", "project", "owner", "startDate", "planRelease", "ones"]},
            {"operation": "appendNote", "writes": ["req.notes"], "required": ["reqId", "text"], "optional": ["title", "sessionId", "dryRun"]},
            {"operation": "recordEvent", "endpoint": "POST /api/requirement/events", "writes": ["events.jsonl", "req.notes"], "required": ["reqId", "type", "summary"], "optional": ["details", "evidence", "decisions", "todos", "relatedFiles", "relatedKnowledgeIds", "triggerTerms", "relatedRepos", "relatedTables", "relatedApis", "candidateType", "dedupeKey", "confidence", "target", "testCases", "idempotencyKey", "appendNote", "dryRun"]},
            {"operation": "writeDoc", "writes": ["token/docType"], "required": ["reqId", "token or docType", "content"], "optional": ["mode=replace|append", "dryRun"]},
            {"operation": "upsertSection", "writes": ["token/docType"], "required": ["reqId", "token or docType", "heading", "content"], "optional": ["dryRun"]},
            {"operation": "upsertNamedSection", "endpoint": "POST /api/requirement/sections/{section}", "writes": ["mapped doc section"], "required": ["reqId", "content"], "optional": ["heading", "docType", "token", "dryRun"]}
        ],
        "eventTypes": ["progress", "decision", "knowledgeReference", "learningCandidate", "skillImprovementCandidate", "issueFound", "rootCause", "testResult", "statusTransition"],
        "agentContext": {
            "endpoint": "GET /api/requirement/context?id=<reqId>&for=agent&intent=<intent>&budget=2000",
            "description": "returns compressed summary docs, recent structured events, recommended write APIs, fixedPhasePrompt and statePhasePrompt"
        },
        "rules": [
            "需求澄清阶段合并旧的需求对齐和方案设计：先读业务知识/经验，再初步调查代码，输出 background.md、technical-plan.md、notes.md 的最小闭环；alignment/impact/memory 仅历史兼容。" ,
            "经验总结阶段替代旧待上线状态：识别本次需求暴露的 skill、业务知识、经验和流程改进，并把已落地/待落地区分记录到 experience-summary.md。",
            "Agent should call context with for=agent for most work; use token context only when the compressed summary is insufficient.",
            "Use recordEvent for facts, evidence, test results, decisions and todos; it stores events.jsonl and can append notes.md.",
            "Use sections/{section} or upsertSection for targeted document updates instead of replacing full markdown files.",
            "Maintain req.technicalPlan throughout implementation: update the global approach before/after non-trivial code changes so humans can review direction before reading diffs.",
            "Every phase context includes prompts/phase-common.md as fixedPhasePrompt; state-specific prompts stay in prompts/phase-*.md as statePhasePrompt.",
            "During all phases, record referenced knowledge/experience IDs as knowledgeReference events and reusable findings as learningCandidate or skillImprovementCandidate events when current-session details may help later experience summary.",
            "Online issue requirements use lightweight statuses 排查中/已确认 and do not need the strict normal requirement lifecycle unless converted to category=需求.",
            "Agent should call edit-plan before selecting files for non-trivial requirement edits.",
            "state.json is the source of truth for status/category; do not direct-edit it.",
            "Use appendNote for free-form progress logs; avoid replacing notes.md.",
            "Use branches.json as machine-readable branch scope; branch.md is legacy/human narrative when present."
        ]
    })
}

fn requirement_token_specs_json() -> Vec<Value> {
    vec![
        token_spec(
            "req.meta",
            "meta.md",
            None,
            "stable identity fields and human summary",
            true,
            vec!["patchMeta"],
        ),
        token_spec(
            "req.state",
            STATE_FILE,
            None,
            "status/category and transition history; state source of truth",
            true,
            vec!["setStatus", "setCategory"],
        ),
        token_spec(
            "req.background",
            "background.md",
            Some("background"),
            "developer-facing business background, goal, scope, current behavior, key rules and decisions",
            true,
            vec!["writeDoc", "upsertSection"],
        ),
        token_spec(
            "req.memory",
            "memory.md",
            Some("memory"),
            "short agent-facing lifecycle memory and compressed context",
            true,
            vec!["writeDoc", "upsertSection"],
        ),
        token_spec(
            "req.branch",
            "branch.md",
            Some("branch"),
            "human-readable branch and merge narrative",
            true,
            vec!["writeDoc", "upsertSection"],
        ),
        token_spec(
            "req.branchScope",
            BRANCH_SCOPE_FILE,
            None,
            "machine-readable repo/branch scope for diff/deploy automation",
            true,
            vec![],
        ),
        token_spec(
            "req.configChanges",
            "config-changes.md",
            Some("config-changes"),
            "DB/Apollo/Nacos/RocketMQ/config changes",
            true,
            vec!["writeDoc", "upsertSection"],
        ),
        token_spec(
            "req.releaseManifest",
            "release-manifest.md",
            Some("release-manifest"),
            "always-visible release change manifest: DB tables, configs, topics, groups, jobs, APIs and manual release actions",
            true,
            vec!["writeDoc", "upsertSection"],
        ),
        token_spec(
            "req.attachments",
            "attachments/",
            None,
            "non-code release assets attached to the requirement: SQL files, config exports, runbooks, screenshots and manual operation evidence",
            false,
            vec![],
        ),
        token_spec(
            "req.technicalPlan",
            "technical-plan.md",
            Some("technical-plan"),
            "agent-maintained implementation plan for global design, touched code, risks and validation before human diff review",
            true,
            vec!["writeDoc", "upsertSection"],
        ),
        token_spec(
            "req.impact",
            "impact.md",
            Some("impact"),
            "impact assessment, core-link risk, rollback plan",
            true,
            vec!["writeDoc", "upsertSection"],
        ),
        token_spec(
            "req.test",
            "test.md",
            Some("test"),
            "test scenarios, self-test/UAT evidence and confidence",
            true,
            vec!["writeDoc", "upsertSection"],
        ),
        token_spec(
            "req.notes",
            "notes.md",
            Some("notes"),
            "append-only progress, decisions and pitfalls",
            false,
            vec!["appendNote", "writeDoc", "upsertSection"],
        ),
        token_spec(
            "req.review",
            "review.md",
            Some("review"),
            "code review and pre-release review summary",
            false,
            vec!["writeDoc", "upsertSection"],
        ),
        token_spec(
            "req.releaseCheck",
            "release-check.md",
            Some("release-check"),
            "release readiness checklist and blocking items",
            false,
            vec!["writeDoc", "upsertSection"],
        ),
        token_spec(
            "req.experienceSummary",
            "experience-summary.md",
            Some("experience-summary"),
            "post-requirement capability evolution summary for knowledge, experience and skill improvements",
            false,
            vec!["writeDoc", "upsertSection"],
        ),
        token_spec(
            "req.alignment",
            "alignment.md",
            Some("alignment"),
            "requirement clarification notes, PRD interpretation and open questions",
            true,
            vec!["writeDoc", "upsertSection"],
        ),
        token_spec(
            "req.prd",
            "prd.md",
            Some("prd"),
            "source PRD or product notes",
            false,
            vec!["writeDoc", "upsertSection"],
        ),
        token_spec(
            "req.codeReview",
            CODE_REVIEW_FILE,
            None,
            "large generated code-review artifact; read only by explicit intent",
            false,
            vec![],
        ),
    ]
}

pub(crate) fn token_spec(
    token: &str,
    file: &str,
    doc_type: Option<&str>,
    role: &str,
    standard: bool,
    write_ops: Vec<&str>,
) -> Value {
    json!({
        "token": token,
        "file": file,
        "docType": doc_type,
        "role": role,
        "standard": standard,
        "writeOps": write_ops,
    })
}

pub(crate) fn normalize_requirement_intent(raw: Option<&str>) -> String {
    let s = raw.unwrap_or("overview").trim().to_lowercase();
    if s.is_empty() || s == "default" {
        "overview".into()
    } else if s.contains("状态") || s.contains("status") || s.contains("state") {
        "status".into()
    } else if s.contains("经验")
        || s.contains("总结")
        || s.contains("复盘")
        || s.contains("knowledge")
        || s.contains("skill")
        || s.contains("evolve")
    {
        "experience-summary".into()
    } else if s.contains("澄清")
        || s.contains("对齐")
        || s.contains("背景")
        || s.contains("业务")
        || s.contains("prd")
        || s.contains("alignment")
        || s.contains("clarif")
    {
        "clarification".into()
    } else if s.contains("自测")
        || s.contains("测试")
        || s.contains("test")
        || s.contains("evidence")
    {
        "self-test".into()
    } else if s.contains("上线")
        || s.contains("release")
        || s.contains("发布")
        || s.contains("上线清单")
        || s.contains("manifest")
        || s.contains("发布清单")
    {
        "release-check".into()
    } else if s.contains("分支") || s.contains("branch") || s.contains("diff") {
        "branch".into()
    } else if s.contains("配置")
        || s.contains("config")
        || s.contains("apollo")
        || s.contains("nacos")
        || s.contains("db")
    {
        "config".into()
    } else if s.contains("影响")
        || s.contains("方案")
        || s.contains("design")
        || s.contains("impact")
    {
        "clarification".into()
    } else if s.contains("进展") || s.contains("note") || s.contains("progress") {
        "progress".into()
    } else if s.contains("review") || s.contains("cr") || s.contains("代码审查") {
        "review".into()
    } else {
        s
    }
}

fn is_supported_requirement_intent(intent: &str) -> bool {
    matches!(
        intent,
        "overview"
            | "status"
            | "progress"
            | "clarification"
            | "design"
            | "branch"
            | "self-test"
            | "release-check"
            | "config"
            | "experience-summary"
            | "review"
    )
}

pub(crate) fn ensure_requirement_intent(intent: &str) -> ApiResult<()> {
    if is_supported_requirement_intent(intent) {
        Ok(())
    } else {
        Err(ApiError::bad_request(format!("invalid intent: {intent}")))
    }
}

pub(crate) fn intent_read_tokens(intent: &str) -> Vec<&'static str> {
    match intent {
        "status" => vec!["req.meta", "req.state", "req.notes"],
        "progress" => vec!["req.meta", "req.technicalPlan", "req.notes"],
        "clarification" | "design" => vec![
            "req.meta",
            "req.prd",
            "req.background",
            "req.technicalPlan",
            "req.notes",
        ],
        "branch" => vec!["req.meta", "req.branchScope", "req.branch", "req.notes"],
        "self-test" => vec![
            "req.meta",
            "req.technicalPlan",
            "req.releaseManifest",
            "req.test",
            "req.notes",
        ],
        "release-check" => vec![
            "req.meta",
            "req.state",
            "req.branchScope",
            "req.releaseManifest",
            "req.attachments",
            "req.technicalPlan",
            "req.test",
            "req.review",
            "req.releaseCheck",
            "req.notes",
        ],
        "config" => vec![
            "req.meta",
            "req.releaseManifest",
            "req.attachments",
            "req.technicalPlan",
            "req.configChanges",
            "req.notes",
        ],
        "experience-summary" => vec![
            "req.meta",
            "req.background",
            "req.notes",
            "req.test",
            "req.review",
            "req.releaseManifest",
            "req.technicalPlan",
            "req.experienceSummary",
        ],
        "review" => vec![
            "req.meta",
            "req.branchScope",
            "req.review",
            "req.technicalPlan",
            "req.codeReview",
        ],
        _ => vec![
            "req.meta",
            "req.state",
            "req.background",
            "req.technicalPlan",
            "req.test",
            "req.notes",
        ],
    }
}
pub(crate) fn intent_write_tokens(intent: &str) -> Vec<&'static str> {
    match intent {
        "status" => vec!["req.state", "req.notes"],
        "progress" => vec!["req.technicalPlan", "req.notes"],
        "branch" => vec!["req.branchScope", "req.notes"],
        "self-test" => vec![
            "req.test",
            "req.technicalPlan",
            "req.releaseManifest",
            "req.notes",
        ],
        "release-check" => vec![
            "req.releaseCheck",
            "req.releaseManifest",
            "req.technicalPlan",
            "req.test",
            "req.review",
            "req.notes",
        ],
        "config" => vec!["req.releaseManifest", "req.technicalPlan", "req.notes"],
        "clarification" | "design" => vec!["req.background", "req.technicalPlan", "req.notes"],
        "review" => vec!["req.review", "req.technicalPlan", "req.notes"],
        "experience-summary" => vec![
            "req.experienceSummary",
            "req.releaseManifest",
            "req.technicalPlan",
            "req.notes",
        ],
        _ => vec!["req.technicalPlan", "req.notes"],
    }
}
pub(crate) fn parse_token_list(raw: &str) -> Vec<&'static str> {
    raw.split([',', '，', ' '])
        .filter_map(canonical_requirement_token)
        .collect()
}

pub(crate) fn canonical_requirement_token(raw: &str) -> Option<&'static str> {
    let s = raw
        .trim()
        .trim_start_matches("req.")
        .trim_end_matches(".md")
        .replace(['-', '_'], "")
        .to_lowercase();
    match s.as_str() {
        "meta" => Some("req.meta"),
        "state" | "statejson" => Some("req.state"),
        "background" => Some("req.background"),
        "memory" => Some("req.memory"),
        "branch" => Some("req.branch"),
        "branchscope" | "branches" | "branchesjson" => Some("req.branchScope"),
        "config" | "configchanges" => Some("req.configChanges"),
        "releasemanifest" | "manifest" | "deploymanifest" => Some("req.releaseManifest"),
        "attachments" | "attachment" | "files" | "releaseattachments" | "noncode"
        | "noncodechanges" => Some("req.attachments"),
        "technicalplan" | "techplan" | "implementationplan" | "solution" => {
            Some("req.technicalPlan")
        }
        "impact" => Some("req.impact"),
        "test" => Some("req.test"),
        "notes" | "note" => Some("req.notes"),
        "review" => Some("req.review"),
        "releasecheck" => Some("req.releaseCheck"),
        "experiencesummary" | "summary" | "retrospective" => Some("req.experienceSummary"),
        "alignment" => Some("req.alignment"),
        "prd" => Some("req.prd"),
        "codereview" | "codereviewjson" => Some("req.codeReview"),
        _ => None,
    }
}

pub(crate) fn requirement_token_file(token: &str) -> Option<&'static str> {
    match canonical_requirement_token(token)? {
        "req.meta" => Some("meta.md"),
        "req.state" => Some(STATE_FILE),
        "req.background" => Some("background.md"),
        "req.memory" => Some("memory.md"),
        "req.branch" => Some("branch.md"),
        "req.branchScope" => Some(BRANCH_SCOPE_FILE),
        "req.configChanges" => Some("config-changes.md"),
        "req.releaseManifest" => Some("release-manifest.md"),
        "req.attachments" => Some("attachments"),
        "req.technicalPlan" => Some("technical-plan.md"),
        "req.impact" => Some("impact.md"),
        "req.test" => Some("test.md"),
        "req.notes" => Some("notes.md"),
        "req.review" => Some("review.md"),
        "req.releaseCheck" => Some("release-check.md"),
        "req.experienceSummary" => Some("experience-summary.md"),
        "req.alignment" => Some("alignment.md"),
        "req.prd" => Some("prd.md"),
        "req.codeReview" => Some(CODE_REVIEW_FILE),
        _ => None,
    }
}

pub(crate) fn requirement_doc_type_for_token(token: &str) -> Option<&'static str> {
    match canonical_requirement_token(token)? {
        "req.background" => Some("background"),
        "req.memory" => Some("memory"),
        "req.branch" => Some("branch"),
        "req.configChanges" => Some("config-changes"),
        "req.releaseManifest" => Some("release-manifest"),
        "req.technicalPlan" => Some("technical-plan"),
        "req.impact" => Some("impact"),
        "req.test" => Some("test"),
        "req.notes" => Some("notes"),
        "req.review" => Some("review"),
        "req.releaseCheck" => Some("release-check"),
        "req.experienceSummary" => Some("experience-summary"),
        "req.alignment" => Some("alignment"),
        "req.prd" => Some("prd"),
        _ => None,
    }
}

pub(crate) fn requirement_token_info(req: &Requirement, token: &str) -> ApiResult<Value> {
    let canonical = canonical_requirement_token(token)
        .ok_or_else(|| ApiError::bad_request(format!("unknown requirement token: {token}")))?;
    let file = requirement_token_file(canonical).unwrap_or_default();
    let dir = req_dir_path(req)?;
    let path = dir.join(file);
    let bytes = if canonical == "req.attachments" {
        attachment_total_bytes(&dir)
    } else {
        path.metadata().map(|m| m.len()).unwrap_or(0)
    };
    let exists = if canonical == "req.attachments" {
        path.is_dir()
    } else {
        path.is_file()
    };
    Ok(json!({
        "token": canonical,
        "file": file,
        "docType": requirement_doc_type_for_token(canonical),
        "path": path.to_string_lossy(),
        "exists": exists,
        "bytes": bytes,
    }))
}

pub(crate) fn build_requirement_edit_plan(req: &Requirement, intent: &str) -> Value {
    let read: Vec<Value> = intent_read_tokens(intent)
        .into_iter()
        .filter_map(|t| requirement_token_info(req, t).ok())
        .collect();
    let write: Vec<Value> = intent_write_tokens(intent)
        .into_iter()
        .filter_map(|t| requirement_token_info(req, t).ok())
        .collect();
    json!({
        "ok": true,
        "reqId": req.id,
        "title": req.title,
        "status": req.status,
        "intent": intent,
        "read": read,
        "write": write,
        "preferredFlow": [
            format!("GET /api/requirement/context?id={}&intent={}&budget=2000", req.id, intent),
            "POST /api/requirement/edit",
            "POST /api/requirement/validate"
        ],
        "writeExamples": {
            "appendNote": {"operation": "appendNote", "reqId": req.id, "title": "进展", "text": "..."},
            "upsertTechnicalPlanSection": {"operation": "upsertSection", "reqId": req.id, "token": "req.technicalPlan", "heading": "总体实现方案", "content": "- ..."},
            "upsertTestSection": {"operation": "upsertSection", "reqId": req.id, "token": "req.test", "heading": "自测证据", "content": "- ..."},
            "setStatus": {"operation": "setStatus", "reqId": req.id, "status": "自测中", "note": "..."}
        },
        "rules": [
            "Read only the tokens listed here unless the task explicitly needs more.",
            "Prefer /api/requirement/edit over direct file edits.",
            "Use appendNote for progress; use upsertSection for targeted document updates.",
            "Run validate after writes."
        ]
    })
}

pub(crate) async fn build_requirement_context(
    req: &Requirement,
    intent: &str,
    tokens: Vec<&'static str>,
    budget: usize,
) -> ApiResult<Value> {
    let dir = req_dir_path(req)?;
    let mut rows = Vec::new();
    let mut remaining = budget;
    let total = tokens.len().max(1);
    for (idx, token) in tokens.into_iter().enumerate() {
        let canonical = canonical_requirement_token(token).unwrap_or(token);
        let Some(file) = requirement_token_file(canonical) else {
            continue;
        };
        if canonical == "req.attachments" {
            let slots_left = total.saturating_sub(idx).max(1);
            let per_file_budget = (remaining / slots_left).clamp(300, 3_000);
            let content = render_requirement_attachments_context(&dir, per_file_budget).await;
            let chars = content.chars().count();
            remaining = remaining.saturating_sub(chars);
            let path = dir.join(file);
            rows.push(json!({
                "token": canonical,
                "file": file,
                "docType": requirement_doc_type_for_token(canonical),
                "path": path.to_string_lossy(),
                "exists": path.is_dir(),
                "bytes": attachment_total_bytes(&dir),
                "contentChars": chars,
                "truncated": false,
                "content": content,
            }));
            continue;
        }
        let path = dir.join(file);
        let exists = path.is_file();
        let bytes = path.metadata().map(|m| m.len()).unwrap_or(0);
        let slots_left = total.saturating_sub(idx).max(1);
        let per_file_budget = (remaining / slots_left).clamp(300, 3_000);
        let (content, truncated, chars) = if exists && remaining > 0 {
            let raw = fs::read_to_string(&path).await.unwrap_or_default();
            let (excerpt, truncated) = truncate_chars(&raw, per_file_budget);
            let chars = excerpt.chars().count();
            remaining = remaining.saturating_sub(chars);
            (excerpt, truncated, chars)
        } else {
            (String::new(), false, 0)
        };
        rows.push(json!({
            "token": canonical,
            "file": file,
            "docType": requirement_doc_type_for_token(canonical),
            "path": path.to_string_lossy(),
            "exists": exists,
            "bytes": bytes,
            "contentChars": chars,
            "truncated": truncated,
            "content": content,
        }));
    }
    Ok(json!({
        "ok": true,
        "reqId": req.id,
        "title": req.title,
        "status": req.status,
        "project": req.project,
        "intent": intent,
        "budget": budget,
        "remainingBudget": remaining,
        "tokens": rows,
        "editPlanUrl": format!("/api/requirement/edit-plan?id={}&intent={}", req.id, intent),
    }))
}

/// Minimal inline markdown: escape, then code spans, bold, links.

/// Compact markdown -> HTML for the requirement context viewer.

pub(crate) fn pretty_json(content: &str) -> Option<String> {
    serde_json::from_str::<Value>(content)
        .ok()
        .map(|v| serde_json::to_string_pretty(&v).unwrap_or_else(|_| content.to_string()))
}

/// Friendly table for req.branchScope (repos + branches), falls back to None.
pub(crate) fn render_branch_scope_html(content: &str) -> Option<String> {
    let value: Value = serde_json::from_str(content).ok()?;
    let repos = value.get("repos")?.as_array()?;
    let mut out = String::from(
        "<table><thead><tr><th>仓库</th><th>角色</th><th>分支</th><th>路径</th></tr></thead><tbody>",
    );
    for repo in repos {
        let name = repo.get("repoName").and_then(Value::as_str).unwrap_or("-");
        let role = repo.get("role").and_then(Value::as_str).unwrap_or("-");
        let path = repo.get("path").and_then(Value::as_str).unwrap_or("-");
        let branches: Vec<String> = repo
            .get("branches")
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(Value::as_str)
                    .map(html_escape)
                    .collect()
            })
            .unwrap_or_default();
        out.push_str(&format!(
            "<tr><td><code>{}</code></td><td>{}</td><td>{}</td><td><code>{}</code></td></tr>",
            html_escape(name),
            html_escape(role),
            if branches.is_empty() {
                "—".to_string()
            } else {
                branches.join("<br/>")
            },
            html_escape(path)
        ));
    }
    out.push_str("</tbody></table>");
    if let Some(updated_at) = value.get("updatedAt").and_then(Value::as_u64) {
        let secs = updated_at as i64 / 1000;
        if let Some(dt) = chrono::DateTime::from_timestamp(secs, 0) {
            out.push_str(&format!(
                "<p class=\"meta-note\">更新时间：{}</p>",
                dt.format("%Y-%m-%d %H:%M:%S")
            ));
        }
    }
    Some(out)
}

pub(crate) fn token_display_label(token: &str) -> String {
    match token {
        "req.meta" => "元信息 Meta".to_string(),
        "req.state" => "状态 State".to_string(),
        "req.background" => "业务背景 Background".to_string(),
        "req.memory" => "记忆 Memory".to_string(),
        "req.branch" => "分支 Branch".to_string(),
        "req.branchScope" => "分支范围 Branch Scope".to_string(),
        "req.configChanges" => "配置变更明细 Config Changes".to_string(),
        "req.releaseManifest" => "上线清单 Release Manifest".to_string(),
        "req.attachments" => "非代码附件 Attachments".to_string(),
        "req.technicalPlan" => "技术方案 Technical Plan".to_string(),
        "req.impact" => "影响范围 Impact".to_string(),
        "req.test" => "自测 Test".to_string(),
        "req.notes" => "进展 Notes".to_string(),
        "req.review" => "审查 Review".to_string(),
        "req.releaseCheck" => "上线检查 Release Check".to_string(),
        "req.experienceSummary" => "经验总结 Experience Summary".to_string(),
        "req.alignment" => "对齐 Alignment".to_string(),
        "req.prd" => "PRD".to_string(),
        "req.codeReview" => "代码审查 Code Review".to_string(),
        _ => token.to_string(),
    }
}

pub(crate) fn render_token_content_html(token: &str, content: &str) -> String {
    let trimmed = content.trim();
    if token == "req.branchScope" {
        if let Some(table) = render_branch_scope_html(content) {
            return table;
        }
    }
    if trimmed.starts_with('{') || trimmed.starts_with('[') {
        if let Some(json) = pretty_json(content) {
            return format!("<pre class=\"json\">{}</pre>", html_escape(&json));
        }
    }
    render_markdown_html(content)
}

const CONTEXT_PAGE_CSS: &str = r#"
    :root { color-scheme: light dark; }
    * { box-sizing: border-box; }
    body {
      margin: 0; padding: 0;
      font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", "PingFang SC", "Microsoft YaHei", "Noto Sans CJK SC", sans-serif;
      background: #f5f6f8; color: #1f2328; line-height: 1.65; font-size: 14px;
    }
    .page-header { background: #161b22; color: #e6edf3; padding: 26px 32px; border-bottom: 3px solid #f0a020; }
    .page-header .crumbs { font-size: 12px; color: #8b949e; margin-bottom: 10px; }
    .page-header .crumbs a { color: #8b949e; }
    .page-header h1 { margin: 0 0 10px; font-size: 22px; line-height: 1.3; }
    .page-header .meta { display: flex; flex-wrap: wrap; gap: 12px; align-items: center; font-size: 13px; color: #c9d1d9; }
    .page-header .meta code { background: rgba(255,255,255,.12); border-radius: 4px; padding: 1px 6px; }
    .badge { display: inline-block; padding: 2px 10px; border-radius: 999px; font-size: 12px; font-weight: 600; background: #f0a020; color: #161b22; }
    .badge-missing { background: #cf222e; color: #fff; }
    .badge-truncated { background: #9a6700; color: #fff; }
    .page-main { max-width: 1020px; margin: 24px auto 64px; padding: 0 24px; }
    .intro { background: #fff; border: 1px solid #e3e6ea; border-radius: 10px; padding: 14px 18px; margin-bottom: 20px; color: #57606a; font-size: 13px; }
    .token { background: #fff; border: 1px solid #e3e6ea; border-radius: 10px; margin-bottom: 20px; overflow: hidden; }
    .token-head { display: flex; align-items: center; justify-content: space-between; gap: 12px; padding: 14px 18px; border-bottom: 1px solid #eef0f2; background: #fafbfc; }
    .token-head h2 { margin: 0; font-size: 16px; }
    .token-idx { font-size: 11px; color: #a0a8b0; }
    .token-head-right { display: inline-flex; align-items: center; gap: 8px; flex-wrap: wrap; justify-content: flex-end; }
    .token-action { border: 1px solid #d0d7de; border-radius: 999px; padding: 5px 10px; background: #fff; color: #1f2328; font-size: 12px; font-weight: 600; cursor: pointer; }
    .token-action:hover { border-color: #0969da; color: #0969da; background: #f6f8fa; }
    .token-action.copy-ok { border-color: #1a7f37; color: #1a7f37; }
    .attachment-list { margin-bottom: 10px; }
    .attachment-list h3 { margin: 0 0 8px; font-size: 14px; color: #1f2328; }
    .attachment-files { display: flex; flex-direction: column; gap: 10px; margin-top: 12px; }
    .attachment-file { border: 1px solid #d0d7de; border-radius: 8px; background: #f6f8fa; }
    .attachment-file > summary { display: flex; align-items: center; gap: 10px; flex-wrap: wrap; padding: 10px 14px; cursor: pointer; list-style: none; }
    .attachment-file > summary::-webkit-details-marker { display: none; }
    .attachment-file > summary::before { content: "▸"; color: #57606a; transition: transform 120ms ease; }
    .attachment-file[open] > summary::before { transform: rotate(90deg); }
    .attachment-name { font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace; font-weight: 600; color: #1f2328; }
    .attachment-badge { color: #0969da; font-size: 12px; font-weight: 600; }
    .attachment-size { color: #6e7781; font-size: 12px; }
    .attachment-file-body { padding: 0 14px 14px; }
    .attachment-file-actions { display: flex; align-items: center; gap: 10px; margin: 10px 0 8px; }
    .attachment-path { font-size: 11px; color: #6e7781; word-break: break-all; }
    .attachment-file pre { background: #161b22; color: #e6edf3; border-radius: 8px; padding: 12px 14px; overflow-x: auto; font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace; font-size: 12px; line-height: 1.55; white-space: pre-wrap; word-break: break-word; max-height: 480px; overflow-y: auto; margin: 0; }
    .attachment-file-source { position: absolute; left: -9999px; width: 1px; height: 1px; opacity: 0; }
    .token-meta { padding: 8px 18px; font-size: 12px; color: #6e7781; background: #fdfefe; border-bottom: 1px solid #f0f1f3; }
    .token-meta code { background: #eef0f2; border-radius: 4px; padding: 1px 5px; font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace; font-size: 11px; }
    .token-body { padding: 16px 18px; overflow-wrap: anywhere; }
    .token-body h1, .token-body h2, .token-body h3, .token-body h4 { margin: 20px 0 8px; }
    .token-body h1:first-child, .token-body h2:first-child, .token-body h3:first-child { margin-top: 0; }
    .token-body p { margin: 8px 0; }
    .token-body blockquote { margin: 8px 0; padding: 8px 12px; border-left: 3px solid #d0d7de; background: #f6f8fa; color: #57606a; border-radius: 0 6px 6px 0; }
    .token-body ul, .token-body ol { margin: 8px 0; padding-left: 22px; }
    .token-body li { margin: 4px 0; }
    .token-body li.task-item { list-style: none; margin-left: -22px; display: flex; gap: 8px; align-items: flex-start; }
    .token-body table { border-collapse: collapse; width: 100%; margin: 10px 0; font-size: 13px; }
    .token-body th, .token-body td { border: 1px solid #d8dee4; padding: 6px 10px; text-align: left; vertical-align: top; }
    .token-body th { background: #f6f8fa; font-weight: 600; white-space: nowrap; }
    .token-body code { background: #f0f1f3; border-radius: 4px; padding: 1px 5px; font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace; font-size: 12px; }
    .token-body pre { background: #161b22; color: #e6edf3; border-radius: 8px; padding: 12px 14px; overflow-x: auto; font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace; font-size: 12px; line-height: 1.55; }
    .token-body pre code { background: transparent; padding: 0; color: inherit; }
    .token-body pre.json { white-space: pre-wrap; word-break: break-all; }
    .token-body hr { border: none; border-top: 1px solid #e3e6ea; margin: 16px 0; }
    .token-body .empty { color: #8b949e; }
    .token-body .meta-note { color: #6e7781; font-size: 12px; }
    .page-footer { text-align: center; padding: 8px 0 48px; font-size: 12px; color: #8b949e; }
    .page-footer a { color: #0969da; }
    @media (prefers-color-scheme: dark) {
      body { background: #0d1117; color: #e6edf3; }
      .intro, .token { background: #161b22; border-color: #30363d; }
      .token-head { background: #1c2128; border-color: #30363d; }
      .token-action { background: #21262d; color: #e6edf3; border-color: #30363d; }
      .token-action:hover { color: #58a6ff; border-color: #58a6ff; background: #30363d; }
      .attachment-list h3 { color: #e6edf3; }
      .attachment-file { background: #1c2128; border-color: #30363d; }
      .attachment-file > summary::before { color: #8b949e; }
      .attachment-name { color: #e6edf3; }
      .attachment-badge { color: #58a6ff; }
      .attachment-path { color: #8b949e; }
      .token-meta { background: #12161c; color: #8b949e; border-color: #30363d; }
      .token-body code, .token-meta code { background: #30363d; }
      .token-body blockquote { background: #1c2128; border-color: #30363d; color: #9da7b3; }
      .token-body th { background: #1c2128; }
      .token-body th, .token-body td { border-color: #30363d; }
      .token-body hr { border-color: #30363d; }
      .intro { color: #9da7b3; }
      .page-footer a { color: #58a6ff; }
    }
"#;

const CONTEXT_PAGE_SCRIPT: &str = r#"
<script>
(function () {
  function setCopyLabel(button, text) {
    const original = button.getAttribute('data-label') || button.textContent || '一键复制';
    if (!button.getAttribute('data-label')) button.setAttribute('data-label', original);
    button.textContent = text;
    button.classList.add('copy-ok');
    window.setTimeout(function () {
      button.textContent = original;
      button.classList.remove('copy-ok');
    }, 1500);
  }
  document.querySelectorAll('.attachment-file').forEach(function (details) {
    const copy = details.querySelector('.attachment-file-copy');
    const source = details.querySelector('.attachment-file-source');
    if (!copy || !source) return;
    copy.addEventListener('click', async function () {
      const text = source.value || source.textContent || '';
      try {
        await navigator.clipboard.writeText(text);
      } catch (err) {
        source.style.position = 'fixed';
        source.style.left = '0';
        source.style.top = '0';
        source.style.opacity = '1';
        source.focus();
        source.select();
        document.execCommand('copy');
        source.style.position = 'absolute';
        source.style.left = '-9999px';
        source.style.opacity = '0';
      }
      setCopyLabel(copy, '已复制');
    });
  });
})();
</script>
"#;

pub(crate) fn render_requirement_context_html(
    req: &Requirement,
    intent: &str,
    value: &Value,
) -> String {
    let title = value
        .get("title")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .unwrap_or(&req.id);
    let status = value.get("status").and_then(Value::as_str).unwrap_or("");
    let project = value.get("project").and_then(Value::as_str).unwrap_or("");
    let budget = value.get("budget").and_then(Value::as_u64).unwrap_or(0);
    let remaining = value
        .get("remainingBudget")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let mut sections = String::new();
    let mut raw_tokens: Vec<String> = Vec::new();
    if let Some(tokens) = value.get("tokens").and_then(Value::as_array) {
        for token in tokens.iter() {
            let t = token.get("token").and_then(Value::as_str).unwrap_or("");
            let file = token.get("file").and_then(Value::as_str).unwrap_or("");
            let path = token.get("path").and_then(Value::as_str).unwrap_or("");
            let exists = token
                .get("exists")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let truncated = token
                .get("truncated")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let bytes = token.get("bytes").and_then(Value::as_u64).unwrap_or(0);
            let content = token.get("content").and_then(Value::as_str).unwrap_or("");
            raw_tokens.push(t.to_string());
            let label = token_display_label(t);
            let mut meta: Vec<String> = Vec::new();
            meta.push(format!("<code>{}</code>", html_escape(file)));
            meta.push(format!("{bytes} 字节"));
            if !exists {
                meta.push("<span class=\"badge badge-missing\">文件缺失</span>".to_string());
            } else if truncated {
                meta.push("<span class=\"badge badge-truncated\">预算内已截断</span>".to_string());
            }
            let body = if content.is_empty() {
                "<p class=\"empty\">暂无内容</p>".to_string()
            } else if t == "req.attachments" {
                // 附件：直接在 HTML 层扫描目录，渲染为 总表(默认展开) + 每个文件一个折叠块。
                req_dir_path(req)
                    .ok()
                    .map(|dir| render_requirement_attachments_html(&dir))
                    .unwrap_or_else(|| "<p class=\"empty\">附件目录不可用</p>".to_string())
            } else {
                render_token_content_html(t, content)
            };
            sections.push_str(&format!(
                "<section class=\"token\"><div class=\"token-head\"><h2>{label}</h2><div class=\"token-head-right\"><span class=\"token-idx\">{}</span></div></div><div class=\"token-meta\">{}<br/><code>{}</code></div><div class=\"token-body\">{body}</div></section>",
                html_escape(path),
                meta.join(" · "),
                html_escape(path)
            ));
        }
    }
    let raw_url = format!(
        "/api/requirement/context?id={}&intent={}&tokens={}&budget={}",
        percent_encode(&req.id),
        percent_encode(intent),
        percent_encode(&raw_tokens.join(",")),
        budget
    );
    let mut html = String::new();
    html.push_str("<!DOCTYPE html>\n<html lang=\"zh-CN\">\n<head>\n<meta charset=\"utf-8\"/>\n<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\"/>\n<title>");
    html.push_str(&html_escape(title));
    html.push_str(" · ");
    html.push_str(&html_escape(intent));
    html.push_str(" · Agent Panel</title>\n<style>");
    html.push_str(CONTEXT_PAGE_CSS);
    html.push_str("</style>\n</head>\n<body>\n<header class=\"page-header\"><div class=\"crumbs\">Agent Panel / 需求 / <a href=\"/requirement?id=");
    html.push_str(&percent_encode(&req.id));
    html.push_str("\">");
    html.push_str(&html_escape(&req.id));
    html.push_str("</a></div><h1>");
    html.push_str(&html_escape(title));
    html.push_str("</h1><div class=\"meta\"><span class=\"badge\">");
    html.push_str(&html_escape(status));
    html.push_str("</span><span>项目：");
    html.push_str(&html_escape(project));
    html.push_str("</span><span>意图：<code>");
    html.push_str(&html_escape(intent));
    html.push_str("</code></span><span>预算：");
    html.push_str(&budget.to_string());
    html.push_str(" 字符（剩余 ");
    html.push_str(&remaining.to_string());
    html.push_str(
        "）</span></div></header>\n<main class=\"page-main\"><div class=\"intro\">以下为该需求「",
    );
    html.push_str(&html_escape(intent));
    html.push_str("」的上下文汇总，按文档分节渲染，便于人工阅读。查看原始 JSON：<a href=\"");
    html.push_str(&raw_url);
    html.push_str("\" rel=\"noreferrer\">原始数据</a></div>");
    html.push_str(&sections);
    html.push_str(CONTEXT_PAGE_SCRIPT);
    html.push_str("</main>\n<footer class=\"page-footer\"><a href=\"");
    html.push_str(&raw_url);
    html.push_str(
        "\" rel=\"noreferrer\">查看原始 JSON 数据</a> · Agent Panel</footer>\n</body>\n</html>",
    );
    html
}

pub(crate) async fn build_requirement_agent_context(
    state: &AppState,
    req: &Requirement,
    intent: &str,
    budget: usize,
    event_limit: usize,
) -> ApiResult<Value> {
    let dir = req_dir_path(req)?;
    let phase_runtime = build_phase_runtime_context(state, req, intent, &dir).await;
    let context_tokens = agent_context_tokens(intent);
    let mut docs = Vec::new();
    let per_doc_budget = (budget / context_tokens.len().max(1)).clamp(300, 2_000);
    for token in context_tokens {
        if token == "req.attachments" {
            let path = dir.join("attachments");
            let raw = render_requirement_attachments_context(&dir, per_doc_budget).await;
            let (summary, truncated) = summarize_requirement_doc(&raw, per_doc_budget);
            docs.push(json!({
                "token": token,
                "file": "attachments/",
                "docType": requirement_doc_type_for_token(token),
                "exists": path.is_dir(),
                "bytes": attachment_total_bytes(&dir),
                "truncated": truncated,
                "summary": summary,
            }));
            continue;
        }
        let Some(file) = requirement_token_file(token) else {
            continue;
        };
        let path = dir.join(file);
        let raw = fs::read_to_string(&path).await.unwrap_or_default();
        let (summary, truncated) = summarize_requirement_doc(&raw, per_doc_budget);
        docs.push(json!({
            "token": token,
            "file": file,
            "docType": requirement_doc_type_for_token(token),
            "exists": path.is_file(),
            "bytes": path.metadata().map(|m| m.len()).unwrap_or(0),
            "truncated": truncated,
            "summary": summary,
        }));
    }
    let events_path = dir.join(REQUIREMENT_EVENTS_FILE);
    let events = read_recent_requirement_events(&events_path, event_limit).await;
    Ok(json!({
        "ok": true,
        "format": "agentRequirementContext.v2",
        "reqId": req.id,
        "title": req.title,
        "status": req.status,
        "project": req.project,
        "projects": req.projects,
        "category": req.category,
        "ones": req.ones,
        "intent": intent,
        "budget": budget,
        "phaseRuntime": phase_runtime,
        "summaryDocs": docs,
        "recentEvents": events,
        "recommendedWrites": recommended_requirement_writes(intent),
        "apis": {
            "recordEvent": "/api/requirement/events",
            "upsertSection": "/api/requirement/sections/{section}",
            "edit": "/api/requirement/edit",
            "validate": "/api/requirement/validate",
            "refreshAgentContext": format!("/api/requirement/context?id={}&for=agent&intent={}&budget={}", req.id, intent, budget)
        },
        "rules": [
            "Treat phaseRuntime as current navigation: it is rebuilt from the requirement's latest state on every context call.",
            "If the session started in an earlier phase, do not keep following the startup prompt; refresh context with for=agent and follow phaseRuntime.fixedPhasePrompt + phaseRuntime.statePhasePrompt.",
            "Skipped phase gaps are risk flags, not hard blockers: record them and continue the user's current task unless a safety gate blocks it.",
            "Prefer recordEvent for facts/status/evidence/decisions; it stores events.jsonl and can append notes.md.",
            "Prefer sections/{section} or upsertSection for targeted impact/test/background/technical-plan updates.",
            "Keep technical-plan.md current when implementation direction, affected files, risks or validation strategy changes.",
            "Read full docs only when this compressed context is insufficient."
        ]
    }))
}

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
        "已确认" => "experience-summary",
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
        "已确认" => vec![
            file_check(dir, "notes.md", "线上问题排查过程和确认结论", true),
            file_check(
                dir,
                "technical-plan.md",
                "根因、影响、后续修复建议或转需求判断",
                true,
            ),
        ],
        "排查中" => vec![
            file_check(dir, "notes.md", "线上问题排查过程", true),
            file_check(dir, "background.md", "问题现象、影响范围和触发条件", false),
            file_check(
                dir,
                "technical-plan.md",
                "排查假设、证据链和根因判断",
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

pub(crate) fn agent_context_tokens(intent: &str) -> Vec<&'static str> {
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

#[derive(Debug)]
pub(crate) struct ReviewGateDecision {
    pub(crate) status: String,
    pub(crate) label: String,
    pub(crate) allows_testing: bool,
    pub(crate) reason: String,
    pub(crate) source: Option<String>,
    pub(crate) review_path: PathBuf,
    pub(crate) ai_review_path: PathBuf,
    pub(crate) actions: Vec<String>,
    pub(crate) stale_repos: Vec<Value>,
    pub(crate) incremental_review: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ReviewSnapshotDrift {
    pub(crate) repo_name: String,
    pub(crate) branch: String,
    pub(crate) project_path: Option<PathBuf>,
    pub(crate) reviewed_target_ref: String,
    pub(crate) reviewed_target_commit: String,
    pub(crate) current_target_ref: String,
    pub(crate) current_target_commit: String,
}

pub(crate) async fn review_gate_json(req: &Requirement) -> ApiResult<Value> {
    let gate = review_gate_decision(req).await?;
    let (risk_tags, inventory_risk) = code_review_risk_for(&req_dir_path(req)?).await;
    Ok(json!({
        "ok": true,
        "reqId": req.id,
        "gate": {
            "status": gate.status,
            "label": gate.label,
            "allowsTesting": gate.allows_testing,
            "reason": gate.reason,
            "source": gate.source,
            "reviewPath": gate.review_path.to_string_lossy(),
            "aiReviewPath": gate.ai_review_path.to_string_lossy(),
            "riskTags": risk_tags,
            "inventoryRisk": inventory_risk,
            "actions": gate.actions,
            "staleRepos": gate.stale_repos,
            "incrementalReview": gate.incremental_review,
            "checkedAt": now_ms(),
        }
    }))
}

pub(crate) async fn ensure_review_gate_allows_testing(req: &Requirement) -> ApiResult<()> {
    let gate = review_gate_decision(req).await?;
    if gate.allows_testing {
        return Ok(());
    }
    Err(ApiError::bad_request(format!(
        "Code Review Gate 未通过（{}）：{}。请先补充 review.md / code-review-ai.md 并明确 `Review Gate: PASS`，或在 review.md 记录 `Review Gate: WAIVED` + 豁免原因。",
        gate.label, gate.reason
    )))
}

pub(crate) async fn review_gate_decision(req: &Requirement) -> ApiResult<ReviewGateDecision> {
    let dir = req_dir_path(req)?;
    let review_path = dir.join("review.md");
    let ai_review_path = dir.join("code-review-ai.md");
    let (_risk_tags, inventory_risk) = code_review_risk_for(&dir).await;
    let mut docs = Vec::<(String, PathBuf, String)>::new();
    for (label, path) in [
        ("review.md".to_string(), review_path.clone()),
        ("code-review-ai.md".to_string(), ai_review_path.clone()),
    ] {
        if let Ok(raw) = fs::read_to_string(&path).await {
            if !raw.trim().is_empty() {
                docs.push((label, path, raw));
            }
        }
    }
    if docs.is_empty() {
        return Ok(ReviewGateDecision {
            status: "missing".into(),
            label: "未执行".into(),
            allows_testing: false,
            reason: "未找到 review.md 或 code-review-ai.md 的代码审查结论".into(),
            source: None,
            review_path,
            ai_review_path,
            actions: vec![
                "生成 code-review.json / code-review-ai.md，或手工补充 review.md".into(),
                "在审查结论中写明 `Review Gate: PASS`、`Review Gate: BLOCKED` 或 `Review Gate: WAIVED`".into(),
            ],
            stale_repos: Vec::new(),
            incremental_review: None,
        });
    }
    for (source, path, raw) in &docs {
        if review_gate_waived(raw) {
            let stale = review_snapshot_drifts(&dir).await;
            if !stale.is_empty() {
                return Ok(stale_review_gate_decision(
                    source.clone(),
                    review_path,
                    ai_review_path,
                    &stale,
                ));
            }
            if review_artifact_requires_fresh_review(&dir, path).await {
                return Ok(refresh_stale_review_gate_decision(
                    source.clone(),
                    review_path,
                    ai_review_path,
                ));
            }
            return Ok(ReviewGateDecision {
                status: "waived".into(),
                label: "用户豁免".into(),
                allows_testing: true,
                reason: "review 文档记录了豁免结论".into(),
                source: Some(source.clone()),
                review_path,
                ai_review_path,
                actions: vec!["保留豁免原因，测试阶段重点覆盖高风险改动".into()],
                stale_repos: Vec::new(),
                incremental_review: None,
            });
        }
    }
    for (source, _path, raw) in &docs {
        if review_gate_blocked(raw) {
            return Ok(ReviewGateDecision {
                status: "blocked".into(),
                label: "有阻塞项".into(),
                allows_testing: false,
                reason: "review 文档存在严重问题、阻塞项或明确 BLOCKED 结论".into(),
                source: Some(source.clone()),
                review_path,
                ai_review_path,
                actions: vec![
                    "修复严重问题后重新审查".into(),
                    "若业务确认可带风险提测，在 review.md 明确 `Review Gate: WAIVED` 和豁免原因"
                        .into(),
                ],
                stale_repos: Vec::new(),
                incremental_review: None,
            });
        }
    }
    for (source, path, raw) in &docs {
        if review_gate_passed(raw) {
            // 库存高危风险：即使写了 PASS，若未包含库存账本专项评估，门禁仍不通过
            if inventory_risk && !review_has_inventory_evidence(raw) {
                return Ok(ReviewGateDecision {
                    status: "inventory-pending".into(),
                    label: "库存风险未评估".into(),
                    allows_testing: false,
                    reason: "本次改动命中库存高危风险，但 review 未包含库存账本矩阵（单据活跃/死亡、DB 库存、redis 可用量、重复释放、遗漏占用、幂等、验证证据）。请补充后重新给出 PASS。".into(),
                    source: Some(source.clone()),
                    review_path,
                    ai_review_path,
                    actions: vec![
                        "在 review.md / code-review-ai.md 补充 `## 库存账本评估`：单据最终是活跃单还是死亡单；DB 库存变化(onHandQty/allocatedQty/临时库位/回库单)；redis 可用量变化(建单-、真取消+、恢复-、回退是否保持占用)；是否存在重复释放(cancel+delete/intercept/MQ重试/接口重试)；是否存在遗漏占用(回池后继续分配/拣货但未重新占用)；是否有幂等保护；验证证据(DB前后/redis前后/日志/单测/边界状态)".into(),
                        "明确 `Review Gate: PASS` 后重试推进；若业务确认带风险提测，使用 `Review Gate: WAIVED` + 豁免原因".into(),
                    ],
                    stale_repos: Vec::new(),
                    incremental_review: None,
                });
            }
            let stale = review_snapshot_drifts(&dir).await;
            if !stale.is_empty() {
                return Ok(stale_review_gate_decision(
                    source.clone(),
                    review_path,
                    ai_review_path,
                    &stale,
                ));
            }
            if review_artifact_requires_fresh_review(&dir, path).await {
                return Ok(refresh_stale_review_gate_decision(
                    source.clone(),
                    review_path,
                    ai_review_path,
                ));
            }
            return Ok(ReviewGateDecision {
                status: "passed".into(),
                label: "审查通过".into(),
                allows_testing: true,
                reason: "review 文档记录了通过结论，且审查快照覆盖当前需求分支 HEAD".into(),
                source: Some(source.clone()),
                review_path,
                ai_review_path,
                actions: vec!["可以推进到测试中；测试阶段按 review 的验收要点回归".into()],
                stale_repos: Vec::new(),
                incremental_review: None,
            });
        }
    }
    Ok(ReviewGateDecision {
        status: "pending".into(),
        label: "待确认".into(),
        allows_testing: false,
        reason: "已找到 review 文档，但缺少明确 PASS / BLOCKED / WAIVED 结论".into(),
        source: docs.first().map(|(source, _, _)| source.clone()),
        review_path,
        ai_review_path,
        actions: vec![
            "在 review.md 顶部补充 `Review Gate: PASS` / `BLOCKED` / `WAIVED`".into(),
            "若使用 AI 审查，确认 code-review-ai.md 后同步结论到 review.md".into(),
        ],
        stale_repos: Vec::new(),
        incremental_review: None,
    })
}

pub(crate) fn stale_review_gate_decision(
    source: String,
    review_path: PathBuf,
    ai_review_path: PathBuf,
    stale: &[ReviewSnapshotDrift],
) -> ReviewGateDecision {
    ReviewGateDecision {
        status: "stale".into(),
        label: "需增量审查".into(),
        allows_testing: false,
        reason: format!(
            "审查结论后仍有 {} 个需求分支 HEAD 发生变化；可优先生成 code-review-incremental.json，仅审查已审 commit 到当前 HEAD 的新增 diff。",
            stale.len()
        ),
        source: Some(source),
        review_path,
        ai_review_path,
        actions: vec![
            "优先点击“生成增量审查包”，只审 reviewedTargetCommit → currentTargetCommit 的新增提交和 diff".into(),
            "把增量审查结论追加/更新到 code-review-ai.md 或 review.md，并重新写明 `Review Gate: PASS` / `BLOCKED` / `WAIVED`".into(),
            "若提示非线性历史、rebase 或 force-push，再回退到全量 code-review.json 审查".into(),
        ],
        stale_repos: stale
            .iter()
            .cloned()
            .map(review_snapshot_drift_json)
            .collect(),
        incremental_review: None,
    }
}

pub(crate) fn refresh_stale_review_gate_decision(
    source: String,
    review_path: PathBuf,
    ai_review_path: PathBuf,
) -> ReviewGateDecision {
    ReviewGateDecision {
        status: "stale".into(),
        label: "需确认增量审查".into(),
        allows_testing: false,
        reason: "代码差异快照或增量审查包晚于当前 review 结论；请确认新增 diff 已审查后再放行。".into(),
        source: Some(source),
        review_path,
        ai_review_path,
        actions: vec![
            "若是测试中追加提交，优先审查 code-review-incremental.json，而不是重审完整 code-review.json".into(),
            "更新 code-review-ai.md / review.md 的审查概览，注明增量覆盖范围和结论".into(),
            "确认所有新增风险标签（尤其库存）已覆盖后，重新写明 `Review Gate: PASS` / `BLOCKED` / `WAIVED`".into(),
        ],
        stale_repos: Vec::new(),
        incremental_review: None,
    }
}

pub(crate) async fn review_artifact_requires_fresh_review(
    req_dir: &Path,
    review_doc_path: &Path,
) -> bool {
    let Some(review_doc_updated_at) = file_modified_ms(review_doc_path).await else {
        return false;
    };
    if let Some(updated_at) = file_modified_ms(&req_dir.join(CODE_REVIEW_INCREMENTAL_FILE)).await {
        if updated_at > review_doc_updated_at {
            return true;
        }
    }
    if let Some(review) = read_json_if_exists(&req_dir.join(CODE_REVIEW_FILE)).await {
        if !review
            .get("previousReviewedSnapshot")
            .unwrap_or(&Value::Null)
            .is_null()
        {
            if let Some(updated_at) = file_modified_ms(&req_dir.join(CODE_REVIEW_FILE)).await {
                return updated_at > review_doc_updated_at;
            }
        }
    }
    false
}

pub(crate) async fn file_modified_ms(path: &Path) -> Option<i64> {
    let meta = fs::metadata(path).await.ok()?;
    Some(system_time_to_ms(meta.modified().unwrap_or(UNIX_EPOCH)))
}

pub(crate) fn review_gate_waived(raw: &str) -> bool {
    let lower = raw.to_lowercase();
    lower.contains("review gate: waived")
        || raw.contains("用户豁免")
        || raw.contains("代码审查豁免")
}

pub(crate) fn review_gate_blocked(raw: &str) -> bool {
    let lower = raw.to_lowercase();
    if lower.contains("review gate: blocked")
        || raw.contains("不可提测")
        || raw.contains("审查不通过")
    {
        return true;
    }
    if raw.contains('❌')
        && (raw.contains("阻塞") || raw.contains("必须修复") || raw.contains("严重问题"))
    {
        return true;
    }
    review_gate_section(raw, &["严重问题", "必须修复", "Blocking Items", "阻塞项"])
        .map(|section| !review_section_is_empty(&section))
        .unwrap_or(false)
}

pub(crate) fn review_gate_passed(raw: &str) -> bool {
    let lower = raw.to_lowercase();
    if lower.contains("review gate: pass")
        || lower.contains("result: pass")
        || raw.contains("代码审查通过")
        || raw.contains("审查通过")
        || raw.contains("可提测")
        || raw.contains("无阻塞")
    {
        return true;
    }
    review_gate_section(raw, &["严重问题", "必须修复"])
        .map(|section| review_section_is_empty(&section))
        .unwrap_or(false)
}

/// 读取 code-review.json 里聚合出的风险标签与库存风险标记。
pub(crate) async fn review_snapshot_drifts(req_dir: &Path) -> Vec<ReviewSnapshotDrift> {
    let Some(review) = read_json_if_exists(&req_dir.join(CODE_REVIEW_FILE)).await else {
        return Vec::new();
    };
    let drifts = review_snapshot_drifts_from_value(&review).await;
    if !drifts.is_empty() {
        if reviewed_incremental_covers_drifts(req_dir, &drifts).await {
            return Vec::new();
        }
        if review_artifact_newer_than_review_docs(req_dir, CODE_REVIEW_INCREMENTAL_FILE).await {
            let incremental_drifts = incremental_review_drifts(req_dir).await;
            if !incremental_drifts.is_empty() {
                return incremental_drifts;
            }
        }
        return drifts;
    }
    if review_artifact_newer_than_review_docs(req_dir, CODE_REVIEW_INCREMENTAL_FILE).await {
        return incremental_review_drifts(req_dir).await;
    }
    Vec::new()
}

pub(crate) async fn review_snapshot_drifts_for_incremental(
    req_dir: &Path,
) -> Vec<ReviewSnapshotDrift> {
    let Some(review) = read_json_if_exists(&req_dir.join(CODE_REVIEW_FILE)).await else {
        return Vec::new();
    };
    let drifts = review_snapshot_drifts_from_value(&review).await;
    if !drifts.is_empty() {
        return drifts;
    }
    review
        .get("previousReviewedSnapshot")
        .and_then(|v| v.get("staleRepos"))
        .and_then(Value::as_array)
        .map(|repos| {
            repos
                .iter()
                .filter_map(review_snapshot_drift_from_stale_repo_value)
                .collect()
        })
        .unwrap_or_default()
}

pub(crate) async fn review_snapshot_drifts_from_value(review: &Value) -> Vec<ReviewSnapshotDrift> {
    let Some(repos) = review.get("repos").and_then(Value::as_array) else {
        return Vec::new();
    };
    let mut drifts = Vec::new();
    for repo in repos {
        let repo_name = value_string(repo, "repoName").unwrap_or_else(|| "repo".to_string());
        let branch = value_string(repo, "branch").unwrap_or_default();
        let reviewed_target_ref = value_string(repo, "resolvedTargetRef").unwrap_or_default();
        let reviewed_target_commit = value_string(repo, "targetCommit").unwrap_or_default();
        let Some(project_path) = value_string(repo, "projectPath").map(PathBuf::from) else {
            continue;
        };
        if branch.trim().is_empty()
            || reviewed_target_commit.trim().is_empty()
            || !project_path.exists()
        {
            continue;
        }
        let (current_target_ref, _) = resolve_target_ref(&project_path, &branch).await;
        let current_target_commit = match resolve_commit(&project_path, &current_target_ref).await {
            Value::String(v) => v,
            _ => String::new(),
        };
        if !current_target_commit.is_empty() && current_target_commit != reviewed_target_commit {
            drifts.push(ReviewSnapshotDrift {
                repo_name,
                branch,
                project_path: Some(project_path),
                reviewed_target_ref,
                reviewed_target_commit,
                current_target_ref,
                current_target_commit,
            });
        }
    }
    drifts
}

pub(crate) async fn incremental_review_drifts(req_dir: &Path) -> Vec<ReviewSnapshotDrift> {
    let Some(review) = read_json_if_exists(&req_dir.join(CODE_REVIEW_INCREMENTAL_FILE)).await
    else {
        return Vec::new();
    };
    let Some(repos) = review.get("repos").and_then(Value::as_array) else {
        return Vec::new();
    };
    repos
        .iter()
        .filter_map(incremental_review_drift_from_repo)
        .collect()
}

pub(crate) fn incremental_review_drift_from_repo(repo: &Value) -> Option<ReviewSnapshotDrift> {
    let reviewed_target_commit =
        value_string(repo, "coverageFromCommit").or_else(|| value_string(repo, "baseCommit"))?;
    let current_target_commit =
        value_string(repo, "coverageToCommit").or_else(|| value_string(repo, "targetCommit"))?;
    if reviewed_target_commit == current_target_commit {
        return None;
    }
    Some(ReviewSnapshotDrift {
        repo_name: value_string(repo, "repoName").unwrap_or_else(|| "repo".to_string()),
        branch: value_string(repo, "branch").unwrap_or_default(),
        project_path: value_string(repo, "projectPath").map(PathBuf::from),
        reviewed_target_ref: value_string(repo, "reviewedTargetRef")
            .or_else(|| value_string(repo, "baseCommit"))
            .unwrap_or_default(),
        reviewed_target_commit,
        current_target_ref: value_string(repo, "currentTargetRef")
            .or_else(|| value_string(repo, "targetCommit"))
            .unwrap_or_default(),
        current_target_commit,
    })
}

pub(crate) fn review_snapshot_drift_from_stale_repo_value(
    repo: &Value,
) -> Option<ReviewSnapshotDrift> {
    let reviewed_target_commit = value_string(repo, "reviewedTargetCommit")?;
    let current_target_commit = value_string(repo, "currentTargetCommit")?;
    if reviewed_target_commit == current_target_commit {
        return None;
    }
    Some(ReviewSnapshotDrift {
        repo_name: value_string(repo, "repoName").unwrap_or_else(|| "repo".to_string()),
        branch: value_string(repo, "branch").unwrap_or_default(),
        project_path: value_string(repo, "projectPath").map(PathBuf::from),
        reviewed_target_ref: value_string(repo, "reviewedTargetRef").unwrap_or_default(),
        reviewed_target_commit,
        current_target_ref: value_string(repo, "currentTargetRef").unwrap_or_default(),
        current_target_commit,
    })
}

pub(crate) async fn reviewed_incremental_covers_drifts(
    req_dir: &Path,
    drifts: &[ReviewSnapshotDrift],
) -> bool {
    if drifts.is_empty()
        || review_artifact_newer_than_review_docs(req_dir, CODE_REVIEW_INCREMENTAL_FILE).await
    {
        return false;
    }
    let Some(review) = read_json_if_exists(&req_dir.join(CODE_REVIEW_INCREMENTAL_FILE)).await
    else {
        return false;
    };
    let Some(repos) = review.get("repos").and_then(Value::as_array) else {
        return false;
    };
    drifts.iter().all(|drift| {
        repos
            .iter()
            .any(|repo| incremental_repo_covers_drift(repo, drift))
    })
}

pub(crate) fn incremental_repo_covers_drift(repo: &Value, drift: &ReviewSnapshotDrift) -> bool {
    if repo
        .get("linearHistory")
        .and_then(Value::as_bool)
        .is_some_and(|v| !v)
    {
        return false;
    }
    incremental_review_drift_from_repo(repo)
        .map(|inc| {
            inc.repo_name == drift.repo_name
                && inc.branch == drift.branch
                && inc.reviewed_target_commit == drift.reviewed_target_commit
                && inc.current_target_commit == drift.current_target_commit
        })
        .unwrap_or(false)
}

pub(crate) async fn review_artifact_newer_than_review_docs(req_dir: &Path, artifact: &str) -> bool {
    let Some(artifact_updated_at) = file_modified_ms(&req_dir.join(artifact)).await else {
        return false;
    };
    let newest_review_doc = ["review.md", "code-review-ai.md"]
        .iter()
        .filter_map(|name| std::fs::metadata(req_dir.join(name)).ok())
        .filter_map(|meta| meta.modified().ok())
        .map(system_time_to_ms)
        .max()
        .unwrap_or(0);
    newest_review_doc <= 0 || artifact_updated_at > newest_review_doc
}

pub(crate) fn value_string(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_string)
}

pub(crate) fn review_snapshot_drift_json(drift: ReviewSnapshotDrift) -> Value {
    json!({
        "repoName": drift.repo_name,
        "branch": drift.branch,
        "projectPath": drift.project_path.map(|p| p.to_string_lossy().to_string()),
        "reviewedTargetRef": drift.reviewed_target_ref,
        "reviewedTargetCommit": drift.reviewed_target_commit,
        "currentTargetRef": drift.current_target_ref,
        "currentTargetCommit": drift.current_target_commit,
    })
}

#[cfg(test)]
pub(crate) fn review_snapshot_drift_from_repo_value(
    repo: &Value,
    current_target_ref: &str,
    current_target_commit: &str,
) -> Option<ReviewSnapshotDrift> {
    let reviewed_target_commit = value_string(repo, "targetCommit")?;
    let current_target_commit = current_target_commit.trim();
    if current_target_commit.is_empty() || current_target_commit == reviewed_target_commit {
        return None;
    }
    Some(ReviewSnapshotDrift {
        repo_name: value_string(repo, "repoName").unwrap_or_else(|| "repo".to_string()),
        branch: value_string(repo, "branch").unwrap_or_default(),
        project_path: value_string(repo, "projectPath").map(PathBuf::from),
        reviewed_target_ref: value_string(repo, "resolvedTargetRef").unwrap_or_default(),
        reviewed_target_commit,
        current_target_ref: current_target_ref.trim().to_string(),
        current_target_commit: current_target_commit.to_string(),
    })
}

/// 读取 code-review.json 里聚合出的风险标签与库存风险标记。
pub(crate) async fn code_review_risk_for(req_dir: &Path) -> (Vec<String>, bool) {
    let mut tags = Vec::<String>::new();
    let mut inventory_risk = false;
    if let Ok(raw) = fs::read_to_string(&req_dir.join(CODE_REVIEW_FILE)).await {
        if let Ok(value) = serde_json::from_str::<Value>(&raw) {
            if let Some(repos) = value.get("repos").and_then(Value::as_array) {
                for repo in repos {
                    if repo
                        .get("inventoryRisk")
                        .and_then(Value::as_bool)
                        .unwrap_or(false)
                    {
                        inventory_risk = true;
                    }
                    if let Some(repo_tags) = repo.get("riskTags").and_then(Value::as_array) {
                        for tag in repo_tags {
                            if let Some(s) = tag.as_str() {
                                if !tags.contains(&s.to_string()) {
                                    tags.push(s.to_string());
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    (tags, inventory_risk)
}

/// 判断 review 文档是否包含库存账本专项评估的证据。
/// 命中库存风险时，要求至少出现库存专项小节，或命中 2 个以上账本关键点。
pub(crate) fn review_has_inventory_evidence(raw: &str) -> bool {
    let lower = raw.to_lowercase();
    if lower.contains("库存账本")
        || lower.contains("库存专项")
        || lower.contains("库存风险评估")
        || lower.contains("库存评估")
    {
        return true;
    }
    let keywords = [
        "可用量",
        "超卖",
        "占用",
        "释放",
        "回库",
        "onhandqty",
        "allocatedqty",
        "redis",
    ];
    keywords.iter().filter(|k| lower.contains(**k)).count() >= 2
}

pub(crate) fn review_gate_section(raw: &str, heading_keywords: &[&str]) -> Option<String> {
    let mut start: Option<(usize, usize)> = None;
    let lines: Vec<&str> = raw.lines().collect();
    for (idx, line) in lines.iter().enumerate() {
        if let Some((level, text)) = parse_markdown_heading(line) {
            let normalized = text.to_lowercase();
            if heading_keywords
                .iter()
                .any(|keyword| normalized.contains(&keyword.to_lowercase()))
            {
                start = Some((idx, level));
                break;
            }
        }
    }
    let (start_idx, start_level) = start?;
    let mut end_idx = lines.len();
    for (idx, line) in lines.iter().enumerate().skip(start_idx + 1) {
        if let Some((level, _)) = parse_markdown_heading(line) {
            if level <= start_level {
                end_idx = idx;
                break;
            }
        }
    }
    Some(lines[start_idx + 1..end_idx].join("\n"))
}

pub(crate) fn review_section_is_empty(section: &str) -> bool {
    !section.lines().any(|line| {
        let clean = line
            .trim()
            .trim_start_matches(['-', '*', ' ', '\t'])
            .trim_start_matches("[ ]")
            .trim_start_matches("[x]")
            .trim_start_matches("[X]")
            .trim()
            .trim_matches('。')
            .trim_matches('.')
            .trim();
        if clean.is_empty() {
            return false;
        }
        let lower = clean.to_lowercase();
        !matches!(
            lower.as_str(),
            "无" | "none" | "n/a" | "na" | "暂无" | "-" | "无。"
        ) && !clean.starts_with("无，")
            && !clean.starts_with("无；")
            && !clean.starts_with("无 ")
    })
}
