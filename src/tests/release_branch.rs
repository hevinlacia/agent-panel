use super::*;

fn release_parent_for_test(req_id: &str) -> Requirement {
    let mut req = default_requirement_for_test(req_id);
    req.req_dir = None;
    req
}

#[test]
fn release_branch_prefix_derivation() {
    // 常规：用户前缀 + feature 段 → 替换为 release
    assert_eq!(
        derive_release_branch_prefix(
            &["hevin.yang/feature/WMS-143-oplog-no-audit-upgrade".to_string()],
            None
        ),
        "hevin.yang/release"
    );
    // 显式前缀优先
    assert_eq!(
        derive_release_branch_prefix(
            &["hevin.yang/feature/WMS-143-x".to_string()],
            Some(" teamx /rel ")
        ),
        "teamx/rel"
    );
    // 无 / 的分支名回退 release
    assert_eq!(derive_release_branch_prefix(&["feature-branch".to_string()], None), "release");
    // 空登记回退 release
    assert_eq!(derive_release_branch_prefix(&[], None), "release");
    // 非 feature 中段保留
    assert_eq!(
        derive_release_branch_prefix(&["hevin.yang/hotfix/WMS-1-x".to_string()], None),
        "hevin.yang/hotfix"
    );
}

#[test]
fn release_branch_seq_parsing() {
    assert_eq!(release_branch_seq("r1"), 1);
    assert_eq!(release_branch_seq("r12"), 12);
    assert_eq!(release_branch_seq(""), 0);
}

#[test]
fn release_branch_registry_roundtrip_and_validate() {
    let dir = std::env::temp_dir().join(format!(
        "agent-panel-release-branch-test-{}",
        chrono_like_unique_suffix()
    ));
    std::fs::create_dir_all(&dir).expect("mkdir");
    let mut file = ReleaseBranchesFile::default();
    file.branches.push(ReleaseBranchEntry {
        id: "r1".into(),
        name: "hevin.yang/release/WMS-100-x-r1".into(),
        created_at: 1,
        status: RELEASE_BRANCH_STATUS_ACTIVE.into(),
        repos: vec![ReleaseBranchRepo {
            repo_name: "yl-cwhsea-wms-log-api".into(),
            role: Some("后端".into()),
            path: Some("/tmp/repo".into()),
            base_branch: "master".into(),
            base_commit: Some("abc".into()),
        }],
        merged_subs: vec![ReleaseMergedSub {
            req_id: "WMS-100-S1-a".into(),
            title: Some("子需求A".into()),
            merged_at: 2,
            repos: vec!["yl-cwhsea-wms-log-api".into()],
            note: None,
        }],
        released_at: None,
        note: None,
    });
    tokio::runtime::Runtime::new()
        .expect("rt")
        .block_on(async { write_release_branches(&dir, &file).await.expect("write") });
    let raw = std::fs::read_to_string(dir.join(RELEASE_BRANCHES_FILE)).expect("read");
    assert!(raw.contains("hevin.yang/release/WMS-100-x-r1"));
    assert!(raw.contains("\"version\":1") || raw.contains("\"version\": 1"));
    let parsed = tokio::runtime::Runtime::new()
        .expect("rt")
        .block_on(async { read_release_branches(&dir).await });
    assert_eq!(parsed.branches.len(), 1);
    assert_eq!(parsed.branches[0].id, "r1");
    assert_eq!(parsed.branches[0].status, "active");
    assert_eq!(parsed.branches[0].merged_subs[0].req_id, "WMS-100-S1-a");
    // 损坏 JSON：validate 报 problem，读取回退默认（不 panic）。
    std::fs::write(dir.join(RELEASE_BRANCHES_FILE), "{not-json").expect("write bad");
    let broken = tokio::runtime::Runtime::new()
        .expect("rt")
        .block_on(async { read_release_branches(&dir).await });
    assert!(broken.branches.is_empty());
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn release_branch_id_ref_resolution_defaults_to_active() {
    let file = ReleaseBranchesFile {
        version: 1,
        updated_at: 0,
        branches: vec![
            ReleaseBranchEntry {
                id: "r1".into(),
                name: "b-r1".into(),
                status: RELEASE_BRANCH_STATUS_RELEASED.into(),
                ..Default::default()
            },
            ReleaseBranchEntry {
                id: "r2".into(),
                name: "b-r2".into(),
                status: RELEASE_BRANCH_STATUS_ACTIVE.into(),
                ..Default::default()
            },
        ],
    };
    // 缺省取 active
    assert_eq!(release_branch_id_by_ref(&file, None).unwrap().id, "r2");
    // 显式 id / 完整分支名均可命中
    assert_eq!(release_branch_id_by_ref(&file, Some("r1")).unwrap().id, "r1");
    assert_eq!(release_branch_id_by_ref(&file, Some("b-r2")).unwrap().id, "r2");
    // 未命中报错
    assert!(release_branch_id_by_ref(&file, Some("r9")).is_err());
    // 空 registry 报错
    let empty = ReleaseBranchesFile::default();
    assert!(release_branch_id_by_ref(&empty, None).is_err());
}
