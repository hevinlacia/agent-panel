use super::*;

#[test]
fn target_from_branch_accepts_lowercase_uat() {
    assert!(matches!(target_from_branch("uat"), Ok(ref t) if t == "uat"));
    assert!(matches!(target_from_branch("test"), Ok(ref t) if t == "test"));
    assert!(matches!(target_from_branch("master"), Ok(ref t) if t == "uat"));
    assert!(matches!(target_from_branch("UAT-2607"), Ok(ref t) if t == "uat"));
    assert!(target_from_branch("production").is_err());
}


#[test]
fn merge_option_label_renders_lowercase_uat_backend() {
    assert_eq!(merge_option_label("backend", "uat"), "后端 UAT (uat)");
    assert_eq!(
        merge_option_label("backend", "UAT-2607"),
        "后端 UAT (UAT-2607)"
    );
    assert_eq!(merge_option_label("backend", "test"), "后端 test");
    assert_eq!(
        merge_option_label("frontend", "master"),
        "前端 UAT (master)"
    );
}


#[test]
fn default_merge_selection_prefers_lowercase_uat_for_backend_testing() {
    let options = vec![
        "test".to_string(),
        "uat".to_string(),
        "UAT-2607".to_string(),
    ];
    // 测试中：后端优先选小写 uat（统一命名），无 uat 时回退 UAT-* 历史分支
    assert_eq!(
        default_merge_selection("测试中", "backend", &options).as_deref(),
        Some("uat")
    );
    let no_lower_uat = vec!["test".to_string(), "UAT-2607".to_string()];
    assert_eq!(
        default_merge_selection("测试中", "backend", &no_lower_uat).as_deref(),
        Some("UAT-2607")
    );
    // 自测中：后端默认 test
    assert_eq!(
        default_merge_selection("自测中", "backend", &options).as_deref(),
        Some("test")
    );
}


#[test]
fn target_branch_matches_repo_accepts_lowercase_uat_for_backend() {
    let backend = BranchRepo {
        repo_name: "yl-cwhsea-wms-outbound-api".to_string(),
        branches: vec![],
        role: Some("后端".to_string()),
        path: Some("~/Developer/company/WMS/backend/yl-cwhsea-wms-outbound-api/".to_string()),
        base_ref: None,
        test_target_branch: None,
        uat_target_branch: None,
    };
    assert!(target_branch_matches_repo(&backend, "uat"));
    assert!(target_branch_matches_repo(&backend, "test"));
    assert!(target_branch_matches_repo(&backend, "UAT-2607"));
    assert!(!target_branch_matches_repo(&backend, "master"));

    let frontend = BranchRepo {
        repo_name: "yl-cwhsea-wms-web-front".to_string(),
        branches: vec![],
        role: Some("前端".to_string()),
        path: Some("~/Developer/company/WMS/frontend/yl-cwhsea-wms-web-front/".to_string()),
        base_ref: None,
        test_target_branch: None,
        uat_target_branch: None,
    };
    assert!(target_branch_matches_repo(&frontend, "master"));
    assert!(!target_branch_matches_repo(&frontend, "uat"));
    assert!(!target_branch_matches_repo(&frontend, "UAT-2607"));
}


#[test]
fn is_production_target_branch_blocks_backend_master_and_frontend_production() {
    // issue 家族（线上问题/测试问题）复现代码禁止合入生产分支：
    // 后端生产分支 master，前端生产分支 production；test/uat/UAT-* 是环境分支，不拦截。
    let backend = BranchRepo {
        repo_name: "yl-cwhsea-wms-outbound-api".to_string(),
        branches: vec![],
        role: Some("后端".to_string()),
        path: Some("~/Developer/company/WMS/backend/yl-cwhsea-wms-outbound-api/".to_string()),
        base_ref: None,
        test_target_branch: None,
        uat_target_branch: None,
    };
    assert!(is_production_target_branch(&backend, "master"));
    assert!(!is_production_target_branch(&backend, "uat"));
    assert!(!is_production_target_branch(&backend, "UAT-2607"));
    assert!(!is_production_target_branch(&backend, "test"));

    let frontend = BranchRepo {
        repo_name: "yl-cwhsea-wms-web-front".to_string(),
        branches: vec![],
        role: Some("前端".to_string()),
        path: Some("~/Developer/company/WMS/frontend/yl-cwhsea-wms-web-front/".to_string()),
        base_ref: None,
        test_target_branch: None,
        uat_target_branch: None,
    };
    assert!(is_production_target_branch(&frontend, "production"));
    // 前端 master 是 UAT 部署分支，不是生产分支，不拦截
    assert!(!is_production_target_branch(&frontend, "master"));
    assert!(!is_production_target_branch(&frontend, "test"));
}


#[tokio::test]
async fn merge_skips_repos_on_exclusion_list() {
    let scope = BranchScope {
        repos: vec![BranchRepo {
            repo_name: "yl-cwhsea-wms-components".into(),
            branches: vec!["feat/x".into()],
            role: Some("组件库".into()),
            path: Some("/tmp/agent-panel-nonexistent-repo".into()),
            ..Default::default()
        }],
        ..Default::default()
    };
    let request = MergeRequest {
        target: "uat".into(),
        target_branch: "uat".into(),
        repo_kind: Some("backend".into()),
    };
    let results = merge_requirement_branches(
        &scope,
        &request,
        false,
        &["yl-cwhsea-wms-components".to_string()],
    )
    .await;
    assert_eq!(results.len(), 1);
    assert_eq!(results[0]["status"], "skipped");
    assert_eq!(results[0]["excluded"], true);
    // 排除名单命中不应触发 git 操作（path 指向不存在的目录，若走到合并会返回 failed）
    assert!(results[0]["message"].as_str().unwrap().contains("排除名单"));
}


#[tokio::test]
async fn merge_does_not_skip_repos_outside_exclusion_list() {
    let scope = BranchScope {
        repos: vec![BranchRepo {
            repo_name: "yl-cwhsea-wms-log-api".into(),
            branches: vec!["feat/x".into()],
            role: Some("后端".into()),
            path: Some("/tmp/agent-panel-nonexistent-repo".into()),
            ..Default::default()
        }],
        ..Default::default()
    };
    let request = MergeRequest {
        target: "uat".into(),
        target_branch: "uat".into(),
        repo_kind: Some("backend".into()),
    };
    let results = merge_requirement_branches(&scope, &request, false, &[]).await;
    assert_eq!(results.len(), 1);
    // 不在排除名单 → 正常走合并流程（此处 path 不存在，结果是 failed 而非 skipped/excluded）
    assert_ne!(results[0]["excluded"], true);
    assert_ne!(results[0]["status"], "skipped");
}

