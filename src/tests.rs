use super::*;
use std::path::{Path, PathBuf};

#[test]
fn normalize_capability_maps_legacy_wms_fields_to_common_schema() {
    let cap = json!({
        "id": "outbound-any-status",
        "domain": "outbound",
        "object": "shipment_header",
        "execution": "script",
        "purpose": "创建任意状态出库单",
        "script": "scripts/wms_create_outbound.py",
        "invocation": "uv run python scripts/wms_create_outbound.py --target <state>",
        "verified_env": "test",
        "verified_date": "2026-07-05",
        "stdout_json": true,
        "exit_code": "success_in_json",
        "targets": [{"name": "shipped", "verified": true}],
        "state_graph": "state-graph/outbound.yaml",
        "recipe": "recipes/outbound/create-any-status-shipment.yaml",
        "pitfalls": ["pitfalls/outbound/stock-not-available.md"],
        "notes": ["autoStatus may advance to 900"]
    });
    let normalized = normalize_capability(Path::new("/tmp/wms-testdata"), &cap);
    let policy = json!({
        "contract": "path_maintenance_contract_v1",
        "mandatory": true,
    });
    let detail = capability_detail_with_policy(Path::new("/tmp/wms-testdata"), &cap, &policy);
    assert_eq!(detail["maintenancePolicy"]["mandatory"], true);
    assert_eq!(
        detail["normalized"]["maintenancePolicy"]["contract"],
        "path_maintenance_contract_v1"
    );
    assert_eq!(normalized["kind"], "testdata");
    assert_eq!(normalized["id"], "outbound-any-status");
    assert_eq!(normalized["title"], "创建任意状态出库单");
    assert_eq!(normalized["runner"]["type"], "script");
    assert_eq!(
        normalized["runner"]["script"],
        "scripts/wms_create_outbound.py"
    );
    assert_eq!(normalized["runner"]["cwd"], "/tmp/wms-testdata");
    assert_eq!(normalized["safety"]["agentPanelExecutes"], false);
    assert_eq!(normalized["verification"]["targets"][0]["name"], "shipped");
    assert_eq!(
        normalized["relatedArtifacts"]["recipe"],
        "recipes/outbound/create-any-status-shipment.yaml"
    );
    assert_eq!(normalized["legacy"]["domain"], "outbound");
}

#[test]
fn skipped_statuses_reports_forward_phase_gaps() {
    assert_eq!(
        skipped_statuses(Some("需求澄清"), "测试中"),
        vec!["开发中".to_string(), "自测中".to_string()]
    );
}

#[test]
fn skipped_statuses_ignores_adjacent_and_backward_moves() {
    assert!(skipped_statuses(Some("需求澄清"), "开发中").is_empty());
    assert!(skipped_statuses(Some("测试中"), "开发中").is_empty());
    assert!(skipped_statuses(None, "开发中").is_empty());
}

#[test]
fn status_transition_alias_normalizes_event_type() {
    assert_eq!(
        normalize_requirement_event_type(Some("phase_transition")),
        "statusTransition"
    );
    assert_eq!(requirement_event_label("statusTransition"), "状态切换");
}

#[test]
fn review_snapshot_drift_detects_changed_target_commit() {
    let repo = json!({
        "repoName": "repo-a",
        "branch": "feature/WMS-1",
        "resolvedTargetRef": "feature/WMS-1",
        "targetCommit": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    });
    let drift = review_snapshot_drift_from_repo_value(
        &repo,
        "feature/WMS-1",
        "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
    )
    .expect("changed commit should be stale");
    assert_eq!(drift.repo_name, "repo-a");
    assert_eq!(drift.branch, "feature/WMS-1");
    assert_eq!(
        drift.reviewed_target_commit,
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
    );
    assert_eq!(
        drift.current_target_commit,
        "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
    );
}

#[test]
fn review_snapshot_drift_ignores_same_or_missing_commit() {
    let repo = json!({
        "repoName": "repo-a",
        "branch": "feature/WMS-1",
        "resolvedTargetRef": "feature/WMS-1",
        "targetCommit": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    });
    assert!(review_snapshot_drift_from_repo_value(
        &repo,
        "feature/WMS-1",
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    )
    .is_none());
    assert!(review_snapshot_drift_from_repo_value(&repo, "feature/WMS-1", "").is_none());
    assert!(review_snapshot_drift_from_repo_value(
        &json!({"repoName":"repo-a"}),
        "feature/WMS-1",
        "bbbb"
    )
    .is_none());
}

#[test]
fn incremental_review_drift_parses_coverage_range() {
    let repo = json!({
        "repoName": "repo-a",
        "branch": "feature/WMS-1",
        "projectPath": "/tmp/repo-a",
        "coverageFromCommit": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "coverageToCommit": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
        "linearHistory": true,
    });
    let drift = incremental_review_drift_from_repo(&repo).expect("incremental drift");
    assert_eq!(drift.repo_name, "repo-a");
    assert_eq!(
        drift.reviewed_target_commit,
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
    );
    assert_eq!(
        drift.current_target_commit,
        "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
    );
}

#[test]
fn incremental_repo_cover_requires_linear_matching_range() {
    let drift = ReviewSnapshotDrift {
        repo_name: "repo-a".to_string(),
        branch: "feature/WMS-1".to_string(),
        project_path: Some(PathBuf::from("/tmp/repo-a")),
        reviewed_target_ref: "feature/WMS-1".to_string(),
        reviewed_target_commit: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string(),
        current_target_ref: "feature/WMS-1".to_string(),
        current_target_commit: "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".to_string(),
    };
    let matching = json!({
        "repoName": "repo-a",
        "branch": "feature/WMS-1",
        "coverageFromCommit": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "coverageToCommit": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
        "linearHistory": true,
    });
    assert!(incremental_repo_covers_drift(&matching, &drift));
    let rebased = json!({
        "repoName": "repo-a",
        "branch": "feature/WMS-1",
        "coverageFromCommit": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "coverageToCommit": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
        "linearHistory": false,
    });
    assert!(!incremental_repo_covers_drift(&rebased, &drift));
    let wrong_head = json!({
        "repoName": "repo-a",
        "branch": "feature/WMS-1",
        "coverageFromCommit": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "coverageToCommit": "cccccccccccccccccccccccccccccccccccccccc",
        "linearHistory": true,
    });
    assert!(!incremental_repo_covers_drift(&wrong_head, &drift));
}

#[test]
fn review_gate_status_transition_condition_is_unchanged() {
    assert!(!should_enforce_review_gate_for_status("需求澄清", "开发中"));
    assert!(!should_enforce_review_gate_for_status("测试中", "开发中"));
    assert!(should_enforce_review_gate_for_status("自测中", "测试中"));
    assert!(!should_enforce_review_gate_for_status("测试中", "测试中"));
    assert!(!should_enforce_review_gate_for_status("开发中", "经验总结"));
    assert!(!should_enforce_review_gate_for_status("测试中", "已完成"));
}

#[test]
fn split_seq_template_with_suffix() {
    let (prefix, suffix) = split_seq_template("WMS-{seq}-demo").unwrap();
    assert_eq!(prefix, "WMS");
    assert_eq!(suffix, "-demo");
}

#[test]
fn split_seq_template_trailing() {
    let (prefix, suffix) = split_seq_template("WMS-{seq}").unwrap();
    assert_eq!(prefix, "WMS");
    assert_eq!(suffix, "");
}

#[test]
fn split_seq_template_strips_trailing_hyphen_in_prefix() {
    // "WMS--{seq}" -> prefix trimmed to "WMS"
    let (prefix, _) = split_seq_template("WMS--{seq}").unwrap();
    assert_eq!(prefix, "WMS");
}

#[test]
fn split_seq_template_rejects_missing_placeholder() {
    assert!(split_seq_template("WMS-043").is_err());
}

#[test]
fn split_seq_template_rejects_multiple_placeholders() {
    assert!(split_seq_template("WMS-{seq}-{seq}").is_err());
}

#[test]
fn split_seq_template_rejects_empty_prefix() {
    assert!(split_seq_template("{seq}-demo").is_err());
}

#[test]
fn split_seq_template_rejects_non_ascii_prefix() {
    assert!(split_seq_template("WMS_测试-{seq}").is_err());
}

#[test]
fn format_seq_id_pads_to_three_digits() {
    assert_eq!(format_seq_id("WMS", 43, "-demo"), "WMS-043-demo");
    assert_eq!(format_seq_id("WMS", 1, ""), "WMS-001");
    // 4-digit numbers are not truncated by {:03}
    assert_eq!(format_seq_id("WMS", 1000, "-x"), "WMS-1000-x");
}

#[test]
fn compute_next_seq_ignores_subrequirements_and_gaps() {
    // Existing WMS data: WMS-003-* sub-requirements share 003, 004 is a gap,
    // max is 042 -> next is 043.
    let ids = vec![
        "WMS-001-log".to_string(),
        "WMS-003-a".to_string(),
        "WMS-003-b".to_string(),
        "WMS-003-c".to_string(),
        "WMS-005-x".to_string(),
        "WMS-042-y".to_string(),
        "OTHER-099-z".to_string(), // different prefix, ignored
    ];
    assert_eq!(compute_next_seq_from_ids(&ids, "WMS", None), 43);
}

#[test]
fn compute_next_seq_respects_floor() {
    let ids = vec!["WMS-010-a".to_string()];
    // max + 1 = 11, but floor = 50
    assert_eq!(compute_next_seq_from_ids(&ids, "WMS", Some(50)), 50);
}

#[test]
fn compute_next_seq_starts_at_one_when_no_match() {
    let ids = vec!["OTHER-099".to_string()];
    assert_eq!(compute_next_seq_from_ids(&ids, "WMS", None), 1);
}

#[test]
fn compute_next_seq_ignores_non_numeric_segments() {
    let ids = vec!["WMS-abc".to_string(), "WMS-005".to_string()];
    // WMS-abc does not match \d+, max numeric = 5 -> next 6
    assert_eq!(compute_next_seq_from_ids(&ids, "WMS", None), 6);
}

#[test]
fn compute_next_seq_matches_subrequirement_numbers() {
    // Ensure the regex captures the number even when followed by a hyphen
    // (sub-requirement case like WMS-003-after-picking-batch).
    let ids = vec!["WMS-003-after-picking-batch".to_string()];
    assert_eq!(compute_next_seq_from_ids(&ids, "WMS", None), 4);
}

#[test]
fn parse_ones_ref_extracts_url_from_pasted_text_with_prefix() {
    // 复制的整段文本：编号 + 标题 + 链接，应从链接中提取编号
    let raw = "JTYC-1347611 上架策略新增指定库位 https://ones.jtexpress.com.cn/project/#/team/5BXYuw3B/issue/JTYC-1347611";
    let r = parse_ones_ref(raw).unwrap();
    assert_eq!(r["raw"], raw);
    assert_eq!(
        r["url"],
        "https://ones.jtexpress.com.cn/project/#/team/5BXYuw3B/issue/JTYC-1347611"
    );
    assert_eq!(r["label"], "JTYC-1347611");
}

#[test]
fn parse_ones_ref_pure_url_extracts_issue_label() {
    let raw = "https://ones.jtexpress.com.cn/project/#/team/5BXYuw3B/issue/JTYC-1347611";
    let r = parse_ones_ref(raw).unwrap();
    assert_eq!(r["url"], raw);
    assert_eq!(r["label"], "JTYC-1347611");
}

#[test]
fn parse_ones_ref_plain_id_has_no_url() {
    let r = parse_ones_ref("JTYC-1347611").unwrap();
    assert_eq!(r["url"], Value::Null);
    assert_eq!(r["label"], "JTYC-1347611");
}

#[test]
fn parse_ones_ref_empty_input_is_none() {
    assert!(parse_ones_ref("").is_none());
    assert!(parse_ones_ref("   ").is_none());
}

#[test]
fn experience_summary_entered_at_picks_last_history_entry() {
    // 多次进入经验总结时取最后一次进入时间。
    let state = json!({
        "status": "经验总结",
        "history": [
            {"status": "测试中", "from": "自测中", "at": 1000},
            {"status": "经验总结", "from": "测试中", "at": 2000},
            {"status": "已完成", "from": "经验总结", "at": 3000},
            {"status": "经验总结", "from": "已完成", "at": 4000},
        ]
    });
    assert_eq!(experience_summary_entered_at_from_state(&state, 9999), 4000);
}

#[test]
fn experience_summary_entered_at_falls_back_when_no_history() {
    // 历史缺失或没有经验总结记录时回退到 updated_at。
    let empty = json!({ "status": "经验总结", "history": [] });
    assert_eq!(experience_summary_entered_at_from_state(&empty, 5555), 5555);
    let no_exp = json!({
        "status": "经验总结",
        "history": [{"status": "开发中", "from": null, "at": 1000}]
    });
    assert_eq!(
        experience_summary_entered_at_from_state(&no_exp, 5555),
        5555
    );
}

#[test]
fn experience_summary_overdue_respects_grace_window() {
    let now = 1_800_000_000_000i64; // ~2027，真实毫秒时间戳量级
    let day_ms = 24 * 3600 * 1000i64;
    // 恰好 48h 之前进入 -> 视为超期（>= 阈值）。
    assert!(experience_summary_overdue(
        now - 2 * day_ms,
        now,
        2 * day_ms
    ));
    // 超期更久 -> 超期。
    assert!(experience_summary_overdue(
        now - 3 * day_ms,
        now,
        2 * day_ms
    ));
    // 48h 内 -> 未超期。
    assert!(!experience_summary_overdue(now - day_ms, now, 2 * day_ms));
    // 未来时间戳（时钟异常）-> 不超期。
    assert!(!experience_summary_overdue(now + 1000, now, 2 * day_ms));
    // entered_at 无效（<=0）-> 不推进。
    assert!(!experience_summary_overdue(0, now, 2 * day_ms));
}

#[test]
fn should_auto_complete_only_for_real_experience_summary_status() {
    let now = 1_800_000_000_000i64;
    let day_ms = 24 * 3600 * 1000i64;
    let stale = json!({
        "status": "经验总结",
        "history": [{"status": "经验总结", "from": "测试中", "at": now - 3 * day_ms}]
    });
    // 真实状态为经验总结且超期 -> 推进。
    assert!(should_auto_complete_experience_summary(
        &stale,
        now,
        now,
        2 * day_ms
    ));
    // 真实状态为经验总结但未超期 -> 不推进。
    let fresh = json!({
        "status": "经验总结",
        "history": [{"status": "经验总结", "from": "测试中", "at": now - day_ms}]
    });
    assert!(!should_auto_complete_experience_summary(
        &fresh,
        now,
        now,
        2 * day_ms
    ));
    // 真实状态为待上线（历史遗留，normalize 为经验总结）即使超期也不推进。
    let waiting = json!({
        "status": "待上线",
        "history": [{"status": "待上线", "from": "测试中", "at": now - 10 * day_ms}]
    });
    assert!(!should_auto_complete_experience_summary(
        &waiting,
        now,
        now,
        2 * day_ms
    ));
    // 真实状态为已完成 -> 不推进。
    let done = json!({
        "status": "已完成",
        "history": [{"status": "已完成", "from": "经验总结", "at": now - 3 * day_ms}]
    });
    assert!(!should_auto_complete_experience_summary(
        &done,
        now,
        now,
        2 * day_ms
    ));
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
        ones: None,
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
        experience_summary_job: None,
        alignment_path: None,
        prd_path: None,
        effort_estimate: None,
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
