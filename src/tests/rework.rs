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
fn dev_loop_kind_truth_table() {
    // 发布就绪 → 开发中 = 发布就绪小循环
    assert_eq!(dev_loop_kind(Some("发布就绪"), "开发中"), DevLoopKind::ReleaseReady);
    // 打回来源保持返工
    assert_eq!(dev_loop_kind(Some("测试中"), "开发中"), DevLoopKind::Rework);
    assert_eq!(dev_loop_kind(Some("人工核查"), "开发中"), DevLoopKind::Rework);
    // 其余回退/前向/目标非开发中都不是循环
    assert_eq!(dev_loop_kind(Some("自测中"), "开发中"), DevLoopKind::None);
    assert_eq!(dev_loop_kind(Some("发布就绪"), "经验总结"), DevLoopKind::None);
    assert_eq!(dev_loop_kind(None, "开发中"), DevLoopKind::None);
    assert_eq!(dev_loop_kind(Some("发布就绪"), "发布就绪"), DevLoopKind::None);
}

#[test]
fn release_ready_round_count_derives_from_history_when_field_missing() {
    let state = json!({
        "status": "发布就绪",
        "history": [
            { "status": "开发中", "from": "需求澄清" },
            { "status": "发布就绪", "from": "测试中" },
            { "status": "开发中", "from": "发布就绪", "loop": "release-ready", "loopRound": 1 },
            { "status": "发布就绪", "from": "开发中" },
            { "status": "开发中", "from": "测试中" }
        ]
    });
    // 只有 from=发布就绪 → 开发中 计小循环轮次；打回不计入
    assert_eq!(release_ready_round_count(&state), 1);
    assert_eq!(rework_round_count(&state), 1);
    // 显式字段优先于派生值
    let explicit = json!({ "status": "发布就绪", "releaseReadyRounds": 3, "history": [] });
    assert_eq!(release_ready_round_count(&explicit), 3);
    assert_eq!(release_ready_round_count(&json!({ "status": "发布就绪" })), 0);
}

#[test]
fn dev_loop_kind_from_state_uses_latest_entry_into_dev() {
    // 最近一次进入开发中来自发布就绪 → 小循环语境（即使此前发生过打回）
    let fast_fix_last = json!({
        "status": "开发中",
        "reworkRounds": 1,
        "history": [
            { "status": "开发中", "from": "需求澄清" },
            { "status": "测试中", "from": "开发中" },
            { "status": "开发中", "from": "测试中" },
            { "status": "发布就绪", "from": "开发中" },
            { "status": "开发中", "from": "发布就绪" }
        ]
    });
    assert_eq!(dev_loop_kind_from_state(&fast_fix_last), DevLoopKind::ReleaseReady);
    // 最近一次进入开发中来自测试中 → 返工语境（小循环之后正常打回不拿错变体）
    let rework_last = json!({
        "status": "开发中",
        "reworkRounds": 1,
        "releaseReadyRounds": 1,
        "history": [
            { "status": "开发中", "from": "发布就绪" },
            { "status": "发布就绪", "from": "开发中" },
            { "status": "测试中", "from": "发布就绪" },
            { "status": "开发中", "from": "测试中" }
        ]
    });
    assert_eq!(dev_loop_kind_from_state(&rework_last), DevLoopKind::Rework);
    // 无历史条目时兜底：reworkRounds > 0 视为返工（老数据），否则普通开发
    assert_eq!(
        dev_loop_kind_from_state(&json!({ "status": "开发中", "reworkRounds": 2 })),
        DevLoopKind::Rework
    );
    assert_eq!(dev_loop_kind_from_state(&json!({ "status": "开发中" })), DevLoopKind::None);
}

#[tokio::test]
async fn release_ready_loop_increments_rounds_and_marks_history() {
    let dir = std::env::temp_dir().join(format!(
        "agent-panel-fastfix-test-{}",
        chrono_like_unique_suffix()
    ));
    std::fs::create_dir_all(&dir).expect("create temp req dir");
    // 首轮：正常推进到发布就绪
    write_requirement_status_checked(
        dir.to_str().unwrap(),
        "发布就绪",
        Some("首轮推进"),
        GateCheckMode::Passed,
    )
    .await
    .expect("forward transition");
    // 小循环回边：发布就绪 → 开发中，计第 1 轮小循环，reworkRounds 不动
    let looped = write_requirement_status_checked(
        dir.to_str().unwrap(),
        "开发中",
        Some("临近上线发现漏改，进入发布就绪小循环"),
        GateCheckMode::Skipped,
    )
    .await
    .expect("release-ready loop transition");
    assert_eq!(looped["releaseReadyRounds"], json!(1));
    assert_eq!(looped["reworkRounds"], json!(0));
    assert_eq!(looped["lastTransition"]["loop"], json!("release-ready"));
    assert_eq!(looped["lastTransition"]["loopRound"], json!(1));
    assert!(looped["lastTransition"].get("rework").is_none());
    // 小循环直通：开发中 → 发布就绪，轮次保持
    let back = write_requirement_status_checked(
        dir.to_str().unwrap(),
        "发布就绪",
        Some("小循环修复完成，增量审查后直通发布就绪"),
        GateCheckMode::Passed,
    )
    .await
    .expect("fast loop forward");
    assert_eq!(back["releaseReadyRounds"], json!(1));
    // 第二轮小循环：计第 2 轮
    let looped_again = write_requirement_status_checked(
        dir.to_str().unwrap(),
        "开发中",
        Some("第二轮小循环"),
        GateCheckMode::Skipped,
    )
    .await
    .expect("second fast loop");
    assert_eq!(looped_again["releaseReadyRounds"], json!(2));
    assert_eq!(looped_again["lastTransition"]["loopRound"], json!(2));
    // category 写入不丢 releaseReadyRounds
    let cat = write_requirement_category(dir.to_str().unwrap(), "需求")
        .await
        .expect("category write");
    assert_eq!(cat["releaseReadyRounds"], json!(2));
    trash_cleanup_rework_dir(&dir);
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

#[tokio::test]
async fn suspend_round_trip_keeps_flow_history_and_resume_target() {
    let dir = std::env::temp_dir().join(format!(
        "agent-panel-suspend-test-{}",
        chrono_like_unique_suffix()
    ));
    std::fs::create_dir_all(&dir).expect("create temp req dir");
    // 先推进到开发中
    write_requirement_status_checked(
        dir.to_str().unwrap(),
        "开发中",
        Some("初始推进"),
        GateCheckMode::Passed,
    )
    .await
    .expect("initial transition");
    // 任意状态可挂起：开发中 → 挂起，不算返工/小循环，也不算跳过流水状态
    let suspended = write_requirement_status_checked(
        dir.to_str().unwrap(),
        "挂起",
        Some("状态流转卡点击挂起"),
        GateCheckMode::Skipped,
    )
    .await
    .expect("suspend transition");
    assert_eq!(suspended["status"], json!("挂起"));
    assert_eq!(suspended["reworkRounds"], json!(0));
    assert_eq!(suspended["releaseReadyRounds"], json!(0));
    assert_eq!(suspended["lastTransition"]["from"], json!("开发中"));
    assert_eq!(suspended["lastTransition"]["skippedStatuses"], json!([]));
    // 恢复目标：状态历史里最近一次非挂起状态 = 开发中
    assert_eq!(resume_status_from_state(&suspended), Some("开发中".to_string()));
    // 恢复到挂起前状态，再走流水不丢失挂起痕迹
    let resumed = write_requirement_status_checked(
        dir.to_str().unwrap(),
        "测试中",
        Some("从挂起恢复后手动推进"),
        GateCheckMode::Skipped,
    )
    .await
    .expect("resume transition");
    assert_eq!(resumed["status"], json!("测试中"));
    assert_eq!(resumed["reworkRounds"], json!(0));
    // 再次挂起：恢复目标变为测试中（最近一次非挂起）
    let suspended_again = write_requirement_status_checked(
        dir.to_str().unwrap(),
        "挂起",
        Some("再次挂起"),
        GateCheckMode::Skipped,
    )
    .await
    .expect("second suspend");
    assert_eq!(resume_status_from_state(&suspended_again), Some("测试中".to_string()));
    // 历史缺失时兜底 previousStatus（非挂起才可用）
    assert_eq!(
        resume_status_from_state(&json!({ "status": "挂起", "previousStatus": "开发中" })),
        Some("开发中".to_string())
    );
    assert_eq!(resume_status_from_state(&json!({ "status": "挂起" })), None);
    trash_cleanup_rework_dir(&dir);
}
