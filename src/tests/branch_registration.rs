use super::*;

/// 分支登记轮次：round 文件命名、目录扫描（sealed 标志）、创建（结构拷贝 + 清空分支）。
#[tokio::test]
async fn branch_rounds_file_names_listing_and_create() {
    assert_eq!(branch_scope_file_for_round(1), "branches.json");
    assert_eq!(branch_scope_file_for_round(2), "branches-round-2.json");
    assert_eq!(branch_scope_file_for_round(12), "branches-round-12.json");

    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = tmp.path();
    // 无登记文件时空列表。
    assert!(list_branch_scope_rounds(dir)
        .await
        .expect("empty")
        .is_empty());

    std::fs::write(
        dir.join("branches.json"),
        json!({"version": 2, "updatedAt": 1, "repos": [
            {"repoName": "a", "branches": ["hevin.yang/feature/x"], "role": "后端", "path": "/tmp/a/"}
        ]})
        .to_string(),
    )
    .expect("write round1");
    // 无关文件与非法轮次号不被扫描。
    std::fs::write(dir.join("branches-round-0.json"), "{}").expect("write round0");
    std::fs::write(dir.join("branches-old.json"), "{}").expect("write decoy");

    let rounds = list_branch_scope_rounds(dir).await.expect("rounds");
    assert_eq!(rounds.len(), 1);
    assert_eq!(rounds[0].round, 1);
    assert!(!rounds[0].sealed);
    assert_eq!(rounds[0].repo_count, 1);
    assert_eq!(rounds[0].branch_count, 1);

    // 创建 round 2：拷贝 repo 结构、清空 branches；round 1 随之封版。
    let (round, path, scope) = create_branch_scope_round(dir).await.expect("create round2");
    assert_eq!(round, 2);
    assert!(path.ends_with("branches-round-2.json"));
    assert_eq!(scope.round, 2);
    assert!(scope.repos.iter().all(|r| r.branches.is_empty()));
    assert_eq!(scope.repos[0].repo_name, "a");
    assert_eq!(scope.repos[0].role.as_deref(), Some("后端"));

    let rounds = list_branch_scope_rounds(dir)
        .await
        .expect("rounds after create");
    assert_eq!(rounds.len(), 2);
    assert!(
        rounds[0].sealed,
        "round 1 must be sealed once round 2 exists"
    );
    assert!(!rounds[1].sealed);

    // 可继续创建 round 3，latest 指向最大轮次。
    let (round3, _, _) = create_branch_scope_round(dir).await.expect("create round3");
    assert_eq!(round3, 3);
    let rounds = list_branch_scope_rounds(dir)
        .await
        .expect("rounds after round3");
    assert_eq!(rounds.len(), 3);
    assert!(rounds.iter().take(2).all(|r| r.sealed));
    assert!(!rounds[2].sealed);
}


/// 差异快照按轮次独立保留：修复轮次的新快照不挤掉原始轮次可回退的历史。
#[tokio::test]
async fn diff_snapshots_isolated_per_round() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = tmp.path();
    let review = |commit: &str| {
        json!({
            "version": 1, "reqId": "R-1", "updatedAt": 1, "baseRef": "origin/master",
            "repos": [{"repoName": "a", "targetCommit": commit, "files": [], "additions": 0, "deletions": 0}]
        })
    };
    // round 1 连存 7 版（不同 commit），只保留最新 5 版。
    for i in 0..7 {
        save_diff_snapshot(dir, review(&format!("c{i}")), 1)
            .await
            .expect("save r1");
    }
    // round 2 存 2 版，不应挤掉 round 1 的任何一版。
    for i in 0..2 {
        save_diff_snapshot(dir, review(&format!("r2c{i}")), 2)
            .await
            .expect("save r2");
    }
    // 同轮次同 base+commit 重复生成不去重会翻倍；这里验证去重后仍为 5+2。
    save_diff_snapshot(dir, review("c6"), 1)
        .await
        .expect("save r1 dedup");
    save_diff_snapshot(dir, review("r2c1"), 2)
        .await
        .expect("save r2 dedup");

    let doc = read_json_if_exists(&dir.join(CODE_DIFF_SNAPSHOTS_FILE))
        .await
        .expect("snapshots doc exists");
    let snapshots = doc
        .get("snapshots")
        .and_then(|v| v.as_array())
        .expect("array");
    let round_of = |s: &serde_json::Value| s.get("round").and_then(serde_json::Value::as_u64);
    let r1 = snapshots.iter().filter(|s| round_of(s) == Some(1)).count();
    let r2 = snapshots.iter().filter(|s| round_of(s) == Some(2)).count();
    assert_eq!(r1, 5, "round 1 keeps its own 5 snapshots");
    assert_eq!(
        r2, 2,
        "round 2 keeps its own snapshots without evicting round 1"
    );
}


#[tokio::test]
async fn branch_registration_put_validates_and_dedupes() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let data = tmp.path().join("data");
    let pi_root = tmp.path().join("pi-sessions");
    let dsh_root = tmp.path().join("dsh-sessions");
    std::fs::create_dir_all(&data).expect("create data dir");
    let proj = tmp.path().join("proj");
    let req_dir = proj.join("req").join("T-910");
    std::fs::create_dir_all(&req_dir).expect("create req dir");
    std::fs::write(
        req_dir.join("meta.md"),
        "---\nreq-id: T-910\ntitle: 分支登记测试\nstatus: 开发中\n---\n正文\n",
    )
    .expect("write meta.md");
    let config = json!({ "requirementScanRoots": [proj.to_string_lossy()] });
    std::fs::write(data.join("config.json"), config.to_string()).expect("write config.json");
    let state = temp_app_state(&data, &pi_root, &dsh_root);

    // fixture：真实 git 仓库（backend 层），带 origin/feature/x 远程追踪引用
    let repo_dir = proj.join("backend").join("rl-log-api");
    std::fs::create_dir_all(&repo_dir).expect("create repo dir");
    let run = |args: &[&str]| {
        std::process::Command::new("git")
            .args(args)
            .current_dir(&repo_dir)
            .env("GIT_AUTHOR_NAME", "t")
            .env("GIT_AUTHOR_EMAIL", "t@t")
            .env("GIT_COMMITTER_NAME", "t")
            .env("GIT_COMMITTER_EMAIL", "t@t")
            .output()
            .expect("git")
    };
    assert!(run(&["init", "-b", "main"]).status.success());
    std::fs::write(repo_dir.join("f.txt"), "v1").expect("write file");
    assert!(run(&["add", "."]).status.success());
    assert!(run(&["commit", "-m", "init"]).status.success());
    assert!(run(&["branch", "feature/x"]).status.success());
    assert!(run(&["update-ref", "refs/remotes/origin/feature/x", "refs/heads/feature/x"]).status.success());

    let put = |state2: AppState, repos: Value, confirm_change: bool, confirm_removal: bool| {
        api_requirement_branch_registration_put(
            State(state2),
            FormOrJson(BranchRegistrationSaveForm {
                req_id: "T-910".into(),
                round: None,
                repos: Some(repos),
                confirm_branch_change: confirm_change,
                confirm_removal: confirm_removal,
                verify_remote: false,
            }),
        )
    };

    // 1) 首次登记：path 省略（从空登记 + 无 workspace root → 必须显式 path）
    let err = put(
        state.clone(),
        json!([{ "repoName": "rl-log-api", "branch": "feature/x" }]),
        false,
        false,
    )
    .await
    .expect_err("path unresolvable should 400");
    assert!(err.message.contains("显式提供 path"), "{}", err.message);

    // 2) 显式 path + 分支实测 → added；role 从 backend 路径推断
    let repo_path_str = repo_dir.to_string_lossy().to_string();
    let v = put(
        state.clone(),
        json!([{ "repoName": "rl-log-api", "branch": "feature/x", "path": repo_path_str }]),
        false,
        false,
    )
    .await
    .expect("register");
    assert_eq!(v.0["changes"]["added"], json!(["rl-log-api"]));
    assert_eq!(v.0["scope"]["repos"][0]["role"], json!("后端"));
    assert_eq!(v.0["scope"]["repos"][0]["path"], json!(repo_path_str));
    let on_disk = std::fs::read_to_string(req_dir.join("branches.json")).expect("branches.json");
    assert!(on_disk.contains("feature/x"));

    // 3) 同仓同分支重复提交 → 幂等 unchanged
    let v = put(
        state.clone(),
        json!([{ "repoName": "rl-log-api", "branch": "feature/x", "path": repo_path_str }]),
        false,
        false,
    )
    .await
    .expect("idempotent");
    assert_eq!(v.0["changes"]["unchanged"], json!(["rl-log-api"]));

    // 4) 换分支未确认 → 400 列出已登记分支
    let err = put(
        state.clone(),
        json!([{ "repoName": "rl-log-api", "branch": "feature/y", "path": repo_path_str }]),
        false,
        false,
    )
    .await
    .expect_err("branch change must be confirmed");
    assert!(err.message.contains("confirmBranchChange"), "{}", err.message);
    assert!(err.message.contains("feature/x"), "{}", err.message);

    // 5) 换分支确认 → updated + warning；分支 feature/y 未建 → 400 提示实测失败
    let err = put(
        state.clone(),
        json!([{ "repoName": "rl-log-api", "branch": "feature/y", "path": repo_path_str }]),
        true,
        false,
    )
    .await
    .expect_err("feature/y does not exist");
    assert!(err.message.contains("feature/y"), "{}", err.message);
    assert!(run(&["branch", "feature/y"]).status.success());
    assert!(run(&["update-ref", "refs/remotes/origin/feature/y", "refs/heads/feature/y"]).status.success());
    let v = put(
        state.clone(),
        json!([{ "repoName": "rl-log-api", "branch": "feature/y", "path": repo_path_str }]),
        true,
        false,
    )
    .await
    .expect("branch change confirmed");
    assert_eq!(v.0["changes"]["updated"], json!(["rl-log-api"]));
    assert_eq!(v.0["scope"]["repos"][0]["branches"], json!(["feature/y"]));

    // 6) 登记第二个仓库后，提交清单漏掉它 → 400 提示 confirmRemoval
    let _ = put(
        state.clone(),
        json!([
            { "repoName": "rl-log-api", "branch": "feature/y", "path": repo_path_str },
            { "repoName": "rl-two", "branch": "feature/y", "path": repo_dir.to_string_lossy() }
        ]),
        false,
        false,
    )
    .await
    .expect("add second repo");
    let err = put(
        state.clone(),
        json!([{ "repoName": "rl-log-api", "branch": "feature/y", "path": repo_path_str }]),
        false,
        false,
    )
    .await
    .expect_err("removal must be confirmed");
    assert!(err.message.contains("confirmRemoval"), "{}", err.message);
    assert!(err.message.contains("rl-two"), "{}", err.message);
    let v = put(
        state.clone(),
        json!([{ "repoName": "rl-log-api", "branch": "feature/y", "path": repo_path_str }]),
        false,
        true,
    )
    .await
    .expect("removal confirmed");
    assert_eq!(v.0["changes"]["removed"], json!(["rl-two"]));
}
