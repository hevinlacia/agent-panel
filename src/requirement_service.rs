use std::{
    collections::{HashMap, HashSet},
    env,
    path::{Path, PathBuf},
};

use anyhow::{anyhow, Result};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::fs;
use uuid::Uuid;

use crate::*;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StatusForm {
    pub(crate) req_id: String,
    pub(crate) status: String,
    pub(crate) note: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CategoryForm {
    pub(crate) req_id: String,
    pub(crate) category: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ConvertIssueForm {
    pub(crate) req_id: String,
    #[serde(default)]
    pub(crate) note: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct OnesForm {
    pub(crate) req_id: String,
    pub(crate) ones: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AssociateForm {
    pub(crate) req_id: String,
    pub(crate) session_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct NewSessionForm {
    pub(crate) req_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RequirementCreateForm {
    pub(crate) req_id: String,
    pub(crate) title: String,
    #[serde(default)]
    pub(crate) project: Option<String>,
    #[serde(default)]
    pub(crate) projects: Option<Vec<String>>,
    #[serde(default)]
    pub(crate) group_path: Option<Vec<String>>,
    #[serde(default)]
    pub(crate) parent_req_id: Option<String>,
    #[serde(default)]
    pub(crate) root: Option<String>,
    #[serde(default)]
    pub(crate) status: Option<String>,
    #[serde(default)]
    pub(crate) category: Option<String>,
    #[serde(default)]
    pub(crate) owner: Option<String>,
    #[serde(default)]
    pub(crate) start_date: Option<String>,
    #[serde(default)]
    pub(crate) plan_release: Option<String>,
    #[serde(default)]
    pub(crate) ones: Option<String>,
    #[serde(default)]
    pub(crate) summary: Option<String>,
    #[serde(default)]
    pub(crate) background: Option<String>,
    #[serde(default)]
    pub(crate) notes: Option<String>,
    #[serde(default)]
    pub(crate) dry_run: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RequirementPatchForm {
    pub(crate) req_id: String,
    #[serde(default)]
    pub(crate) title: Option<String>,
    #[serde(default)]
    pub(crate) project: Option<String>,
    #[serde(default)]
    pub(crate) projects: Option<Vec<String>>,
    #[serde(default)]
    pub(crate) status: Option<String>,
    #[serde(default)]
    pub(crate) category: Option<String>,
    #[serde(default)]
    pub(crate) owner: Option<String>,
    #[serde(default)]
    pub(crate) start_date: Option<String>,
    #[serde(default)]
    pub(crate) plan_release: Option<String>,
    #[serde(default)]
    pub(crate) ones: Option<String>,
    #[serde(default)]
    pub(crate) note: Option<String>,
    #[serde(default)]
    pub(crate) dry_run: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RequirementNoteForm {
    pub(crate) req_id: String,
    pub(crate) text: String,
    #[serde(default)]
    pub(crate) title: Option<String>,
    #[serde(default)]
    pub(crate) session_id: Option<String>,
    #[serde(default)]
    pub(crate) dry_run: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RequirementDocForm {
    pub(crate) req_id: String,
    pub(crate) doc_type: String,
    pub(crate) content: String,
    #[serde(default)]
    pub(crate) mode: Option<String>,
    #[serde(default)]
    pub(crate) dry_run: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RequirementValidateForm {
    pub(crate) req_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RequirementEditForm {
    pub(crate) req_id: String,
    pub(crate) operation: String,
    #[serde(default)]
    pub(crate) token: Option<String>,
    #[serde(default)]
    pub(crate) doc_type: Option<String>,
    #[serde(default)]
    pub(crate) content: Option<String>,
    #[serde(default)]
    pub(crate) text: Option<String>,
    #[serde(default)]
    pub(crate) title: Option<String>,
    #[serde(default)]
    pub(crate) heading: Option<String>,
    #[serde(default)]
    pub(crate) mode: Option<String>,
    #[serde(default)]
    pub(crate) status: Option<String>,
    #[serde(default)]
    pub(crate) category: Option<String>,
    #[serde(default)]
    pub(crate) note: Option<String>,
    #[serde(default)]
    pub(crate) session_id: Option<String>,
    #[serde(default)]
    pub(crate) fields: Option<HashMap<String, String>>,
    #[serde(default)]
    pub(crate) dry_run: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RequirementSectionForm {
    pub(crate) req_id: String,
    pub(crate) content: String,
    #[serde(default)]
    pub(crate) token: Option<String>,
    #[serde(default)]
    pub(crate) doc_type: Option<String>,
    #[serde(default)]
    pub(crate) heading: Option<String>,
    #[serde(default)]
    pub(crate) dry_run: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RequirementEventForm {
    pub(crate) req_id: String,
    #[serde(default, alias = "type")]
    pub(crate) event_type: Option<String>,
    #[serde(default)]
    pub(crate) title: Option<String>,
    #[serde(default)]
    pub(crate) summary: Option<String>,
    #[serde(default)]
    pub(crate) details: Option<String>,
    #[serde(default)]
    pub(crate) evidence: Vec<String>,
    #[serde(default)]
    pub(crate) decisions: Vec<String>,
    #[serde(default)]
    pub(crate) todos: Vec<String>,
    #[serde(default)]
    pub(crate) related_files: Vec<String>,
    #[serde(default)]
    pub(crate) related_knowledge_ids: Vec<String>,
    #[serde(default)]
    pub(crate) trigger_terms: Vec<String>,
    #[serde(default)]
    pub(crate) related_repos: Vec<String>,
    #[serde(default)]
    pub(crate) related_tables: Vec<String>,
    #[serde(default)]
    pub(crate) related_apis: Vec<String>,
    #[serde(default)]
    pub(crate) candidate_type: Option<String>,
    #[serde(default)]
    pub(crate) dedupe_key: Option<String>,
    #[serde(default)]
    pub(crate) confidence: Option<String>,
    #[serde(default)]
    pub(crate) target: Option<String>,
    #[serde(default)]
    pub(crate) test_cases: Vec<RequirementEventTestCase>,
    #[serde(default)]
    pub(crate) status: Option<String>,
    #[serde(default)]
    pub(crate) risk_level: Option<String>,
    #[serde(default)]
    pub(crate) tags: Vec<String>,
    #[serde(default)]
    pub(crate) session_id: Option<String>,
    #[serde(default)]
    pub(crate) idempotency_key: Option<String>,
    #[serde(default)]
    pub(crate) append_note: Option<bool>,
    #[serde(default)]
    pub(crate) dry_run: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RequirementEventTestCase {
    #[serde(default)]
    pub(crate) name: String,
    #[serde(default)]
    pub(crate) result: String,
    #[serde(default)]
    pub(crate) evidence: Option<String>,
}

pub(crate) async fn create_requirement(
    state: &AppState,
    form: RequirementCreateForm,
) -> ApiResult<Value> {
    let title = clean_required(&form.title, "title")?;
    let category = form
        .category
        .as_deref()
        .unwrap_or("需求")
        .trim()
        .to_string();
    ensure_category(&category)?;
    let mut status = form
        .status
        .as_deref()
        .and_then(normalize_status_value)
        .unwrap_or_else(|| {
            if category == "线上问题" {
                "排查中".to_string()
            } else {
                "需求澄清".to_string()
            }
        });
    if category == "线上问题" && form.status.is_none() {
        status = "排查中".to_string();
    }
    ensure_status(&status)?;
    let dry_run = form.dry_run.unwrap_or(false);
    let base = resolve_create_req_root(state, form.root.as_deref()).await?;
    let (req_id, target_dir) =
        resolve_req_id_and_target_dir(state, &base, form.req_id.trim(), &form, dry_run).await?;

    let projects = normalize_projects(form.project.as_deref(), form.projects.as_deref());
    let project = projects
        .first()
        .cloned()
        .unwrap_or_else(|| DEFAULT_PROJECT_NAME.to_string());
    let owner = clean_optional(form.owner.as_deref()).unwrap_or_else(|| "unknown".to_string());
    let start_date = clean_optional(form.start_date.as_deref()).unwrap_or_else(today_ymd);
    ensure_date_or_unknown(&start_date, "startDate")?;
    let plan_release =
        clean_optional(form.plan_release.as_deref()).unwrap_or_else(|| "unknown".to_string());
    ensure_date_or_unknown(&plan_release, "planRelease")?;
    let ones = clean_optional(form.ones.as_deref()).unwrap_or_default();
    let summary = clean_optional(form.summary.as_deref()).unwrap_or_else(|| "待补充".to_string());
    let background = clean_optional(form.background.as_deref());
    let notes = clean_optional(form.notes.as_deref());

    let files = requirement_create_files(
        &req_id,
        &title,
        &status,
        &project,
        &projects,
        &category,
        &owner,
        &start_date,
        &plan_release,
        &ones,
        &summary,
        background.as_deref(),
        notes.as_deref(),
    );
    let planned: Vec<String> = files
        .iter()
        .map(|(name, _)| target_dir.join(name).to_string_lossy().to_string())
        .collect();
    if !dry_run {
        fs::create_dir_all(&target_dir).await?;
        for (name, body) in &files {
            atomic_write_text(&target_dir.join(name), body).await?;
        }
    }
    let validation = if dry_run {
        json!({ "ok": true, "dryRun": true, "problems": [], "warnings": [] })
    } else {
        let req = load_requirement_from_dir(&target_dir, &req_id, &[project.clone()], &[])
            .await?
            .ok_or_else(|| anyhow!("created requirement cannot be loaded"))?;
        validate_requirement(state, &req).await?
    };
    Ok(json!({
        "ok": true,
        "dryRun": dry_run,
        "reqId": req_id,
        "title": title,
        "status": status,
        "category": category,
        "project": project,
        "projects": projects,
        "reqDir": target_dir.to_string_lossy(),
        "files": planned,
        "validation": validation,
    }))
}

pub(crate) async fn update_requirement(
    state: &AppState,
    form: RequirementPatchForm,
) -> ApiResult<Value> {
    let req = get_real_requirement(state, &form.req_id).await?;
    let dir = req_dir_path(&req)?;
    ensure_requirement_dir_writable(state, &dir).await?;
    let dry_run = form.dry_run.unwrap_or(false);
    let mut changes = Vec::<String>::new();
    let mut planned_files = Vec::<String>::new();

    let meta_path = dir.join("meta.md");
    let mut meta_next = fs::read_to_string(&meta_path)
        .await
        .unwrap_or_default()
        .replace("\r\n", "\n");
    if let Some(title) = clean_optional(form.title.as_deref()) {
        meta_next = set_frontmatter_field(&meta_next, "title", &title);
        meta_next = update_meta_summary_line(&meta_next, "Title", &title);
        changes.push("meta.title".into());
    }
    if let Some(project) = clean_optional(form.project.as_deref()) {
        meta_next = set_frontmatter_field(&meta_next, "project", &project);
        changes.push("meta.project".into());
    }
    if let Some(projects) = form.projects.as_deref() {
        let value =
            unique_strings(projects.iter().map(|s| s.trim().to_string()).collect()).join(", ");
        meta_next = set_frontmatter_field(&meta_next, "projects", &value);
        changes.push("meta.projects".into());
    }
    if let Some(owner) = clean_optional(form.owner.as_deref()) {
        meta_next = set_frontmatter_field(&meta_next, "owner", &owner);
        meta_next = update_meta_summary_line(&meta_next, "Owner", &owner);
        changes.push("meta.owner".into());
    }
    if let Some(start_date) = clean_optional(form.start_date.as_deref()) {
        ensure_date_or_unknown(&start_date, "startDate")?;
        meta_next = set_frontmatter_field(&meta_next, "start-date", &start_date);
        meta_next = update_meta_summary_line(&meta_next, "Start date", &start_date);
        changes.push("meta.startDate".into());
    }
    if let Some(plan_release) = clean_optional(form.plan_release.as_deref()) {
        ensure_date_or_unknown(&plan_release, "planRelease")?;
        meta_next = set_frontmatter_field(&meta_next, "plan-release", &plan_release);
        meta_next = update_meta_summary_line(&meta_next, "Planned release", &plan_release);
        changes.push("meta.planRelease".into());
    }
    if let Some(ones) = form.ones.as_deref() {
        let value = ones.trim().to_string();
        meta_next = set_frontmatter_field(&meta_next, "ones", &value);
        changes.push("meta.ones".into());
    }
    if let Some(category) = form.category.as_deref() {
        ensure_category(category)?;
        changes.push("state.category".into());
        if !dry_run {
            write_requirement_category(dir.to_string_lossy().as_ref(), category).await?;
        }
        planned_files.push(dir.join(STATE_FILE).to_string_lossy().to_string());
    }
    if let Some(status) = form.status.as_deref() {
        let status = canonical_status(status)?;
        if should_enforce_review_gate_for_status(&req.status, &status) {
            ensure_review_gate_allows_testing(&req).await?;
        }
        changes.push("state.status".into());
        if !dry_run {
            let st = write_requirement_status(
                dir.to_string_lossy().as_ref(),
                &status,
                form.note.as_deref(),
            )
            .await?;
            if !matches!(st.get("changed").and_then(Value::as_bool), Some(false)) {
                record_status_transition_event(state, &req, &st, form.note.as_deref()).await?;
                planned_files.push(
                    dir.join(REQUIREMENT_EVENTS_FILE)
                        .to_string_lossy()
                        .to_string(),
                );
            }
        }
        planned_files.push(dir.join(STATE_FILE).to_string_lossy().to_string());
    }
    let old_meta = fs::read_to_string(&meta_path)
        .await
        .unwrap_or_default()
        .replace("\r\n", "\n");
    if meta_next != old_meta {
        planned_files.push(meta_path.to_string_lossy().to_string());
        if !dry_run {
            atomic_write_text(&meta_path, &meta_next).await?;
        }
    }

    let validation = if dry_run {
        json!({ "ok": true, "dryRun": true, "problems": [], "warnings": [] })
    } else {
        let refreshed = get_real_requirement(state, &form.req_id).await?;
        validate_requirement(state, &refreshed).await?
    };
    Ok(json!({
        "ok": true,
        "dryRun": dry_run,
        "reqId": req.id,
        "changes": unique_strings(changes),
        "files": unique_strings(planned_files),
        "validation": validation,
    }))
}

pub(crate) async fn append_requirement_note(
    state: &AppState,
    form: RequirementNoteForm,
) -> ApiResult<Value> {
    let req = get_real_requirement(state, &form.req_id).await?;
    let dir = req_dir_path(&req)?;
    ensure_requirement_dir_writable(state, &dir).await?;
    let text = clean_required(&form.text, "text")?;
    ensure_text_size(&text, "text")?;
    let dry_run = form.dry_run.unwrap_or(false);
    let path = dir.join("notes.md");
    let raw = fs::read_to_string(&path)
        .await
        .unwrap_or_else(|_| format!("# {} Notes\n", req.id));
    let title = clean_optional(form.title.as_deref()).unwrap_or_else(|| "Agent Update".to_string());
    let session = clean_optional(form.session_id.as_deref()).unwrap_or_default();
    let block = format!(
        "\n\n## {} - {}\n{}{}\n",
        today_ymd(),
        title,
        if session.is_empty() {
            String::new()
        } else {
            format!("- Session: `{}`\n", session)
        },
        text.trim()
    );
    let next = format!("{}{}", raw.trim_end(), block);
    if !dry_run {
        atomic_write_text(&path, &next).await?;
    }
    Ok(json!({
        "ok": true,
        "dryRun": dry_run,
        "reqId": req.id,
        "file": path.to_string_lossy(),
        "appendedBytes": block.len(),
    }))
}

pub(crate) async fn record_requirement_event(
    state: &AppState,
    form: RequirementEventForm,
) -> ApiResult<Value> {
    let req = get_real_requirement(state, &form.req_id).await?;
    let dir = req_dir_path(&req)?;
    ensure_requirement_dir_writable(state, &dir).await?;
    let event_type = normalize_requirement_event_type(form.event_type.as_deref());
    let title = clean_optional(form.title.as_deref())
        .or_else(|| clean_optional(form.summary.as_deref()))
        .unwrap_or_else(|| requirement_event_label(&event_type).to_string());
    let summary = clean_optional(form.summary.as_deref()).unwrap_or_else(|| title.clone());
    ensure_text_size(&summary, "summary")?;
    let details = clean_optional(form.details.as_deref()).unwrap_or_default();
    ensure_text_size(&details, "details")?;
    let dry_run = form.dry_run.unwrap_or(false);
    let append_note = form.append_note.unwrap_or(true);
    let event_id = clean_optional(form.idempotency_key.as_deref())
        .unwrap_or_else(|| format!("{}-{}", now_ms(), Uuid::new_v4()));
    let event = json!({
        "id": event_id,
        "reqId": req.id,
        "type": event_type,
        "title": title,
        "summary": summary,
        "details": details,
        "evidence": clean_string_vec(form.evidence),
        "decisions": clean_string_vec(form.decisions),
        "todos": clean_string_vec(form.todos),
        "relatedFiles": clean_string_vec(form.related_files),
        "relatedKnowledgeIds": clean_string_vec(form.related_knowledge_ids),
        "triggerTerms": clean_string_vec(form.trigger_terms),
        "relatedRepos": clean_string_vec(form.related_repos),
        "relatedTables": clean_string_vec(form.related_tables),
        "relatedApis": clean_string_vec(form.related_apis),
        "candidateType": clean_optional(form.candidate_type.as_deref()),
        "dedupeKey": clean_optional(form.dedupe_key.as_deref()),
        "confidence": clean_optional(form.confidence.as_deref()),
        "target": clean_optional(form.target.as_deref()),
        "testCases": form.test_cases,
        "status": clean_optional(form.status.as_deref()),
        "riskLevel": clean_optional(form.risk_level.as_deref()),
        "tags": clean_string_vec(form.tags),
        "sessionId": clean_optional(form.session_id.as_deref()),
        "createdAt": now_ms(),
    });
    let events_path = dir.join(REQUIREMENT_EVENTS_FILE);
    let note_text = render_requirement_event_note(&event);
    let mut files = vec![events_path.to_string_lossy().to_string()];
    if append_note {
        files.push(dir.join("notes.md").to_string_lossy().to_string());
    }
    if !dry_run {
        let existing = fs::read_to_string(&events_path).await.unwrap_or_default();
        let event_already_exists = requirement_event_exists(
            &existing,
            event.get("id").and_then(Value::as_str).unwrap_or_default(),
        );
        if !event_already_exists {
            let line = serde_json::to_string(&event)?;
            let next = if existing.trim().is_empty() {
                format!("{}\n", line)
            } else {
                format!("{}\n{}\n", existing.trim_end(), line)
            };
            atomic_write_text(&events_path, &next).await?;
            if append_note {
                append_requirement_note(
                    state,
                    RequirementNoteForm {
                        req_id: req.id.clone(),
                        text: note_text.clone(),
                        title: Some(format!("事件：{}", title)),
                        session_id: clean_optional(form.session_id.as_deref()),
                        dry_run: Some(false),
                    },
                )
                .await?;
            }
        }
    }
    Ok(json!({
        "ok": true,
        "dryRun": dry_run,
        "reqId": req.id,
        "operation": "recordEvent",
        "event": event,
        "notePreview": note_text,
        "files": unique_strings(files),
    }))
}

pub(crate) fn requirement_section_form_to_edit(
    section: String,
    form: RequirementSectionForm,
) -> ApiResult<RequirementEditForm> {
    let doc_type = form
        .doc_type
        .or_else(|| {
            form.token
                .as_deref()
                .and_then(requirement_doc_type_for_token)
                .map(str::to_string)
        })
        .or_else(|| requirement_section_default_doc_type(&section).map(str::to_string));
    let heading = form
        .heading
        .or_else(|| Some(requirement_section_default_heading(&section).to_string()));
    Ok(RequirementEditForm {
        req_id: form.req_id,
        operation: "upsertSection".to_string(),
        token: None,
        doc_type,
        content: Some(form.content),
        text: None,
        title: None,
        heading,
        mode: None,
        status: None,
        category: None,
        note: None,
        session_id: None,
        fields: None,
        dry_run: form.dry_run,
    })
}

pub(crate) fn requirement_section_default_doc_type(section: &str) -> Option<&'static str> {
    let s = normalize_section_key(section);
    if matches!(
        s.as_str(),
        "test" | "tests" | "selftest" | "uat" | "testcase" | "testcases"
    ) {
        Some("test")
    } else if matches!(
        s.as_str(),
        "impact" | "risk" | "risks" | "rootcause" | "boxcodeissue" | "issue"
    ) {
        Some("impact")
    } else if matches!(
        s.as_str(),
        "background" | "design" | "scope" | "decision" | "decisions"
    ) {
        Some("background")
    } else if matches!(s.as_str(), "memory" | "summary" | "agentcontext") {
        Some("memory")
    } else if matches!(s.as_str(), "config" | "configchanges") {
        Some("config-changes")
    } else if matches!(
        s.as_str(),
        "technicalplan" | "techplan" | "implementation" | "implementationplan" | "solution"
    ) {
        Some("technical-plan")
    } else if matches!(s.as_str(), "release" | "manifest" | "releasemanifest") {
        Some("release-manifest")
    } else if matches!(s.as_str(), "review" | "codereview") {
        Some("review")
    } else if matches!(s.as_str(), "notes" | "note" | "progress") {
        Some("notes")
    } else {
        None
    }
}

pub(crate) fn requirement_section_default_heading(section: &str) -> &str {
    let s = normalize_section_key(section);
    match s.as_str() {
        "boxcodeissue" => "boxCode 问题",
        "rootcause" => "根因分析",
        "test" | "tests" | "testcase" | "testcases" => "测试场景",
        "selftest" => "自测证据",
        "uat" => "UAT 验证",
        "impact" => "影响面评估",
        "risk" | "risks" => "风险与回滚",
        "decision" | "decisions" => "关键决策",
        "summary" | "agentcontext" => "Agent 摘要",
        "config" | "configchanges" => "配置变更",
        "technicalplan" | "techplan" | "implementation" | "implementationplan" | "solution" => {
            "技术方案"
        }
        "release" | "manifest" | "releasemanifest" => "上线清单",
        "review" | "codereview" => "代码审查结论",
        "progress" => "进展记录",
        _ => section.trim(),
    }
}

pub(crate) fn normalize_section_key(value: &str) -> String {
    value
        .trim()
        .trim_start_matches("req.")
        .trim_end_matches(".md")
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect::<String>()
        .to_lowercase()
}

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
                    owner: None,
                    start_date: None,
                    plan_release: None,
                    ones: None,
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
                    owner: None,
                    start_date: None,
                    plan_release: None,
                    ones: None,
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
                owner: field_value(&fields, &["owner"]),
                start_date: field_value(&fields, &["startDate", "start-date"]),
                plan_release: field_value(&fields, &["planRelease", "plan-release"]),
                ones: field_value(&fields, &["ones"]),
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

pub(crate) async fn validate_requirement(state: &AppState, req: &Requirement) -> ApiResult<Value> {
    let dir = req_dir_path(req)?;
    ensure_requirement_dir_writable(state, &dir).await?;
    let mut problems = Vec::<String>::new();
    let mut warnings = Vec::<String>::new();
    let mut files = HashMap::<String, bool>::new();
    let required_files = [
        "meta.md",
        STATE_FILE,
        "background.md",
        "technical-plan.md",
        "notes.md",
    ];
    let optional_files = [
        BRANCH_SCOPE_FILE,
        "test.md",
        "release-manifest.md",
        "review.md",
        "code-review-ai.md",
        CODE_REVIEW_FILE,
        "release-check.md",
        "experience-summary.md",
        "prd.md",
        // Legacy compatibility: readable when present, no longer required for new requirements.
        "alignment.md",
        "impact.md",
        "memory.md",
        "branch.md",
        "config-changes.md",
    ];
    for file in required_files.into_iter().chain(optional_files.into_iter()) {
        let exists = dir.join(file).is_file();
        files.insert(file.to_string(), exists);
        if file == "meta.md" && !exists {
            problems.push("missing meta.md".into());
        } else if required_files.contains(&file) && !exists {
            warnings.push(format!("missing core file {file}"));
        }
    }
    let meta_path = dir.join("meta.md");
    let raw = fs::read_to_string(&meta_path).await.unwrap_or_default();
    let fm = parse_frontmatter(&raw);
    match fm.fields.get("req-id") {
        Some(id) if id == &req.id => {}
        Some(id) => problems.push(format!("meta req-id mismatch: {id} != {}", req.id)),
        None => problems.push("meta missing req-id".into()),
    }
    if fm
        .fields
        .get("title")
        .map(|v| v.trim().is_empty())
        .unwrap_or(true)
    {
        problems.push("meta missing title".into());
    }
    if let Some(status) = fm.fields.get("status") {
        if normalize_status(Some(status)).is_none() {
            problems.push(format!("invalid meta status: {status}"));
        }
    } else if req.status.trim().is_empty() {
        problems.push("missing status".into());
    }
    if let Some(category) = fm.fields.get("category") {
        if normalize_category(Some(category)).is_none() {
            problems.push(format!("invalid meta category: {category}"));
        }
    }
    if let Some(state_json) = read_requirement_state(&dir).await? {
        if let Some(status) = state_json.get("status").and_then(Value::as_str) {
            if normalize_status(Some(&status.to_string())).is_none() {
                problems.push(format!("invalid state status: {status}"));
            }
        }
        if let Some(category) = state_json.get("category").and_then(Value::as_str) {
            if normalize_category(Some(&category.to_string())).is_none() {
                problems.push(format!("invalid state category: {category}"));
            }
        }
    }
    let branches_path = dir.join(BRANCH_SCOPE_FILE);
    if branches_path.is_file() && read_branch_scope(&dir).await?.is_none() {
        warnings.push("branches.json exists but has no valid repos".into());
    }
    Ok(json!({
        "ok": problems.is_empty(),
        "reqId": req.id,
        "reqDir": dir.to_string_lossy(),
        "problems": problems,
        "warnings": warnings,
        "files": files,
    }))
}

pub(crate) fn req_dir_path(req: &Requirement) -> ApiResult<PathBuf> {
    req.req_dir
        .as_deref()
        .filter(|v| !v.trim().is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| {
            ApiError::bad_request(format!("requirement has no writable dir: {}", req.id))
        })
}

pub(crate) async fn resolve_create_req_root(
    state: &AppState,
    requested: Option<&str>,
) -> ApiResult<PathBuf> {
    let roots = writable_req_roots(state).await?;
    if roots.is_empty() {
        return Err(ApiError::bad_request(
            "no requirementScanRoots configured; set /settings requirement scan roots first",
        ));
    }
    if let Some(raw) = clean_optional(requested) {
        let requested_path = normalize_user_path(&raw);
        for root in &roots {
            if same_or_child_path(&requested_path, root) {
                return Ok(root.clone());
            }
            if let Some(parent) = root.parent().and_then(|p| p.parent()) {
                if path_eq(&requested_path, parent) {
                    return Ok(root.clone());
                }
            }
        }
        return Err(ApiError::bad_request(format!(
            "root is not under configured requirementScanRoots: {raw}"
        )));
    }
    Ok(roots[0].clone())
}

pub(crate) async fn writable_req_roots(state: &AppState) -> ApiResult<Vec<PathBuf>> {
    let cfg = read_config(state).await?;
    let roots = normalize_scan_roots(cfg.requirement_scan_roots);
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for root in roots {
        let root_path = PathBuf::from(root);
        let mut candidates = Vec::new();
        if root_path.file_name().and_then(|v| v.to_str()) == Some("req") {
            candidates.push(root_path.clone());
        } else {
            candidates.push(root_path.join(".agents/req"));
            candidates.push(root_path.join("req"));
        }
        for candidate in candidates {
            let key = normalize_path_string(&candidate);
            if seen.insert(key) {
                out.push(candidate);
            }
        }
    }
    Ok(out)
}

pub(crate) async fn ensure_requirement_dir_writable(state: &AppState, dir: &Path) -> ApiResult<()> {
    ensure_path_inside_req_roots(state, dir).await
}

pub(crate) async fn ensure_path_inside_req_roots(state: &AppState, path: &Path) -> ApiResult<()> {
    let roots = writable_req_roots(state).await?;
    if roots.iter().any(|root| same_or_child_path(path, root)) {
        Ok(())
    } else {
        Err(ApiError::bad_request(format!(
            "path is outside configured requirement roots: {}",
            path.to_string_lossy()
        )))
    }
}

pub(crate) fn same_or_child_path(path: &Path, root: &Path) -> bool {
    normalize_path_string(path) == normalize_path_string(root)
        || normalize_path_string(path).starts_with(&(normalize_path_string(root) + "/"))
}

pub(crate) fn path_eq(a: &Path, b: &Path) -> bool {
    normalize_path_string(a) == normalize_path_string(b)
}

pub(crate) fn normalize_path_string(path: &Path) -> String {
    let abs = if path.is_absolute() {
        path.to_path_buf()
    } else {
        env::current_dir().unwrap_or_default().join(path)
    };
    let mut parts = Vec::new();
    for comp in abs.components() {
        match comp {
            std::path::Component::ParentDir => {
                parts.pop();
            }
            std::path::Component::CurDir => {}
            _ => parts.push(comp.as_os_str().to_string_lossy().to_string()),
        }
    }
    if parts.is_empty() {
        "/".into()
    } else if parts.first().map(|s| s.as_str()) == Some("/") {
        format!("/{}", parts[1..].join("/"))
    } else {
        parts.join("/")
    }
}

pub(crate) fn normalize_user_path(raw: &str) -> PathBuf {
    let trimmed = raw.trim();
    if trimmed == "~" {
        home_dir().unwrap_or_default()
    } else if let Some(rest) = trimmed.strip_prefix("~/") {
        home_dir().unwrap_or_default().join(rest)
    } else {
        let path = PathBuf::from(trimmed);
        if path.is_absolute() {
            path
        } else {
            env::current_dir().unwrap_or_default().join(path)
        }
    }
}

pub(crate) fn ensure_req_id(value: &str) -> ApiResult<String> {
    let v = value.trim();
    if v == DEFAULT_REQ_ID || v.len() < 2 || v.len() > 128 {
        return Err(ApiError::bad_request("invalid reqId length"));
    }
    if !v.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        || v.starts_with('-')
        || v.ends_with('-')
        || v.contains("--")
    {
        return Err(ApiError::bad_request(
            "reqId must use ASCII letters, numbers and single hyphens only",
        ));
    }
    Ok(v.to_string())
}

/// Parse a reqId template containing a single `{seq}` placeholder into
/// `(prefix, suffix)`. `WMS-{seq}-demo` -> `("WMS", "-demo")`.
pub(crate) fn split_seq_template(template: &str) -> ApiResult<(String, String)> {
    let count = template.matches("{seq}").count();
    if count == 0 {
        return Err(ApiError::bad_request("reqId template must contain {seq}"));
    }
    if count > 1 {
        return Err(ApiError::bad_request(
            "reqId template may contain at most one {seq}",
        ));
    }
    let idx = template.find("{seq}").expect("checked count above");
    let prefix = template[..idx].trim_end_matches('-').to_string();
    let suffix = template[idx + 5..].to_string();
    if prefix.is_empty() {
        return Err(ApiError::bad_request(
            "reqId template must have a non-empty prefix before {seq}",
        ));
    }
    if !prefix
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-')
    {
        return Err(ApiError::bad_request(
            "reqId prefix must be ASCII alphanumeric or hyphen",
        ));
    }
    Ok((prefix, suffix))
}

/// Format the final reqId from prefix, sequence (zero-padded to 3 digits) and
/// suffix. `("WMS", 43, "-demo")` -> `WMS-043-demo`.
pub(crate) fn format_seq_id(prefix: &str, seq: u64, suffix: &str) -> String {
    format!("{prefix}-{:03}{suffix}", seq)
}

/// Pure helper: compute the next sequence number for `prefix` from a list of
/// existing requirement ids. Sub-requirements sharing a number (e.g.
/// `WMS-003-*`) and gaps are handled by taking `max + 1`. `floor` forces a
/// minimum (used when retrying after a collision).
pub(crate) fn compute_next_seq_from_ids(ids: &[String], prefix: &str, floor: Option<u64>) -> u64 {
    let re = Regex::new(&format!("^{}-(\\d+)(-|$)", regex::escape(prefix))).expect("valid regex");
    let mut max_seq: u64 = 0;
    for id in ids {
        if let Some(caps) = re.captures(id) {
            if let Ok(n) = caps[1].parse::<u64>() {
                if n > max_seq {
                    max_seq = n;
                }
            }
        }
    }
    let mut next = max_seq + 1;
    if let Some(f) = floor {
        next = next.max(f);
    }
    next
}

/// Allocate the next sequence number for `prefix` by scanning existing
/// requirements on disk.
pub(crate) async fn allocate_next_seq(
    state: &AppState,
    prefix: &str,
    floor: Option<u64>,
) -> ApiResult<u64> {
    let reqs = scan_hermes_requirements(state).await?;
    let ids: Vec<String> = reqs.iter().map(|r| r.id.clone()).collect();
    Ok(compute_next_seq_from_ids(&ids, prefix, floor))
}

/// Compute the target directory for a fully-resolved reqId, resolving parent
/// requirement or group path from the create form.
pub(crate) async fn compute_create_target_dir(
    state: &AppState,
    base: &Path,
    req_id: &str,
    form: &RequirementCreateForm,
) -> ApiResult<PathBuf> {
    if let Some(parent_id) = clean_optional(form.parent_req_id.as_deref()) {
        let parent = get_real_requirement(state, &parent_id).await?;
        let parent_dir = req_dir_path(&parent)?;
        ensure_requirement_dir_writable(state, &parent_dir).await?;
        Ok(parent_dir.join(req_id))
    } else {
        let mut dir = base.to_path_buf();
        for segment in form.group_path.as_deref().unwrap_or_default() {
            dir = dir.join(ensure_safe_segment(segment, "groupPath")?);
        }
        Ok(dir.join(req_id))
    }
}

/// Resolve the final reqId and target directory for a create request.
///
/// When `template` contains `{seq}`, the next sequence number is allocated
/// from existing requirements and (for non-dry-run) the target directory is
/// atomically reserved with `fs::create_dir`, retrying on collision. When
/// `template` has no `{seq}`, it is validated as-is and the target directory
/// is checked for prior existence.
pub(crate) async fn resolve_req_id_and_target_dir(
    state: &AppState,
    base: &Path,
    template: &str,
    form: &RequirementCreateForm,
    dry_run: bool,
) -> ApiResult<(String, PathBuf)> {
    if !template.contains("{seq}") {
        let req_id = ensure_req_id(template)?;
        let target_dir = compute_create_target_dir(state, base, &req_id, form).await?;
        ensure_path_inside_req_roots(state, &target_dir).await?;
        if target_dir.exists() {
            return Err(ApiError::bad_request(format!(
                "requirement directory already exists: {}",
                target_dir.to_string_lossy()
            )));
        }
        return Ok((req_id, target_dir));
    }

    let (prefix, suffix) = split_seq_template(template)?;
    let max_retries: u32 = 5;
    let mut floor: Option<u64> = None;
    for attempt in 0..=max_retries {
        let seq = allocate_next_seq(state, &prefix, floor).await?;
        let req_id = ensure_req_id(&format_seq_id(&prefix, seq, &suffix))?;
        let target_dir = compute_create_target_dir(state, base, &req_id, form).await?;
        ensure_path_inside_req_roots(state, &target_dir).await?;
        if dry_run {
            return Ok((req_id, target_dir));
        }
        if let Some(parent) = target_dir.parent() {
            fs::create_dir_all(parent).await?;
        }
        match fs::create_dir(&target_dir).await {
            Ok(()) => return Ok((req_id, target_dir)),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                if attempt == max_retries {
                    return Err(ApiError::bad_request(format!(
                        "requirement directory already exists after {max_retries} retries: {}",
                        target_dir.to_string_lossy()
                    )));
                }
                floor = Some(seq + 1);
                continue;
            }
            Err(e) => return Err(anyhow!("create requirement dir failed: {e}").into()),
        }
    }
    unreachable!("retry loop exhausted without returning")
}

pub(crate) fn ensure_safe_segment(value: &str, field: &str) -> ApiResult<String> {
    let v = value.trim();
    if v.is_empty() || v == "." || v == ".." || v.len() > 128 {
        return Err(ApiError::bad_request(format!("invalid {field} segment")));
    }
    if !v
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        || v.contains('/')
        || v.contains('\\')
    {
        return Err(ApiError::bad_request(format!(
            "{field} segment must be ASCII and path-safe"
        )));
    }
    Ok(v.to_string())
}

pub(crate) fn normalize_projects(
    project: Option<&str>,
    projects: Option<&[String]>,
) -> Vec<String> {
    let mut values = Vec::new();
    if let Some(project) = clean_optional(project) {
        values.push(project);
    }
    if let Some(projects) = projects {
        values.extend(
            projects
                .iter()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty()),
        );
    }
    if values.is_empty() {
        values.push(DEFAULT_PROJECT_NAME.to_string());
    }
    unique_strings(values)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn requirement_create_files(
    req_id: &str,
    title: &str,
    status: &str,
    project: &str,
    projects: &[String],
    category: &str,
    owner: &str,
    start_date: &str,
    plan_release: &str,
    ones: &str,
    summary: &str,
    background: Option<&str>,
    notes: Option<&str>,
) -> Vec<(&'static str, String)> {
    let meta = build_meta_doc(
        req_id,
        title,
        status,
        project,
        projects,
        category,
        owner,
        start_date,
        plan_release,
        ones,
        summary,
    );
    vec![
        ("meta.md", meta),
        (STATE_FILE, template_state(status, category)),
        (
            "background.md",
            background
                .map(str::to_string)
                .unwrap_or_else(|| template_background(req_id)),
        ),
        ("technical-plan.md", template_technical_plan(req_id)),
        (
            "notes.md",
            notes
                .map(str::to_string)
                .unwrap_or_else(|| template_notes(req_id)),
        ),
    ]
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn build_meta_doc(
    req_id: &str,
    title: &str,
    status: &str,
    project: &str,
    projects: &[String],
    category: &str,
    owner: &str,
    start_date: &str,
    plan_release: &str,
    ones: &str,
    summary: &str,
) -> String {
    let mut fm = vec![
        format!("req-id: {}", yaml_quote(req_id)),
        format!("title: {}", yaml_quote(title)),
        format!("status: {}", yaml_quote(status)),
        format!("project: {}", yaml_quote(project)),
    ];
    if projects.len() > 1 {
        fm.push(format!("projects: {}", yaml_quote(&projects.join(", "))));
    }
    fm.push(format!("category: {}", yaml_quote(category)));
    fm.push(format!("owner: {}", yaml_quote(owner)));
    fm.push(format!("start-date: {}", yaml_quote(start_date)));
    fm.push(format!("plan-release: {}", yaml_quote(plan_release)));
    if !ones.trim().is_empty() {
        fm.push(format!("ones: {}", yaml_quote(ones)));
    }
    format!(
        "---\n{}\n---\n\n# {} {}\n\n## Summary\n- Title: {}\n- Status: {}\n- Owner: {}\n- Start date: {}\n- Planned release: {}\n- Project: {}\n\n{}\n\n## Scope\n- Include:\n  - 待补充\n- Exclude:\n  - 待补充\n\n## Open Questions\n- 待补充\n",
        fm.join("\n"),
        req_id,
        title,
        title,
        status,
        owner,
        start_date,
        plan_release,
        projects.join(" / "),
        summary.trim()
    )
}

pub(crate) fn template_state(status: &str, category: &str) -> String {
    serde_json::to_string_pretty(&json!({
        "version": 1,
        "status": status,
        "previousStatus": Value::Null,
        "changed": true,
        "lastTransition": Value::Null,
        "category": category,
        "updatedAt": now_ms(),
        "history": [{"status": status, "from": Value::Null, "at": now_ms(), "note": "created", "skippedStatuses": []}]
    }))
    .unwrap_or_else(|_| format!("{{\n  \"version\": 1,\n  \"status\": \"{}\"\n}}\n", status))
}

pub(crate) fn template_alignment(req_id: &str) -> String {
    format!("# {req_id} 需求澄清\n\n## 业务目标\n- 待补充：这次需求要解决的业务问题和成功标准。\n\n## 场景与角色\n- 待补充：涉及的业务角色、对象、入口和主流程。\n\n## PRD 解读\n- 来源：待补充\n- 已确认：待补充\n- 不确定：待补充\n\n## 初步代码调查\n- 相关仓库/模块：待补充\n- 现有系统行为：待补充\n- 初步实现方向：待补充\n\n## 范围与非目标\n- Include：待补充\n- Exclude：待补充\n\n## 待确认问题\n- [ ] 待补充\n")
}

pub(crate) fn template_background(req_id: &str) -> String {
    format!("# {req_id} 业务背景文档\n\n> 面向不熟悉业务的开发、测试和后续经验总结使用；尽量用业务语言说明为什么做、当前怎么运转、这次改变什么。\n\n## 一句话背景\n- 待补充\n\n## 业务目标\n- 待补充\n\n## 业务对象与角色\n- 对象：待补充\n- 角色：待补充\n- 入口：待补充\n\n## 当前系统行为\n- 待补充\n\n## 本次需求改变\n- 待补充\n\n## 关键业务规则\n- 待补充\n\n## 沟通口径\n- 产品/业务确认点：待补充\n- 测试重点：待补充\n\n## 关联知识与经验\n- 业务知识：待补充\n- 历史经验：待补充\n")
}

pub(crate) fn template_memory(req_id: &str, title: &str) -> String {
    format!("# {req_id} Memory\n\n## 当前目标\n- {title}\n\n## 当前进展\n- 已创建需求，待补充进展。\n\n## 关键决策\n- 待补充\n\n## 待办 / 风险\n- [ ] 待补充\n")
}

pub(crate) fn template_branch(req_id: &str) -> String {
    format!("# {req_id} Branches\n\n| Item | Value |\n| --- | --- |\n| Source branch | unknown |\n| Target branch | unknown |\n| Project path | unknown |\n| Merge status | 开发中 |\n\n## Commit / Diff Notes\n- 待补充\n")
}

pub(crate) fn template_config_changes(req_id: &str) -> String {
    format!("# {req_id} Config Changes\n\n> 低层配置明细；上线总览请同步维护 release-manifest.md。\n\n## DB 变更\n- 暂无\n\n## Apollo / Nacos 变更\n- 暂无\n\n## RocketMQ / Console 变更\n- 暂无\n")
}

pub(crate) fn template_release_manifest(req_id: &str) -> String {
    format!("# {req_id} 上线清单\n\n> 贯穿需求全流程维护；用于上线前快速确认本次改了哪些配置、表、Topic、Group、Job、接口和人工动作，避免发布遗漏。\n\n## Summary\n- 结论：暂无上线资产变更 / 待补充\n- 最后更新：待补充\n- 负责人：待补充\n\n## DB / 表变更\n| 类型 | 表/库 | 变更内容 | 环境 | 是否需上线执行 | 回滚/备注 |\n| --- | --- | --- | --- | --- | --- |\n| 无 | - | - | - | 否 | - |\n\n## 配置变更\n| 类型 | Namespace/配置源 | Key/名称 | 变更内容 | 环境 | 是否已发布 | 备注 |\n| --- | --- | --- | --- | --- | --- | --- |\n| 无 | - | - | - | - | 否 | - |\n\n## MQ / Topic / Group\n| 类型 | Topic | Group/Tag | 生产者 | 消费者 | 控制台动作 | 备注 |\n| --- | --- | --- | --- | --- | --- | --- |\n| 无 | - | - | - | - | 否 | - |\n\n## Job / 定时任务 / 开关\n| 类型 | 名称 | 动作 | 环境 | 是否需人工处理 | 备注 |\n| --- | --- | --- | --- | --- | --- |\n| 无 | - | - | - | 否 | - |\n\n## API / 外部依赖\n| 类型 | 接口/系统 | 变更 | 是否需通知 | 备注 |\n| --- | --- | --- | --- | --- |\n| 无 | - | - | 否 | - |\n\n## 上线人工动作\n- [ ] 暂无\n\n## 风险与回滚提醒\n- 待补充\n")
}

pub(crate) fn template_technical_plan(req_id: &str) -> String {
    format!("# {req_id} 技术方案\n\n> Agent 在执行需求过程中持续维护；用于人工在看代码差异前快速判断实现方向、影响范围、风险控制和验证路径。\n\n## 方案摘要\n- 当前结论：待补充\n- 最后更新：待补充\n- 实现状态：待设计 / 开发中 / 已实现 / 待验证\n\n## 实现目标与非目标\n- 目标：待补充\n- 非目标：待补充\n\n## 总体实现方案\n- 方案路径：待补充\n- 选择原因：待补充\n- 替代方案与取舍：待补充\n\n## 影响范围\n| 应用/模块 | 关键文件/类 | 改动类型 | 说明 |\n| --- | --- | --- | --- |\n| 待补充 | 待补充 | 新增/修改/删除 | - |\n\n## 核心流程变化\n- 改造前：待补充\n- 改造后：待补充\n- 关键状态/数据流：待补充\n\n## 数据、配置与兼容性\n- DB/表字段：暂无 / 待补充\n- 配置/Apollo/Nacos：暂无 / 待补充\n- MQ/Job/外部接口：暂无 / 待补充\n- 兼容性：待补充\n\n## 风险、灰度与回滚\n- 核心链路风险：待评估\n- 性能/并发/幂等风险：待评估\n- 灰度/开关：待补充\n- 回滚方案：待补充\n\n## 验证计划\n- 单测：待补充\n- 接口/链路自测：待补充\n- 回归范围：待补充\n- 观测日志/DB 证据：待补充\n\n## 人工审查关注点\n- 待补充\n\n## 待确认问题\n- 待补充\n")
}

pub(crate) fn template_impact(req_id: &str) -> String {
    format!("# {req_id} Impact\n\n## 风险等级\n- 待评估\n\n## 核心链路影响\n- 待补充\n\n## 回滚方案\n- 待补充\n")
}

pub(crate) fn template_test(req_id: &str) -> String {
    format!("# {req_id} Test\n\n## 测试场景清单\n\n| ID | 场景描述 | 触发方式 | 前置条件 | 预期结果 | 证据标准 |\n| --- | --- | --- | --- | --- | --- |\n| S1 | 待补充 | 待补充 | 待补充 | 待补充 | 日志 + DB + 副作用 + 反向检查 |\n\n## 自测记录\n- ⬜ 待执行\n\n## UAT 回归记录\n- ⬜ 待执行\n")
}

pub(crate) fn template_experience_summary(req_id: &str) -> String {
    format!("# {req_id} 经验总结\n\n## 本次需求结论\n- 待补充\n\n## 新发现的业务知识\n| 发现 | 是否已落地 | 目标位置 | 备注 |\n| --- | --- | --- | --- |\n| 待补充 | 否 | .agents/business-knowledge/ | - |\n\n## 新发现的经验 / 踩坑\n| 经验 | 是否已落地 | 目标位置 | 备注 |\n| --- | --- | --- | --- |\n| 待补充 | 否 | .agents/experiences/ | - |\n\n## Skill 改进机会\n| Skill | 问题 / 机会 | 动作 | 状态 |\n| --- | --- | --- | --- |\n| 待补充 | 待补充 | 新增/优化/不处理 | 待落地 |\n\n## 流程改进\n- 待补充\n\n## 已落地清单\n- [ ] 待补充\n\n## 待落地清单\n- [ ] 待补充\n")
}

pub(crate) fn template_notes(req_id: &str) -> String {
    format!("# {req_id} Notes\n\n## 当前状态\n- 需求已创建。\n\n## 待跟进\n- [ ] 补充需求背景、影响面、分支和测试证据。\n")
}

pub(crate) fn update_meta_summary_line(raw: &str, label: &str, value: &str) -> String {
    let prefix = format!("- {label}:");
    let mut changed = false;
    let lines: Vec<String> = raw
        .split('\n')
        .map(|line| {
            if line.trim_start().starts_with(&prefix) {
                changed = true;
                format!("- {label}: {value}")
            } else {
                line.to_string()
            }
        })
        .collect();
    if changed {
        lines.join("\n")
    } else {
        raw.to_string()
    }
}

pub(crate) fn requirement_doc_template(req: &Requirement, doc_file: &str) -> String {
    match doc_file {
        "alignment.md" => template_alignment(&req.id),
        "background.md" => template_background(&req.id),
        "memory.md" => template_memory(&req.id, &req.title),
        "branch.md" => template_branch(&req.id),
        "config-changes.md" => template_config_changes(&req.id),
        "release-manifest.md" => template_release_manifest(&req.id),
        "technical-plan.md" => template_technical_plan(&req.id),
        "impact.md" => template_impact(&req.id),
        "test.md" => template_test(&req.id),
        "experience-summary.md" => template_experience_summary(&req.id),
        "notes.md" => template_notes(&req.id),
        _ => String::new(),
    }
}

pub(crate) fn requirement_doc_file(doc_type: &str) -> ApiResult<&'static str> {
    match doc_type.trim() {
        "background" | "background.md" => Ok("background.md"),
        "memory" | "memory.md" => Ok("memory.md"),
        "branch" | "branch.md" => Ok("branch.md"),
        "config" | "config-changes" | "config-changes.md" => Ok("config-changes.md"),
        "release-manifest" | "releasemanifest" | "manifest" | "release-manifest.md" => {
            Ok("release-manifest.md")
        }
        "technical-plan"
        | "technicalplan"
        | "implementation-plan"
        | "implementationplan"
        | "tech-plan"
        | "techplan"
        | "solution"
        | "technical-plan.md" => Ok("technical-plan.md"),
        "impact" | "impact.md" => Ok("impact.md"),
        "test" | "test.md" => Ok("test.md"),
        "notes" | "notes.md" => Ok("notes.md"),
        "review" | "review.md" => Ok("review.md"),
        "release-check" | "releasecheck" | "release-check.md" => Ok("release-check.md"),
        "experience-summary" | "experiencesummary" | "experience-summary.md" => {
            Ok("experience-summary.md")
        }
        "alignment" | "alignment.md" => Ok("alignment.md"),
        "prd" | "prd.md" => Ok("prd.md"),
        other => Err(ApiError::bad_request(format!(
            "unsupported docType: {other}"
        ))),
    }
}

pub(crate) fn ensure_doc_heading(req_id: &str, doc_file: &str, content: &str) -> String {
    let clean = content.trim_start_matches('\u{feff}').trim_start();
    if clean.starts_with('#') {
        format!("{}\n", content.trim_end())
    } else {
        format!("# {} {}\n\n{}\n", req_id, doc_file, content.trim_end())
    }
}

pub(crate) fn extract_completed_at(state: &Value) -> Option<i64> {
    state
        .get("history")
        .and_then(Value::as_array)?
        .iter()
        .rev()
        .find(|h| h.get("status").and_then(Value::as_str) == Some("已完成"))
        .and_then(|h| h.get("at").and_then(Value::as_i64))
}

pub(crate) async fn read_requirement_state(dir: &Path) -> Result<Option<Value>> {
    let path = dir.join(STATE_FILE);
    if path.is_file() {
        return Ok(read_json_if_exists(&path).await);
    }
    Ok(None)
}

pub(crate) async fn write_requirement_status(
    req_dir: &str,
    new_status: &str,
    note: Option<&str>,
) -> Result<Value> {
    let dir = PathBuf::from(req_dir);
    let path = dir.join(STATE_FILE);
    let previous = read_requirement_state(&dir)
        .await?
        .unwrap_or_else(|| json!({ "version": 1, "history": [] }));
    let from = previous
        .get("status")
        .and_then(Value::as_str)
        .map(|s| s.to_string());
    let changed = from.as_deref() != Some(new_status);
    let mut history = previous
        .get("history")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let transition = if changed {
        json!({
            "status": new_status,
            "from": from,
            "at": now_ms(),
            "note": note.unwrap_or(""),
            "skippedStatuses": skipped_statuses(from.as_deref(), new_status)
        })
    } else {
        Value::Null
    };
    if changed {
        history.push(transition.clone());
    }
    if history.len() > 50 {
        history = history[history.len() - 50..].to_vec();
    }
    let state = json!({
        "version": 1,
        "status": new_status,
        "previousStatus": from,
        "changed": changed,
        "lastTransition": transition,
        "category": previous.get("category").cloned().unwrap_or(Value::Null),
        "updatedAt": now_ms(),
        "history": history
    });
    atomic_write_json(&path, &state).await?;
    Ok(state)
}

pub(crate) async fn write_requirement_category(req_dir: &str, new_category: &str) -> Result<Value> {
    let dir = PathBuf::from(req_dir);
    let path = dir.join(STATE_FILE);
    let previous = read_requirement_state(&dir)
        .await?
        .unwrap_or_else(|| json!({ "version": 1, "status": "开发中", "history": [] }));
    let state = json!({
        "version": 1,
        "status": previous.get("status").and_then(Value::as_str).unwrap_or("开发中"),
        "category": new_category,
        "updatedAt": now_ms(),
        "history": previous.get("history").cloned().unwrap_or_else(|| json!([]))
    });
    atomic_write_json(&path, &state).await?;
    Ok(state)
}

pub(crate) async fn write_requirement_ones(req_dir: &str, ones: &str) -> Result<String> {
    let path = PathBuf::from(req_dir).join("meta.md");
    let raw = fs::read_to_string(&path).await.unwrap_or_default();
    let normalized = raw.replace("\r\n", "\n");
    let value = ones.trim().to_string();
    let next = set_frontmatter_field(&normalized, "ones", &value);
    atomic_write_text(&path, &next).await?;
    Ok(value)
}

pub(crate) async fn load_fixed_phase_prompt(state: &AppState) -> String {
    load_prompt_file(
        state,
        PHASE_COMMON_PROMPT_FILE,
        "请在推进当前任务的同时，实时记录可复用经验、业务知识和 skill 改进候选。",
    )
    .await
}

pub(crate) async fn load_phase_prompt(state: &AppState, status: &str) -> String {
    load_prompt_file(
        state,
        phase_prompt_file(status),
        &format!("本阶段状态：{status}。请遵循 Agent Panel 需求文件协议推进。"),
    )
    .await
}

pub(crate) async fn load_prompt_file(
    state: &AppState,
    prompt_file: &str,
    fallback: &str,
) -> String {
    let path = state.project_root.join(prompt_file);
    fs::read_to_string(path)
        .await
        .unwrap_or_else(|_| fallback.to_string())
}

pub(crate) fn phase_prompt_file(status: &str) -> &'static str {
    match status {
        "需求澄清" | "需求对齐" | "方案设计" => "prompts/phase-clarify.md",
        "开发中" => "prompts/phase-dev.md",
        "自测中" => "prompts/phase-selftest.md",
        "测试中" => "prompts/phase-testing.md",
        "排查中" | "已确认" => "prompts/phase-online-issue.md",
        "经验总结" | "待上线" => "prompts/phase-experience-summary.md",
        "已完成" => "prompts/phase-done.md",
        _ => "prompts/phase-dev.md",
    }
}
