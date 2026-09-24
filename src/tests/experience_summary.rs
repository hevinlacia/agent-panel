use super::*;

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
fn expire_stale_job_gate_blocks_failed_and_incomplete() {
    // 总结失败：无论自动总结开关，绝不自动推进，需求停留在经验总结等用户处理。
    assert!(!expire_stale_job_allows_complete("failed", true));
    assert!(!expire_stale_job_allows_complete("failed", false));
    // 自动总结开启：未完成（pending/running/无 job）不推进，避免总结没做完就被关闭。
    assert!(!expire_stale_job_allows_complete("pending", true));
    assert!(!expire_stale_job_allows_complete("running", true));
    assert!(!expire_stale_job_allows_complete("", true));
    // 自动总结开启：已完成/跳过的总结可以兜底推进。
    assert!(expire_stale_job_allows_complete("completed", true));
    assert!(expire_stale_job_allows_complete("skipped", true));
    // 自动总结关闭（纯手动模式）：非失败状态超期可推进。
    assert!(expire_stale_job_allows_complete("", false));
    assert!(expire_stale_job_allows_complete("pending", false));
    assert!(expire_stale_job_allows_complete("completed", false));
}

// ── dispatch 限额与状态分类（纯函数语义锁定） ─────────────────────────────────

use crate::experience_summary::{
    classify_experience_job, experience_dispatch_can_spawn, experience_job_is_stale,
    ExperienceJobClass,
};

#[test]
fn dispatch_gate_serializes_with_max_agents_one() {
    // maxAgents=1：无占用可派发；占用 1 即排队（同时只能跑一个经验总结任务）
    assert!(experience_dispatch_can_spawn(0, 1));
    assert!(!experience_dispatch_can_spawn(1, 1));
    assert!(!experience_dispatch_can_spawn(5, 1));
    // 更大额度：active < max 才派发，达到即排队
    assert!(experience_dispatch_can_spawn(2, 3));
    assert!(!experience_dispatch_can_spawn(3, 3));
    // clamp 下限 1：0 不是合法配置语义（防呆）
    assert!(!experience_dispatch_can_spawn(0, 0));
}

#[test]
fn classify_job_status_covers_dispatch_paths() {
    assert!(matches!(classify_experience_job("completed", 1, 2), ExperienceJobClass::Completed));
    assert!(matches!(classify_experience_job("running", 1, 2), ExperienceJobClass::Running));
    // attempts < 上限 → 重试转 pending；到顶 → 终态失败
    assert!(matches!(classify_experience_job("failed", 1, 2), ExperienceJobClass::Retryable));
    assert!(matches!(classify_experience_job("failed", 2, 2), ExperienceJobClass::FailedFinal));
    assert!(matches!(classify_experience_job("pending", 0, 2), ExperienceJobClass::Pending));
    assert!(matches!(classify_experience_job("", 0, 2), ExperienceJobClass::Pending));
    assert!(matches!(classify_experience_job("paused", 0, 2), ExperienceJobClass::Unknown));
}

#[test]
fn stale_transition_only_applies_to_running_jobs() {
    let now = 10_000_000_i64;
    let stale = 2 * 60 * 60 * 1000;
    assert!(experience_job_is_stale("running", Some(now - stale - 1), now, stale));
    // 恰好到点不算超（严格大于）
    assert!(!experience_job_is_stale("running", Some(now - stale), now, stale));
    assert!(!experience_job_is_stale("running", Some(now - 1000), now, stale));
    assert!(!experience_job_is_stale("running", None, now, stale));
    assert!(!experience_job_is_stale("pending", Some(now - stale - 1), now, stale));
}
