use super::*;

#[test]
fn rework_transition_truth_table() {
    // 打回回退边：人工核查/测试中 → 开发中 计返工
    assert!(is_rework_transition(Some("人工核查"), "开发中"));
    assert!(is_rework_transition(Some("测试中"), "开发中"));
    // 自测中退回开发中是正常纠偏，不计数
    assert!(!is_rework_transition(Some("自测中"), "开发中"));
    // 前向流转与同状态不计数
    assert!(!is_rework_transition(Some("人工核查"), "测试中"));
    assert!(!is_rework_transition(Some("测试中"), "测试中"));
    assert!(!is_rework_transition(None, "开发中"));
}

#[test]
fn rework_round_count_derives_from_history_when_field_missing() {
    // 历史需求无显式 reworkRounds 字段：从 history 派生
    let state = json!({
        "status": "开发中",
        "history": [
            { "status": "开发中", "from": "需求澄清" },
            { "status": "测试中", "from": "开发中" },
            { "status": "开发中", "from": "测试中" },
            { "status": "测试中", "from": "开发中" },
            { "status": "开发中", "from": "人工核查" }
        ]
    });
    assert_eq!(rework_round_count(&state), 2);
    // 显式字段优先于派生值
    let explicit = json!({ "status": "开发中", "reworkRounds": 5, "history": [] });
    assert_eq!(rework_round_count(&explicit), 5);
    // 无历史无字段 = 0
    assert_eq!(rework_round_count(&json!({ "status": "开发中" })), 0);
}

#[tokio::test]
async fn rework_bounce_back_increments_rounds_and_marks_history() {
    let dir = std::env::temp_dir().join(format!(
        "agent-panel-rework-test-{}",
        chrono_like_unique_suffix()
    ));
    std::fs::create_dir_all(&dir).expect("create temp req dir");
    // 首轮：正常前向流转到测试中
    write_requirement_status_checked(
        dir.to_str().unwrap(),
        "测试中",
        Some("首轮推进"),
        GateCheckMode::Passed,
    )
    .await
    .expect("forward transition");
    // 打回：测试中 → 开发中，计第 1 轮
    let bounced = write_requirement_status_checked(
        dir.to_str().unwrap(),
        "开发中",
        Some("人工核查发现回归失败，打回"),
        GateCheckMode::Skipped,
    )
    .await
    .expect("rework transition");
    assert_eq!(bounced["reworkRounds"], json!(1));
    assert_eq!(bounced["lastTransition"]["rework"], json!(true));
    assert_eq!(bounced["lastTransition"]["reworkRound"], json!(1));
    // 重新前向推进：轮次保持
    let forward = write_requirement_status_checked(
        dir.to_str().unwrap(),
        "测试中",
        Some("返工后重新推进"),
        GateCheckMode::Passed,
    )
    .await
    .expect("forward after rework");
    assert_eq!(forward["reworkRounds"], json!(1));
    // 第二次打回：计第 2 轮
    let bounced_again = write_requirement_status_checked(
        dir.to_str().unwrap(),
        "开发中",
        Some("第二轮打回"),
        GateCheckMode::Skipped,
    )
    .await
    .expect("second rework");
    assert_eq!(bounced_again["reworkRounds"], json!(2));
    assert_eq!(bounced_again["lastTransition"]["reworkRound"], json!(2));
    // category 写入不丢 reworkRounds（该路径整体重建 state.json）
    let cat = write_requirement_category(dir.to_str().unwrap(), "需求")
        .await
        .expect("category write");
    assert_eq!(cat["reworkRounds"], json!(2));
    trash_cleanup_rework_dir(&dir);
}

fn trash_cleanup_rework_dir(dir: &Path) {
    // 测试临时目录：属本测试自建产物，直接删除
    let _ = std::fs::remove_dir_all(dir);
}
