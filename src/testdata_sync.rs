//! testdata pack hub 侧文件 API（REQ-169）：pack 真源在 agent-panel-data 数据仓，
//! hub 为 SSOT，编辑走本模块端点（sha1 乐观锁）；worker 执行前按 manifest hash
//! 差量物化到本地数据仓 clone（见 snapshot_sync 的 sync 入口，阶段 2）。
//!
//! - GET  /api/testdata/manifest      全量文件清单（path/sha1/size），物化对比用
//! - GET  /api/testdata/file?path=    单文件读（content + sha1）
//! - PUT  /api/testdata/file          单文件写（乐观锁；路径白名单 + 后缀白名单）
//!
//! pack 根解析（双模式同一套 handler）：按 requirementScanRoots 找 `<root>/.agents/testdata`
//! （hub 命中数据仓 /srv/agent-panel/data/company/云仓/WMS/...；worker 配置含工作区根
//! 时命中工作区 pack，Phase 3 symlink 后与数据仓 clone 等价）；均未命中时兜底 devpath
//! 工作区 pack。node_modules/.env 等排除规则与 pack 自带 .gitignore 对齐。

use crate::http::{ApiError, ApiResult, FormOrJson};
use crate::AppState;

use axum::extract::{Query, State};
use axum::Json;
use serde::Deserialize;
use serde_json::json;
use std::path::PathBuf;

/// 清单/读写排除目录（与 pack .gitignore 对齐；node_modules 由本机安装，不入 SSOT）。
const EXCLUDED_DIRS: &[&str] = &[
    "node_modules",
    "__pycache__",
    ".git",
    ".dagu",
    "reports",
    "target",
];
/// 排除文件名/模式：登录凭证类与环境文件永不入 hub/manifest。
const EXCLUDED_FILE_PREFIXES: &[&str] = &[".env"];
const EXCLUDED_FILE_SUFFIXES: &[&str] = &[".log"];
/// 允许读写的文本后缀（pack 全部为文本；白名单兜底防意外二进制）。
const ALLOWED_EXTENSIONS: &[&str] = &[
    "yaml", "yml", "json", "md", "py", "mjs", "js", "sql", "bru", "txt", "toml", "cfg", "example",
];
/// manifest 条目上限（防失控目录打爆响应；当前 pack 254 文件，余量充足）。
const MANIFEST_MAX_ENTRIES: usize = 5000;

/// 解析 testdata pack 根：scan roots 里第一个存在 `<root>/.agents/testdata` 的候选优先，
/// 其次 devpath 工作区 pack。都找不到时 400（未迁移或配置缺失）。
pub(crate) async fn testdata_pack_root(state: &AppState) -> ApiResult<PathBuf> {
    let cfg = crate::config::read_config(state).await?;
    let mut candidates: Vec<PathBuf> = Vec::new();
    for root in crate::config::normalize_scan_roots(cfg.requirement_scan_roots) {
        candidates.push(PathBuf::from(root).join(".agents").join("testdata"));
    }
    candidates.push(crate::paths::wms_testdata_pack_root());
    let mut seen = std::collections::HashSet::new();
    for cand in candidates {
        let key = cand.to_string_lossy().to_string();
        if seen.insert(key) && cand.is_dir() {
            return Ok(cand);
        }
    }
    Err(ApiError::bad_request(
        "testdata pack 不存在：数据仓与工作区均未找到 .agents/testdata（先完成 REQ-169 阶段 1 迁移或检查 requirementScanRoots）",
    ))
}

/// pack 内相对路径白名单：拒绝穿越/排除目录/隐藏与不允许后缀；返回规范化 rel。
fn resolve_pack_rel_path(input: &str) -> ApiResult<String> {
    let v = input.trim().trim_start_matches("./");
    if v.is_empty() || v.starts_with('/') || v.split('/').any(|s| s == ".." || s.is_empty()) {
        return Err(ApiError::bad_request(format!(
            "非法文件路径：{input}（只允许 pack 内相对路径）"
        )));
    }
    let segs: Vec<&str> = v.split('/').collect();
    for (i, seg) in segs.iter().enumerate() {
        if i < segs.len() - 1 && EXCLUDED_DIRS.contains(seg) {
            return Err(ApiError::bad_request(format!(
                "路径命中排除目录 {seg}（依赖目录由本机安装，不入 SSOT）：{input}"
            )));
        }
        if seg.starts_with('.') {
            return Err(ApiError::bad_request(format!(
                "不允许隐藏文件/目录：{input}"
            )));
        }
    }
    let file = segs
        .last()
        .copied()
        .ok_or_else(|| ApiError::bad_request("空路径"))?;
    if EXCLUDED_FILE_PREFIXES.iter().any(|p| file.starts_with(p))
        || EXCLUDED_FILE_SUFFIXES.iter().any(|s| file.ends_with(s))
    {
        return Err(ApiError::bad_request(format!(
            "文件命中排除规则（凭证/环境/日志不入 SSOT）：{input}"
        )));
    }
    let ext = file
        .rsplit_once('.')
        .map(|(_, e)| e.to_ascii_lowercase())
        .unwrap_or_default();
    if !ALLOWED_EXTENSIONS.contains(&ext.as_str()) {
        return Err(ApiError::bad_request(format!(
            "不支持的文件后缀 .{ext}（允许：{}）",
            ALLOWED_EXTENSIONS.join("/")
        )));
    }
    Ok(v.to_string())
}

/// 文件是否应入 manifest：任一目录段命中排除目录/隐藏段即排除；文件再按后缀/排除名过滤。
fn manifest_included(rel: &str, is_dir: bool) -> bool {
    let segs: Vec<&str> = rel.split('/').collect();
    let (dir_segs, file) = if is_dir {
        (segs.as_slice(), "")
    } else {
        let n = segs.len();
        (
            &segs[..n.saturating_sub(1)],
            segs.last().copied().unwrap_or_default(),
        )
    };
    if dir_segs
        .iter()
        .any(|s| EXCLUDED_DIRS.contains(s) || s.starts_with('.'))
    {
        return false;
    }
    if is_dir {
        return true;
    }
    if EXCLUDED_FILE_PREFIXES.iter().any(|p| file.starts_with(p))
        || EXCLUDED_FILE_SUFFIXES.iter().any(|s| file.ends_with(s))
    {
        return false;
    }
    let ext = file
        .rsplit_once('.')
        .map(|(_, e)| e.to_ascii_lowercase())
        .unwrap_or_default();
    ALLOWED_EXTENSIONS.contains(&ext.as_str())
}

/// GET /api/testdata/manifest：全量文件清单（升序），worker 物化与一致性对比用。
pub(crate) async fn api_testdata_manifest_get(
    State(state): State<AppState>,
) -> ApiResult<Json<serde_json::Value>> {
    let root = testdata_pack_root(&state).await?;
    let mut files = Vec::<serde_json::Value>::new();
    let mut stack = vec![root.clone()];
    while let Some(dir) = stack.pop() {
        let mut entries = match tokio::fs::read_dir(&dir).await {
            Ok(v) => v,
            Err(_) => continue,
        };
        while let Ok(Some(entry)) = entries.next_entry().await {
            let path = entry.path();
            let rel = path
                .strip_prefix(&root)
                .unwrap_or(&path)
                .to_string_lossy()
                .to_string();
            if rel.is_empty() {
                continue;
            }
            let is_dir = path.is_dir();
            if !manifest_included(&rel, is_dir) {
                continue;
            }
            if is_dir {
                stack.push(path.clone());
                continue;
            }
            if files.len() >= MANIFEST_MAX_ENTRIES {
                return Err(ApiError::bad_request(format!(
                    "manifest 超过 {MANIFEST_MAX_ENTRIES} 条上限：pack 异常膨胀，先人工排查"
                )));
            }
            let content = match tokio::fs::read_to_string(&path).await {
                Ok(v) => v,
                Err(_) => continue, // 非文本/暂不可读：跳过（白名单后缀下不应出现）
            };
            files.push(json!({
                "path": rel,
                "sha1": crate::requirement_api::sha1_hex(&content),
                "size": content.len(),
            }));
        }
    }
    files.sort_by(|a, b| {
        a.get("path")
            .and_then(|v| v.as_str())
            .cmp(&b.get("path").and_then(|v| v.as_str()))
    });
    Ok(Json(json!({
        "ok": true,
        "fileCount": files.len(),
        "files": files,
        "generatedAt": crate::util::now_ms(),
    })))
}

#[derive(Debug, Deserialize)]
pub(crate) struct TestdataFileReadQuery {
    pub(crate) path: Option<String>,
}

/// GET /api/testdata/file?path=api/catalog.yaml：单文件读（不存在时 exists=false + sha=null）。
pub(crate) async fn api_testdata_file_get(
    State(state): State<AppState>,
    Query(query): Query<TestdataFileReadQuery>,
) -> ApiResult<Json<serde_json::Value>> {
    let rel = resolve_pack_rel_path(query.path.as_deref().unwrap_or_default())?;
    let root = testdata_pack_root(&state).await?;
    let path = root.join(&rel);
    let (exists, content) = match tokio::fs::read_to_string(&path).await {
        Ok(text) => (true, text),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => (false, String::new()),
        Err(e) => return Err(ApiError::from(e)),
    };
    let sha = if exists {
        Some(crate::requirement_api::sha1_hex(&content))
    } else {
        None
    };
    Ok(Json(json!({
        "ok": true,
        "path": rel,
        "exists": exists,
        "content": content,
        "sha": sha.clone(),
        "baseSha": sha,
        "size": content.len(),
    })))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TestdataFileWriteForm {
    pub(crate) path: String,
    pub(crate) content: String,
    /// GET 返回的内容 sha1（乐观锁）。省略 = 仅允许新建（文件已存在则 409）。
    /// 兼容别名 `sha`（与 requirement file API 同约定）。
    #[serde(alias = "sha")]
    pub(crate) base_sha: Option<String>,
}

/// PUT /api/testdata/file：单文件写（乐观锁）。目录不存在时自动创建（如新增 recipes/<x>/）。
pub(crate) async fn api_testdata_file_put(
    State(state): State<AppState>,
    FormOrJson(form): FormOrJson<TestdataFileWriteForm>,
) -> ApiResult<Json<serde_json::Value>> {
    let rel = resolve_pack_rel_path(&form.path)?;
    let root = testdata_pack_root(&state).await?;
    let path = root.join(&rel);
    let current = match tokio::fs::read_to_string(&path).await {
        Ok(text) => Some(text),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(ApiError::from(e)),
    };
    let current_sha = current.as_deref().map(crate::requirement_api::sha1_hex);
    let base_sha = form
        .base_sha
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_ascii_lowercase());
    let empty_content_sha = crate::requirement_api::sha1_hex("");
    match (&current_sha, &base_sha) {
        (Some(cur), Some(base)) if cur != base => {
            return Err(ApiError::conflict(format!(
                "文件已被修改（当前 sha {cur}，请求基于 {base}）；重新 GET /api/testdata/file?path={} 获取最新内容与 sha 后再写",
                rel
            )));
        }
        (Some(_), None) => {
            return Err(ApiError::conflict(
                "文件已存在：更新必须携带 GET 返回的 baseSha；如需覆盖请先 GET",
            ));
        }
        (None, Some(base)) if *base == empty_content_sha => {}
        (None, Some(_)) => {
            return Err(ApiError::conflict(
                "文件不存在（可能已被删除）：传回的 baseSha 不匹配；新建请省略 baseSha",
            ));
        }
        // sha 相等的更新、以及新建（无 baseSha）：放行
        _ => {}
    }
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    crate::util::atomic_write_text(&path, &form.content).await?;
    Ok(Json(json!({
        "ok": true,
        "path": rel,
        "sha": crate::requirement_api::sha1_hex(&form.content),
        "size": form.content.len(),
        "receivedAt": crate::util::now_ms(),
    })))
}

// ---------- worker 物化（hub SSOT → 本地 pack，REQ-169 阶段 2） ----------

/// 本地同步水位文件：记录每个文件上次同步成功的 hub sha。冲突判定依据：
/// 本地 sha == 水位 → 本地未被手改，可安全覆盖；水位缺失/不匹配 → 视为本地改动，报冲突。
const SYNC_STATE_FILE: &str = ".testdata-sync-state.json";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TestdataSyncForm {
    /// 强制覆盖：忽略本地改动，以 hub 为准重建（含删除多余文件）。
    pub(crate) force: Option<bool>,
}

type ShaMap = std::collections::BTreeMap<String, String>;

async fn load_sync_state(root: &std::path::Path) -> ShaMap {
    tokio::fs::read_to_string(root.join(SYNC_STATE_FILE))
        .await
        .ok()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .and_then(|doc| {
            let map = doc.get("files")?.clone();
            serde_json::from_value::<ShaMap>(map).ok()
        })
        .unwrap_or_default()
}

async fn save_sync_state(root: &std::path::Path, files: &ShaMap) {
    let doc = json!({ "version": 1, "updatedAt": crate::util::now_ms(), "files": files });
    if let Ok(text) = serde_json::to_string_pretty(&doc) {
        crate::util::atomic_write_text(&root.join(SYNC_STATE_FILE), &text)
            .await
            .ok();
    }
}

/// 本地 pack 的 sha 表（与 manifest 同过滤规则；水位文件本身除外）。
async fn local_sha_map(root: &std::path::Path) -> ShaMap {
    let mut out = ShaMap::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(mut entries) = tokio::fs::read_dir(&dir).await else {
            continue;
        };
        while let Ok(Some(entry)) = entries.next_entry().await {
            let path = entry.path();
            let Ok(rel) = path.strip_prefix(root) else {
                continue;
            };
            let rel = rel.to_string_lossy().to_string();
            if rel.is_empty() || rel == SYNC_STATE_FILE {
                continue;
            }
            let is_dir = path.is_dir();
            if !manifest_included(&rel, is_dir) {
                continue;
            }
            if is_dir {
                stack.push(path);
                continue;
            }
            if let Ok(content) = tokio::fs::read_to_string(&path).await {
                out.insert(rel, crate::requirement_api::sha1_hex(&content));
            }
        }
    }
    out
}

async fn fetch_hub_manifest(hub: &str) -> Result<ShaMap, String> {
    let mut request = reqwest::Client::new().get(format!("{hub}/api/testdata/manifest"));
    if let Ok(token) = std::env::var("PANEL_WORKER_TOKEN") {
        if !token.is_empty() {
            request = request.header("X-Worker-Token", token);
        }
    }
    let resp = request
        .send()
        .await
        .map_err(|e| format!("manifest 拉取失败：{e}"))?;
    if !resp.status().is_success() {
        return Err(format!(
            "manifest 拉取失败（HTTP {}）",
            resp.status().as_u16()
        ));
    }
    let doc: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("manifest 解析失败：{e}"))?;
    let files = doc
        .get("files")
        .and_then(|v| v.as_array())
        .ok_or_else(|| "manifest 响应缺少 files".to_string())?;
    let mut map = ShaMap::new();
    for f in files {
        if let (Some(p), Some(s)) = (
            f.get("path").and_then(|v| v.as_str()),
            f.get("sha1").and_then(|v| v.as_str()),
        ) {
            map.insert(p.to_string(), s.to_string());
        }
    }
    Ok(map)
}

async fn fetch_hub_file(hub: &str, path: &str) -> Result<String, String> {
    let mut request = reqwest::Client::new().get(format!(
        "{hub}/api/testdata/file?path={}",
        encode_pack_path(path)
    ));
    if let Ok(token) = std::env::var("PANEL_WORKER_TOKEN") {
        if !token.is_empty() {
            request = request.header("X-Worker-Token", token);
        }
    }
    let resp = request
        .send()
        .await
        .map_err(|e| format!("文件拉取失败 {path}：{e}"))?;
    if !resp.status().is_success() {
        return Err(format!(
            "文件拉取失败 {path}（HTTP {}）",
            resp.status().as_u16()
        ));
    }
    let doc: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("文件解析失败 {path}：{e}"))?;
    doc.get("content")
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .ok_or_else(|| format!("文件响应缺少 content：{path}"))
}

/// 物化核心（可注入 hub/root 便于测试）：按 manifest 差量更新本地 pack。
/// 冲突语义：本地 sha 既不等于 hub sha 也不等于上次同步水位（=本地被手改）→ 跳过并记冲突。
pub(crate) async fn sync_testdata_from_hub_with(
    hub: &str,
    root: &std::path::Path,
    force: bool,
) -> Result<serde_json::Value, String> {
    let hub_map = fetch_hub_manifest(hub).await?;
    let local = local_sha_map(root).await;
    let mut state_map = load_sync_state(root).await;
    let mut updated = Vec::<String>::new();
    let mut deleted = Vec::<String>::new();
    let mut conflicts = Vec::<serde_json::Value>::new();
    let mut warnings = Vec::<String>::new();
    for (path, hub_sha) in &hub_map {
        let local_sha = local.get(path);
        if local_sha.map(String::as_str) == Some(hub_sha.as_str()) {
            state_map.insert(path.clone(), hub_sha.clone());
            continue;
        }
        let unchanged_since_sync = match local_sha {
            Some(l) => state_map.get(path).map(String::as_str) == Some(l.as_str()),
            None => true, // 本地缺失：视为无改动，直接拉
        };
        if !force && !unchanged_since_sync {
            conflicts.push(json!({
                "path": path,
                "localSha": local_sha.clone(),
                "hubSha": hub_sha,
                "hint": "本地文件被手改且未走 hub API；确认后 POST /api/testdata/sync force=true 覆盖，或把改动经 PUT /api/testdata/file 提交 hub",
            }));
            continue;
        }
        let content = fetch_hub_file(hub, path).await?;
        let target = root.join(path);
        if let Some(parent) = target.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| format!("创建目录失败 {path}：{e}"))?;
        }
        crate::util::atomic_write_text(&target, &content)
            .await
            .map_err(|e| format!("写入失败 {path}：{e}"))?;
        state_map.insert(path.clone(), hub_sha.clone());
        updated.push(path.clone());
    }
    // hub 已删除的文件：本地未被手改（水位匹配）时跟随删除；否则保留并告警。
    for (path, local_sha) in &local {
        if hub_map.contains_key(path) {
            continue;
        }
        let unchanged_since_sync =
            state_map.get(path).map(String::as_str) == Some(local_sha.as_str());
        if force || unchanged_since_sync {
            tokio::fs::remove_file(root.join(path))
                .await
                .map_err(|e| format!("删除失败 {path}：{e}"))?;
            state_map.remove(path);
            deleted.push(path.clone());
        } else {
            warnings.push(format!("hub 已删除但本地有改动，保留：{path}"));
        }
    }
    save_sync_state(root, &state_map).await;
    Ok(json!({
        "ok": conflicts.is_empty(),
        "updated": updated,
        "deleted": deleted,
        "conflicts": conflicts,
        "warnings": warnings,
        "hubFileCount": hub_map.len(),
    }))
}

/// worker 便捷入口：读 PANEL_HUB_URL；未配置（docs/纯本机）返回 Ok(None) 表示无需物化。
pub(crate) async fn sync_testdata_from_hub(
    state: &AppState,
    force: bool,
) -> Result<Option<serde_json::Value>, String> {
    let hub = std::env::var("PANEL_HUB_URL").unwrap_or_default();
    if hub.is_empty() {
        return Ok(None);
    }
    let root = testdata_pack_root(state)
        .await
        .map_err(|e| e.message.clone())?;
    sync_testdata_from_hub_with(&hub, &root, force)
        .await
        .map(Some)
}

/// 执行前新鲜度保障：run/trigger 前调用。冲突（本地手改未提交）时返回 Err 阻断执行，
/// 同步失败（hub 不可达等）仅返回 warning 字符串不阻断（离线兜底本地副本执行）。
pub(crate) async fn ensure_local_pack_fresh(state: &AppState) -> Result<Option<String>, String> {
    match sync_testdata_from_hub(state, false).await {
        Ok(None) => Ok(None),
        Ok(Some(doc)) => {
            let conflicts = doc.get("conflicts").and_then(|v| v.as_array());
            if let Some(list) = conflicts.filter(|l| !l.is_empty()) {
                let paths: Vec<String> = list
                    .iter()
                    .filter_map(|c| c.get("path").and_then(|v| v.as_str()).map(str::to_string))
                    .collect();
                return Err(format!(
                    "testdata pack 本地副本与 hub 存在未同步冲突：{}；先 PUT /api/testdata/file 提交改动或 POST /api/testdata/sync force=true 放弃本地改动",
                    paths.join("、")
                ));
            }
            let warnings = doc.get("warnings").and_then(|v| v.as_array()).map(|list| {
                list.iter()
                    .filter_map(|w| w.as_str())
                    .collect::<Vec<_>>()
                    .join("；")
            });
            Ok(warnings.filter(|w| !w.is_empty()))
        }
        Err(e) => Ok(Some(format!(
            "testdata pack 同步失败（使用本地副本执行）：{e}"
        ))),
    }
}

/// POST /api/testdata/sync：显式触发物化（worker 执行；hub 自身即 SSOT，返回 skipped）。
pub(crate) async fn api_testdata_sync_post(
    State(state): State<AppState>,
    FormOrJson(form): FormOrJson<TestdataSyncForm>,
) -> ApiResult<Json<serde_json::Value>> {
    match sync_testdata_from_hub(&state, form.force.unwrap_or(false)).await {
        Ok(None) => Ok(Json(json!({
            "ok": true,
            "skipped": "当前进程即 SSOT（docs 模式或未配置 PANEL_HUB_URL），无需物化",
        }))),
        Ok(Some(doc)) => Ok(Json(doc)),
        Err(e) => Err(ApiError::bad_request(format!("testdata 同步失败：{e}"))),
    }
}

/// pack 相对路径的 query 百分号编码（路径段安全字符集：字母数字与 -_.~/）。
fn encode_pack_path(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    for b in path.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}
