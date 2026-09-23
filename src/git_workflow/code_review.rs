use super::*;

pub(crate) async fn run_code_review_scan(
    req_dir: &Path,
    req_id: &str,
    scope: &BranchScope,
) -> Result<Value> {
    let previous_review = read_json_if_exists(&req_dir.join(CODE_REVIEW_FILE)).await;
    let previous_drifts = review_snapshot_drifts(req_dir).await;
    let mut repos = Vec::new();
    for repo in &scope.repos {
        let branches = if repo.branches.is_empty() {
            vec![String::new()]
        } else {
            repo.branches.clone()
        };
        for branch in branches {
            repos.push(scan_repo_branch(repo, &branch).await);
        }
    }
    let mut risk_tags = Vec::<String>::new();
    let mut inventory_risk = false;
    for repo in &repos {
        if let Some(tags) = repo.get("riskTags").and_then(Value::as_array) {
            for tag in tags {
                if let Some(s) = tag.as_str() {
                    if !risk_tags.contains(&s.to_string()) {
                        risk_tags.push(s.to_string());
                    }
                }
            }
        }
        if repo
            .get("inventoryRisk")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            inventory_risk = true;
        }
    }
    let previous_snapshot = previous_review
        .as_ref()
        .filter(|_| !previous_drifts.is_empty())
        .map(|value| {
            json!({
                "updatedAt": value.get("updatedAt").cloned().unwrap_or(Value::Null),
                "staleRepos": previous_drifts.iter().cloned().map(review_snapshot_drift_json).collect::<Vec<_>>(),
                "repos": value.get("repos").cloned().unwrap_or_else(|| json!([])),
                "note": "preserved because a full diff refresh replaced a stale reviewed snapshot; prefer incremental review when possible",
            })
        });
    let review = json!({
        "version": 1,
        "reqId": req_id,
        "updatedAt": now_ms(),
        "baseRef": "origin/master",
        "frontendBaseRef": "origin/production",
        "backendBaseRef": "origin/master",
        "sourceFallback": scope.fallback,
        "riskTags": risk_tags,
        "inventoryRisk": inventory_risk,
        "previousReviewedSnapshot": previous_snapshot,
        "repos": repos,
    });
    // 同步导出多行 patch 伴随文件；JSON 里的 diff 字段是单行转义串，reviewer 读 JSON 会被单行截断。
    let patch_written = export_diff_patch_file(req_dir, CODE_REVIEW_PATCH_FILE, &review).await?;
    let mut review = review;
    if patch_written {
        if let Some(obj) = review.as_object_mut() {
            obj.insert("diffPatchFile".to_string(), json!(CODE_REVIEW_PATCH_FILE));
        }
    }
    atomic_write_json(&req_dir.join(CODE_REVIEW_FILE), &review).await?;
    Ok(review)
}

pub(crate) async fn run_code_review_incremental_scan(
    req_dir: &Path,
    req_id: &str,
) -> Result<Value> {
    let drifts = review_snapshot_drifts_for_incremental(req_dir).await;
    if drifts.is_empty() {
        return Err(anyhow!(
            "当前 code-review.json 未发现 reviewed target commit 与当前 HEAD 的差异，无需生成增量审查包"
        ));
    }
    let mut repos = Vec::new();
    for drift in &drifts {
        repos.push(scan_incremental_review_drift(drift).await);
    }
    let mut risk_tags = Vec::<String>::new();
    let mut inventory_risk = false;
    for repo in &repos {
        if let Some(tags) = repo.get("riskTags").and_then(Value::as_array) {
            for tag in tags {
                if let Some(s) = tag.as_str() {
                    if !risk_tags.contains(&s.to_string()) {
                        risk_tags.push(s.to_string());
                    }
                }
            }
        }
        if repo
            .get("inventoryRisk")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            inventory_risk = true;
        }
    }
    let review = json!({
        "version": 1,
        "reqId": req_id,
        "updatedAt": now_ms(),
        "mode": "incremental",
        "sourceSnapshot": CODE_REVIEW_FILE,
        "baseDescription": "reviewed targetCommit from the last full code-review snapshot",
        "targetDescription": "current requirement branch HEAD",
        "riskTags": risk_tags,
        "inventoryRisk": inventory_risk,
        "repos": repos,
    });
    let patch_written =
        export_diff_patch_file(req_dir, CODE_REVIEW_INCREMENTAL_PATCH_FILE, &review).await?;
    let mut review = review;
    if patch_written {
        if let Some(obj) = review.as_object_mut() {
            obj.insert(
                "diffPatchFile".to_string(),
                json!(CODE_REVIEW_INCREMENTAL_PATCH_FILE),
            );
        }
    }
    atomic_write_json(&req_dir.join(CODE_REVIEW_INCREMENTAL_FILE), &review).await?;
    Ok(review)
}

/// 需求状态是否默认增量审查：发布就绪及之后（发布就绪/经验总结/已完成）= true；
/// 发布就绪之前（含线上问题等非需求流状态）= false，默认全量审查。
/// 对应策略：发版就绪前全量审查，发版就绪起只增量审查新增提交。
pub(crate) fn status_prefers_incremental(status: &str) -> bool {
    let release_ready_rank = REQ_FLOW_STATUSES
        .iter()
        .position(|s| *s == "发布就绪")
        .unwrap_or(4);
    requirement_flow_status_rank(status)
        .map(|rank| rank >= release_ready_rank)
        .unwrap_or(false)
}

/// 一键准备代码审查材料（决策树编排）：
/// 模式优先级：显式 mode（full/incremental）> 需求状态默认（发布就绪前全量、发布就绪起增量）。
/// 1. code-review.json 不存在 -> 全量首扫（mode=full-initial；没有已审基线，增量无从谈起）
/// 2. 存在且无漂移（全量快照已覆盖当前 HEAD）：
///    - 增量包比 review 结论新（上次增量还没审完）-> 复用现有增量包（mode=incremental-pending）
///    - 否则 -> 全量快照仍有效（mode=full-ready）
/// 3. 存在且有漂移：
///    - 偏好增量（发布就绪起默认或显式 incremental）且全部线性历史 -> 生成增量包（mode=incremental）
///    - 偏好全量（发布就绪前默认或显式 full）-> 全量重扫（mode=full-regenerate）
///    - 任一非线性历史（force-push/rebase）-> 全量重扫（mode=full-regenerate，增量 diff 不可靠）
/// 返回材料路径 + 模式 + handoff 提示，调用方把 materialPath 直接写进 reviewer handoff。
pub(crate) async fn prepare_review_materials(
    req_dir: &Path,
    req_id: &str,
    scope: &BranchScope,
    mode: Option<&str>,
    status: &str,
) -> Result<Value> {
    let explicit_mode = mode
        .map(str::trim)
        .filter(|m| !m.is_empty() && !m.eq_ignore_ascii_case("auto"))
        .map(|m| m.to_ascii_lowercase());
    if let Some(m) = &explicit_mode {
        if m != "full" && m != "incremental" {
            return Err(anyhow!(
                "mode 只支持 full / incremental / auto（缺省），收到：{m}"
            ));
        }
    }
    let prefer_incremental = explicit_mode
        .as_deref()
        .map(|m| m == "incremental")
        .unwrap_or_else(|| status_prefers_incremental(status));
    let existing_full = read_json_if_exists(&req_dir.join(CODE_REVIEW_FILE)).await;
    let mut warnings = Vec::<String>::new();
    let (mode, reason, material_kind, review_doc) = match existing_full {
        None => {
            if explicit_mode.as_deref() == Some("incremental") {
                return Err(anyhow!(
                    "显式要求增量审查，但需求目录还没有 code-review.json（没有已审查基线），无法增量；请先执行全量审查（mode=full 或省略 mode）"
                ));
            }
            let review = run_code_review_scan(req_dir, req_id, scope).await?;
            (
                "full-initial",
                "需求目录还没有 code-review.json，已自动生成全量审查快照".to_string(),
                "full",
                review,
            )
        }
        Some(existing) => {
            let drifts = review_snapshot_drifts(req_dir).await;
            if drifts.is_empty() {
                // 无漂移：现有全量快照已覆盖当前 HEAD，本身就是全量材料。
                if explicit_mode.as_deref() != Some("full")
                    && review_artifact_newer_than_review_docs(req_dir, CODE_REVIEW_INCREMENTAL_FILE)
                        .await
                {
                    if let Some(inc) =
                        read_json_if_exists(&req_dir.join(CODE_REVIEW_INCREMENTAL_FILE)).await
                    {
                        (
                            "incremental-pending",
                            "上一次生成的增量审查包还没有产生更新的审查结论，直接复用，不重复生成"
                                .to_string(),
                            "incremental",
                            inc,
                        )
                    } else {
                        let review = run_code_review_scan(req_dir, req_id, scope).await?;
                        (
                            "full-initial",
                            "需求目录还没有 code-review.json，已自动生成全量审查快照".to_string(),
                            "full",
                            review,
                        )
                    }
                } else {
                    if review_artifact_requires_fresh_review(req_dir, &req_dir.join("review.md"))
                        .await
                    {
                        warnings.push(
                            "全量快照刚刷新且比 review.md 结论新，需基于最新快照重新给出 Review Gate 结论".to_string(),
                        );
                    }
                    (
                        "full-ready",
                        "审查快照覆盖的 target commit 与当前需求分支 HEAD 一致，无漂移，全量快照材料仍然有效".to_string(),
                        "full",
                        existing,
                    )
                }
            } else if prefer_incremental {
                let mut all_linear = true;
                for drift in &drifts {
                    if !drift_is_linear(drift).await {
                        all_linear = false;
                        break;
                    }
                }
                if all_linear {
                    let inc = run_code_review_incremental_scan(req_dir, req_id).await?;
                    let mode_reason = if explicit_mode.as_deref() == Some("incremental") {
                        "显式要求增量审查".to_string()
                    } else {
                        format!("需求状态「{status}」已到发布就绪及之后，默认增量审查")
                    };
                    (
                        "incremental",
                        format!(
                            "检测到 {} 个仓库的需求分支 HEAD 已推进（{}）；{}，已生成增量审查包，只覆盖 reviewed commit → HEAD 的新增 diff",
                            drifts.len(),
                            drifts.iter().map(|d| d.repo_name.as_str()).collect::<Vec<_>>().join("、"),
                            mode_reason
                        ),
                        "incremental",
                        inc,
                    )
                } else if explicit_mode.as_deref() == Some("incremental") {
                    return Err(anyhow!(
                        "显式要求增量审查，但检测到非线性历史（reviewed commit 不是当前 HEAD 祖先，可能 rebase/force-push），增量 diff 不可靠；请改用 mode=full 全量重扫"
                    ));
                } else {
                    warnings.push(
                        "检测到非线性历史（reviewed commit 不是当前 HEAD 的祖先，可能 rebase/force-push），增量 diff 不可靠，已回退全量重扫".to_string(),
                    );
                    let review = run_code_review_scan(req_dir, req_id, scope).await?;
                    (
                        "full-regenerate",
                        "存在非线性历史漂移，已重新生成全量审查快照".to_string(),
                        "full",
                        review,
                    )
                }
            } else {
                let mode_reason = if explicit_mode.as_deref() == Some("full") {
                    "显式要求全量审查".to_string()
                } else {
                    format!("需求状态「{status}」在发布就绪之前，默认全量审查")
                };
                let review = run_code_review_scan(req_dir, req_id, scope).await?;
                (
                    "full-regenerate",
                    format!(
                        "检测到 {} 个仓库的需求分支 HEAD 已推进（{}）；{}，已重新生成全量审查快照",
                        drifts.len(),
                        drifts.iter().map(|d| d.repo_name.as_str()).collect::<Vec<_>>().join("、"),
                        mode_reason
                    ),
                    "full",
                    review,
                )
            }
        }
    };

    let is_incremental = material_kind == "incremental";
    let material_file = if is_incremental {
        CODE_REVIEW_INCREMENTAL_FILE
    } else {
        CODE_REVIEW_FILE
    };
    let material_path = req_dir.join(material_file);
    let patch_file = if is_incremental {
        CODE_REVIEW_INCREMENTAL_PATCH_FILE
    } else {
        CODE_REVIEW_PATCH_FILE
    };
    let patch_path = req_dir.join(patch_file);
    // 存量材料没有 patch 伴随文件时即时补导出（新快照在生成时已写过，此处幂等自愈）。
    if !tokio::fs::try_exists(&patch_path).await.unwrap_or(false) {
        let _ = export_diff_patch_file(req_dir, patch_file, &review_doc).await?;
    }
    let patch_available = tokio::fs::try_exists(&patch_path).await.unwrap_or(false);
    let repos_summary = review_doc
        .get("repos")
        .and_then(Value::as_array)
        .map(|repos| {
            repos
                .iter()
                .map(|r| {
                    let from = if is_incremental {
                        value_string(r, "coverageFromCommit")
                            .or_else(|| value_string(r, "baseCommit"))
                            .unwrap_or_default()
                    } else {
                        value_string(r, "baseCommit").unwrap_or_default()
                    };
                    let to = if is_incremental {
                        value_string(r, "coverageToCommit")
                            .or_else(|| value_string(r, "targetCommit"))
                            .unwrap_or_default()
                    } else {
                        value_string(r, "targetCommit").unwrap_or_default()
                    };
                    json!({
                        "repoName": value_string(r, "repoName").unwrap_or_default(),
                        "branch": value_string(r, "branch").unwrap_or_default(),
                        "fromCommit": from,
                        "toCommit": to,
                        "additions": r.get("additions").cloned().unwrap_or(json!(0)),
                        "deletions": r.get("deletions").cloned().unwrap_or(json!(0)),
                        "riskTags": r.get("riskTags").cloned().unwrap_or_else(|| json!([])),
                        "diffTruncated": r.get("diffTruncated").and_then(Value::as_bool).unwrap_or(false),
                        "linearHistory": r.get("linearHistory").and_then(Value::as_bool),
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let risk_tags: Vec<String> = review_doc
        .get("riskTags")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    let inventory_risk = review_doc
        .get("inventoryRisk")
        .and_then(Value::as_bool)
        .unwrap_or(false);

    let mut handoff_hints = Vec::new();
    handoff_hints.push(
        "审查必须按 agent-panel-code-review skill 执行：加载该 skill 后按其流程审查（材料已备好：四轮审查法见 skill 的 references/review-methodology.md）；结论文件 code-review-ai.md 顶部必须带 `Source: agent-panel-code-review skill` 标记，否则门禁判 skill-required 不放行".to_string(),
    );
    if patch_available {
        handoff_hints.push(format!(
            "diff 全文用 read 读多行 patch 文件 {}（按真实换行落盘，超过 2000 行用 offset 续读）；不要直接 read {} 的 diff 字段——JSON 里 diff 是单行转义串，超出 read 单行限制会被截断",
            patch_path.display(),
            material_path.display()
        ));
        handoff_hints.push(format!(
            "files/commits/riskTags 等元数据在 {}（read 时跳过其单行 diff 字段即可）",
            material_path.display()
        ));
    } else {
        handoff_hints.push(format!(
            "reviewer 直接 read {}：diff 字段已含 --unified=80 全文及 files/commits/riskTags 元数据，无需再执行 git 命令",
            material_path.display()
        ));
    }
    if is_incremental {
        handoff_hints.push(
            "只审查覆盖范围内的新增 diff（fromCommit → toCommit）；审完在 review.md 注明覆盖范围并重写 Review Gate 结论，门禁按结论文件 mtime + commit 指纹判定覆盖".to_string(),
        );
    } else {
        handoff_hints.push(
            "全量审查不只看 diff：每个改动文件的完整实现、直接调用方/被调方、相关 Mapper XML/枚举/配置都在扩大阅读范围内（必要上下文代码都要读），扩大阅读清单写进审查概览；审完在 review.md 重写 Review Gate 结论（PASS / BLOCKED / WAIVED）".to_string(),
        );
    }
    handoff_hints.push(
        "问题分级 P0 阻断/P1 严重/P2 一般/P3 优化：逐条核查、先验证再上报、不输出冗余告警；P0/P1 非空必须 `Review Gate: BLOCKED`，每条 finding 标注 文件:行 + 等级 + 描述 + 修复建议，并同步产出 annotations hunk note".to_string(),
    );
    handoff_hints.push(
        "审查必须与 test.md 自测清单三分类交叉核对（重点边界场景测试、高并发/大流量场景测试的风险场景分析与测试结果），结论写进 code-review-ai.md 的「## 自测清单交叉评估」小节；diff 风险未被自测覆盖、或自测声称通过但代码无真实保护（无幂等/锁/判空）都要在评估里指出（后者 P1）".to_string(),
    );
    if inventory_risk {
        handoff_hints.push(
            "本次改动命中库存高危风险：审查必须包含库存账本专项评估（单据活跃/死亡、DB 库存、redis 可用量、重复释放、遗漏占用、幂等、验证证据），否则 PASS 不通过门禁".to_string(),
        );
    }
    handoff_hints.push(
        "代码问题备注（code-annotations.json）是门禁硬要求：为本次 diff 的关键文件（建议 ≤10 个，优先 riskTags 命中/核心链路文件）逐个产出 `repo/path` + `summary`（改动目的与设计思路）+ `variables`（关键变量/字段：名 | 含义 | 为何重要）+ `flow`（数据/状态流转，可用 mermaid flowchart）+ `notes`；每条 finding（严重/建议）都必须对应一条 hunk note（anchor.hunkHeader 从 patch 的 @@ 行复制），让人在差异页看代码时能直接看到问题说明".to_string(),
    );
    handoff_hints.push(
        "annotations 顶层必须带 reviewedCommit 指纹（repo → 本材料各仓的 targetCommit，增量包取 coverageToCommit）：门禁与差异页用它与当前快照比对判定备注是否锚定旧 diff；缺失或不一致会被门禁判 annotations-required/annotations-stale 拦截".to_string(),
    );
    handoff_hints.push(
        "reviewer 无写权限：annotations 内容先随审查结论一起输出，由主 agent 复核后调 `PUT /api/requirement/annotations` 落盘（全量覆盖旧版）；写入时机与审查快照同批，说明锚定当前审查 diff，避免说明栏与代码漂移".to_string(),
    );
    handoff_hints.push(
        "审查必须产出结构化清单 review-checklist.json：把本次审查要点逐项列成清单（每仓库关键风险、幂等/并发、库存、配置、部署顺序等），reviewer 随结论输出、主 agent 复核后调 `PUT /api/requirement/review-checklist` 落盘，每项 conclusion ∈ pass/fail/na；门禁要求全部项有结论且无 fail 项 + `Review Gate: PASS` 才放行，缺清单或缺结论会被拦截".to_string(),
    );
    for repo in &repos_summary {
        if repo
            .get("diffTruncated")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            handoff_hints.push(format!(
                "仓库 {} 的 diff 超过输出上限被截断，审查结论需注明覆盖范围，必要时分仓重试",
                repo.get("repoName").and_then(Value::as_str).unwrap_or("?")
            ));
        }
    }
    handoff_hints.extend(warnings.iter().cloned());

    Ok(json!({
        "mode": mode,
        "reason": reason,
        "materialKind": material_kind,
        "materialFile": material_file,
        "materialPath": material_path.to_string_lossy(),
        "diffPatchFile": if patch_available { json!(patch_file) } else { Value::Null },
        "riskTags": risk_tags,
        "inventoryRisk": inventory_risk,
        "repos": repos_summary,
        "warnings": warnings,
        "handoffHints": handoff_hints,
        "checkedAt": now_ms(),
    }))
}

/// 判定单条漂移是否线性历史：reviewed target commit 是否为当前 HEAD 的祖先。
/// 缺 projectPath 或 git 调用失败时按非线性处理（保守回退全量审查）。
async fn drift_is_linear(drift: &ReviewSnapshotDrift) -> bool {
    let Some(project_path) = drift.project_path.as_ref() else {
        return false;
    };
    if drift.reviewed_target_commit.is_empty() || drift.current_target_commit.is_empty() {
        return false;
    }
    let ancestor = git(
        project_path,
        &[
            "merge-base",
            "--is-ancestor",
            &drift.reviewed_target_commit,
            &drift.current_target_commit,
        ],
        30_000,
        COMMAND_OUTPUT_LIMIT,
    )
    .await;
    ancestor.ok
}

pub(crate) async fn scan_incremental_review_drift(drift: &ReviewSnapshotDrift) -> Value {
    let mut warnings = Vec::<String>::new();
    let Some(project_path) = drift.project_path.as_ref() else {
        return json!({
            "repoName": drift.repo_name,
            "branch": drift.branch,
            "mode": "incremental",
            "baseCommit": drift.reviewed_target_commit,
            "targetCommit": drift.current_target_commit,
            "files": Vec::<CodeReviewFileStat>::new(),
            "additions": 0,
            "deletions": 0,
            "riskTags": Vec::<String>::new(),
            "inventoryRisk": false,
            "linearHistory": false,
            "diff": "",
            "diffTruncated": false,
            "warnings": ["缺少 projectPath，无法生成增量 diff"],
            "error": "missing projectPath",
        });
    };
    let ancestor = git(
        project_path,
        &[
            "merge-base",
            "--is-ancestor",
            &drift.reviewed_target_commit,
            &drift.current_target_commit,
        ],
        30_000,
        COMMAND_OUTPUT_LIMIT,
    )
    .await;
    let linear_history = ancestor.ok;
    if !linear_history {
        warnings.push("上次审查 commit 不是当前 HEAD 的祖先，分支可能 rebase/force-push；为安全起见建议重新做全量审查".into());
    }
    let commit_range = format!(
        "{}..{}",
        drift.reviewed_target_commit, drift.current_target_commit
    );
    let commits = git(
        project_path,
        &[
            "log",
            "--oneline",
            "--decorate=short",
            "--max-count=80",
            &commit_range,
        ],
        30_000,
        COMMAND_OUTPUT_LIMIT,
    )
    .await;
    if !commits.ok {
        warnings.push(format!("增量提交列表读取失败：{}", short_err(&commits)));
    }
    let name_status = git(
        project_path,
        &[
            "diff",
            "--name-status",
            "--find-renames",
            &drift.reviewed_target_commit,
            &drift.current_target_commit,
            "--",
        ],
        30_000,
        COMMAND_OUTPUT_LIMIT,
    )
    .await;
    if !name_status.ok {
        warnings.push(format!("增量文件列表读取失败：{}", short_err(&name_status)));
    }
    let numstat = git(
        project_path,
        &[
            "diff",
            "--numstat",
            "--find-renames",
            &drift.reviewed_target_commit,
            &drift.current_target_commit,
            "--",
        ],
        30_000,
        COMMAND_OUTPUT_LIMIT,
    )
    .await;
    if !numstat.ok {
        warnings.push(format!("增量增删行统计读取失败：{}", short_err(&numstat)));
    }
    let diff = git(
        project_path,
        &[
            "diff",
            "--no-ext-diff",
            "--no-color",
            "--find-renames",
            "--unified=80",
            &drift.reviewed_target_commit,
            &drift.current_target_commit,
            "--",
        ],
        60_000,
        DIFF_OUTPUT_LIMIT,
    )
    .await;
    if !diff.ok {
        warnings.push(format!("增量 Diff 读取失败：{}", short_err(&diff)));
    }
    let files = merge_file_stats(&name_status.stdout, &numstat.stdout);
    let additions: i64 = files.iter().map(|f| f.additions).sum();
    let deletions: i64 = files.iter().map(|f| f.deletions).sum();
    let risk_tags = aggregate_risk_tags(&files);
    let inventory_risk = risk_tags.iter().any(|t| t == "库存");
    json!({
        "repoName": drift.repo_name,
        "projectPath": project_path.to_string_lossy(),
        "branch": drift.branch,
        "mode": "incremental",
        "reviewedTargetRef": drift.reviewed_target_ref,
        "currentTargetRef": drift.current_target_ref,
        "baseCommit": drift.reviewed_target_commit,
        "targetCommit": drift.current_target_commit,
        "coverageFromCommit": drift.reviewed_target_commit,
        "coverageToCommit": drift.current_target_commit,
        "linearHistory": linear_history,
        "commits": if commits.ok { commits.stdout.lines().filter(|l| !l.trim().is_empty()).map(str::to_string).collect::<Vec<_>>() } else { Vec::<String>::new() },
        "files": files,
        "additions": additions,
        "deletions": deletions,
        "riskTags": risk_tags,
        "inventoryRisk": inventory_risk,
        "diff": if diff.ok { diff.stdout.clone() } else { String::new() },
        "diffTruncated": diff.output_truncated,
        "warnings": warnings,
        "error": if diff.ok { Value::Null } else { Value::String(short_err(&diff)) },
    })
}

pub(crate) async fn run_master_diff_scan(
    req_id: &str,
    scope: &BranchScope,
    base_ref: &str,
    round: u32,
) -> Result<Value> {
    let mut repos = Vec::new();
    for repo in &scope.repos {
        let branches = if repo.branches.is_empty() {
            vec![String::new()]
        } else {
            repo.branches.clone()
        };
        let repo_base = if repo.role.as_deref() == Some("前端")
            || repo.path.as_deref().unwrap_or("").contains("/frontend/")
        {
            "origin/production"
        } else {
            base_ref
        };
        for branch in branches {
            repos.push(scan_repo_branch_with_base(repo, &branch, Some(repo_base)).await);
        }
    }
    Ok(json!({
        "version": 1,
        "reqId": req_id,
        "round": round,
        "updatedAt": now_ms(),
        "baseRef": base_ref,
        "frontendBaseRef": "origin/production",
        "backendBaseRef": "origin/master",
        "sourceFallback": scope.fallback,
        "repos": repos,
    }))
}

/// 把快照里各仓库的 diff 字段拼成多行 patch 伴随文件（read 友好）。
/// JSON 里的 diff 是单行转义串，reviewer 用 read 读 JSON 会被单行截断；
/// patch 文件按真实换行落盘，read 可分页读取。返回是否写出了非空文件；
/// 所有仓库 diff 均为空时清掉旧 patch 文件并返回 false，避免材料与内容不一致。
pub(crate) async fn export_diff_patch_file(
    req_dir: &Path,
    patch_file: &str,
    review: &Value,
) -> Result<bool> {
    let mut out = String::new();
    let repos = review.get("repos").and_then(Value::as_array);
    if let Some(repos) = repos {
        for r in repos {
            let diff = value_string(r, "diff").unwrap_or_default();
            if diff.trim().is_empty() {
                continue;
            }
            let repo_name = value_string(r, "repoName").unwrap_or_else(|| "?".to_string());
            let branch = value_string(r, "branch").unwrap_or_default();
            out.push_str(&format!(
                "# ==== repo: {repo_name} (branch: {branch}) ====\n"
            ));
            out.push_str(diff.trim_end());
            out.push_str("\n\n");
        }
    }
    if out.is_empty() {
        let _ = tokio::fs::remove_file(req_dir.join(patch_file)).await;
        return Ok(false);
    }
    atomic_write_text(&req_dir.join(patch_file), &out).await?;
    Ok(true)
}

/// Save a freshly generated master-diff snapshot onto the requirement's
/// snapshot stack (newest first, capped at MAX_DIFF_SNAPSHOTS) and return the
/// whole stack. The diff page renders any entry of this stack, so "undo a
/// refresh" is just switching back to the previous entry — no regeneration.
pub(crate) async fn save_diff_snapshot(
    req_dir: &Path,
    review: Value,
    round: u32,
) -> Result<Vec<Value>> {
    const MAX_DIFF_SNAPSHOTS_PER_ROUND: usize = 5;
    let mut snapshots: Vec<Value> = read_json_if_exists(&req_dir.join(CODE_DIFF_SNAPSHOTS_FILE))
        .await
        .and_then(|doc| doc.get("snapshots").cloned())
        .and_then(|s| serde_json::from_value::<Vec<Value>>(s).ok())
        .unwrap_or_default();
    let mut snapshot = review;
    if let Some(obj) = snapshot.as_object_mut() {
        obj.insert("savedAt".to_string(), json!(now_ms()));
        obj.insert("round".to_string(), json!(round));
        // Remove any previous snapshot of the same round with the identical
        // base+target commits: re-generating the same diff should not grow the
        // history stack.
        let sig = (
            round,
            obj.get("baseRef")
                .and_then(|v| v.as_str())
                .map(str::to_string),
            repo_commit_signature(&snapshot),
        );
        snapshots.retain(|s| {
            let other_sig = (
                s.get("round").and_then(Value::as_u64).unwrap_or(1) as u32,
                s.get("baseRef")
                    .and_then(|v| v.as_str())
                    .map(str::to_string),
                repo_commit_signature(s),
            );
            other_sig != sig
        });
    }
    snapshots.insert(0, snapshot);
    // 每轮次独立保留最近 N 版：修复轮次的快照不挤掉原始轮次可回退的历史。
    let mut per_round: HashMap<u32, usize> = HashMap::new();
    snapshots.retain(|s| {
        let r = s.get("round").and_then(Value::as_u64).unwrap_or(1) as u32;
        let count = per_round.entry(r).or_insert(0);
        *count += 1;
        *count <= MAX_DIFF_SNAPSHOTS_PER_ROUND
    });
    let doc = json!({ "version": 1, "updatedAt": now_ms(), "snapshots": snapshots });
    atomic_write_json(&req_dir.join(CODE_DIFF_SNAPSHOTS_FILE), &doc).await?;
    Ok(snapshots)
}

/// Fingerprint of a snapshot's per-repo target commits; snapshots with the
/// same base ref and same target commits show the same diff.
fn repo_commit_signature(snapshot: &Value) -> Vec<(String, String)> {
    snapshot
        .get("repos")
        .and_then(|v| v.as_array())
        .map(|repos| {
            repos
                .iter()
                .filter_map(|r| {
                    Some((
                        r.get("repoName")?.as_str()?.to_string(),
                        r.get("targetCommit")?.as_str()?.to_string(),
                    ))
                })
                .collect()
        })
        .unwrap_or_default()
}

pub(crate) fn resolve_code_review_project_path(
    project_path: Option<&str>,
    repo_name: &str,
) -> Option<PathBuf> {
    let raw = project_path?.trim();
    if raw.is_empty() {
        return None;
    }
    let expanded = if raw == "~" {
        home_dir().ok()?
    } else if let Some(rest) = raw.strip_prefix("~/") {
        home_dir().ok()?.join(rest)
    } else {
        PathBuf::from(raw)
    };
    let resolved = if expanded.is_absolute() {
        expanded
    } else {
        env::current_dir().ok()?.join(expanded)
    };
    if resolved.exists() {
        return Some(resolved);
    }
    let leaf = if repo_name.trim().is_empty() {
        resolved.file_name()?.to_string_lossy().to_string()
    } else {
        repo_name.trim().to_string()
    };
    let mut roots = Vec::new();
    if let Some(parent) = resolved.parent() {
        roots.push(parent.to_path_buf());
    }
    roots.push(crate::paths::wms_root());
    for root in roots {
        for area in ["backend", "frontend", "pda", "infra"] {
            let candidate = root.join(area).join(&leaf);
            if candidate.exists() {
                return Some(candidate);
            }
        }
    }
    Some(resolved)
}

pub(crate) fn classify_code_review_risk_tags(file: &CodeReviewFileStat) -> Vec<String> {
    let p = file.path.to_lowercase();
    let mut tags = Vec::new();
    if p.contains("/test/") || p.contains("src/test") {
        tags.push("测试".to_string());
    }
    if p.contains("controller") || p.contains("resource") || p.contains("/api/") {
        tags.push("API".to_string());
    }
    if p.contains("service") || p.contains("manager") {
        tags.push("Service".to_string());
    }
    if p.contains("mapper")
        || p.ends_with(".xml")
        || p.contains("dao")
        || p.ends_with(".sql")
        || p.ends_with("pom.xml")
    {
        tags.push("DB".to_string());
    }
    if p.contains("listener")
        || p.contains("consumer")
        || p.contains("kafka")
        || p.contains("rocket")
        || p.contains("rabbit")
        || p.contains("mq")
    {
        tags.push("MQ".to_string());
    }
    if p.contains("config")
        || p.ends_with(".yml")
        || p.ends_with(".yaml")
        || p.ends_with(".properties")
    {
        tags.push("配置".to_string());
    }
    // 库存高危风险：命中库存相关文件/表，门禁将强制要求库存账本专项评估
    let inventory_hints = [
        "inventorycache",
        "inventorychange",
        "inventoryallocation",
        "shipmentheaderservice",
        "shipmentdetailservice",
        "shipmentrollback",
        "backorder",
        "location_inventory",
        "shipment_alloc_request",
        "shipment_detail",
        "shipment_header",
        "inventoryprestatus",
        "onhandqty",
        "allocatedqty",
    ];
    if inventory_hints.iter().any(|h| p.contains(h)) {
        tags.push("库存".to_string());
    }
    if file.additions + file.deletions >= 500 {
        tags.push("大改动".to_string());
    }
    tags
}

pub(crate) fn aggregate_risk_tags(files: &[CodeReviewFileStat]) -> Vec<String> {
    let mut tags = Vec::<String>::new();
    for file in files {
        for tag in &file.risk_tags {
            if !tags.contains(tag) {
                tags.push(tag.clone());
            }
        }
    }
    tags
}
