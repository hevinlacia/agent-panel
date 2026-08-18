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
    get_real_requirement, system_time_to_ms, truncate_chars, ApiResult, AppState, IdQuery,
};

pub(crate) async fn api_attachments(
    State(state): State<AppState>,
    Query(query): Query<IdQuery>,
) -> ApiResult<Json<Value>> {
    let id = query.id.or(query.req_id).unwrap_or_default();
    let req = get_real_requirement(&state, &id).await?;
    let dir = PathBuf::from(req.req_dir.unwrap_or_default());
    let rows = requirement_attachment_rows(&dir, query.budget.unwrap_or(600)).await;
    Ok(Json(json!({ "attachments": rows })))
}

pub(crate) async fn requirement_attachment_rows(dir: &Path, sample_budget: usize) -> Vec<Value> {
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
                let (excerpt, truncated) = truncate_chars(&raw, sample_budget.min(1200));
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
    let rows = requirement_attachment_rows(dir, sample_budget).await;
    if rows.is_empty() {
        return "# 非代码附件\n\n- 暂无 attachments/ 附件。\n".to_string();
    }
    let mut out = String::from("# 非代码附件 / Release Attachments\n\n> 这些是需求目录 attachments/ 下的非代码发版资产，发版前应与上线清单一并核对。\n\n| 文件 | 类型/统计 | 大小 | 更新时间 | 路径 |\n| --- | --- | ---: | --- | --- |\n");
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
