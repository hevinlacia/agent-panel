use super::*;

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
        "incident" => Some("req.incident"),
        "rootcause" => Some("req.rootCause"),
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
        "req.incident" => Some("incident.md"),
        "req.rootCause" => Some("root-cause.md"),
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
        "req.incident" => Some("incident"),
        "req.rootCause" => Some("root-cause"),
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
