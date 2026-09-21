use super::*;

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
    let mut oversized_docs = Vec::<(String, u64)>::new();
    for file in required_files.into_iter().chain(optional_files.into_iter()) {
        let path = dir.join(file);
        let exists = path.is_file();
        files.insert(file.to_string(), exists);
        if exists && file.ends_with(".md") {
            if let Ok(meta) = fs::metadata(&path).await {
                if meta.len() > DOC_SPLIT_WARN_BYTES {
                    oversized_docs.push((file.to_string(), meta.len()));
                }
            }
        }
        if file == "meta.md" && !exists {
            problems.push("missing meta.md".into());
        } else if required_files.contains(&file) && !exists {
            warnings.push(format!("missing core file {file}"));
        }
    }
    // 文档分册阈值告警：主文档超阈值提示索引化拆分；分册自身超阈值提示继续细拆。
    for (file, bytes) in &oversized_docs {
        warnings.push(format!(
            "{file} {}KB 超过拆分阈值 {}KB：主文件应索引化，明细用 POST /api/requirement/doc-part 拆到 docs/<doc>/ 分册（单分册建议 ≤300 行）",
            bytes / 1024,
            DOC_SPLIT_WARN_BYTES / 1024
        ));
    }
    if let Ok(parts) = scan_doc_parts(&dir, None).await {
        for part in parts {
            if part.bytes > DOC_SPLIT_WARN_BYTES {
                warnings.push(format!(
                    "分册 {} {}KB 超过拆分阈值 {}KB：建议按主题继续细拆分册",
                    part.rel_path,
                    part.bytes / 1024,
                    DOC_SPLIT_WARN_BYTES / 1024
                ));
            }
            if !part.index_linked {
                warnings.push(format!(
                    "分册 {} 未被主文档索引引用（主文档 {} 缺少指向它的索引行）",
                    part.rel_path, part.doc_base
                ));
            }
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
    // 引用式需求组校验：group.json 结构、发布策略、成员存在性与嵌套限制。
    let group_path = dir.join(GROUP_FILE);
    if group_path.is_file() {
        match load_group_json(&dir).await {
            Some(group) => {
                if group.members.is_empty() {
                    warnings.push("group.json has no members".into());
                }
                if let Some(policy) = &group.release_policy {
                    if !GROUP_RELEASE_POLICIES.contains(&policy.as_str()) {
                        problems.push(format!(
                            "invalid group.json releasePolicy: {policy}（可选 together / independent）"
                        ));
                    }
                }
                for member in &group.members {
                    if member.req_id == req.id {
                        problems.push("group.json references itself as a member".into());
                        continue;
                    }
                    match get_real_requirement(state, &member.req_id).await {
                        Ok(member_req) => {
                            if let Some(member_dir) = &member_req.req_dir {
                                if is_requirement_group(Path::new(member_dir)).await {
                                    problems.push(format!(
                                        "group member {} is itself a requirement group; nesting is not supported",
                                        member.req_id
                                    ));
                                }
                            }
                        }
                        Err(_) => warnings.push(format!(
                            "group member not found (stale reference): {}",
                            member.req_id
                        )),
                    }
                }
            }
            None => problems.push(
                "group.json exists but is not valid (expects version/releasePolicy/members with reqId)".into(),
            ),
        }
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
