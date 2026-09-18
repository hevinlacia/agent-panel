use super::*;

pub(crate) fn req_dir_path(req: &Requirement) -> ApiResult<PathBuf> {
    req.req_dir
        .as_deref()
        .filter(|v| !v.trim().is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| {
            ApiError::bad_request(format!("requirement has no writable dir: {}", req.id))
        })
}

pub(crate) async fn resolve_create_req_root(
    state: &AppState,
    requested: Option<&str>,
) -> ApiResult<PathBuf> {
    let roots = writable_req_roots(state).await?;
    if roots.is_empty() {
        return Err(ApiError::bad_request(
            "no requirementScanRoots configured; set /settings requirement scan roots first",
        ));
    }
    if let Some(raw) = clean_optional(requested) {
        let requested_path = normalize_user_path(&raw);
        for root in &roots {
            if same_or_child_path(&requested_path, root) {
                return Ok(root.clone());
            }
            if let Some(parent) = root.parent().and_then(|p| p.parent()) {
                if path_eq(&requested_path, parent) {
                    return Ok(root.clone());
                }
            }
        }
        return Err(ApiError::bad_request(format!(
            "root is not under configured requirementScanRoots: {raw}"
        )));
    }
    Ok(roots[0].clone())
}

pub(crate) async fn writable_req_roots(state: &AppState) -> ApiResult<Vec<PathBuf>> {
    let cfg = read_config(state).await?;
    let roots = normalize_scan_roots(cfg.requirement_scan_roots);
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for root in roots {
        let root_path = PathBuf::from(root);
        let mut candidates = Vec::new();
        if root_path.file_name().and_then(|v| v.to_str()) == Some("req") {
            candidates.push(root_path.clone());
        } else {
            candidates.push(root_path.join(".agents/req"));
            candidates.push(root_path.join("req"));
        }
        for candidate in candidates {
            let key = normalize_path_string(&candidate);
            if seen.insert(key) {
                out.push(candidate);
            }
        }
    }
    Ok(out)
}

pub(crate) async fn ensure_requirement_dir_writable(state: &AppState, dir: &Path) -> ApiResult<()> {
    ensure_path_inside_req_roots(state, dir).await
}

pub(crate) async fn ensure_path_inside_req_roots(state: &AppState, path: &Path) -> ApiResult<()> {
    let roots = writable_req_roots(state).await?;
    if roots.iter().any(|root| same_or_child_path(path, root)) {
        Ok(())
    } else {
        Err(ApiError::bad_request(format!(
            "path is outside configured requirement roots: {}",
            path.to_string_lossy()
        )))
    }
}

pub(crate) fn same_or_child_path(path: &Path, root: &Path) -> bool {
    normalize_path_string(path) == normalize_path_string(root)
        || normalize_path_string(path).starts_with(&(normalize_path_string(root) + "/"))
}

pub(crate) fn path_eq(a: &Path, b: &Path) -> bool {
    normalize_path_string(a) == normalize_path_string(b)
}

pub(crate) fn normalize_path_string(path: &Path) -> String {
    let abs = if path.is_absolute() {
        path.to_path_buf()
    } else {
        env::current_dir().unwrap_or_default().join(path)
    };
    let mut parts = Vec::new();
    for comp in abs.components() {
        match comp {
            std::path::Component::ParentDir => {
                parts.pop();
            }
            std::path::Component::CurDir => {}
            _ => parts.push(comp.as_os_str().to_string_lossy().to_string()),
        }
    }
    if parts.is_empty() {
        "/".into()
    } else if parts.first().map(|s| s.as_str()) == Some("/") {
        format!("/{}", parts[1..].join("/"))
    } else {
        parts.join("/")
    }
}

pub(crate) fn normalize_user_path(raw: &str) -> PathBuf {
    let trimmed = raw.trim();
    if trimmed == "~" {
        home_dir().unwrap_or_default()
    } else if let Some(rest) = trimmed.strip_prefix("~/") {
        home_dir().unwrap_or_default().join(rest)
    } else {
        let path = PathBuf::from(trimmed);
        if path.is_absolute() {
            path
        } else {
            env::current_dir().unwrap_or_default().join(path)
        }
    }
}

pub(crate) fn ensure_req_id(value: &str) -> ApiResult<String> {
    let v = value.trim();
    if v == DEFAULT_REQ_ID || v.len() < 2 || v.len() > 128 {
        return Err(ApiError::bad_request("invalid reqId length"));
    }
    if !v.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        || v.starts_with('-')
        || v.ends_with('-')
        || v.contains("--")
    {
        return Err(ApiError::bad_request(
            "reqId must use ASCII letters, numbers and single hyphens only",
        ));
    }
    Ok(v.to_string())
}
