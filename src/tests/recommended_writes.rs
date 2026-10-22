use super::*;

/// recommendedWrites 与 entry checks 联动（2026-10-22 反馈：Entry Checks 缺
/// reproduce.md 时推荐里反而没有它）+ when 时机标注 + 已定位专属推荐集。
#[test]
fn recommended_writes_link_missing_entry_checks() {
    // missing required 检查 → 置顶 PUT file 建议
    let missing = vec![json!({
        "id": "reproduce-md",
        "label": "复现文档：复现步骤 + 预期判定",
        "required": true,
        "ok": false,
        "status": "missing",
        "source": "reproduce.md",
    })];
    let writes = crate::recommended_requirement_writes("issue-located", &missing);
    assert!(!writes.is_empty());
    let top = &writes[0];
    assert_eq!(top["method"], "PUT");
    assert_eq!(top["path"], "/api/requirement/file");
    assert_eq!(top["body"]["path"], "reproduce.md");
    assert!(
        top["when"].as_str().unwrap_or("").contains("优先补齐"),
        "missing-driven item should be first with when hint"
    );

    // 已满足的检查不进推荐
    let ok_check = vec![json!({
        "id": "root-cause-md", "label": "根因", "required": true,
        "ok": true, "status": "ok", "source": "root-cause.md",
    })];
    let writes_ok = crate::recommended_requirement_writes("issue-located", &ok_check);
    assert!(
        !writes_ok.iter().any(|w| {
            w["body"]["path"] == "root-cause.md"
                && w["when"].as_str().unwrap_or("").contains("优先补齐")
        }),
        "satisfied checks must not appear as missing-driven recommendations"
    );

    // 已定位专属静态推荐含 rootCause 事件
    assert!(
        writes.iter().any(|w| {
            w["path"] == "/api/requirement/events" && w["body"]["type"] == "rootCause"
        }),
        "issue-located should recommend rootCause event"
    );

    // 每条推荐都带 when 时机说明
    for w in &writes {
        assert!(
            w.get("when")
                .and_then(Value::as_str)
                .map(|v| !v.is_empty())
                .unwrap_or(false),
            "every recommendation carries a when hint: {w}"
        );
    }

    // overview 兜底：同 path 多条按 type 区分，无完全重复
    let plain = crate::recommended_requirement_writes("overview", &[]);
    let keys: std::collections::HashSet<String> = plain
        .iter()
        .map(|w| {
            format!(
                "{}|{}|{}",
                w["method"].as_str().unwrap_or(""),
                w["path"].as_str().unwrap_or(""),
                w["body"]["type"].as_str().unwrap_or("")
            )
        })
        .collect();
    assert_eq!(
        keys.len(),
        plain.len(),
        "no duplicate method|path|type entries"
    );
}
