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
 *   - buildBranchScopeAiPrompt(req, branchMd): { system, user }
 *   - parseBranchScopeAiJson(raw): unknown | null
 *   - normalizeBranchScopeRepos(parsed): BranchRepo[]
 *   - runAiBranchScopeExtraction(req, branchMd, opts): AiBranchScopeResult
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
import type { BranchRepo } from "./branchScope.ts"

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

/**
 * Build a simpler prompt for a direct LLM call (no fork job). Asks the
 * model to return a single JSON object matching the branches.json schema,
 * without the ===UPDATE=== delimiter protocol used by the background
 * auto-extract pipeline.
 */
export function buildBranchScopeAiPrompt(
  req: Pick<Requirement, "id" | "title">,
  branchMd: string,
): { system: string; user: string } {
  const title = (req.title || "").trim() || req.id
  const system = [
    "你是需求分支信息结构化助手。请阅读用户提供的 branch.md，把其中涉及的“仓库 ↔ 需求分支”提取成结构化 JSON。",
    "只输出 JSON，不要输出任何解释、Markdown 代码块或额外文字。",
    "repoName 必须用完整仓库名（如 yl-cwhsea-wms-outbound-api），短名要补全成 yl-cwhsea-wms-<短名>。",
    "branches 只收录 feature/fix 分支（含 /），绝不收录 master/test/uat 等基准分支。",
    "无法确定的字段省略，不要编造。JSON 必须合法。",
  ].join("\n")
  const user = [
    `需求：${title}`,
    "",
    "=== branch.md 原文 ===",
    branchMd.trim() || "(空)",
    "",
    "请输出 branches.json 的 JSON 对象，schema：",
    "{",
    '  "version": 2,',
    '  "updatedAt": 0,',
    '  "repos": [',
    '    { "repoName": "yl-cwhsea-wms-xxx-api", "branches": ["hevin.yang/feature/xxx"], "role": "后端", "path": "~/Developer/.../" }',
    "  ]",
    "}",
    "updatedAt 填 0，系统会自动补当前时间。",
  ].join("\n")
  return { system, user }
}

/**
 * Extract a JSON object from a raw LLM response. Strips Markdown code
 * fences and ===UPDATE=== delimiter wrappers, then locates the first
 * balanced `{` … `}` block. Returns null when no valid JSON is found.
 */
export function parseBranchScopeAiJson(raw: string): unknown | null {
  if (!raw || !raw.trim()) return null
  // Strip ===UPDATE: branches.json=== wrapper if present (prompt from
  // the fork-job path may be reused).
  let text = raw
  const updateMatch = text.match(/===UPDATE:\s*branches\.json===\s*([\s\S]*?)(?:===|$)/)
  if (updateMatch) text = updateMatch[1]
  // Strip ```json ... ``` or ``` ... ``` fences.
  const fenceMatch = text.match(/```(?:json)?\s*([\s\S]*?)```/)
  if (fenceMatch) text = fenceMatch[1]
  text = text.trim()
  // Fast path: direct parse.
  try { return JSON.parse(text) } catch { /* fall through */ }
  // Slow path: find the first balanced { ... } block.
  const start = text.indexOf("{")
  if (start < 0) return null
  let depth = 0
  let inStr = false
  let escape = false
  for (let i = start; i < text.length; i++) {
    const ch = text[i]
    if (escape) { escape = false; continue }
    if (ch === "\\") { escape = true; continue }
    if (ch === '"') { inStr = !inStr; continue }
    if (inStr) continue
    if (ch === "{") depth++
    else if (ch === "}") {
      depth--
      if (depth === 0) {
 try { return JSON.parse(text.slice(start, i + 1)) } catch { return null } }
    }
  }
  return null
}

/**
 * Validate and normalize the parsed LLM JSON into a BranchRepo[] list.
 * Mirrors the tolerance of readBranchScope: skips entries without a
 * repoName, trims branch strings, drops base branches. Returns an
 * empty array when nothing usable is found.
 */
export function normalizeBranchScopeRepos(parsed: unknown): BranchRepo[] {
  if (!parsed || typeof parsed !== "object") return []
  const o = parsed as Record<string, unknown>
  const rawRepos = Array.isArray(o.repos) ? o.repos : []
  const repos: BranchRepo[] = []
  for (const r of rawRepos) {
    if (!r || typeof r !== "object") continue
    const rr = r as Record<string, unknown>
    const repoName = typeof rr.repoName === "string" ? rr.repoName.trim() : ""
    if (!repoName) continue
    const branches = Array.isArray(rr.branches)
      ? rr.branches.filter((b): b is string => typeof b === "string" && !!b.trim()).map((b) => b.trim())
      : []
    repos.push({
      repoName,
      branches,
      role: typeof rr.role === "string" && rr.role.trim() ? rr.role.trim() : undefined,
      path: typeof rr.path === "string" && rr.path.trim() ? rr.path.trim() : undefined,
    })
  }
  return repos
}

/**
 * Result of an AI branch-scope extraction. On success, `repos` holds
 * the validated BranchRepo list ready to persist; on failure, `error`
 * is set and `repos` is empty.
 */
export interface AiBranchScopeResult {
  repos: BranchRepo[]
  model: string
  updatedAt: number
  error?: string
}

/**
 * Call an OpenAI-compatible chat completion endpoint to extract a
 * structured branches.json from branch.md text. Never throws - failures
 * land in the returned `error` field so the UI can render them inline.
 * Reuses the baseUrl / API key resolved from the selected pi provider
 * by the caller (src/server.tsx route); the model is the chosen pi modelId.
 */
export async function runAiBranchScopeExtraction(
  req: Pick<Requirement, "id" | "title">,
  branchMd: string,
  opts: { baseUrl: string; apiKey: string; model: string; fallbackModel: string },
): Promise<AiBranchScopeResult> {
  const updatedAt = Date.now()
  const base = (opts.baseUrl || "").trim().replace(/\/+$/, "")
  const model = (opts.model || "").trim() || (opts.fallbackModel || "").trim()
  const key = (opts.apiKey || "").trim()
  if (!base || !model || !key) {
    return { repos: [], model, updatedAt, error: "missing-config" }
  }
  const { system, user } = buildBranchScopeAiPrompt(req, branchMd)
  let endpoint: string
  try {
    endpoint = new URL("chat/completions", base + "/").href
  } catch {
    return { repos: [], model, updatedAt, error: `invalid baseUrl: ${base}` }
  }
  try {
    const controller = new AbortController()
    const timer = setTimeout(() => controller.abort(), 120_000)
    const res = await fetch(endpoint, {
      method: "POST",
      headers: { "Content-Type": "application/json", Authorization: `Bearer ${key}` },
      body: JSON.stringify({ model, messages: [{ role: "system", content: system }, { role: "user", content: user }], temperature: 0, stream: false }),
      signal: controller.signal,
    })
    clearTimeout(timer)
    if (!res.ok) {
      const detail = await res.text().catch(() => "")
      return { repos: [], model, updatedAt, error: `HTTP ${res.status}: ${detail.slice(0, 500)}` }
    }
    const data = (await res.json()) as { choices?: { message?: { content?: string } }[] }
    const content = data.choices?.[0]?.message?.content?.trim() || ""
    if (!content) {
      return { repos: [], model, updatedAt, error: "empty response from model" }
    }
    const parsed = parseBranchScopeAiJson(content)
    if (!parsed) {
      return { repos: [], model, updatedAt, error: "模型返回的内容无法解析为 JSON" }
    }
    const repos = normalizeBranchScopeRepos(parsed)
    if (repos.length === 0) {
      return { repos: [], model, updatedAt, error: "未识别到任何仓库或分支" }
    }
    return { repos, model, updatedAt }
  } catch (err) {
    const msg = err instanceof Error ? err.message : String(err)
    return { repos: [], model, updatedAt, error: msg }
  }
}
