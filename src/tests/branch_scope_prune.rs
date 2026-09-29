use super::*;

/// prune_no_diff_repos：只移除"全部登记分支均真正无文件差异"的应用；
/// diff 读取失败（error 非空）/截断/文件列表读取失败的应用保留；写回刷新 updated_at。
#[tokio::test]
async fn prune_no_diff_repos_removes_only_truly_empty_repos() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = tmp.path();
    let scope = BranchScope {
        version: 2,
        updated_at: 1,
        repos: vec![
            BranchRepo {
                repo_name: "has-diff".into(),
                branches: vec!["feat/a".into()],
                role: Some("后端".into()),
                path: None,
                base_ref: None,
                test_target_branch: None,
                uat_target_branch: None,
            },
            BranchRepo {
                repo_name: "merged".into(),
                branches: vec!["feat/b".into()],
                role: None,
                path: None,
                base_ref: None,
                test_target_branch: None,
                uat_target_branch: None,
            },
            BranchRepo {
                repo_name: "diff-broken".into(),
                branches: vec!["feat/c".into()],
                role: None,
                path: None,
                base_ref: None,
                test_target_branch: None,
                uat_target_branch: None,
            },
        ],
        fallback: false,
        round: 1,
    };
    std::fs::write(
        dir.join("branches.json"),
        serde_json::to_string(&scope).expect("scope json"),
    )
    .expect("write branches.json");

    let review = json!({
        "repos": [
            {"repoName": "has-diff", "branch": "feat/a", "files": [{"path": "x.java", "additions": 1, "deletions": 0}], "additions": 1, "deletions": 0, "diff": "diff --git", "diffTruncated": false, "error": null, "warnings": []},
            {"repoName": "merged", "branch": "feat/b", "files": [], "additions": 0, "deletions": 0, "diff": "", "diffTruncated": false, "error": null, "warnings": []},
            {"repoName": "diff-broken", "branch": "feat/c", "files": [], "additions": 0, "deletions": 0, "diff": "", "diffTruncated": false, "error": "git diff failed", "warnings": []}
        ]
    });

    let (removed, written) = prune_no_diff_repos(dir, &scope, &review)
        .await
        .expect("prune");
    assert_eq!(removed, vec!["merged".to_string()]);
    assert!(written, "scope file must be rewritten");

    let saved: BranchScope =
        serde_json::from_str(&std::fs::read_to_string(dir.join("branches.json")).expect("read"))
            .expect("parse saved scope");
    let names: Vec<String> = saved.repos.iter().map(|r| r.repo_name.clone()).collect();
    assert_eq!(
        names,
        vec!["has-diff".to_string(), "diff-broken".to_string()]
    );
    assert_eq!(saved.version, 2);
    assert!(saved.updated_at > 1, "updated_at must be refreshed");
}

/// name-status/numstat 读取失败（warnings 带"文件列表读取失败"）时 files 为空但差异可能存在，不判空。
#[tokio::test]
async fn prune_no_diff_repos_keeps_repos_with_failed_file_listing() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = tmp.path();
    let scope = BranchScope {
        version: 2,
        updated_at: 1,
        repos: vec![BranchRepo {
            repo_name: "listing-failed".into(),
            branches: vec!["feat/d".into()],
            role: None,
            path: None,
            base_ref: None,
            test_target_branch: None,
            uat_target_branch: None,
        }],
        fallback: false,
        round: 2,
    };
    std::fs::write(
        dir.join("branches-round-2.json"),
        serde_json::to_string(&scope).expect("scope json"),
    )
    .expect("write round2");

    let review = json!({
        "repos": [
            {"repoName": "listing-failed", "branch": "feat/d", "files": [], "additions": 0, "deletions": 0, "diff": "", "diffTruncated": false, "error": null, "warnings": ["文件列表读取失败：git died"]}
        ]
    });

    let (removed, written) = prune_no_diff_repos(dir, &scope, &review)
        .await
        .expect("prune");
    assert!(removed.is_empty(), "listing failure must not be pruned");
    assert!(!written);
    assert!(dir.join("branches-round-2.json").exists());
}

/// 全部应用都无差异时不写文件（登记不允许清空），但仍返回候选供调用方提示。
#[tokio::test]
async fn prune_no_diff_repos_never_empties_the_scope_file() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = tmp.path();
    let scope = BranchScope {
        version: 2,
        updated_at: 1,
        repos: vec![BranchRepo {
            repo_name: "only-one".into(),
            branches: vec!["feat/e".into()],
            role: None,
            path: None,
            base_ref: None,
            test_target_branch: None,
            uat_target_branch: None,
        }],
        fallback: false,
        round: 1,
    };
    let raw_before = serde_json::to_string(&scope).expect("scope json");
    std::fs::write(dir.join("branches.json"), raw_before.clone()).expect("write");

    let review = json!({
        "repos": [
            {"repoName": "only-one", "branch": "feat/e", "files": [], "additions": 0, "deletions": 0, "diff": "", "diffTruncated": false, "error": null, "warnings": []}
        ]
    });

    let (removed, written) = prune_no_diff_repos(dir, &scope, &review)
        .await
        .expect("prune");
    assert_eq!(removed, vec!["only-one".to_string()]);
    assert!(
        !written,
        "scope file must stay untouched when it would empty"
    );
    assert_eq!(
        std::fs::read_to_string(dir.join("branches.json")).expect("read"),
        raw_before,
        "file content must be unchanged"
    );
}
