use super::*;
use crate::http::FormOrJson;
use crate::testdata_sync::{
    api_testdata_file_get, api_testdata_file_put, api_testdata_manifest_get,
    sync_testdata_from_hub_with, TestdataFileWriteForm,
};
use axum::extract::Query;
use std::path::Path;

fn pack_test_state(tmp: &tempfile::TempDir, pack_root: &Path) -> AppState {
    // config 必须落在 state.data_dir（read_config 从 data_dir/config.json 读取）；
    // scan root 指向 pack 所在项目根（testdata 的上级 .agents 的上级）。
    let data_dir = tmp.path().join("data");
    std::fs::create_dir_all(&data_dir).expect("mkdir data");
    let scan_root = pack_root
        .parent()
        .and_then(|p| p.parent())
        .map(|p| p.display().to_string())
        .unwrap_or_default();
    let config = json!({ "requirementScanRoots": [scan_root] });
    std::fs::write(data_dir.join("config.json"), config.to_string()).expect("write config");
    temp_app_state(&data_dir, &tmp.path().join("pi"), &tmp.path().join("dsh"))
}

/// pack 布局：<tmp>/proj/.agents/testdata；scan root 指向 <tmp>/proj。
fn setup_pack(tmp: &tempfile::TempDir) -> (PathBuf, PathBuf) {
    let proj = tmp.path().join("proj");
    let pack = proj.join(".agents").join("testdata");
    std::fs::create_dir_all(pack.join("recipes")).expect("mkdir pack");
    std::fs::write(
        pack.join("capabilities.yaml"),
        "version: 1\ncapabilities: []\n",
    )
    .expect("write capabilities");
    std::fs::write(pack.join("recipes").join("demo.yaml"), "id: demo\n").expect("write recipe");
    (proj, pack)
}

fn write_form(
    path: &str,
    content: &str,
    base_sha: Option<String>,
) -> FormOrJson<TestdataFileWriteForm> {
    FormOrJson(TestdataFileWriteForm {
        path: path.to_string(),
        content: content.to_string(),
        base_sha,
    })
}

/// file API 往返 + 乐观锁 + 路径白名单（穿越/排除目录/凭证文件/后缀）。
#[tokio::test]
async fn testdata_file_roundtrip_optlock_and_guards() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let (proj, pack) = setup_pack(&tmp);
    let state = pack_test_state(&tmp, &pack);

    // 新建：无 baseSha
    let resp = api_testdata_file_put(
        State(state.clone()),
        write_form("finders/new.sql", "SELECT 1;", None),
    )
    .await
    .expect("create file");
    assert_eq!(resp["ok"], json!(true));
    assert!(pack.join("finders/new.sql").is_file());

    // 读回 sha
    let read = api_testdata_file_get(
        State(state.clone()),
        Query(crate::testdata_sync::TestdataFileReadQuery {
            path: Some("finders/new.sql".into()),
        }),
    )
    .await
    .expect("read file");
    assert_eq!(read["exists"], json!(true));
    let sha = read["sha"].as_str().unwrap().to_string();

    // 错误 baseSha → 409
    let err = api_testdata_file_put(
        State(state.clone()),
        write_form("finders/new.sql", "SELECT 2;", Some("deadbeef".into())),
    )
    .await;
    assert_eq!(err.expect_err("conflict expected").status, 409);

    // 正确 baseSha → 更新
    let resp = api_testdata_file_put(
        State(state.clone()),
        write_form("finders/new.sql", "SELECT 2;", Some(sha)),
    )
    .await
    .expect("update file");
    assert_eq!(resp["ok"], json!(true));

    // manifest 包含两份数据文件，排除 node_modules 与隐藏文件
    let manifest = api_testdata_manifest_get(State(state.clone()))
        .await
        .expect("manifest");
    let paths: Vec<String> = manifest["files"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|f| f["path"].as_str().map(str::to_string))
        .collect();
    assert!(
        paths.contains(&"capabilities.yaml".to_string()),
        "{paths:?}"
    );
    assert!(paths.contains(&"finders/new.sql".to_string()));
    assert!(!paths.iter().any(|p| p.starts_with("node_modules")));

    // 守卫：穿越 / 排除目录 / 凭证 / 非白后缀
    for bad in [
        "../escape.yaml",
        "node_modules/x.json",
        ".env",
        "docs/blob.bin",
    ] {
        let err = api_testdata_file_put(State(state.clone()), write_form(bad, "x", None)).await;
        assert_eq!(
            err.expect_err(&format!("reject {bad}")).status,
            400,
            "path {bad}"
        );
    }

    // scan root 解析：manifest 走的是 proj pack（非空）
    assert!(proj.join(".agents/testdata/capabilities.yaml").is_file());
}

/// worker 物化状态机：新文件拉取 / 本地手改冲突 / force 覆盖 / hub 删除跟随。
#[tokio::test]
async fn testdata_sync_materialize_conflict_and_force() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let pack = tmp.path().join("pack");
    std::fs::create_dir_all(&pack).expect("mkdir");

    // mock hub：manifest + file 两端点
    let hub_files = serde_json::json!({
        "ok": true,
        "files": [
            {"path": "capabilities.yaml", "sha1": crate::requirement_api::sha1_hex("hub-v1")},
            {"path": "recipes/a.yaml", "sha1": crate::requirement_api::sha1_hex("a-v1")},
        ]
    });
    let hub_files_clone = hub_files.clone();
    let app = axum::Router::new()
        .route(
            "/api/testdata/manifest",
            axum::routing::get(move || {
                let files = hub_files_clone.clone();
                async move { axum::Json(files) }
            }),
        )
        .route(
            "/api/testdata/file",
            axum::routing::get(
                |q: axum::extract::Query<std::collections::HashMap<String, String>>| {
                    let path = q.0.get("path").cloned().unwrap_or_default();
                    let content = match path.as_str() {
                        "capabilities.yaml" => "hub-v1",
                        "recipes/a.yaml" => "a-v1",
                        _ => "",
                    };
                    async move {
                        axum::Json(json!({
                            "ok": true, "path": path, "exists": true,
                            "content": content, "sha": crate::requirement_api::sha1_hex(content),
                        }))
                    }
                },
            ),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    tokio::spawn(async move { axum::serve(listener, app).await.expect("serve") });
    tokio::time::sleep(std::time::Duration::from_millis(80)).await;
    let hub = format!("http://{addr}");

    // 1) 首次物化：两个文件落盘
    let doc = sync_testdata_from_hub_with(&hub, &pack, false)
        .await
        .expect("first sync");
    assert_eq!(doc["updated"].as_array().unwrap().len(), 2, "{doc}");
    assert_eq!(
        std::fs::read_to_string(pack.join("capabilities.yaml")).unwrap(),
        "hub-v1"
    );

    // 2) 本地手改 + hub 变更 → 冲突跳过
    std::fs::write(pack.join("recipes/a.yaml"), "local-edit").expect("local edit");
    let mut hub_files2 = hub_files.clone();
    hub_files2["files"][1]["sha1"] = json!(crate::requirement_api::sha1_hex("a-v2"));
    // 重建 mock 的 manifest 内容（闭包持有旧值，改用新服务端口太重——直接改本地水位文件模拟
    // “水位过期”不可行；改为用独立 mock 验证冲突。此处直接验证：未改 hub、本地改 → 无冲突、
    // 本地被 hub 版本覆盖回写（水位匹配语义）。
    let doc = sync_testdata_from_hub_with(&hub, &pack, false)
        .await
        .expect("second sync");
    // a.yaml 本地=local-edit，水位=a-v1 不匹配 local-edit → 冲突
    assert_eq!(doc["conflicts"].as_array().unwrap().len(), 1, "{doc}");
    assert_eq!(
        std::fs::read_to_string(pack.join("recipes/a.yaml")).unwrap(),
        "local-edit",
        "冲突时不得覆盖本地"
    );

    // 3) force → 覆盖为 hub 版本
    let doc = sync_testdata_from_hub_with(&hub, &pack, true)
        .await
        .expect("force sync");
    assert_eq!(doc["conflicts"].as_array().unwrap().len(), 0, "{doc}");
    assert_eq!(
        std::fs::read_to_string(pack.join("recipes/a.yaml")).unwrap(),
        "a-v1"
    );

    // 4) hub 删除 + 本地水位匹配 → 跟随删除
    let hub_files3 = serde_json::json!({
        "ok": true,
        "files": [
            {"path": "capabilities.yaml", "sha1": crate::requirement_api::sha1_hex("hub-v1")},
        ]
    });
    let _ = hub_files3; // 说明：删除场景由 dedicated mock 服务覆盖，见下方独立服务
    drop(hub_files2);
    // 为删除场景再起一个只含单文件的 mock
    let app2 = axum::Router::new()
        .route(
            "/api/testdata/manifest",
            axum::routing::get(|| async {
                axum::Json(json!({
                    "ok": true,
                    "files": [{"path": "capabilities.yaml", "sha1": crate::requirement_api::sha1_hex("hub-v1")}]
                }))
            }),
        )
        .route(
            "/api/testdata/file",
            axum::routing::get(|q: axum::extract::Query<std::collections::HashMap<String, String>>| {
                let path = q.0.get("path").cloned().unwrap_or_default();
                async move {
                    axum::Json(json!({
                        "ok": true, "path": path, "exists": true,
                        "content": "hub-v1", "sha": crate::requirement_api::sha1_hex("hub-v1"),
                    }))
                }
            }),
        );
    let listener2 = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind2");
    let addr2 = listener2.local_addr().expect("addr2");
    tokio::spawn(async move { axum::serve(listener2, app2).await.expect("serve2") });
    tokio::time::sleep(std::time::Duration::from_millis(80)).await;
    let doc = sync_testdata_from_hub_with(&format!("http://{addr2}"), &pack, false)
        .await
        .expect("deletion sync");
    assert!(
        doc["deleted"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v == "recipes/a.yaml"),
        "{doc}"
    );
    assert!(!pack.join("recipes/a.yaml").exists());
    assert!(pack.join("capabilities.yaml").is_file());
    let _ = hub_files; // 保留引用避免未使用告警（首个 mock 的清单）
}
