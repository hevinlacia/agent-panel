use super::*;

pub(crate) async fn load_fixed_phase_prompt(state: &AppState) -> String {
    load_prompt_file(
        state,
        PHASE_COMMON_PROMPT_FILE,
        "请在推进当前任务的同时，实时记录可复用经验、业务知识和 skill 改进候选。",
    )
    .await
}

pub(crate) async fn load_phase_prompt(state: &AppState, status: &str) -> String {
    load_prompt_file(
        state,
        phase_prompt_file(status),
        &format!("本阶段状态：{status}。请遵循 Agent Panel 需求文件协议推进。"),
    )
    .await
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
        "排查中" | "已定位" | "已修复" | "已复盘" | "已关闭" => {
            "prompts/phase-online-issue.md"
        }
        "经验总结" | "待上线" => "prompts/phase-experience-summary.md",
        "发布就绪" => "prompts/phase-deploy.md",
        "已完成" => "prompts/phase-done.md",
        _ => "prompts/phase-dev.md",
    }
}
