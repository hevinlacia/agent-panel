/**
 * AI-powered relative effort estimation for requirements.
 *
 * Role: call an OpenAI-compatible chat endpoint to evaluate a relative
 * effort *coefficient* (e.g. 0.5x–5.0x) for a requirement, based on its
 * on-disk context files. The final estimated hours = baseHours × coefficient.
 * The coefficient is designed to be comparable across different requirements
 * so the user can rank and plan work.
 *
 * Public surface:
 *   - runAiEffortEstimation(req, context, opts): EffortEstimationResult
 *   - parseEffortEstimateResponse(raw): ParsedEffortEstimate | null
 *   - types: EffortEstimate, EffortFactor, EffortEstimationResult
 *
 * Constraints / safety:
 *   - Pure module for prompt building + response parsing; the only I/O is
 *     the `fetch` call inside `runAiEffortEstimation`, which never throws
 *     (failures land in `result.error`).
 *   - Reuses the code-review base URL / API key; the model falls back to
 *     `codeReviewModel` when `effortEstimateModel` is empty.
 *   - The LLM never sees secrets or session transcripts—only curated
 *     requirement markdown files, each capped to a prompt budget.
 *
 * Read-this-with:
 *   - `src/codeReview.ts` (the OpenAI-compatible call pattern this follows)
 *   - `src/requirements.ts` (the Requirement type + file paths)
 *   - `src/server.tsx` (the API endpoint that invokes this)
 */

import type { Requirement } from "./requirements.ts"

/** One scored factor in the estimation breakdown. */
export interface EffortFactor {
  name: string
  /** 1–5 scale, higher = more complex. */
  score: number
  reason: string
}

/** Persisted effort estimate written to `<req-dir>/effort-estimate.json`. */
export interface EffortEstimate {
  version: 1
  /** Relative multiplier, e.g. 2.0 means "twice the base effort". */
  coefficient: number
  /** The base hours configured at estimation time. */
  baseHours: number
  /** baseHours × coefficient, rounded to 1 decimal. */
  estimatedHours: number
  factors: EffortFactor[]
  summary: string
  model: string
  updatedAt: number
}

/** Result of an AI estimation call. On success, `estimate` is set; on failure, `error`. */
export interface EffortEstimationResult {
  estimate: EffortEstimate | null
  model: string
  error?: string
}

/** Internal type for the parsed LLM JSON before enrichment. */
interface ParsedEffortEstimate {
  coefficient: number
  factors: EffortFactor[]
  summary: string
}

/** Requirement files fed to the model, in priority order. */
const CONTEXT_FILES = [
  "meta.md",
  "background.md",
  "impact.md",
  "branch.md",
  "test.md",
  "config-changes.md",
  "notes.md",
] as const

const CONTEXT_FILE_CHAR_LIMIT = 10_000

const SYSTEM_PROMPT = [
  "你是一名资深技术项目经理，擅长评估软件开发需求的相对工时复杂度。",
  "你需要评估一个相对系数（coefficient），用于衡量不同需求之间的工时差异。",
  "系数参考标准：",
  "- 0.25：琐碎（改配置、修文案、加日志）",
  "- 0.5：简单（单接口、无核心链路、无 DB 变更）",
  "- 1.0：标准（典型功能开发，1-2 个文件，常规测试）",
  "- 1.5：中等（多模块联动，有 DB 或配置变更）",
  "- 2.0：复杂（核心链路改造，MQ/DB 变更，需回归测试）",
  "- 3.0：高复杂（跨系统、数据迁移、架构调整）",
  "- 5.0：极高（大规模重构、核心架构变更）",
  "",
  "你需要评估以下 6 个维度，每个打 1-5 分（越高越复杂），然后给出一个综合系数：",
  "1. 影响范围：涉及几个仓库/模块/文件",
  "2. 业务逻辑复杂度：状态机、分支逻辑、边界条件的复杂程度",
  "3. 核心链路影响：是否触及入库、库存、出库、复核、发运、回传等核心流程",
  "4. 数据变更：DB DDL/DML、MQ 新增/修改、数据迁移",
  "5. 集成复杂度：外部 API 调用、DTS、跨系统交互",
  "6. 测试复杂度：需要多少测试场景、是否需要全链路回归",
  "",
  "输出格式为 JSON（不要用 markdown 代码块包裹）：",
  '{"coefficient": <数字>, "factors": [{"name": "<维度名>", "score": <1-5>, "reason": "<一句话原因>"}], "summary": "<一句话总结>"}',
  "",
  "注意：coefficient 不要简单等于各维度分数的平均值，要综合考量整体复杂度。如果需求信息不足，基于已有信息做保守估计并在 summary 中说明。",
].join("\n")

/**
 * Build the user message from the requirement context files.
 * Reads each file, caps to a char budget, and concatenates with headers.
 */
export function buildEffortEstimateUserMessage(
  req: Pick<Requirement, "id" | "title" | "status" | "project" | "description">,
  contextFiles: Record<string, string>,
): string {
  const parts: string[] = [
    `需求 ID：${req.id}`,
    `需求标题：${req.title}`,
    `需求状态：${req.status}`,
    `项目：${req.project}`,
    "",
    "=== 需求文件内容 ===",
  ]
  for (const name of CONTEXT_FILES) {
    const content = contextFiles[name]?.trim()
    if (!content) continue
    const trimmed = content.length > CONTEXT_FILE_CHAR_LIMIT
      ? content.slice(0, CONTEXT_FILE_CHAR_LIMIT) + "\n…(截断)"
      : content
    parts.push(`--- ${name} ---`)
    parts.push(trimmed)
    parts.push("")
  }
  if (parts.length <= 6) {
    parts.push("（未找到任何需求文件，仅基于标题和描述评估）")
  }
  return parts.join("\n")
}

/**
 * Parse the LLM's JSON response. Strips markdown fences and extracts the
 * first balanced JSON object. Returns null on failure.
 */
export function parseEffortEstimateResponse(raw: string): ParsedEffortEstimate | null {
  if (!raw || !raw.trim()) return null
  let text = raw.trim()
  // Strip ```json ... ``` fences.
  const fenceMatch = text.match(/```(?:json)?\s*([\s\S]*?)```/)
  if (fenceMatch) text = fenceMatch[1].trim()
  // Fast path.
  try {
    return validateParsed(JSON.parse(text))
  } catch { /* fall through */ }
  // Slow path: find first balanced { ... }.
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
        try { return validateParsed(JSON.parse(text.slice(start, i + 1))) } catch { return null }
      }
    }
  }
  return null
}

function validateParsed(parsed: unknown): ParsedEffortEstimate | null {
  if (!parsed || typeof parsed !== "object") return null
  const o = parsed as Record<string, unknown>
  const coefficient = typeof o.coefficient === "number" ? o.coefficient : NaN
  if (!Number.isFinite(coefficient) || coefficient <= 0) return null
  const factors: EffortFactor[] = []
  if (Array.isArray(o.factors)) {
    for (const f of o.factors) {
      if (!f || typeof f !== "object") continue
      const fr = f as Record<string, unknown>
      const name = typeof fr.name === "string" ? fr.name.trim() : ""
      if (!name) continue
      const score = typeof fr.score === "number" ? Math.max(1, Math.min(5, Math.round(fr.score))) : 3
      const reason = typeof fr.reason === "string" ? fr.reason.trim() : ""
      factors.push({ name, score, reason })
    }
  }
  const summary = typeof o.summary === "string" ? o.summary.trim() : ""
  return {
    coefficient: Math.max(0.25, Math.min(5, Math.round(coefficient * 100) / 100)),
    factors,
    summary,
  }
}

/**
 * Call an OpenAI-compatible chat endpoint to estimate the relative effort
 * coefficient for a requirement. Never throws—failures land in `result.error`.
 */
export async function runAiEffortEstimation(
  req: Pick<Requirement, "id" | "title" | "status" | "project" | "description">,
  contextFiles: Record<string, string>,
  opts: { baseUrl: string; apiKey: string; model: string; fallbackModel: string; baseHours: number },
): Promise<EffortEstimationResult> {
  const base = (opts.baseUrl || "").trim().replace(/\/+$/, "")
  const model = (opts.model || "").trim() || (opts.fallbackModel || "").trim()
  const key = (opts.apiKey || "").trim()
  if (!base || !model || !key) {
    return { estimate: null, model, error: "missing-config" }
  }
  const userMessage = buildEffortEstimateUserMessage(req, contextFiles)
  let endpoint: string
  try {
    endpoint = new URL("chat/completions", base + "/").href
  } catch {
    return { estimate: null, model, error: `invalid baseUrl: ${base}` }
  }
  try {
    const controller = new AbortController()
    const timer = setTimeout(() => controller.abort(), 120_000)
    const res = await fetch(endpoint, {
      method: "POST",
      headers: { "Content-Type": "application/json", Authorization: `Bearer ${key}` },
      body: JSON.stringify({
        model,
        messages: [
          { role: "system", content: SYSTEM_PROMPT },
          { role: "user", content: userMessage },
        ],
        temperature: 0.3,
        stream: false,
      }),
      signal: controller.signal,
    })
    clearTimeout(timer)
    if (!res.ok) {
      const detail = await res.text().catch(() => "")
      return { estimate: null, model, error: `HTTP ${res.status}: ${detail.slice(0, 500)}` }
    }
    const data = (await res.json()) as { choices?: { message?: { content?: string } }[] }
    const content = data.choices?.[0]?.message?.content?.trim() || ""
    if (!content) {
      return { estimate: null, model, error: "empty response from model" }
    }
    const parsed = parseEffortEstimateResponse(content)
    if (!parsed) {
      return { estimate: null, model, error: "模型返回的内容无法解析为 JSON" }
    }
    const estimatedHours = Math.round(opts.baseHours * parsed.coefficient * 10) / 10
    const estimate: EffortEstimate = {
      version: 1,
      coefficient: parsed.coefficient,
      baseHours: opts.baseHours,
      estimatedHours,
      factors: parsed.factors,
      summary: parsed.summary,
      model,
      updatedAt: Date.now(),
    }
    return { estimate, model }
  } catch (err) {
    const msg = err instanceof Error ? err.message : String(err)
    return { estimate: null, model, error: msg }
  }
}
