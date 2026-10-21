use super::*;
use crate::http::FormOrJson;
use std::process::Command as StdCommand;

fn delivery_test_state(tmp: &tempfile::TempDir) -> AppState {
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

fn delivery_form(req_id: &str) -> RequirementCreateForm {
    RequirementCreateForm {
        req_id: req_id.to_string(),
        title: "快照直传测试".to_string(),
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
    }
}

/// 直传端点：full 快照落盘 code-review.json，随行 incremental 落盘增量包。
#[tokio::test]
async fn review_snapshot_endpoint_persists_full_and_incremental() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let state = delivery_test_state(&tmp);
    let created = create_requirement(&state, delivery_form("REQ-931"))
        .await
        .expect("create");
    let rid = created["reqId"].as_str().unwrap().to_string();

    let resp = api_requirement_review_snapshot_post(
        State(state.clone()),
        FormOrJson(ReviewSnapshotDeliveryForm {
            req_id: rid.clone(),
            kind: Some("full".to_string()),
            content: json!({"repos": [{"repoName": "demo", "targetCommit": "abc"}]}),
            incremental: Some(json!({
                "kind": "incremental",
                "content": {"mode": "incremental", "drifts": []}
            })),
        }),
    )
    .await
    .expect("deliver snapshot");
    assert_eq!(resp["ok"], json!(true));
    assert_eq!(resp["incrementalWritten"], json!(true));

    let dir = PathBuf::from(created["reqDir"].as_str().unwrap());
    let full = read_json_if_exists(&dir.join("code-review.json"))
        .await
        .expect("full snapshot persisted");
    assert_eq!(full["repos"][0]["targetCommit"], json!("abc"));
    let inc = read_json_if_exists(&dir.join("code-review-incremental.json"))
        .await
        .expect("incremental persisted");
    assert_eq!(inc["mode"], json!("incremental"));
}

/// worker fallback：branches.json 缺失时从 hub API 拉登记写本地（活链路不依赖 git）。
#[tokio::test]
async fn fetch_branch_scope_from_hub_writes_local_copy() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let state = delivery_test_state(&tmp);
    let created = create_requirement(&state, delivery_form("REQ-932"))
        .await
        .expect("create");
    let rid = created["reqId"].as_str().unwrap().to_string();
    let dir = PathBuf::from(created["reqDir"].as_str().unwrap());
    assert!(
        !dir.join("branches.json").exists(),
        "precondition: no local scope"
    );

    // 内嵌 mock hub：返回 branch-registration 响应
    let app = axum::Router::new().route(
        "/api/requirement/branch-registration",
        axum::routing::get(|| async {
            axum::Json(json!({
                "ok": true,
                "scope": {
                    "version": 2,
                    "repos": [{"repoName": "yl-cwhsea-wms-outbound-api", "branch": "hevin.yang/x", "role": "后端"}]
                }
            }))
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    tokio::spawn(async move { axum::serve(listener, app).await.expect("serve") });
    tokio::time::sleep(std::time::Duration::from_millis(80)).await;

    // PANEL_HUB_URL 指向 mock（测试进程内串行设置，无其他测试消费该变量）
    std::env::set_var("PANEL_HUB_URL", format!("http://{addr}"));
    let ok = fetch_branch_scope_from_hub(&dir, &rid, 1).await;
    std::env::remove_var("PANEL_HUB_URL");
    assert!(ok, "hub fallback fetch should succeed");

    let scope = read_json_if_exists(&dir.join("branches.json"))
        .await
        .expect("branches.json written by fallback");
    assert_eq!(
        scope["repos"][0]["repoName"],
        json!("yl-cwhsea-wms-outbound-api")
    );
    // git 仓库外写文件即可，无额外断言；防止 unused 警告
    let _ = StdCommand::new("true").output();
}
