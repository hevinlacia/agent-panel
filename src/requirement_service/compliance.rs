use super::*;

/// GET /api/requirement/compliance：跨状态合规快照。
///
/// 解决「中途接手的新会话不知道其他状态要求」：不需要读过所有 phase prompt，
/// 查这一个接口即可拿到 —— 状态历史（含重算的跳过状态与逐次 gateCheck）+
/// 各阶段契约产物齐缺（自测清单 / UAT 回归 / 审查门禁 / release-check / attachments）+
/// 可执行的 gaps 清单。release-check、人工核查材料准备、经验总结、发布预检都应先拉它。
pub(crate) async fn api_requirement_compliance(
    State(state): State<AppState>,
    Query(q): Query<ComplianceQuery>,
) -> ApiResult<Json<Value>> {
    let id = q
        .id
        .or(q.req_id)
        .ok_or_else(|| ApiError::bad_request("missing id"))?;
    let req = get_real_requirement(&state, &id).await?;
    let dir = req_dir_path(&req)?;
    let state_json = read_requirement_state(&dir)
        .await
        .ok()
        .flatten()
        .unwrap_or_else(|| json!({ "version": 1, "status": req.status, "history": [] }));
    let history = state_json
        .get("history")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    // 1) 流转路径 + 按状态序重算被跳过的状态 + 逐次 gateCheck 汇总。
    // 不信任 history 条目自填的 skippedStatuses（WMS-136 实测 UI 直跳时为空数组），
    // 统一用 REQ_STATUSES 序号重算。
    let mut path = Vec::new();
    let mut missed: Vec<String> = Vec::new();
    let mut gate_checks: HashMap<String, String> = HashMap::new();
    for item in &history {
        let from = item
            .get("from")
            .and_then(Value::as_str)
            .map(str::to_string);
        let to = item
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        if !to.is_empty() {
            for s in skipped_statuses(from.as_deref(), &to) {
                if !missed.contains(&s) {
                    missed.push(s);
                }
            }
            if let Some(gc) = item.get("gateCheck").and_then(Value::as_str) {
                gate_checks.insert(to.clone(), gc.to_string());
            }
        }
        path.push(json!({
            "from": from,
            "status": to,
            "at": item.get("at"),
            "note": item.get("note"),
            "gateCheck": item.get("gateCheck"),
            "skippedStatuses": item.get("skippedStatuses"),
        }));
    }

    // 2) 各阶段契约产物快照（复用门禁同一套解析，结论与门禁一致）。
    let selftest = selftest_checklist_eval(&req).await;
    let uat = uat_regression_eval(&req).await;
    let review = review_gate_decision(&req).await.ok();
    let release_check = release_check_snapshot(&dir).await;
    let attachments = attachments_snapshot(&dir).await;

    // 3) gaps：可执行的缺失清单（按当前状态位置裁剪，避免对澄清阶段报 UAT 缺失）。
    let flow_pos =
        |s: &str| REQ_FLOW_STATUSES.iter().position(|x| *x == s);
    let cur = flow_pos(&req.status);
    let mut gaps: Vec<String> = Vec::new();
    for s in &missed {
        gaps.push(format!("历史流转跳过了状态「{s}」"));
    }
    if let Some(ci) = cur {
        let idx_test = flow_pos("测试中");
        let idx_ready = flow_pos("发布就绪");
        if idx_test.is_some_and(|ti| ci >= ti) {
            if !uat.problems.is_empty() {
                gaps.push(format!(
                    "UAT 回归证据缺失：{}",
                    uat.problems.iter().take(3).cloned().collect::<Vec<_>>().join("；")
                ));
            }
            if !selftest.problems.is_empty() {
                gaps.push(format!(
                    "自测清单当前不满足：{}",
                    selftest.problems.iter().take(2).cloned().collect::<Vec<_>>().join("；")
                ));
            }
        }
        if idx_ready.is_some_and(|ri| ci >= ri) && release_check.get("found") != Some(&json!(true))
        {
            gaps.push("已到发布就绪但 release-check.md 缺失：发布预检未完成".to_string());
        }
    }
    if let Some(r) = &review {
        if !r.allows_testing {
            gaps.push(format!("代码审查门禁当前不满足：{}", r.reason));
        }
    }

    Ok(Json(json!({
        "ok": true,
        "id": req.id,
        "title": req.title,
        "status": req.status,
        "category": req.category,
        "source": req.source,
        "hotfix": req.is_hotfix(),
        "flow": {
            "path": path,
            "missedStatuses": missed,
            "gateChecks": gate_checks,
        },
        "artifacts": {
            "selftestChecklist": {
                "problems": selftest.problems,
                "warnings": selftest.warnings,
            },
            "uatRegression": {
                "problems": uat.problems,
                "warnings": uat.warnings,
            },
            "reviewGate": review.map(|r| json!({
                "decision": r.status,
                "label": r.label,
                "allowsTesting": r.allows_testing,
                "reason": r.reason,
                "warnings": r.warnings,
            })),
            "releaseCheck": release_check,
            "attachments": attachments,
        },
        "gaps": gaps,
        "checkedAt": now_ms(),
    })))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ComplianceQuery {
    pub(crate) id: Option<String>,
    #[serde(alias = "reqId")]
    pub(crate) req_id: Option<String>,
}

/// release-check.md 结论快照：结论行 + 阻塞项计数（宽松解析，找不到就是原始行文本）。
async fn release_check_snapshot(dir: &Path) -> Value {
    let path = dir.join("release-check.md");
    if !path.is_file() {
        return json!({ "found": false });
    }
    let body = tokio::fs::read_to_string(&path).await.unwrap_or_default();
    let mut result_line: Option<String> = None;
    let mut blocking_line: Option<String> = None;
    let mut blocking_count: Option<u64> = None;
    for line in body.lines() {
        let t = line.trim();
        if result_line.is_none() && t.contains("Result:") {
            result_line = Some(t.to_string());
        }
        if blocking_line.is_none() && t.contains("阻塞项") {
            blocking_line = Some(t.to_string());
            blocking_count = extract_blocking_count(t);
        }
    }
    json!({
        "found": true,
        "resultLine": result_line,
        "blockingLine": blocking_line,
        "blockingCount": blocking_count,
    })
}

/// 从「阻塞项：0」/「阻塞项 2 个」等行里提取数字。
fn extract_blocking_count(line: &str) -> Option<u64> {
    let idx = line.find("阻塞项")? + "阻塞项".len();
    let rest = &line[idx..];
    let mut chars = rest.chars().peekable();
    while let Some(c) = chars.peek() {
        if c.is_ascii_digit() {
            break;
        }
        chars.next();
    }
    let mut s = String::new();
    for c in chars {
        if c.is_ascii_digit() {
            s.push(c);
        } else if !s.is_empty() {
            break;
        }
    }
    s.parse().ok()
}

/// attachments/ 上线资产目录快照。
async fn attachments_snapshot(dir: &Path) -> Value {
    let path = dir.join("attachments");
    if !path.is_dir() {
        return json!({ "exists": false, "files": [], "sqlCount": 0, "releaseConfig": false });
    }
    let mut files: Vec<String> = Vec::new();
    let mut sql_count = 0usize;
    if let Ok(mut rd) = tokio::fs::read_dir(&path).await {
        while let Ok(Some(entry)) = rd.next_entry().await {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.ends_with(".sql") {
                sql_count += 1;
            }
            files.push(name);
        }
    }
    files.sort();
    json!({
        "exists": true,
        "files": files,
        "sqlCount": sql_count,
        "releaseConfig": path.join("release-config.md").is_file(),
    })
}
