use super::*;

#[test]
fn pi_session_file_exists_matches_filename_suffix() {
    let root = tempfile::tempdir().expect("tempdir");
    let id = "019f6a66-9601-74ea-bbaf-7640861b1959";
    // 未创建任何 session 文件时视为未使用。
    assert!(!pi_session_file_exists(root.path(), id));
    // pi 命名：<timestamp>_<sessionId>.jsonl，位于编码 cwd 子目录下。
    let session_file = root
        .path()
        .join("--home-hevin-Developer--")
        .join(format!("2026-07-16T10-08-55-809Z_{id}.jsonl"));
    write_file(&session_file, "{\"type\":\"session\"}\n");
    assert!(pi_session_file_exists(root.path(), id));
    // 其他 session id 不命中。
    assert!(!pi_session_file_exists(
        root.path(),
        "00000000-0000-0000-0000-000000000000"
    ));
}


#[test]
fn dsh_session_file_exists_matches_workspace_dir_name() {
    let root = tempfile::tempdir().expect("tempdir");
    let id = "8b1c2a34-1111-2222-3333-444455556666";
    assert!(!dsh_session_file_exists(root.path(), id));
    // DSH 命名：<workspace>/<sessionId>/session.jsonl.zstd。
    let session_file = root
        .path()
        .join("--home-hevin-Developer--")
        .join(id)
        .join("session.jsonl.zstd");
    write_file(&session_file, "placeholder");
    assert!(dsh_session_file_exists(root.path(), id));
    // 未压缩的 session.jsonl 也算。
    let other = "aaaaaaaa-bbbb-cccc-dddd-eeeeffff0000";
    let plain = root
        .path()
        .join("--tmp--")
        .join(other)
        .join("session.jsonl");
    write_file(&plain, "placeholder");
    assert!(dsh_session_file_exists(root.path(), other));
}


#[test]
fn associations_store_defaults_pending_commands_for_legacy_files() {
    // 旧版 associations.json 没有 pendingCommands 字段时必须能加载为空 map。
    let legacy = "{\"version\":2,\"associations\":{\"WMS-001\":[\"sid-1\"]}}";
    let store: AssociationsStore = serde_json::from_str(legacy).expect("parse legacy store");
    assert!(store.pending_commands.is_empty());
    assert_eq!(store.associations.get("WMS-001").map(Vec::len), Some(1));

    // 含 pendingCommands 的文件能完整往返。
    let mut store = AssociationsStore::default();
    store.pending_commands.insert(
        "WMS-001".to_string(),
        PendingSessionCommand {
            session_id: "sid-2".to_string(),
            command: "pi --session-id sid-2".to_string(),
            harness: "pi".to_string(),
            context_path: "/tmp/ctx/sid-2.md".to_string(),
            created_at: 1_720_000_000_000,
        },
    );
    let raw = serde_json::to_string(&store).expect("serialize store");
    let round: AssociationsStore = serde_json::from_str(&raw).expect("parse store");
    let pending = round.pending_commands.get("WMS-001").expect("pending kept");
    assert_eq!(pending.session_id, "sid-2");
    assert_eq!(pending.harness, "pi");
    assert_eq!(pending.created_at, 1_720_000_000_000);
}


/// new-session 命令生命周期端到端（进程内直调 handler，全部落盘在临时目录）：
/// 1) 首次复制生成 session id；2) 重复复制复用同一 id；3) session 被使用后自动换新；
/// 4) force 强制刷新并清理未使用的旧 pending；5) GET pending-session 只读视图。
#[tokio::test]
async fn new_session_reuses_pending_until_used_then_refreshes() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let data = tmp.path().join("data");
    let pi_root = tmp.path().join("pi-sessions");
    let dsh_root = tmp.path().join("dsh-sessions");
    std::fs::create_dir_all(&data).expect("create data dir");
    let proj = tmp.path().join("proj");
    let req_dir = proj.join("req").join("T-001");
    std::fs::create_dir_all(&req_dir).expect("create req dir");
    std::fs::write(
        req_dir.join("meta.md"),
        "---\nreq-id: T-001\ntitle: 会话命令测试\nstatus: 开发中\n---\n正文\n",
    )
    .expect("write meta.md");
    let config = json!({ "requirementScanRoots": [proj.to_string_lossy()] });
    std::fs::write(data.join("config.json"), config.to_string()).expect("write config.json");
    let state = temp_app_state(&data, &pi_root, &dsh_root);
    let form = |force: Option<bool>| {
        FormOrJson(NewSessionForm {
            req_id: "T-001".into(),
            force,
        })
    };

    // 1) 首次复制：生成新 session id，写入关联 + ctx + pending。
    let res1 = api_requirement_new_session(State(state.clone()), form(None))
        .await
        .expect("first copy")
        .0;
    assert_eq!(res1["reused"], json!(false));
    assert_eq!(res1["harness"], json!("pi"));
    let sid1 = res1["sessionId"].as_str().expect("sessionId").to_string();
    let command1 = res1["command"].as_str().expect("command");
    assert!(
        command1.contains(&format!("pi --session-id {sid1}")),
        "command: {command1}"
    );
    assert!(
        command1.contains("--name"),
        "command carries --name so pi persists at startup"
    );
    assert!(
        data.join("ctx").join(format!("{sid1}.md")).is_file(),
        "ctx written"
    );

    // 2) 重复复制：复用同一 session id，不新产生。
    let res2 = api_requirement_new_session(State(state.clone()), form(None))
        .await
        .expect("second copy")
        .0;
    assert_eq!(res2["reused"], json!(true));
    assert_eq!(res2["sessionId"], json!(sid1));

    // 3) session 被使用（pi 库出现 <ts>_<id>.jsonl）：下次复制自动换新 id。
    let cwd_dir = pi_root.join("--tmp--proj");
    std::fs::create_dir_all(&cwd_dir).expect("create cwd dir");
    std::fs::write(
        cwd_dir.join(format!("2026-07-16T10-08-55-809Z_{sid1}.jsonl")),
        "{\"type\":\"session\"}\n",
    )
    .expect("write used session file");
    let res3 = api_requirement_new_session(State(state.clone()), form(None))
        .await
        .expect("third copy")
        .0;
    assert_eq!(res3["reused"], json!(false));
    let sid3 = res3["sessionId"]
        .as_str()
        .expect("new sessionId")
        .to_string();
    assert_ne!(sid3, sid1, "used pending must auto-refresh to a new id");
    // 已使用的 sid1 保留关联；新 pending 是 sid3。
    let assoc = load_associations(&state).await.expect("load associations");
    let list = assoc.associations.get("T-001").expect("associated list");
    assert!(list.contains(&sid1), "used session stays associated");
    assert!(list.contains(&sid3), "new pending session associated");
    let pending = load_pending_command(&state, "T-001")
        .await
        .expect("load pending")
        .expect("pending exists");
    assert_eq!(pending.session_id, sid3);

    // 4) 强制刷新：无视使用状态换新 id；未使用过的 sid3 被清理（解除关联 + 删 ctx）。
    let res4 = api_requirement_new_session(State(state.clone()), form(Some(true)))
        .await
        .expect("force refresh")
        .0;
    assert_eq!(res4["reused"], json!(false));
    let sid4 = res4["sessionId"]
        .as_str()
        .expect("forced sessionId")
        .to_string();
    assert_ne!(sid4, sid3);
    let assoc = load_associations(&state).await.expect("load associations");
    let list = assoc.associations.get("T-001").expect("associated list");
    assert!(
        !list.contains(&sid3),
        "unused force-discarded session dissociated"
    );
    assert!(list.contains(&sid1), "used session still associated");
    assert!(list.contains(&sid4), "fresh pending session associated");
    assert!(
        !data.join("ctx").join(format!("{sid3}.md")).exists(),
        "unused discarded ctx file removed"
    );

    // 5) GET pending-session：只读视图，报告 pending 与 used=false。
    let view = api_requirement_pending_session(
        State(state.clone()),
        Query(IdQuery {
            id: Some("T-001".into()),
            ..Default::default()
        }),
    )
    .await
    .expect("pending view")
    .0;
    assert_eq!(view["pending"]["sessionId"], json!(sid4));
    assert_eq!(view["pending"]["used"], json!(false));
    assert_eq!(view["pending"]["harness"], json!("pi"));
}

// ---- requirement group（引用式需求组 group.json）----

