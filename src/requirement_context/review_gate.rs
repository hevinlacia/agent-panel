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
    /// 放行但需用户注意的警示（如 P1 严重问题待修复、无法测试项结果未知）。
    pub(crate) warnings: Vec<String>,
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
    let annotations = review_annotations_summary(&review_annotations_gate_state(&dir).await);
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
            "annotations": annotations,
            "warnings": gate.warnings,
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
        "Code Review Gate 未通过（{}）：{}。请加载 agent-panel-code-review skill 按流程执行审查（POST /api/requirement/review-materials 备料 → 深度审查 → 写 code-review-ai.md 并带 skill 标记 → PUT annotations/review-checklist）；门禁发现问题后主 agent 必须结合当前会话的需求上下文亲自全面复审，不能只派子 agent，或在 review.md 记录 `Review Gate: WAIVED` + 豁免原因。",
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

/// 代码问题备注（code-annotations.json）的门禁状态。
enum ReviewAnnotationsState {
    /// 文件缺失、非 JSON 或 files 为空
    Missing,
    /// 存在但 reviewedCommit 提交指纹与最新审查材料不一致（备注锚定旧 diff）
    Stale(String),
    /// 存在且指纹覆盖当前材料
    Valid,
}

/// 加载并核对代码问题备注：必须存在非空 files，且 reviewedCommit 指纹覆盖
/// 最新审查材料（code-review.json / code-review-incremental.json 较新者）的每个仓库。
/// 与差异页备注 stale 判定（annotationStaleRepos）同源；无材料快照时只要求备注存在。
async fn review_annotations_gate_state(req_dir: &Path) -> ReviewAnnotationsState {
    let Some(annotations) = read_json_if_exists(&req_dir.join(CODE_ANNOTATIONS_FILE)).await else {
        return ReviewAnnotationsState::Missing;
    };
    let has_files = annotations
        .get("files")
        .and_then(Value::as_array)
        .map(|files| {
            files.iter().any(|f| {
                !f.get("repo")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .unwrap_or_default()
                    .is_empty()
                    && !f.get("path")
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .unwrap_or_default()
                        .is_empty()
            })
        })
        .unwrap_or(false);
    if !has_files {
        return ReviewAnnotationsState::Missing;
    }
    let full_path = req_dir.join(CODE_REVIEW_FILE);
    let inc_path = req_dir.join(CODE_REVIEW_INCREMENTAL_FILE);
    let (material, material_label) = match (
        read_json_if_exists(&full_path).await,
        read_json_if_exists(&inc_path).await,
    ) {
        (Some(full), Some(inc)) => {
            let full_t = file_modified_ms(&full_path).await.unwrap_or(0);
            let inc_t = file_modified_ms(&inc_path).await.unwrap_or(0);
            if inc_t > full_t {
                (inc, CODE_REVIEW_INCREMENTAL_FILE)
            } else {
                (full, CODE_REVIEW_FILE)
            }
        }
        (Some(full), None) => (full, CODE_REVIEW_FILE),
        (None, Some(inc)) => (inc, CODE_REVIEW_INCREMENTAL_FILE),
        // 无材料快照（纯手工审查场景）：只要求备注存在，不做指纹核对。
        (None, None) => return ReviewAnnotationsState::Valid,
    };
    let Some(repos) = material.get("repos").and_then(Value::as_array) else {
        return ReviewAnnotationsState::Valid;
    };
    let Some(reviewed) = annotations.get("reviewedCommit").and_then(Value::as_object) else {
        return ReviewAnnotationsState::Stale(format!(
            "code-annotations.json 缺少 reviewedCommit 提交指纹（repo → {material_label} 里各仓的 targetCommit），差异页与门禁无法确认备注锚定的 diff"
        ));
    };
    let mut stale_repos = Vec::<String>::new();
    for repo in repos {
        let repo_name = value_string(repo, "repoName").unwrap_or_default();
        if repo_name.is_empty() {
            continue;
        }
        let commit = value_string(repo, "targetCommit")
            .filter(|c| !c.is_empty())
            .or_else(|| {
                value_string(repo, "coverageToCommit").filter(|c| !c.is_empty())
            });
        let Some(commit) = commit else {
            continue;
        };
        let fingerprint = reviewed
            .get(repo_name.as_str())
            .and_then(Value::as_str)
            .unwrap_or_default();
        if fingerprint != commit {
            stale_repos.push(repo_name);
        }
    }
    if stale_repos.is_empty() {
        ReviewAnnotationsState::Valid
    } else {
        ReviewAnnotationsState::Stale(format!(
            "仓库 {} 的备注提交指纹与 {material_label} 的 targetCommit 不一致（备注锚定旧 diff）",
            stale_repos.join("、")
        ))
    }
}

/// 门禁详情用：代码问题备注概要。
fn review_annotations_summary(state: &ReviewAnnotationsState) -> Value {
    match state {
        ReviewAnnotationsState::Missing => json!({ "present": false, "stale": false }),
        ReviewAnnotationsState::Stale(reason) => {
            json!({ "present": true, "stale": true, "reason": reason })
        }
        ReviewAnnotationsState::Valid => json!({ "present": true, "stale": false }),
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
                "加载 agent-panel-code-review skill 执行代码审查：POST /api/requirement/review-materials 备料（发布就绪前默认全量、发布就绪起默认增量，可用 mode 覆盖）".into(),
                "按 skill 流程写 code-review-ai.md（顶部含 `Review Gate: PASS/BLOCKED` 与 `Source: agent-panel-code-review skill` 标记）并 PUT review-checklist + annotations".into(),
                "用户明确要求手工审查/豁免时，在 review.md 写 `Review Gate: WAIVED` + 豁免原因".into(),
            ],
            warnings: Vec::new(),
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
                warnings: Vec::new(),
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
                    "修复严重问题后，主 agent 结合当前会话的需求上下文亲自全面复审（不只派子 agent），再重新写结论".into(),
                    "若业务确认可带风险提测，在 review.md 明确 `Review Gate: WAIVED` 和豁免原因"
                        .into(),
                ],
                warnings: Vec::new(),
                stale_repos: Vec::new(),
                incremental_review: None,
            });
        }
    }
    for (source, path, raw) in &docs {
        if review_gate_passed(raw) {
            // 门禁强制：PASS 结论必须经 agent-panel-code-review skill 产出（结论文档带 skill 标记）。
            // 未经 skill 的浅层 PASS 不再放行；用户手工审查/外部审查走 WAIVED 豁免路径。
            if !docs
                .iter()
                .any(|(_, _, doc_raw)| review_gate_skill_marked(doc_raw))
            {
                return Ok(skill_required_review_gate_decision(
                    source.clone(),
                    review_path.clone(),
                    ai_review_path.clone(),
                ));
            }
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
                        warnings: Vec::new(),
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
                        warnings: Vec::new(),
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
                            warnings: Vec::new(),
                            stale_repos: Vec::new(),
                            incremental_review: None,
                        });
                    }
                    // 全部 pass/na：清单齐备，继续代码问题备注与库存专项检查。
                }
            }
            // 代码问题备注硬校验：PASS 必须携带差异页可见的代码问题备注（code-annotations.json），
            // 且提交指纹锚定当前审查材料；缺失或过期都会拦截，保证看 diff 时能看到审查发现。
            match review_annotations_gate_state(&dir).await {
                ReviewAnnotationsState::Missing => {
                    return Ok(annotations_required_review_gate_decision(
                        source.clone(),
                        review_path.clone(),
                        ai_review_path.clone(),
                    ));
                }
                ReviewAnnotationsState::Stale(reason) => {
                    return Ok(annotations_stale_review_gate_decision(
                        source.clone(),
                        review_path.clone(),
                        ai_review_path.clone(),
                        reason,
                    ));
                }
                ReviewAnnotationsState::Valid => {}
            }
            // 自测清单交叉评估硬校验：test.md 已有自测清单时，review 必须包含
            // 「自测清单交叉评估」小节，交叉核对边界/并发流量风险分析的覆盖度与测试结果矛盾点。
            if test_md_has_selftest_checklist(&dir).await
                && !docs.iter().any(|(_, _, doc_raw)| {
                    review_gate_section(doc_raw, &["自测清单交叉评估"])
                        .map(|section| !review_section_is_empty(&section))
                        .unwrap_or(false)
                })
            {
                return Ok(crosscheck_missing_review_gate_decision(
                    source.clone(),
                    review_path.clone(),
                    ai_review_path.clone(),
                ));
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
                    warnings: Vec::new(),
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
                // P1 严重问题：放行但警示，提醒用户注意（P0 已在 blocked 分支拦截）。
                warnings: p1_warnings(&docs),
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
            "加载 agent-panel-code-review skill 执行审查并把结论写入 code-review-ai.md / review.md（含 `Review Gate: PASS/BLOCKED` 与 skill 标记）".into(),
            "用户明确要求手工审查/豁免时，在 review.md 写 `Review Gate: WAIVED` + 豁免原因".into(),
        ],
        warnings: Vec::new(),
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
            "主 agent 结合会话上下文亲自全面复审：加载 agent-panel-code-review skill 并重新备料（POST /api/requirement/review-materials，按需求状态默认全量/增量）".into(),
            "把复审结论追加/更新到 code-review-ai.md 或 review.md，并重新写明 `Review Gate: PASS` / `BLOCKED` / `WAIVED`".into(),
            "若提示非线性历史、rebase 或 force-push，再回退到全量 code-review.json 审查".into(),
        ],
        warnings: Vec::new(),
        stale_repos: stale
            .iter()
            .cloned()
            .map(review_snapshot_drift_json)
            .collect(),
        incremental_review: None,
    }
}

/// 收集结论文档里的 P1 严重问题警示：P1 非空 → 放行但提醒用户注意。
fn p1_warnings(docs: &[(String, PathBuf, String)]) -> Vec<String> {
    let mut warnings = Vec::new();
    for (source, _, raw) in docs {
        if let Some(section) = review_gate_section(raw, &["P1"]) {
            if review_section_is_empty(&section) {
                continue;
            }
            let items = section
                .lines()
                .filter(|l| {
                    let t = l.trim();
                    t.starts_with('-') || t.starts_with('*') || t.starts_with('|')
                })
                .count();
            let count = if items > 0 {
                format!("{items} 条")
            } else {
                "若干".to_string()
            };
            warnings.push(format!(
                "P1 严重问题 {count} 待修复（结论来源：{source}）：已放行提测，但请尽快安排修复，避免带病上线"
            ));
        }
    }
    warnings
}

/// test.md 是否已有「自测清单」小节（有才要求 review 做自测清单交叉评估）。
async fn test_md_has_selftest_checklist(dir: &Path) -> bool {
    let Ok(body) = tokio::fs::read_to_string(dir.join("test.md")).await else {
        return false;
    };
    body.lines().any(|line| {
        parse_markdown_heading(line)
            .map(|(_, text)| text.contains("自测清单"))
            .unwrap_or(false)
    })
}

/// test.md 已有自测清单但 review 缺「自测清单交叉评估」：审查必须交叉核对边界/并发流量风险与测试结果。
pub(crate) fn crosscheck_missing_review_gate_decision(
    source: String,
    review_path: PathBuf,
    ai_review_path: PathBuf,
) -> ReviewGateDecision {
    ReviewGateDecision {
        status: "crosscheck-missing".into(),
        label: "自测交叉评估缺失".into(),
        allows_testing: false,
        reason: "test.md 已有自测清单，但 review 文档缺少非空的「自测清单交叉评估」小节；门禁要求审查交叉核对边界场景测试与高并发/大流量场景测试的风险分析覆盖度、测试结果与审查发现的矛盾点".into(),
        source: Some(source),
        review_path,
        ai_review_path,
        actions: vec![
            "读 test.md「自测清单」三分类（重点边界场景测试、高并发/大流量场景测试的风险场景分析与测试结果），在 review 文档补「## 自测清单交叉评估」：① diff 风险是否被自测风险分析覆盖；② 自测失败/无法测试项与审查发现的互相印证；③ 自测声称通过但代码无保护的并发/边界风险（发现即 P1）".into(),
            "重新写明 `Review Gate: PASS` / `BLOCKED` 后重查门禁".into(),
        ],
        warnings: Vec::new(),
        stale_repos: Vec::new(),
        incremental_review: None,
    }
}

/// PASS 但缺代码问题备注：审查发现必须落到差异页备注，让人看 diff 时能看到。
pub(crate) fn annotations_required_review_gate_decision(
    source: String,
    review_path: PathBuf,
    ai_review_path: PathBuf,
) -> ReviewGateDecision {
    ReviewGateDecision {
        status: "annotations-required".into(),
        label: "代码问题备注缺失".into(),
        allows_testing: false,
        reason: "review 写了通过结论，但 code-annotations.json 缺失或 files 为空；门禁要求审查产出必须包含代码问题备注（每条 finding 对应一条 hunk 备注），让人在差异页看代码时能直接看到问题说明".into(),
        source: Some(source),
        review_path,
        ai_review_path,
        actions: vec![
            "按 agent-panel-code-review skill 产出代码问题备注：为审查材料的关键文件逐个写 summary + 问题 hunk notes（anchor.hunkHeader 从 patch 的 @@ 行复制），每条 finding 对应一条备注".to_string(),
            "PUT /api/requirement/annotations 全量覆盖落盘，顶层必须带 reviewedCommit 指纹（repo → 审查材料里各仓的 targetCommit），供差异页与门禁判定备注是否锚定当前 diff".to_string(),
            "落盘后重新 GET /api/requirement/review-gate 确认通过".to_string(),
        ],
        warnings: Vec::new(),
        stale_repos: Vec::new(),
        incremental_review: None,
    }
}

/// 代码问题备注存在但锚定旧 diff：需要按最新材料复核并重新落盘。
pub(crate) fn annotations_stale_review_gate_decision(
    source: String,
    review_path: PathBuf,
    ai_review_path: PathBuf,
    reason: String,
) -> ReviewGateDecision {
    ReviewGateDecision {
        status: "annotations-stale".into(),
        label: "代码问题备注过期".into(),
        allows_testing: false,
        reason: format!(
            "{reason}；备注锚定的是旧 diff，需要按最新审查材料复核新增差异并重新落盘备注，否则差异页备注会误导阅读"
        ),
        source: Some(source),
        review_path,
        ai_review_path,
        actions: vec![
            "加载 agent-panel-code-review skill 重新备料（POST /api/requirement/review-materials，按需求状态默认增量/全量），复核新增 diff".to_string(),
            "更新 code-annotations.json：新文件补充备注、失效备注修正，刷新 reviewedCommit 指纹后 PUT 落盘".to_string(),
            "同步更新 code-review-ai.md / review.md 的审查结论并重新写明 `Review Gate: PASS` / `BLOCKED` / `WAIVED`".to_string(),
        ],
        warnings: Vec::new(),
        stale_repos: Vec::new(),
        incremental_review: None,
    }
}

/// PASS 结论存在但未经 agent-panel-code-review skill 产出：门禁不放行，要求走 skill 流程。
pub(crate) fn skill_required_review_gate_decision(
    source: String,
    review_path: PathBuf,
    ai_review_path: PathBuf,
) -> ReviewGateDecision {
    ReviewGateDecision {
        status: "skill-required".into(),
        label: "需经审查 skill".into(),
        allows_testing: false,
        reason: "review 文档写了通过结论，但没有 agent-panel-code-review skill 的产出标记；门禁强制代码审查按 skill 流程执行（备料 → 四轮深度审查 → 结论落盘），避免浅层 PASS 漏掉严重问题".into(),
        source: Some(source),
        review_path,
        ai_review_path,
        actions: vec![
            "加载 agent-panel-code-review skill 并按流程执行：POST /api/requirement/review-materials 备料 → 按 references/review-methodology.md 四轮审查 → 写 code-review-ai.md（顶部含 `Review Gate: PASS/BLOCKED` 与 `Source: agent-panel-code-review skill` 标记）→ PUT review-checklist + annotations".into(),
            "用户明确要求手工审查/豁免时，在 review.md 顶行写 `Review Gate: WAIVED` + 豁免原因".into(),
        ],
        warnings: Vec::new(),
        stale_repos: Vec::new(),
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
            "主 agent 结合会话上下文亲自全面复审：加载 agent-panel-code-review skill 重新备料并复审（优先增量，不重审完整 code-review.json）".into(),
            "更新 code-review-ai.md / review.md 的审查概览，注明增量覆盖范围和结论".into(),
            "确认所有新增风险标签（尤其库存）已覆盖后，重新写明 `Review Gate: PASS` / `BLOCKED` / `WAIVED`".into(),
        ],
        warnings: Vec::new(),
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

/// 审查产出必须来自 agent-panel-code-review skill：结论文档需带 skill 标记
/// （skill 会写入 `Source: agent-panel-code-review skill`，大小写不敏感匹配）。
/// 门禁据此强制审查流程，防止手写浅层 PASS 绕过审查。
pub(crate) fn review_gate_skill_marked(raw: &str) -> bool {
    raw.to_lowercase().contains("agent-panel-code-review")
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
    if raw.contains("P0") || raw.contains("P1") {
        // 新格式（P0-P3 分级）：只有 P0 阻断级问题非空才拦截；
        // P1 严重级放行但门禁警示（见 passed 分支的 warnings）。
        return review_gate_section(raw, &["P0"])
            .map(|section| !review_section_is_empty(&section))
            .unwrap_or(false);
    }
    // 旧格式启发式：严重问题/阻塞项小节非空或 ❌ + 阻塞表述 → 拦截。
    if raw.contains('❌')
        && (raw.contains("阻塞") || raw.contains("严重问题"))
    {
        return true;
    }
    review_gate_section(raw, &["严重问题", "Blocking Items", "阻塞项"])
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
