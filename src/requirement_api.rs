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
    let st = write_requirement_status(
        req.req_dir.as_deref().unwrap_or_default(),
        &status,
        body.note.as_deref(),
    )
    .await?;
    if !matches!(st.get("changed").and_then(Value::as_bool), Some(false)) {
        record_status_transition_event(&state, &req, &st, body.note.as_deref()).await?;
        if status == "经验总结" {
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
    let req = get_real_requirement(&state, &body.req_id).await?;
    let category_state =
        write_requirement_category(req.req_dir.as_deref().unwrap_or_default(), "需求").await?;
    let status_state = write_requirement_status(
        req.req_dir.as_deref().unwrap_or_default(),
        "需求澄清",
        body.note
            .as_deref()
            .or(Some("线上问题已确认需要代码/需求流程承接")),
    )
    .await?;
    let event = record_requirement_event(
        &state,
        RequirementEventForm {
            req_id: req.id.clone(),
            event_type: Some("decision".to_string()),
            title: Some("线上问题转需求".to_string()),
            summary: Some("线上问题已转为普通需求流程".to_string()),
            details: body.note,
            evidence: Vec::new(),
            decisions: vec![
                "category: 线上问题 -> 需求".to_string(),
                "status: 需求澄清".to_string(),
            ],
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
            status: Some("需求澄清".to_string()),
            risk_level: None,
            tags: vec![
                "online-issue".to_string(),
                "convert-to-requirement".to_string(),
            ],
            session_id: None,
            idempotency_key: Some(format!("{}-convert-issue-{}", req.id, now_ms())),
            append_note: Some(true),
            dry_run: Some(false),
        },
    )
    .await?;
    Ok(Json(json!({
        "ok": true,
        "reqId": req.id,
        "categoryState": category_state,
        "statusState": status_state,
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

pub(crate) async fn api_requirement_new_session(
    State(state): State<AppState>,
    form: FormOrJson<NewSessionForm>,
) -> ApiResult<Json<Value>> {
    let body = form.0;
    let req = get_real_requirement(&state, &body.req_id).await?;
    let session_id = Uuid::new_v4().to_string();
    associate_session(&state, &body.req_id, &session_id).await?;
    let ctx_path = write_injection_context(&state, &req, &session_id).await?;
    let title = shell_quote(&req.title);
    let ctx = shell_quote(ctx_path.to_string_lossy().as_ref());
    let pi_command = format!(
        "pi --session-id {} --name {} --append-system-prompt @{}",
        session_id, title, ctx
    );
    let project_root = requirement_project_root(&req).map(|p| p.to_string_lossy().to_string());
    let command = if let Some(root) = &project_root {
        format!("cd {} && {}", shell_quote(root), pi_command)
    } else {
        pi_command
    };
    Ok(Json(
        json!({ "ok": true, "sessionId": session_id, "command": command, "contextPath": ctx_path, "cwd": project_root }),
    ))
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
    let req_dir = PathBuf::from(req.req_dir.unwrap_or_default());
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
    Ok(Json(
        json!({ "ok": true, "branchScope": branch_scope, "review": review }),
    ))
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
    let sessions = scan_pi_sessions(&state, query.days).await?;
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
