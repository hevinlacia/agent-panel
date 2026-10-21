use std::{
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};

use axum::{
    extract::{Query, State},
    Json,
};
use regex::Regex;
use serde_json::{json, Value};
use tokio::fs;

use crate::markdown::html_escape;
use crate::{
    atomic_write_text, ensure_safe_segment, get_real_requirement, req_dir_path, sha1_hex,
    system_time_to_ms, truncate_chars, AgentFileReadQuery, AgentFileWriteForm, ApiError, ApiResult,
    AppState, FormOrJson, IdQuery,
};

pub(crate) async fn api_attachments(
    State(state): State<AppState>,
    Query(query): Query<IdQuery>,
) -> ApiResult<Json<Value>> {
    let id = query.id.or(query.req_id).unwrap_or_default();
    let req = get_real_requirement(&state, &id).await?;
    let dir = PathBuf::from(req.req_dir.unwrap_or_default());
    // full=1 时返回附件全文（上限 200KB），供需求详情页折叠全文展示与一键复制；默认只带 600 字符摘要。
    let full = matches!(
        query.full.as_deref(),
        Some("1") | Some("true") | Some("yes")
    );
    let rows = requirement_attachment_rows(&dir, query.budget.unwrap_or(600), full).await;
    Ok(Json(json!({ "attachments": rows })))
}

pub(crate) async fn requirement_attachment_rows(
    dir: &Path,
    sample_budget: usize,
    full: bool,
) -> Vec<Value> {
    let attachments_dir = dir.join("attachments");
    let mut rows = Vec::new();
    if let Ok(mut rd) = fs::read_dir(&attachments_dir).await {
        while let Ok(Some(entry)) = rd.next_entry().await {
            let Ok(meta) = entry.metadata().await else {
                continue;
            };
            if !meta.is_file() {
                continue;
            }
            let path = entry.path();
            let filename = entry.file_name().to_string_lossy().to_string();
            let ext = path
                .extension()
                .and_then(|s| s.to_str())
                .unwrap_or_default()
                .to_ascii_lowercase();
            let mut summary = Vec::new();
            let mut sample = String::new();
            if matches!(
                ext.as_str(),
                "sql" | "txt" | "md" | "yaml" | "yml" | "json" | "csv"
            ) {
                let raw = fs::read_to_string(&path).await.unwrap_or_default();
                if ext == "sql" {
                    let update_count = Regex::new(r"(?i)\bupdate\b")
                        .unwrap()
                        .find_iter(&raw)
                        .count();
                    let insert_count = Regex::new(r"(?i)\binsert\b")
                        .unwrap()
                        .find_iter(&raw)
                        .count();
                    let delete_count = Regex::new(r"(?i)\bdelete\b")
                        .unwrap()
                        .find_iter(&raw)
                        .count();
                    let alter_count = Regex::new(r"(?i)\balter\b")
                        .unwrap()
                        .find_iter(&raw)
                        .count();
                    if update_count > 0 {
                        summary.push(format!("{update_count} UPDATE"));
                    }
                    if insert_count > 0 {
                        summary.push(format!("{insert_count} INSERT"));
                    }
                    if delete_count > 0 {
                        summary.push(format!("{delete_count} DELETE"));
                    }
                    if alter_count > 0 {
                        summary.push(format!("{alter_count} ALTER"));
                    }
                }
                let (excerpt, truncated) = truncate_chars(
                    &raw,
                    if full {
                        200_000
                    } else {
                        sample_budget.min(1200)
                    },
                );
                sample = if truncated {
                    format!("{}\n…", excerpt.trim_end())
                } else {
                    excerpt
                };
            }
            rows.push(json!({
                "filename": filename,
                "path": path.to_string_lossy(),
                "relativePath": path.strip_prefix(dir).ok().map(|p| p.to_string_lossy().to_string()).unwrap_or_else(|| path.to_string_lossy().to_string()),
                "extension": ext,
                "size": meta.len(),
                "mtime": system_time_to_ms(meta.modified().unwrap_or(UNIX_EPOCH)),
                "summary": summary,
                "sample": sample,
            }));
        }
    }
    rows.sort_by(|a, b| {
        a.get("filename")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .cmp(
                b.get("filename")
                    .and_then(Value::as_str)
                    .unwrap_or_default(),
            )
    });
    rows
}

pub(crate) fn attachment_total_bytes(dir: &Path) -> u64 {
    let attachments_dir = dir.join("attachments");
    let mut total = 0;
    if let Ok(rd) = std::fs::read_dir(&attachments_dir) {
        for entry in rd.flatten() {
            if let Ok(meta) = entry.metadata() {
                if meta.is_file() {
                    total += meta.len();
                }
            }
        }
    }
    total
}

pub(crate) async fn render_requirement_attachments_context(
    dir: &Path,
    sample_budget: usize,
) -> String {
    let rows = requirement_attachment_rows(dir, sample_budget, false).await;
    if rows.is_empty() {
        return "# 非代码附件\n\n- 暂无 attachments/ 附件。\n".to_string();
    }
    let mut out = String::from("# 上线资产附件 / Release Attachments\n\n> 需求目录 attachments/ 下的上线资产事实源：SQL（文件头注释块含用途/目标环境/上线是否需执行/回滚方式）与配置/人工动作记录；发布预检时逐项核对，标注「上线是否需执行: 否」的为测试专用资产。\n\n| 文件 | 类型/统计 | 大小 | 更新时间 | 路径 |\n| --- | --- | ---: | --- | --- |\n");
    for row in &rows {
        let filename = row.get("filename").and_then(Value::as_str).unwrap_or("-");
        let ext = row.get("extension").and_then(Value::as_str).unwrap_or("-");
        let summary = row
            .get("summary")
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join(" / ")
            })
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| ext.to_string());
        let size = row.get("size").and_then(Value::as_u64).unwrap_or(0);
        let mtime = row.get("mtime").and_then(Value::as_i64).unwrap_or(0);
        let path = row.get("path").and_then(Value::as_str).unwrap_or("-");
        out.push_str(&format!(
            "| `{}` | {} | {} | {} | `{}` |\n",
            filename,
            summary,
            human_bytes(size),
            format_ms(mtime),
            path
        ));
    }
    out.push_str("\n## 附件内容摘要\n");
    for row in &rows {
        let filename = row.get("filename").and_then(Value::as_str).unwrap_or("-");
        let summary = row
            .get("summary")
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join(" / ")
            })
            .unwrap_or_default();
        out.push_str(&format!("\n### {}\n", filename));
        if !summary.is_empty() {
            out.push_str(&format!("- 统计：{}\n", summary));
        }
        if let Some(sample) = row
            .get("sample")
            .and_then(Value::as_str)
            .filter(|s| !s.trim().is_empty())
        {
            out.push_str("```\n");
            out.push_str(sample.trim_end());
            out.push_str("\n```\n");
        } else {
            out.push_str("- 二进制或暂不预览内容；请按路径打开核对。\n");
        }
    }
    out
}

// ---- Agent 附件读写（GET/PUT /api/requirement/attachment）----
//
// 定位：2026-10-20 数据 SSOT 迁入容器 hub 后，本机 `.agents/req/` 只是只读镜像，
// 旧「agent 直写 attachments/ 文件」路径会导致改动落在本地 clone 工作区与 hub 漂移
// （面板/门禁/合规快照看不到附件）。本端点是附件唯一可靠写通道：白名单限
// `attachments/<file>` 单层 + 文本后缀，sha1 乐观锁与 /api/requirement/file 同构。

/// 附件可写后缀白名单：与只读渲染（requirement_attachment_rows）支持的文本类型完全一致。
const ATTACHMENT_ALLOWED_EXTS: [&str; 7] = ["sql", "txt", "md", "yaml", "yml", "json", "csv"];

/// 附件相对路径白名单：统一归一化为 `attachments/<file>` 单层路径。
/// 兼容两种入参：`attachments/xxx.sql` 或裸文件名 `xxx.sql`（减少 agent 选择成本）。
/// 拒绝：绝对路径、`..` 穿越、空段、多层子目录、非白名单后缀、非 path-safe 文件名。
pub(crate) fn resolve_attachment_rel_path(input: &str) -> ApiResult<String> {
    let v = input.trim().trim_start_matches("./");
    if v.is_empty() || v.starts_with('/') || v.split('/').any(|s| s == ".." || s.is_empty()) {
        return Err(ApiError::bad_request(format!(
            "非法附件路径：{input}（只允许 attachments/<file> 单层路径）"
        )));
    }
    let name = v.strip_prefix("attachments/").unwrap_or(v);
    if name.contains('/') {
        return Err(ApiError::bad_request(format!(
            "只允许 attachments/ 单层附件文件，不支持子目录：{input}"
        )));
    }
    let safe = ensure_safe_segment(name, "附件名")?;
    let ext = safe
        .rsplit_once('.')
        .map(|(_, e)| e.to_ascii_lowercase())
        .unwrap_or_default();
    if !ATTACHMENT_ALLOWED_EXTS.contains(&ext.as_str()) {
        return Err(ApiError::bad_request(format!(
            "附件后缀 .{ext} 不在白名单（允许：sql/txt/md/yaml/yml/json/csv）：{input}"
        )));
    }
    Ok(format!("attachments/{safe}"))
}

/// GET /api/requirement/attachment?id=&path=：读单个附件，返回内容 + sha 乐观锁版本
/// （列表接口 /api/requirement/attachments 只给摘要不带 sha，更新前用它取 baseSha）。
pub(crate) async fn api_attachment_get(
    State(state): State<AppState>,
    Query(query): Query<AgentFileReadQuery>,
) -> ApiResult<Json<Value>> {
    let req_id = query.id.or(query.req_id).unwrap_or_default();
    let req = get_real_requirement(&state, &req_id).await?;
    let rel = resolve_attachment_rel_path(query.path.as_deref().unwrap_or_default())?;
    let dir = req_dir_path(&req)?;
    let path = dir.join(&rel);
    let (exists, content) = match fs::read_to_string(&path).await {
        Ok(text) => (true, text),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => (false, String::new()),
        Err(e) => return Err(ApiError::from(e)),
    };
    let exists_sha = if exists {
        Some(sha1_hex(&content))
    } else {
        None
    };
    Ok(Json(json!({
        "id": req.id,
        "path": rel,
        "exists": exists,
        "content": content,
        "sha": exists_sha.clone(),
        "baseSha": exists_sha,
        "size": content.len(),
    })))
}

/// PUT /api/requirement/attachment：带 sha 乐观锁写附件。语义与 /api/requirement/file 一致：
/// 新建可省 baseSha（含空内容标记 sha 降级）；更新必须带 GET 返回的 baseSha，
/// 不匹配返回 409（重新 GET 再写）。
pub(crate) async fn api_attachment_put(
    State(state): State<AppState>,
    form: FormOrJson<AgentFileWriteForm>,
) -> ApiResult<Json<Value>> {
    let form = form.0;
    let req_id = form.id.or(form.req_id).unwrap_or_default();
    let req = get_real_requirement(&state, &req_id).await?;
    let rel = resolve_attachment_rel_path(&form.path)?;
    let dir = req_dir_path(&req)?;
    let path = dir.join(&rel);
    let current = match fs::read_to_string(&path).await {
        Ok(text) => Some(text),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(ApiError::from(e)),
    };
    let current_sha = current.as_deref().map(sha1_hex);
    let base_sha = form
        .base_sha
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_ascii_lowercase());
    let empty_content_sha = sha1_hex("");
    match (&current_sha, &base_sha) {
        (Some(cur), Some(base)) if cur != base => {
            return Err(ApiError::conflict(format!(
                "附件已被修改（当前 sha {cur}，请求基于 {base}）；重新 GET /api/requirement/attachment 获取最新内容与 sha 后再写"
            )));
        }
        (Some(_), None) => {
            return Err(ApiError::conflict(
                "附件已存在：更新必须携带 GET 返回的 baseSha；如需覆盖请先 GET",
            ));
        }
        (None, Some(base)) if *base == empty_content_sha => {}
        (None, Some(_)) => {
            return Err(ApiError::conflict(
                "附件不存在（可能已被删除）：新建请省略 baseSha，或重新 GET 后直接写",
            ));
        }
        _ => {}
    }
    atomic_write_text(&path, &form.content).await?;
    Ok(Json(json!({
        "ok": true,
        "id": req.id,
        "path": rel,
        "sha": sha1_hex(&form.content),
        "size": form.content.len(),
        "created": current.is_none(),
    })))
}

pub(crate) fn human_bytes(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1} MB", bytes as f64 / 1024.0 / 1024.0)
    }
}

pub(crate) fn format_ms(ms: i64) -> String {
    if ms <= 0 {
        return "-".to_string();
    }
    chrono::DateTime::from_timestamp(ms / 1000, 0)
        .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
        .unwrap_or_else(|| "-".to_string())
}

/// 附件清单上下文 HTML：总表默认展开；每个文件一个可折叠块（默认摘要，点击展开全文），
/// 并带一个“一键复制”按钮复制该文件全文。
pub(crate) fn render_requirement_attachments_html(dir: &Path) -> String {
    const MAX_EMBED_BYTES: u64 = 200 * 1024;
    let attachments_dir = dir.join("attachments");
    struct FileRow {
        filename: String,
        ext: String,
        summary: Vec<String>,
        size: u64,
        mtime: i64,
        path: String,
        body: String,
        truncated: bool,
    }
    let mut files: Vec<FileRow> = Vec::new();
    if let Ok(rd) = std::fs::read_dir(&attachments_dir) {
        for entry in rd.flatten() {
            let Ok(meta) = entry.metadata() else { continue };
            if !meta.is_file() {
                continue;
            }
            let path = entry.path();
            let filename = entry.file_name().to_string_lossy().to_string();
            let ext = path
                .extension()
                .and_then(|s| s.to_str())
                .unwrap_or_default()
                .to_ascii_lowercase();
            let mut summary = Vec::new();
            let mut body = String::new();
            let mut truncated = false;
            if matches!(
                ext.as_str(),
                "sql" | "txt" | "md" | "yaml" | "yml" | "json" | "csv"
            ) {
                let raw = std::fs::read_to_string(&path).unwrap_or_default();
                if ext == "sql" {
                    let update_count = Regex::new(r"(?i)\bupdate\b")
                        .unwrap()
                        .find_iter(&raw)
                        .count();
                    let insert_count = Regex::new(r"(?i)\binsert\b")
                        .unwrap()
                        .find_iter(&raw)
                        .count();
                    let delete_count = Regex::new(r"(?i)\bdelete\b")
                        .unwrap()
                        .find_iter(&raw)
                        .count();
                    let alter_count = Regex::new(r"(?i)\balter\b")
                        .unwrap()
                        .find_iter(&raw)
                        .count();
                    if update_count > 0 {
                        summary.push(format!("{update_count} UPDATE"));
                    }
                    if insert_count > 0 {
                        summary.push(format!("{insert_count} INSERT"));
                    }
                    if delete_count > 0 {
                        summary.push(format!("{delete_count} DELETE"));
                    }
                    if alter_count > 0 {
                        summary.push(format!("{alter_count} ALTER"));
                    }
                }
                if raw.len() as u64 <= MAX_EMBED_BYTES {
                    body = raw;
                } else {
                    let (excerpt, _) = truncate_chars(&raw, MAX_EMBED_BYTES as usize);
                    body = format!("{}\n…（文件过大，已截断）", excerpt.trim_end());
                    truncated = true;
                }
            }
            files.push(FileRow {
                filename,
                ext,
                summary,
                size: meta.len(),
                mtime: system_time_to_ms(meta.modified().unwrap_or(UNIX_EPOCH)),
                path: path.to_string_lossy().to_string(),
                body,
                truncated,
            });
        }
    }
    files.sort_by(|a, b| a.filename.cmp(&b.filename));
    if files.is_empty() {
        return "<p class=\"empty\">暂无 attachments/ 附件。</p>".to_string();
    }
    let mut out = String::from("<div class=\"attachment-list\"><h3>附件清单</h3><table><thead><tr><th>文件</th><th>类型/统计</th><th>大小</th><th>更新时间</th><th>路径</th></tr></thead><tbody>");
    for f in &files {
        let summary = if f.summary.is_empty() {
            html_escape(&f.ext)
        } else {
            f.summary
                .iter()
                .map(|s| html_escape(s))
                .collect::<Vec<_>>()
                .join(" / ")
        };
        out.push_str(&format!(
            "<tr><td><code>{}</code></td><td>{}</td><td>{}</td><td>{}</td><td><code>{}</code></td></tr>",
            html_escape(&f.filename),
            summary,
            human_bytes(f.size),
            format_ms(f.mtime),
            html_escape(&f.path)
        ));
    }
    out.push_str("</tbody></table></div>");
    out.push_str("<div class=\"attachment-files\">");
    for (idx, f) in files.iter().enumerate() {
        let summary = if f.summary.is_empty() {
            html_escape(&f.ext)
        } else {
            f.summary
                .iter()
                .map(|s| html_escape(s))
                .collect::<Vec<_>>()
                .join(" / ")
        };
        let badge = if f.truncated {
            "<span class=\"badge badge-truncated\">已截断</span>".to_string()
        } else if !f.body.is_empty() {
            String::new()
        } else {
            "<span class=\"badge badge-missing\">二进制/不可预览</span>".to_string()
        };
        out.push_str(&format!(
            "<details class=\"attachment-file\"><summary><span class=\"attachment-name\">{}</span> <span class=\"attachment-badge\">{}</span> <span class=\"attachment-size\">{}</span> {}</summary><div class=\"attachment-file-body\"><div class=\"attachment-file-actions\"><button type=\"button\" class=\"token-action attachment-file-copy\" data-idx=\"{}\">一键复制</button><code class=\"attachment-path\">{}</code></div><pre>{}</pre><textarea class=\"attachment-file-source\" id=\"attachment-source-{}\" readonly>{}</textarea></div></details>",
            html_escape(&f.filename),
            summary,
            human_bytes(f.size),
            badge,
            idx,
            html_escape(&f.path),
            if f.body.is_empty() {
                "该文件为二进制或不可预览内容，请按路径打开核对。".to_string()
            } else {
                html_escape(&f.body)
            },
            idx,
            html_escape(&f.body)
        ));
    }
    out.push_str("</div>");
    out
}
