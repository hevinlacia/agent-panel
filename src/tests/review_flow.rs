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

    // 1) 无清单 → checklist-missing（即使写了 PASS 也不放行）
    let decision = review_gate_decision(&req).await.expect("decision");
    assert_eq!(decision.status, "checklist-missing");
    assert!(!decision.allows_testing);

    // 2) 清单有 fail 项 → blocked
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

    // 3) 全部 pass/na + PASS 标记 → passed
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
    assert_eq!(decision.status, "passed", "{}", decision.reason);
    assert!(decision.allows_testing);

    // 4) 清单 JSON 损坏 → checklist-error
    std::fs::write(req_dir.join(REVIEW_CHECKLIST_FILE), "{oops").expect("corrupt");
    let decision = review_gate_decision(&req).await.expect("decision");
    assert_eq!(decision.status, "checklist-error");
    assert!(!decision.allows_testing);
}

