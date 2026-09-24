use super::*;

pub(crate) async fn create_requirement(
    state: &AppState,
    form: RequirementCreateForm,
) -> ApiResult<Value> {
    let title = clean_required(&form.title, "title")?;
    // 类别推导：reqId 模板带问题编号池前缀（WMS-INC-/WMS-TST-）时按前缀推导类别。
    // 用户约定：登记问题未强调测试环境的默认线上问题；显式 category 与前缀冲突时拒绝。
    let id_template_raw = form.req_id.trim();
    let derived_category = derive_category_from_id_template(id_template_raw);
    let category = match form.category.as_deref().map(str::trim).filter(|v| !v.is_empty()) {
        Some(explicit) => {
            if let Some(derived) = derived_category {
                if explicit != derived {
                    return Err(ApiError::bad_request(format!(
                        "reqId 使用问题编号池前缀（{derived} 对应 WMS-INC-/WMS-TST-），category 应为「{derived}」（未强调测试环境的问题默认线上问题），当前为「{explicit}」"
                    )));
                }
            }
            explicit.to_string()
        }
        None => derived_category.unwrap_or("需求").to_string(),
    };
    ensure_category(&category)?;
    let source = form
        .source
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(|v| ensure_source(v).map(|_| v.to_string()))
        .transpose()?
        .unwrap_or_else(|| "产品推动".to_string());
    let mut status = form
        .status
        .as_deref()
        .and_then(normalize_status_value)
        .unwrap_or_else(|| {
            if is_issue_category(&category) {
                "排查中".to_string()
            } else {
                "需求澄清".to_string()
            }
        });
    if is_issue_category(&category) && form.status.is_none() {
        status = "排查中".to_string();
    }
    ensure_status(&status)?;
    // 需求组（members 非空）只能使用 category=需求；issue 家族不支持组。
    let is_group = form.members.as_ref().is_some_and(|m| !m.is_empty());
    if is_group && is_issue_category(&category) {
        return Err(ApiError::bad_request(
            "需求组（members 非空）只能使用 category=需求；线上问题/测试问题不支持组",
        ));
    }
    let dry_run = form.dry_run.unwrap_or(false);
    // 串行化创建临界区：{seq} 占号靠扫描已有需求，扫描 -> 预留目录 -> 写 meta.md 必须
    // 原子完成，否则两个并行创建会观察到同一个 max seq 且各自成功（目录名含 slug 不同，
    // create_dir 的 AlreadyExists 兼底检测不到），导致撞号。dry_run 只预览不占号，无需加锁。
    let _create_guard = if dry_run {
        None
    } else {
        Some(state.requirement_create_lock.lock().await)
    };
    let base = resolve_create_req_root(state, form.root.as_deref()).await?;
    let id_template = normalize_create_id_template(form.req_id.trim(), &category, is_group)?;
    let (req_id, target_dir) =
        resolve_req_id_and_target_dir(state, &base, &id_template, &form, dry_run).await?;

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
    // 创建时绑定线上问题：仅 category=需求 时允许，且每个 id 必须是真实存在的线上问题。
    let mut issues: Vec<String> = Vec::new();
    if let Some(raw_issues) = form.issues.as_ref() {
        if category != "需求" {
            return Err(ApiError::bad_request(
                "只有 category=需求 的记录可以绑定线上问题（issues）",
            ));
        }
        issues = unique_strings(raw_issues.clone());
        for id in &issues {
            let issue = get_real_requirement(state, id)
                .await
                .map_err(|_| ApiError::bad_request(format!("关联线上问题不存在：{id}")))?;
            if issue.category.as_deref() != Some("线上问题") {
                return Err(ApiError::bad_request(format!(
                    "{} 不是线上问题（category={}），只能绑定 category=线上问题 的记录",
                    id,
                    issue.category.unwrap_or_else(|| "需求".into())
                )));
            }
        }
    }
    let summary = clean_optional(form.summary.as_deref()).unwrap_or_else(|| "待补充".to_string());
    let background = clean_optional(form.background.as_deref());
    let notes = clean_optional(form.notes.as_deref());
    // 引用式需求组：members 非空时校验成员（存在、非自身、非组）并构建 group.json。
    let group_file = build_group_file_for_create(
        state,
        &req_id,
        form.members.as_ref(),
        form.release_policy.as_deref(),
    )
    .await?;

    let files = requirement_create_files(
        &req_id,
        &title,
        &status,
        &project,
        &projects,
        &category,
        &source,
        &owner,
        &start_date,
        &plan_release,
        &ones,
        &issues,
        &summary,
        background.as_deref(),
        notes.as_deref(),
        None,
    );
    let mut planned: Vec<String> = files
        .iter()
        .map(|(name, _)| target_dir.join(name).to_string_lossy().to_string())
        .collect();
    if group_file.is_some() {
        planned.push(target_dir.join(GROUP_FILE).to_string_lossy().to_string());
    }
    if !dry_run {
        fs::create_dir_all(&target_dir).await?;
        for (name, body) in &files {
            atomic_write_text(&target_dir.join(name), body).await?;
        }
        if let Some(group) = &group_file {
            atomic_write_json(&target_dir.join(GROUP_FILE), group).await?;
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
        "source": source,
        "project": project,
        "projects": projects,
        "reqDir": target_dir.to_string_lossy(),
        "files": planned,
        "group": group_file.map(|g| json!({
            "releasePolicy": g.release_policy,
            "members": g.members.iter().map(|m| json!({
                "reqId": m.req_id,
                "note": m.note,
            })).collect::<Vec<_>>(),
        })),
        "validation": validation,
    }))
}

/// 创建子需求：从大需求（父需求）拆出并行执行单元。
///
/// - ID 由服务端分配：`<父票号>-S<n>[-slug]`，目录平铺在父需求同级，不占全局 seq 池；
/// - 复制父需求 background/technical-plan/impact/test 文档作快照起点（标注快照来源，
///   之后独立维护）；不复制 events/state/branches/code-review；
/// - 不绑 ONES/plan-release/issues（父需求承载）；不建分支（分支初始化走
///   POST /api/requirement/sub/init-branches，以父分支为 base 派生 `-sub<n>` 分支）；
/// - 初始状态 需求创建，走子需求独立轻量状态机。
pub(crate) async fn create_sub_requirement(
    state: &AppState,
    form: SubRequirementCreateForm,
) -> ApiResult<Value> {
    let title = clean_required(&form.title, "title")?;
    let parent = get_real_requirement(state, &form.parent_req_id).await?;
    if parent.category.as_deref() != Some("需求") {
        return Err(ApiError::bad_request(format!(
            "父需求 {} 不是 category=需求 的普通需求，不能拆子需求",
            parent.id
        )));
    }
    if parent.group_members.is_some() {
        return Err(ApiError::bad_request(format!(
            "{} 是引用式需求组，不支持拆子需求（如需并行请对成员需求各自推进）",
            parent.id
        )));
    }
    if parent.is_sub_req || parent.parent_req_id.is_some() {
        return Err(ApiError::bad_request(format!(
            "{} 已是子需求；不支持子需求嵌套（只允许一层拆分）",
            parent.id
        )));
    }
    if matches!(parent.status.as_str(), "经验总结" | "已完成") {
        return Err(ApiError::bad_request(format!(
            "父需求 {} 已进入 {}，不再拆子需求",
            parent.id, parent.status
        )));
    }
    let dry_run = form.dry_run.unwrap_or(false);
    // 与普通创建共享串行化临界区：子需求编号也靠扫描已有需求分配。
    let _create_guard = if dry_run {
        None
    } else {
        Some(state.requirement_create_lock.lock().await)
    };
    let (req_id, target_dir) =
        resolve_sub_req_id_and_target_dir(state, &parent, form.slug.as_deref(), dry_run).await?;
    let projects = parent.projects.clone();
    let project = projects
        .first()
        .cloned()
        .unwrap_or_else(|| DEFAULT_PROJECT_NAME.to_string());
    // 父需求目录：后续复制文档与继承 owner 都要用。
    let parent_dir = req_dir_path(&parent)?;
    let parent_meta = fs::read_to_string(parent_dir.join("meta.md"))
        .await
        .unwrap_or_default();
    let parent_owner = parse_frontmatter(&parent_meta)
        .fields
        .get("owner")
        .cloned()
        .unwrap_or_default();
    let owner = clean_optional(form.owner.as_deref()).unwrap_or_else(|| {
        if parent_owner.trim().is_empty() {
            "unknown".to_string()
        } else {
            parent_owner
        }
    });
    let start_date = today_ymd();
    let summary = clean_optional(form.summary.as_deref()).unwrap_or_else(|| "待补充".to_string());
    let status = "需求创建";

    // 复制父需求文档作快照起点（预置快照标注；空文件/缺失跳过）。
    let mut copied_docs = Vec::<String>::new();
    let mut snapshot_files: Vec<(&'static str, String)> = Vec::new();
    for doc in ["background.md", "technical-plan.md", "impact.md", "test.md"] {
        let src = parent_dir.join(doc);
        let raw = if src.is_file() {
            fs::read_to_string(&src).await.unwrap_or_default()
        } else {
            String::new()
        };
        if raw.trim().is_empty() {
            continue;
        }
        let body = format!("{}{}", snapshot_note(&parent.id, doc), raw.trim_start());
        snapshot_files.push((doc, body));
        copied_docs.push(doc.to_string());
    }

    let mut files = requirement_create_files(
        &req_id,
        &title,
        status,
        &project,
        &projects,
        "需求",
        &parent.source,
        &owner,
        &start_date,
        "unknown",
        "",
        &[],
        &summary,
        None,
        None,
        Some(parent.id.as_str()),
    );
    // 父需求快照覆盖同名的 background/technical-plan 模板，impact/test 直接追加。
    for (name, body) in snapshot_files {
        if let Some(slot) = files.iter_mut().find(|(n, _)| *n == name) {
            slot.1 = body;
        } else {
            files.push((name, body));
        }
    }
    let planned: Vec<String> = files
        .iter()
        .map(|(name, _)| target_dir.join(name).to_string_lossy().to_string())
        .collect();
    if !dry_run {
        for (name, body) in &files {
            atomic_write_text(&target_dir.join(name), body).await?;
        }
    }
    let validation = if dry_run {
        json!({ "ok": true, "dryRun": true, "problems": [], "warnings": [] })
    } else {
        let req = load_requirement_from_dir(&target_dir, &req_id, &[project.clone()], &[])
            .await?
            .ok_or_else(|| anyhow!("created sub-requirement cannot be loaded"))?;
        validate_requirement(state, &req).await?
    };
    // 父需求 notes 留痕（dry-run 跳过）。
    if !dry_run {
        let note = format!(
            "拆分子需求：`{}`（{}）；子分支以父分支为 base，合回父分支后父需求统一集成。",
            req_id, title
        );
        let _ = append_requirement_note(
            state,
            RequirementNoteForm {
                req_id: parent.id.clone(),
                text: note,
                title: Some("拆分子需求".to_string()),
                session_id: None,
                dry_run: Some(false),
            },
        )
        .await;
    }
    Ok(json!({
        "ok": true,
        "dryRun": dry_run,
        "reqId": req_id,
        "title": title,
        "status": status,
        "parentReqId": parent.id,
        "reqDir": target_dir.to_string_lossy(),
        "files": planned,
        "copiedDocs": copied_docs,
        "nextStep": "子需求仅建记录不建分支；并行开发前调 POST /api/requirement/sub/init-branches 以父分支为 base 派生 -sub<n> 分支",
        "validation": validation,
    }))
}

/// 创建需求组时校验成员并构建 group.json 内容；members 为空时返回 None。
///
/// 规则：成员必须是已存在的需求；不能引用自身；成员自身不能是需求组（不支持嵌套）；
/// releasePolicy 仅在创建组时有效，可选 together / independent（默认 independent）。
async fn build_group_file_for_create(
    state: &AppState,
    self_id: &str,
    members: Option<&Vec<GroupMemberInput>>,
    release_policy: Option<&str>,
) -> ApiResult<Option<GroupFile>> {
    let Some(members) = members.filter(|m| !m.is_empty()) else {
        if let Some(policy) = clean_optional(release_policy) {
            return Err(ApiError::bad_request(format!(
                "releasePolicy ({policy}) 仅在创建需求组（members 非空）时有效"
            )));
        }
        return Ok(None);
    };
    let policy = match clean_optional(release_policy) {
        Some(p) => {
            if !GROUP_RELEASE_POLICIES.contains(&p.as_str()) {
                return Err(ApiError::bad_request(format!(
                    "invalid releasePolicy: {p}（可选 together / independent）"
                )));
            }
            p
        }
        None => "independent".to_string(),
    };
    let mut refs = Vec::new();
    for member in members {
        let id = clean_required(member.req_id.trim(), "members[].reqId")?;
        if id == self_id {
            return Err(ApiError::bad_request("需求组不能引用自身作为成员"));
        }
        let member_req = get_real_requirement(state, &id).await.map_err(|_| {
            ApiError::bad_request(format!("需求组成员不存在：{id}（成员必须是已存在的需求）"))
        })?;
        if let Some(dir) = &member_req.req_dir {
            if is_requirement_group(Path::new(dir)).await {
                return Err(ApiError::bad_request(format!(
                    "需求组成员 {id} 自身已是需求组；不支持组嵌套"
                )));
            }
        }
        refs.push(GroupMemberRef {
            req_id: id,
            note: clean_optional(member.note.as_deref()),
            title: None,
            status: None,
            found: true,
            nested: false,
        });
    }
    Ok(Some(GroupFile {
        version: 1,
        release_policy: Some(policy),
        members: refs,
    }))
}

/// 需求组状态为派生值（min 成员需求流状态），禁止手动 setStatus。
pub(crate) async fn ensure_group_status_locked(req: &Requirement) -> ApiResult<()> {
    if let Some(dir) = &req.req_dir {
        if is_requirement_group(Path::new(dir)).await {
            return Err(ApiError::bad_request(format!(
                "{} 是引用式需求组：组状态为派生值（min 成员状态），不能手动设置，请推进成员需求状态",
                req.id
            )));
        }
    }
    Ok(())
}

pub(crate) async fn update_requirement(
    state: &AppState,
    form: RequirementPatchForm,
) -> ApiResult<Value> {
    let req = get_real_requirement(state, &form.req_id).await?;
    // 子需求字段边界：ONES/plan-release/issues/类别都由父需求承载，不允许在子需求上设置。
    if req.is_sub_req
        && (form.ones.is_some()
            || form.plan_release.is_some()
            || form.issues.is_some()
            || form.category.is_some())
    {
        return Err(ApiError::bad_request(
            "子需求不绑定 ONES/plan-release/issues，也不能切换类别（这些由父需求承载）",
        ));
    }
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
    if let Some(source) = form.source.as_deref() {
        ensure_source(source)?;
        meta_next = set_frontmatter_field(&meta_next, "source", source);
        changes.push("meta.source".into());
    }
    if let Some(raw_issues) = form.issues.as_ref() {
        let issue_ids = unique_strings(raw_issues.clone());
        for id in &issue_ids {
            let issue = get_real_requirement(state, id)
                .await
                .map_err(|_| ApiError::bad_request(format!("关联线上问题不存在：{id}")))?;
            if issue.category.as_deref() != Some("线上问题") {
                return Err(ApiError::bad_request(format!(
                    "{} 不是线上问题（category={}），只能绑定 category=线上问题 的记录",
                    id,
                    issue.category.unwrap_or_else(|| "需求".into())
                )));
            }
        }
        meta_next = set_frontmatter_field(&meta_next, "issues", &issue_ids.join(", "));
        changes.push("meta.issues".into());
    }
    let mut auto_advanced_issues: Vec<String> = Vec::new();
    if let Some(status) = form.status.as_deref() {
        let status = canonical_status(status)?;
        // 需求组状态为派生值，禁止手动设置（PATCH / edit setStatus 同样拦截）。
        ensure_group_status_locked(&req).await?;
        // 子需求走独立轻量状态机；普通需求/issue 禁止使用子需求专属状态。
        if req.is_sub_req {
            ensure_sub_req_status_transition(&req, &status)?;
        } else {
            ensure_status_allowed_for_non_sub(&status)?;
        }
        // 状态流转门禁（配置驱动）：review / selftest-checklist / test-scenario / issue-*。
        // edit 接口主要供 agent 与脚本使用，始终强校验；人工在 Panel UI 上改状态走 status 接口并跳过。
        ensure_status_transition_gates(state, &req, &status).await?;
        changes.push("state.status".into());
        if !dry_run {
            let st = write_requirement_status_checked(
                dir.to_string_lossy().as_ref(),
                &status,
                form.note.as_deref(),
                GateCheckMode::Passed,
            )
            .await?;
            if !matches!(st.get("changed").and_then(Value::as_bool), Some(false)) {
                record_status_transition_event(state, &req, &st, form.note.as_deref()).await?;
                planned_files.push(
                    dir.join(REQUIREMENT_EVENTS_FILE)
                        .to_string_lossy()
                        .to_string(),
                );
                // 需求进入 >= 经验总结 时，自动把绑定的未解决线上问题推进到已修复。
                auto_advanced_issues = auto_advance_linked_issues(state, &req, &status).await;
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
        "autoAdvancedIssues": auto_advanced_issues,
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
