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

/// 开发推动的需求测试门禁：进入「测试中」前必须先完成测试场景文档。
pub(crate) async fn ensure_test_scenario_allows_testing(req: &Requirement) -> ApiResult<()> {
    let Some(dir) = req.req_dir.as_deref() else {
        return Ok(());
    };
    let body = tokio::fs::read_to_string(PathBuf::from(dir).join("test-scenario.md"))
        .await
        .unwrap_or_default();
    if doc_has_filled_items(&body) {
        return Ok(());
    }
    Err(ApiError::bad_request(
        "开发推动的需求进入「测试中」前必须先完成测试场景文档 test-scenario.md（需求说明 + 开发评估的测试范围 + 测试覆盖场景）；请在需求详情页「测试场景」面板生成并填写，或让 agent 通过 doc API 更新",
    ))
}

pub(crate) async fn write_requirement_status(
    req_dir: &str,
    new_status: &str,
    note: Option<&str>,
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
            "skippedStatuses": skipped_statuses(from.as_deref(), new_status)
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
