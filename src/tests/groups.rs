use super::*;

#[tokio::test]
async fn create_requirement_group_rejects_issue_category_and_legacy_pool_id() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let state = group_test_state(&tmp);
    create_requirement(&state, group_test_form("G-M3", "成员3"))
        .await
        .expect("create member");

    // 组不能使用 issue 类别
    let mut issue_group = group_test_form("WMS-GRP-001-bad", "issue 类别组");
    issue_group.category = Some("线上问题".to_string());
    issue_group.members = Some(vec![group_test_member_input("G-M3")]);
    let err = create_requirement(&state, issue_group)
        .await
        .expect_err("group with issue category must fail");
    assert!(err.message.contains("category=需求"), "{:?}", err);

    // 组必须使用 WMS-GRP 池：legacy 需求池形态被拒
    let mut legacy = group_test_form("WMS-126-legacy-combo", "需求池 id 的组");
    legacy.members = Some(vec![group_test_member_input("G-M3")]);
    let err = create_requirement(&state, legacy)
        .await
        .expect_err("legacy pool group id must fail");
    assert!(err.message.contains("WMS-GRP"), "{:?}", err);

    // 默认池 + {seq} 模板：组创建成功并自动分配 WMS-GRP-001
    let mut ok = group_test_form("", "默认池组");
    ok.members = Some(vec![group_test_member_input("G-M3")]);
    ok.req_id = "WMS-GRP-{seq}-combo".to_string();
    let res = create_requirement(&state, ok).await.expect("create group");
    assert_eq!(res["reqId"], json!("WMS-GRP-001-combo"));
}


/// 并行创建不得撞号（回归）：两个 `{seq}` 模板并发创建（slug 不同）必须拿到连续且
/// 不同的序号。旧实现仅靠 create_dir 的 AlreadyExists 兜底，slug 不同则目录名不同
/// 永不冲突；而扫描式占号也观察不到未写 meta.md 的预留目录，导致两个请求都拿到 117。
/// 现由 `requirement_create_lock` 串行化扫描 -> 预留 -> 写 meta 临界区。
#[tokio::test]
async fn concurrent_create_allocates_distinct_seq_numbers() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let data = tmp.path().join("data");
    let pi_root = tmp.path().join("pi-sessions");
    let dsh_root = tmp.path().join("dsh-sessions");
    std::fs::create_dir_all(&data).expect("create data dir");
    let proj = tmp.path().join("proj");
    std::fs::create_dir_all(&proj).expect("create proj dir");
    let config = json!({ "requirementScanRoots": [proj.to_string_lossy()] });
    std::fs::write(data.join("config.json"), config.to_string()).expect("write config.json");
    let state = temp_app_state(&data, &pi_root, &dsh_root);

    let form = |slug: &str| RequirementCreateForm {
        req_id: format!("T-{{seq}}-{slug}"),
        title: format!("并发占号 {slug}"),
        project: None,
        projects: None,
        group_path: None,
        parent_req_id: None,
        root: None,
        status: None,
        category: None,
        source: None,
        owner: None,
        start_date: None,
        plan_release: None,
        ones: None,
        issues: None,
        members: None,
        release_policy: None,
        summary: None,
        background: None,
        notes: None,
        dry_run: None,
    };

    // 并发创建两个不同 slug 的需求：旧实现在这里双双拿到 T-001。
    let (res_a, res_b) = tokio::join!(
        create_requirement(&state, form("aaa")),
        create_requirement(&state, form("bbb"))
    );
    let id_a = res_a.expect("create a")["reqId"]
        .as_str()
        .expect("reqId a")
        .to_string();
    let id_b = res_b.expect("create b")["reqId"]
        .as_str()
        .expect("reqId b")
        .to_string();
    assert_ne!(id_a, id_b, "parallel creates must not share a reqId");
    let mut ids = vec![id_a, id_b];
    ids.sort();
    assert_eq!(ids, vec!["T-001-aaa", "T-002-bbb"]);
    // 创建根是 scan root 下的 .agents/req（writable_req_roots 展开规则）
    let req_root = proj.join(".agents/req");
    assert!(req_root.join("T-001-aaa").join("meta.md").is_file());
    assert!(req_root.join("T-002-bbb").join("meta.md").is_file());
}


fn group_test_form(req_id: &str, title: &str) -> RequirementCreateForm {
    RequirementCreateForm {
        req_id: req_id.to_string(),
        title: title.to_string(),
        project: None,
        projects: None,
        group_path: None,
        parent_req_id: None,
        root: None,
        status: None,
        category: None,
        source: None,
        owner: None,
        start_date: None,
        plan_release: None,
        ones: None,
        issues: None,
        members: None,
        release_policy: None,
        summary: None,
        background: None,
        notes: None,
        dry_run: None,
    }
}


fn group_test_state(tmp: &tempfile::TempDir) -> AppState {
    let data = tmp.path().join("data");
    let proj = tmp.path().join("proj");
    std::fs::create_dir_all(&data).expect("create data dir");
    std::fs::create_dir_all(&proj).expect("create proj dir");
    let config = json!({ "requirementScanRoots": [proj.to_string_lossy()] });
    std::fs::write(data.join("config.json"), config.to_string()).expect("write config.json");
    temp_app_state(
        &data,
        &tmp.path().join("pi-sessions"),
        &tmp.path().join("dsh-sessions"),
    )
}


fn group_test_member_input(id: &str) -> GroupMemberInput {
    GroupMemberInput {
        req_id: id.to_string(),
        note: None,
    }
}


#[tokio::test]
async fn group_status_aggregation_takes_min_member_status() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let state = group_test_state(&tmp);
    create_requirement(&state, group_test_form("G-MEMBER-A", "成员A"))
        .await
        .expect("create member a");
    create_requirement(&state, group_test_form("G-MEMBER-B", "成员B"))
        .await
        .expect("create member b");
    let req_a = get_real_requirement(&state, "G-MEMBER-A")
        .await
        .expect("req a");
    write_requirement_status(req_a.req_dir.as_deref().unwrap_or_default(), "测试中", None)
        .await
        .expect("set status a");
    let req_b = get_real_requirement(&state, "G-MEMBER-B")
        .await
        .expect("req b");
    write_requirement_status(req_b.req_dir.as_deref().unwrap_or_default(), "开发中", None)
        .await
        .expect("set status b");

    let mut form = group_test_form("WMS-GRP-001-aggregation", "联合需求组");
    form.members = Some(vec![
        group_test_member_input("G-MEMBER-A"),
        group_test_member_input("G-MEMBER-B"),
    ]);
    form.release_policy = Some("together".to_string());
    create_requirement(&state, form)
        .await
        .expect("create group");

    let reqs = list_requirements(&state).await.expect("list");
    let group = reqs
        .iter()
        .find(|r| r.id == "WMS-GRP-001-aggregation")
        .expect("group req");
    let members = group.group_members.as_ref().expect("group members");
    assert_eq!(members.len(), 2);
    assert!(members.iter().all(|m| m.found));
    assert_eq!(
        members
            .iter()
            .find(|m| m.req_id == "G-MEMBER-A")
            .and_then(|m| m.status.clone()),
        Some("测试中".to_string())
    );
    // 总进度 = min(成员进度)：A 测试中(3)、B 开发中(1) -> 开发中，瓶颈 B。
    assert_eq!(group.group_status.as_deref(), Some("开发中"));
    assert_eq!(group.group_bottleneck.as_deref(), Some("G-MEMBER-B"));
    assert_eq!(group.group_policy.as_deref(), Some("together"));
    // 对外主状态同步为派生聚合值（不再停留在创建时的静态状态）。
    assert_eq!(group.status, "开发中");
    // description 摘要里的静态 Status 行也同步为聚合状态。
    assert!(group.description.contains("- Status: 开发中"), "{}", group.description);
    // 成员反向引用所属组。
    let member = reqs
        .iter()
        .find(|r| r.id == "G-MEMBER-A")
        .expect("member a");
    assert!(member
        .member_of
        .iter()
        .any(|g| g == "WMS-GRP-001-aggregation"));
}


#[tokio::test]
async fn create_requirement_group_writes_group_json_and_defaults_policy() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let state = group_test_state(&tmp);
    create_requirement(&state, group_test_form("G-M1", "成员1"))
        .await
        .expect("create member");
    let mut form = group_test_form("WMS-GRP-002", "默认策略组");
    form.members = Some(vec![group_test_member_input("G-M1")]);
    let res = create_requirement(&state, form)
        .await
        .expect("create group");
    assert_eq!(res["group"]["releasePolicy"], json!("independent"));
    let req_dir = res["reqDir"].as_str().expect("reqDir").to_string();
    let raw = std::fs::read_to_string(std::path::Path::new(&req_dir).join(GROUP_FILE))
        .expect("group.json exists");
    let file: GroupFile = serde_json::from_str(&raw).expect("parse group.json");
    assert_eq!(file.release_policy.as_deref(), Some("independent"));
    assert_eq!(file.members.len(), 1);
    assert_eq!(file.members[0].req_id, "G-M1");
}


#[tokio::test]
async fn create_requirement_group_rejects_invalid_members_and_policy() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let state = group_test_state(&tmp);
    create_requirement(&state, group_test_form("G-M2", "成员2"))
        .await
        .expect("create member");

    // 成员不存在。
    let mut missing = group_test_form("WMS-GRP-003", "坏成员组");
    missing.members = Some(vec![group_test_member_input("G-NOPE")]);
    let err = create_requirement(&state, missing)
        .await
        .expect_err("missing member must fail");
    assert!(err.message.contains("需求组成员不存在"), "{:?}", err);

    // 自引用。
    let mut self_ref = group_test_form("WMS-GRP-004", "自引用组");
    self_ref.members = Some(vec![group_test_member_input("WMS-GRP-004")]);
    let err = create_requirement(&state, self_ref)
        .await
        .expect_err("self reference must fail");
    assert!(err.message.contains("自身"), "{:?}", err);

    // 非法 releasePolicy。
    let mut bad_policy = group_test_form("WMS-GRP-005", "坏策略组");
    bad_policy.members = Some(vec![group_test_member_input("G-M2")]);
    bad_policy.release_policy = Some("sometimes".to_string());
    let err = create_requirement(&state, bad_policy)
        .await
        .expect_err("bad policy must fail");
    assert!(err.message.contains("invalid releasePolicy"), "{:?}", err);

    // releasePolicy 未随 members 一起传。
    let mut lone_policy = group_test_form("WMS-GRP-006", "孤立策略");
    lone_policy.release_policy = Some("together".to_string());
    let err = create_requirement(&state, lone_policy)
        .await
        .expect_err("lone policy must fail");
    assert!(err.message.contains("releasePolicy"), "{:?}", err);

    // 组嵌套：成员自身是组。
    let mut inner = group_test_form("WMS-GRP-007", "内层组");
    inner.members = Some(vec![group_test_member_input("G-M2")]);
    create_requirement(&state, inner)
        .await
        .expect("create inner group");
    let mut outer = group_test_form("WMS-GRP-008", "外层组");
    outer.members = Some(vec![group_test_member_input("WMS-GRP-007")]);
    let err = create_requirement(&state, outer)
        .await
        .expect_err("nested group must fail");
    assert!(err.message.contains("组嵌套"), "{:?}", err);
}


#[tokio::test]
async fn validate_requirement_reports_group_json_problems_and_warnings() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let state = group_test_state(&tmp);
    create_requirement(&state, group_test_form("G-M3", "成员3"))
        .await
        .expect("create member");
    let mut form = group_test_form("WMS-GRP-009", "校验组");
    form.members = Some(vec![group_test_member_input("G-M3")]);
    let res = create_requirement(&state, form)
        .await
        .expect("create group");
    let req_dir = res["reqDir"].as_str().expect("reqDir").to_string();
    let dir = std::path::Path::new(&req_dir);

    // 合法 group.json -> ok。
    let req = get_real_requirement(&state, "WMS-GRP-009")
        .await
        .expect("req");
    let ok = validate_requirement(&state, &req).await.expect("validate");
    assert_eq!(ok["ok"], json!(true), "{}", ok);

    // 失效引用 -> warning（不阻塞）。
    std::fs::write(
        dir.join(GROUP_FILE),
        json!({"version": 1, "releasePolicy": "independent", "members": [{"reqId": "G-M3"}, {"reqId": "G-GONE"}]}).to_string(),
    )
    .expect("write stale group.json");
    let stale = validate_requirement(&state, &req)
        .await
        .expect("validate stale");
    assert_eq!(stale["ok"], json!(true));
    assert!(
        stale["warnings"]
            .as_array()
            .expect("warnings")
            .iter()
            .any(|w| w.as_str().unwrap_or("").contains("G-GONE")),
        "{}",
        stale
    );

    // 自引用 + 非法 policy -> problems。
    std::fs::write(
        dir.join(GROUP_FILE),
        json!({"version": 1, "releasePolicy": "maybe", "members": [{"reqId": "WMS-GRP-009"}]})
            .to_string(),
    )
    .expect("write bad group.json");
    let bad = validate_requirement(&state, &req)
        .await
        .expect("validate bad");
    assert_eq!(bad["ok"], json!(false));
    assert!(bad["problems"]
        .as_array()
        .expect("problems")
        .iter()
        .any(|p| p.as_str().unwrap_or("").contains("references itself")));
    assert!(bad["problems"]
        .as_array()
        .expect("problems")
        .iter()
        .any(|p| p.as_str().unwrap_or("").contains("releasePolicy")));

    // 坏 JSON -> problem。
    std::fs::write(dir.join(GROUP_FILE), "{ not json").expect("write broken group.json");
    let broken = validate_requirement(&state, &req)
        .await
        .expect("validate broken");
    assert_eq!(broken["ok"], json!(false));
    assert!(broken["problems"]
        .as_array()
        .expect("problems")
        .iter()
        .any(|p| p.as_str().unwrap_or("").contains("group.json")));
}


#[tokio::test]
async fn group_status_is_locked_against_manual_status_writes() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let state = group_test_state(&tmp);
    create_requirement(&state, group_test_form("G-M4", "成员4"))
        .await
        .expect("create member");
    let mut form = group_test_form("WMS-GRP-010", "只读状态组");
    form.members = Some(vec![group_test_member_input("G-M4")]);
    create_requirement(&state, form)
        .await
        .expect("create group");

    // PATCH / edit setStatus 路径被拦截。
    let err = update_requirement(
        &state,
        RequirementPatchForm {
            req_id: "WMS-GRP-010".to_string(),
            title: None,
            project: None,
            projects: None,
            status: Some("已完成".to_string()),
            category: None,
            source: None,
            owner: None,
            start_date: None,
            plan_release: None,
            ones: None,
            issues: None,
            note: None,
            dry_run: None,
        },
    )
    .await
    .expect_err("group status must be locked");
    assert!(err.message.contains("派生值"), "{:?}", err);

    // 成员不受影响，可以正常推进（组聚合状态随成员变化）。
    let member = get_real_requirement(&state, "G-M4").await.expect("member");
    write_requirement_status(
        member.req_dir.as_deref().unwrap_or_default(),
        "自测中",
        None,
    )
    .await
    .expect("member status ok");
    let reqs = list_requirements(&state).await.expect("list");
    let group = reqs.iter().find(|r| r.id == "WMS-GRP-010").expect("group");
    assert_eq!(group.group_status.as_deref(), Some("自测中"));
    // 对外主状态随成员推进同步为派生值。
    assert_eq!(group.status, "自测中");
}

