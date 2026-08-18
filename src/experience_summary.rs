use std::{
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::{fs, process::Command};
use uuid::Uuid;

use crate::*;

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ExperienceSummaryJob {
    #[serde(default)]
    pub(crate) version: i64,
    #[serde(default)]
    pub(crate) req_id: String,
    #[serde(default)]
    pub(crate) status: String,
    #[serde(default)]
    pub(crate) session_id: Option<String>,
    #[serde(default)]
    pub(crate) model: Option<String>,
    #[serde(default)]
    pub(crate) started_at: Option<i64>,
    #[serde(default)]
    pub(crate) finished_at: Option<i64>,
    #[serde(default)]
    pub(crate) attempts: i64,
    #[serde(default)]
    pub(crate) error: Option<String>,
    #[serde(default)]
    pub(crate) report_path: Option<String>,
    #[serde(default)]
    pub(crate) updated_at: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ExperienceSummaryJobForm {
    pub(crate) req_id: String,
    #[serde(default)]
    pub(crate) note: Option<String>,
    #[serde(default)]
    pub(crate) error: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ExperienceSummaryDispatchForm {
    #[serde(default)]
    pub(crate) req_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ExperienceSummaryCompleteForm {
    pub(crate) req_id: String,
    #[serde(default)]
    pub(crate) session_id: Option<String>,
    #[serde(default)]
    pub(crate) note: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ExperienceSummaryDispatchReport {
    pub(crate) enabled: bool,
    pub(crate) max_agents: usize,
    pub(crate) active: usize,
    pub(crate) queued: usize,
    pub(crate) completed: usize,
    pub(crate) failed: usize,
    pub(crate) dispatched: Vec<Value>,
    pub(crate) skipped: Vec<Value>,
}

pub(crate) fn experience_summary_job_path(dir: &Path) -> PathBuf {
    dir.join(EXPERIENCE_SUMMARY_JOB_FILE)
}

pub(crate) async fn read_experience_summary_job(
    dir: &Path,
) -> Result<Option<ExperienceSummaryJob>> {
    let Some(value) = read_json_if_exists(&experience_summary_job_path(dir)).await else {
        return Ok(None);
    };
    Ok(serde_json::from_value(value).ok())
}

pub(crate) async fn write_experience_summary_job(
    dir: &Path,
    job: &ExperienceSummaryJob,
) -> Result<()> {
    atomic_write_json(&experience_summary_job_path(dir), job).await
}

pub(crate) fn pending_experience_summary_job(
    req: &Requirement,
    dir: &Path,
) -> ExperienceSummaryJob {
    ExperienceSummaryJob {
        version: 1,
        req_id: req.id.clone(),
        status: "pending".to_string(),
        session_id: None,
        model: None,
        started_at: None,
        finished_at: None,
        attempts: 0,
        error: None,
        report_path: Some(
            dir.join("experience-summary.md")
                .to_string_lossy()
                .to_string(),
        ),
        updated_at: now_ms(),
    }
}

pub(crate) fn normalize_experience_summary_job_value(
    req_id: &str,
    dir: &Path,
    job: Option<ExperienceSummaryJob>,
) -> Option<Value> {
    let mut job = job?;
    if job.version <= 0 {
        job.version = 1;
    }
    if job.req_id.trim().is_empty() {
        job.req_id = req_id.to_string();
    }
    if job.status.trim().is_empty() {
        job.status = "pending".to_string();
    }
    if job
        .report_path
        .as_deref()
        .unwrap_or_default()
        .trim()
        .is_empty()
    {
        job.report_path = Some(
            dir.join("experience-summary.md")
                .to_string_lossy()
                .to_string(),
        );
    }
    serde_json::to_value(job).ok()
}

pub(crate) fn experience_summary_stage(req: &Requirement) -> String {
    experience_summary_stage_from_job_value(req.experience_summary_job.as_ref(), &req.status)
}

pub(crate) fn experience_summary_stage_from_job_value(
    job: Option<&Value>,
    req_status: &str,
) -> String {
    let job_status = job
        .and_then(|v| v.get("status"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    match job_status {
        "completed" => "completed".to_string(),
        "running" | "pending" => "running".to_string(),
        "failed" => "failed".to_string(),
        "skipped" => "skipped".to_string(),
        _ if req_status == "经验总结" => "available".to_string(),
        _ => "none".to_string(),
    }
}

pub(crate) fn experience_summary_stats_from_items(items: &[Value]) -> Value {
    let mut available = 0;
    let mut running = 0;
    let mut completed = 0;
    let mut failed = 0;
    let mut skipped = 0;
    for item in items {
        match item.get("stage").and_then(Value::as_str).unwrap_or("none") {
            "available" => available += 1,
            "running" => running += 1,
            "completed" => completed += 1,
            "failed" => failed += 1,
            "skipped" => skipped += 1,
            _ => {}
        }
    }
    json!({
        "total": items.len(),
        "available": available,
        "running": running,
        "completed": completed,
        "failed": failed,
        "skipped": skipped,
    })
}

pub(crate) async fn dispatch_experience_summary_jobs(
    state: &AppState,
    only_req_id: Option<&str>,
) -> ApiResult<ExperienceSummaryDispatchReport> {
    let _guard = state.experience_summary_dispatch.lock().await;
    let cfg = read_config(state).await?;
    let max_agents = clamp_experience_summary_max_agents(cfg.experience_summary_max_agents);
    let mut report = ExperienceSummaryDispatchReport {
        enabled: cfg.auto_experience_summary,
        max_agents,
        active: 0,
        queued: 0,
        completed: 0,
        failed: 0,
        dispatched: Vec::new(),
        skipped: Vec::new(),
    };
    if !cfg.auto_experience_summary && only_req_id.is_none() {
        return Ok(report);
    }

    let mut reqs = list_requirements(state).await?;
    if let Some(req_id) = only_req_id {
        reqs.retain(|r| r.id == req_id);
    }
    let now = now_ms();

    for req in reqs {
        if req.status != "经验总结" {
            if matches!(
                req.experience_summary_job
                    .as_ref()
                    .and_then(|j| j.get("status"))
                    .and_then(Value::as_str),
                Some("completed")
            ) {
                report.completed += 1;
            }
            continue;
        }
        let dir = match req_dir_path(&req) {
            Ok(d) => d,
            Err(e) => {
                report
                    .skipped
                    .push(json!({ "reqId": req.id, "reason": e.message }));
                continue;
            }
        };
        let mut job = read_experience_summary_job(&dir)
            .await?
            .unwrap_or_else(|| pending_experience_summary_job(&req, &dir));
        job.req_id = req.id.clone();
        job.version = 1;
        if job
            .report_path
            .as_deref()
            .unwrap_or_default()
            .trim()
            .is_empty()
        {
            job.report_path = Some(
                dir.join("experience-summary.md")
                    .to_string_lossy()
                    .to_string(),
            );
        }
        if job.status == "running"
            && job
                .started_at
                .map(|started| now - started > EXPERIENCE_SUMMARY_JOB_STALE_MS)
                .unwrap_or(false)
        {
            job.status = "failed".to_string();
            job.finished_at = Some(now);
            job.error = Some("自动经验总结 agent 超过 12 小时未回写完成状态".to_string());
            job.updated_at = now;
            write_experience_summary_job(&dir, &job).await?;
        }
        match job.status.as_str() {
            "completed" => {
                report.completed += 1;
                continue;
            }
            "running" => {
                report.active += 1;
                continue;
            }
            "failed" => {
                report.failed += 1;
                if only_req_id.is_none() {
                    continue;
                }
            }
            "pending" | "" => {}
            other => {
                report.skipped.push(json!({ "reqId": req.id, "reason": format!("unsupported job status: {other}") }));
                continue;
            }
        }
        if !cfg.auto_experience_summary && only_req_id.is_none() {
            report
                .skipped
                .push(json!({ "reqId": req.id, "reason": "auto experience summary disabled" }));
            continue;
        }
        if report.active >= max_agents {
            job.status = "pending".to_string();
            job.updated_at = now;
            write_experience_summary_job(&dir, &job).await?;
            report.queued += 1;
            continue;
        }
        let session_id = Uuid::new_v4().to_string();
        let model = clean_optional(Some(&cfg.experience_summary_pi_model));
        match spawn_experience_summary_agent(state, &req, &dir, &session_id, model.as_deref()).await
        {
            Ok(message) => {
                job.status = "running".to_string();
                job.session_id = Some(session_id.clone());
                job.model = model.clone();
                job.started_at = Some(now_ms());
                job.finished_at = None;
                job.attempts += 1;
                job.error = None;
                job.updated_at = now_ms();
                write_experience_summary_job(&dir, &job).await?;
                report.active += 1;
                report.dispatched.push(json!({
                    "reqId": req.id,
                    "sessionId": session_id,
                    "model": model,
                    "message": message,
                    "job": job,
                }));
            }
            Err(err) => {
                job.status = "failed".to_string();
                job.finished_at = Some(now_ms());
                job.error = Some(err.to_string());
                job.updated_at = now_ms();
                write_experience_summary_job(&dir, &job).await?;
                report.failed += 1;
                report
                    .skipped
                    .push(json!({ "reqId": req.id, "reason": err.to_string() }));
            }
        }
    }
    Ok(report)
}

pub(crate) async fn spawn_experience_summary_agent(
    state: &AppState,
    req: &Requirement,
    dir: &Path,
    session_id: &str,
    model: Option<&str>,
) -> Result<String> {
    associate_session(state, &req.id, session_id).await?;
    let ctx_path = write_injection_context(state, req, session_id).await?;
    let cwd = requirement_project_root(req).unwrap_or_else(|| state.project_root.as_ref().clone());
    let report_path = dir.join("experience-summary.md");
    let prompt = format!(
        "自动经验总结任务。需求：{req_id} - {title}\n\n\
         目标：按当前经验总结阶段规则，读取 Agent Panel 候选上下文，回顾本需求文档和结构化事件，沉淀可复用业务知识、经验/踩坑和 skill 改进机会。\n\
         必须先读取：GET http://127.0.0.1:7331/api/requirement/experience-summary-context?id={req_id}&limit=200\n\
         必须写入：experience-summary.md（路径：{report_path}），区分已落地和待落地。\n\
         可安全落地的知识/经验请通过 Agent Panel API 写入 business-knowledge / experiences；不要把未验证猜测写成稳定事实。\n\
         完成后必须调用：POST http://127.0.0.1:7331/api/experience-summary/jobs/complete，JSON body 为 {{\"reqId\":\"{req_id}\",\"sessionId\":\"{session_id}\",\"note\":\"自动经验总结完成\"}}。\n\
         若无法完成，请尽量把原因写入 notes.md 或通过 /api/requirement/events 记录。",
        req_id = req.id,
        title = req.title,
        report_path = report_path.to_string_lossy(),
        session_id = session_id,
    );
    let mut cmd = Command::new("pi");
    cmd.current_dir(&cwd)
        .arg("-p")
        .arg("--session-id")
        .arg(session_id)
        .arg("--name")
        .arg(format!("经验总结 {}", req.id))
        .arg("--append-system-prompt")
        .arg(format!("@{}", ctx_path.to_string_lossy()))
        .arg("--tools")
        .arg("bash,read,write,edit")
        .arg("--approve");
    if let Some(model) = model.map(str::trim).filter(|v| !v.is_empty()) {
        cmd.arg("--model").arg(model);
    }
    cmd.arg(prompt)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let mut child = cmd
        .spawn()
        .with_context(|| format!("spawn pi auto experience-summary agent for {}", req.id))?;
    let pid = child.id().unwrap_or(0);
    tokio::spawn(async move {
        let _ = child.wait().await;
    });
    Ok(format!("pi agent spawned pid={pid}, cwd={}", cwd.display()))
}

pub(crate) fn requirement_project_root(req: &Requirement) -> Option<PathBuf> {
    let req_dir = PathBuf::from(req.req_dir.as_ref()?);
    for ancestor in req_dir.ancestors() {
        if ancestor.file_name().and_then(|v| v.to_str()) == Some("req") {
            if ancestor
                .parent()
                .and_then(|p| p.file_name())
                .and_then(|v| v.to_str())
                == Some(".agents")
            {
                return ancestor
                    .parent()
                    .and_then(|p| p.parent())
                    .map(Path::to_path_buf);
            }
            return ancestor.parent().map(Path::to_path_buf);
        }
    }
    None
}

pub(crate) async fn write_injection_context(
    state: &AppState,
    req: &Requirement,
    session_id: &str,
) -> Result<PathBuf> {
    let dir = state.data_dir.join(INJECTION_CTX_SUBDIR);
    fs::create_dir_all(&dir).await?;
    let path = dir.join(format!("{}.md", session_id));
    let intent = default_intent_for_status(&req.status);
    let budget = 5_000;
    let agent_context = build_requirement_agent_context(state, req, intent, budget, 8).await;
    let body = match &agent_context {
        Ok(context) => {
            render_injection_context(req, session_id, intent, budget, Some(context), None)
        }
        Err(err) => render_injection_context(
            req,
            session_id,
            intent,
            budget,
            None,
            Some(err.message.as_str()),
        ),
    };
    atomic_write_text(&path, &body).await?;
    Ok(path)
}

pub(crate) fn render_injection_context(
    req: &Requirement,
    session_id: &str,
    intent: &str,
    budget: usize,
    agent_context: Option<&Value>,
    context_error: Option<&str>,
) -> String {
    let refresh_url = format!(
        "/api/requirement/context?id={}&for=agent&intent={}&budget={}",
        req.id, intent, budget
    );
    let mut out = format!(
        "# Agent Panel Requirement Startup Context\n\n> 这是 Agent Panel 创建 pi session 时注入的一次性启动上下文。你已经具备需求的压缩背景，第一轮不要再询问“这个需求是什么”；除非用户要求澄清，直接围绕当前任务推进。\n\n## Requirement\n- Req ID: {}\n- Title: {}\n- Startup Status: {}\n- Startup Intent: {}\n- Session ID: {}\n- Project: {}\n- Directory: {}\n- Refresh Context: `{}`\n\n",
        req.id,
        req.title,
        req.status,
        intent,
        session_id,
        req.project,
        req.req_dir.clone().unwrap_or_default(),
        refresh_url
    );
    if !req.description.trim().is_empty() {
        out.push_str("## User-Facing Description\n");
        out.push_str(req.description.trim());
        out.push_str("\n\n");
    }
    if let Some(err) = context_error {
        out.push_str("## Startup Context Warning\n");
        out.push_str("- Agent Panel could not build the compressed context: ");
        out.push_str(err);
        out.push_str("\n- Continue with the requirement metadata above, then refresh context through the API when available.\n\n");
    }
    if let Some(context) = agent_context {
        append_agent_context_markdown(&mut out, context);
    }
    out.push_str("## Operating Rules\n");
    out.push_str("1. This startup snapshot is only for initial grounding; after status changes or before substantial edits, refresh the agent context URL above.\n");
    out.push_str("2. Prefer `POST /api/requirement/events`, `/api/requirement/sections/{section}`, or `/api/requirement/edit` over directly rewriting requirement files.\n");
    out.push_str("3. Keep `technical-plan.md` and `notes.md` current when implementation direction, risks, validation evidence, or open questions change.\n");
    out.push_str("4. Record real-time `knowledgeReference`, `learningCandidate`, and `skillImprovementCandidate` events when current-session details may help later experience summary.\n");
    out.push_str("5. If `memory.md` exists, treat it as a useful lifecycle memo; if it is missing, rely on the compressed background / technical plan / notes summaries instead of assuming context is absent.\n");
    out
}

pub(crate) fn append_agent_context_markdown(out: &mut String, context: &Value) {
    if let Some(phase) = context.get("phaseRuntime") {
        out.push_str("## Current Phase Snapshot\n");
        push_value_bullet(out, "Current Status", phase.get("currentStatus"));
        push_value_bullet(out, "Intent", phase.get("intent"));
        push_value_bullet(out, "Recommended Intent", phase.get("recommendedIntent"));
        push_value_bullet(out, "Fixed Prompt File", phase.get("fixedPhasePromptFile"));
        push_value_bullet(out, "State Prompt File", phase.get("statePhasePromptFile"));
        if let Some(prompt) = phase.get("fixedPhasePrompt").and_then(Value::as_str) {
            let (excerpt, truncated) = truncate_chars(prompt.trim(), 1_400);
            out.push_str("\n### Fixed Lifecycle Prompt\n```text\n");
            out.push_str(excerpt.trim());
            if truncated {
                out.push_str("\n[fixed prompt truncated in startup context]");
            }
            out.push_str("\n```\n");
        }
        if let Some(prompt) = phase.get("statePhasePrompt").and_then(Value::as_str) {
            let (excerpt, truncated) = truncate_chars(prompt.trim(), 1_600);
            out.push_str("\n### State-Specific Phase Prompt\n```text\n");
            out.push_str(excerpt.trim());
            if truncated {
                out.push_str("\n[state prompt truncated in startup context]");
            }
            out.push_str("\n```\n");
        }
        if let Some(checks) = phase.get("entryChecks").and_then(Value::as_array) {
            out.push_str("\n### Entry Checks\n");
            for check in checks.iter().take(8) {
                let status = check
                    .get("status")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown");
                let label = check
                    .get("label")
                    .and_then(Value::as_str)
                    .unwrap_or("entry check");
                let source = check.get("source").and_then(Value::as_str).unwrap_or("-");
                out.push_str(&format!("- [{}] {} ({})\n", status, label, source));
            }
        }
        if let Some(gaps) = phase.get("phaseGaps") {
            if let Some(missing) = gaps
                .get("missingRequiredEntryChecks")
                .and_then(Value::as_array)
                .filter(|items| !items.is_empty())
            {
                out.push_str("\n### Phase Gap Warnings\n");
                for item in missing.iter().take(6) {
                    let label = item
                        .get("label")
                        .and_then(Value::as_str)
                        .unwrap_or("missing required check");
                    out.push_str(&format!("- Missing: {}\n", label));
                }
            }
        }
        out.push('\n');
    }

    if let Some(docs) = context.get("summaryDocs").and_then(Value::as_array) {
        out.push_str("## Compressed Requirement Docs\n");
        for doc in docs.iter().take(8) {
            let token = doc
                .get("token")
                .and_then(Value::as_str)
                .unwrap_or("req.doc");
            let file = doc.get("file").and_then(Value::as_str).unwrap_or("-");
            let exists = doc.get("exists").and_then(Value::as_bool).unwrap_or(false);
            let bytes = doc.get("bytes").and_then(Value::as_u64).unwrap_or(0);
            let truncated = doc
                .get("truncated")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            out.push_str(&format!(
                "### {} ({})\n- Exists: {} · Bytes: {}{}\n",
                token,
                file,
                exists,
                bytes,
                if truncated { " · Truncated" } else { "" }
            ));
            let excerpt = doc
                .get("summary")
                .and_then(|summary| summary.get("excerpt"))
                .and_then(Value::as_str)
                .unwrap_or_default();
            if excerpt.trim().is_empty() {
                out.push_str("_No startup excerpt captured; file may be missing or empty._\n\n");
            } else {
                let (excerpt, local_truncated) = truncate_chars(excerpt.trim(), 1_200);
                out.push_str(excerpt.trim());
                if local_truncated {
                    out.push_str("\n[doc excerpt truncated in startup context]");
                }
                out.push_str("\n\n");
            }
        }
    }

    if let Some(events) = context.get("recentEvents").and_then(Value::as_array) {
        if !events.is_empty() {
            out.push_str("## Recent Structured Events\n");
            for event in events.iter().take(8) {
                let event_type =
                    str_field(event, &["type", "eventType", "operation"]).unwrap_or("event");
                let summary = str_field(event, &["summary", "title", "text", "note"])
                    .map(str::to_string)
                    .unwrap_or_else(|| value_to_inline(event));
                let (summary, truncated) = truncate_chars(summary.trim(), 360);
                out.push_str(&format!(
                    "- [{}] {}{}\n",
                    event_type,
                    summary.trim(),
                    if truncated { " …" } else { "" }
                ));
            }
            out.push('\n');
        }
    }

    if let Some(apis) = context.get("apis").and_then(Value::as_object) {
        out.push_str("## Requirement APIs\n");
        for (name, value) in apis {
            out.push_str(&format!("- {}: `{}`\n", name, value_to_inline(value)));
        }
        out.push('\n');
    }

    if let Some(writes) = context.get("recommendedWrites").and_then(Value::as_array) {
        if !writes.is_empty() {
            out.push_str("## Recommended Next Writes\n");
            for item in writes.iter().take(6) {
                let method = item.get("method").and_then(Value::as_str).unwrap_or("POST");
                let path = item.get("path").and_then(Value::as_str).unwrap_or("-");
                out.push_str(&format!("- `{}` `{}`\n", method, path));
            }
            out.push('\n');
        }
    }
}

pub(crate) fn push_value_bullet(out: &mut String, label: &str, value: Option<&Value>) {
    if let Some(value) = value {
        let rendered = value_to_inline(value);
        if !rendered.trim().is_empty() && rendered != "null" {
            out.push_str(&format!("- {}: {}\n", label, rendered));
        }
    }
}

pub(crate) fn str_field<'a>(value: &'a Value, keys: &[&str]) -> Option<&'a str> {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(Value::as_str))
}

pub(crate) fn value_to_inline(value: &Value) -> String {
    match value {
        Value::Null => "null".to_string(),
        Value::Bool(v) => v.to_string(),
        Value::Number(v) => v.to_string(),
        Value::String(v) => v.clone(),
        Value::Array(items) => items
            .iter()
            .map(value_to_inline)
            .collect::<Vec<_>>()
            .join(" / "),
        Value::Object(_) => serde_json::to_string(value).unwrap_or_else(|_| value.to_string()),
    }
}

pub(crate) async fn experience_summary_dispatch_loop(state: AppState) {
    let mut ticker =
        tokio::time::interval(Duration::from_secs(EXPERIENCE_AUTO_SUMMARY_INTERVAL_SECS));
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        ticker.tick().await;
        match dispatch_experience_summary_jobs(&state, None).await {
            Ok(report) => tracing::info!(
                active = report.active,
                queued = report.queued,
                completed = report.completed,
                failed = report.failed,
                dispatched = report.dispatched.len(),
                "experience summary auto-dispatch scan complete"
            ),
            Err(e) => tracing::warn!("experience summary auto-dispatch scan failed: {e:?}"),
        }
    }
}

/// 从 state.json 提取该需求进入「经验总结」状态的时间戳（毫秒）。
/// 取 history 中最后一次 status == 经验总结 的 at；历史缺失时回退到 updated_at。
pub(crate) fn experience_summary_entered_at_from_state(
    state: &Value,
    fallback_updated_at: i64,
) -> i64 {
    if let Some(history) = state.get("history").and_then(Value::as_array) {
        for entry in history.iter().rev() {
            let is_experience = entry
                .get("status")
                .and_then(Value::as_str)
                .map(|s| s == "经验总结")
                .unwrap_or(false);
            if is_experience {
                return entry
                    .get("at")
                    .and_then(Value::as_i64)
                    .unwrap_or(fallback_updated_at);
            }
        }
    }
    fallback_updated_at
}

/// 判定：进入经验总结的时间早于 (now - grace_ms) 即视为超期，应自动推进为已完成。
pub(crate) fn experience_summary_overdue(entered_at: i64, now: i64, grace_ms: i64) -> bool {
    entered_at > 0 && now - entered_at >= grace_ms
}

/// 判定：该需求当前是否应被自动推进为已完成。
/// 要求 state.json 真实状态为「经验总结」且进入该状态超过 grace_ms；
/// 「待上线」等历史遗留状态不参与自动推进。
pub(crate) fn should_auto_complete_experience_summary(
    state: &Value,
    fallback_updated_at: i64,
    now: i64,
    grace_ms: i64,
) -> bool {
    let real_status = state
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if real_status != "经验总结" {
        return false;
    }
    let entered_at = experience_summary_entered_at_from_state(state, fallback_updated_at);
    experience_summary_overdue(entered_at, now, grace_ms)
}

/// 扫描所有需求，将停留在「经验总结」状态超过阈值的需求自动改为「已完成」。
/// 返回本次实际推进的数量。
pub(crate) async fn expire_stale_experience_summary(state: &AppState) -> Result<usize> {
    let reqs = list_requirements(state).await?;
    let now = now_ms();
    let mut changed = 0;
    for req in reqs {
        if req.status != "经验总结" {
            continue;
        }
        let dir = match req_dir_path(&req) {
            Ok(d) => d,
            Err(_) => continue,
        };
        let Some(state_value) = read_requirement_state(&dir).await? else {
            continue;
        };
        if !should_auto_complete_experience_summary(
            &state_value,
            req.updated_at,
            now,
            EXPERIENCE_SUMMARY_GRACE_MS,
        ) {
            continue;
        }
        let job = read_experience_summary_job(&dir).await?;
        let job_status = job.as_ref().map(|j| j.status.as_str()).unwrap_or("");
        let cfg = read_config(state).await.unwrap_or_default();
        if cfg.auto_experience_summary && !matches!(job_status, "completed" | "skipped") {
            // 自动总结开启时，48h 自动完成只作为“agent 已完成但状态未推进”的兜底；
            // pending/running/failed 不直接吞掉为已完成，避免总结还没做完就被关闭。
            continue;
        }
        let st = write_requirement_status(
            dir.to_string_lossy().as_ref(),
            "已完成",
            Some(EXPERIENCE_AUTO_COMPLETE_NOTE),
        )
        .await?;
        // 事件记录失败不阻断推进，仅告警（避免单个需求写失败阻塞整个扫描）。
        if let Err(e) =
            record_status_transition_event(state, &req, &st, Some(EXPERIENCE_AUTO_COMPLETE_NOTE))
                .await
        {
            tracing::warn!(req_id = %req.id, "auto-complete event record failed: {e:?}");
        } else {
            tracing::info!(req_id = %req.id, "auto-completed requirement after staying in 经验总结 >48h");
        }
        changed += 1;
    }
    Ok(changed)
}

/// 常驻后台任务：每隔固定周期扫描一次，自动推进超期停留在经验总结状态的需求。
pub(crate) async fn expire_stale_experience_summary_loop(state: AppState) {
    let mut ticker =
        tokio::time::interval(Duration::from_secs(EXPERIENCE_AUTO_COMPLETE_INTERVAL_SECS));
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        ticker.tick().await;
        match expire_stale_experience_summary(&state).await {
            Ok(n) => tracing::info!(
                "experience summary auto-complete scan: {n} requirement(s) advanced to 已完成"
            ),
            Err(e) => tracing::warn!("experience summary auto-complete scan failed: {e:#}"),
        }
    }
}
