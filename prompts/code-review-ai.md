# code-review-ai.md 审查提示词（已由 skill 接管）

> 本模板已停止维护。代码审查流程与提示词的单一真源是
> `agent-panel-code-review` skill（`$WMS_WORKSPACE_ROOT/.agents/skills/agent-panel-code-review/`）：
>
> - 流程与产物：`SKILL.md`（备料 → 四轮深度审查 → code-review-ai.md / review-checklist / annotations → 门禁自检）
> - 深度方法论：`references/review-methodology.md`（四轮审查法、横切检查清单、库存账本矩阵、反例推演）
>
> 门禁已强制结论文档带 `Source: agent-panel-code-review skill` 标记；历史模板内容如与 skill 冲突，以 skill 为准。

<details>
<summary>历史模板存档（仅供参考，勿再按此执行）</summary>

你正在为 Agent Panel 需求做 AI 代码审查：{{REQ_ID}} - {{REQ_TITLE}}
需求目录：{{REQ_DIR}}

## 审查材料
1. 增量二次审查：若存在 `code-review-incremental.json`，优先读取它；它只包含上次已审 `targetCommit` 到当前 HEAD 的新增提交与 diff，用于"测试中状态又改代码"的快速复审。
2. 全量首次审查：若不存在增量包，读取需求目录下的 `code-review.json`（repos[].diff 是每个仓库相对生产基线的逐文件 unified diff）与核心需求上下文文件（meta.md、background.md、technical-plan.md、test.md、release-manifest.md、notes.md）。
3. 增量包约束：若任一 repo 的 `linearHistory=false`，说明分支可能 rebase/force-push，不能只靠增量审查，应回退到全量。

## 评估角度
从逻辑严谨性与性能两个维度审查每处改动，并标注 [逻辑] 或 [性能]。

## 输出
文件顶部必须给出门禁结论：`Review Gate: PASS`、`Review Gate: BLOCKED` 或 `Review Gate: WAIVED`。

## 风险识别（必做）
命中 `库存` 风险标签时必须完成「库存账本评估」。

</details>
