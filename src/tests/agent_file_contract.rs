use super::*;

fn contract_test_state(tmp: &tempfile::TempDir) -> AppState {
    // 预置 .git 目录：验证 relDir 向上找仓库根的逻辑
    std::fs::create_dir_all(tmp.path().join(".git")).expect("create .git");
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

fn contract_form(req_id: &str, title: &str, source: Option<&str>) -> RequirementCreateForm {
    let mut form = RequirementCreateForm {
        req_id: req_id.to_string(),
        title: title.to_string(),
        project: None,
        projects: None,
        group_path: None,
        parent_req_id: None,
        root: None,
        status: None,
        category: None,
        source: source.map(str::to_string),
        owner: None,
        start_date: None,
        plan_release: None,
        submit_test_date: None,
        ones: None,
        issues: None,
        members: None,
        release_policy: None,
        consolidated: None,
        summary: None,
        background: None,
        notes: None,
        dry_run: None,
        region: None,
    };
    form
}

/// #9：创建响应带 sourceDefaulted（缺省「产品推动」时标记 true）；
/// #7：relDir 为仓库相对路径（reqDir 仍是容器内绝对路径）。
#[tokio::test]
async fn create_response_marks_source_default_and_rel_dir() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let state = contract_test_state(&tmp);
    let res = create_requirement(&state, contract_form("REQ-901", "默认 source", None))
        .await
        .expect("create default-source req");
    assert_eq!(res["sourceDefaulted"], json!(true));
    let rel = res["relDir"].as_str().expect("relDir present");
    assert!(!rel.starts_with('/'), "relDir must be repo-relative: {rel}");
    assert!(
        rel.contains(".agents/req"),
        "relDir contains req tail: {rel}"
    );
    assert!(res["reqDir"].as_str().unwrap().starts_with('/'));

    let res2 = create_requirement(
        &state,
        contract_form("REQ-902", "显式 source", Some("开发推动")),
    )
    .await
    .expect("create explicit-source req");
    assert_eq!(res2["sourceDefaulted"], json!(false));
}

/// #2：GET 不存在文件 sha 为 null（不诱导把空内容 sha 当乐观锁版本）；
/// 旧 GET 形状的空内容 sha 传回 PUT 时自动降级为新建。
#[tokio::test]
async fn agent_file_missing_file_null_sha_and_empty_marker_degrades_to_create() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let state = contract_test_state(&tmp);
    let res = create_requirement(&state, contract_form("REQ-911", "file host", None))
        .await
        .expect("create req");
    let rid = res["reqId"].as_str().unwrap().to_string();

    let get = api_requirement_file_get(
        State(state.clone()),
        Query(AgentFileReadQuery {
            id: Some(rid.clone()),
            req_id: None,
            path: Some("manual-check.md".to_string()),
        }),
    )
    .await
    .expect("get missing file");
    assert_eq!(get["exists"], json!(false));
    assert!(get["sha"].is_null(), "missing file sha must be null");
    assert!(get["baseSha"].is_null());

    // 旧客户端行为：把 GET 给的（曾经的）空内容 sha 原样传回 → 应降级为新建而非 409
    let put = api_requirement_file_put(
        State(state.clone()),
        FormOrJson(AgentFileWriteForm {
            id: Some(rid.clone()),
            req_id: None,
            path: "manual-check.md".to_string(),
            content: "# 手工核查\n".to_string(),
            base_sha: Some(sha1_hex("")),
        }),
    )
    .await
    .expect("put with empty-marker sha should create");
    assert_eq!(put["created"], json!(true));

    // 真冲突仍拦截：基于旧 sha 更新已被修改的文件 → 409
    let stale = AgentFileWriteForm {
        id: Some(rid),
        req_id: None,
        path: "manual-check.md".to_string(),
        content: "stale".to_string(),
        base_sha: Some(sha1_hex("")),
    };
    let err = api_requirement_file_put(State(state), FormOrJson(stale))
        .await
        .expect_err("stale baseSha must conflict");
    assert_eq!(err.status, axum::http::StatusCode::CONFLICT);
}

/// #1：PUT 表单兼容 GET 返回的字段名 `sha`（camelCase baseSha 仍为主名）。
#[test]
fn agent_file_write_form_accepts_sha_alias() {
    let f: AgentFileWriteForm = serde_json::from_value(json!({
        "id": "REQ-1", "path": "notes.md", "content": "c", "sha": "abc123"
    }))
    .expect("sha alias accepted");
    assert_eq!(f.base_sha.as_deref(), Some("abc123"));
    let f2: AgentFileWriteForm = serde_json::from_value(json!({
        "id": "REQ-1", "path": "notes.md", "content": "c", "baseSha": "def456"
    }))
    .expect("baseSha still primary");
    assert_eq!(f2.base_sha.as_deref(), Some("def456"));
}

/// #8：issue 家族 validate 不再误报 background/technical-plan；进入已定位后要求 root-cause.md。
#[tokio::test]
async fn validate_issue_family_core_files_match_templates() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let state = contract_test_state(&tmp);
    let mut form = contract_form("WMS-INC-920", "issue 家族 validate", None);
    form.category = Some("线上问题".to_string());
    let res = create_requirement(&state, form)
        .await
        .expect("create issue");
    let warnings = res["validation"]["warnings"]
        .as_array()
        .expect("validation warnings array");
    assert!(
        !warnings
            .iter()
            .any(|w| w.as_str().unwrap_or("").contains("background.md")),
        "issue family must not warn missing background.md: {warnings:?}"
    );
    assert!(
        !warnings
            .iter()
            .any(|w| w.as_str().unwrap_or("").contains("technical-plan.md")),
        "issue family must not warn missing technical-plan.md: {warnings:?}"
    );

    // 普通需求删掉 background.md → 警告仍在（不放宽普通需求校验）
    let res2 = create_requirement(&state, contract_form("REQ-921", "normal req", None))
        .await
        .expect("create normal");
    let dir = std::path::PathBuf::from(res2["reqDir"].as_str().unwrap());
    std::fs::remove_file(dir.join("background.md")).expect("remove background");
    let req = load_requirement_from_dir(&dir, "REQ-921", &[], &[])
        .await
        .expect("load")
        .expect("req exists");
    let v = validate_requirement(&state, &req).await.expect("validate");
    assert!(v["warnings"]
        .as_array()
        .unwrap()
        .iter()
        .any(|w| w.as_str().unwrap_or("").contains("background.md")));

    // issue 家族推进到已定位（无 root-cause.md）→ 新增针对性警告；补齐后消失
    let issue_dir = std::path::PathBuf::from(res["reqDir"].as_str().unwrap());
    let state_path = issue_dir.join("state.json");
    let mut st: Value =
        serde_json::from_str(&std::fs::read_to_string(&state_path).expect("read state")).unwrap();
    st["status"] = json!("已定位");
    std::fs::write(&state_path, serde_json::to_string(&st).unwrap()).expect("write state");
    let issue_req = load_requirement_from_dir(&issue_dir, res["reqId"].as_str().unwrap(), &[], &[])
        .await
        .expect("load issue")
        .expect("issue exists");
    let v2 = validate_requirement(&state, &issue_req)
        .await
        .expect("validate issue");
    assert!(v2["warnings"]
        .as_array()
        .unwrap()
        .iter()
        .any(|w| w.as_str().unwrap_or("").contains("root-cause.md")));
    std::fs::write(issue_dir.join("root-cause.md"), "# 根因\n").expect("write root-cause");
    let v3 = validate_requirement(&state, &issue_req)
        .await
        .expect("validate issue again");
    assert!(!v3["warnings"]
        .as_array()
        .unwrap()
        .iter()
        .any(|w| w.as_str().unwrap_or("").contains("root-cause.md")));
}
