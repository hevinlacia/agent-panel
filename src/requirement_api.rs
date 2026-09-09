use std::{collections::HashSet, path::PathBuf};

use axum::{
    extract::{Path as AxumPath, Query, State},
    http::HeaderMap,
    response::{Html, IntoResponse, Response},
    Json,
};
use serde_json::{json, Value};
use tokio::fs;
use uuid::Uuid;

use crate::*;

pub(crate) async fn api_requirements(State(state): State<AppState>) -> ApiResult<Json<Value>> {
    let requirements = list_requirements(&state).await?;
    Ok(Json(json!({ "requirements": requirements })))
}

pub(crate) async fn api_requirement(
    State(state): State<AppState>,
    Query(query): Query<IdQuery>,
) -> ApiResult<Json<Value>> {
    let id = query.id.or(query.req_id).unwrap_or_default();
    let req = get_requirement(&state, &id).await?;
    Ok(Json(json!({ "requirement": req })))
}

pub(crate) async fn api_requirements_post(
    State(state): State<AppState>,
    form: FormOrJson<RequirementCreateForm>,
) -> ApiResult<Json<Value>> {
    let created = create_requirement(&state, form.0).await?;
    Ok(Json(created))
}

pub(crate) async fn api_requirement_patch(
    State(state): State<AppState>,
    form: FormOrJson<RequirementPatchForm>,
) -> ApiResult<Json<Value>> {
    let updated = update_requirement(&state, form.0).await?;
    Ok(Json(updated))
}

pub(crate) async fn api_requirement_update(
    State(state): State<AppState>,
    form: FormOrJson<RequirementPatchForm>,
) -> ApiResult<Json<Value>> {
    let updated = update_requirement(&state, form.0).await?;
    Ok(Json(updated))
}

pub(crate) async fn api_requirement_notes(
    State(state): State<AppState>,
    form: FormOrJson<RequirementNoteForm>,
) -> ApiResult<Json<Value>> {
    let value = append_requirement_note(&state, form.0).await?;
    Ok(Json(value))
}

pub(crate) async fn api_requirement_events(
    State(state): State<AppState>,
    form: FormOrJson<RequirementEventForm>,
) -> ApiResult<Json<Value>> {
    let value = record_requirement_event(&state, form.0).await?;
    Ok(Json(value))
}

pub(crate) async fn api_requirement_section(
    State(state): State<AppState>,
    AxumPath(section): AxumPath<String>,
    form: FormOrJson<RequirementSectionForm>,
) -> ApiResult<Json<Value>> {
    let edit = requirement_section_form_to_edit(section, form.0)?;
    let value = upsert_requirement_section(&state, edit).await?;
    Ok(Json(value))
}

pub(crate) async fn api_requirement_doc(
    State(state): State<AppState>,
    form: FormOrJson<RequirementDocForm>,
) -> ApiResult<Json<Value>> {
    let value = write_requirement_doc(&state, form.0).await?;
    Ok(Json(value))
}

pub(crate) async fn api_requirement_doc_get(
    State(state): State<AppState>,
    Query(query): Query<IdQuery>,
) -> ApiResult<Json<Value>> {
    let req_id = query.id.or(query.req_id).unwrap_or_default();
    let req = get_real_requirement(&state, &req_id).await?;
    let doc_type = query
        .file
        .as_deref()
        .or(query.kind.as_deref())
        .unwrap_or("background");
    let doc_file = requirement_doc_file(doc_type)?;
    let dir = req_dir_path(&req)?;
    let path = dir.join(doc_file);
    let exists = path.is_file();
    let content = if exists {
        fs::read_to_string(&path).await.unwrap_or_default()
    } else {
        String::new()
    };
    let template = if exists {
        String::new()
    } else {
        requirement_doc_template(&req, doc_file)
    };
    Ok(Json(json!({
        "ok": true,
        "reqId": req.id,
        "docType": doc_type,
        "file": doc_file,
        "path": path.to_string_lossy(),
        "exists": exists,
        "content": content,
        "template": template,
    })))
}

pub(crate) async fn api_requirement_validate(
    State(state): State<AppState>,
    form: FormOrJson<RequirementValidateForm>,
) -> ApiResult<Json<Value>> {
    let req = get_real_requirement(&state, &form.0.req_id).await?;
    let value = validate_requirement(&state, &req).await?;
    Ok(Json(value))
}

pub(crate) async fn api_requirement_schema() -> Json<Value> {
    Json(requirement_api_schema())
}

pub(crate) async fn api_requirement_edit_plan(
    State(state): State<AppState>,
    Query(query): Query<IdQuery>,
) -> ApiResult<Json<Value>> {
    let req_id = query.id.or(query.req_id).unwrap_or_default();
    let req = get_real_requirement(&state, &req_id).await?;
    let intent = normalize_requirement_intent(query.intent.as_deref());
    ensure_requirement_intent(&intent)?;
    Ok(Json(build_requirement_edit_plan(&req, &intent)))
}

pub(crate) async fn api_requirement_context(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<IdQuery>,
) -> ApiResult<Response> {
    let req_id = query.id.or(query.req_id).unwrap_or_default();
    let req = get_real_requirement(&state, &req_id).await?;
    let intent = normalize_requirement_intent(query.intent.as_deref());
    ensure_requirement_intent(&intent)?;
    let budget = query.budget.unwrap_or(2_000).clamp(400, 12_000);
    let agent_context = query
        .for_agent
        .as_deref()
        .map(|v| v.eq_ignore_ascii_case("agent"))
        .unwrap_or(false)
        || query
            .kind
            .as_deref()
            .map(|v| v.eq_ignore_ascii_case("agent"))
            .unwrap_or(false);
    if agent_context {
        let limit = query.limit.unwrap_or(8).clamp(1, 30);
        let value = build_requirement_agent_context(&state, &req, &intent, budget, limit).await?;
        return Ok(Json(value).into_response());
    }
    let tokens = query
        .tokens
        .as_deref()
        .map(parse_token_list)
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| intent_read_tokens(&intent));
    let value = build_requirement_context(&req, &intent, tokens, budget).await?;
    // Browser/human-friendly rendering: explicit `format=html` or a text/html Accept header.
    // Programmatic callers (agents, curl) keep receiving JSON.
    let wants_html = query
        .format
        .as_deref()
        .map(|f| f.eq_ignore_ascii_case("html") || f.eq_ignore_ascii_case("md"))
        .unwrap_or(false)
        || headers
            .get("accept")
            .and_then(|v| v.to_str().ok())
            .map(|v| v.to_ascii_lowercase().contains("text/html"))
            .unwrap_or(false);
    if wants_html {
        return Ok(Html(render_requirement_context_html(&req, &intent, &value)).into_response());
    }
    Ok(Json(value).into_response())
}

pub(crate) async fn api_requirement_experience_summary_context(
    State(state): State<AppState>,
    Query(query): Query<IdQuery>,
) -> ApiResult<Json<Value>> {
    let req_id = query.id.or(query.req_id).unwrap_or_default();
    let req = get_real_requirement(&state, &req_id).await?;
    let dir = req_dir_path(&req)?;
    let events = read_recent_requirement_events(
        &dir.join(REQUIREMENT_EVENTS_FILE),
        query.limit.unwrap_or(200).clamp(20, 500),
    )
    .await;
    let mut referenced_ids = Vec::<String>::new();
    let mut references = Vec::<Value>::new();
    let mut learning_candidates = Vec::<Value>::new();
    let mut skill_candidates = Vec::<Value>::new();
    let mut other_candidates = Vec::<Value>::new();
    for event in &events {
        let event_type = event
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default();
        match event_type {
            "knowledgeReference" => {
                if let Some(ids) = event.get("relatedKnowledgeIds").and_then(Value::as_array) {
                    referenced_ids.extend(ids.iter().filter_map(Value::as_str).map(str::to_string));
                }
                references.push(event.clone());
            }
            "learningCandidate" => learning_candidates.push(event.clone()),
            "skillImprovementCandidate" => skill_candidates.push(event.clone()),
            _ => {
                if event.get("dedupeKey").and_then(Value::as_str).is_some()
                    || event.get("candidateType").and_then(Value::as_str).is_some()
                {
                    other_candidates.push(event.clone());
                }
            }
        }
    }
    let referenced_ids = unique_strings(referenced_ids);
    let duplicate_hints: Vec<Value> = learning_candidates
        .iter()
        .chain(skill_candidates.iter())
        .chain(other_candidates.iter())
        .map(|event| {
            let ids: Vec<String> = event
                .get("relatedKnowledgeIds")
                .and_then(Value::as_array)
                .map(|items| items.iter().filter_map(Value::as_str).map(str::to_string).collect())
                .unwrap_or_default();
            let overlaps: Vec<String> = ids
                .iter()
                .filter(|id| referenced_ids.contains(id))
                .cloned()
                .collect();
            json!({
                "eventId": event.get("id").cloned().unwrap_or(Value::Null),
                "summary": event.get("summary").cloned().unwrap_or(Value::Null),
                "dedupeKey": event.get("dedupeKey").cloned().unwrap_or(Value::Null),
                "relatedKnowledgeIds": ids,
                "overlapsReferencedKnowledge": overlaps,
                "suggestion": if overlaps.is_empty() { "review-and-possibly-land" } else { "likely-duplicate-or-update-existing" }
            })
        })
        .collect();
    Ok(Json(json!({
        "ok": true,
        "reqId": req.id,
        "category": req.category,
        "status": req.status,
        "eventsPath": dir.join(REQUIREMENT_EVENTS_FILE).to_string_lossy(),
        "referencedKnowledgeIds": referenced_ids,
        "knowledgeReferences": references,
        "learningCandidates": learning_candidates,
        "skillImprovementCandidates": skill_candidates,
        "otherCandidates": other_candidates,
        "duplicateHints": duplicate_hints,
        "recommendedWorkflow": [
            "1. Review knowledgeReferences first: do not recreate existing knowledge/experience.",
            "2. For each learningCandidate, search Agent Panel knowledge/experience by triggerTerms and relatedKnowledgeIds.",
            "3. Update existing item when duplicate or supplemental; create new item only when reusable and evidenced.",
            "4. Mark inferred facts as draft/needs-confirmation instead of active.",
            "5. Write final decisions to experience-summary.md.",
            "6. When auto summary is finished, call POST /api/experience-summary/jobs/complete."
        ],
        "recommendedWrites": [
            {"method":"POST","path":"/api/knowledge","purpose":"land business knowledge or experience after dedupe"},
            {"method":"POST","path":"/api/requirement/doc","body":{"reqId": req.id, "docType":"experience-summary", "mode":"replace", "content":"# ..."}},
            {"method":"POST","path":"/api/experience-summary/jobs/complete","body":{"reqId": req.id, "sessionId":"<current-session-id>", "note":"experience summary finished"}}
        ]
    })))
}

pub(crate) async fn api_requirement_experience_summary_report(
    State(state): State<AppState>,
    Query(query): Query<IdQuery>,
) -> ApiResult<Json<Value>> {
    let req_id = query.id.or(query.req_id).unwrap_or_default();
    let req = get_real_requirement(&state, &req_id).await?;
    let dir = req_dir_path(&req)?;
    let path = dir.join("experience-summary.md");
    let content = fs::read_to_string(&path).await.unwrap_or_default();
    let job = read_experience_summary_job(&dir).await?;
    Ok(Json(json!({
        "ok": true,
        "reqId": req.id,
        "path": path.to_string_lossy(),
        "exists": path.is_file(),
        "content": content,
        "job": normalize_experience_summary_job_value(&req.id, &dir, job),
    })))
}

pub(crate) async fn api_experience_summary_jobs(
    State(state): State<AppState>,
    Query(query): Query<IdQuery>,
) -> ApiResult<Json<Value>> {
    let mut reqs = list_requirements(&state).await?;
    if let Some(project) = query
        .project
        .as_deref()
        .map(str::trim)
        .filter(|p| !p.is_empty())
    {
        reqs.retain(|r| r.projects.iter().any(|p| p == project) || r.project == project);
    }
    if let Some(status) = query
        .status
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        reqs.retain(|r| experience_summary_stage(&r) == status || r.status == status);
    } else {
        reqs.retain(|r| r.status == "经验总结" || r.experience_summary_job.is_some());
    }
    let cfg = read_config(&state).await?;
    let items: Vec<Value> = reqs
        .into_iter()
        .map(|req| json!({
            "req": req,
            "stage": experience_summary_stage_from_job_value(req.experience_summary_job.as_ref(), &req.status),
        }))
        .collect();
    let stats = experience_summary_stats_from_items(&items);
    Ok(Json(json!({
        "ok": true,
        "generatedAt": now_ms(),
        "config": {
            "enabled": cfg.auto_experience_summary,
            "model": cfg.experience_summary_pi_model,
            "maxAgents": clamp_experience_summary_max_agents(cfg.experience_summary_max_agents)
        },
        "stats": stats,
        "items": items,
    })))
}

pub(crate) async fn api_experience_summary_jobs_dispatch(
    State(state): State<AppState>,
    Json(form): Json<ExperienceSummaryDispatchForm>,
) -> ApiResult<Json<Value>> {
    let report = dispatch_experience_summary_jobs(&state, form.req_id.as_deref()).await?;
    Ok(Json(json!({ "ok": true, "report": report })))
}

pub(crate) async fn api_experience_summary_jobs_retry(
    State(state): State<AppState>,
    Json(form): Json<ExperienceSummaryJobForm>,
) -> ApiResult<Json<Value>> {
    let req = get_real_requirement(&state, &form.req_id).await?;
    let dir = req_dir_path(&req)?;
    ensure_requirement_dir_writable(&state, &dir).await?;
    let existing = read_experience_summary_job(&dir).await?.unwrap_or_default();
    let now = now_ms();
    let job = ExperienceSummaryJob {
        version: 1,
        req_id: req.id.clone(),
        status: "pending".to_string(),
        session_id: None,
        model: None,
        started_at: None,
        finished_at: None,
        attempts: existing.attempts,
        error: form.note.or(form.error),
        report_path: Some(
            dir.join("experience-summary.md")
                .to_string_lossy()
                .to_string(),
        ),
        updated_at: now,
    };
    write_experience_summary_job(&dir, &job).await?;
    let report = dispatch_experience_summary_jobs(&state, Some(&req.id)).await?;
    Ok(Json(json!({ "ok": true, "job": job, "dispatch": report })))
}

pub(crate) async fn api_experience_summary_jobs_complete(
    State(state): State<AppState>,
    Json(form): Json<ExperienceSummaryCompleteForm>,
) -> ApiResult<Json<Value>> {
    let req = get_real_requirement(&state, &form.req_id).await?;
    let dir = req_dir_path(&req)?;
    ensure_requirement_dir_writable(&state, &dir).await?;
    let mut job = read_experience_summary_job(&dir)
        .await?
        .unwrap_or_else(|| pending_experience_summary_job(&req, &dir));
    if let Some(session_id) = clean_optional(form.session_id.as_deref()) {
        job.session_id = Some(session_id);
    }
    let now = now_ms();
    job.version = 1;
    job.req_id = req.id.clone();
    job.status = "completed".to_string();
    job.finished_at = Some(now);
    job.error = None;
    job.report_path = Some(
        dir.join("experience-summary.md")
            .to_string_lossy()
            .to_string(),
    );
    job.updated_at = now;
    write_experience_summary_job(&dir, &job).await?;
    record_requirement_event(
        &state,
        RequirementEventForm {
            req_id: req.id.clone(),
            event_type: Some("progress".to_string()),
            title: Some("自动经验总结完成".to_string()),
            summary: Some("自动经验总结已完成".to_string()),
            details: form.note.clone(),
            evidence: job.report_path.clone().into_iter().collect(),
            decisions: Vec::new(),
            todos: Vec::new(),
            related_files: vec![
                "experience-summary.md".to_string(),
                EXPERIENCE_SUMMARY_JOB_FILE.to_string(),
            ],
            related_knowledge_ids: Vec::new(),
            trigger_terms: Vec::new(),
            related_repos: Vec::new(),
            related_tables: Vec::new(),
            related_apis: Vec::new(),
            candidate_type: None,
            dedupe_key: None,
            confidence: Some("confirmed".to_string()),
            target: Some("experience-summary".to_string()),
            test_cases: Vec::new(),
            status: Some(req.status.clone()),
            risk_level: None,
            tags: vec![
                "experience-summary".to_string(),
                "auto-summary".to_string(),
                "completed".to_string(),
            ],
            session_id: job.session_id.clone(),
            idempotency_key: Some(format!("{}-auto-experience-summary-completed", req.id)),
            append_note: Some(true),
            dry_run: Some(false),
        },
    )
    .await?;
    let mut status_state = Value::Null;
    if req.status == "经验总结" {
        status_state = write_requirement_status(
            dir.to_string_lossy().as_ref(),
            "已完成",
            Some("自动经验总结完成，需求自动推进为已完成"),
        )
        .await?;
        let refreshed_req = Requirement {
            status: "经验总结".to_string(),
            ..req.clone()
        };
        if let Err(e) = record_status_transition_event(
            &state,
            &refreshed_req,
            &status_state,
            Some("自动经验总结完成，需求自动推进为已完成"),
        )
        .await
        {
            tracing::warn!(req_id = %req.id, "auto-summary complete status event failed: {e:?}");
        }
    }
    Ok(Json(
        json!({ "ok": true, "job": job, "state": status_state }),
    ))
}

pub(crate) async fn api_requirement_edit(
    State(state): State<AppState>,
    form: FormOrJson<RequirementEditForm>,
) -> ApiResult<Json<Value>> {
    let value = apply_requirement_edit(&state, form.0).await?;
    Ok(Json(value))
}

pub(crate) async fn api_requirement_status(
    State(state): State<AppState>,
    form: FormOrJson<StatusForm>,
) -> ApiResult<Json<Value>> {
    let body = form.0;
    let status = canonical_status(&body.status)?;
    let req = get_real_requirement(&state, &body.req_id).await?;
    if should_enforce_review_gate_for_status(&req.status, &status) {
        ensure_review_gate_allows_testing(&req).await?;
    }
    if req.source == "开发推动" && status == "测试中" {
        ensure_test_scenario_allows_testing(&req).await?;
    }
    let st = write_requirement_status(
        req.req_dir.as_deref().unwrap_or_default(),
        &status,
        body.note.as_deref(),
    )
    .await?;
    if !matches!(st.get("changed").and_then(Value::as_bool), Some(false)) {
        record_status_transition_event(&state, &req, &st, body.note.as_deref()).await?;
        if status == "经验总结"
            || (req.category.as_deref() == Some("线上问题") && status == "已复盘")
        {
            let cfg = read_config(&state).await.unwrap_or_default();
            if cfg.auto_experience_summary {
                if let Err(e) = dispatch_experience_summary_jobs(&state, Some(&req.id)).await {
                    tracing::warn!(req_id = %req.id, "auto experience summary dispatch after status change failed: {e:?}");
                }
            }
        }
    }
    Ok(Json(json!({ "ok": true, "state": st })))
}

pub(crate) async fn api_requirement_category(
    State(state): State<AppState>,
    form: FormOrJson<CategoryForm>,
) -> ApiResult<Json<Value>> {
    let body = form.0;
    ensure_category(&body.category)?;
    let req = get_real_requirement(&state, &body.req_id).await?;
    let st = write_requirement_category(req.req_dir.as_deref().unwrap_or_default(), &body.category)
        .await?;
    let mut status_state = Value::Null;
    if body.category == "线上问题" && !ISSUE_STATUSES.contains(&req.status.as_str()) {
        status_state = write_requirement_status(
            req.req_dir.as_deref().unwrap_or_default(),
            "排查中",
            Some("切换为线上问题，进入轻量排查流程"),
        )
        .await?;
    } else if body.category == "需求" && ISSUE_STATUSES.contains(&req.status.as_str()) {
        status_state = write_requirement_status(
            req.req_dir.as_deref().unwrap_or_default(),
            "需求澄清",
            Some("从线上问题切回需求流程"),
        )
        .await?;
    }
    Ok(Json(
        json!({ "ok": true, "state": st, "statusState": status_state }),
    ))
}

pub(crate) async fn api_requirement_convert_issue(
    State(state): State<AppState>,
    form: FormOrJson<ConvertIssueForm>,
) -> ApiResult<Json<Value>> {
    let body = form.0;
    let issue = get_real_requirement(&state, &body.req_id).await?;
    if issue.category.as_deref() != Some("线上问题") {
        return Err(ApiError::bad_request(format!(
            "{} 不是线上问题，无需转换",
            issue.id
        )));
    }
    // 代码修复路径：创建独立的普通需求承接修复，线上问题保持 category=线上问题 不变，
    // 绑定在新需求的 meta.md issues 字段；需求进入 >= 经验总结 时自动把问题推进到已修复。
    let created = create_requirement(
        &state,
        RequirementCreateForm {
            req_id: String::new(),
            title: format!("{}（代码修复）", issue.title),
            project: Some(issue.project.clone()),
            projects: Some(issue.projects.clone()),
            group_path: None,
            parent_req_id: None,
            root: None,
            status: Some("需求澄清".to_string()),
            category: Some("需求".to_string()),
            owner: None,
            start_date: None,
            plan_release: None,
            ones: None,
            source: None,
            issues: Some(vec![issue.id.clone()]),
            summary: Some(format!(
                "由线上问题 {} 转出的代码修复需求；排查过程见原问题。",
                issue.id
            )),
            background: None,
            notes: None,
            dry_run: Some(false),
        },
    )
    .await?;
    let fix_req_id = created
        .get("reqId")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    // 问题侧：仍处于排查中则推进到已定位（根因已明确、需要代码修复）；已定位/已修复等保持不变。
    let issue_status_state = if issue.status == "排查中" {
        write_requirement_status(
            issue.req_dir.as_deref().unwrap_or_default(),
            "已定位",
            Some(&format!("已转代码修复需求 {fix_req_id}")),
        )
        .await?
    } else {
        Value::Null
    };
    let event = record_requirement_event(
        &state,
        RequirementEventForm {
            req_id: issue.id.clone(),
            event_type: Some("decision".to_string()),
            title: Some("线上问题转代码修复需求".to_string()),
            summary: Some(format!(
                "已创建修复需求 {fix_req_id} 并绑定本问题；需求进入经验总结后自动推进本问题到已修复"
            )),
            details: body.note,
            evidence: Vec::new(),
            decisions: vec![format!("fixRequirement: {fix_req_id}")],
            todos: Vec::new(),
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
            status: None,
            risk_level: None,
            tags: vec![
                "online-issue".to_string(),
                "convert-to-requirement".to_string(),
            ],
            session_id: None,
            idempotency_key: Some(format!("{}-convert-issue-{}", issue.id, now_ms())),
            append_note: Some(true),
            dry_run: Some(false),
        },
    )
    .await?;
    Ok(Json(json!({
        "ok": true,
        "reqId": issue.id,
        "fixReqId": fix_req_id,
        "issueStatusState": issue_status_state,
        "created": created,
        "event": event,
    })))
}

pub(crate) async fn api_requirement_ones(
    State(state): State<AppState>,
    form: FormOrJson<OnesForm>,
) -> ApiResult<Json<Value>> {
    let body = form.0;
    let req = get_real_requirement(&state, &body.req_id).await?;
    let ones = body.ones.unwrap_or_default();
    let stored = write_requirement_ones(req.req_dir.as_deref().unwrap_or_default(), &ones).await?;
    Ok(Json(
        json!({ "ok": true, "ones": stored, "ref": parse_ones_ref(&stored) }),
    ))
}

pub(crate) async fn api_requirement_associate(
    State(state): State<AppState>,
    form: FormOrJson<AssociateForm>,
) -> ApiResult<Json<Value>> {
    let body = form.0;
    associate_session(&state, &body.req_id, &body.session_id).await?;
    Ok(Json(json!({ "ok": true })))
}

pub(crate) async fn api_requirement_dissociate(
    State(state): State<AppState>,
    form: FormOrJson<AssociateForm>,
) -> ApiResult<Json<Value>> {
    let body = form.0;
    dissociate_session(&state, &body.req_id, &body.session_id).await?;
    Ok(Json(json!({ "ok": true })))
}

/// Resolve the requirement a dsh session is bound to (reads associations.json).
/// Used by the dsh-agentpanel-requirement plugin to recover a binding across
/// process restarts without scanning session logs.
pub(crate) async fn api_requirement_by_session(
    State(state): State<AppState>,
    Query(query): Query<IdQuery>,
) -> ApiResult<Json<Value>> {
    let session_id = query.session_id.unwrap_or_default();
    if session_id.is_empty() {
        return Err(ApiError::bad_request("missing sessionId"));
    }
    let store = crate::requirement_index::load_associations(&state).await?;
    let req_id = store
        .associations
        .iter()
        .find(|(_, sids)| sids.iter().any(|s| s == &session_id))
        .map(|(key, _)| key.clone());
    Ok(Json(json!({
        "ok": true,
        "sessionId": session_id,
        "reqId": req_id,
    })))
}

pub(crate) async fn api_requirement_new_session(
    State(state): State<AppState>,
    form: FormOrJson<NewSessionForm>,
) -> ApiResult<Json<Value>> {
    let body = form.0;
    let req = get_real_requirement(&state, &body.req_id).await?;
    let project_root = requirement_project_root(&req).map(|p| p.to_string_lossy().to_string());
    // dsh-web 模式：不生成启动命令，改为对常驻 dsh 进程发 RPC——
    // 1) session.create 预分配 session id；2) commands/execute 触发
    // /requirement-bind（dsh 插件写入关联并准备注入需求上下文）。
    if crate::sessions::current_harness(&state) == "dsh-web" {
        let cfg = crate::config::read_config(&state).await.unwrap_or_default();
        let client = crate::dsh_client::DshClient::new(&cfg.dsh_api_base_url);
        if !client.healthy().await {
            return Err(ApiError::bad_request(format!(
                "dsh 未运行（{}）：请先启动 dsh --profile web，或在 Settings 里调整 dshApiBaseUrl",
                cfg.dsh_api_base_url
            )));
        }
        let session_id = Uuid::new_v4().to_string();
        let created = client
            .create_session(&session_id, project_root.as_deref())
            .await
            .map_err(|e| ApiError::from(anyhow::anyhow!(e.to_string())))?;
        let bind = client
            .run_command(&created, &format!("/requirement-bind {}", req.id))
            .await
            .map_err(|e| ApiError::from(anyhow::anyhow!(e.to_string())))?;
        // Mirror the association in our own index so the requirement page shows
        // the session immediately (associations.json is the durable authority;
        // this keeps the in-memory view in sync).
        associate_session(&state, &req.id, &created).await?;
        // An empty `commands/execute` value means the /requirement-bind command
        // is not registered — the dsh-agentpanel-requirement plugin is not
        // mounted in the running dsh yet. The binding itself is already
        // durable (agent-panel store), so only the context injection is
        // deferred until the plugin is loaded.
        let command_dispatched = bind
            .get("result")
            .and_then(|r| r.get("kind"))
            .and_then(Value::as_str)
            == Some("success");
        let url = format!("{}/", cfg.dsh_api_base_url);
        return Ok(Json(json!({
            "ok": true,
            "harness": "dsh-web",
            "sessionId": created,
            "url": url,
            "cwd": project_root,
            "bind": bind,
            "contextInjectionReady": command_dispatched,
        })));
    }
    // dsh-tui / pi 模式：与当前 harness 同构的可粘贴终端命令，带 pending 复用：
    // - 重复点击复制命令时复用未使用过的 pending session id（不新产生 id）；
    // - pending 的 session 已被使用（harness session 库里已有对应 session 文件）时自动换新；
    // - force=true（强制刷新）时无视使用状态直接废弃旧 pending、生成新 id；
    // - pending 与当前 harness 不匹配（中途切换 harness）也视为过期重新生成。
    let harness = crate::sessions::current_harness(&state);
    let binary = if harness == "dsh-tui" {
        "dsh-tui"
    } else {
        "pi"
    };
    let force = body.force.unwrap_or(false);
    if let Some(pending) = load_pending_command(&state, &req.id).await? {
        let stale = pending.harness != harness;
        let used = !stale
            && crate::sessions::session_id_is_used(&state, &pending.session_id, &pending.harness)
                .await;
        if !force && !stale && !used {
            // 未使用过：原样复用，不产生新 session id。
            return Ok(Json(json!(
                { "ok": true, "harness": harness, "sessionId": pending.session_id, "command": pending.command, "contextPath": pending.context_path, "cwd": project_root, "reused": true }
            )));
        }
        // 废弃 pending：已使用的 session 保留关联；未使用过的（force 刷新或 harness 切换）
        // 同步清理关联和 ctx 文件，避免留下悬空 session。
        if !used {
            let _ = dissociate_session(&state, &req.id, &pending.session_id).await;
            let _ = fs::remove_file(&pending.context_path).await;
        }
        let _ = save_pending_command(&state, &req.id, None).await;
    }
    let session_id = Uuid::new_v4().to_string();
    associate_session(&state, &body.req_id, &session_id).await?;
    let ctx_path = write_injection_context(&state, &req, &session_id).await?;
    let title = shell_quote(&req.title);
    let ctx = shell_quote(ctx_path.to_string_lossy().as_ref());
    let core_command = format!(
        "{} --session-id {} --name {} --append-system-prompt @{}",
        binary, session_id, title, ctx
    );
    let command = if let Some(root) = &project_root {
        format!("cd {} && {}", shell_quote(root), core_command)
    } else {
        core_command
    };
    save_pending_command(
        &state,
        &req.id,
        Some(PendingSessionCommand {
            session_id: session_id.clone(),
            command: command.clone(),
            harness: harness.clone(),
            context_path: ctx_path.to_string_lossy().to_string(),
            created_at: now_ms(),
        }),
    )
    .await?;
    Ok(Json(
        json!({ "ok": true, "harness": harness, "sessionId": session_id, "command": command, "contextPath": ctx_path, "cwd": project_root, "reused": false }),
    ))
}

/// Read-only view of the requirement's pending terminal launch command (no
/// side effects). Lets the requirement page show the current command without
/// generating one; `used` hints whether the next copy click will auto-generate
/// a fresh session id.
pub(crate) async fn api_requirement_pending_session(
    State(state): State<AppState>,
    Query(query): Query<IdQuery>,
) -> ApiResult<Json<Value>> {
    let id = query.id.or(query.req_id).unwrap_or_default();
    let req = get_real_requirement(&state, &id).await?;
    let harness = crate::sessions::current_harness(&state);
    let pending = load_pending_command(&state, &req.id).await?;
    let Some(pending) = pending else {
        return Ok(Json(
            json!({ "ok": true, "harness": harness, "pending": null }),
        ));
    };
    let used =
        crate::sessions::session_id_is_used(&state, &pending.session_id, &pending.harness).await;
    Ok(Json(json!({
        "ok": true,
        "harness": harness,
        "pending": {
            "sessionId": pending.session_id,
            "command": pending.command,
            "contextPath": pending.context_path,
            "harness": pending.harness,
            "createdAt": pending.created_at,
            "used": used,
        },
    })))
}

/// Unassociated session candidates (of the current harness) for a requirement,
/// newest first, filtered to sessions whose directory overlaps the project root.
/// Lets the user link a dsh-tui session it opened manually.
pub(crate) async fn api_requirement_session_candidates(
    State(state): State<AppState>,
    Query(query): Query<IdQuery>,
) -> ApiResult<Json<Value>> {
    let id = query.id.or(query.req_id).unwrap_or_default();
    let req = get_real_requirement(&state, &id).await?;
    let harness = crate::sessions::current_harness(&state);
    let project_root = requirement_project_root(&req).map(|p| p.to_string_lossy().to_string());
    let exclude: HashSet<String> = req.session_ids.iter().cloned().collect();
    let candidates =
        crate::sessions::scan_session_candidates(&state, project_root.as_deref(), &exclude).await?;
    Ok(Json(json!({
        "harness": harness,
        "projectRoot": project_root,
        "candidates": candidates,
    })))
}

pub(crate) async fn api_requirement_code_review(
    State(state): State<AppState>,
    Query(query): Query<IdQuery>,
) -> ApiResult<Json<Value>> {
    let id = query.id.or(query.req_id).unwrap_or_default();
    let req = get_real_requirement(&state, &id).await?;
    let req_dir = PathBuf::from(req.req_dir.unwrap_or_default());
    let branch_scope = read_branch_scope(&req_dir).await?;
    let review = read_json_if_exists(&req_dir.join(CODE_REVIEW_FILE)).await;
    let incremental_review = read_json_if_exists(&req_dir.join(CODE_REVIEW_INCREMENTAL_FILE)).await;
    Ok(Json(
        json!({ "ok": true, "branchScope": branch_scope, "review": review, "incrementalReview": incremental_review }),
    ))
}

pub(crate) async fn api_requirement_code_review_post(
    State(state): State<AppState>,
    form: FormOrJson<CodeReviewForm>,
) -> ApiResult<Json<Value>> {
    let req = get_real_requirement(&state, &form.0.req_id).await?;
    let req_dir = PathBuf::from(req.req_dir.unwrap_or_default());
    let branch_scope = read_branch_scope(&req_dir).await?.ok_or_else(|| {
        ApiError::bad_request(format!(
            "missing {BRANCH_SCOPE_FILE}; run req-branches-update first"
        ))
    })?;
    // 刷新代码差异时不再同步生产基线分支;
    // 如需同步本地 base 分支到最新远端,由独立的“同步生产基线”按钮触发(/api/requirement/sync-base)。
    let review = run_code_review_scan(&req_dir, &req.id, &branch_scope).await?;
    Ok(Json(json!({
        "ok": true,
        "branchScope": branch_scope,
        "review": review,
    })))
}

pub(crate) async fn api_requirement_code_review_incremental_post(
    State(state): State<AppState>,
    form: FormOrJson<CodeReviewForm>,
) -> ApiResult<Json<Value>> {
    let req = get_real_requirement(&state, &form.0.req_id).await?;
    let req_dir = PathBuf::from(req.req_dir.unwrap_or_default());
    let branch_scope = read_branch_scope(&req_dir).await?;
    let review = run_code_review_incremental_scan(&req_dir, &req.id).await?;
    Ok(Json(json!({
        "ok": true,
        "branchScope": branch_scope,
        "incrementalReview": review,
    })))
}

pub(crate) async fn api_requirement_review_gate(
    State(state): State<AppState>,
    Query(query): Query<IdQuery>,
) -> ApiResult<Json<Value>> {
    let id = query.id.or(query.req_id).unwrap_or_default();
    let req = get_real_requirement(&state, &id).await?;
    Ok(Json(review_gate_json(&req).await?))
}

pub(crate) async fn api_requirement_master_diff(
    State(state): State<AppState>,
    form: FormOrJson<CodeReviewForm>,
) -> ApiResult<Json<Value>> {
    let body = form.0;
    let req = get_real_requirement(&state, &body.req_id).await?;
    let req_dir = PathBuf::from(req.req_dir.ok_or_else(|| {
        ApiError::bad_request("requirement has no directory; cannot save diff snapshot".to_string())
    })?);
    let branch_scope = read_branch_scope(&req_dir).await?.ok_or_else(|| {
        ApiError::bad_request(format!(
            "missing {BRANCH_SCOPE_FILE}; run req-branches-update first"
        ))
    })?;
    let base_ref = body
        .base_ref
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .unwrap_or("origin/master");
    let review = run_master_diff_scan(&req.id, &branch_scope, base_ref).await?;
    // 快照对比模式：生成后入栈保存（保留最近 5 版，同 base+target 提交去重），
    // 差异页展示栈内任意版本；误点刷新可回退上一版，无需重新生成。
    let snapshots = save_diff_snapshot(&req_dir, review).await?;
    Ok(Json(
        json!({ "ok": true, "branchScope": branch_scope, "snapshots": snapshots }),
    ))
}

/// Read the requirement's saved master-diff snapshot stack (newest first).
/// Returns an empty list when none has been generated yet.
pub(crate) async fn api_requirement_diff_snapshots_get(
    State(state): State<AppState>,
    Query(query): Query<IdQuery>,
) -> ApiResult<Json<Value>> {
    let id = query.id.or(query.req_id).unwrap_or_default();
    let req = get_real_requirement(&state, &id).await?;
    let req_dir = PathBuf::from(req.req_dir.unwrap_or_default());
    let doc = read_json_if_exists(&req_dir.join(CODE_DIFF_SNAPSHOTS_FILE)).await;
    let snapshots = doc
        .and_then(|d| d.get("snapshots").cloned())
        .unwrap_or_else(|| json!([]));
    Ok(Json(json!({ "ok": true, "snapshots": snapshots })))
}

/// Read the code-annotations.json snapshot for a requirement (null when absent).
/// Annotations explain key variables, design intent, and data/state flow per
/// diffed file; they are written either by a pi session (diff-annotate skill)
/// or manually from the diff inspector panel.
pub(crate) async fn api_requirement_annotations_get(
    State(state): State<AppState>,
    Query(query): Query<IdQuery>,
) -> ApiResult<Json<Value>> {
    let id = query.id.or(query.req_id).unwrap_or_default();
    let req = get_real_requirement(&state, &id).await?;
    let req_dir = PathBuf::from(req.req_dir.unwrap_or_default());
    let annotations = read_json_if_exists(&req_dir.join(CODE_ANNOTATIONS_FILE)).await;
    Ok(Json(json!({ "ok": true, "annotations": annotations })))
}

/// Save the whole code-annotations.json snapshot for a requirement.
/// Accepts { reqId, annotations } where annotations is the full document;
/// updatedAt is stamped here so callers never forge it.
pub(crate) async fn api_requirement_annotations_put(
    State(state): State<AppState>,
    form: FormOrJson<AnnotationsSaveForm>,
) -> ApiResult<Json<Value>> {
    let body = form.0;
    let req = get_real_requirement(&state, &body.req_id).await?;
    let req_dir = PathBuf::from(req.req_dir.ok_or_else(|| {
        ApiError::bad_request("requirement has no directory; cannot save annotations".to_string())
    })?);
    ensure_requirement_dir_writable(&state, &req_dir).await?;
    let mut annotations = body.annotations.unwrap_or(json!({}));
    if !annotations.is_object() {
        return Err(ApiError::bad_request(
            "annotations must be a JSON object".to_string(),
        ));
    }
    if let Some(obj) = annotations.as_object_mut() {
        obj.insert("updatedAt".to_string(), json!(now_ms()));
        obj.entry("version").or_insert(json!(1));
        obj.entry("reqId").or_insert(json!(req.id));
    }
    let path = req_dir.join(CODE_ANNOTATIONS_FILE);
    atomic_write_json(&path, &annotations).await?;
    Ok(Json(json!({ "ok": true, "annotations": annotations })))
}

pub(crate) async fn api_requirement_sync_base(
    State(state): State<AppState>,
    form: FormOrJson<SyncBaseForm>,
) -> ApiResult<Json<Value>> {
    let req = get_real_requirement(&state, &form.0.req_id).await?;
    let branch_scope = read_branch_scope(&PathBuf::from(req.req_dir.unwrap_or_default()))
        .await?
        .ok_or_else(|| {
            ApiError::bad_request(format!(
                "missing {BRANCH_SCOPE_FILE}; run req-branches-update first"
            ))
        })?;
    let mut results = Vec::new();
    for repo in &branch_scope.repos {
        results.push(sync_repo_base_branch(repo).await);
    }
    Ok(Json(json!({
        "ok": true,
        "generatedAt": now_ms(),
        "results": results,
    })))
}

pub(crate) async fn api_requirement_merge_options(
    State(state): State<AppState>,
    Query(query): Query<IdQuery>,
) -> ApiResult<Json<Value>> {
    let id = query.id.or(query.req_id).unwrap_or_default();
    let req = get_real_requirement(&state, &id).await?;
    let req_dir = PathBuf::from(req.req_dir.as_deref().unwrap_or_default());
    let branch_scope = read_branch_scope(&req_dir).await?.ok_or_else(|| {
        ApiError::bad_request(format!(
            "missing {BRANCH_SCOPE_FILE}; run req-branches-update first"
        ))
    })?;
    let options = build_merge_options(&branch_scope, &req.status).await;
    Ok(Json(json!({
        "ok": true,
        "reqId": req.id,
        "status": req.status,
        "generatedAt": now_ms(),
        "branchScope": branch_scope,
        "options": options,
    })))
}

pub(crate) async fn api_requirement_merge_branch(
    State(state): State<AppState>,
    form: FormOrJson<MergeBranchForm>,
) -> ApiResult<Json<Value>> {
    let body = form.0;
    let req = get_real_requirement(&state, &body.req_id).await?;
    let req_dir = PathBuf::from(req.req_dir.as_deref().unwrap_or_default());
    let branch_scope = read_branch_scope(&req_dir).await?.ok_or_else(|| {
        ApiError::bad_request(format!(
            "missing {BRANCH_SCOPE_FILE}; run req-branches-update first"
        ))
    })?;
    let merge_request = normalize_merge_request(&body)?;
    let results = merge_requirement_branches(&branch_scope, &merge_request).await;
    let status = merge_overall_status(&results);
    Ok(Json(json!({
        "ok": matches!(status, "merged" | "skipped" | "empty"),
        "reqId": req.id,
        "target": merge_request.target,
        "targetBranch": merge_request.target_branch,
        "repoKind": merge_request.repo_kind,
        "status": status,
        "generatedAt": now_ms(),
        "branchScope": branch_scope,
        "results": results,
    })))
}

pub(crate) async fn api_requirement_merge_status(
    State(state): State<AppState>,
    Query(query): Query<IdQuery>,
) -> ApiResult<Json<Value>> {
    let id = query.id.or(query.req_id).unwrap_or_default();
    let req = get_real_requirement(&state, &id).await?;
    let req_dir = PathBuf::from(req.req_dir.as_deref().unwrap_or_default());
    let branch_scope = read_branch_scope(&req_dir).await?.ok_or_else(|| {
        ApiError::bad_request(format!(
            "missing {BRANCH_SCOPE_FILE}; run req-branches-update first"
        ))
    })?;
    let target = match query
        .target
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
    {
        Some(raw) => Some(normalize_merge_target(raw)?),
        None => None,
    };
    let results = inspect_requirement_merge_status(&branch_scope, target.clone()).await;
    let status = merge_overall_status(&results);
    Ok(Json(json!({
        "ok": true,
        "reqId": req.id,
        "target": target,
        "status": status,
        "generatedAt": now_ms(),
        "branchScope": branch_scope,
        "results": results,
    })))
}

pub(crate) async fn api_requirement_prod_mrs(
    State(state): State<AppState>,
    form: FormOrJson<ProdMrForm>,
) -> ApiResult<Json<Value>> {
    let req = get_real_requirement(&state, &form.0.req_id).await?;
    ensure_review_gate_allows_testing(&req).await?;
    let req_dir = PathBuf::from(req.req_dir.as_deref().unwrap_or_default());
    let branch_scope = read_branch_scope(&req_dir).await?.ok_or_else(|| {
        ApiError::bad_request(format!(
            "missing {BRANCH_SCOPE_FILE}; run req-branches-update first"
        ))
    })?;
    let results = generate_prod_mrs(&req, &branch_scope).await?;
    Ok(Json(json!({
        "ok": true,
        "reqId": req.id,
        "generatedAt": now_ms(),
        "branchScope": branch_scope,
        "results": results,
    })))
}

pub(crate) async fn api_auto_drive() -> Json<Value> {
    Json(
        json!({ "jobs": [], "active": 0, "blocked": 0, "queue": { "active": 0, "queued": 0 }, "message": "auto-drive was removed with the legacy Node backend" }),
    )
}

pub(crate) async fn api_auto_drive_post() -> Json<Value> {
    Json(
        json!({ "jobs": [], "errors": [], "message": "auto-drive is not available in the Rust rewrite yet" }),
    )
}

pub(crate) async fn api_recommendations(
    State(state): State<AppState>,
    Query(query): Query<IdQuery>,
) -> ApiResult<Json<Value>> {
    let req_id = query.id.or(query.req_id).unwrap_or_default();
    let req = get_requirement(&state, &req_id).await?;
    let existing: HashSet<String> = req
        .as_ref()
        .map(|r| r.session_ids.iter().cloned().collect())
        .unwrap_or_default();
    let sessions = scan_sessions_for_current_harness(&state, query.days).await?;
    let recommendations: Vec<Value> = sessions
        .into_iter()
        .filter(|s| !existing.contains(&s.id))
        .take(12)
        .map(|session| json!({ "session": session, "score": 25, "reasons": ["recent pi session"] }))
        .collect();
    Ok(Json(json!({ "recommendations": recommendations })))
}

pub(crate) async fn api_effort_estimate(
    State(state): State<AppState>,
    form: FormOrJson<NewSessionForm>,
) -> ApiResult<Json<Value>> {
    let req = get_real_requirement(&state, &form.0.req_id).await?;
    let estimate = json!({
        "version": 1,
        "coefficient": 1.0,
        "baseHours": 4,
        "estimatedHours": 4,
        "factors": [],
        "summary": "Rust rewrite placeholder: AI effort estimation has not been reimplemented yet.",
        "model": "manual-placeholder",
        "updatedAt": now_ms()
    });
    if let Some(dir) = req.req_dir {
        let path = PathBuf::from(dir).join("effort-estimate.json");
        atomic_write_json(&path, &estimate).await?;
    }
    Ok(Json(json!({ "ok": true, "estimate": estimate })))
}
