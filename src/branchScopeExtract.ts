/**
 * Branch-scope extraction: build the prompt that asks a background
 * agent to convert a free-form `branch.md` into the structured
 * `branches.json` consumed by `BranchScopeCard`.
 *
 * Role: the "生成 branches.json" button on the detail page triggers a
 * background `opencode run --fork` job. This module builds the prompt
 * and the output contract so the agent emits a single
 * `===UPDATE: branches.json===` block that `parseAutoExtractOutput`
 * (reused from autoExtract.ts) + the autoAdopt writer can persist
 * without any new spawn/queue/notify plumbing.
 *
 * Public surface:
 *   - buildBranchScopePrompt(req, branchMd): string
 *
 * Constraints / safety:
 *   - Pure function; no I/O. The caller reads `branch.md` and spawns.
 *   - The prompt forbids touching any file other than branches.json,
 *     so the autoAdopt writer (whitelist-gated) has nothing else to do.
 *   - Reuses the `===UPDATE: <file>===` delimiter protocol from
 *     autoExtract.ts; branches.json must be in ALLOWED_UPDATE_FILES.
 *
 * Read-this-with:
 *   - `src/branchScope.ts` (the schema this prompt produces)
 *   - `src/autoExtract.ts` (parseAutoExtractOutput + ALLOWED_UPDATE_FILES)
 *   - `src/extractJobs.ts` (autoAdopt writer that persists the output)
 */

import type { Requirement } from "./requirements.ts"

const SCHEMA_EXAMPLE = `{
  "version": 2,
  "updatedAt": 0,  // 填 0 即可，系统自动补当前时间
  "repos": [
    {
      "repoName": "yl-cwhsea-wms-xxx-api",
      "branches": ["hevin.yang/feature/xxx"],
      "role": "后端",
      "path": "~/Developer/.../"
    }
  ]
}`

/**
 * Build the prompt for the branch-scope generation job.
 *
 * The agent receives the full `branch.md` text and the `branches.json`
 * schema, and must reply with exactly one `===UPDATE: branches.json===`
 * block containing valid JSON, plus a `===SUMMARY===` line. It is told
 * to harvest only feature/fix branches (not base branches like
 * master/test/UAT-2607) and to pair each repo with its merge progress.
 */
export function buildBranchScopePrompt(
  req: Pick<Requirement, "id" | "title">,
  branchMd: string,
): string {
  const title = (req.title || "").trim() || req.id
  return [
    `你是需求分支信息结构化助手。请阅读需求《${title}》的 branch.md，把其中涉及的"仓库 ↔ 需求分支"提取成结构化 JSON。`,
    "",
    "=== branch.md 原文 ===",
    branchMd.trim() || "(空)",
    "",
    "## 输出格式（严格遵守）",
    "",
    "只输出一个文件更新块，格式如下：",
    "===UPDATE: branches.json===",
    "<完整的 JSON 内容，不要用 markdown 代码块包裹>",
    "",
    "然后输出变更说明：",
    "===SUMMARY===",
    "<一句话说明识别到几个仓库、几个分支，不超过 2 行>",
    "",
    "## branches.json schema",
    "",
    "```json",
    SCHEMA_EXAMPLE,
    "```",
    "",
    "## 规则",
    "1. 只输出 branches.json 一个文件，不要输出 branch.md 或其他文件",
    "2. repoName 必须用完整仓库名（如 yl-cwhsea-wms-outbound-api），短名（如 outbound-api）要补全成 yl-cwhsea-wms-<短名>",
    "3. branches 只收录本次需求创建的 feature/fix 分支（含 /，如 hevin.yang/feature/xxx、yhw/fix-xxx），绝不收录 master/test/uat/UAT-2607 等基准分支",
    "4. 同一仓库多个需求分支时，全部列入 branches 数组",
    "5. 归档/历史/已失效分支不收录（除非它仍是当前发布路径）",
    "6. 无法确定的字段省略，不要编造",
    "7. JSON 必须合法（可被 JSON.parse 解析），不要有尾逗号",
    "8. 不要执行任何 shell 命令或工具调用，直接输出 JSON 文本；updatedAt 填 0，系统会自动补当前时间",
    "",
    "不要写客套话，不要用 ```json 包裹 ===UPDATE 块内的内容。",
  ].join("\n")
}
