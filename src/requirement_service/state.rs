use super::*;

pub(crate) fn extract_completed_at(state: &Value) -> Option<i64> {
    state
        .get("history")
        .and_then(Value::as_array)?
        .iter()
        .rev()
        .find(|h| h.get("status").and_then(Value::as_str) == Some("已完成"))
        .and_then(|h| h.get("at").and_then(Value::as_i64))
}

pub(crate) async fn read_requirement_state(dir: &Path) -> Result<Option<Value>> {
    let path = dir.join(STATE_FILE);
    if path.is_file() {
        return Ok(read_json_if_exists(&path).await);
    }
    Ok(None)
}

/// 需求状态达到「经验总结」及以上时，才允许自动推进关联线上问题。
pub(crate) fn should_auto_advance_issues(new_status: &str) -> bool {
    matches!(new_status, "经验总结" | "发布就绪" | "已完成")
}

/// 子需求轻量状态机流转合法性：需求创建 → 开发中 → 已合入；前两态可直接已取消；
/// 已合入/已取消为终态不可重开（后续增量拆新子需求）。无门禁（合入主需求不校验 Review Gate，
/// 代码审查在父需求内统一走）。
pub(crate) fn ensure_sub_req_status_transition(
    req: &Requirement,
    new_status: &str,
) -> ApiResult<()> {
    if !SUB_REQ_STATUSES.contains(&new_status) {
        return Err(ApiError::bad_request(format!(
            "子需求状态只能是 {} 之一，不能设置为 {new_status}（环境集成/发布/经验总结都在父需求流转）",
            SUB_REQ_STATUSES.join("/")
        )));
    }
    let from = req.status.as_str();
    if from == new_status {
        return Ok(());
    }
    let allowed: &[&str] = match from {
        "需求创建" => &["开发中", "已取消"],
        "开发中" => &["已合入", "已取消"],
        "已合入" | "已取消" => &[],
        // 兼容历史：子需求状态缺失/异常时允许进入任意子状态重新规整。
        _ => SUB_REQ_STATUSES,
    };
    if allowed.contains(&new_status) {
        Ok(())
    } else if allowed.is_empty() {
        Err(ApiError::bad_request(format!(
            "子需求 {from} 是终态，不可再流转；后续增量请拆新子需求"
        )))
    } else {
        Err(ApiError::bad_request(format!(
            "子需求状态不允许从 {from} 流转到 {new_status}（允许：{}）",
            allowed.join("/")
        )))
    }
}

/// 普通需求/issue 禁止使用子需求专属状态（避免状态集污染需求主流）。
pub(crate) fn ensure_status_allowed_for_non_sub(new_status: &str) -> ApiResult<()> {
    if SUB_ONLY_STATUSES.contains(&new_status) {
        return Err(ApiError::bad_request(format!(
            "{new_status} 是子需求专属状态，普通需求/线上问题不能使用"
        )));
    }
    Ok(())
}

/// 需求进入 >= 经验总结 后，自动把绑定的、仍处于排查中/已定位的线上问题推进到已修复。
/// 已修复/已复盘/已关闭的线上问题不会被回退；绑定错误或目录不可写时静默跳过。
pub(crate) async fn auto_advance_linked_issues(
    state: &AppState,
    req: &Requirement,
    new_status: &str,
) -> Vec<String> {
    if !should_auto_advance_issues(new_status) || req.issues.is_empty() {
        return Vec::new();
    }
    let mut advanced = Vec::new();
    for issue_id in &req.issues {
        let Ok(issue) = get_real_requirement(state, issue_id).await else {
            continue;
        };
        if issue.category.as_deref() != Some("线上问题") {
            continue;
        }
        if matches!(issue.status.as_str(), "已修复" | "已复盘" | "已关闭") {
            continue;
        }
        let Ok(issue_dir) = req_dir_path(&issue) else {
            continue;
        };
        let note = format!("关联需求 {} 进入{}，自动推进", req.id, new_status);
        let Ok(st) =
            write_requirement_status(&issue_dir.to_string_lossy(), "已修复", Some(&note)).await
        else {
            continue;
        };
        if matches!(st.get("changed").and_then(Value::as_bool), Some(true)) {
            let _ = record_status_transition_event(state, &issue, &st, Some(&note)).await;
            advanced.push(issue.id.clone());
        }
    }
    advanced
}

/// 文档是否已有实际内容：非空且至少一条非「待补充」的列表项。
pub(crate) fn doc_has_filled_items(body: &str) -> bool {
    if body.trim().is_empty() {
        return false;
    }
    body.lines()
        .any(|line| line.trim().starts_with("- ") && !line.contains("待补充"))
        || body
            .lines()
            .any(|line| line.trim().starts_with("| ") && !line.contains("待补充"))
}

/// 一次状态流转当时的门禁校验方式，写入 state.json 历史供状态流转卡片区分展示。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GateCheckMode {
    /// agent/API 推进：门禁已强制校验且通过（未通过会在写入前被拦截）。
    Passed,
    /// 人工在面板 UI 上修改：跳过门禁，未校验。
    Skipped,
    /// 系统内部流转（自动推进/类别切换/自动完成等）：不涉及门禁。
    None,
}

impl GateCheckMode {
    fn as_str(self) -> &'static str {
        match self {
            GateCheckMode::Passed => "passed",
            GateCheckMode::Skipped => "skipped",
            GateCheckMode::None => "none",
        }
    }
}

pub(crate) async fn write_requirement_status(
    req_dir: &str,
    new_status: &str,
    note: Option<&str>,
) -> Result<Value> {
    write_requirement_status_checked(req_dir, new_status, note, GateCheckMode::None).await
}

pub(crate) async fn write_requirement_status_checked(
    req_dir: &str,
    new_status: &str,
    note: Option<&str>,
    gate_check: GateCheckMode,
) -> Result<Value> {
    let dir = PathBuf::from(req_dir);
    let path = dir.join(STATE_FILE);
    let previous = read_requirement_state(&dir)
        .await?
        .unwrap_or_else(|| json!({ "version": 1, "history": [] }));
    let from = previous
        .get("status")
        .and_then(Value::as_str)
        .map(|s| s.to_string());
    let changed = from.as_deref() != Some(new_status);
    let mut history = previous
        .get("history")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let transition = if changed {
        json!({
            "status": new_status,
            "from": from,
            "at": now_ms(),
            "note": note.unwrap_or(""),
            "skippedStatuses": skipped_statuses(from.as_deref(), new_status),
            "gateCheck": gate_check.as_str()
        })
    } else {
        Value::Null
    };
    if changed {
        history.push(transition.clone());
    }
    if history.len() > 50 {
        history = history[history.len() - 50..].to_vec();
    }
    let state = json!({
        "version": 1,
        "status": new_status,
        "previousStatus": from,
        "changed": changed,
        "lastTransition": transition,
        "category": previous.get("category").cloned().unwrap_or(Value::Null),
        "updatedAt": now_ms(),
        "history": history
    });
    atomic_write_json(&path, &state).await?;
    Ok(state)
}

pub(crate) async fn write_requirement_category(req_dir: &str, new_category: &str) -> Result<Value> {
    let dir = PathBuf::from(req_dir);
    let path = dir.join(STATE_FILE);
    let previous = read_requirement_state(&dir)
        .await?
        .unwrap_or_else(|| json!({ "version": 1, "status": "开发中", "history": [] }));
    let state = json!({
        "version": 1,
        "status": previous.get("status").and_then(Value::as_str).unwrap_or("开发中"),
        "category": new_category,
        "updatedAt": now_ms(),
        "history": previous.get("history").cloned().unwrap_or_else(|| json!([]))
    });
    atomic_write_json(&path, &state).await?;
    Ok(state)
}

pub(crate) async fn write_requirement_ones(req_dir: &str, ones: &str) -> Result<String> {
    let path = PathBuf::from(req_dir).join("meta.md");
    let raw = fs::read_to_string(&path).await.unwrap_or_default();
    let normalized = raw.replace("\r\n", "\n");
    let value = ones.trim().to_string();
    let next = set_frontmatter_field(&normalized, "ones", &value);
    atomic_write_text(&path, &next).await?;
    Ok(value)
}
