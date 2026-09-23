use super::*;

#[test]
fn doc_has_filled_items_rejects_template_only_content() {
    assert!(!doc_has_filled_items(""));
    assert!(!doc_has_filled_items("# 标题\n\n- 待补充\n- 待补充\n"));
    assert!(doc_has_filled_items(
        "# 标题\n\n- 已确认：修复出库单取消释放\n"
    ));
    assert!(doc_has_filled_items(
        "| 1 | 登录 | 无 | 打开首页 | 跳转 | P0 |\n"
    ));
}


#[test]
fn build_meta_doc_writes_source_line() {
    let meta = build_meta_doc(
        "WMS-100-x",
        "t",
        "需求澄清",
        "WMS",
        &["WMS".into()],
        "需求",
        "开发推动",
        "hevin",
        "2026-01-01",
        "unknown",
        "",
        &[],
        "s",
        None,
    );
    assert!(meta.contains("source: 开发推动"));
    let default_meta = build_meta_doc(
        "WMS-100-x",
        "t",
        "需求澄清",
        "WMS",
        &["WMS".into()],
        "需求",
        "产品推动",
        "hevin",
        "2026-01-01",
        "unknown",
        "",
        &[],
        "s",
        None,
    );
    assert!(default_meta.contains("source: 产品推动"));
}


#[test]
fn build_meta_doc_writes_issues_frontmatter_when_bound() {
    let with = build_meta_doc(
        "WMS-100-fix-x",
        "t",
        "需求澄清",
        "WMS",
        &["WMS".into()],
        "需求",
        "产品推动",
        "hevin",
        "2026-01-01",
        "unknown",
        "",
        &["WMS-099-issue".into()],
        "s",
        None,
    );
    assert!(with.contains("issues: WMS-099-issue"));
    let without = build_meta_doc(
        "WMS-100-fix-x",
        "t",
        "需求澄清",
        "WMS",
        &["WMS".into()],
        "需求",
        "产品推动",
        "hevin",
        "2026-01-01",
        "unknown",
        "",
        &[],
        "s",
        None,
    );
    assert!(!without.contains("issues:"));
}


#[test]
fn online_issue_doc_type_resolves_to_troubleshooting_md() {
    assert_eq!(
        requirement_doc_file("troubleshooting").unwrap(),
        "troubleshooting.md"
    );
    assert_eq!(
        requirement_doc_file("排查经验").unwrap(),
        "troubleshooting.md"
    );
    let tpl = requirement_doc_template(
        &default_requirement_for_test("WMS-001"),
        "troubleshooting.md",
    );
    assert!(tpl.contains("## 怎么排查"));
    assert!(tpl.contains("## 怎么修复"));
    assert!(tpl.contains("## 根因"));
}


#[test]
fn incident_and_root_cause_doc_types_resolve_with_verifiable_evidence_templates() {
    assert_eq!(requirement_doc_file("incident").unwrap(), "incident.md");
    assert_eq!(requirement_doc_file("现象").unwrap(), "incident.md");
    assert_eq!(requirement_doc_file("root-cause").unwrap(), "root-cause.md");
    assert_eq!(requirement_doc_file("rootcause").unwrap(), "root-cause.md");
    assert_eq!(requirement_doc_file("根因").unwrap(), "root-cause.md");
    let incident_tpl =
        requirement_doc_template(&default_requirement_for_test("WMS-001"), "incident.md");
    assert!(incident_tpl.contains("## 影响范围"));
    assert!(incident_tpl.contains("## 复现步骤"));
    let root_cause_tpl =
        requirement_doc_template(&default_requirement_for_test("WMS-001"), "root-cause.md");
    assert!(root_cause_tpl.contains("## 证据链"));
    assert!(root_cause_tpl.contains("验证 SQL"));
    assert!(root_cause_tpl.contains("可全局搜索的关键字片段"));
    assert!(root_cause_tpl.contains("## 修复路径决策"));
}


#[test]
fn phase_entry_checks_issue_doc_set_with_legacy_technical_plan_fallback() {
    let dir = std::env::temp_dir().join(format!(
        "agent-panel-entry-checks-{}-{}",
        std::process::id(),
        chrono_like_unique_suffix()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    // 排查中：incident + notes 必填，未创建时 required 检查不通过。
    let checks = phase_entry_checks("排查中", &dir);
    let incident_check = checks
        .iter()
        .find(|c| c["id"] == "incident-md")
        .expect("incident check should exist");
    assert_eq!(incident_check["required"], true);
    assert_eq!(incident_check["ok"], false);
    std::fs::write(dir.join("incident.md"), "内容").unwrap();
    std::fs::write(dir.join("notes.md"), "内容").unwrap();
    let checks = phase_entry_checks("排查中", &dir);
    assert!(checks
        .iter()
        .all(|c| c["required"] == false || c["ok"] == true));
    // 已定位：root-cause 缺失但存量 technical-plan.md 兼容放行。
    std::fs::write(dir.join("technical-plan.md"), "存量根因记录").unwrap();
    let checks = phase_entry_checks("已定位", &dir);
    let fallback_check = checks
        .iter()
        .find(|c| c["id"] == "root-cause-md-or-technical-plan-md")
        .expect("root-cause fallback check should exist");
    assert_eq!(fallback_check["required"], true);
    assert_eq!(fallback_check["ok"], true);
    // 新问题写 root-cause.md 后同样满足。
    std::fs::write(dir.join("root-cause.md"), "根因与证据链").unwrap();
    let checks = phase_entry_checks("已定位", &dir);
    let fallback_check = checks
        .iter()
        .find(|c| c["id"] == "root-cause-md-or-technical-plan-md")
        .unwrap();
    assert_eq!(fallback_check["ok"], true);
    let _ = std::fs::remove_dir_all(&dir);
}


#[test]
fn agent_context_tokens_issue_category_uses_issue_doc_set() {
    // token→file 映射必须覆盖 issue 专属 token，否则上下文组装会静默丢弃。
    assert_eq!(requirement_token_file("req.incident"), Some("incident.md"));
    assert_eq!(
        requirement_token_file("req.rootCause"),
        Some("root-cause.md")
    );
    assert_eq!(
        requirement_doc_type_for_token("req.incident"),
        Some("incident")
    );
    assert_eq!(
        requirement_doc_type_for_token("req.rootCause"),
        Some("root-cause")
    );
    let tokens = agent_context_tokens("overview", true);
    assert!(tokens.contains(&"req.incident"));
    assert!(tokens.contains(&"req.rootCause"));
    assert!(!tokens.contains(&"req.technicalPlan"));
    let progress = agent_context_tokens("progress", true);
    assert!(progress.contains(&"req.rootCause"));
    assert!(!progress.contains(&"req.technicalPlan"));
    let normal = agent_context_tokens("overview", false);
    assert!(normal.contains(&"req.technicalPlan"));
    assert!(!normal.contains(&"req.incident"));
}


#[test]
fn requirement_create_files_issue_category_scaffolds_incident_doc_set() {
    let issue_files = requirement_create_files(
        "WMS-100-issue",
        "t",
        "排查中",
        "WMS",
        &["WMS".to_string()],
        "线上问题",
        "开发推动",
        "hevin",
        "2026-01-01",
        "unknown",
        "",
        &[],
        "s",
        None,
        None,
        None,
    );
    let names: Vec<&str> = issue_files.iter().map(|(n, _)| *n).collect();
    assert!(names.contains(&"incident.md"));
    assert!(names.contains(&"notes.md"));
    assert!(!names.contains(&"background.md"));
    assert!(!names.contains(&"technical-plan.md"));

    // 测试问题与线上问题同属 issue 家族：同样脚手架 incident.md 文档集
    let test_issue_files = requirement_create_files(
        "WMS-TST-001-uat-bug",
        "t",
        "排查中",
        "WMS",
        &["WMS".to_string()],
        "测试问题",
        "产品推动",
        "hevin",
        "2026-01-01",
        "unknown",
        "",
        &[],
        "s",
        None,
        None,
        None,
    );
    let test_names: Vec<&str> = test_issue_files.iter().map(|(n, _)| *n).collect();
    assert!(test_names.contains(&"incident.md"));
    assert!(test_names.contains(&"notes.md"));
    assert!(!test_names.contains(&"background.md"));
    assert!(!test_names.contains(&"technical-plan.md"));

    let normal_files = requirement_create_files(
        "WMS-101-req",
        "t",
        "需求澄清",
        "WMS",
        &["WMS".to_string()],
        "需求",
        "产品推动",
        "hevin",
        "2026-01-01",
        "unknown",
        "",
        &[],
        "s",
        None,
        None,
        None,
    );
    let normal_names: Vec<&str> = normal_files.iter().map(|(n, _)| *n).collect();
    assert!(normal_names.contains(&"background.md"));
    assert!(normal_names.contains(&"technical-plan.md"));
    assert!(!normal_names.contains(&"incident.md"));
}


#[test]
fn render_markdown_handles_heading_list_table_code() {
    let md = "# 标题\n\n> 引用行\n\n## 小节\n\n| A | B |\n| --- | --- |\n| 1 | 2 |\n\n- 项一\n- 项二\n\n- [ ] 待办\n- [x] 已完成\n\n```rust\nlet x = 1;\n```\n\n**加粗** 和 `code`。";
    let html = render_markdown_html(md);
    assert!(html.contains("<h1>标题</h1>"));
    assert!(html.contains("<h2>小节</h2>"));
    assert!(html.contains("<blockquote>引用行</blockquote>"));
    assert!(html.contains("<th>A</th><th>B</th>"));
    assert!(html.contains("<td>1</td><td>2</td>"));
    assert!(html.contains("<ul>"));
    assert!(html.contains("<li>项一</li>"));
    assert!(html.contains("class=\"task-item\""));
    assert!(html.contains("checked"));
    assert!(html.contains("<pre><code class=\"lang-rust\">let x = 1;"));
    assert!(html.contains("<strong>加粗</strong>"));
    assert!(html.contains("<code>code</code>"));
}


#[test]
fn render_markdown_escapes_html_and_keeps_dashes() {
    let md = "<script>alert(1)</script> 与 `a < b`\n\n---\n";
    let html = render_markdown_html(md);
    assert!(html.contains("&lt;script&gt;"));
    assert!(!html.contains("<script>"));
    assert!(html.contains("<code>a &lt; b</code>"));
    assert!(html.contains("<hr/>"));
}


#[test]
fn render_branch_scope_produces_repo_table() {
    let json = r#"{"version":2,"repos":[{"repoName":"yl-cwhsea-wms-pda-api","branches":["hevin.yang/feature/WMS-070"],"role":"PDA后端","path":"~/Developer/company/WMS/"}]}"#;
    let html = render_branch_scope_html(json).expect("branch scope renders");
    assert!(html.contains("<th>仓库</th>"));
    assert!(html.contains("yl-cwhsea-wms-pda-api"));
    assert!(html.contains("PDA后端"));
    let not_json = render_branch_scope_html("not json");
    assert!(not_json.is_none());
}


#[test]
fn context_page_contains_sections_and_raw_link() {
    let value = json!({
        "ok": true,
        "reqId": "WMS-X",
        "title": "测试需求",
        "status": "测试中",
        "project": "WMS",
        "intent": "release-check",
        "budget": 3000,
        "remainingBudget": 500,
        "tokens": [
            {"token": "req.releaseManifest", "file": "release-manifest.md", "path": "/x/release-manifest.md", "exists": true, "truncated": false, "bytes": 100, "content": "## DB 变更\n- 无"},
            {"token": "req.branchScope", "file": "branch-scope.json", "path": "/x/branch-scope.json", "exists": true, "truncated": false, "bytes": 50, "content": "{\"repos\":[]}"}
        ]
    });
    let req = Requirement {
        id: "WMS-X".to_string(),
        title: "测试需求".to_string(),
        status: "测试中".to_string(),
        project: "WMS".to_string(),
        projects: vec![],
        group_path: vec![],
        description: String::new(),
        session_ids: vec![],
        category: None,
        source: "产品推动".to_string(),
        ones: None,
        issues: Vec::new(),
        plan_release: None,
        created_at: 0,
        updated_at: 0,
        completed_at: None,
        req_dir: None,
        meta_path: None,
        background_path: None,
        branch_path: None,
        test_path: None,
        notes_path: None,
        config_path: None,
        impact_path: None,
        memory_path: None,
        review_path: None,
        technical_plan_path: None,
        release_manifest_path: None,
        release_check_path: None,
        experience_summary_path: None,
        troubleshooting_path: None,
        incident_path: None,
        root_cause_path: None,
        test_scenario_path: None,
        experience_summary_job: None,
        alignment_path: None,
        prd_path: None,
        effort_estimate: None,
        group_members: None,
        group_policy: None,
        member_of: Vec::new(),
        group_status: None,
        group_bottleneck: None,
        parent_req_id: None,
        is_sub_req: false,
        sub_reqs: Vec::new(),
    };
    let html = render_requirement_context_html(&req, "release-check", &value);
    assert!(html.contains("上线清单 Release Manifest"));
    assert!(html.contains("分支范围 Branch Scope"));
    assert!(html.contains("查看原始 JSON"));
    assert!(html.contains("<h2>DB 变更</h2>"));
    assert!(html.contains("<title>测试需求 · release-check · Agent Panel</title>"));
    assert!(html.contains("<h1>测试需求</h1>"));
    assert!(html.contains("测试中"));
}

// ---- git_workflow: 后端 UAT 分支统一命名 uat（WMS 2026-08 起） ----


#[test]
fn truncate_chars_tail_keeps_latest_content() {
    let (out, truncated) = truncate_chars_tail("abcdef", 4);
    assert!(truncated);
    assert!(out.ends_with("cdef"));
    assert!(out.starts_with("…[truncated"));
    let (out, truncated) = truncate_chars_tail("abc", 10);
    assert!(!truncated);
    assert_eq!(out, "abc");
}


#[test]
fn resolve_doc_part_rel_path_rejects_traversal_and_non_docs() {
    assert_eq!(
        resolve_doc_part_rel_path("docs/notes/001-x.md").expect("ok"),
        "docs/notes/001-x.md"
    );
    assert!(resolve_doc_part_rel_path("docs/../secret.md").is_err());
    assert!(resolve_doc_part_rel_path("notes.md").is_err());
    assert!(resolve_doc_part_rel_path("docs/notes/sub/001-x.md").is_err());
    assert!(resolve_doc_part_rel_path("docs/notes/001-x.txt").is_err());
    assert!(resolve_doc_part_rel_path("docs/notes/.md").is_err());
}


/// 分册端到端：创建 → 索引行追加 → 序号递增 → 分册读取 → 清单 → validate 阈值告警。
#[tokio::test]
async fn doc_part_create_list_and_index_end_to_end() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let data = tmp.path().join("data");
    let pi_root = tmp.path().join("pi-sessions");
    let dsh_root = tmp.path().join("dsh-sessions");
    std::fs::create_dir_all(&data).expect("create data dir");
    let proj = tmp.path().join("proj");
    let req_dir = proj.join("req").join("WMS-041-big-notes");
    std::fs::create_dir_all(&req_dir).expect("create req dir");
    std::fs::write(
        req_dir.join("meta.md"),
        "---\nreq-id: WMS-041-big-notes\ntitle: 大需求\nstatus: 开发中\nproject: WMS\ncategory: 需求\n---\n正文\n",
    )
    .expect("write meta.md");
    std::fs::write(
        req_dir.join("notes.md"),
        "# WMS-041 执行笔记\n\n- 早期记录\n",
    )
    .expect("write notes.md");
    let config = json!({ "requirementScanRoots": [proj.to_string_lossy()] });
    std::fs::write(data.join("config.json"), config.to_string()).expect("write config.json");
    let state = temp_app_state(&data, &pi_root, &dsh_root);

    // 1) 创建第一个分册：写文件 + 主文档追加索引行
    let first = api_requirement_doc_part_post(
        State(state.clone()),
        FormOrJson(DocPartCreateForm {
            req_id: "WMS-041-big-notes".into(),
            doc_type: "notes".into(),
            slug: "integration-debug".into(),
            title: Some("联调排查".into()),
            summary: Some("联调期问题与结论".into()),
            content: "联调过程中的详细记录".into(),
            dry_run: Some(false),
        }),
    )
    .await
    .expect("create part 1");
    assert_eq!(first["part"]["filename"], json!("001-integration-debug.md"));
    assert_eq!(
        first["part"]["relPath"],
        json!("docs/notes/001-integration-debug.md")
    );
    let notes = std::fs::read_to_string(req_dir.join("notes.md")).expect("read notes");
    assert!(notes.contains("## 分册索引"));
    assert!(notes.contains(
        "- [001-integration-debug.md](docs/notes/001-integration-debug.md) — 联调期问题与结论"
    ));
    assert!(notes.contains("- 早期记录"));

    // 2) 第二个分册序号递增
    let second = api_requirement_doc_part_post(
        State(state.clone()),
        FormOrJson(DocPartCreateForm {
            req_id: "WMS-041-big-notes".into(),
            doc_type: "notes".into(),
            slug: "regression".into(),
            title: None,
            summary: None,
            content: "回归测试记录".into(),
            dry_run: Some(false),
        }),
    )
    .await
    .expect("create part 2");
    assert_eq!(second["part"]["filename"], json!("002-regression.md"));

    // 3) 分册路径读取：GET doc 支持 docs/ 相对路径
    let part_doc = api_requirement_doc_get(
        State(state.clone()),
        Query(IdQuery {
            id: Some("WMS-041-big-notes".into()),
            file: Some("docs/notes/001-integration-debug.md".into()),
            ..Default::default()
        }),
    )
    .await
    .expect("read part doc");
    assert_eq!(part_doc.0["exists"], json!(true));
    let content = part_doc.0["content"].as_str().expect("content");
    assert!(content.contains("# 联调排查"));
    assert!(content.contains("联调过程中的详细记录"));

    // 4) 分册清单：count=2 且 indexLinked=true
    let list = api_requirement_doc_parts_get(
        State(state.clone()),
        Query(IdQuery {
            id: Some("WMS-041-big-notes".into()),
            file: Some("notes".into()),
            ..Default::default()
        }),
    )
    .await
    .expect("list parts");
    assert_eq!(list.0["count"], json!(2));
    let parts = list.0["parts"].as_array().expect("parts");
    assert!(parts.iter().all(|p| p["indexLinked"] == json!(true)));

    // 5) validate：主文档超 40KB → 拆分阈值告警
    std::fs::write(
        req_dir.join("notes.md"),
        format!("# notes\n{}", "x".repeat(41 * 1024)),
    )
    .expect("write big notes");
    // 重建索引行，避免未索引告警干扰断言
    let _ = api_requirement_doc_part_post(
        State(state.clone()),
        FormOrJson(DocPartCreateForm {
            req_id: "WMS-041-big-notes".into(),
            doc_type: "notes".into(),
            slug: "reindex".into(),
            title: None,
            summary: None,
            content: "占位".into(),
            dry_run: Some(false),
        }),
    )
    .await
    .expect("recreate index");
    let req = get_real_requirement(&state, "WMS-041-big-notes")
        .await
        .expect("req");
    let validation = validate_requirement(&state, &req).await.expect("validate");
    let warnings = validation["warnings"].as_array().expect("warnings");
    assert!(warnings.iter().any(|w| {
        w.as_str()
            .map(|s| s.contains("超过拆分阈值") && s.contains("notes.md"))
            .unwrap_or(false)
    }));
}

