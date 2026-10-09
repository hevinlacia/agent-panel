use super::*;

/// issue 家族状态机收紧：线上问题/测试问题只允许轻流程状态（+挂起），拒绝常规需求流状态；
/// 普通需求不受影响。修复进展由关联 FIX 需求承载，问题侧不进入需求流状态机。
#[test]
fn ensure_status_for_category_rejects_req_flow_status_for_issues() {
    // 轻流程 5 状态 + 挂起：放行
    for status in ["排查中", "已定位", "已修复", "已复盘", "已关闭", "挂起"] {
        assert!(
            ensure_status_for_category(Some("线上问题"), status).is_ok(),
            "线上问题应允许 {status}"
        );
        assert!(
            ensure_status_for_category(Some("测试问题"), status).is_ok(),
            "测试问题应允许 {status}"
        );
    }
    // 常规需求流状态：拒绝
    for status in ["需求澄清", "开发中", "自测中", "测试中", "人工核查", "发布就绪", "经验总结", "已完成"] {
        let err = ensure_status_for_category(Some("线上问题"), status)
            .expect_err(&format!("线上问题应拒绝 {status}"));
        assert!(err.message.contains(status));
    }
    // 子需求专属状态仍然被基础校验拒绝
    assert!(ensure_status_for_category(Some("线上问题"), "已合入").is_err());
    // 普通需求维持全集校验：需求流状态放行、轻流程状态也不误伤（存量兼容）
    assert!(ensure_status_for_category(Some("需求"), "需求澄清").is_ok());
    assert!(ensure_status_for_category(Some("需求"), "排查中").is_ok());
    // category 缺失时保持旧行为（全集校验），由 meta 数据质量兜底
    assert!(ensure_status_for_category(None, "开发中").is_ok());
}
