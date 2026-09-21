use super::*;

/// 需求文档分册根目录：`<reqDir>/docs/<doc-base>/<NNN>-<slug>.md`。
pub(crate) const DOCS_DIR_NAME: &str = "docs";
/// 核心需求文档拆分告警阈值：超过该大小的 .md 文档在 validate 中提示拆分分册。
pub(crate) const DOC_SPLIT_WARN_BYTES: u64 = 40 * 1024;
/// 分册索引段标题：panel 在主文档末尾自动维护该段（创建分册时追加索引行）。
pub(crate) const DOC_PARTS_INDEX_HEADING: &str = "## 分册索引";

/// 从 docType 解析分册基名：`notes` -> `notes`，`technical-plan` -> `technical-plan`。
/// 主文档文件名（notes.md）与分册目录（docs/notes/）共享基名。
fn doc_base_name(doc_type: &str) -> ApiResult<String> {
    let doc_file = requirement_doc_file(doc_type)?;
    Ok(doc_file.trim_end_matches(".md").to_string())
}

/// 校验并规范化分册相对路径：仅接受 `docs/<base>/<NNN>-<slug>.md` 形态，
/// 拒绝路径穿越与非 docs 前缀；返回规范化后的相对路径。
pub(crate) fn resolve_doc_part_rel_path(input: &str) -> ApiResult<String> {
    let v = input.trim().trim_start_matches("./");
    let mut segs = v.split('/');
    let first = segs.next().unwrap_or_default();
    if first != DOCS_DIR_NAME {
        return Err(ApiError::bad_request(format!(
            "分册路径必须以 {DOCS_DIR_NAME}/<doc-base>/<file>.md 开头：{input}"
        )));
    }
    let base = ensure_safe_segment(segs.next().unwrap_or_default(), "docs/<doc-base>")?;
    let file = segs.next().unwrap_or_default();
    if segs.next().is_some() {
        return Err(ApiError::bad_request(format!("分册路径层级过深：{input}")));
    }
    if !file.ends_with(".md") || file == ".md" {
        return Err(ApiError::bad_request(format!(
            "分册文件必须是 .md 结尾：{input}"
        )));
    }
    ensure_safe_segment(file.trim_end_matches(".md"), "分册文件名")?;
    Ok(format!("{DOCS_DIR_NAME}/{base}/{file}"))
}

/// 分册信息（扫描/清单用）。`title` 读取分册首个 H1；`indexLinked` 表示主文档索引段是否已包含该分册链接。
#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DocPartInfo {
    pub(crate) doc_base: String,
    pub(crate) filename: String,
    /// 相对需求目录路径：docs/notes/001-slug.md
    pub(crate) rel_path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) title: Option<String>,
    pub(crate) bytes: u64,
    pub(crate) updated_at: i64,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub(crate) index_linked: bool,
}

async fn part_title(path: &Path) -> Option<String> {
    let raw = fs::read_to_string(path).await.ok()?;
    raw.lines()
        .find_map(|line| line.strip_prefix("# ").map(str::trim).map(str::to_string))
        .filter(|t| !t.is_empty())
}

async fn main_doc_links_index(main_path: &Path, rel_path: &str) -> bool {
    fs::read_to_string(main_path)
        .await
        .map(|raw| raw.contains(&format!("({rel_path})")))
        .unwrap_or(false)
}

/// 扫描需求目录下全部分册：遍历 docs/*/ 子目录中的 .md 文件。
/// `only_base` 传入时只返回该基名（如 "notes"）的分册。
pub(crate) async fn scan_doc_parts(
    dir: &Path,
    only_base: Option<&str>,
) -> Result<Vec<DocPartInfo>> {
    let docs_root = dir.join(DOCS_DIR_NAME);
    let mut out = Vec::new();
    let mut read_dirs = match fs::read_dir(&docs_root).await {
        Ok(rd) => rd,
        Err(_) => return Ok(out),
    };
    while let Some(entry) = read_dirs.next_entry().await? {
        if !entry.file_type().await?.is_dir() {
            continue;
        }
        let base = entry.file_name().to_string_lossy().to_string();
        if let Some(want) = only_base {
            if base != want {
                continue;
            }
        }
        let main_path = dir.join(format!("{base}.md"));
        let mut files = fs::read_dir(entry.path()).await?;
        while let Some(f) = files.next_entry().await? {
            let filename = f.file_name().to_string_lossy().to_string();
            if !filename.ends_with(".md") || !f.file_type().await?.is_file() {
                continue;
            }
            let meta = f.metadata().await?;
            let rel_path = format!("{DOCS_DIR_NAME}/{base}/{filename}");
            out.push(DocPartInfo {
                title: part_title(&f.path()).await,
                doc_base: base.clone(),
                filename,
                index_linked: main_doc_links_index(&main_path, &rel_path).await,
                bytes: meta.len(),
                updated_at: system_time_to_ms(meta.modified().unwrap_or(std::time::UNIX_EPOCH)),
                rel_path,
            });
        }
    }
    out.sort_by(|a, b| a.rel_path.cmp(&b.rel_path));
    Ok(out)
}

/// 在主文档末尾维护分册索引段：无 `## 分册索引` 段则创建，有则把索引行追加到文件末尾
/// （索引段约定位于主文档末尾，追加即入段）。
async fn append_index_entry(
    main_path: &Path,
    req_id: &str,
    doc_file: &str,
    entry: &str,
) -> Result<()> {
    let raw = match fs::read_to_string(main_path).await {
        Ok(raw) => raw.replace("\r\n", "\n"),
        Err(_) => format!("# {req_id} {doc_file}\n"),
    };
    let trimmed = raw.trim_end();
    let next = if trimmed.contains(DOC_PARTS_INDEX_HEADING) {
        format!("{trimmed}\n{entry}\n")
    } else {
        format!("{trimmed}\n\n{DOC_PARTS_INDEX_HEADING}\n\n{entry}\n")
    };
    atomic_write_text(main_path, &next).await
}

/// 创建文档分册：写 `docs/<base>/<NNN>-<slug>.md`，并把索引行追加到主文档。
/// 分册序号在分册目录内 max+1；slug 冲突时递增序号重试。
pub(crate) async fn create_doc_part(state: &AppState, form: DocPartCreateForm) -> ApiResult<Value> {
    let req = get_real_requirement(state, &form.req_id).await?;
    let dir = req_dir_path(&req)?;
    ensure_requirement_dir_writable(state, &dir).await?;
    let base = doc_base_name(&form.doc_type)?;
    let slug = ensure_safe_segment(form.slug.trim().trim_end_matches(".md"), "slug")?;
    if slug.is_empty() {
        return Err(ApiError::bad_request("slug 不能为空"));
    }
    let content = form.content.replace("\r\n", "\n");
    ensure_text_size(&content, "content")?;
    if content.trim().is_empty() {
        return Err(ApiError::bad_request("分册 content 不能为空"));
    }
    let dry_run = form.dry_run.unwrap_or(false);
    let parts_dir = dir.join(DOCS_DIR_NAME).join(&base);
    if !dry_run {
        fs::create_dir_all(&parts_dir).await?;
    }
    // 序号分配：扫描现有分册取 max+1（保证序号唯一递增，与 slug 无关），同名撞名时再递增重试。
    let seq_re = Regex::new(r"^(\d+)-").expect("valid regex");
    let mut max_seq: u64 = 0;
    if let Ok(mut rd) = fs::read_dir(&parts_dir).await {
        while let Ok(Some(entry)) = rd.next_entry().await {
            let name = entry.file_name().to_string_lossy().to_string();
            if let Some(caps) = seq_re.captures(&name) {
                if let Ok(n) = caps[1].parse::<u64>() {
                    if n > max_seq {
                        max_seq = n;
                    }
                }
            }
        }
    }
    let mut seq: u64 = max_seq + 1;
    let mut filename = format!("{seq:03}-{slug}.md");
    for _ in 0..8 {
        if !parts_dir.join(&filename).exists() {
            break;
        }
        seq += 1;
        filename = format!("{seq:03}-{slug}.md");
    }
    let rel_path = format!("{DOCS_DIR_NAME}/{base}/{filename}");
    let part_path = dir.join(&rel_path);
    let title = clean_optional(form.title.as_deref());
    // 分册正文缺 H1 时补标题行，保证 part_title 可提取。
    let body = if content.trim_start().starts_with("# ") {
        content.trim_start().to_string()
    } else {
        let heading = title
            .clone()
            .unwrap_or_else(|| format!("{} {} · {}", req.id, base, slug));
        format!("# {heading}\n\n{}", content.trim())
    };
    let body = format!("{}\n", body.trim_end());
    let summary = clean_optional(form.summary.as_deref())
        .or_else(|| title.clone())
        .unwrap_or_else(|| slug.clone());
    let entry = format!("- [{filename}]({rel_path}) — {summary} @ {}", today_ymd());
    let main_doc_file = format!("{base}.md");
    let main_path = dir.join(&main_doc_file);
    let planned = vec![
        part_path.to_string_lossy().to_string(),
        main_path.to_string_lossy().to_string(),
    ];
    if !dry_run {
        atomic_write_text(&part_path, &body).await?;
        append_index_entry(&main_path, &req.id, &main_doc_file, &entry).await?;
    }
    Ok(json!({
        "ok": true,
        "dryRun": dry_run,
        "reqId": req.id,
        "docType": form.doc_type,
        "docBase": base,
        "part": {
            "filename": filename,
            "relPath": rel_path,
            "path": part_path.to_string_lossy(),
            "bytes": body.len(),
            "title": title,
        },
        "indexEntry": entry,
        "mainDoc": main_doc_file,
        "files": planned,
        "hint": "主文档为索引：概览写在主文档，明细写入分册（建议单分册 ≤300 行）；agent 需要细节时按 relPath 直接 read 分册文件。",
    }))
}

/// 分册清单：可选按 docType 基名过滤；附带主文档大小与拆分阈值，供面板/agent 判断是否该拆。
pub(crate) async fn list_doc_parts(
    state: &AppState,
    req_id: &str,
    doc_type: Option<&str>,
) -> ApiResult<Value> {
    let req = get_real_requirement(state, req_id).await?;
    let dir = req_dir_path(&req)?;
    let base = match doc_type
        .map(str::trim)
        .filter(|v| !v.is_empty() && !v.contains('/'))
    {
        Some(dt) => Some(doc_base_name(dt)?),
        None => None,
    };
    let mut parts = scan_doc_parts(&dir, base.as_deref()).await?;
    if let Some(main_base) = &base {
        let main_path = dir.join(format!("{main_base}.md"));
        for part in &mut parts {
            part.index_linked = main_doc_links_index(&main_path, &part.rel_path).await;
        }
    }
    let main_doc = base.as_ref().map(|b| {
        let path = dir.join(format!("{b}.md"));
        let bytes = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        json!({ "file": format!("{b}.md"), "bytes": bytes, "overSplitThreshold": bytes > DOC_SPLIT_WARN_BYTES })
    });
    Ok(json!({
        "ok": true,
        "reqId": req.id,
        "docType": base,
        "partsDir": dir.join(DOCS_DIR_NAME).to_string_lossy(),
        "splitWarnBytes": DOC_SPLIT_WARN_BYTES,
        "mainDoc": main_doc,
        "count": parts.len(),
        "parts": parts,
        "hint": "分册按 relPath 字典序即序号序；主文档为索引，明细在分册。创建新分册用 POST /api/requirement/doc-part。",
    }))
}
