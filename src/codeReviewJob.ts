/**
 * AI code-review job store and prompt contract.
 *
 * Role: track dashboard-launched pi agents that review a requirement's
 * code diff and write `<req-dir>/code-review-ai.md`. Mirrors the auto-drive
 * pattern (src/requirementAutoDrive.ts) but lighter - no phase/blocker
 * model, just run -> done/failed. The pi agent reads `code-review.json`
 * (the PRO diff snapshot) plus the requirement files and writes the
 * review Markdown; the dashboard only tracks run status and serves the file.
 *
 * Public surface:
 *   - initCodeReviewAiJobs(): load persisted jobs at startup
 *   - createCodeReviewAiJob / updateCodeReviewAiJob / getLatestCodeReviewAiJob
 *   - buildCodeReviewAiJobName / buildCodeReviewAiPrompt
 *   - finalizeCodeReviewAiJob: map a finished process result to job state
 *   - readCodeReviewAiMarkdown: read the persisted review file
 *   - CODE_REVIEW_AI_FILE: the output filename ("code-review-ai.md")
 *
 * Constraints / safety:
 *   - No shell execution here; server.tsx owns process spawning.
 *   - Writes only to the job store file; the pi agent writes code-review-ai.md.
 *   - Never throws on read; missing/corrupt store is treated as empty.
 *
 * Read-this-with:
 *   - src/requirementAutoDrive.ts (the heavier job pattern this follows)
 *   - src/server.tsx (code-review AI routes that drive this store)
 */

import { mkdir, readFile, stat, writeFile, rename } from "node:fs/promises"
import { existsSync } from "node:fs"
import { randomBytes } from "node:crypto"
import { homedir } from "node:os"
import { dirname, join } from "node:path"

import type { Requirement } from "./requirements.ts"
import type { QueuedOpencodeProcessResult } from "./opencodeProcessQueue.ts"

export const CODE_REVIEW_AI_FILE = "code-review-ai.md"

export type CodeReviewAiJobState = "queued" | "running" | "done" | "failed"

export interface CodeReviewAiJob {
  id: string
  reqId: string
  sessionId: string
  state: CodeReviewAiJobState
  /** Pi provider/model used for the run, e.g. "deepseek/deepseek-chat". */
  model: string
  startedAt: number
  doneAt: number | null
  durationMs: number
  summary: string
  error: string | null
  createdAt: number
  updatedAt: number
}

interface PersistedStore {
  version: 1
  jobs: CodeReviewAiJob[]
}

const DEFAULT_STORE_PATH = join(
  homedir(),
  ".local",
  "share",
  "agent-panel",
  "code-review-ai-jobs.json",
)
const STORE_VERSION = 1
const MAX_JOBS = 200

let _storePath = DEFAULT_STORE_PATH
let _jobs = new Map<string, CodeReviewAiJob>()

function newJobId(): string {
  return `cr_${randomBytes(6).toString("hex")}`
}

async function ensureDir(): Promise<void> {
  const dir = dirname(_storePath)
  if (!existsSync(dir)) await mkdir(dir, { recursive: true })
}

async function atomicWrite(path: string, body: string): Promise<void> {
  await ensureDir()
  const tmp = `${path}.tmp.${process.pid}.${Date.now()}`
  await writeFile(tmp, body, "utf-8")
  await rename(tmp, path)
}

async function loadFromDisk(): Promise<void> {
  _jobs.clear()
  if (!existsSync(_storePath)) return
  try {
    const raw = await readFile(_storePath, "utf-8")
    const store = JSON.parse(raw) as PersistedStore
    for (const job of store.jobs || []) {
      if (!job || typeof job.id !== "string") continue
      _jobs.set(job.id, job)
    }
  } catch {
    _jobs.clear()
  }
}

async function saveToDisk(): Promise<void> {
  const jobs = [..._jobs.values()]
    .sort((a, b) => b.updatedAt - a.updatedAt)
    .slice(0, MAX_JOBS)
  _jobs = new Map(jobs.map((job) => [job.id, job]))
  await atomicWrite(_storePath, JSON.stringify({ version: STORE_VERSION, jobs }, null, 2) + "\n")
}

/** Load persisted jobs. Call once at server startup. */
export async function initCodeReviewAiJobs(): Promise<void> {
  await loadFromDisk()
}

/** Create a queued job snapshot before the pi process actually starts. */
export function createCodeReviewAiJob(
  req: Requirement,
  sessionId: string,
  model: string,
): CodeReviewAiJob {
  const now = Date.now()
  const job: CodeReviewAiJob = {
    id: newJobId(),
    reqId: req.id,
    sessionId,
    state: "queued",
    model,
    startedAt: now,
    doneAt: null,
    durationMs: 0,
    summary: "已加入 AI 代码审查队列。",
    error: null,
    createdAt: now,
    updatedAt: now,
  }
  _jobs.set(job.id, job)
  saveToDisk().catch(() => {})
  return job
}

/** Patch one job in place and persist it for dashboard polling. */
export function updateCodeReviewAiJob(
  id: string,
  partial: Partial<Omit<CodeReviewAiJob, "id" | "createdAt">>,
): CodeReviewAiJob | null {
  const current = _jobs.get(id)
  if (!current) return null
  const next: CodeReviewAiJob = { ...current, ...partial, updatedAt: partial.updatedAt ?? Date.now() }
  _jobs.set(id, next)
  saveToDisk().catch(() => {})
  return next
}

/** Return the newest job for a requirement, used to show run status. */
export function getLatestCodeReviewAiJob(reqId: string): CodeReviewAiJob | null {
  const jobs = [..._jobs.values()].filter((job) => job.reqId === reqId)
  if (jobs.length === 0) return null
  return jobs.sort((a, b) => b.updatedAt - a.updatedAt)[0]
}

/** Build the pi session name shown in session lists and detail pages. */
export function buildCodeReviewAiJobName(req: Requirement): string {
  return `${req.id} AI代码审查 ${req.title}`.slice(0, 100)
}

/**
 * Build the non-interactive pi prompt. The agent reads the persisted diff
 * snapshot (`code-review.json`) and requirement files, then writes its
 * Markdown review to `code-review-ai.md`. No code mutations, only the one
 * output file.
 */
export function buildCodeReviewAiPrompt(req: Requirement): string {
  return [
    `你正在为 Agent Panel 需求做 AI 代码审查：${req.id} - ${req.title}`,
    req.reqDir ? `需求目录：${req.reqDir}` : "需求目录：未知",
    "",
    "任务：",
    "1. 读取需求目录下的 code-review.json，其中 repos[].diff 是每个仓库相对生产基线的逐文件 unified diff。",
    "2. 读取需求上下文文件：meta.md、background.md、branch.md、impact.md、test.md、config-changes.md、notes.md。",
    "3. 基于 diff 和需求上下文做严格 code review。",
    `4. 将审查结果写入需求目录下的 ${CODE_REVIEW_AI_FILE}（覆盖已有内容），Markdown 格式。`,
    "",
    "审查关注：逻辑错误、边界与空值、并发与事务、资源泄漏、与需求不符的实现、安全与配置风险、可维护性。",
    "",
    `${CODE_REVIEW_AI_FILE} 必须包含以下小节：`,
    "## 审查概览",
    "（需求标题、变更仓库与文件数、审查模型、时间）",
    "## 严重问题（必须修复）",
    "## 改进建议",
    "## 测试验收要点",
    "## 亮点",
    "",
    "要求：具体到文件与代码片段，不要泛泛而谈；某小节无内容写「无」。",
    "",
    "注意：",
    "- 只读 code-review.json 和需求文件，不要修改任何代码或需求文件，唯一写入的文件是 code-review-ai.md。",
    "- 审查结论必须基于 code-review.json 中 repos[].diff 的实际差异；如果某仓库 diff 为空，说明该仓库相对基线无改动。",
  ].join("\n")
}

/**
 * Convert a finished process result into the persisted job outcome. A
 * zero-exit run is "done" even if the agent left no file (the dashboard
 * surfaces "结果尚未写入" so the user can re-run); a non-zero exit or
 * timeout is "failed".
 */
export function finalizeCodeReviewAiJob(
  jobId: string,
  result: QueuedOpencodeProcessResult,
): CodeReviewAiJob | null {
  let state: CodeReviewAiJobState = "done"
  let summary = "AI 代码审查完成，结果已写入 code-review-ai.md。"
  let error: string | null = null
  if (result.timedOut) {
    state = "failed"
    summary = "AI 代码审查超时，已被终止。"
    error = "审查超过时间上限，请检查 code-review-ai.md 是否已部分写入，或重新审查。"
  } else if (result.exitCode !== 0) {
    state = "failed"
    summary = `pi agent 异常退出（exit ${result.exitCode ?? "unknown"}）。`
    error = "审查进程异常退出，请查看会话日志。"
  }
  return updateCodeReviewAiJob(jobId, {
    state,
    summary,
    error,
    doneAt: Date.now(),
    durationMs: result.durationMs,
  })
}

/** Read the AI review markdown file if it exists, with its mtime. */
export async function readCodeReviewAiMarkdown(reqDir: string): Promise<{ content: string; updatedAt: number } | null> {
  const p = join(reqDir, CODE_REVIEW_AI_FILE)
  if (!existsSync(p)) return null
  try {
    const [content, s] = await Promise.all([readFile(p, "utf-8"), stat(p)])
    return { content, updatedAt: s.mtimeMs }
  } catch {
    return null
  }
}
