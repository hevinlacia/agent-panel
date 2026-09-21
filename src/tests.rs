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
fn status_gate_rules_default_seed_preserves_historical_gates() {
    let rules = default_status_gate_rules();
    let gates_of = |from: &str, to: &str| {
        rules
            .iter()
            .find(|r| r.from == from && r.to == to)
            .map(|r| r.gates.clone())
            .unwrap_or_default()
    };
    // 历史行为：进入测试中需要代码审查 + 测试场景文档（开发推动在门禁内自判）。
    assert_eq!(
        gates_of("开发中", "测试中"),
        vec!["review", "test-scenario"]
    );
    // 新增：自测中 → 测试中 还要求自测清单。
    assert_eq!(
        gates_of("自测中", "测试中"),
        vec!["review", "selftest-checklist", "test-scenario"]
    );
    // 线上问题硬门禁迁移为默认规则。
    assert_eq!(gates_of("排查中", "已定位"), vec!["issue-root-cause"]);
    assert_eq!(gates_of("已修复", "已复盘"), vec!["issue-troubleshooting"]);
    // 未配置的流转不拦。
    assert!(gates_of("需求澄清", "开发中").is_empty());
    assert!(gates_of("测试中", "发布就绪").is_empty());
}

#[test]
fn status_gate_rules_normalize_validates_and_dedupes() {
    let rules = vec![
        StatusGateRule {
            from: "自测中".into(),
            to: "测试中".into(),
            gates: vec!["review".into(), " review ".into()],
        },
        StatusGateRule {
            from: "自测中".into(),
            to: "测试中".into(),
            gates: vec!["selftest-checklist".into()],
        },
    ];
    let normalized = normalize_status_gate_rules_strict(rules).expect("valid rules");
    assert_eq!(normalized.len(), 1);
    assert_eq!(normalized[0].from, "自测中");
    assert_eq!(normalized[0].to, "测试中");
    // 同对规则后者覆盖前者；trim 后去重。
    assert_eq!(normalized[0].gates, vec!["selftest-checklist"]);
    let bad_gate = normalize_status_gate_rules_strict(vec![StatusGateRule {
        from: "开发中".into(),
        to: "测试中".into(),
        gates: vec!["nope".into()],
    }]);
    assert!(bad_gate.is_err());
    let bad_status = normalize_status_gate_rules_strict(vec![StatusGateRule {
        from: "不存在的状态".into(),
        to: "测试中".into(),
        gates: vec!["review".into()],
    }]);
    assert!(bad_status.is_err());
    let same = normalize_status_gate_rules_strict(vec![StatusGateRule {
        from: "测试中".into(),
        to: "测试中".into(),
        gates: vec![],
    }]);
    assert!(same.is_err());
    // 宽容模式：非法条目静默丢弃。
    let lenient = normalize_status_gate_rules_lenient(vec![StatusGateRule {
        from: "开发中".into(),
        to: "测试中".into(),
        gates: vec!["nope".into(), "review".into()],
    }]);
    assert_eq!(lenient.len(), 1);
    assert_eq!(lenient[0].gates, vec!["review"]);
}

#[test]
fn effective_status_gates_none_uses_defaults_empty_disables() {
    let mut cfg = AppConfig::default();
    assert_eq!(effective_status_gates(&cfg), default_status_gate_rules());
    // 显式空数组 = 关闭所有门禁。
    cfg.status_gates = Some(Vec::new());
    assert!(effective_status_gates(&cfg).is_empty());
    cfg.status_gates = Some(vec![StatusGateRule {
        from: "开发中".into(),
        to: "自测中".into(),
        gates: vec!["selftest-checklist".into()],
    }]);
    let rules = effective_status_gates(&cfg);
    assert_eq!(rules.len(), 1);
    assert_eq!(rules[0].gates, vec!["selftest-checklist"]);
}

/// 状态门禁端到端（进程内直调 handler）：配置驱动分发 + via=ui 人工跳过。
#[tokio::test]
async fn status_transition_gates_config_driven_and_ui_skipped() {
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
        "---\nreq-id: T-001\ntitle: 门禁测试\nstatus: 自测中\n---\n正文\n",
    )
    .expect("write meta.md");
    // 只配置 自测中→测试中 一条门禁：其它流转不应被拦。
    let config = json!({
        "requirementScanRoots": [proj.to_string_lossy()],
        "statusGates": [
            { "from": "自测中", "to": "测试中", "gates": ["selftest-checklist"] }
        ]
    });
    std::fs::write(data.join("config.json"), config.to_string()).expect("write config.json");
    let state = temp_app_state(&data, &pi_root, &dsh_root);
    let form = |status: &str, via: Option<String>| {
        FormOrJson(StatusForm {
            req_id: "T-001".into(),
            status: status.into(),
            note: None,
            via,
        })
    };

    // 1) agent 推进（无 via）：test.md 缺自测清单 → 拦截。
    let blocked = api_requirement_status(State(state.clone()), form("测试中", None))
        .await
        .expect_err("missing selftest checklist must block agent transition");
    assert!(format!("{:?}", blocked).contains("自测门禁"));

    // 2) via=ui：人工在面板上修改状态，直接跳过门禁。
    let _ = api_requirement_status(State(state.clone()), form("测试中", Some("ui".into())))
        .await
        .expect("ui transition skips gates");

    // 3) 反向流转未配置门禁：agent 推进也放行。
    let _ = api_requirement_status(State(state.clone()), form("自测中", None))
        .await
        .expect("unconfigured reverse transition passes");

    // 4) 补齐自测清单后，agent 推进自测中→测试中放行。
    std::fs::write(
        req_dir.join("test.md"),
        "# T-001 Test\n\n## 自测清单\n\n| # | 自测项 | 结果 | 失败/无法测试原因 |\n| --- | --- | --- | --- |\n| 1 | 回退接口 | 通过 | - |\n| 2 | 消费链路 | 无法测试 | test 环境 OMS 未订阅 topic，无法联调 |\n",
    )
    .expect("write test.md");
    let _ = api_requirement_status(State(state.clone()), form("测试中", None))
        .await
        .expect("filled checklist passes agent transition");

    // 5) 未配置的 开发中→测试中：即使清单缺失也不拦（门禁完全由配置驱动）。
    let _ = api_requirement_status(State(state.clone()), form("开发中", Some("ui".into())))
        .await
        .expect("ui back to 开发中");
    std::fs::remove_file(req_dir.join("test.md")).expect("remove test.md");
    let _ = api_requirement_status(State(state.clone()), form("测试中", None))
        .await
        .expect("unconfigured pair has no gates");
}

/// 状态流转卡片 API：门禁三态（failed 预览 / unverified 人工跳过 / passed 流转时已通过）。
#[tokio::test]
async fn status_flow_reports_gate_states_with_unverified() {
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
        "---\nreq-id: T-001\ntitle: 门禁卡片测试\nstatus: 自测中\n---\n正文\n",
    )
    .expect("write meta.md");
    let config = json!({
        "requirementScanRoots": [proj.to_string_lossy()],
        "statusGates": [
            { "from": "自测中", "to": "测试中", "gates": ["selftest-checklist"] }
        ]
    });
    std::fs::write(data.join("config.json"), config.to_string()).expect("write config.json");
    let state = temp_app_state(&data, &pi_root, &dsh_root);
    let flow = || async {
        api_requirement_status_flow(
            State(state.clone()),
            Query(IdQuery {
                id: Some("T-001".into()),
                ..Default::default()
            }),
        )
        .await
        .expect("status flow")
        .0
    };
    let junction_gate = |v: &Value| {
        v["transitions"]
            .as_array()
            .expect("transitions")
            .iter()
            .find(|t| t["to"] == json!("测试中"))
            .expect("transition into 测试中")["gates"]
            .as_array()
            .expect("gates")[0]
            .clone()
    };

    // 1) 未走过的流转：实时预览评估，缺自测清单 → failed。
    let v = flow().await;
    assert_eq!(v["statuses"].as_array().expect("statuses").len(), 7);
    assert_eq!(v["currentIndex"], json!(2));
    assert_eq!(junction_gate(&v)["state"], json!("failed"));

    // 2) via=ui 人工推进越过门禁 → 已走过的流转标 unverified。
    let _ = api_requirement_status(
        State(state.clone()),
        FormOrJson(StatusForm {
            req_id: "T-001".into(),
            status: "测试中".into(),
            note: None,
            via: Some("ui".into()),
        }),
    )
    .await
    .expect("ui transition");
    let v = flow().await;
    assert_eq!(v["currentIndex"], json!(3));
    let gate = junction_gate(&v);
    assert_eq!(gate["state"], json!("unverified"));
    assert!(gate["reason"]
        .as_str()
        .expect("reason")
        .contains("跳过门禁校验"));

    // 3) 退回自测中后，该流转重新变为未走完 → 回到实时预览评估 failed。
    let _ = api_requirement_status(
        State(state.clone()),
        FormOrJson(StatusForm {
            req_id: "T-001".into(),
            status: "自测中".into(),
            note: None,
            via: Some("ui".into()),
        }),
    )
    .await
    .expect("ui transition back");
    let v = flow().await;
    assert_eq!(junction_gate(&v)["state"], json!("failed"));

    // 4) 补齐自测清单后 agent 推进（无 via，门禁校验通过）→ 标 passed。
    std::fs::write(
        req_dir.join("test.md"),
        "# T-001 Test\n\n## 自测清单\n\n| # | 自测项 | 结果 | 失败/无法测试原因 |\n| --- | --- | --- | --- |\n| 1 | 回退接口 | 通过 | - |\n",
    )
    .expect("write test.md");
    let _ = api_requirement_status(
        State(state.clone()),
        FormOrJson(StatusForm {
            req_id: "T-001".into(),
            status: "测试中".into(),
            note: None,
            via: None,
        }),
    )
    .await
    .expect("agent transition");
    let v = flow().await;
    let gate = junction_gate(&v);
    assert_eq!(gate["state"], json!("passed"));
    assert!(gate["reason"]
        .as_str()
        .expect("reason")
        .contains("已通过门禁校验"));

    // 5) 用户场景：门禁材料完好，人工 via=ui 回退再重进测试中 → 不再标 unverified，
    //    实时评估材料满足 → 显示 passed（注明实时评估）。
    let _ = api_requirement_status(
        State(state.clone()),
        FormOrJson(StatusForm {
            req_id: "T-001".into(),
            status: "自测中".into(),
            note: None,
            via: Some("ui".into()),
        }),
    )
    .await
    .expect("ui rollback");
    let _ = api_requirement_status(
        State(state.clone()),
        FormOrJson(StatusForm {
            req_id: "T-001".into(),
            status: "测试中".into(),
            note: None,
            via: Some("ui".into()),
        }),
    )
    .await
    .expect("ui re-enter");
    let v = flow().await;
    let gate = junction_gate(&v);
    assert_eq!(
        gate["state"],
        json!("passed"),
        "材料满足时人工流转也应显示通过"
    );
    assert!(gate["reason"]
        .as_str()
        .expect("reason")
        .contains("实时评估通过"));
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
        normalize_create_id_template("", "线上问题", false).unwrap(),
        "WMS-INC-{seq}"
    );
    assert_eq!(
        normalize_create_id_template("", "需求", false).unwrap(),
        "WMS-{seq}"
    );
    // 模板：issue 强制 WMS-INC 前缀，保留 suffix
    assert_eq!(
        normalize_create_id_template("WMS-{seq}", "线上问题", false).unwrap(),
        "WMS-INC-{seq}"
    );
    assert_eq!(
        normalize_create_id_template("WMS-{seq}-hotfix", "线上问题", false).unwrap(),
        "WMS-INC-{seq}-hotfix"
    );
    assert_eq!(
        normalize_create_id_template("WMS-INC-{seq}", "线上问题", false).unwrap(),
        "WMS-INC-{seq}"
    );
    // 具体 id：需求透传；issue 必须 WMS-INC-<序号> 形态
    assert_eq!(
        normalize_create_id_template("WMS-112-fix-x", "需求", false).unwrap(),
        "WMS-112-fix-x"
    );
    assert_eq!(
        normalize_create_id_template("WMS-INC-031-x", "线上问题", false).unwrap(),
        "WMS-INC-031-x"
    );
    assert!(normalize_create_id_template("WMS-112-fix-x", "线上问题", false).is_err());
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
        normalize_create_id_template("", "测试问题", false).unwrap(),
        "WMS-TST-{seq}"
    );
    assert_eq!(
        normalize_create_id_template("TST-{seq}-uat-bug", "测试问题", false).unwrap(),
        "WMS-TST-{seq}-uat-bug"
    );
    assert_eq!(
        normalize_create_id_template("WMS-TST-005-uat-bug", "测试问题", false).unwrap(),
        "WMS-TST-005-uat-bug"
    );
    assert!(normalize_create_id_template("WMS-INC-005-x", "测试问题", false).is_err());
    assert!(normalize_create_id_template("WMS-005-x", "测试问题", false).is_err());
    // 需求类别不受 TST 池影响：模板原样透传
    assert_eq!(
        normalize_create_id_template("WMS-TST-005-x", "需求", false).unwrap(),
        "WMS-TST-005-x"
    );
}

#[test]
fn normalize_create_id_template_enforces_group_pool() {
    // 空模板：需求组默认 WMS-GRP-{seq} 独立编号池
    assert_eq!(
        normalize_create_id_template("", "需求", true).unwrap(),
        "WMS-GRP-{seq}"
    );
    // {seq} 模板：改写前缀为 WMS-GRP，保留 suffix
    assert_eq!(
        normalize_create_id_template("WMS-{seq}", "需求", true).unwrap(),
        "WMS-GRP-{seq}"
    );
    assert_eq!(
        normalize_create_id_template("WMS-{seq}-combo-checkout", "需求", true).unwrap(),
        "WMS-GRP-{seq}-combo-checkout"
    );
    assert_eq!(
        normalize_create_id_template("WMS-GRP-{seq}", "需求", true).unwrap(),
        "WMS-GRP-{seq}"
    );
    // 具体 id：必须 WMS-GRP-<序号> 形态，否则拒绝（组不占用 WMS-<seq> 需求池）
    assert_eq!(
        normalize_create_id_template("WMS-GRP-001-combo", "需求", true).unwrap(),
        "WMS-GRP-001-combo"
    );
    assert!(normalize_create_id_template("WMS-126-combo", "需求", true).is_err());
    // 组编号池独立：GRP 序号独立于需求序号
    assert_eq!(
        compute_next_seq_from_ids(
            &[
                "WMS-126-remove-rabbitmq-combo".to_string(),
                "WMS-GRP-003-a".to_string(),
                "WMS-INC-031-b".to_string(),
            ],
            "WMS-GRP",
            None
        ),
        4
    );
}

#[tokio::test]
async fn create_requirement_group_rejects_issue_category_and_legacy_pool_id() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let state = group_test_state(&tmp);
    create_requirement(&state, group_test_form("G-M3", "成员3"))
        .await
        .expect("create member");

    // 组不能使用 issue 类别
    let mut issue_group = group_test_form("WMS-GRP-001-bad", "issue 类别组");
    issue_group.category = Some("线上问题".to_string());
    issue_group.members = Some(vec![group_test_member_input("G-M3")]);
    let err = create_requirement(&state, issue_group)
        .await
        .expect_err("group with issue category must fail");
    assert!(err.message.contains("category=需求"), "{:?}", err);

    // 组必须使用 WMS-GRP 池：legacy 需求池形态被拒
    let mut legacy = group_test_form("WMS-126-legacy-combo", "需求池 id 的组");
    legacy.members = Some(vec![group_test_member_input("G-M3")]);
    let err = create_requirement(&state, legacy)
        .await
        .expect_err("legacy pool group id must fail");
    assert!(err.message.contains("WMS-GRP"), "{:?}", err);

    // 默认池 + {seq} 模板：组创建成功并自动分配 WMS-GRP-001
    let mut ok = group_test_form("", "默认池组");
    ok.members = Some(vec![group_test_member_input("G-M3")]);
    ok.req_id = "WMS-GRP-{seq}-combo".to_string();
    let res = create_requirement(&state, ok).await.expect("create group");
    assert_eq!(res["reqId"], json!("WMS-GRP-001-combo"));
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
        members: None,
        release_policy: None,
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
    let id_a = res_a.expect("create a")["reqId"]
        .as_str()
        .expect("reqId a")
        .to_string();
    let id_b = res_b.expect("create b")["reqId"]
        .as_str()
        .expect("reqId b")
        .to_string();
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
    assert_eq!(
        normalize_source(Some(&"开发推动".to_string())).unwrap(),
        "开发推动"
    );
    assert_eq!(
        normalize_source(Some(&"产品推动".to_string())).unwrap(),
        "产品推动"
    );
    assert!(normalize_source(Some(&"外部".to_string())).is_none());
    assert!(ensure_source("开发推动").is_ok());
    assert!(ensure_source("QA").is_err());
}

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
fn selftest_checklist_missing_section_fails() {
    let problems = validate_selftest_checklist("# Test\n\n## 自测记录\n- ⬜ 待执行\n");
    assert!(problems.iter().any(|p| p.contains("未找到")));
}

#[test]
fn selftest_checklist_requires_result_and_reason() {
    let body = "## 自测清单\n\n| # | 自测项 | 结果 | 失败/无法测试原因 |\n| --- | --- | --- | --- |\n| 1 | 场景A | 通过 | - |\n| 2 | 场景B | 待测试 | - |\n| 3 | 场景C | 失败 | - |\n";
    let problems = validate_selftest_checklist(body);
    assert_eq!(problems.len(), 2);
    assert!(problems[0].contains("场景B") && problems[0].contains("缺少测试结果"));
    assert!(problems[1].contains("场景C") && problems[1].contains("原因"));
}

#[test]
fn selftest_checklist_passes_with_block_reasons() {
    let body = "## 自测清单\n\n| # | 自测项 | 结果 | 失败/无法测试原因 |\n| --- | --- | --- | --- |\n| 1 | 场景A | 通过 | - |\n| 2 | 场景B | 无法测试 | test 环境 OMS 未订阅 wms-shipment-update-topic，无法联调 |\n| 3 | 场景C | ❌ | 消费端未部署，等待 UAT 验证 |\n| 4 | 场景D | 失败：test 环境服务未部署 | - |\n";
    assert!(validate_selftest_checklist(body).is_empty());
}

#[test]
fn selftest_checklist_requires_result_column_and_items() {
    let no_result_col = validate_selftest_checklist(
        "## 自测清单\n\n| 项目 | 状态 |\n| --- | --- |\n| 场景A | 通过 |\n",
    );
    assert!(no_result_col.iter().any(|p| p.contains("结果")));
    let empty = validate_selftest_checklist(
        "## 自测清单\n\n| # | 自测项 | 结果 | 失败/无法测试原因 |\n| --- | --- | --- | --- |\n",
    );
    assert!(empty.iter().any(|p| p.contains("没有")));
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

// ---- requirement group（引用式需求组 group.json）----

fn group_test_form(req_id: &str, title: &str) -> RequirementCreateForm {
    RequirementCreateForm {
        req_id: req_id.to_string(),
        title: title.to_string(),
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
        members: None,
        release_policy: None,
        summary: None,
        background: None,
        notes: None,
        dry_run: None,
    }
}

fn group_test_state(tmp: &tempfile::TempDir) -> AppState {
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

fn group_test_member_input(id: &str) -> GroupMemberInput {
    GroupMemberInput {
        req_id: id.to_string(),
        note: None,
    }
}

#[tokio::test]
async fn group_status_aggregation_takes_min_member_status() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let state = group_test_state(&tmp);
    create_requirement(&state, group_test_form("G-MEMBER-A", "成员A"))
        .await
        .expect("create member a");
    create_requirement(&state, group_test_form("G-MEMBER-B", "成员B"))
        .await
        .expect("create member b");
    let req_a = get_real_requirement(&state, "G-MEMBER-A")
        .await
        .expect("req a");
    write_requirement_status(req_a.req_dir.as_deref().unwrap_or_default(), "测试中", None)
        .await
        .expect("set status a");
    let req_b = get_real_requirement(&state, "G-MEMBER-B")
        .await
        .expect("req b");
    write_requirement_status(req_b.req_dir.as_deref().unwrap_or_default(), "开发中", None)
        .await
        .expect("set status b");

    let mut form = group_test_form("WMS-GRP-001-aggregation", "联合需求组");
    form.members = Some(vec![
        group_test_member_input("G-MEMBER-A"),
        group_test_member_input("G-MEMBER-B"),
    ]);
    form.release_policy = Some("together".to_string());
    create_requirement(&state, form)
        .await
        .expect("create group");

    let reqs = list_requirements(&state).await.expect("list");
    let group = reqs
        .iter()
        .find(|r| r.id == "WMS-GRP-001-aggregation")
        .expect("group req");
    let members = group.group_members.as_ref().expect("group members");
    assert_eq!(members.len(), 2);
    assert!(members.iter().all(|m| m.found));
    assert_eq!(
        members
            .iter()
            .find(|m| m.req_id == "G-MEMBER-A")
            .and_then(|m| m.status.clone()),
        Some("测试中".to_string())
    );
    // 总进度 = min(成员进度)：A 测试中(3)、B 开发中(1) -> 开发中，瓶颈 B。
    assert_eq!(group.group_status.as_deref(), Some("开发中"));
    assert_eq!(group.group_bottleneck.as_deref(), Some("G-MEMBER-B"));
    assert_eq!(group.group_policy.as_deref(), Some("together"));
    // 成员反向引用所属组。
    let member = reqs
        .iter()
        .find(|r| r.id == "G-MEMBER-A")
        .expect("member a");
    assert!(member
        .member_of
        .iter()
        .any(|g| g == "WMS-GRP-001-aggregation"));
}

#[tokio::test]
async fn create_requirement_group_writes_group_json_and_defaults_policy() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let state = group_test_state(&tmp);
    create_requirement(&state, group_test_form("G-M1", "成员1"))
        .await
        .expect("create member");
    let mut form = group_test_form("WMS-GRP-002", "默认策略组");
    form.members = Some(vec![group_test_member_input("G-M1")]);
    let res = create_requirement(&state, form)
        .await
        .expect("create group");
    assert_eq!(res["group"]["releasePolicy"], json!("independent"));
    let req_dir = res["reqDir"].as_str().expect("reqDir").to_string();
    let raw = std::fs::read_to_string(std::path::Path::new(&req_dir).join(GROUP_FILE))
        .expect("group.json exists");
    let file: GroupFile = serde_json::from_str(&raw).expect("parse group.json");
    assert_eq!(file.release_policy.as_deref(), Some("independent"));
    assert_eq!(file.members.len(), 1);
    assert_eq!(file.members[0].req_id, "G-M1");
}

#[tokio::test]
async fn create_requirement_group_rejects_invalid_members_and_policy() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let state = group_test_state(&tmp);
    create_requirement(&state, group_test_form("G-M2", "成员2"))
        .await
        .expect("create member");

    // 成员不存在。
    let mut missing = group_test_form("WMS-GRP-003", "坏成员组");
    missing.members = Some(vec![group_test_member_input("G-NOPE")]);
    let err = create_requirement(&state, missing)
        .await
        .expect_err("missing member must fail");
    assert!(err.message.contains("需求组成员不存在"), "{:?}", err);

    // 自引用。
    let mut self_ref = group_test_form("WMS-GRP-004", "自引用组");
    self_ref.members = Some(vec![group_test_member_input("WMS-GRP-004")]);
    let err = create_requirement(&state, self_ref)
        .await
        .expect_err("self reference must fail");
    assert!(err.message.contains("自身"), "{:?}", err);

    // 非法 releasePolicy。
    let mut bad_policy = group_test_form("WMS-GRP-005", "坏策略组");
    bad_policy.members = Some(vec![group_test_member_input("G-M2")]);
    bad_policy.release_policy = Some("sometimes".to_string());
    let err = create_requirement(&state, bad_policy)
        .await
        .expect_err("bad policy must fail");
    assert!(err.message.contains("invalid releasePolicy"), "{:?}", err);

    // releasePolicy 未随 members 一起传。
    let mut lone_policy = group_test_form("WMS-GRP-006", "孤立策略");
    lone_policy.release_policy = Some("together".to_string());
    let err = create_requirement(&state, lone_policy)
        .await
        .expect_err("lone policy must fail");
    assert!(err.message.contains("releasePolicy"), "{:?}", err);

    // 组嵌套：成员自身是组。
    let mut inner = group_test_form("WMS-GRP-007", "内层组");
    inner.members = Some(vec![group_test_member_input("G-M2")]);
    create_requirement(&state, inner)
        .await
        .expect("create inner group");
    let mut outer = group_test_form("WMS-GRP-008", "外层组");
    outer.members = Some(vec![group_test_member_input("WMS-GRP-007")]);
    let err = create_requirement(&state, outer)
        .await
        .expect_err("nested group must fail");
    assert!(err.message.contains("组嵌套"), "{:?}", err);
}

#[tokio::test]
async fn validate_requirement_reports_group_json_problems_and_warnings() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let state = group_test_state(&tmp);
    create_requirement(&state, group_test_form("G-M3", "成员3"))
        .await
        .expect("create member");
    let mut form = group_test_form("WMS-GRP-009", "校验组");
    form.members = Some(vec![group_test_member_input("G-M3")]);
    let res = create_requirement(&state, form)
        .await
        .expect("create group");
    let req_dir = res["reqDir"].as_str().expect("reqDir").to_string();
    let dir = std::path::Path::new(&req_dir);

    // 合法 group.json -> ok。
    let req = get_real_requirement(&state, "WMS-GRP-009")
        .await
        .expect("req");
    let ok = validate_requirement(&state, &req).await.expect("validate");
    assert_eq!(ok["ok"], json!(true), "{}", ok);

    // 失效引用 -> warning（不阻塞）。
    std::fs::write(
        dir.join(GROUP_FILE),
        json!({"version": 1, "releasePolicy": "independent", "members": [{"reqId": "G-M3"}, {"reqId": "G-GONE"}]}).to_string(),
    )
    .expect("write stale group.json");
    let stale = validate_requirement(&state, &req)
        .await
        .expect("validate stale");
    assert_eq!(stale["ok"], json!(true));
    assert!(
        stale["warnings"]
            .as_array()
            .expect("warnings")
            .iter()
            .any(|w| w.as_str().unwrap_or("").contains("G-GONE")),
        "{}",
        stale
    );

    // 自引用 + 非法 policy -> problems。
    std::fs::write(
        dir.join(GROUP_FILE),
        json!({"version": 1, "releasePolicy": "maybe", "members": [{"reqId": "WMS-GRP-009"}]})
            .to_string(),
    )
    .expect("write bad group.json");
    let bad = validate_requirement(&state, &req)
        .await
        .expect("validate bad");
    assert_eq!(bad["ok"], json!(false));
    assert!(bad["problems"]
        .as_array()
        .expect("problems")
        .iter()
        .any(|p| p.as_str().unwrap_or("").contains("references itself")));
    assert!(bad["problems"]
        .as_array()
        .expect("problems")
        .iter()
        .any(|p| p.as_str().unwrap_or("").contains("releasePolicy")));

    // 坏 JSON -> problem。
    std::fs::write(dir.join(GROUP_FILE), "{ not json").expect("write broken group.json");
    let broken = validate_requirement(&state, &req)
        .await
        .expect("validate broken");
    assert_eq!(broken["ok"], json!(false));
    assert!(broken["problems"]
        .as_array()
        .expect("problems")
        .iter()
        .any(|p| p.as_str().unwrap_or("").contains("group.json")));
}

#[tokio::test]
async fn group_status_is_locked_against_manual_status_writes() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let state = group_test_state(&tmp);
    create_requirement(&state, group_test_form("G-M4", "成员4"))
        .await
        .expect("create member");
    let mut form = group_test_form("WMS-GRP-010", "只读状态组");
    form.members = Some(vec![group_test_member_input("G-M4")]);
    create_requirement(&state, form)
        .await
        .expect("create group");

    // PATCH / edit setStatus 路径被拦截。
    let err = update_requirement(
        &state,
        RequirementPatchForm {
            req_id: "WMS-GRP-010".to_string(),
            title: None,
            project: None,
            projects: None,
            status: Some("已完成".to_string()),
            category: None,
            source: None,
            owner: None,
            start_date: None,
            plan_release: None,
            ones: None,
            issues: None,
            note: None,
            dry_run: None,
        },
    )
    .await
    .expect_err("group status must be locked");
    assert!(err.message.contains("派生值"), "{:?}", err);

    // 成员不受影响，可以正常推进（组聚合状态随成员变化）。
    let member = get_real_requirement(&state, "G-M4").await.expect("member");
    write_requirement_status(
        member.req_dir.as_deref().unwrap_or_default(),
        "自测中",
        None,
    )
    .await
    .expect("member status ok");
    let reqs = list_requirements(&state).await.expect("list");
    let group = reqs.iter().find(|r| r.id == "WMS-GRP-010").expect("group");
    assert_eq!(group.group_status.as_deref(), Some("自测中"));
}

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

/// 门禁验证详情页 API：实时校验状态 + 按门禁类型的详细内容。
#[tokio::test]
async fn status_gate_detail_reports_state_and_detail() {
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
        "---\nreq-id: T-001\ntitle: 门禁详情测试\nstatus: 自测中\n---\n正文\n",
    )
    .expect("write meta.md");
    let config = json!({
        "requirementScanRoots": [proj.to_string_lossy()],
        "statusGates": [
            { "from": "自测中", "to": "测试中", "gates": ["selftest-checklist"] }
        ]
    });
    std::fs::write(data.join("config.json"), config.to_string()).expect("write config.json");
    let state = temp_app_state(&data, &pi_root, &dsh_root);
    let detail_of = |gate: &str| {
        api_requirement_status_gate_detail(
            State(state.clone()),
            Query(GateDetailQuery {
                id: Some("T-001".into()),
                req_id: None,
                gate: Some(gate.into()),
            }),
        )
    };
    // 自测清单门禁：缺 test.md → failed + problems 非空。
    let v = detail_of("selftest-checklist").await.expect("detail").0;
    assert_eq!(v["state"], json!("failed"));
    assert_eq!(v["label"], json!("自测清单门禁"));
    let problems = v["detail"]["problems"].as_array().expect("problems");
    assert!(!problems.is_empty());
    // 代码审查门禁：无 review 文档 → failed + 门禁动作非空（详情页承接原卡片的门禁动作）。
    let v = detail_of("review").await.expect("detail").0;
    assert_eq!(v["state"], json!("failed"));
    let actions = v["detail"]["actions"].as_array().expect("actions");
    assert!(!actions.is_empty());
    // 未知门禁 id：评估放行（dispatch 兑底语义一致）。
    let v = detail_of("nope").await.expect("detail").0;
    assert_eq!(v["state"], json!("passed"));
}

// ===================== 子需求（sub-requirement）单元测试 =====================

#[test]
fn sub_req_seq_extracts_trailing_number() {
    assert_eq!(sub_req_seq("WMS-049-S1-fix-logging"), Some(1));
    assert_eq!(sub_req_seq("WMS-049-S2"), Some(2));
    assert_eq!(sub_req_seq("WMS-049-S12-x-y"), Some(12));
    assert_eq!(sub_req_seq("WMS-049-wave-pick-task"), None);
    assert_eq!(sub_req_seq("WMS-GRP-001-x"), None);
}

#[test]
fn extract_ticket_prefix_handles_pool_prefixes() {
    assert_eq!(
        extract_ticket_prefix("WMS-049-wave-pick-task"),
        Some("WMS-049".to_string())
    );
    assert_eq!(
        extract_ticket_prefix("WMS-INC-007-foo"),
        Some("WMS-INC-007".to_string())
    );
    assert_eq!(
        extract_ticket_prefix("WMS-TST-012-bar-baz"),
        Some("WMS-TST-012".to_string())
    );
    assert_eq!(extract_ticket_prefix("no-numeric-segment"), None);
}

#[test]
fn sub_branch_name_appends_suffix() {
    assert_eq!(
        sub_branch_name("hevin.yang/feature/WMS-049-wave-pick-task", 1),
        "hevin.yang/feature/WMS-049-wave-pick-task-sub1"
    );
    assert_eq!(sub_branch_name("feature/x-", 3), "feature/x-sub3");
}

fn sub_req_for_test(status: &str) -> Requirement {
    let mut req = default_requirement_for_test("WMS-049-S1-fix-logging");
    req.status = status.to_string();
    req.is_sub_req = true;
    req.parent_req_id = Some("WMS-049-wave-pick-task".to_string());
    req
}

#[test]
fn sub_status_transition_enforces_flow() {
    use requirement_service::ensure_sub_req_status_transition;
    // 正常流转
    assert!(ensure_sub_req_status_transition(&sub_req_for_test("需求创建"), "开发中").is_ok());
    assert!(ensure_sub_req_status_transition(&sub_req_for_test("开发中"), "已合入").is_ok());
    assert!(ensure_sub_req_status_transition(&sub_req_for_test("开发中"), "已取消").is_ok());
    assert!(ensure_sub_req_status_transition(&sub_req_for_test("需求创建"), "已取消").is_ok());
    // 跳过流转拒绝
    assert!(ensure_sub_req_status_transition(&sub_req_for_test("需求创建"), "已合入").is_err());
    // 终态拒绝
    assert!(ensure_sub_req_status_transition(&sub_req_for_test("已合入"), "开发中").is_err());
    assert!(ensure_sub_req_status_transition(&sub_req_for_test("已取消"), "开发中").is_err());
    assert!(ensure_sub_req_status_transition(&sub_req_for_test("已合入"), "已取消").is_err());
    // 子状态集之外拒绝
    assert!(ensure_sub_req_status_transition(&sub_req_for_test("开发中"), "测试中").is_err());
    assert!(ensure_sub_req_status_transition(&sub_req_for_test("开发中"), "已完成").is_err());
    // 同状态 no-op 放行
    assert!(ensure_sub_req_status_transition(&sub_req_for_test("开发中"), "开发中").is_ok());
}

#[test]
fn non_sub_req_cannot_use_sub_only_statuses() {
    use requirement_service::ensure_status_allowed_for_non_sub;
    assert!(ensure_status_allowed_for_non_sub("需求创建").is_err());
    assert!(ensure_status_allowed_for_non_sub("已合入").is_err());
    assert!(ensure_status_allowed_for_non_sub("已取消").is_err());
    assert!(ensure_status_allowed_for_non_sub("开发中").is_ok());
    assert!(ensure_status_allowed_for_non_sub("排查中").is_ok());
}

#[test]
fn build_meta_doc_writes_parent_req_id_frontmatter() {
    let meta = build_meta_doc(
        "WMS-049-S1-fix-logging",
        "t",
        "需求创建",
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
        Some("WMS-049-wave-pick-task"),
    );
    assert!(meta.contains("parent-req-id: WMS-049-wave-pick-task"));
    assert!(meta.contains("- Parent requirement: WMS-049-wave-pick-task"));
    let without = build_meta_doc(
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
    assert!(!without.contains("parent-req-id"));
}

#[test]
fn snapshot_note_mentions_parent_and_independence() {
    let note = snapshot_note("WMS-049-wave-pick-task", "background.md");
    assert!(note.contains("快照自父需求 `WMS-049-wave-pick-task`"));
    assert!(note.contains("background.md"));
    assert!(note.contains("独立维护"));
}

/// 子需求端到端（进程内直调 handler）：创建 → 文档快照复制 → 列表过滤 → 父子回填 →
/// 子需求轻量状态机 → 字段边界（ONES/plan-release/issues/类别）。
#[tokio::test]
async fn sub_requirement_create_flow_end_to_end() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let data = tmp.path().join("data");
    let pi_root = tmp.path().join("pi-sessions");
    let dsh_root = tmp.path().join("dsh-sessions");
    std::fs::create_dir_all(&data).expect("create data dir");
    let proj = tmp.path().join("proj");
    let parent_dir = proj.join("req").join("WMS-049-wave-pick-task");
    std::fs::create_dir_all(&parent_dir).expect("create parent req dir");
    std::fs::write(
        parent_dir.join("meta.md"),
        "---\nreq-id: WMS-049-wave-pick-task\ntitle: 波次拣选任务\nstatus: 开发中\nproject: WMS\ncategory: 需求\nsource: 产品推动\nowner: hevin\nstart-date: 2026-01-01\nplan-release: unknown\n---\n父需求正文\n",
    )
    .expect("write parent meta.md");
    std::fs::write(
        parent_dir.join("state.json"),
        json!({"version": 1, "status": "开发中", "category": "需求", "history": []}).to_string(),
    )
    .expect("write parent state.json");
    std::fs::write(
        parent_dir.join("background.md"),
        "# 波次拣选背景\n- 业务目标\n",
    )
    .expect("write background");
    std::fs::write(parent_dir.join("technical-plan.md"), "# 技术方案\n- 步骤\n")
        .expect("write technical-plan");
    let config = json!({ "requirementScanRoots": [proj.to_string_lossy()] });
    std::fs::write(data.join("config.json"), config.to_string()).expect("write config.json");
    let state = temp_app_state(&data, &pi_root, &dsh_root);

    // 1) 创建子需求（dry-run 预览不落盘）。
    let preview = api_requirement_create_sub(
        State(state.clone()),
        FormOrJson(SubRequirementCreateForm {
            parent_req_id: "WMS-049-wave-pick-task".into(),
            title: "修复拣选日志".into(),
            slug: Some("fix-logging".into()),
            owner: None,
            summary: Some("负责日志改造".into()),
            dry_run: Some(true),
        }),
    )
    .await
    .expect("dry-run create sub");
    assert_eq!(preview["reqId"], json!("WMS-049-S1-fix-logging"));
    assert_eq!(preview["parentReqId"], json!("WMS-049-wave-pick-task"));

    // 2) 真实创建：ID 分配 + 文档快照复制 + parent-req-id frontmatter。
    let created = api_requirement_create_sub(
        State(state.clone()),
        FormOrJson(SubRequirementCreateForm {
            parent_req_id: "WMS-049-wave-pick-task".into(),
            title: "修复拣选日志".into(),
            slug: Some("fix-logging".into()),
            owner: None,
            summary: Some("负责日志改造".into()),
            dry_run: Some(false),
        }),
    )
    .await
    .expect("create sub");
    assert_eq!(created["reqId"], json!("WMS-049-S1-fix-logging"));
    let sub_dir = PathBuf::from(created["reqDir"].as_str().expect("reqDir"));
    let sub_meta = std::fs::read_to_string(sub_dir.join("meta.md")).expect("read sub meta");
    assert!(sub_meta.contains("parent-req-id: WMS-049-wave-pick-task"));
    assert!(sub_meta.contains("status: 需求创建"));
    assert!(!sub_meta.contains("ones:"));
    let sub_background =
        std::fs::read_to_string(sub_dir.join("background.md")).expect("read sub background");
    assert!(sub_background.contains("快照自父需求 `WMS-049-wave-pick-task`"));
    assert!(sub_background.contains("波次拣选背景"));
    // 父需求 notes 留痕
    let parent_notes = std::fs::read_to_string(parent_dir.join("notes.md")).expect("parent notes");
    assert!(parent_notes.contains("拆分子需求：`WMS-049-S1-fix-logging`"));

    // 3) 列表过滤：默认不含子需求；includeSubs=true 包含；父需求 subReqs 回填。
    let list_default = api_requirements(State(state.clone()), Query(IdQuery::default()))
        .await
        .expect("default list");
    let ids_default: Vec<&str> = list_default.0["requirements"]
        .as_array()
        .expect("arr")
        .iter()
        .filter_map(|r| r["id"].as_str())
        .collect();
    assert!(!ids_default.contains(&"WMS-049-S1-fix-logging"));
    let list_all = api_requirements(
        State(state.clone()),
        Query(IdQuery {
            include_subs: Some(true),
            ..Default::default()
        }),
    )
    .await
    .expect("all list");
    let subs_of_parent: Vec<Value> = list_all.0["requirements"]
        .as_array()
        .expect("arr")
        .iter()
        .find(|r| r["id"] == json!("WMS-049-wave-pick-task"))
        .expect("parent")
        .get("subReqs")
        .expect("subReqs")
        .as_array()
        .expect("subReqs")
        .clone();
    assert_eq!(subs_of_parent.len(), 1);
    assert_eq!(subs_of_parent[0]["reqId"], json!("WMS-049-S1-fix-logging"));
    assert_eq!(subs_of_parent[0]["status"], json!("需求创建"));
    assert_eq!(subs_of_parent[0]["found"], json!(true));

    // 4) 子需求轻量状态机：跳过流转拒绝（via=ui 也不放行），正常流转放行。
    let status_form = |status: &str, via: Option<String>| {
        FormOrJson(StatusForm {
            req_id: "WMS-049-S1-fix-logging".into(),
            status: status.into(),
            note: None,
            via,
        })
    };
    let skipped = api_requirement_status(
        State(state.clone()),
        status_form("已合入", Some("ui".into())),
    )
    .await
    .expect_err("需求创建 -> 已合入 must be rejected even via=ui");
    assert!(format!("{:?}", skipped).contains("不允许"));
    let _ = api_requirement_status(
        State(state.clone()),
        status_form("开发中", Some("ui".into())),
    )
    .await
    .expect("需求创建 -> 开发中");
    let _ = api_requirement_status(State(state.clone()), status_form("已合入", None))
        .await
        .expect("开发中 -> 已合入");
    let terminal = api_requirement_status(State(state.clone()), status_form("开发中", None))
        .await
        .expect_err("terminal status must not transition");
    assert!(format!("{:?}", terminal).contains("终态"));

    // 5) 普通需求不能用子需求专属状态；子需求不能绑 ONES / 切类别。
    let normal_blocked = api_requirement_status(
        State(state.clone()),
        FormOrJson(StatusForm {
            req_id: "WMS-049-wave-pick-task".into(),
            status: "需求创建".into(),
            note: None,
            via: Some("ui".into()),
        }),
    )
    .await
    .expect_err("normal req must not use sub-only status");
    assert!(format!("{:?}", normal_blocked).contains("子需求专属状态"));
    let ones_blocked = api_requirement_update(
        State(state.clone()),
        FormOrJson(RequirementPatchForm {
            req_id: "WMS-049-S1-fix-logging".into(),
            title: None,
            project: None,
            projects: None,
            status: None,
            category: None,
            source: None,
            owner: None,
            start_date: None,
            plan_release: None,
            ones: Some("https://ones.example/issue".into()),
            issues: None,
            note: None,
            dry_run: None,
        }),
    )
    .await
    .expect_err("sub must not bind ones");
    assert!(format!("{:?}", ones_blocked).contains("父需求承载"));

    // 6) 守卫：不能给子需求再拆子需求（只允许一层）。
    let nested = api_requirement_create_sub(
        State(state.clone()),
        FormOrJson(SubRequirementCreateForm {
            parent_req_id: "WMS-049-S1-fix-logging".into(),
            title: "嵌套子需求".into(),
            slug: None,
            owner: None,
            summary: None,
            dry_run: Some(true),
        }),
    )
    .await
    .expect_err("nested sub must be rejected");
    assert!(format!("{:?}", nested).contains("不支持子需求嵌套"));
}

// ===================== 文档分册（doc parts）单元测试 =====================

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

#[tokio::test]
async fn diff_patch_export_writes_multiline_companion() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let req_dir = tmp.path().to_path_buf();
    let review = json!({
        "repos": [
            {"repoName": "repo-a", "branch": "feat/x", "diff": "diff --git a/f b/f\n@@ -1,2 +1,3 @@\n context\n-old\n+new\n"},
            {"repoName": "repo-b", "branch": "", "diff": ""},
        ]
    });
    let written =
        crate::git_workflow::export_diff_patch_file(&req_dir, CODE_REVIEW_PATCH_FILE, &review)
            .await
            .expect("export");
    assert!(written);
    let text = std::fs::read_to_string(req_dir.join(CODE_REVIEW_PATCH_FILE)).expect("read patch");
    assert!(text.starts_with("# ==== repo: repo-a (branch: feat/x) ====\n"));
    // patch 文件里的换行必须是真实换行（多行文本），而非 JSON 里的 \n 字面量单行。
    assert!(text.contains("\n-old\n+new\n"), "patch 应按真实换行落盘");
    assert!(!text.contains("repo-b"), "空 diff 仓库不产生段落");

    // 空快照：返回 false 并清掉旧 patch，避免材料引用与内容不一致。
    let empty = json!({"repos": [{"repoName": "repo-c", "branch": "", "diff": ""}]});
    std::fs::write(req_dir.join(CODE_REVIEW_PATCH_FILE), "stale").expect("write stale");
    let written =
        crate::git_workflow::export_diff_patch_file(&req_dir, CODE_REVIEW_PATCH_FILE, &empty)
            .await
            .expect("export empty");
    assert!(!written);
    assert!(
        !req_dir.join(CODE_REVIEW_PATCH_FILE).exists(),
        "空快照应清掉旧 patch 文件"
    );
}

#[tokio::test]
async fn review_checklist_put_validates_and_renders_review_md() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let data = tmp.path().join("data");
    let pi_root = tmp.path().join("pi-sessions");
    let dsh_root = tmp.path().join("dsh-sessions");
    std::fs::create_dir_all(&data).expect("create data dir");
    let proj = tmp.path().join("proj");
    let req_dir = proj.join("req").join("T-900");
    std::fs::create_dir_all(&req_dir).expect("create req dir");
    std::fs::write(
        req_dir.join("meta.md"),
        "---\nreq-id: T-900\ntitle: 审查清单测试\nstatus: 自测中\n---\n正文\n",
    )
    .expect("write meta.md");
    let config = json!({ "requirementScanRoots": [proj.to_string_lossy()] });
    std::fs::write(data.join("config.json"), config.to_string()).expect("write config.json");
    let state = temp_app_state(&data, &pi_root, &dsh_root);

    // 1) conclusion 非法 → 400 且错误信息教格式
    let bad = api_requirement_review_checklist_put(
        State(state.clone()),
        FormOrJson(ReviewChecklistSaveForm {
            req_id: "T-900".into(),
            items: Some(json!([{ "id": "C1", "title": "幂等", "conclusion": "todo" }])),
        }),
    )
    .await;
    let err = bad.expect_err("invalid conclusion must be rejected");
    assert!(err.message.contains("只允许 pass / fail / na"), "{}", err.message);
    assert!(err.message.contains("items"), "错误信息应包含 schema 提示");

    // 2) 合法保存 → 写 review-checklist.json + review.md 受管小节
    let ok = api_requirement_review_checklist_put(
        State(state.clone()),
        FormOrJson(ReviewChecklistSaveForm {
            req_id: "T-900".into(),
            items: Some(json!([
                { "id": "C1", "title": "幂等与并发安全", "conclusion": "PASS", "note": "锁内复查" },
                { "id": "C2", "title": "部署顺序", "conclusion": "na" }
            ])),
        }),
    )
    .await
    .expect("valid save");
    assert_eq!(ok.0["ok"], json!(true));
    let saved = std::fs::read_to_string(req_dir.join(REVIEW_CHECKLIST_FILE)).expect("checklist file");
    assert!(saved.contains("\"conclusion\": \"pass\""), "conclusion 归一化为小写: {saved}");
    let review_md = std::fs::read_to_string(req_dir.join("review.md")).expect("review.md");
    assert!(review_md.contains("<!-- panel:review-checklist:start -->"));
    assert!(review_md.contains("✅ pass"));
    assert!(review_md.contains("➖ na"));

    // 3) 重复保存 → 受管块替换而非叠加
    let _ = api_requirement_review_checklist_put(
        State(state.clone()),
        FormOrJson(ReviewChecklistSaveForm {
            req_id: "T-900".into(),
            items: Some(json!([{ "title": "新要点", "conclusion": "fail", "note": "发现问题" }])),
        }),
    )
    .await
    .expect("resave");
    let review_md = std::fs::read_to_string(req_dir.join("review.md")).expect("review.md v2");
    assert_eq!(
        review_md.matches("<!-- panel:review-checklist:start -->").count(),
        1,
        "受管块只保留一份"
    );
    assert!(review_md.contains("❌ fail"));
    // 缺 id 自动编号 C1
    let saved = std::fs::read_to_string(req_dir.join(REVIEW_CHECKLIST_FILE)).expect("checklist v2");
    assert!(saved.contains("\"id\": \"C1\""));
}

#[tokio::test]
async fn review_gate_requires_completed_checklist() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let data = tmp.path().join("data");
    let pi_root = tmp.path().join("pi-sessions");
    let dsh_root = tmp.path().join("dsh-sessions");
    std::fs::create_dir_all(&data).expect("create data dir");
    let proj = tmp.path().join("proj");
    let req_dir = proj.join("req").join("T-901");
    std::fs::create_dir_all(&req_dir).expect("create req dir");
    std::fs::write(
        req_dir.join("meta.md"),
        "---\nreq-id: T-901\ntitle: 门禁清单校验\nstatus: 自测中\n---\n正文\n",
    )
    .expect("write meta.md");
    let config = json!({ "requirementScanRoots": [proj.to_string_lossy()] });
    std::fs::write(data.join("config.json"), config.to_string()).expect("write config.json");
    std::fs::write(
        req_dir.join("review.md"),
        "# T-901 审查\n\nReview Gate: PASS\n",
    )
    .expect("write review.md");
    let mut req = default_requirement(Vec::new());
    req.id = "T-901".to_string();
    req.title = "门禁清单校验".to_string();
    req.status = "自测中".to_string();
    req.req_dir = Some(req_dir.to_string_lossy().to_string());

    // 1) 无清单 → checklist-missing（即使写了 PASS 也不放行）
    let decision = review_gate_decision(&req).await.expect("decision");
    assert_eq!(decision.status, "checklist-missing");
    assert!(!decision.allows_testing);

    // 2) 清单有 fail 项 → blocked
    std::fs::write(
        req_dir.join(REVIEW_CHECKLIST_FILE),
        json!({
            "version": 1, "reqId": "T-901",
            "items": [
                { "id": "C1", "title": "幂等", "conclusion": "pass" },
                { "id": "C2", "title": "部署顺序", "conclusion": "fail" }
            ]
        })
        .to_string(),
    )
    .expect("write checklist");
    let decision = review_gate_decision(&req).await.expect("decision");
    assert_eq!(decision.status, "blocked", "{}", decision.reason);
    assert!(decision.reason.contains("C2"), "{}", decision.reason);

    // 3) 全部 pass/na + PASS 标记 → passed
    std::fs::write(
        req_dir.join(REVIEW_CHECKLIST_FILE),
        json!({
            "version": 1, "reqId": "T-901",
            "items": [
                { "id": "C1", "title": "幂等", "conclusion": "pass" },
                { "id": "C2", "title": "部署顺序", "conclusion": "na", "note": "单仓" }
            ]
        })
        .to_string(),
    )
    .expect("write checklist v2");
    let decision = review_gate_decision(&req).await.expect("decision");
    assert_eq!(decision.status, "passed", "{}", decision.reason);
    assert!(decision.allows_testing);

    // 4) 清单 JSON 损坏 → checklist-error
    std::fs::write(req_dir.join(REVIEW_CHECKLIST_FILE), "{oops").expect("corrupt");
    let decision = review_gate_decision(&req).await.expect("decision");
    assert_eq!(decision.status, "checklist-error");
    assert!(!decision.allows_testing);
}

#[tokio::test]
async fn selftest_gate_waived_for_hotfix_requirements() {
    // 抢修模式（issues 非空）：test.md 缺失也放行
    let mut hotfix = default_requirement(Vec::new());
    hotfix.id = "T-902".to_string();
    hotfix.req_dir = Some("/nonexistent-t-902".to_string());
    hotfix.issues = vec!["WMS-999-hotfix-source".to_string()];
    assert!(hotfix.is_hotfix());
    assert!(selftest_checklist_problems(&hotfix).await.is_empty());

    // 普通需求：test.md 缺失 → 仍拦截
    let mut normal = default_requirement(Vec::new());
    normal.id = "T-903".to_string();
    normal.req_dir = Some("/nonexistent-t-903".to_string());
    assert!(!normal.is_hotfix());
    assert!(selftest_checklist_problems(&normal).await.is_empty() == false);
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
