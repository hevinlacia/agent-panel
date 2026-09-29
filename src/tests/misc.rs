use super::*;
use std::collections::BTreeMap;

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
    assert_eq!(normalized["safety"]["agentPanelExecutes"], true);
    assert_eq!(
        normalized["safety"]["executionEndpoint"],
        "/api/testdata/run"
    );
    assert_eq!(normalized["verification"]["targets"][0]["name"], "shipped");
    assert_eq!(
        normalized["relatedArtifacts"]["recipe"],
        "recipes/outbound/create-any-status-shipment.yaml"
    );
    assert_eq!(normalized["legacy"]["domain"], "outbound");
}

#[test]
fn testdata_run_env_allowlist_accepts_known_and_rejects_unknown() {
    assert_eq!(ensure_testdata_env("test").unwrap(), "test");
    assert_eq!(ensure_testdata_env(" uat-cn ").unwrap(), "uat-cn");
    assert_eq!(ensure_testdata_env("uat-sea").unwrap(), "uat-sea");
    assert!(ensure_testdata_env("prod").is_err());
    assert!(ensure_testdata_env("TEST").is_err());
    assert!(ensure_testdata_env("").is_err());
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

#[test]
fn normalize_skill_path_overrides_trims_expands_home_and_drops_empty() {
    let map = BTreeMap::from([
        (
            "  req-tracker  ".to_string(),
            "  /tmp/skills/  ".to_string(),
        ),
        (
            "/agent-panel-code-review/".to_string(),
            "~/.agents/skills-x".to_string(),
        ),
        ("wms-test-data-creation".to_string(), "   ".to_string()),
        ("".to_string(), "/tmp/orphan".to_string()),
    ]);
    let normalized = normalize_skill_path_overrides(map);
    assert_eq!(normalized.get("req-tracker").unwrap(), "/tmp/skills");
    let home = util::home_dir().unwrap().to_string_lossy().to_string();
    assert_eq!(
        normalized.get("agent-panel-code-review").unwrap(),
        &format!("{home}/.agents/skills-x")
    );
    // 空 value / 空 key 被剔除。
    assert!(!normalized.contains_key("wms-test-data-creation"));
    assert_eq!(normalized.len(), 2);
}

#[test]
fn resolve_skill_path_prefers_override_and_supports_three_shapes() {
    let tmp = tempfile::tempdir().unwrap();
    // 姿势 1：override 直接指向 SKILL.md 文件。
    let file_root = tmp.path().join("as-file");
    std::fs::create_dir_all(&file_root).unwrap();
    let file_skill = file_root.join("SKILL.md");
    std::fs::write(&file_skill, "# file skill").unwrap();
    // 姿势 2：override 指向 skills 根（下有同名子目录）。
    let root = tmp.path().join("skills-root");
    std::fs::create_dir_all(root.join("req-tracker")).unwrap();
    std::fs::write(root.join("req-tracker").join("SKILL.md"), "# root skill").unwrap();
    // 姿势 3：override 指向具体 skill 目录（目录内直接放 SKILL.md）。
    let dir = tmp.path().join("skill-dir");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("SKILL.md"), "# dir skill").unwrap();

    let overrides = BTreeMap::from([
        (
            "agent-panel-code-review".to_string(),
            file_skill.to_string_lossy().to_string(),
        ),
        (
            "req-tracker".to_string(),
            root.to_string_lossy().to_string(),
        ),
        (
            "wms-test-data-creation".to_string(),
            dir.to_string_lossy().to_string(),
        ),
        (
            "missing-skill".to_string(),
            tmp.path().join("nope").to_string_lossy().to_string(),
        ),
    ]);
    // 三种配置姿势都解析出存在的 SKILL.md。
    assert_eq!(
        resolve_skill_path_with(&overrides, "agent-panel-code-review"),
        file_skill.to_string_lossy()
    );
    assert_eq!(
        resolve_skill_path_with(&overrides, "req-tracker"),
        root.join("req-tracker").join("SKILL.md").to_string_lossy()
    );
    assert_eq!(
        resolve_skill_path_with(&overrides, "wms-test-data-creation"),
        dir.join("SKILL.md").to_string_lossy()
    );
    // override 命中后不回退默认链：目录不存在时按“skill 自身目录”解析，
    // 不存在的路径原样返回，由检查 API 暴露。
    assert_eq!(
        resolve_skill_path_with(&overrides, "missing-skill"),
        tmp.path().join("nope").join("SKILL.md").to_string_lossy()
    );
    // 未配置 override 的 skill 走默认链（cwd / WMS 工作区）。
    let empty = BTreeMap::new();
    assert!(resolve_skill_path_with(&empty, "req-tracker").ends_with("SKILL.md"));
}
