use super::*;

#[derive(Debug)]
pub(crate) struct ReviewGateDecision {
    pub(crate) status: String,
    pub(crate) label: String,
    pub(crate) allows_testing: bool,
    pub(crate) reason: String,
    pub(crate) source: Option<String>,
    pub(crate) review_path: PathBuf,
    pub(crate) ai_review_path: PathBuf,
    pub(crate) actions: Vec<String>,
    pub(crate) stale_repos: Vec<Value>,
    pub(crate) incremental_review: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ReviewSnapshotDrift {
    pub(crate) repo_name: String,
    pub(crate) branch: String,
    pub(crate) project_path: Option<PathBuf>,
    pub(crate) reviewed_target_ref: String,
    pub(crate) reviewed_target_commit: String,
    pub(crate) current_target_ref: String,
    pub(crate) current_target_commit: String,
}

pub(crate) async fn review_gate_json(req: &Requirement) -> ApiResult<Value> {
    let gate = review_gate_decision(req).await?;
    let dir = req_dir_path(req)?;
    let (risk_tags, inventory_risk) = code_review_risk_for(&dir).await;
    let checklist = review_checklist_summary(&load_review_checklist(&dir).await);
    Ok(json!({
        "ok": true,
        "reqId": req.id,
        "gate": {
            "status": gate.status,
            "label": gate.label,
            "allowsTesting": gate.allows_testing,
            "reason": gate.reason,
            "source": gate.source,
            "reviewPath": gate.review_path.to_string_lossy(),
            "aiReviewPath": gate.ai_review_path.to_string_lossy(),
            "riskTags": risk_tags,
            "inventoryRisk": inventory_risk,
            "checklist": checklist,
            "actions": gate.actions,
            "staleRepos": gate.stale_repos,
            "incrementalReview": gate.incremental_review,
            "checkedAt": now_ms(),
        }
    }))
}

pub(crate) async fn ensure_review_gate_allows_testing(req: &Requirement) -> ApiResult<()> {
    let gate = review_gate_decision(req).await?;
    if gate.allows_testing {
        return Ok(());
    }
    Err(ApiError::bad_request(format!(
        "Code Review Gate 未通过（{}）：{}。请先补充 review.md / code-review-ai.md 并明确 `Review Gate: PASS`，或在 review.md 记录 `Review Gate: WAIVED` + 豁免原因。",
        gate.label, gate.reason
    )))
}

/// 审查清单（review-checklist.json）的加载结果。
enum ReviewChecklistState {
    Missing,
    Invalid(String),
    Valid(Value),
}

fn review_checklist_schema_hint() -> String {
    "期望格式：{\"reqId\":\"<需求ID>\",\"items\":[{\"id\":\"C1\",\"title\":\"检查项标题\",\"conclusion\":\"pass|fail|na\",\"note\":\"选填说明\",\"evidence\":\"选填证据\"}]}；items 必须是非空数组，conclusion 只允许 pass / fail / na".to_string()
}

/// 加载并校验审查清单；格式问题一律返回 Invalid（带精确修复指引），不猜格式。
async fn load_review_checklist(req_dir: &Path) -> ReviewChecklistState {
    let Ok(raw) = tokio::fs::read_to_string(req_dir.join(REVIEW_CHECKLIST_FILE)).await else {
        return ReviewChecklistState::Missing;
    };
    let Ok(doc) = serde_json::from_str::<Value>(&raw) else {
        return ReviewChecklistState::Invalid(
            "review-checklist.json 不是合法 JSON；请调 PUT /api/requirement/review-checklist 重建（写入时自动校验格式）"
                .to_string(),
        );
    };
    let Some(items) = doc
        .get("items")
        .and_then(Value::as_array)
        .filter(|a| !a.is_empty())
    else {
        return ReviewChecklistState::Invalid(
            "review-checklist.json 的 items 必须是非空数组；请调 PUT /api/requirement/review-checklist 重建"
                .to_string(),
        );
    };
    for item in items {
        let conclusion = item
            .get("conclusion")
            .and_then(Value::as_str)
            .map(str::trim)
            .unwrap_or_default()
            .to_lowercase();
        if !["pass", "fail", "na"].contains(&conclusion.as_str()) {
            let id = item.get("id").and_then(Value::as_str).unwrap_or("?");
            let title = item.get("title").and_then(Value::as_str).unwrap_or("");
            return ReviewChecklistState::Invalid(format!(
                "清单项 {id}（{title}）的 conclusion = \"{conclusion}\" 非法：只允许 pass / fail / na；请调 PUT /api/requirement/review-checklist 修正"
            ));
        }
    }
    ReviewChecklistState::Valid(doc)
}

/// 门禁详情用：清单概要（机器判定状态 + 逐项内容）。
fn review_checklist_summary(state: &ReviewChecklistState) -> Value {
    match state {
        ReviewChecklistState::Missing => {
            json!({ "present": false, "total": 0, "concluded": 0, "failed": 0 })
        }
        ReviewChecklistState::Invalid(msg) => {
            json!({ "present": false, "error": msg, "total": 0, "concluded": 0, "failed": 0 })
        }
        ReviewChecklistState::Valid(doc) => {
            let items = doc
                .get("items")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            let failed: Vec<Value> = items
                .iter()
                .filter(|i| i.get("conclusion").and_then(Value::as_str) == Some("fail"))
                .map(|i| json!({ "id": i.get("id"), "title": i.get("title") }))
                .collect();
            json!({
                "present": true,
                "total": items.len(),
                "concluded": items.len(),
                "failed": failed.len(),
                "failedItems": failed,
                "items": items,
            })
        }
    }
}

pub(crate) async fn review_gate_decision(req: &Requirement) -> ApiResult<ReviewGateDecision> {
    let dir = req_dir_path(req)?;
    let review_path = dir.join("review.md");
    let ai_review_path = dir.join("code-review-ai.md");
    let (_risk_tags, inventory_risk) = code_review_risk_for(&dir).await;
    let mut docs = Vec::<(String, PathBuf, String)>::new();
    for (label, path) in [
        ("review.md".to_string(), review_path.clone()),
        ("code-review-ai.md".to_string(), ai_review_path.clone()),
    ] {
        if let Ok(raw) = fs::read_to_string(&path).await {
            if !raw.trim().is_empty() {
                docs.push((label, path, raw));
            }
        }
    }
    if docs.is_empty() {
        return Ok(ReviewGateDecision {
            status: "missing".into(),
            label: "未执行".into(),
            allows_testing: false,
            reason: "未找到 review.md 或 code-review-ai.md 的代码审查结论".into(),
            source: None,
            review_path,
            ai_review_path,
            actions: vec![
                "生成 code-review.json / code-review-ai.md，或手工补充 review.md".into(),
                "在审查结论中写明 `Review Gate: PASS`、`Review Gate: BLOCKED` 或 `Review Gate: WAIVED`".into(),
            ],
            stale_repos: Vec::new(),
            incremental_review: None,
        });
    }
    for (source, path, raw) in &docs {
        if review_gate_waived(raw) {
            let stale = review_snapshot_drifts(&dir).await;
            if !stale.is_empty() {
                return Ok(stale_review_gate_decision(
                    source.clone(),
                    review_path,
                    ai_review_path,
                    &stale,
                ));
            }
            if review_artifact_requires_fresh_review(&dir, path).await {
                return Ok(refresh_stale_review_gate_decision(
                    source.clone(),
                    review_path,
                    ai_review_path,
                ));
            }
            return Ok(ReviewGateDecision {
                status: "waived".into(),
                label: "用户豁免".into(),
                allows_testing: true,
                reason: "review 文档记录了豁免结论".into(),
                source: Some(source.clone()),
                review_path,
                ai_review_path,
                actions: vec!["保留豁免原因，测试阶段重点覆盖高风险改动".into()],
                stale_repos: Vec::new(),
                incremental_review: None,
            });
        }
    }
    for (source, _path, raw) in &docs {
        if review_gate_blocked(raw) {
            return Ok(ReviewGateDecision {
                status: "blocked".into(),
                label: "有阻塞项".into(),
                allows_testing: false,
                reason: "review 文档存在严重问题、阻塞项或明确 BLOCKED 结论".into(),
                source: Some(source.clone()),
                review_path,
                ai_review_path,
                actions: vec![
                    "修复严重问题后重新审查".into(),
                    "若业务确认可带风险提测，在 review.md 明确 `Review Gate: WAIVED` 和豁免原因"
                        .into(),
                ],
                stale_repos: Vec::new(),
                incremental_review: None,
            });
        }
    }
    for (source, path, raw) in &docs {
        if review_gate_passed(raw) {
            // 审查清单硬校验：清单列出的每一项都必须有结论（pass/fail/na），全部有结论才继续放行链路。
            let checklist = load_review_checklist(&dir).await;
            match &checklist {
                ReviewChecklistState::Missing => {
                    return Ok(ReviewGateDecision {
                        status: "checklist-missing".into(),
                        label: "审查清单缺失".into(),
                        allows_testing: false,
                        reason: "review-checklist.json 不存在：门禁要求审查清单列出的每一项都有结论（pass/fail/na）后才能放行".into(),
                        source: Some(source.clone()),
                        review_path: review_path.clone(),
                        ai_review_path: ai_review_path.clone(),
                        actions: vec![
                            "整理本次审查要点（每仓库关键风险、幂等/并发、库存、配置、部署顺序等），逐项给出结论".to_string(),
                            format!("调用 PUT /api/requirement/review-checklist 写入清单，body 示例：{{\"reqId\":\"{}\",\"items\":[{{\"id\":\"C1\",\"title\":\"幂等与并发安全\",\"conclusion\":\"pass\",\"note\":\"...\",\"evidence\":\"...\"}}]}}", req.id),
                            "conclusion 枚举：pass=通过 / fail=未通过 / na=不适用；存在 fail 项会被拦截；保存后自动渲染进 review.md「## 审查清单」小节".to_string(),
                        ],
                        stale_repos: Vec::new(),
                        incremental_review: None,
                    });
                }
                ReviewChecklistState::Invalid(msg) => {
                    return Ok(ReviewGateDecision {
                        status: "checklist-error".into(),
                        label: "审查清单格式错误".into(),
                        allows_testing: false,
                        reason: msg.clone(),
                        source: Some(source.clone()),
                        review_path: review_path.clone(),
                        ai_review_path: ai_review_path.clone(),
                        actions: vec![
                            "按上方错误信息修正清单；推荐直接调 PUT /api/requirement/review-checklist 重建（写入时自动校验格式）".to_string(),
                            review_checklist_schema_hint(),
                        ],
                        stale_repos: Vec::new(),
                        incremental_review: None,
                    });
                }
                ReviewChecklistState::Valid(doc) => {
                    let items = doc
                        .get("items")
                        .and_then(Value::as_array)
                        .cloned()
                        .unwrap_or_default();
                    let failed: Vec<String> = items
                        .iter()
                        .filter(|i| i.get("conclusion").and_then(Value::as_str) == Some("fail"))
                        .map(|i| {
                            format!(
                                "{}（{}）",
                                i.get("id").and_then(Value::as_str).unwrap_or("?"),
                                i.get("title").and_then(Value::as_str).unwrap_or("")
                            )
                        })
                        .collect();
                    if !failed.is_empty() {
                        return Ok(ReviewGateDecision {
                            status: "blocked".into(),
                            label: "清单存在未通过项".into(),
                            allows_testing: false,
                            reason: format!("审查清单 {} 项结论为 fail：{}", failed.len(), failed.join("、")),
                            source: Some(source.clone()),
                            review_path: review_path.clone(),
                            ai_review_path: ai_review_path.clone(),
                            actions: vec![
                                "修复对应问题后重新审查，把结论改为 pass（写明修复 commit/验证证据）".to_string(),
                                "业务确认带风险提测时，在 review.md 明确 `Review Gate: WAIVED` + 豁免原因".to_string(),
                            ],
                            stale_repos: Vec::new(),
                            incremental_review: None,
                        });
                    }
                    // 全部 pass/na：清单齐备，继续库存专项与新鲜度检查。
                }
            }
            // 库存高危风险：即使写了 PASS，若未包含库存账本专项评估，门禁仍不通过
            if inventory_risk && !review_has_inventory_evidence(raw) {
                return Ok(ReviewGateDecision {
                    status: "inventory-pending".into(),
                    label: "库存风险未评估".into(),
                    allows_testing: false,
                    reason: "本次改动命中库存高危风险，但 review 未包含库存账本矩阵（单据活跃/死亡、DB 库存、redis 可用量、重复释放、遗漏占用、幂等、验证证据）。请补充后重新给出 PASS。".into(),
                    source: Some(source.clone()),
                    review_path,
                    ai_review_path,
                    actions: vec![
                        "在 review.md / code-review-ai.md 补充 `## 库存账本评估`：单据最终是活跃单还是死亡单；DB 库存变化(onHandQty/allocatedQty/临时库位/回库单)；redis 可用量变化(建单-、真取消+、恢复-、回退是否保持占用)；是否存在重复释放(cancel+delete/intercept/MQ重试/接口重试)；是否存在遗漏占用(回池后继续分配/拣货但未重新占用)；是否有幂等保护；验证证据(DB前后/redis前后/日志/单测/边界状态)".into(),
                        "明确 `Review Gate: PASS` 后重试推进；若业务确认带风险提测，使用 `Review Gate: WAIVED` + 豁免原因".into(),
                    ],
                    stale_repos: Vec::new(),
                    incremental_review: None,
                });
            }
            let stale = review_snapshot_drifts(&dir).await;
            if !stale.is_empty() {
                return Ok(stale_review_gate_decision(
                    source.clone(),
                    review_path,
                    ai_review_path,
                    &stale,
                ));
            }
            if review_artifact_requires_fresh_review(&dir, path).await {
                return Ok(refresh_stale_review_gate_decision(
                    source.clone(),
                    review_path,
                    ai_review_path,
                ));
            }
            return Ok(ReviewGateDecision {
                status: "passed".into(),
                label: "审查通过".into(),
                allows_testing: true,
                reason: "review 文档记录了通过结论，审查清单全部项均有结论，且审查快照覆盖当前需求分支 HEAD".into(),
                source: Some(source.clone()),
                review_path,
                ai_review_path,
                actions: vec!["可以推进到测试中；测试阶段按 review 的验收要点回归".into()],
                stale_repos: Vec::new(),
                incremental_review: None,
            });
        }
    }
    Ok(ReviewGateDecision {
        status: "pending".into(),
        label: "待确认".into(),
        allows_testing: false,
        reason: "已找到 review 文档，但缺少明确 PASS / BLOCKED / WAIVED 结论".into(),
        source: docs.first().map(|(source, _, _)| source.clone()),
        review_path,
        ai_review_path,
        actions: vec![
            "在 review.md 顶部补充 `Review Gate: PASS` / `BLOCKED` / `WAIVED`".into(),
            "若使用 AI 审查，确认 code-review-ai.md 后同步结论到 review.md".into(),
        ],
        stale_repos: Vec::new(),
        incremental_review: None,
    })
}

pub(crate) fn stale_review_gate_decision(
    source: String,
    review_path: PathBuf,
    ai_review_path: PathBuf,
    stale: &[ReviewSnapshotDrift],
) -> ReviewGateDecision {
    ReviewGateDecision {
        status: "stale".into(),
        label: "需增量审查".into(),
        allows_testing: false,
        reason: format!(
            "审查结论后仍有 {} 个需求分支 HEAD 发生变化；可优先生成 code-review-incremental.json，仅审查已审 commit 到当前 HEAD 的新增 diff。",
            stale.len()
        ),
        source: Some(source),
        review_path,
        ai_review_path,
        actions: vec![
            "优先点击“生成增量审查包”，只审 reviewedTargetCommit → currentTargetCommit 的新增提交和 diff".into(),
            "把增量审查结论追加/更新到 code-review-ai.md 或 review.md，并重新写明 `Review Gate: PASS` / `BLOCKED` / `WAIVED`".into(),
            "若提示非线性历史、rebase 或 force-push，再回退到全量 code-review.json 审查".into(),
        ],
        stale_repos: stale
            .iter()
            .cloned()
            .map(review_snapshot_drift_json)
            .collect(),
        incremental_review: None,
    }
}

pub(crate) fn refresh_stale_review_gate_decision(
    source: String,
    review_path: PathBuf,
    ai_review_path: PathBuf,
) -> ReviewGateDecision {
    ReviewGateDecision {
        status: "stale".into(),
        label: "需确认增量审查".into(),
        allows_testing: false,
        reason: "代码差异快照或增量审查包晚于当前 review 结论；请确认新增 diff 已审查后再放行。".into(),
        source: Some(source),
        review_path,
        ai_review_path,
        actions: vec![
            "若是测试中追加提交，优先审查 code-review-incremental.json，而不是重审完整 code-review.json".into(),
            "更新 code-review-ai.md / review.md 的审查概览，注明增量覆盖范围和结论".into(),
            "确认所有新增风险标签（尤其库存）已覆盖后，重新写明 `Review Gate: PASS` / `BLOCKED` / `WAIVED`".into(),
        ],
        stale_repos: Vec::new(),
        incremental_review: None,
    }
}

pub(crate) async fn review_artifact_requires_fresh_review(
    req_dir: &Path,
    review_doc_path: &Path,
) -> bool {
    let Some(review_doc_updated_at) = file_modified_ms(review_doc_path).await else {
        return false;
    };
    if let Some(updated_at) = file_modified_ms(&req_dir.join(CODE_REVIEW_INCREMENTAL_FILE)).await {
        if updated_at > review_doc_updated_at {
            return true;
        }
    }
    if let Some(review) = read_json_if_exists(&req_dir.join(CODE_REVIEW_FILE)).await {
        if !review
            .get("previousReviewedSnapshot")
            .unwrap_or(&Value::Null)
            .is_null()
        {
            if let Some(updated_at) = file_modified_ms(&req_dir.join(CODE_REVIEW_FILE)).await {
                return updated_at > review_doc_updated_at;
            }
        }
    }
    false
}

pub(crate) async fn file_modified_ms(path: &Path) -> Option<i64> {
    let meta = fs::metadata(path).await.ok()?;
    Some(system_time_to_ms(meta.modified().unwrap_or(UNIX_EPOCH)))
}

pub(crate) fn review_gate_waived(raw: &str) -> bool {
    let lower = raw.to_lowercase();
    lower.contains("review gate: waived")
        || raw.contains("用户豁免")
        || raw.contains("代码审查豁免")
}

pub(crate) fn review_gate_blocked(raw: &str) -> bool {
    let lower = raw.to_lowercase();
    if lower.contains("review gate: blocked")
        || raw.contains("不可提测")
        || raw.contains("审查不通过")
    {
        return true;
    }
    if raw.contains('❌')
        && (raw.contains("阻塞") || raw.contains("必须修复") || raw.contains("严重问题"))
    {
        return true;
    }
    review_gate_section(raw, &["严重问题", "必须修复", "Blocking Items", "阻塞项"])
        .map(|section| !review_section_is_empty(&section))
        .unwrap_or(false)
}

pub(crate) fn review_gate_passed(raw: &str) -> bool {
    let lower = raw.to_lowercase();
    if lower.contains("review gate: pass")
        || lower.contains("result: pass")
        || raw.contains("代码审查通过")
        || raw.contains("审查通过")
        || raw.contains("可提测")
        || raw.contains("无阻塞")
    {
        return true;
    }
    review_gate_section(raw, &["严重问题", "必须修复"])
        .map(|section| review_section_is_empty(&section))
        .unwrap_or(false)
}
