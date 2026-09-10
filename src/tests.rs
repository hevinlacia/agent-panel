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
fn online_issue_status_machine_uses_new_statuses_with_legacy_aliases() {
    // 新状态机：排查中 → 已定位 → 已修复 → 已复盘 / 已关闭。
    for s in ["排查中", "已定位", "已修复", "已复盘", "已关闭"] {
        assert_eq!(canonical_status(s).unwrap(), s);
    }
    // 旧状态兼容：已确认/问题确认 → 已定位。
    assert_eq!(canonical_status("已确认").unwrap(), "已定位");
    assert_eq!(canonical_status("问题确认").unwrap(), "已定位");
    assert_eq!(canonical_status("线上排查").unwrap(), "排查中");
}

#[test]
fn normalize_create_id_template_splits_issue_pool() {
    // 空模板：按类别给默认池
    assert_eq!(
        normalize_create_id_template("", "线上问题").unwrap(),
        "WMS-INC-{seq}"
    );
    assert_eq!(normalize_create_id_template("", "需求").unwrap(), "WMS-{seq}");
    // 模板：issue 强制 WMS-INC 前缀，保留 suffix
    assert_eq!(
        normalize_create_id_template("WMS-{seq}", "线上问题").unwrap(),
        "WMS-INC-{seq}"
    );
    assert_eq!(
        normalize_create_id_template("WMS-{seq}-hotfix", "线上问题").unwrap(),
        "WMS-INC-{seq}-hotfix"
    );
    assert_eq!(
        normalize_create_id_template("WMS-INC-{seq}", "线上问题").unwrap(),
        "WMS-INC-{seq}"
    );
    // 具体 id：需求透传；issue 必须 WMS-INC-<序号> 形态
    assert_eq!(
        normalize_create_id_template("WMS-112-fix-x", "需求").unwrap(),
        "WMS-112-fix-x"
    );
    assert_eq!(
        normalize_create_id_template("WMS-INC-031-x", "线上问题").unwrap(),
        "WMS-INC-031-x"
    );
    assert!(normalize_create_id_template("WMS-112-fix-x", "线上问题").is_err());
    // 前缀分池：INC 序号独立于需求序号
    assert_eq!(
        compute_next_seq_from_ids(
            &["WMS-INC-031-a".to_string(), "WMS-111-b".to_string()],
            "WMS-INC",
            None
        ),
        32
    );
    assert_eq!(
        compute_next_seq_from_ids(
            &["WMS-INC-031-a".to_string(), "WMS-111-b".to_string()],
            "WMS",
            None
        ),
        112
    );
    // 测试问题独立编号池：默认 WMS-TST-{seq}，强制 TST 前缀，与 INC/需求池互不干扰
    assert_eq!(
        normalize_create_id_template("", "测试问题").unwrap(),
        "WMS-TST-{seq}"
    );
    assert_eq!(
        normalize_create_id_template("TST-{seq}-uat-bug", "测试问题").unwrap(),
        "WMS-TST-{seq}-uat-bug"
    );
    assert_eq!(
        normalize_create_id_template("WMS-TST-005-uat-bug", "测试问题").unwrap(),
        "WMS-TST-005-uat-bug"
    );
    assert!(normalize_create_id_template("WMS-INC-005-x", "测试问题").is_err());
    assert!(normalize_create_id_template("WMS-005-x", "测试问题").is_err());
    // 需求类别不受 TST 池影响：模板原样透传
    assert_eq!(
        normalize_create_id_template("WMS-TST-005-x", "需求").unwrap(),
        "WMS-TST-005-x"
    );
}

/// 并行创建不得撞号（回归）：两个 `{seq}` 模板并发创建（slug 不同）必须拿到连续且
/// 不同的序号。旧实现仅靠 create_dir 的 AlreadyExists 兜底，slug 不同则目录名不同
/// 永不冲突；而扫描式占号也观察不到未写 meta.md 的预留目录，导致两个请求都拿到 117。
/// 现由 `requirement_create_lock` 串行化扫描 -> 预留 -> 写 meta 临界区。
#[tokio::test]
async fn concurrent_create_allocates_distinct_seq_numbers() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let data = tmp.path().join("data");
    let pi_root = tmp.path().join("pi-sessions");
    let dsh_root = tmp.path().join("dsh-sessions");
    std::fs::create_dir_all(&data).expect("create data dir");
    let proj = tmp.path().join("proj");
    std::fs::create_dir_all(&proj).expect("create proj dir");
    let config = json!({ "requirementScanRoots": [proj.to_string_lossy()] });
    std::fs::write(data.join("config.json"), config.to_string()).expect("write config.json");
    let state = temp_app_state(&data, &pi_root, &dsh_root);

    let form = |slug: &str| RequirementCreateForm {
        req_id: format!("T-{{seq}}-{slug}"),
        title: format!("并发占号 {slug}"),
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
        ones: None,
        issues: None,
        summary: None,
        background: None,
        notes: None,
        dry_run: None,
    };

    // 并发创建两个不同 slug 的需求：旧实现在这里双双拿到 T-001。
    let (res_a, res_b) = tokio::join!(
        create_requirement(&state, form("aaa")),
        create_requirement(&state, form("bbb"))
    );
    let id_a = res_a.expect("create a")["reqId"].as_str().expect("reqId a").to_string();
    let id_b = res_b.expect("create b")["reqId"].as_str().expect("reqId b").to_string();
    assert_ne!(id_a, id_b, "parallel creates must not share a reqId");
    let mut ids = vec![id_a, id_b];
    ids.sort();
    assert_eq!(ids, vec!["T-001-aaa", "T-002-bbb"]);
    // 创建根是 scan root 下的 .agents/req（writable_req_roots 展开规则）
    let req_root = proj.join(".agents/req");
    assert!(req_root.join("T-001-aaa").join("meta.md").is_file());
    assert!(req_root.join("T-002-bbb").join("meta.md").is_file());
}

#[test]
fn experience_summary_dispatch_triggers_for_issue_reviewed() {
    let mut req = default_requirement_for_test("WMS-INC-111-x");
    req.status = "已复盘".to_string();
    req.category = Some("线上问题".to_string());
    assert!(experience_summary_triggered(&req));
    req.status = "已修复".to_string();
    assert!(!experience_summary_triggered(&req));
    req.status = "经验总结".to_string();
    req.category = Some("需求".to_string());
    assert!(experience_summary_triggered(&req));
    req.status = "需求澄清".to_string();
    assert!(!experience_summary_triggered(&req));
}

#[test]
fn experience_summary_prompt_issue_variant_covers_troubleshooting_and_dup_check() {
    let mut req = default_requirement_for_test("WMS-INC-111-x");
    req.status = "已复盘".to_string();
    req.category = Some("线上问题".to_string());
    let prompt =
        experience_summary_prompt(&req, Path::new("/tmp/x/experience-summary.md"), "sess-1");
    assert!(prompt.contains("troubleshooting.md"));
    assert!(prompt.contains("默认有价值"));
    assert!(prompt.contains("/api/agent/knowledge/query"));
    assert!(prompt.contains("WMS-INC-111-x"));
    let mut normal = default_requirement_for_test("WMS-120-x");
    normal.status = "经验总结".to_string();
    normal.category = Some("需求".to_string());
    let prompt =
        experience_summary_prompt(&normal, Path::new("/tmp/y/experience-summary.md"), "s2");
    assert!(!prompt.contains("troubleshooting.md"));
    assert!(prompt.contains("experience-summary-context"));
}

#[test]
fn online_issue_statuses_map_to_online_issue_phase_prompt() {
    for s in ["排查中", "已定位", "已修复", "已复盘", "已关闭"] {
        assert_eq!(phase_prompt_file(s), "prompts/phase-online-issue.md");
    }
}

#[test]
fn source_normalizes_to_known_values_with_default() {
    assert_eq!(normalize_source(Some(&"开发推动".to_string())).unwrap(), "开发推动");
    assert_eq!(normalize_source(Some(&"产品推动".to_string())).unwrap(), "产品推动");
    assert!(normalize_source(Some(&"外部".to_string())).is_none());
    assert!(ensure_source("开发推动").is_ok());
    assert!(ensure_source("QA").is_err());
}

#[test]
fn doc_has_filled_items_rejects_template_only_content() {
    assert!(!doc_has_filled_items(""));
    assert!(!doc_has_filled_items("# 标题\n\n- 待补充\n- 待补充\n"));
    assert!(doc_has_filled_items("# 标题\n\n- 已确认：修复出库单取消释放\n"));
    assert!(doc_has_filled_items("| 1 | 登录 | 无 | 打开首页 | 跳转 | P0 |\n"));
}

#[test]
fn build_meta_doc_writes_source_line() {
    let meta = build_meta_doc(
        "WMS-100-x", "t", "需求澄清", "WMS", &["WMS".into()], "需求", "开发推动", "hevin",
        "2026-01-01", "unknown", "", &[], "s",
    );
    assert!(meta.contains("source: 开发推动"));
    let default_meta = build_meta_doc(
        "WMS-100-x", "t", "需求澄清", "WMS", &["WMS".into()], "需求", "产品推动", "hevin",
        "2026-01-01", "unknown", "", &[], "s",
    );
    assert!(default_meta.contains("source: 产品推动"));
}

#[test]
fn build_meta_doc_writes_issues_frontmatter_when_bound() {
    let with = build_meta_doc(
        "WMS-100-fix-x", "t", "需求澄清", "WMS", &["WMS".into()], "需求", "产品推动", "hevin",
        "2026-01-01", "unknown", "", &["WMS-099-issue".into()], "s",
    );
    assert!(with.contains("issues: WMS-099-issue"));
    let without = build_meta_doc(
        "WMS-100-fix-x", "t", "需求澄清", "WMS", &["WMS".into()], "需求", "产品推动", "hevin",
        "2026-01-01", "unknown", "", &[], "s",
    );
    assert!(!without.contains("issues:"));
}

#[test]
fn should_auto_advance_issues_only_for_experience_or_later() {
    assert!(should_auto_advance_issues("经验总结"));
    assert!(should_auto_advance_issues("发布就绪"));
    assert!(should_auto_advance_issues("已完成"));
    assert!(!should_auto_advance_issues("测试中"));
    assert!(!should_auto_advance_issues("开发中"));
    assert!(!should_auto_advance_issues("已定位"));
}

#[test]
fn online_issue_doc_type_resolves_to_troubleshooting_md() {
    assert_eq!(requirement_doc_file("troubleshooting").unwrap(), "troubleshooting.md");
    assert_eq!(requirement_doc_file("排查经验").unwrap(), "troubleshooting.md");
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
    let incident_tpl = requirement_doc_template(
        &default_requirement_for_test("WMS-001"),
        "incident.md",
    );
    assert!(incident_tpl.contains("## 影响范围"));
    assert!(incident_tpl.contains("## 复现步骤"));
    let root_cause_tpl = requirement_doc_template(
        &default_requirement_for_test("WMS-001"),
        "root-cause.md",
    );
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
    assert!(checks.iter().all(|c| c["required"] == false || c["ok"] == true));
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

fn chrono_like_unique_suffix() -> u128 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0)
}

#[test]
fn agent_context_tokens_issue_category_uses_issue_doc_set() {
    // token→file 映射必须覆盖 issue 专属 token，否则上下文组装会静默丢弃。
    assert_eq!(requirement_token_file("req.incident"), Some("incident.md"));
    assert_eq!(requirement_token_file("req.rootCause"), Some("root-cause.md"));
    assert_eq!(requirement_doc_type_for_token("req.incident"), Some("incident"));
    assert_eq!(requirement_doc_type_for_token("req.rootCause"), Some("root-cause"));
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
        "WMS-100-issue", "t", "排查中", "WMS", &["WMS".to_string()], "线上问题", "开发推动", "hevin",
        "2026-01-01", "unknown", "", &[], "s", None, None,
    );
    let names: Vec<&str> = issue_files.iter().map(|(n, _)| *n).collect();
    assert!(names.contains(&"incident.md"));
    assert!(names.contains(&"notes.md"));
    assert!(!names.contains(&"background.md"));
    assert!(!names.contains(&"technical-plan.md"));

    // 测试问题与线上问题同属 issue 家族：同样脚手架 incident.md 文档集
    let test_issue_files = requirement_create_files(
        "WMS-TST-001-uat-bug", "t", "排查中", "WMS", &["WMS".to_string()], "测试问题", "产品推动", "hevin",
        "2026-01-01", "unknown", "", &[], "s", None, None,
    );
    let test_names: Vec<&str> = test_issue_files.iter().map(|(n, _)| *n).collect();
    assert!(test_names.contains(&"incident.md"));
    assert!(test_names.contains(&"notes.md"));
    assert!(!test_names.contains(&"background.md"));
    assert!(!test_names.contains(&"technical-plan.md"));

    let normal_files = requirement_create_files(
        "WMS-101-req", "t", "需求澄清", "WMS", &["WMS".to_string()], "需求", "产品推动", "hevin",
        "2026-01-01", "unknown", "", &[], "s", None, None,
    );
    let normal_names: Vec<&str> = normal_files.iter().map(|(n, _)| *n).collect();
    assert!(normal_names.contains(&"background.md"));
    assert!(normal_names.contains(&"technical-plan.md"));
    assert!(!normal_names.contains(&"incident.md"));
}

fn default_requirement_for_test(req_id: &str) -> Requirement {
    let mut req = default_requirement(Vec::new());
    req.id = req_id.to_string();
    req
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
fn slug_segment_cjk_turns_chinese_title_into_pinyin_id() {
    // 中文标题不再退化成只剩 domain 的 id（如 biz-wms-wms）
    let slug = slug_segment_cjk("WMS 盘点账实调整落表链路", "fallback");
    assert!(slug.starts_with("wms-"), "got: {slug}");
    assert_ne!(slug, "wms", "中文标题 slug 不应退化成 wms: {slug}");
    assert!(
        slug.contains("pan") && slug.contains("luo"),
        "应包含拼音: {slug}"
    );
    // id 组合
    let id = format!("biz-wms-{slug}");
    assert!(id.len() <= 200, "id 过长: {id}");
}

#[test]
fn slug_segment_cjk_keeps_ascii_behavior() {
    assert_eq!(
        slug_segment_cjk("Receipt Callback Retry", "fb"),
        "receipt-callback-retry"
    );
    assert_eq!(
        slug_segment_cjk("WMS-1234-receipt", "fb"),
        "wms-1234-receipt"
    );
}

#[test]
fn slug_segment_cjk_empty_falls_back() {
    assert_eq!(slug_segment_cjk("???", "fallback"), "fallback");
    assert_eq!(slug_segment_cjk("", "fallback"), "fallback");
}

#[test]
fn slug_segment_cjk_caps_length() {
    let long = "盘点账实调整落表链路盘点账实调整落表链路盘点账实调整落表链路盘点账实调整落表链路盘点账实调整落表链路";
    let slug = slug_segment_cjk(long, "fb");
    assert!(
        slug.chars().count() <= 64,
        "应截断到 64: {}",
        slug.chars().count()
    );
}

#[test]
fn dashboard_stats_release_schedule_and_next_release() {
    let now = now_ms();
    let today_key = local_date_key_from_ms(now);
    let date_after = |days: i64| {
        chrono::NaiveDate::parse_from_str(&today_key, "%Y-%m-%d")
            .unwrap()
            .checked_add_signed(chrono::Duration::days(days))
            .unwrap()
            .format("%Y-%m-%d")
            .to_string()
    };
    let tomorrow = date_after(1);
    let far = date_after(20);
    let mk = |id: &str, plan: Option<&str>| {
        let mut r = default_requirement(vec![]);
        r.id = id.to_string();
        r.plan_release = plan.map(|s| s.to_string());
        r
    };
    let reqs = vec![
        mk("R-today-1", Some(today_key.as_str())),
        mk("R-today-2", Some(today_key.as_str())),
        mk("R-tomorrow", Some(tomorrow.as_str())),
        mk("R-far", Some(far.as_str())),
        mk("R-unknown", Some("unknown")),
        mk("R-none", None),
    ];
    let stats = build_dashboard_stats(reqs, now);
    assert_eq!(stats.total, 6);
    assert_eq!(stats.release_schedule.len(), 14);
    assert_eq!(stats.release_schedule[0].date, today_key);
    assert_eq!(stats.release_schedule[0].count, 2);
    assert_eq!(stats.release_schedule[1].date, tomorrow);
    assert_eq!(stats.release_schedule[1].count, 1);
    assert!(stats.release_schedule[2..].iter().all(|d| d.count == 0));
    let next = stats.next_release.clone().expect("next release expected");
    assert_eq!(next.date, today_key);
    assert_eq!(next.count, 2);

    // 最近发版日在 14 天窗口之外时，next_release 仍应命中该日期，窗口内计数全为 0。
    let stats_far = build_dashboard_stats(vec![mk("R-far", Some(far.as_str()))], now);
    assert_eq!(
        stats_far
            .release_schedule
            .iter()
            .map(|d| d.count)
            .sum::<usize>(),
        0
    );
    let next_far = stats_far.next_release.expect("far next release expected");
    assert_eq!(next_far.date, far);
    assert_eq!(next_far.count, 1);

    // 全部未登记日期时无最近发版日。
    let stats_unknown = build_dashboard_stats(vec![mk("R-unknown", Some("unknown"))], now);
    assert!(stats_unknown.next_release.is_none());
}

fn write_file(path: &Path, content: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create parent dirs");
    }
    std::fs::write(path, content).expect("write file");
}

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

fn temp_app_state(data: &Path, pi_root: &Path, dsh_root: &Path) -> AppState {
    AppState {
        project_root: Arc::new(data.to_path_buf()),
        data_dir: Arc::new(data.to_path_buf()),
        pi_session_root: Arc::new(pi_root.to_path_buf()),
        dsh_session_root: Arc::new(dsh_root.to_path_buf()),
        cainiao_mock: Arc::new(Mutex::new(None)),
        experience_summary_dispatch: Arc::new(Mutex::new(())),
        requirement_create_lock: Arc::new(Mutex::new(())),
    }
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
