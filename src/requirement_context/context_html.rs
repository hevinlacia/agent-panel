use super::*;

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
        // notes 是追加型文档：最新内容在尾部，截断取尾（最新优先）；其余文档仍取头。
        let (content, truncated, chars) = if exists && remaining > 0 {
            let raw = fs::read_to_string(&path).await.unwrap_or_default();
            let (excerpt, truncated) = if file == "notes.md" {
                truncate_chars_tail(&raw, per_file_budget)
            } else {
                truncate_chars(&raw, per_file_budget)
            };
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
    let context_tokens = agent_context_tokens(intent, req.category.as_deref() == Some("线上问题"));
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
        // notes 追加型文档取尾摘要（最新章节/最新记录优先），其余文档取头。
        let (summary, truncated) = if file == "notes.md" {
            summarize_requirement_doc_tail(&raw, per_doc_budget)
        } else {
            summarize_requirement_doc(&raw, per_doc_budget)
        };
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
    let mut rules = vec![
        "Treat phaseRuntime as current navigation: it is rebuilt from the requirement's latest state on every context call.",
        "If the session started in an earlier phase, do not keep following the startup prompt; refresh context with for=agent and follow phaseRuntime.fixedPhasePrompt + phaseRuntime.statePhasePrompt.",
        "Skipped phase gaps are risk flags, not hard blockers: record them and continue the user's current task unless a safety gate blocks it.",
        "Prefer recordEvent for facts/status/evidence/decisions; it stores events.jsonl and can append notes.md.",
        "Prefer sections/{section} or upsertSection for targeted impact/test/background/technical-plan updates.",
        "Doc parts: a core doc with a `## 分册索引` section keeps details in docs/<doc>/<NNN>-<slug>.md parts; the main file is an index. Read part files directly by their relPath when details are needed; create new parts via POST /api/requirement/doc-part instead of appending to oversized main docs (validate warns over the split threshold).",
        "Keep technical-plan.md current when implementation direction, affected files, risks or validation strategy changes.",
        "Read full docs only when this compressed context is insufficient.",
    ];
    if req.category.as_deref() == Some("线上问题") {
        rules.push(
            "线上问题文档集：incident.md（现象/影响/时间线）、root-cause.md（根因+可复核证据链+修复决策）、troubleshooting.md（复盘经验）、notes.md（过程流水）；不维护 technical-plan.md/branch.md/config-changes.md/test.md。允许维护 branches.json（req-branches-update 登记）并在需求分支写复现/验证代码，但只能合入 test/UAT 环境分支，后端 master、前端 production 等生产分支会被合并接口拦截；不生成生产 MR。",
        );
        rules.push(
            "每条证据必须附用户可独立复核的验证线索：日志=带时区时间范围+tid/唯一关键字；DB=验证 SQL（表/条件/预期结果）；代码=应用+文件+可搜关键字片段；配置=环境+namespace/key。agent 知道≠证据成立。",
        );
    }
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
        "rules": rules
    }))
}
