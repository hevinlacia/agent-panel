use super::*;

/// Parse a reqId template containing a single `{seq}` placeholder into
/// `(prefix, suffix)`. `REQ-{seq}-demo` -> `("REQ", "-demo")`.
/// 线上问题独立编号池前缀：与普通需求主池分开计数，目录仍共用 req 根；
/// 目录身份证创建后不变，setCategory 切换类别不改号。
/// 编号池前缀只表达**类型**，不表达项目（项目归属由需求所在工作区/仓库名决定）；
/// 旧 `WMS-`/`WMS-INC-`/`WMS-TST-`/`WMS-GRP-` 前缀继续兼容，具体 id 不改名，
/// 新建默认用无项目字的新式前缀。
pub(crate) const ISSUE_ID_PREFIX: &str = "INC";
/// 测试问题独立编号池：承接 UAT 测试反馈中不属于常规需求的轻量问题。
pub(crate) const TEST_ISSUE_ID_PREFIX: &str = "TST";
/// 需求组（引用式需求组，group.json）独立编号池：组是需求聚合视图，不占用主需求池。
pub(crate) const GROUP_ID_PREFIX: &str = "GRP";
/// 普通需求主池（新建默认；旧 WMS- 前缀谱系延续）。
pub(crate) const REQ_ID_PREFIX: &str = "REQ";
/// 旧主池前缀：与 REQ 共用序号空间（谱系延续，REQ 从 WMS 主池最大序号之后继续）。
pub(crate) const LEGACY_REQ_ID_PREFIX: &str = "WMS";
/// 修复需求独立编号池：category=需求 且绑定 issues（线上问题）的记录 = 代码修复性质，
/// 与普通需求主池（REQ）分开计数；由创建时的 issues 绑定自动路由，不靠人工选择。
/// 修复也放在需求列表中（category=需求，同一套状态机），仅前缀+req-kind=fix 区分。
pub(crate) const FIX_ID_PREFIX: &str = "FIX";
/// 整合需求（consolidated requirement）独立编号池：父需求 + 子需求 + 发布分支模型，
/// 不占用主需求池；子需求 id = `<父票号>-S<n>[-slug]` 继承父前缀。
pub(crate) const ROLLUP_ID_PREFIX: &str = "ROLLUP";

/// issue 家族类别（共用轻量状态机与 incident.md 文档集）：线上问题 / 测试问题。
pub(crate) fn is_issue_category(category: &str) -> bool {
    matches!(category, "线上问题" | "测试问题")
}

fn issue_id_prefix(category: &str) -> Option<&'static str> {
    match category {
        "线上问题" => Some(ISSUE_ID_PREFIX),
        "测试问题" => Some(TEST_ISSUE_ID_PREFIX),
        _ => None,
    }
}

/// 同一序号空间的池前缀别名（新旧式互为别名）：新建用新式前缀，存量旧式 id 继续有效，
/// 序号取全部别名形式的最大值延续，避免口语里的同一序号指两个需求。
pub(crate) fn pool_lineage_prefixes(prefix: &str) -> Vec<String> {
    match prefix.trim().trim_end_matches('-') {
        REQ_ID_PREFIX | LEGACY_REQ_ID_PREFIX => {
            vec![REQ_ID_PREFIX.to_string(), LEGACY_REQ_ID_PREFIX.to_string()]
        }
        ISSUE_ID_PREFIX | "WMS-INC" => {
            vec![ISSUE_ID_PREFIX.to_string(), "WMS-INC".to_string()]
        }
        TEST_ISSUE_ID_PREFIX | "WMS-TST" => {
            vec![TEST_ISSUE_ID_PREFIX.to_string(), "WMS-TST".to_string()]
        }
        GROUP_ID_PREFIX | "WMS-GRP" => {
            vec![GROUP_ID_PREFIX.to_string(), "WMS-GRP".to_string()]
        }
        FIX_ID_PREFIX => {
            vec![FIX_ID_PREFIX.to_string()]
        }
        other => vec![other.to_string()],
    }
}

/// 需求主池专用的“其他池”前缀：主池模板/具体 id 不允许占用这些池（避免池污染）。
fn is_reserved_non_main_pool_prefix(prefix: &str) -> bool {
    matches!(
        prefix.trim().trim_end_matches('-'),
        ISSUE_ID_PREFIX
            | "WMS-INC"
            | TEST_ISSUE_ID_PREFIX
            | "WMS-TST"
            | GROUP_ID_PREFIX
            | "WMS-GRP"
            | FIX_ID_PREFIX
            | ROLLUP_ID_PREFIX
    )
}

/// 具体 id 是否落在保留池形态上：`<保留前缀>-<数字>...`（如 WMS-TST-005-x、GRP-001-x）。
fn concrete_id_uses_reserved_pool_prefix(id: &str) -> bool {
    [
        ISSUE_ID_PREFIX,
        "WMS-INC",
        TEST_ISSUE_ID_PREFIX,
        "WMS-TST",
        GROUP_ID_PREFIX,
        "WMS-GRP",
        FIX_ID_PREFIX,
        ROLLUP_ID_PREFIX,
    ]
    .iter()
    .any(|p| {
        Regex::new(&format!("^{}-(\\d+)(-|$)", regex::escape(p)))
            .expect("valid regex")
            .is_match(id.trim())
    })
}

/// 从 reqId 模板前缀推导问题类别（纯函数）：INC-/WMS-INC- → 线上问题，TST-/WMS-TST- → 测试问题。
/// 用户约定：登记问题时未强调测试环境的默认线上问题。
pub(crate) fn derive_category_from_id_template(raw: &str) -> Option<&'static str> {
    let v = raw.trim();
    for (prefix, category) in [
        (ISSUE_ID_PREFIX, "线上问题"),
        ("WMS-INC", "线上问题"),
        (TEST_ISSUE_ID_PREFIX, "测试问题"),
        ("WMS-TST", "测试问题"),
    ] {
        if v.starts_with(&format!("{prefix}-")) {
            return Some(category);
        }
    }
    None
}

/// 创建时按类别/形态规范化 reqId 模板：
/// - 需求组（members 非空）强制使用 `GRP-{seq}` 独立编号池（旧 WMS-GRP 具体 id 继续有效）；
/// - 整合需求（consolidated=true）强制使用 `ROLLUP-{seq}` 独立编号池；
/// - 修复需求（is_fix：category=需求 且绑定 issues）强制使用 `FIX-{seq}` 独立编号池；
/// - 空模板给类别默认池：需求 `REQ-{seq}`、修复 `FIX-{seq}`、线上问题 `INC-{seq}`、测试问题 `TST-{seq}`；
/// - issue/组/整合/修复池模板带 {seq} 时改写为对应池前缀保留 suffix；具体 id 校验形态；
/// - 需求主池模板透传（旧 WMS-{seq} 兼容），但不得占用其他池前缀。
pub(crate) fn normalize_create_id_template(
    raw: &str,
    category: &str,
    is_group: bool,
    is_consolidated: bool,
    is_fix: bool,
) -> ApiResult<String> {
    let v = raw.trim();
    if is_group {
        if v.is_empty() {
            return Ok(format!("{GROUP_ID_PREFIX}-{{seq}}"));
        }
        if v.contains("{seq}") {
            let (_, suffix) = split_seq_template(v)?;
            return Ok(format!("{GROUP_ID_PREFIX}-{{seq}}{suffix}"));
        }
        let valid = pool_lineage_prefixes(GROUP_ID_PREFIX)
            .iter()
            .any(|prefix| {
                Regex::new(&format!("^{}-(\\d+)(-|$)", regex::escape(prefix)))
                    .expect("valid regex")
                    .is_match(v)
            });
        if !valid {
            return Err(ApiError::bad_request(format!(
                "需求组 reqId 必须使用 {GROUP_ID_PREFIX}-<序号> 独立编号（如 {GROUP_ID_PREFIX}-001-slug）；组是需求聚合视图，不占用主需求池"
            )));
        }
        return Ok(v.to_string());
    }
    if is_consolidated {
        if is_issue_category(category) {
            return Err(ApiError::bad_request(
                "整合需求（consolidated=true）只能使用 category=需求，不能用于线上问题/测试问题",
            ));
        }
        if v.is_empty() {
            return Ok(format!("{ROLLUP_ID_PREFIX}-{{seq}}"));
        }
        if v.contains("{seq}") {
            let (_, suffix) = split_seq_template(v)?;
            return Ok(format!("{ROLLUP_ID_PREFIX}-{{seq}}{suffix}"));
        }
        let re = Regex::new(&format!("^{}-(\\d+)(-|$)", ROLLUP_ID_PREFIX)).expect("valid regex");
        if !re.is_match(v) {
            return Err(ApiError::bad_request(format!(
                "整合需求 reqId 必须使用 {ROLLUP_ID_PREFIX}-<序号> 独立编号（如 {ROLLUP_ID_PREFIX}-001-prod-log-fix-rollup）；整合需求不占用主需求池"
            )));
        }
        return Ok(v.to_string());
    }
    if is_fix {
        if v.is_empty() {
            return Ok(format!("{FIX_ID_PREFIX}-{{seq}}"));
        }
        if v.contains("{seq}") {
            let (_, suffix) = split_seq_template(v)?;
            return Ok(format!("{FIX_ID_PREFIX}-{{seq}}{suffix}"));
        }
        let valid = pool_lineage_prefixes(FIX_ID_PREFIX)
            .iter()
            .any(|prefix| {
                Regex::new(&format!("^{}-(\\d+)(-|$)", regex::escape(prefix)))
                    .expect("valid regex")
                    .is_match(v)
            });
        if !valid {
            return Err(ApiError::bad_request(format!(
                "修复需求 reqId 必须使用 {FIX_ID_PREFIX}-<序号> 独立编号（如 {FIX_ID_PREFIX}-001-slug）；绑定 issues 的记录是代码修复性质，不占用需求主池；不绑 issues 请去掉 issues 字段用 REQ 池"
            )));
        }
        return Ok(v.to_string());
    }
    let Some(prefix) = issue_id_prefix(category) else {
        if v.is_empty() {
            return Ok(format!("{REQ_ID_PREFIX}-{{seq}}"));
        }
        if v.contains("{seq}") {
            let (template_prefix, suffix) = split_seq_template(v)?;
            if is_reserved_non_main_pool_prefix(&template_prefix) {
                return Err(ApiError::bad_request(format!(
                    "需求主池模板不能占用其他编号池前缀 {template_prefix}；整合需求请传 consolidated=true，组/问题请用对应类别"
                )));
            }
            return Ok(v.to_string());
        }
        if concrete_id_uses_reserved_pool_prefix(v) {
            return Err(ApiError::bad_request(
                "需求主池 reqId 不能占用其他编号池前缀（INC/TST/GRP/FIX/ROLLUP 及其旧式 WMS-xxx 别名）；整合需求请传 consolidated=true，组/问题/修复请用对应类别或绑定 issues",
            ));
        }
        return Ok(v.to_string());
    };
    if v.is_empty() {
        return Ok(format!("{prefix}-{{seq}}"));
    }
    if v.contains("{seq}") {
        let (_, suffix) = split_seq_template(v)?;
        return Ok(format!("{prefix}-{{seq}}{suffix}"));
    }
    let valid = pool_lineage_prefixes(prefix).iter().any(|alias| {
        Regex::new(&format!("^{}-(\\d+)(-|$)", regex::escape(alias)))
            .expect("valid regex")
            .is_match(v)
    });
    if !valid {
        let hint = if category == "线上问题" {
            "；复现/验证代码可直接登记分支开发（仅测试环境），正式生产修复承接请创建 category=需求 的普通需求并绑定本问题"
        } else {
            ""
        };
        return Err(ApiError::bad_request(format!(
            "{category} reqId 必须使用 {prefix}-<序号> 独立编号（如 {prefix}-003-slug，旧 WMS-INC-/WMS-TST- 形态兼容）{hint}"
        )));
    }
    Ok(v.to_string())
}

pub(crate) fn split_seq_template(template: &str) -> ApiResult<(String, String)> {
    let count = template.matches("{seq}").count();
    if count == 0 {
        return Err(ApiError::bad_request("reqId template must contain {seq}"));
    }
    if count > 1 {
        return Err(ApiError::bad_request(
            "reqId template may contain at most one {seq}",
        ));
    }
    let idx = template.find("{seq}").expect("checked count above");
    let prefix = template[..idx].trim_end_matches('-').to_string();
    let suffix = template[idx + 5..].to_string();
    if prefix.is_empty() {
        return Err(ApiError::bad_request(
            "reqId template must have a non-empty prefix before {seq}",
        ));
    }
    if !prefix
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-')
    {
        return Err(ApiError::bad_request(
            "reqId prefix must be ASCII alphanumeric or hyphen",
        ));
    }
    Ok((prefix, suffix))
}

/// Format the final reqId from prefix, sequence (zero-padded to 3 digits) and
/// suffix. `("WMS", 43, "-demo")` -> `WMS-043-demo`.
pub(crate) fn format_seq_id(prefix: &str, seq: u64, suffix: &str) -> String {
    format!("{prefix}-{:03}{suffix}", seq)
}

/// Pure helper: compute the next sequence number for `prefix` from a list of
/// existing requirement ids. Sub-requirements sharing a number (e.g.
/// `WMS-003-*`) and gaps are handled by taking `max + 1`. `floor` forces a
/// minimum (used when retrying after a collision).
pub(crate) fn compute_next_seq_from_ids(ids: &[String], prefix: &str, floor: Option<u64>) -> u64 {
    let re = Regex::new(&format!("^{}-(\\d+)(-|$)", regex::escape(prefix))).expect("valid regex");
    let mut max_seq: u64 = 0;
    for id in ids {
        if let Some(caps) = re.captures(id) {
            if let Ok(n) = caps[1].parse::<u64>() {
                if n > max_seq {
                    max_seq = n;
                }
            }
        }
    }
    let mut next = max_seq + 1;
    if let Some(f) = floor {
        next = next.max(f);
    }
    next
}

/// Allocate the next sequence number for `prefix` by scanning existing
/// requirements on disk. 谱系感知：同序号空间的新旧别名前缀（REQ↔WMS、INC↔WMS-INC 等）
/// 一起取最大值，新前缀从存量旧池最大序号之后延续。
pub(crate) async fn allocate_next_seq(
    state: &AppState,
    prefix: &str,
    floor: Option<u64>,
) -> ApiResult<u64> {
    let reqs = scan_hermes_requirements(state).await?;
    let ids: Vec<String> = reqs.iter().map(|r| r.id.clone()).collect();
    Ok(compute_next_seq_from_ids_lineage(&ids, prefix, floor))
}

/// 谱系感知版 compute_next_seq_from_ids：prefix 的全部别名形式一起计数。
pub(crate) fn compute_next_seq_from_ids_lineage(
    ids: &[String],
    prefix: &str,
    floor: Option<u64>,
) -> u64 {
    let aliases = pool_lineage_prefixes(prefix);
    let mut max_seq: u64 = 0;
    for alias in &aliases {
        let next = compute_next_seq_from_ids(ids, alias, None).saturating_sub(1);
        if next > max_seq {
            max_seq = next;
        }
    }
    let mut next = max_seq + 1;
    if let Some(f) = floor {
        next = next.max(f);
    }
    next
}

/// Compute the target directory for a fully-resolved reqId, resolving parent
/// requirement or group path from the create form.
pub(crate) async fn compute_create_target_dir(
    state: &AppState,
    base: &Path,
    req_id: &str,
    form: &RequirementCreateForm,
) -> ApiResult<PathBuf> {
    if let Some(parent_id) = clean_optional(form.parent_req_id.as_deref()) {
        let parent = get_real_requirement(state, &parent_id).await?;
        let parent_dir = req_dir_path(&parent)?;
        ensure_requirement_dir_writable(state, &parent_dir).await?;
        Ok(parent_dir.join(req_id))
    } else {
        let mut dir = base.to_path_buf();
        for segment in form.group_path.as_deref().unwrap_or_default() {
            dir = dir.join(ensure_safe_segment(segment, "groupPath")?);
        }
        Ok(dir.join(req_id))
    }
}

/// Resolve the final reqId and target directory for a create request.
///
/// Callers must hold `state.requirement_create_lock` (non-dry-run): when
/// `template` contains `{seq}`, the next sequence number is allocated by
/// scanning existing requirements, and only the create lock makes the
/// scan -> reserve -> write-meta sequence atomic against parallel creates.
/// The `fs::create_dir` collision retry below stays as defense-in-depth for
/// same-slug collisions. When `template` has no `{seq}`, it is validated
/// as-is and the target directory is checked for prior existence.
pub(crate) async fn resolve_req_id_and_target_dir(
    state: &AppState,
    base: &Path,
    template: &str,
    form: &RequirementCreateForm,
    dry_run: bool,
) -> ApiResult<(String, PathBuf)> {
    if !template.contains("{seq}") {
        let req_id = ensure_req_id(template)?;
        let target_dir = compute_create_target_dir(state, base, &req_id, form).await?;
        ensure_path_inside_req_roots(state, &target_dir).await?;
        if target_dir.exists() {
            return Err(ApiError::bad_request(format!(
                "requirement directory already exists: {}",
                target_dir.to_string_lossy()
            )));
        }
        return Ok((req_id, target_dir));
    }

    let (prefix, suffix) = split_seq_template(template)?;
    let max_retries: u32 = 5;
    let mut floor: Option<u64> = None;
    for attempt in 0..=max_retries {
        let seq = allocate_next_seq(state, &prefix, floor).await?;
        let req_id = ensure_req_id(&format_seq_id(&prefix, seq, &suffix))?;
        let target_dir = compute_create_target_dir(state, base, &req_id, form).await?;
        ensure_path_inside_req_roots(state, &target_dir).await?;
        if dry_run {
            return Ok((req_id, target_dir));
        }
        if let Some(parent) = target_dir.parent() {
            fs::create_dir_all(parent).await?;
        }
        match fs::create_dir(&target_dir).await {
            Ok(()) => return Ok((req_id, target_dir)),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                if attempt == max_retries {
                    return Err(ApiError::bad_request(format!(
                        "requirement directory already exists after {max_retries} retries: {}",
                        target_dir.to_string_lossy()
                    )));
                }
                floor = Some(seq + 1);
                continue;
            }
            Err(e) => return Err(anyhow!("create requirement dir failed: {e}").into()),
        }
    }
    unreachable!("retry loop exhausted without returning")
}

/// 从需求 id 提取票号前缀（首段连续到第一个全数字段）：`WMS-049-wave-pick-task` ->
/// `WMS-049`，`WMS-INC-007-foo` -> `WMS-INC-007`；无数字段返回 None。
pub(crate) fn extract_ticket_prefix(req_id: &str) -> Option<String> {
    let mut acc = String::new();
    for seg in req_id.split('-') {
        if !seg.is_empty() && seg.chars().all(|c| c.is_ascii_digit()) {
            return Some(if acc.is_empty() {
                seg.to_string()
            } else {
                format!("{acc}-{seg}")
            });
        }
        if seg.is_empty() {
            return None;
        }
        if acc.is_empty() {
            acc = seg.to_string();
        } else {
            acc = format!("{acc}-{seg}");
        }
    }
    None
}

/// 解析子需求 req id 与目标目录（目录平铺在父需求同级，ID = `<父票号>-S<n>[-slug]`）。
///
/// - n 在父需求维度内递增（max+1），已取消/已合入的编号不复用（自然不复用：只取 max+1）；
/// - 碰撞（同名目录或已有同 id 需求）时递增 n 重试，最多 5 次；
/// - 调用方需持有 `state.requirement_create_lock`（非 dry-run），与普通创建共享串行化语义。
pub(crate) async fn resolve_sub_req_id_and_target_dir(
    state: &AppState,
    parent: &Requirement,
    slug: Option<&str>,
    dry_run: bool,
) -> ApiResult<(String, PathBuf)> {
    let ticket = extract_ticket_prefix(&parent.id).ok_or_else(|| {
        ApiError::bad_request(format!(
            "cannot derive sub-requirement id from parent id `{}` (no numeric ticket segment)",
            parent.id
        ))
    })?;
    let slug_seg = match slug.map(str::trim).filter(|v| !v.is_empty()) {
        Some(s) => format!("-{}", ensure_safe_segment(s, "slug")?),
        None => String::new(),
    };
    let reqs = scan_hermes_requirements(state).await?;
    let mut max_seq: u64 = 0;
    let mut existing_ids: HashSet<String> = HashSet::new();
    for r in &reqs {
        existing_ids.insert(r.id.clone());
        if r.parent_req_id.as_deref() == Some(parent.id.as_str()) {
            if let Some(n) = sub_req_seq(&r.id) {
                if n > max_seq {
                    max_seq = n;
                }
            }
        }
    }
    // 平铺在父需求同级目录（req 根或父需求所在分组目录）。
    let parent_dir = req_dir_path(parent)?;
    let base = parent_dir
        .parent()
        .map(PathBuf::from)
        .ok_or_else(|| ApiError::bad_request("parent requirement dir has no parent directory"))?;
    let mut n = max_seq + 1;
    let max_retries: u32 = 5;
    for _attempt in 0..=max_retries {
        let req_id = format!("{ticket}-S{n}{slug_seg}");
        ensure_req_id(&req_id)?;
        let target_dir = base.join(&req_id);
        if existing_ids.contains(&req_id) || target_dir.exists() {
            n += 1;
            continue;
        }
        if !dry_run {
            fs::create_dir(&target_dir).await?;
        }
        return Ok((req_id, target_dir));
    }
    Err(ApiError::bad_request(
        "cannot allocate a free sub-requirement id after retries; try a different slug",
    ))
}

pub(crate) fn ensure_safe_segment(value: &str, field: &str) -> ApiResult<String> {
    let v = value.trim();
    if v.is_empty() || v == "." || v == ".." || v.len() > 128 {
        return Err(ApiError::bad_request(format!("invalid {field} segment")));
    }
    if !v
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        || v.contains('/')
        || v.contains('\\')
    {
        return Err(ApiError::bad_request(format!(
            "{field} segment must be ASCII and path-safe"
        )));
    }
    Ok(v.to_string())
}

pub(crate) fn normalize_projects(
    project: Option<&str>,
    projects: Option<&[String]>,
) -> Vec<String> {
    let mut values = Vec::new();
    if let Some(project) = clean_optional(project) {
        values.push(project);
    }
    if let Some(projects) = projects {
        values.extend(
            projects
                .iter()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty()),
        );
    }
    if values.is_empty() {
        values.push(DEFAULT_PROJECT_NAME.to_string());
    }
    unique_strings(values)
}
