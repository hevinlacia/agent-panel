use super::*;

/// 全部通过且证据齐备的 UAT 回归小节。
fn uat_all_pass_body() -> String {
    "# T-001 Test\n\n## UAT 回归\n\n| # | 场景 | 结果 | 证据 |\n| --- | --- | --- | --- |\n| 1 | 混合明细任务确认 | 通过 | UAT tid=abc123；DB 头 900、缺货明细 700 保留 |\n| 2 | 取消明细行展示 | 通过 | 接口返回 code=0；详情页截图核对 |\n"
        .to_string()
}

#[test]
fn uat_missing_section_blocks() {
    let (eval, na, items) = validate_uat_regression("# T Test\n\n## 自测清单\n\n| # | 自测项 | 结果 |\n| --- | --- | --- |\n| 1 | x | 通过 |\n");
    assert!(eval.problems.iter().any(|p| p.contains("未找到")));
    assert!(eval.warnings.is_empty());
    assert!(na.is_none());
    assert!(items.is_empty());
}

#[test]
fn uat_all_pass_with_real_evidence_releases() {
    let (eval, na, items) = validate_uat_regression(&uat_all_pass_body());
    assert!(eval.problems.is_empty(), "{:?}", eval.problems);
    assert!(eval.warnings.is_empty());
    assert!(na.is_none());
    assert_eq!(items.len(), 2);
    assert_eq!(items[0].result, "pass");
    assert!(items[0].evidence.as_deref().unwrap_or_default().contains("tid"));
}

#[test]
fn uat_pass_without_evidence_blocks() {
    let body = "# T Test\n\n## UAT 回归\n\n| # | 场景 | 结果 | 证据 |\n| --- | --- | --- | --- |\n| 1 | 混合任务确认 | 通过 | - |\n";
    let (eval, _, _) = validate_uat_regression(body);
    assert!(
        eval.problems.iter().any(|p| p.contains("证据列为空/占位")),
        "{:?}",
        eval.problems
    );
}

#[test]
fn uat_pass_with_plan_like_evidence_blocks() {
    // WMS-136 实测反例：把状态流转动作当 UAT 回归证据。
    let body = "# T Test\n\n## UAT 回归\n\n| # | 场景 | 结果 | 证据 |\n| --- | --- | --- | --- |\n| 1 | 混合任务确认 | 通过 | 用户手工回归后推进发布就绪 |\n";
    let (eval, _, _) = validate_uat_regression(body);
    assert!(
        eval.problems
            .iter()
            .any(|p| p.contains("计划/流程动作") && p.contains("后推进")),
        "{:?}",
        eval.problems
    );
}

#[test]
fn uat_fail_row_blocks_with_or_without_reason() {
    let with_reason = "# T Test\n\n## UAT 回归\n\n| # | 场景 | 结果 | 证据 |\n| --- | --- | --- | --- |\n| 1 | 混合任务确认 | 失败 | 缺货明细被置 800，DB 证据 tid=x |\n";
    let (eval, _, _) = validate_uat_regression(with_reason);
    assert!(eval.problems.iter().any(|p| p.contains("失败项")));
    let without_reason = "# T Test\n\n## UAT 回归\n\n| # | 场景 | 结果 | 证据 |\n| --- | --- | --- | --- |\n| 1 | 混合任务确认 | 失败 | - |\n";
    let (eval2, _, _) = validate_uat_regression(without_reason);
    assert!(eval2.problems.iter().any(|p| p.contains("写明具体的失败原因")));
}

#[test]
fn uat_cannot_with_reason_warns_but_releases() {
    let body = "# T Test\n\n## UAT 回归\n\n| # | 场景 | 结果 | 证据 |\n| --- | --- | --- | --- |\n| 1 | 取消明细行展示 | 无法测试 | UAT 无整单取消入口，由测试同事人工覆盖 |\n";
    let (eval, _, items) = validate_uat_regression(body);
    assert!(eval.problems.is_empty(), "{:?}", eval.problems);
    assert!(eval.warnings.iter().any(|w| w.contains("无法测试")));
    assert_eq!(items[0].result, "cannot");
}

#[test]
fn uat_cannot_without_reason_blocks() {
    let body = "# T Test\n\n## UAT 回归\n\n| # | 场景 | 结果 | 证据 |\n| --- | --- | --- | --- |\n| 1 | 取消明细行展示 | 无法测试 | - |\n";
    let (eval, _, _) = validate_uat_regression(body);
    assert!(eval.problems.iter().any(|p| p.contains("写明具体原因")));
}

#[test]
fn uat_not_executed_blocks_with_specific_hint() {
    let body = "# T Test\n\n## UAT 回归\n\n| # | 场景 | 结果 | 证据 |\n| --- | --- | --- | --- |\n| 1 | 待补充场景 | 未执行 | - |\n";
    let (eval, _, _) = validate_uat_regression(body);
    assert!(
        eval.problems.iter().any(|p| p.contains("未执行")),
        "{:?}",
        eval.problems
    );
}

#[test]
fn uat_whole_section_not_applicable_releases_with_warning() {
    let body = "# T Test\n\n## UAT 回归\n\n不适用：纯文案与样式调整，无接口与数据行为变化\n\n| # | 场景 | 结果 | 证据 |\n| --- | --- | --- | --- |\n| 1 | x | 通过 | y |\n";
    let (eval, na, _) = validate_uat_regression(body);
    assert!(eval.problems.is_empty(), "{:?}", eval.problems);
    assert!(eval.warnings.iter().any(|w| w.contains("不适用")));
    assert_eq!(na.as_deref(), Some("纯文案与样式调整，无接口与数据行为变化"));
}

#[test]
fn uat_heading_variant_and_missing_evidence_column() {
    // 「UAT 回归记录」旧标题也能命中。
    let body = "# T Test\n\n## UAT 回归记录\n\n| # | 场景 | 结果 | 证据 |\n| --- | --- | --- | --- |\n| 1 | 场景A | 通过 | DB 核对通过 |\n";
    let (eval, _, _) = validate_uat_regression(body);
    assert!(eval.problems.is_empty(), "{:?}", eval.problems);
    // 表头缺「证据」列 → 拦截。
    let no_ev = "# T Test\n\n## UAT 回归\n\n| # | 场景 | 结果 |\n| --- | --- | --- |\n| 1 | 场景A | 通过 |\n";
    let (eval2, _, _) = validate_uat_regression(no_ev);
    assert!(eval2.problems.iter().any(|p| p.contains("证据」列")));
}

#[test]
fn uat_table_without_rows_blocks() {
    let body = "# T Test\n\n## UAT 回归\n\n| # | 场景 | 结果 | 证据 |\n| --- | --- | --- | --- |\n";
    let (eval, _, _) = validate_uat_regression(body);
    assert!(eval.problems.iter().any(|p| p.contains("没有任何回归条目")));
}
