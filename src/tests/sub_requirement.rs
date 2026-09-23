use super::*;

#[test]
fn sub_req_seq_extracts_trailing_number() {
    assert_eq!(sub_req_seq("WMS-049-S1-fix-logging"), Some(1));
    assert_eq!(sub_req_seq("WMS-049-S2"), Some(2));
    assert_eq!(sub_req_seq("WMS-049-S12-x-y"), Some(12));
    assert_eq!(sub_req_seq("WMS-049-wave-pick-task"), None);
    assert_eq!(sub_req_seq("WMS-GRP-001-x"), None);
}


#[test]
fn sub_branch_name_appends_suffix() {
    assert_eq!(
        sub_branch_name("hevin.yang/feature/WMS-049-wave-pick-task", 1),
        "hevin.yang/feature/WMS-049-wave-pick-task-sub1"
    );
    assert_eq!(sub_branch_name("feature/x-", 3), "feature/x-sub3");
}


fn sub_req_for_test(status: &str) -> Requirement {
    let mut req = default_requirement_for_test("WMS-049-S1-fix-logging");
    req.status = status.to_string();
    req.is_sub_req = true;
    req.parent_req_id = Some("WMS-049-wave-pick-task".to_string());
    req
}


#[test]
fn sub_status_transition_enforces_flow() {
    use requirement_service::ensure_sub_req_status_transition;
    // 正常流转
    assert!(ensure_sub_req_status_transition(&sub_req_for_test("需求创建"), "开发中").is_ok());
    assert!(ensure_sub_req_status_transition(&sub_req_for_test("开发中"), "已合入").is_ok());
    assert!(ensure_sub_req_status_transition(&sub_req_for_test("开发中"), "已取消").is_ok());
    assert!(ensure_sub_req_status_transition(&sub_req_for_test("需求创建"), "已取消").is_ok());
    // 跳过流转拒绝
    assert!(ensure_sub_req_status_transition(&sub_req_for_test("需求创建"), "已合入").is_err());
    // 终态拒绝
    assert!(ensure_sub_req_status_transition(&sub_req_for_test("已合入"), "开发中").is_err());
    assert!(ensure_sub_req_status_transition(&sub_req_for_test("已取消"), "开发中").is_err());
    assert!(ensure_sub_req_status_transition(&sub_req_for_test("已合入"), "已取消").is_err());
    // 子状态集之外拒绝
    assert!(ensure_sub_req_status_transition(&sub_req_for_test("开发中"), "测试中").is_err());
    assert!(ensure_sub_req_status_transition(&sub_req_for_test("开发中"), "已完成").is_err());
    // 同状态 no-op 放行
    assert!(ensure_sub_req_status_transition(&sub_req_for_test("开发中"), "开发中").is_ok());
}


#[test]
fn non_sub_req_cannot_use_sub_only_statuses() {
    use requirement_service::ensure_status_allowed_for_non_sub;
    assert!(ensure_status_allowed_for_non_sub("需求创建").is_err());
    assert!(ensure_status_allowed_for_non_sub("已合入").is_err());
    assert!(ensure_status_allowed_for_non_sub("已取消").is_err());
    assert!(ensure_status_allowed_for_non_sub("开发中").is_ok());
    assert!(ensure_status_allowed_for_non_sub("排查中").is_ok());
}


#[test]
fn build_meta_doc_writes_parent_req_id_frontmatter() {
    let meta = build_meta_doc(
        "WMS-049-S1-fix-logging",
        "t",
        "需求创建",
        "WMS",
        &["WMS".into()],
        "需求",
        "产品推动",
        "hevin",
        "2026-01-01",
        "unknown",
        "",
        &[],
        "s",
        Some("WMS-049-wave-pick-task"),
    );
    assert!(meta.contains("parent-req-id: WMS-049-wave-pick-task"));
    assert!(meta.contains("- Parent requirement: WMS-049-wave-pick-task"));
    let without = build_meta_doc(
        "WMS-100-x",
        "t",
        "需求澄清",
        "WMS",
        &["WMS".into()],
        "需求",
        "产品推动",
        "hevin",
        "2026-01-01",
        "unknown",
        "",
        &[],
        "s",
        None,
    );
    assert!(!without.contains("parent-req-id"));
}


#[test]
fn snapshot_note_mentions_parent_and_independence() {
    let note = snapshot_note("WMS-049-wave-pick-task", "background.md");
    assert!(note.contains("快照自父需求 `WMS-049-wave-pick-task`"));
    assert!(note.contains("background.md"));
    assert!(note.contains("独立维护"));
}


/// 子需求端到端（进程内直调 handler）：创建 → 文档快照复制 → 列表过滤 → 父子回填 →
/// 子需求轻量状态机 → 字段边界（ONES/plan-release/issues/类别）。
#[tokio::test]
async fn sub_requirement_create_flow_end_to_end() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let data = tmp.path().join("data");
    let pi_root = tmp.path().join("pi-sessions");
    let dsh_root = tmp.path().join("dsh-sessions");
    std::fs::create_dir_all(&data).expect("create data dir");
    let proj = tmp.path().join("proj");
    let parent_dir = proj.join("req").join("WMS-049-wave-pick-task");
    std::fs::create_dir_all(&parent_dir).expect("create parent req dir");
    std::fs::write(
        parent_dir.join("meta.md"),
        "---\nreq-id: WMS-049-wave-pick-task\ntitle: 波次拣选任务\nstatus: 开发中\nproject: WMS\ncategory: 需求\nsource: 产品推动\nowner: hevin\nstart-date: 2026-01-01\nplan-release: unknown\n---\n父需求正文\n",
    )
    .expect("write parent meta.md");
    std::fs::write(
        parent_dir.join("state.json"),
        json!({"version": 1, "status": "开发中", "category": "需求", "history": []}).to_string(),
    )
    .expect("write parent state.json");
    std::fs::write(
        parent_dir.join("background.md"),
        "# 波次拣选背景\n- 业务目标\n",
    )
    .expect("write background");
    std::fs::write(parent_dir.join("technical-plan.md"), "# 技术方案\n- 步骤\n")
        .expect("write technical-plan");
    let config = json!({ "requirementScanRoots": [proj.to_string_lossy()] });
    std::fs::write(data.join("config.json"), config.to_string()).expect("write config.json");
    let state = temp_app_state(&data, &pi_root, &dsh_root);

    // 1) 创建子需求（dry-run 预览不落盘）。
    let preview = api_requirement_create_sub(
        State(state.clone()),
        FormOrJson(SubRequirementCreateForm {
            parent_req_id: "WMS-049-wave-pick-task".into(),
            title: "修复拣选日志".into(),
            slug: Some("fix-logging".into()),
            owner: None,
            summary: Some("负责日志改造".into()),
            dry_run: Some(true),
        }),
    )
    .await
    .expect("dry-run create sub");
    assert_eq!(preview["reqId"], json!("WMS-049-S1-fix-logging"));
    assert_eq!(preview["parentReqId"], json!("WMS-049-wave-pick-task"));

    // 2) 真实创建：ID 分配 + 文档快照复制 + parent-req-id frontmatter。
    let created = api_requirement_create_sub(
        State(state.clone()),
        FormOrJson(SubRequirementCreateForm {
            parent_req_id: "WMS-049-wave-pick-task".into(),
            title: "修复拣选日志".into(),
            slug: Some("fix-logging".into()),
            owner: None,
            summary: Some("负责日志改造".into()),
            dry_run: Some(false),
        }),
    )
    .await
    .expect("create sub");
    assert_eq!(created["reqId"], json!("WMS-049-S1-fix-logging"));
    let sub_dir = PathBuf::from(created["reqDir"].as_str().expect("reqDir"));
    let sub_meta = std::fs::read_to_string(sub_dir.join("meta.md")).expect("read sub meta");
    assert!(sub_meta.contains("parent-req-id: WMS-049-wave-pick-task"));
    assert!(sub_meta.contains("status: 需求创建"));
    assert!(!sub_meta.contains("ones:"));
    let sub_background =
        std::fs::read_to_string(sub_dir.join("background.md")).expect("read sub background");
    assert!(sub_background.contains("快照自父需求 `WMS-049-wave-pick-task`"));
    assert!(sub_background.contains("波次拣选背景"));
    // 父需求 notes 留痕
    let parent_notes = std::fs::read_to_string(parent_dir.join("notes.md")).expect("parent notes");
    assert!(parent_notes.contains("拆分子需求：`WMS-049-S1-fix-logging`"));

    // 3) 列表过滤：默认不含子需求；includeSubs=true 包含；父需求 subReqs 回填。
    let list_default = api_requirements(State(state.clone()), Query(IdQuery::default()))
        .await
        .expect("default list");
    let ids_default: Vec<&str> = list_default.0["requirements"]
        .as_array()
        .expect("arr")
        .iter()
        .filter_map(|r| r["id"].as_str())
        .collect();
    assert!(!ids_default.contains(&"WMS-049-S1-fix-logging"));
    let list_all = api_requirements(
        State(state.clone()),
        Query(IdQuery {
            include_subs: Some(true),
            ..Default::default()
        }),
    )
    .await
    .expect("all list");
    let subs_of_parent: Vec<Value> = list_all.0["requirements"]
        .as_array()
        .expect("arr")
        .iter()
        .find(|r| r["id"] == json!("WMS-049-wave-pick-task"))
        .expect("parent")
        .get("subReqs")
        .expect("subReqs")
        .as_array()
        .expect("subReqs")
        .clone();
    assert_eq!(subs_of_parent.len(), 1);
    assert_eq!(subs_of_parent[0]["reqId"], json!("WMS-049-S1-fix-logging"));
    assert_eq!(subs_of_parent[0]["status"], json!("需求创建"));
    assert_eq!(subs_of_parent[0]["found"], json!(true));

    // 4) 子需求轻量状态机：跳过流转拒绝（via=ui 也不放行），正常流转放行。
    let status_form = |status: &str, via: Option<String>| {
        FormOrJson(StatusForm {
            req_id: "WMS-049-S1-fix-logging".into(),
            status: status.into(),
            note: None,
            via,
        })
    };
    let skipped = api_requirement_status(
        State(state.clone()),
        status_form("已合入", Some("ui".into())),
    )
    .await
    .expect_err("需求创建 -> 已合入 must be rejected even via=ui");
    assert!(format!("{:?}", skipped).contains("不允许"));
    let _ = api_requirement_status(
        State(state.clone()),
        status_form("开发中", Some("ui".into())),
    )
    .await
    .expect("需求创建 -> 开发中");
    let _ = api_requirement_status(State(state.clone()), status_form("已合入", None))
        .await
        .expect("开发中 -> 已合入");
    let terminal = api_requirement_status(State(state.clone()), status_form("开发中", None))
        .await
        .expect_err("terminal status must not transition");
    assert!(format!("{:?}", terminal).contains("终态"));

    // 5) 普通需求不能用子需求专属状态；子需求不能绑 ONES / 切类别。
    let normal_blocked = api_requirement_status(
        State(state.clone()),
        FormOrJson(StatusForm {
            req_id: "WMS-049-wave-pick-task".into(),
            status: "需求创建".into(),
            note: None,
            via: Some("ui".into()),
        }),
    )
    .await
    .expect_err("normal req must not use sub-only status");
    assert!(format!("{:?}", normal_blocked).contains("子需求专属状态"));
    let ones_blocked = api_requirement_update(
        State(state.clone()),
        FormOrJson(RequirementPatchForm {
            req_id: "WMS-049-S1-fix-logging".into(),
            title: None,
            project: None,
            projects: None,
            status: None,
            category: None,
            source: None,
            owner: None,
            start_date: None,
            plan_release: None,
            ones: Some("https://ones.example/issue".into()),
            issues: None,
            note: None,
            dry_run: None,
        }),
    )
    .await
    .expect_err("sub must not bind ones");
    assert!(format!("{:?}", ones_blocked).contains("父需求承载"));

    // 6) 守卫：不能给子需求再拆子需求（只允许一层）。
    let nested = api_requirement_create_sub(
        State(state.clone()),
        FormOrJson(SubRequirementCreateForm {
            parent_req_id: "WMS-049-S1-fix-logging".into(),
            title: "嵌套子需求".into(),
            slug: None,
            owner: None,
            summary: None,
            dry_run: Some(true),
        }),
    )
    .await
    .expect_err("nested sub must be rejected");
    assert!(format!("{:?}", nested).contains("不支持子需求嵌套"));
}

// ===================== 文档分册（doc parts）单元测试 =====================

