use super::*;

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
fn should_auto_advance_issues_only_for_experience_or_later() {
    assert!(should_auto_advance_issues("经验总结"));
    assert!(should_auto_advance_issues("发布就绪"));
    assert!(should_auto_advance_issues("已完成"));
    assert!(!should_auto_advance_issues("测试中"));
    assert!(!should_auto_advance_issues("开发中"));
    assert!(!should_auto_advance_issues("已定位"));
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


