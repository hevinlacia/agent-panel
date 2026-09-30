use super::*;

pub(crate) async fn load_fixed_phase_prompt(state: &AppState) -> String {
    load_prompt_file(
        state,
        PHASE_COMMON_PROMPT_FILE,
        "请在推进当前任务的同时，实时记录可复用经验、业务知识和 skill 改进候选。",
    )
    .await
}

pub(crate) async fn load_phase_prompt(
    state: &AppState,
    status: &str,
    loop_kind: DevLoopKind,
    rework_rounds: u64,
    release_ready_rounds: u64,
) -> String {
    let file = phase_prompt_file_for(status, loop_kind);
    let fallback = match (status, loop_kind) {
        ("开发中", DevLoopKind::Rework) => format!(
            "本阶段状态：{status}（第 {rework_rounds} 轮返工）。请遵循 Agent Panel 需求文件协议推进：只修复打回意见相关代码，在 test.md 追加返工回归小节，重新推进前刷新增量审查快照。"
        ),
        ("开发中", DevLoopKind::ReleaseReady) => format!(
            "本阶段状态：{status}（发布就绪小循环第 {release_ready_rounds} 轮）。请遵循 Agent Panel 需求文件协议推进：上线前小改动快速迭代，默认不写单测、只部署 UAT（CN+SEA 成对）并在 UAT 测试，重新推进发布就绪前刷新增量审查快照。"
        ),
        _ => format!("本阶段状态：{status}。请遵循 Agent Panel 需求文件协议推进。"),
    };
    load_prompt_file(state, file, &fallback).await
}

pub(crate) async fn load_prompt_file(
    state: &AppState,
    prompt_file: &str,
    fallback: &str,
) -> String {
    let path = state.project_root.join(prompt_file);
    fs::read_to_string(path)
        .await
        .unwrap_or_else(|_| fallback.to_string())
}

pub(crate) fn phase_prompt_file(status: &str) -> &'static str {
    match status {
        "需求澄清" | "需求对齐" | "方案设计" => "prompts/phase-clarify.md",
        "开发中" => "prompts/phase-dev.md",
        "自测中" => "prompts/phase-selftest.md",
        "测试中" => "prompts/phase-testing.md",
        "人工核查" | "人工复测" => "prompts/phase-manual-check.md",
        "排查中" | "已定位" | "已修复" | "已复盘" | "已关闭" => {
            "prompts/phase-online-issue.md"
        }
        "经验总结" | "待上线" => "prompts/phase-experience-summary.md",
        "发布就绪" => "prompts/phase-deploy.md",
        "已完成" => "prompts/phase-done.md",
        _ => "prompts/phase-dev.md",
    }
}

/// 循环语境感知的阶段 prompt 文件：
/// 开发中 + 最近一次进入开发中来自打回（人工核查/测试中）→ 返工变体，
/// 语境是「打回后小修小补 + 增量验证」；
/// 开发中 + 最近一次进入开发中来自发布就绪 → 发布就绪小循环变体，
/// 语境是「上线前快速修复：免单测、UAT-only、增量审查后直通发布就绪」。
/// 判定依据是 dev_loop_kind_from_state（最近一次来源），不是累计轮次。
pub(crate) fn phase_prompt_file_for(status: &str, loop_kind: DevLoopKind) -> &'static str {
    if status == "开发中" {
        match loop_kind {
            DevLoopKind::Rework => return PHASE_DEV_REWORK_PROMPT_FILE,
            DevLoopKind::ReleaseReady => return PHASE_DEV_RELEASE_READY_PROMPT_FILE,
            DevLoopKind::None => {}
        }
    }
    phase_prompt_file(status)
}
