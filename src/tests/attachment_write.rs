use super::agent_file_contract::{contract_form, contract_test_state};
use super::*;

#[test]
fn attachment_path_allows_single_layer_and_bare_name() {
    // 带 attachments/ 前缀与裸文件名两种写法都归一化为 attachments/<file>
    assert_eq!(
        resolve_attachment_rel_path("attachments/01-fix.sql").unwrap(),
        "attachments/01-fix.sql"
    );
    assert_eq!(
        resolve_attachment_rel_path("01-fix.sql").unwrap(),
        "attachments/01-fix.sql"
    );
    assert_eq!(
        resolve_attachment_rel_path("release-config.md").unwrap(),
        "attachments/release-config.md"
    );
    assert_eq!(
        resolve_attachment_rel_path("attachments/vars.yaml").unwrap(),
        "attachments/vars.yaml"
    );
}

#[test]
fn attachment_path_rejects_traversal_dirs_and_bad_exts() {
    assert!(resolve_attachment_rel_path("../escape.sql").is_err());
    assert!(resolve_attachment_rel_path("/abs/x.sql").is_err());
    assert!(resolve_attachment_rel_path("attachments/../../x.sql").is_err());
    assert!(resolve_attachment_rel_path("attachments/sub/x.sql").is_err());
    assert!(resolve_attachment_rel_path("sub/x.sql").is_err());
    assert!(resolve_attachment_rel_path("").is_err());
    assert!(resolve_attachment_rel_path("x.sh").is_err());
    assert!(resolve_attachment_rel_path("x.exe").is_err());
    assert!(resolve_attachment_rel_path("noext").is_err());
}

/// 附件写主链路：新建（无 baseSha）→ GET 拿 sha → 更新 → 列表接口可见。
#[tokio::test]
async fn attachment_put_get_roundtrip_and_listing() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let state = contract_test_state(&tmp);
    let res = create_requirement(&state, contract_form("REQ-930", "attachment host", None))
        .await
        .expect("create req");
    let rid = res["reqId"].as_str().unwrap().to_string();

    // 新建：裸文件名省略 baseSha
    let put = api_attachment_put(
        State(state.clone()),
        FormOrJson(AgentFileWriteForm {
            id: Some(rid.clone()),
            req_id: None,
            path: "01-fix-inventory.sql".to_string(),
            content: "-- 用途: test\nUPDATE t SET a = 1;\n".to_string(),
            base_sha: None,
        }),
    )
    .await
    .expect("create attachment");
    assert_eq!(put["created"], json!(true));
    assert_eq!(put["path"], json!("attachments/01-fix-inventory.sql"));

    // GET：拿到内容与 baseSha
    let get = api_attachment_get(
        State(state.clone()),
        Query(AgentFileReadQuery {
            id: Some(rid.clone()),
            req_id: None,
            path: Some("attachments/01-fix-inventory.sql".to_string()),
        }),
    )
    .await
    .expect("get attachment");
    assert_eq!(get["exists"], json!(true));
    let sha = get["baseSha"]
        .as_str()
        .expect("baseSha present")
        .to_string();
    assert_eq!(
        get["content"],
        json!("-- 用途: test\nUPDATE t SET a = 1;\n")
    );

    // 更新：带正确 baseSha
    let put2 = api_attachment_put(
        State(state.clone()),
        FormOrJson(AgentFileWriteForm {
            id: Some(rid.clone()),
            req_id: None,
            path: "attachments/01-fix-inventory.sql".to_string(),
            content: "-- 用途: test v2\nUPDATE t SET a = 2;\n".to_string(),
            base_sha: Some(sha),
        }),
    )
    .await
    .expect("update attachment");
    assert_eq!(put2["created"], json!(false));

    // 列表接口可见新附件
    let list = api_attachments(
        State(state.clone()),
        Query(IdQuery {
            id: Some(rid.clone()),
            ..Default::default()
        }),
    )
    .await
    .expect("list attachments");
    let rows = list["attachments"].as_array().expect("rows");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["filename"], json!("01-fix-inventory.sql"));
}

/// 乐观锁语义与 file API 一致：已存在无 baseSha 409、旧 sha 409、空标记 sha 降级新建。
#[tokio::test]
async fn attachment_put_optimistic_lock_semantics() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let state = contract_test_state(&tmp);
    let res = create_requirement(&state, contract_form("REQ-931", "attachment lock", None))
        .await
        .expect("create req");
    let rid = res["reqId"].as_str().unwrap().to_string();
    let form = |content: &str, base: Option<String>, path: &str| AgentFileWriteForm {
        id: Some(rid.clone()),
        req_id: None,
        path: path.to_string(),
        content: content.to_string(),
        base_sha: base,
    };

    // 已存在 + 无 baseSha → 409（防误覆盖）
    api_attachment_put(
        State(state.clone()),
        FormOrJson(form("v1", None, "release-config.md")),
    )
    .await
    .expect("create");
    let err = api_attachment_put(
        State(state.clone()),
        FormOrJson(form("overwrite", None, "release-config.md")),
    )
    .await
    .expect_err("missing baseSha must conflict");
    assert_eq!(err.status, axum::http::StatusCode::CONFLICT);

    // 旧 sha → 409
    let stale = api_attachment_put(
        State(state.clone()),
        FormOrJson(form("stale", Some(sha1_hex("old")), "release-config.md")),
    )
    .await
    .expect_err("stale sha must conflict");
    assert_eq!(stale.status, axum::http::StatusCode::CONFLICT);

    // 空内容标记 sha 传回 → 降级新建而非 409
    let put = api_attachment_put(
        State(state.clone()),
        FormOrJson(form("-- new\n", Some(sha1_hex("")), "02-new.sql")),
    )
    .await
    .expect("empty-marker sha degrades to create");
    assert_eq!(put["created"], json!(true));
}

/// 白名单后缀拒绝：.sh 附件被拒，落盘文件不出现。
#[tokio::test]
async fn attachment_put_rejects_non_whitelisted_ext() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let state = contract_test_state(&tmp);
    let res = create_requirement(&state, contract_form("REQ-932", "attachment ext", None))
        .await
        .expect("create req");
    let rid = res["reqId"].as_str().unwrap().to_string();
    let err = api_attachment_put(
        State(state.clone()),
        FormOrJson(AgentFileWriteForm {
            id: Some(rid),
            req_id: None,
            path: "run.sh".to_string(),
            content: "rm -rf /".to_string(),
            base_sha: None,
        }),
    )
    .await
    .expect_err("non-whitelisted ext must be rejected");
    assert_eq!(err.status, axum::http::StatusCode::BAD_REQUEST);
}
