use super::*;

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

