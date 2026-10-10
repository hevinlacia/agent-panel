//! Diff 页面行级 blame API：按仓库目录 + commit + 文件路径返回每行的修改人与修改日期，
//! 供代码差异页「行信息」按钮展示（再点收起，数据由前端保留）。

use std::collections::HashMap;
use std::path::Path;

use axum::{extract::{Query, State}, Json};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::*;

/// `git blame --line-porcelain` 输出上限（字节）：超大文件按行边界截断，
/// 响应带 truncated 标志，前端对缺失行留空即可。
const BLAME_OUTPUT_LIMIT: usize = 8_000_000;

#[derive(Debug, Deserialize)]
pub(crate) struct GitBlameQuery {
    /// 仓库本地目录（来自差异快照 repo.projectPath）
    #[serde(alias = "projectPath")]
    project_path: String,
    /// 目标 commit（差异快照 repo.targetCommit）
    commit: String,
    /// 仓库相对文件路径
    path: String,
}

/// GET /api/git/blame
/// 返回 `{ ok, path, commit, totalLines, truncated, lines: [{ no, author, date }] }`。
pub(crate) async fn api_git_blame(
    State(_state): State<AppState>,
    Query(query): Query<GitBlameQuery>,
) -> ApiResult<Json<Value>> {
    let repo_dir = Path::new(query.project_path.trim());
    // projectPath 必须是本地 git 仓库目录；防快照数据被改后指向任意路径。
    if !repo_dir.is_dir() || !repo_dir.join(".git").exists() {
        return Err(ApiError::bad_request(format!(
            "projectPath 不是本地 git 仓库目录: {}",
            query.project_path.trim()
        )));
    }
    let file_path = query.path.trim();
    validate_repo_relative_path(file_path)?;

    // commit 先经 rev-parse 验证，避免任意字符串直接进入 blame 参数。
    let commit = query.commit.trim();
    let resolved = resolve_commit(repo_dir, commit).await;
    let commit_sha = resolved
        .as_str()
        .ok_or_else(|| ApiError::bad_request(format!("commit 无法解析: {commit}")))?
        .to_string();

    let blamed = git(
        repo_dir,
        &["blame", "--line-porcelain", &commit_sha, "--", file_path],
        60_000,
        BLAME_OUTPUT_LIMIT,
    )
    .await;
    if !blamed.ok {
        return Err(ApiError::bad_request(short_err(&blamed)));
    }

    // porcelain 解析：header 行（<sha> <orig> <final> [count]）开启一条记录；
    // author/author-time 元数据只在 sha 首次出现时输出，缓存到 map；"\t" 行 = 一条文件行。
    let mut authors: HashMap<String, (String, String)> = HashMap::new();
    let mut cur_sha = String::new();
    let mut cur_final: usize = 0;
    let mut pending_author = String::new();
    let mut pending_time = String::new();
    let mut lines: Vec<Value> = Vec::new();
    for raw in blamed.stdout.lines() {
        if let Some(content) = raw.strip_prefix('\t') {
            let _ = content;
            let entry = authors
                .entry(cur_sha.clone())
                .or_insert_with(|| (pending_author.clone(), blame_date(&pending_time)));
            lines.push(json!({ "no": cur_final, "author": entry.0, "date": entry.1 }));
            continue;
        }
        let bytes = raw.as_bytes();
        let is_header = bytes.len() >= 41
            && bytes[..40].iter().all(|b| b.is_ascii_hexdigit())
            && bytes[40] == b' ';
        if is_header {
            let mut parts = raw.split_whitespace();
            cur_sha = parts.next().unwrap_or("").to_string();
            let _orig = parts.next();
            cur_final = parts.next().and_then(|v| v.parse().ok()).unwrap_or(0);
            pending_author.clear();
            pending_time.clear();
            continue;
        }
        if let Some(v) = raw.strip_prefix("author ") {
            pending_author = v.to_string();
        } else if let Some(v) = raw.strip_prefix("author-time ") {
            pending_time = v.trim().to_string();
        }
    }

    Ok(Json(json!({
        "ok": true,
        "path": file_path,
        "commit": commit_sha,
        "totalLines": lines.len(),
        "truncated": blamed.output_truncated,
        "lines": lines,
    })))
}

/// author-time（unix 秒）→ 本地时区 YYYY-MM-DD；解析失败返回空串。
fn blame_date(unix_secs: &str) -> String {
    let secs: i64 = unix_secs.parse().unwrap_or(0);
    if secs <= 0 {
        return String::new();
    }
    chrono::DateTime::from_timestamp(secs, 0)
        .map(|d| d.with_timezone(&chrono::Local).format("%Y-%m-%d").to_string())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blame_date_formats_local_ymd() {
        // 固定时间戳 2026-01-15 00:00:00 UTC → 任意本地时区日期非空且形如 YYYY-MM-DD。
        let d = blame_date("1768435200");
        assert_eq!(d.len(), 10);
        assert_eq!(d.as_bytes()[4], b'-');
        assert_eq!(blame_date("0"), "");
        assert_eq!(blame_date("abc"), "");
    }

    #[test]
    fn porcelain_header_detection_requires_sha_and_space() {
        let header = "a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2 12 15 4";
        let bytes = header.as_bytes();
        assert!(bytes.len() >= 41 && bytes[..40].iter().all(|b| b.is_ascii_hexdigit()) && bytes[40] == b' ');
        let meta = "author 张三";
        let mbytes = meta.as_bytes();
        assert!(!(mbytes.len() >= 41 && mbytes[..40].iter().all(|b| b.is_ascii_hexdigit())));
    }
}
