use super::*;

/// Parse a reqId template containing a single `{seq}` placeholder into
/// `(prefix, suffix)`. `WMS-{seq}-demo` -> `("WMS", "-demo")`.
/// 线上问题独立编号池前缀：与普通需求（WMS-）分开计数，目录仍共用 req 根；
/// 目录身份证创建后不变，setCategory 切换类别不改号。
pub(crate) const ISSUE_ID_PREFIX: &str = "WMS-INC";
/// 测试问题独立编号池：承接 UAT 测试反馈中不属于常规需求的轻量问题。
pub(crate) const TEST_ISSUE_ID_PREFIX: &str = "WMS-TST";
/// 需求组（引用式需求组，group.json）独立编号池：组是需求聚合视图，不占用 WMS-<seq> 需求池。
pub(crate) const GROUP_ID_PREFIX: &str = "WMS-GRP";

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

/// 创建时按类别/形态规范化 reqId 模板：
/// - 需求组（members 非空）强制使用 `WMS-GRP-{seq}` 独立编号池，规则与 issue 家族一致；
/// - 空模板给类别默认池（需求 `WMS-{seq}`，线上问题 `WMS-INC-{seq}`，测试问题 `WMS-TST-{seq}`）；
/// - issue 类别强制使用本类别前缀：模板改写前缀保留 suffix；具体 id 校验形态，避免误用需求池；
/// - 需求模板原样透传。
pub(crate) fn normalize_create_id_template(
    raw: &str,
    category: &str,
    is_group: bool,
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
        let re = Regex::new(&format!("^{GROUP_ID_PREFIX}-(\\d+)(-|$)")).expect("valid regex");
        if !re.is_match(v) {
            return Err(ApiError::bad_request(format!(
                "需求组 reqId 必须使用 {GROUP_ID_PREFIX}-<序号> 独立编号（如 {GROUP_ID_PREFIX}-001-slug）；组是需求聚合视图，不占用 WMS-<seq> 需求池"
            )));
        }
        return Ok(v.to_string());
    }
    let Some(prefix) = issue_id_prefix(category) else {
        return Ok(if v.is_empty() {
            "WMS-{seq}".to_string()
        } else {
            v.to_string()
        });
    };
    if v.is_empty() {
        return Ok(format!("{prefix}-{{seq}}"));
    }
    if v.contains("{seq}") {
        let (_, suffix) = split_seq_template(v)?;
        return Ok(format!("{prefix}-{{seq}}{suffix}"));
    }
    let re = Regex::new(&format!("^{prefix}-(\\d+)(-|$)")).expect("valid regex");
    if !re.is_match(v) {
        let hint = if category == "线上问题" {
            "；复现/验证代码可直接登记分支开发（仅测试环境），正式生产修复承接请创建 category=需求 的普通需求并绑定本问题"
        } else {
            ""
        };
        return Err(ApiError::bad_request(format!(
            "{category} reqId 必须使用 {prefix}-<序号> 独立编号（如 {prefix}-003-slug）{hint}"
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
/// requirements on disk.
pub(crate) async fn allocate_next_seq(
    state: &AppState,
    prefix: &str,
    floor: Option<u64>,
) -> ApiResult<u64> {
    let reqs = scan_hermes_requirements(state).await?;
    let ids: Vec<String> = reqs.iter().map(|r| r.id.clone()).collect();
    Ok(compute_next_seq_from_ids(&ids, prefix, floor))
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
