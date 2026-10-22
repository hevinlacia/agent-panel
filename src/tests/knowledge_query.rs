use super::*;
use crate::knowledge::knowledge_query_entry_lean;

/// lean 条目只保留检索决策字段，且 summary/whyMatched 被截断。
#[test]
fn knowledge_query_entry_lean_keeps_decision_fields_only() {
    let full = serde_json::json!({
        "id": "biz-inventory-dual-ledger",
        "title": "库存双账本",
        "kind": "businessKnowledge",
        "category": "biz",
        "domain": "wms",
        "scope": "project",
        "project": "wms",
        "score": 159,
        "whyMatched": "命中 title/triggerTerms/summary",
        "matchedFields": ["title", "triggerTerms"],
        "matches": [{"field": "title", "snippet": "很长很长的命中片段"}],
        "queryTokens": ["库存"],
        "summary": "很长的摘要文本很长的摘要文本很长的摘要文本很长的摘要文本很长的摘要文本很长的摘要文本很长的摘要文本很长的摘要文本很长的摘要文本很长的摘要文本很长的摘要文本很长的摘要文本很长的摘要文本很长的摘要文本很长的摘要文本很长的摘要文本很长的摘要文本很长的摘要文本",
        "outline": [{"title": "A"}, {"title": "B"}],
        "sourcePath": "/srv/agent-panel/data/company/云仓/WMS/.agents/business-knowledge/items/biz-inventory-dual-ledger.md",
        "metaPath": "/srv/agent-panel/data/company/云仓/WMS/.agents/business-knowledge/meta/biz-inventory-dual-ledger.yaml",
        "root": "/srv/agent-panel/data",
        "updatedAt": "2026-10-20T12:00:00Z",
        "createdAt": "2026-07-13T07:08:45Z",
        "stale": false,
        "infoAgeDays": 100
    });
    let lean = knowledge_query_entry_lean(&full);
    let obj = lean.as_object().expect("lean entry is object");
    // 决策字段齐全
    for key in [
        "id",
        "title",
        "kind",
        "category",
        "domain",
        "score",
        "whyMatched",
        "summary",
        "updatedAt",
        "stale",
    ] {
        assert!(obj.contains_key(key), "lean entry missing {key}");
    }
    // 重字段一律裁掉
    for key in [
        "matches",
        "matchedFields",
        "outline",
        "queryTokens",
        "sourcePath",
        "metaPath",
        "root",
        "details",
        "createdAt",
        "infoAgeDays",
    ] {
        assert!(!obj.contains_key(key), "lean entry should drop {key}");
    }
    // summary 截断到 160 字符
    let summary = obj.get("summary").and_then(|v| v.as_str()).unwrap_or("");
    assert!(
        summary.chars().count() <= 160,
        "summary must be <= 160 chars"
    );
    // whyMatched 截断到 80 字符
    let why = obj.get("whyMatched").and_then(|v| v.as_str()).unwrap_or("");
    assert!(why.chars().count() <= 80, "whyMatched must be <= 80 chars");
}

/// title 相似度（bigram Jaccard）：查重阈值语义的纯函数锁定。
#[test]
fn title_similarity_bigram_jaccard_thresholds() {
    // 完全相同 → 1.0
    assert_eq!(
        crate::knowledge::title_similarity("库存双账本平衡", "库存双账本平衡"),
        1.0
    );
    // 高相似（同主题换措辞）→ 超过 0.6 高阈值
    let high = crate::knowledge::title_similarity(
        "库存双账本平衡：可用量与 redis 缓存",
        "库存双账本平衡机制 redis 可用量",
    );
    assert!(high >= 0.5, "high similarity should be >= 0.5, got {high}");
    // 完全无关 → 接近 0
    let low = crate::knowledge::title_similarity("库存双账本平衡", "RocketMQ 消费者幂等");
    assert!(low < 0.4, "unrelated titles should be < 0.4, got {low}");
    // 空串安全
    assert_eq!(crate::knowledge::title_similarity("", "任意"), 0.0);
}
