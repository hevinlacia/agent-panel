use super::*;

pub(crate) async fn write_requirement_doc(
    state: &AppState,
    form: RequirementDocForm,
) -> ApiResult<Value> {
    let req = get_real_requirement(state, &form.req_id).await?;
    let dir = req_dir_path(&req)?;
    ensure_requirement_dir_writable(state, &dir).await?;
    let content = form.content.replace("\r\n", "\n");
    ensure_text_size(&content, "content")?;
    let doc_file = requirement_doc_file(&form.doc_type)?;
    let mode = form.mode.as_deref().unwrap_or("replace").trim();
    if !matches!(mode, "replace" | "append") {
        return Err(ApiError::bad_request(format!("invalid mode: {mode}")));
    }
    let dry_run = form.dry_run.unwrap_or(false);
    let path = dir.join(doc_file);
    let next = if mode == "append" {
        let raw = fs::read_to_string(&path)
            .await
            .unwrap_or_else(|_| format!("# {} {}\n", req.id, doc_file));
        format!("{}\n\n{}\n", raw.trim_end(), content.trim())
    } else {
        ensure_doc_heading(&req.id, doc_file, &content)
    };
    if !dry_run {
        atomic_write_text(&path, &next).await?;
    }
    Ok(json!({
        "ok": true,
        "dryRun": dry_run,
        "reqId": req.id,
        "docType": form.doc_type,
        "mode": mode,
        "file": path.to_string_lossy(),
        "bytes": next.len(),
    }))
}

pub(crate) async fn apply_requirement_edit(
    state: &AppState,
    form: RequirementEditForm,
) -> ApiResult<Value> {
    let op = form.operation.trim();
    match op {
        "setStatus" | "status" => {
            let status = clean_required_opt(form.status.as_deref(), "status")?;
            let status = canonical_status(&status)?;
            update_requirement(
                state,
                RequirementPatchForm {
                    req_id: form.req_id,
                    title: None,
                    project: None,
                    projects: None,
                    status: Some(status),
                    category: None,
                    source: None,
                    owner: None,
                    start_date: None,
                    plan_release: None,
                    ones: None,
                    issues: None,
                    note: form.note,
                    dry_run: form.dry_run,
                },
            )
            .await
        }
        "setCategory" | "category" => {
            let category = clean_required_opt(form.category.as_deref(), "category")?;
            update_requirement(
                state,
                RequirementPatchForm {
                    req_id: form.req_id,
                    title: None,
                    project: None,
                    projects: None,
                    status: None,
                    category: Some(category),
                    source: None,
                    owner: None,
                    start_date: None,
                    plan_release: None,
                    ones: None,
                    issues: None,
                    note: None,
                    dry_run: form.dry_run,
                },
            )
            .await
        }
        "patchMeta" | "meta" => {
            let fields = form.fields.unwrap_or_default();
            let patch = RequirementPatchForm {
                req_id: form.req_id,
                title: field_value(&fields, &["title"]),
                project: field_value(&fields, &["project"]),
                projects: None,
                status: None,
                category: None,
                source: field_value(&fields, &["source"]),
                owner: field_value(&fields, &["owner"]),
                start_date: field_value(&fields, &["startDate", "start-date"]),
                plan_release: field_value(&fields, &["planRelease", "plan-release"]),
                ones: field_value(&fields, &["ones"]),
                issues: None,
                note: None,
                dry_run: form.dry_run,
            };
            if patch.title.is_none()
                && patch.project.is_none()
                && patch.owner.is_none()
                && patch.start_date.is_none()
                && patch.plan_release.is_none()
                && patch.ones.is_none()
            {
                return Err(ApiError::bad_request("patchMeta has no supported fields"));
            }
            update_requirement(state, patch).await
        }
        "appendNote" | "appendNotes" | "note" => {
            let text = form.text.or(form.content).unwrap_or_default();
            append_requirement_note(
                state,
                RequirementNoteForm {
                    req_id: form.req_id,
                    text,
                    title: form.title,
                    session_id: form.session_id,
                    dry_run: form.dry_run,
                },
            )
            .await
        }
        "writeDoc" | "replaceDoc" | "appendDoc" | "doc" => {
            let doc_type = resolve_doc_type(form.doc_type.as_deref(), form.token.as_deref())?;
            let mode = if op == "appendDoc" {
                Some("append".to_string())
            } else if op == "replaceDoc" {
                Some("replace".to_string())
            } else {
                form.mode
            };
            write_requirement_doc(
                state,
                RequirementDocForm {
                    req_id: form.req_id,
                    doc_type,
                    content: form.content.or(form.text).unwrap_or_default(),
                    mode,
                    dry_run: form.dry_run,
                },
            )
            .await
        }
        "upsertSection" | "section" => upsert_requirement_section(state, form).await,
        other => Err(ApiError::bad_request(format!(
            "unsupported requirement edit operation: {other}"
        ))),
    }
}

pub(crate) fn field_value(fields: &HashMap<String, String>, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| fields.get(*key))
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

pub(crate) fn resolve_doc_type(doc_type: Option<&str>, token: Option<&str>) -> ApiResult<String> {
    if let Some(doc_type) = clean_optional(doc_type) {
        requirement_doc_file(&doc_type)?;
        return Ok(doc_type);
    }
    let token = token.ok_or_else(|| ApiError::bad_request("missing token or docType"))?;
    requirement_doc_type_for_token(token)
        .map(str::to_string)
        .ok_or_else(|| {
            ApiError::bad_request(format!("token is not a writable markdown doc: {token}"))
        })
}

pub(crate) async fn upsert_requirement_section(
    state: &AppState,
    form: RequirementEditForm,
) -> ApiResult<Value> {
    let req = get_real_requirement(state, &form.req_id).await?;
    let dir = req_dir_path(&req)?;
    ensure_requirement_dir_writable(state, &dir).await?;
    let doc_type = resolve_doc_type(form.doc_type.as_deref(), form.token.as_deref())?;
    let doc_file = requirement_doc_file(&doc_type)?;
    let heading = clean_required_opt(form.heading.as_deref(), "heading")?;
    let merged_content = form.content.or(form.text);
    let content = clean_required_opt(merged_content.as_deref(), "content")?;
    ensure_text_size(&content, "content")?;
    let dry_run = form.dry_run.unwrap_or(false);
    let path = dir.join(doc_file);
    let raw = fs::read_to_string(&path)
        .await
        .unwrap_or_else(|_| format!("# {} {}\n", req.id, doc_file));
    let next = upsert_markdown_section(&raw, &heading, &content);
    if !dry_run {
        atomic_write_text(&path, &next).await?;
    }
    let validation = if dry_run {
        json!({ "ok": true, "dryRun": true, "problems": [], "warnings": [] })
    } else {
        let refreshed = get_real_requirement(state, &req.id).await?;
        validate_requirement(state, &refreshed).await?
    };
    Ok(json!({
        "ok": true,
        "dryRun": dry_run,
        "reqId": req.id,
        "operation": "upsertSection",
        "docType": doc_type,
        "heading": heading,
        "file": path.to_string_lossy(),
        "bytes": next.len(),
        "validation": validation,
    }))
}
