use super::*;

#[test]
fn review_snapshot_drift_detects_changed_target_commit() {
    let repo = json!({
        "repoName": "repo-a",
        "branch": "feature/WMS-1",
        "resolvedTargetRef": "feature/WMS-1",
        "targetCommit": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    });
    let drift = review_snapshot_drift_from_repo_value(
        &repo,
        "feature/WMS-1",
        "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
    )
    .expect("changed commit should be stale");
    assert_eq!(drift.repo_name, "repo-a");
    assert_eq!(drift.branch, "feature/WMS-1");
    assert_eq!(
        drift.reviewed_target_commit,
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
    );
    assert_eq!(
        drift.current_target_commit,
        "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
    );
}


#[test]
fn review_snapshot_drift_ignores_same_or_missing_commit() {
    let repo = json!({
        "repoName": "repo-a",
        "branch": "feature/WMS-1",
        "resolvedTargetRef": "feature/WMS-1",
        "targetCommit": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    });
    assert!(review_snapshot_drift_from_repo_value(
        &repo,
        "feature/WMS-1",
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    )
    .is_none());
    assert!(review_snapshot_drift_from_repo_value(&repo, "feature/WMS-1", "").is_none());
    assert!(review_snapshot_drift_from_repo_value(
        &json!({"repoName":"repo-a"}),
        "feature/WMS-1",
        "bbbb"
    )
    .is_none());
}


#[test]
fn incremental_review_drift_parses_coverage_range() {
    let repo = json!({
        "repoName": "repo-a",
        "branch": "feature/WMS-1",
        "projectPath": "/tmp/repo-a",
        "coverageFromCommit": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "coverageToCommit": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
        "linearHistory": true,
    });
    let drift = incremental_review_drift_from_repo(&repo).expect("incremental drift");
    assert_eq!(drift.repo_name, "repo-a");
    assert_eq!(
        drift.reviewed_target_commit,
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
    );
    assert_eq!(
        drift.current_target_commit,
        "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
    );
}


#[test]
fn incremental_repo_cover_requires_linear_matching_range() {
    let drift = ReviewSnapshotDrift {
        repo_name: "repo-a".to_string(),
        branch: "feature/WMS-1".to_string(),
        project_path: Some(PathBuf::from("/tmp/repo-a")),
        reviewed_target_ref: "feature/WMS-1".to_string(),
        reviewed_target_commit: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string(),
        current_target_ref: "feature/WMS-1".to_string(),
        current_target_commit: "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".to_string(),
    };
    let matching = json!({
        "repoName": "repo-a",
        "branch": "feature/WMS-1",
        "coverageFromCommit": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "coverageToCommit": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
        "linearHistory": true,
    });
    assert!(incremental_repo_covers_drift(&matching, &drift));
    let rebased = json!({
        "repoName": "repo-a",
        "branch": "feature/WMS-1",
        "coverageFromCommit": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "coverageToCommit": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
        "linearHistory": false,
    });
    assert!(!incremental_repo_covers_drift(&rebased, &drift));
    let wrong_head = json!({
        "repoName": "repo-a",
        "branch": "feature/WMS-1",
        "coverageFromCommit": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "coverageToCommit": "cccccccccccccccccccccccccccccccccccccccc",
        "linearHistory": true,
    });
    assert!(!incremental_repo_covers_drift(&wrong_head, &drift));
}


#[tokio::test]
async fn diff_patch_export_writes_multiline_companion() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let req_dir = tmp.path().to_path_buf();
    let review = json!({
        "repos": [
            {"repoName": "repo-a", "branch": "feat/x", "diff": "diff --git a/f b/f\n@@ -1,2 +1,3 @@\n context\n-old\n+new\n"},
            {"repoName": "repo-b", "branch": "", "diff": ""},
        ]
    });
    let written =
        crate::git_workflow::export_diff_patch_file(&req_dir, CODE_REVIEW_PATCH_FILE, &review)
            .await
            .expect("export");
    assert!(written);
    let text = std::fs::read_to_string(req_dir.join(CODE_REVIEW_PATCH_FILE)).expect("read patch");
    assert!(text.starts_with("# ==== repo: repo-a (branch: feat/x) ====\n"));
    // patch 文件里的换行必须是真实换行（多行文本），而非 JSON 里的 \n 字面量单行。
    assert!(text.contains("\n-old\n+new\n"), "patch 应按真实换行落盘");
    assert!(!text.contains("repo-b"), "空 diff 仓库不产生段落");

    // 空快照：返回 false 并清掉旧 patch，避免材料引用与内容不一致。
    let empty = json!({"repos": [{"repoName": "repo-c", "branch": "", "diff": ""}]});
    std::fs::write(req_dir.join(CODE_REVIEW_PATCH_FILE), "stale").expect("write stale");
    let written =
        crate::git_workflow::export_diff_patch_file(&req_dir, CODE_REVIEW_PATCH_FILE, &empty)
            .await
            .expect("export empty");
    assert!(!written);
    assert!(
        !req_dir.join(CODE_REVIEW_PATCH_FILE).exists(),
        "空快照应清掉旧 patch 文件"
    );
}


#[tokio::test]
async fn review_checklist_put_validates_and_renders_review_md() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let data = tmp.path().join("data");
    let pi_root = tmp.path().join("pi-sessions");
    let dsh_root = tmp.path().join("dsh-sessions");
    std::fs::create_dir_all(&data).expect("create data dir");
    let proj = tmp.path().join("proj");
    let req_dir = proj.join("req").join("T-900");
    std::fs::create_dir_all(&req_dir).expect("create req dir");
    std::fs::write(
        req_dir.join("meta.md"),
        "---\nreq-id: T-900\ntitle: 审查清单测试\nstatus: 自测中\n---\n正文\n",
    )
    .expect("write meta.md");
    let config = json!({ "requirementScanRoots": [proj.to_string_lossy()] });
    std::fs::write(data.join("config.json"), config.to_string()).expect("write config.json");
    let state = temp_app_state(&data, &pi_root, &dsh_root);

    // 1) conclusion 非法 → 400 且错误信息教格式
    let bad = api_requirement_review_checklist_put(
        State(state.clone()),
        FormOrJson(ReviewChecklistSaveForm {
            req_id: "T-900".into(),
            items: Some(json!([{ "id": "C1", "title": "幂等", "conclusion": "todo" }])),
        }),
    )
    .await;
    let err = bad.expect_err("invalid conclusion must be rejected");
    assert!(err.message.contains("只允许 pass / fail / na"), "{}", err.message);
    assert!(err.message.contains("items"), "错误信息应包含 schema 提示");

    // 2) 合法保存 → 写 review-checklist.json + review.md 受管小节
    let ok = api_requirement_review_checklist_put(
        State(state.clone()),
        FormOrJson(ReviewChecklistSaveForm {
            req_id: "T-900".into(),
            items: Some(json!([
                { "id": "C1", "title": "幂等与并发安全", "conclusion": "PASS", "note": "锁内复查" },
                { "id": "C2", "title": "部署顺序", "conclusion": "na" }
            ])),
        }),
    )
    .await
    .expect("valid save");
    assert_eq!(ok.0["ok"], json!(true));
    let saved = std::fs::read_to_string(req_dir.join(REVIEW_CHECKLIST_FILE)).expect("checklist file");
    assert!(saved.contains("\"conclusion\": \"pass\""), "conclusion 归一化为小写: {saved}");
    let review_md = std::fs::read_to_string(req_dir.join("review.md")).expect("review.md");
    assert!(review_md.contains("<!-- panel:review-checklist:start -->"));
    assert!(review_md.contains("✅ pass"));
    assert!(review_md.contains("➖ na"));

    // 3) 重复保存 → 受管块替换而非叠加
    let _ = api_requirement_review_checklist_put(
        State(state.clone()),
        FormOrJson(ReviewChecklistSaveForm {
            req_id: "T-900".into(),
            items: Some(json!([{ "title": "新要点", "conclusion": "fail", "note": "发现问题" }])),
        }),
    )
    .await
    .expect("resave");
    let review_md = std::fs::read_to_string(req_dir.join("review.md")).expect("review.md v2");
    assert_eq!(
        review_md.matches("<!-- panel:review-checklist:start -->").count(),
        1,
        "受管块只保留一份"
    );
    assert!(review_md.contains("❌ fail"));
    // 缺 id 自动编号 C1
    let saved = std::fs::read_to_string(req_dir.join(REVIEW_CHECKLIST_FILE)).expect("checklist v2");
    assert!(saved.contains("\"id\": \"C1\""));
}


#[tokio::test]
async fn review_gate_requires_completed_checklist() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let data = tmp.path().join("data");
    let pi_root = tmp.path().join("pi-sessions");
    let dsh_root = tmp.path().join("dsh-sessions");
    std::fs::create_dir_all(&data).expect("create data dir");
    let proj = tmp.path().join("proj");
    let req_dir = proj.join("req").join("T-901");
    std::fs::create_dir_all(&req_dir).expect("create req dir");
    std::fs::write(
        req_dir.join("meta.md"),
        "---\nreq-id: T-901\ntitle: 门禁清单校验\nstatus: 自测中\n---\n正文\n",
    )
    .expect("write meta.md");
    let config = json!({ "requirementScanRoots": [proj.to_string_lossy()] });
    std::fs::write(data.join("config.json"), config.to_string()).expect("write config.json");
    std::fs::write(
        req_dir.join("review.md"),
        "# T-901 审查\n\nReview Gate: PASS\n",
    )
    .expect("write review.md");
    let mut req = default_requirement(Vec::new());
    req.id = "T-901".to_string();
    req.title = "门禁清单校验".to_string();
    req.status = "自测中".to_string();
    req.req_dir = Some(req_dir.to_string_lossy().to_string());

    // 1) PASS 但无 skill 标记 → skill-required（门禁强制审查走 agent-panel-code-review skill，
    //    标记校验优先于清单校验：未经 skill 的浅层 PASS 直接拦下）
    let decision = review_gate_decision(&req).await.expect("decision");
    assert_eq!(decision.status, "skill-required", "{}", decision.reason);
    assert!(!decision.allows_testing);

    // 2) 补上 skill 标记但仍无清单 → checklist-missing
    std::fs::write(
        req_dir.join("review.md"),
        "# T-901 审查\n\nReview Gate: PASS\nSource: agent-panel-code-review skill\n",
    )
    .expect("rewrite review.md with skill marker");
    let decision = review_gate_decision(&req).await.expect("decision");
    assert_eq!(decision.status, "checklist-missing");
    assert!(!decision.allows_testing);

    // 3) 清单有 fail 项 → blocked
    std::fs::write(
        req_dir.join(REVIEW_CHECKLIST_FILE),
        json!({
            "version": 1, "reqId": "T-901",
            "items": [
                { "id": "C1", "title": "幂等", "conclusion": "pass" },
                { "id": "C2", "title": "部署顺序", "conclusion": "fail" }
            ]
        })
        .to_string(),
    )
    .expect("write checklist");
    let decision = review_gate_decision(&req).await.expect("decision");
    assert_eq!(decision.status, "blocked", "{}", decision.reason);
    assert!(decision.reason.contains("C2"), "{}", decision.reason);

    // 4) 全部 pass/na + PASS 标记 + skill 标记，但无代码问题备注 → annotations-required
    std::fs::write(
        req_dir.join(REVIEW_CHECKLIST_FILE),
        json!({
            "version": 1, "reqId": "T-901",
            "items": [
                { "id": "C1", "title": "幂等", "conclusion": "pass" },
                { "id": "C2", "title": "部署顺序", "conclusion": "na", "note": "单仓" }
            ]
        })
        .to_string(),
    )
    .expect("write checklist v2");
    let decision = review_gate_decision(&req).await.expect("decision");
    assert_eq!(decision.status, "annotations-required", "{}", decision.reason);
    assert!(!decision.allows_testing);

    // 5) 落盘代码问题备注（无材料快照场景：只要求 files 非空）→ passed
    std::fs::write(
        req_dir.join(CODE_ANNOTATIONS_FILE),
        json!({
            "version": 1, "reqId": "T-901",
            "files": [
                { "repo": "repo-a", "path": "src/A.java", "summary": "改动目的", "notes": [] }
            ]
        })
        .to_string(),
    )
    .expect("write annotations");
    let decision = review_gate_decision(&req).await.expect("decision");
    assert_eq!(decision.status, "passed", "{}", decision.reason);
    assert!(decision.allows_testing);

    // 6) 材料快照存在但备注缺 reviewedCommit 指纹 → annotations-stale
    std::fs::write(
        req_dir.join(CODE_REVIEW_FILE),
        json!({
            "version": 1, "reqId": "T-901",
            "repos": [
                { "repoName": "repo-a", "branch": "feature/x", "targetCommit": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa" }
            ]
        })
        .to_string(),
    )
    .expect("write material snapshot");
    let decision = review_gate_decision(&req).await.expect("decision");
    assert_eq!(decision.status, "annotations-stale", "{}", decision.reason);
    assert!(!decision.allows_testing);

    // 7) 补上与材料一致的 reviewedCommit 指纹 → passed
    std::fs::write(
        req_dir.join(CODE_ANNOTATIONS_FILE),
        json!({
            "version": 1, "reqId": "T-901",
            "reviewedCommit": { "repo-a": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa" },
            "files": [
                { "repo": "repo-a", "path": "src/A.java", "summary": "改动目的", "notes": [] }
            ]
        })
        .to_string(),
    )
    .expect("rewrite annotations with fingerprint");
    let decision = review_gate_decision(&req).await.expect("decision");
    assert_eq!(decision.status, "passed", "{}", decision.reason);
    assert!(decision.allows_testing);

    // 8) 清单 JSON 损坏 → checklist-error
    std::fs::write(req_dir.join(REVIEW_CHECKLIST_FILE), "{oops").expect("corrupt");
    let decision = review_gate_decision(&req).await.expect("decision");
    assert_eq!(decision.status, "checklist-error");
    assert!(!decision.allows_testing);
}

#[tokio::test]
async fn review_gate_blocks_on_p0_findings() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let data = tmp.path().join("data");
    let proj = tmp.path().join("proj");
    let req_dir = proj.join("req").join("T-904");
    std::fs::create_dir_all(&data).expect("create data dir");
    std::fs::create_dir_all(&req_dir).expect("create req dir");
    std::fs::write(
        req_dir.join("meta.md"),
        "---\nreq-id: T-904\ntitle: P0 拦截\nstatus: 自测中\n---\n正文\n",
    )
    .expect("write meta.md");
    let config = json!({ "requirementScanRoots": [proj.to_string_lossy()] });
    std::fs::write(data.join("config.json"), config.to_string()).expect("write config.json");
    let mut req = default_requirement(Vec::new());
    req.id = "T-904".to_string();
    req.status = "自测中".to_string();
    req.req_dir = Some(req_dir.to_string_lossy().to_string());

    // 文档写 PASS 但 P0 小节有实质内容 → 门禁按实际内容拦截
    std::fs::write(
        req_dir.join("review.md"),
        "# T-904 审查\n\nReview Gate: PASS\nSource: agent-panel-code-review skill\n\n## P0 阻断\n\n- [P0][逻辑] InventoryService.java:88：并发下库存重复释放，会导致数据错乱\n",
    )
    .expect("write review.md");
    let decision = review_gate_decision(&req).await.expect("decision");
    assert_eq!(decision.status, "blocked", "{}", decision.reason);
    assert!(!decision.allows_testing);
}

#[tokio::test]
async fn review_gate_passes_with_p1_warnings() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let data = tmp.path().join("data");
    let proj = tmp.path().join("proj");
    let req_dir = proj.join("req").join("T-906");
    std::fs::create_dir_all(&data).expect("create data dir");
    std::fs::create_dir_all(&req_dir).expect("create req dir");
    std::fs::write(
        req_dir.join("meta.md"),
        "---\nreq-id: T-906\ntitle: P1 警示放行\nstatus: 自测中\n---\n正文\n",
    )
    .expect("write meta.md");
    let config = json!({ "requirementScanRoots": [proj.to_string_lossy()] });
    std::fs::write(data.join("config.json"), config.to_string()).expect("write config.json");
    let mut req = default_requirement(Vec::new());
    req.id = "T-906".to_string();
    req.status = "自测中".to_string();
    req.req_dir = Some(req_dir.to_string_lossy().to_string());

    // P1 严重问题非空：放行（allows_testing=true）但带警示
    std::fs::write(
        req_dir.join("review.md"),
        "# T-906 审查\n\nReview Gate: PASS\nSource: agent-panel-code-review skill\n\n## P0 阻断\n\n无\n\n## P1 严重\n\n- [P1][性能] OrderMapper.xml:42：新增查询未走索引，大单量仓会拖慢接口\n",
    )
    .expect("write review.md");
    std::fs::write(
        req_dir.join(REVIEW_CHECKLIST_FILE),
        json!({ "version": 1, "reqId": "T-906", "items": [
            { "id": "C1", "title": "性能红线", "conclusion": "pass" }
        ] })
        .to_string(),
    )
    .expect("write checklist");
    std::fs::write(
        req_dir.join(CODE_ANNOTATIONS_FILE),
        json!({ "version": 1, "reqId": "T-906", "files": [
            { "repo": "repo-a", "path": "src/A.java", "summary": "改动目的", "notes": [] }
        ] })
        .to_string(),
    )
    .expect("write annotations");

    let decision = review_gate_decision(&req).await.expect("decision");
    assert_eq!(decision.status, "passed", "{}", decision.reason);
    assert!(decision.allows_testing);
    assert!(decision.warnings.iter().any(|w| w.contains("P1") && w.contains("待修复")), "{:?}", decision.warnings);
}

#[tokio::test]
async fn review_gate_requires_selftest_crosscheck() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let data = tmp.path().join("data");
    let proj = tmp.path().join("proj");
    let req_dir = proj.join("req").join("T-905");
    std::fs::create_dir_all(&data).expect("create data dir");
    std::fs::create_dir_all(&req_dir).expect("create req dir");
    std::fs::write(
        req_dir.join("meta.md"),
        "---\nreq-id: T-905\ntitle: 交叉评估校验\nstatus: 自测中\n---\n正文\n",
    )
    .expect("write meta.md");
    // test.md 已有自测清单（三分类）→ review 必须含自测清单交叉评估
    std::fs::write(
        req_dir.join("test.md"),
        "# T-905 Test\n\n## 自测清单\n\n### 主流程测试\n\n| # | 自测项 | 结果 | 失败/无法测试原因 |\n| --- | --- | --- | --- |\n| 1 | 主流程 | 通过 | - |\n\n### 边界场景测试\n\n不适用：无边界输入\n\n### 高并发/大流量场景测试\n\n不适用：无并发路径\n",
    )
    .expect("write test.md");
    let config = json!({ "requirementScanRoots": [proj.to_string_lossy()] });
    std::fs::write(data.join("config.json"), config.to_string()).expect("write config.json");
    let mut req = default_requirement(Vec::new());
    req.id = "T-905".to_string();
    req.status = "自测中".to_string();
    req.req_dir = Some(req_dir.to_string_lossy().to_string());

    std::fs::write(
        req_dir.join("review.md"),
        "# T-905 审查\n\nReview Gate: PASS\nSource: agent-panel-code-review skill\n",
    )
    .expect("write review.md");
    std::fs::write(
        req_dir.join(REVIEW_CHECKLIST_FILE),
        json!({ "version": 1, "reqId": "T-905", "items": [
            { "id": "C1", "title": "幂等", "conclusion": "pass" }
        ] })
        .to_string(),
    )
    .expect("write checklist");
    std::fs::write(
        req_dir.join(CODE_ANNOTATIONS_FILE),
        json!({ "version": 1, "reqId": "T-905", "files": [
            { "repo": "repo-a", "path": "src/A.java", "summary": "改动目的", "notes": [] }
        ] })
        .to_string(),
    )
    .expect("write annotations");

    // 1) 缺自测清单交叉评估 → crosscheck-missing
    let decision = review_gate_decision(&req).await.expect("decision");
    assert_eq!(decision.status, "crosscheck-missing", "{}", decision.reason);
    assert!(!decision.allows_testing);

    // 2) 补上交叉评估小节 → passed
    std::fs::write(
        req_dir.join("review.md"),
        "# T-905 审查\n\nReview Gate: PASS\nSource: agent-panel-code-review skill\n\n## 自测清单交叉评估\n\n- 边界/并发流量：自测已标不适用，扩大阅读未发现新增边界与并发风险\n- P0/P1：无\n",
    )
    .expect("rewrite review.md with crosscheck");
    let decision = review_gate_decision(&req).await.expect("decision");
    assert_eq!(decision.status, "passed", "{}", decision.reason);
    assert!(decision.allows_testing);
}

#[test]
fn status_prefers_incremental_only_from_release_ready() {
    // 发布就绪之前（含更早需求流状态）默认全量审查
    assert!(!status_prefers_incremental("需求澄清"));
    assert!(!status_prefers_incremental("开发中"));
    assert!(!status_prefers_incremental("自测中"));
    assert!(!status_prefers_incremental("测试中"));
    // 发布就绪及之后默认增量审查
    assert!(status_prefers_incremental("发布就绪"));
    assert!(status_prefers_incremental("经验总结"));
    assert!(status_prefers_incremental("已完成"));
    // 线上问题等非需求流状态：保守全量
    assert!(!status_prefers_incremental("排查中"));
    assert!(!status_prefers_incremental("未知状态"));
}

#[tokio::test]
async fn prepare_review_materials_rejects_unknown_mode() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let req_dir = tmp.path().to_path_buf();
    let scope = BranchScope::default();
    let err = prepare_review_materials(&req_dir, "T-902", &scope, Some("xyz"), "自测中")
        .await
        .expect_err("unknown mode must be rejected");
    assert!(err.to_string().contains("mode 只支持"), "{}", err);
    // auto 显式传值等价缺省，不报错（空 scope 也不应该在 mode 校验层失败）
    let _ = prepare_review_materials(&req_dir, "T-902", &scope, Some("auto"), "自测中").await;
}

