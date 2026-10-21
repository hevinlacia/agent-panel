use super::*;
use crate::http::FormOrJson;
use std::process::Command as StdCommand;

fn delivery_test_state(tmp: &tempfile::TempDir) -> AppState {
    std::fs::create_dir_all(tmp.path().join(".git")).expect("create .git for relDir");
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

/// 同步 git 命令执行器（测试专用；与 crate::git_workflow::git 同名遮蔽无碍，本地定义优先）。
#[allow(dead_code)]
fn git_out(dir: &std::path::Path, args: &[&str]) -> String {
    let out = StdCommand::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "test")
        .env("GIT_AUTHOR_EMAIL", "test@localhost")
        .env("GIT_COMMITTER_NAME", "test")
        .env("GIT_COMMITTER_EMAIL", "test@localhost")
        .output()
        .expect("git run");
    assert!(
        out.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&out.stderr)
    );
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

/// dir-bundle 端点：返回 relDir + 全部文本文件内容（排除 attachments/）。
#[tokio::test]
async fn dir_bundle_returns_rel_dir_and_files() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let state = delivery_test_state(&tmp);
    let created = create_requirement(&state, delivery_form("REQ-933"))
        .await
        .expect("create");
    let rid = created["reqId"].as_str().unwrap().to_string();
    let dir = PathBuf::from(created["reqDir"].as_str().unwrap());
    std::fs::create_dir_all(dir.join("attachments")).expect("mkdir attachments");
    std::fs::write(dir.join("attachments/x.sql"), "SELECT 1;").expect("write attachment");
    std::fs::write(dir.join("branches.json"), r#"{"version":2,"repos":[]}"#).expect("write scope");

    let query: IdQuery = serde_json::from_value(json!({ "id": rid })).expect("parse IdQuery");
    let resp = api_requirement_dir_bundle_get(State(state), axum::extract::Query(query))
        .await
        .expect("dir bundle");
    let rel_dir = resp["relDir"].as_str().expect("relDir");
    assert!(!rel_dir.starts_with('/'));
    let files = resp["files"].as_array().expect("files");
    let paths: Vec<&str> = files.iter().filter_map(|f| f["path"].as_str()).collect();
    assert!(paths.contains(&"branches.json"), "files: {paths:?}");
    assert!(
        paths.iter().all(|p| !p.starts_with("attachments/")),
        "attachments excluded: {paths:?}"
    );
}

/// worker 物化：mock hub 返回 dir-bundle → 本地数据仓物化需求目录 → 可被解析。
#[tokio::test]
async fn fetch_requirement_bundle_materializes_local_dir() {
    use crate::git_workflow::fetch_requirement_bundle_from_hub;

    let tmp = tempfile::tempdir().expect("tempdir");
    // 数据仓：带 agent-panel-data remote（worker_data_repo_root 识别条件）
    let data = tmp.path().join("data");
    std::fs::create_dir_all(&data).expect("mkdir data");
    git_out(&data, &["init", "-q", "-b", "main"]);
    git_out(
        &data,
        &[
            "remote",
            "add",
            "origin",
            "git@github.com:hevinlacia/agent-panel-data.git",
        ],
    );
    // 扫描根 = 数据仓（可解析 .agents/req）
    let config = json!({ "requirementScanRoots": [data.to_string_lossy()] });
    std::fs::write(data.join("config.json"), config.to_string()).expect("write config");
    let state = temp_app_state(
        &data,
        &tmp.path().join("pi-sessions"),
        &tmp.path().join("dsh-sessions"),
    );

    // 前置：本地解析不到
    assert!(get_real_requirement(&state, "INC-940").await.is_err());

    // mock hub 返回 dir-bundle
    let app = axum::Router::new().route(
        "/api/requirement/dir-bundle",
        axum::routing::get(|| async {
            axum::Json(json!({
                "ok": true,
                "relDir": ".agents/req/INC-940-demo",
                "files": [
                    {"path": "meta.md", "content": "---\nreq-id: INC-940-demo\n---\n# t"},
                    {"path": "state.json", "content": "{\"status\":\"排查中\"}"},
                    {"path": "branches.json", "content": "{\"version\":2,\"repos\":[]}"}
                ]
            }))
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    tokio::spawn(async move { axum::serve(listener, app).await.expect("serve") });
    tokio::time::sleep(std::time::Duration::from_millis(80)).await;
    std::env::set_var("PANEL_HUB_URL", format!("http://{addr}"));
    let result = fetch_requirement_bundle_from_hub(&state, "INC-940-demo").await;
    std::env::remove_var("PANEL_HUB_URL");
    let target = result.expect("materialize");
    assert!(
        target.join("branches.json").exists(),
        "branches.json materialized"
    );
    // 物化后可解析
    let req = get_real_requirement(&state, "INC-940-demo")
        .await
        .expect("resolve after materialize");
    assert_eq!(req.id, "INC-940-demo");
}
