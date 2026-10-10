//! Diff 页面展开上下文用的文件行切片 API：按仓库目录 + commit + 文件路径
//! 返回指定行区间内容与总行数，供前端把 diff 中被折叠的未修改区域展开显示。

use std::path::Path;

use axum::{extract::{Query, State}, Json};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::*;

/// 单次返回的最大行数：防止把整个超大文件一次性塞进响应。
const FILE_LINES_MAX_PER_REQUEST: usize = 2_500;
/// `git show` 输出上限（字节）：超过按行边界截断，totalLines 只统计已取回部分。
const FILE_LINES_OUTPUT_LIMIT: usize = 4_000_000;

#[derive(Debug, Deserialize)]
pub(crate) struct FileLinesQuery {
    /// 仓库本地目录（来自差异快照 repo.projectPath）
    #[serde(alias = "projectPath")]
    project_path: String,
    /// 目标 commit（差异快照 repo.targetCommit / baseCommit）
    commit: String,
    /// 仓库相对文件路径
    path: String,
    /// 1-based 起始行（含），缺省 1
    from: Option<usize>,
    /// 1-based 结束行（含），缺省 from + MAX - 1
    to: Option<usize>,
}

/// GET /api/git/file-lines
/// 返回 `{ ok, path, commit, from, to, totalLines, binary, outputTruncated, lines }`。
pub(crate) async fn api_git_file_lines(
    State(_state): State<AppState>,
    Query(query): Query<FileLinesQuery>,
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

    // commit 先经 rev-parse 验证，避免任意字符串（如选项注入形态）直接进入 show 参数。
    let commit = query.commit.trim();
    let resolved = resolve_commit(repo_dir, commit).await;
    let commit_sha = resolved
        .as_str()
        .ok_or_else(|| ApiError::bad_request(format!("commit 无法解析: {commit}")))?
        .to_string();

    let from = query.from.unwrap_or(1).max(1);
    let to = query
        .to
        .unwrap_or(from + FILE_LINES_MAX_PER_REQUEST - 1)
        .max(from);
    if to - from + 1 > FILE_LINES_MAX_PER_REQUEST {
        return Err(ApiError::bad_request(format!(
            "行区间过大：单次最多 {FILE_LINES_MAX_PER_REQUEST} 行"
        )));
    }

    let spec = format!("{commit_sha}:{}", unescape_repo_path(file_path));
    let shown = git(repo_dir, &["show", spec.as_str()], 30_000, FILE_LINES_OUTPUT_LIMIT).await;
    if !shown.ok {
        return Err(ApiError::bad_request(short_err(&shown)));
    }
    if is_binary_content(&shown.stdout) {
        return Ok(Json(json!({
            "ok": true,
            "path": file_path,
            "commit": commit_sha,
            "from": from,
            "to": to,
            "totalLines": 0,
            "binary": true,
            "outputTruncated": shown.output_truncated,
            "lines": Vec::<String>::new(),
        })));
    }
    let all_lines: Vec<&str> = shown.stdout.split('\n').collect();
    // "a\nb\n" split 后末尾多一个空串，不属于文件内容。
    let total_lines = if all_lines.last().is_some_and(|l| l.is_empty()) {
        all_lines.len() - 1
    } else {
        all_lines.len()
    };
    let start = (from - 1).min(total_lines);
    let end = to.min(total_lines);
    let lines: Vec<String> = all_lines[start..end]
        .iter()
        .map(|l| (*l).to_string())
        .collect();
    Ok(Json(json!({
        "ok": true,
        "path": file_path,
        "commit": commit_sha,
        "from": start + 1,
        "to": end,
        "totalLines": total_lines,
        "binary": false,
        "outputTruncated": shown.output_truncated,
        "lines": lines,
    })))
}

/// 拒绝绝对路径、`..` 上跳、空路径与控制字符，只允许仓库相对路径。
pub(crate) fn validate_repo_relative_path(path: &str) -> Result<(), ApiError> {
    if path.is_empty() {
        return Err(ApiError::bad_request("path 不能为空".to_string()));
    }
    if path.starts_with('/') || path.starts_with('\\') || path.contains('\0') {
        return Err(ApiError::bad_request(format!("非法文件路径: {path}")));
    }
    let windows_drive = path.as_bytes().get(1) == Some(&b':');
    if windows_drive {
        return Err(ApiError::bad_request(format!("非法文件路径: {path}")));
    }
    if path.split(['/', '\\']).any(|seg| seg == "..") {
        return Err(ApiError::bad_request(format!("非法文件路径: {path}")));
    }
    Ok(())
}

/// 兼容 core.quotepath 关闭后前端传来的正常 UTF-8 路径；占位实现，保留转义扩展位。
fn unescape_repo_path(path: &str) -> String {
    path.to_string()
}

/// 前 8KB 出现 NUL 视为二进制内容，不展示也不折叠。
fn is_binary_content(content: &str) -> bool {
    let probe = content.as_bytes();
    let len = probe.len().min(8192);
    probe[..len].contains(&0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_repo_relative_path_rejects_escape_and_absolute() {
        assert!(validate_repo_relative_path("src/main/java/A.java").is_ok());
        assert!(validate_repo_relative_path("src/中文/目录.rs").is_ok());
        assert!(validate_repo_relative_path("").is_err());
        assert!(validate_repo_relative_path("/etc/passwd").is_err());
        assert!(validate_repo_relative_path("C:\\win").is_err());
        assert!(validate_repo_relative_path("src/../../etc/passwd").is_err());
        assert!(validate_repo_relative_path("a\0b").is_err());
    }

    #[test]
    fn is_binary_content_detects_nul() {
        assert!(!is_binary_content("plain text\nlines\n"));
        assert!(is_binary_content("text\0binary"));
    }
}
