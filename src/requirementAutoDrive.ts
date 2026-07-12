/**
 * Requirement auto-drive job store and prompt contract.
 *
 * Role: track dashboard-launched pi agents that automatically advance
 * requirements until they hit a human-review or uncertainty gate.
 * Public surface: job lifecycle helpers, prompt builder, and result classifier.
 * Constraints: no shell execution here; server.tsx owns process spawning.
 * Read-this-with: src/server.tsx auto-drive routes and src/notifications.ts.
 */

import { mkdir, readFile, writeFile, rename } from "node:fs/promises"
import { existsSync } from "node:fs"
import { randomBytes } from "node:crypto"
import { homedir } from "node:os"
import { dirname, join } from "node:path"

import type { Requirement, ReqStatus } from "./requirements.ts"
import type { QueuedOpencodeProcessResult } from "./opencodeProcessQueue.ts"

export type AutoDriveJobState = "queued" | "running" | "blocked" | "done" | "failed"

export interface AutoDriveJob {
  id: string
  reqId: string
  reqTitle: string
  reqStatus: ReqStatus
  reqDir: string | null
  state: AutoDriveJobState
  phase: ReqStatus
  sessionId: string | null
  notificationId: string | null
  summary: string
  blockers: string[]
  stdout: string
  stderr: string
  exitCode: number | null
  timedOut: boolean
  queuedMs: number
  durationMs: number
  createdAt: number
  updatedAt: number
  startedAt: number | null
  doneAt: number | null
}

interface PersistedStore {
  version: 1
  jobs: AutoDriveJob[]
}

const DEFAULT_STORE_PATH = join(
  homedir(),
  ".local",
  "share",
  "agent-panel",
  "auto-drive-jobs.json",
)
const STORE_VERSION = 1
const MAX_JOBS = 300
const OUTPUT_CAP = 24_000

let _storePath = DEFAULT_STORE_PATH
let _jobs = new Map<string, AutoDriveJob>()

function newJobId(): string {
  return `drive_${randomBytes(6).toString("hex")}`
}

function clipText(text: string): string {
  if (text.length <= OUTPUT_CAP) return text
  return `${text.slice(0, OUTPUT_CAP)}\n…[truncated]`
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

/** Load persisted auto-drive jobs. Call once at server startup. */
export async function initAutoDriveJobs(): Promise<void> {
  await loadFromDisk()
}

/** Create a queued job snapshot before the pi process actually starts. */
export function createAutoDriveJob(
  req: Requirement,
  sessionId: string,
  notificationId: string | null,
): AutoDriveJob {
  const now = Date.now()
  const job: AutoDriveJob = {
    id: newJobId(),
    reqId: req.id,
    reqTitle: req.title,
    reqStatus: req.status,
    reqDir: req.reqDir ?? null,
    state: "queued",
    phase: req.status,
    sessionId,
    notificationId,
    summary: "已加入自动推进队列。",
    blockers: [],
    stdout: "",
    stderr: "",
    exitCode: null,
    timedOut: false,
    queuedMs: 0,
    durationMs: 0,
    createdAt: now,
    updatedAt: now,
    startedAt: null,
    doneAt: null,
  }
  _jobs.set(job.id, job)
  saveToDisk().catch(() => {})
  return job
}

/** Patch one job in place and persist it for dashboard polling. */
export function updateAutoDriveJob(
  id: string,
  partial: Partial<Omit<AutoDriveJob, "id" | "createdAt">>,
): AutoDriveJob | null {
  const current = _jobs.get(id)
  if (!current) return null
  const next: AutoDriveJob = {
    ...current,
    ...partial,
    stdout: partial.stdout !== undefined ? clipText(partial.stdout) : current.stdout,
    stderr: partial.stderr !== undefined ? clipText(partial.stderr) : current.stderr,
    updatedAt: partial.updatedAt ?? Date.now(),
  }
  _jobs.set(id, next)
  saveToDisk().catch(() => {})
  return next
}

/** Return jobs newest-first, optionally narrowed to one requirement. */
export function getAutoDriveJobs(opts: { reqId?: string } = {}): AutoDriveJob[] {
  return [..._jobs.values()]
    .filter((job) => !opts.reqId || job.reqId === opts.reqId)
    .sort((a, b) => b.updatedAt - a.updatedAt)
}

/** Return the newest job for a requirement, used to block duplicate launches. */
export function getLatestAutoDriveJobForRequirement(reqId: string): AutoDriveJob | null {
  return getAutoDriveJobs({ reqId })[0] ?? null
}

/** Build the pi session name shown in session lists and detail pages. */
export function buildAutoDriveJobName(req: Requirement): string {
  return `${req.id} 自动推进 ${req.title}`.slice(0, 100)
}

/** Build the non-interactive pi prompt with explicit human gate rules. */
export function buildAutoDrivePrompt(req: Requirement): string {
  return [
    `你正在自动推进 Agent Panel 需求：${req.id} — ${req.title}`,
    `当前状态：${req.status}`,
    req.reqDir ? `需求目录：${req.reqDir}` : "需求目录：未知",
    "",
    "目标：在不牺牲准确性和可靠性的前提下，自动完成当前阶段里确定性的工作。",
    "你可以读取需求上下文、扫描/修改关联代码、运行检查、补充需求文件，并在证据足够时推进状态。",
    "",
    "硬性人工门禁：",
    "- 需求对齐：业务目标、包含范围、排除范围、验收标准、关键流程或开放问题未确认时，必须停止并列问题清单。",
    "- 方案设计：影响面、数据/配置变更、回滚方案或技术路径不清楚时，必须停止并列问题清单。",
    "- 测试覆盖：核心场景、异常分支、回归范围、日志/DB/副作用/反向检查标准不完整时，必须停止并让用户审核测试矩阵。",
    "- 上线风险：涉及 DB、Apollo/Nacos、MQ、数据订正、删除逻辑、兼容性或发布顺序不确定时，必须停止并列风险清单。",
    "- 不要因为想继续推进而替用户确认业务口径；如果你不确定自己是否理解，也要把不确定点列出来。",
    "",
    "执行要求：",
    "1. 先读取已注入的需求上下文和需求目录中的文件。",
    "2. 只做确定性的自动推进；遇到人工审核/歧义/证据不足立即停止。",
    "3. 若修改了代码或需求文件，运行可用的最小验证，并记录结果。",
    "4. 不要编造测试证据、日志、DB 结果或用户确认结论。",
    "",
    "最终回答必须包含以下机器可读区块：",
    "AUTO_DRIVE_STATUS: DONE 或 BLOCKED",
    "SUMMARY:",
    "- 本次完成的推进动作",
    "BLOCKERS:",
    "- 如果需要人工审核或有不清楚的问题，逐条列出；没有则写 none",
    "NEXT_ACTIONS:",
    "- 建议下一步",
  ].join("\n")
}

/** Extract human-review blockers from the pi agent final output. */
export function extractAutoDriveBlockers(text: string): string[] {
  const lines = text.replace(/\r\n/g, "\n").split("\n")
  const blockers: string[] = []
  let inBlock = false
  const headingRe = /^(?:#+\s*)?(BLOCKERS|阻塞|问题清单|待确认|人工审核|需要人工|需确认|Open Questions|Questions)\b/i
  const sectionHeadingRe = /^(?:#+\s*)?[A-Z][A-Z_ ]{2,}\s*[:：]?$/
  const keywordRe = /(需要人工|人工审核|待确认|不清楚|不确定|阻塞|疑问|证据不足|测试.*覆盖|验收.*标准)/
  for (const raw of lines) {
    const line = raw.trim()
    if (!line) {
      if (inBlock) inBlock = false
      continue
    }
    const normalizedHeading = line.replace(/[:：]$/, "")
    if (headingRe.test(normalizedHeading)) {
      inBlock = true
      continue
    }
    if (inBlock && sectionHeadingRe.test(line)) {
      inBlock = false
      continue
    }
    const clean = line.replace(/^[-*]\s+/, "").replace(/^\d+[.)]\s+/, "").trim()
    if (!clean || /^(none|无|没有|n\/a)$/i.test(clean)) continue
    if (inBlock || keywordRe.test(clean)) blockers.push(clean)
    if (blockers.length >= 12) break
  }
  if (blockers.length === 0 && keywordRe.test(text)) {
    blockers.push("agent 输出包含人工确认/阻塞信号，请打开详情查看完整日志。")
  }
  return [...new Set(blockers)]
}

/** Convert a finished process result into the persisted auto-drive outcome. */
export function finalizeAutoDriveJobFromResult(
  jobId: string,
  result: QueuedOpencodeProcessResult,
): AutoDriveJob | null {
  const text = `${result.stdout}\n${result.stderr}`
  let state: AutoDriveJobState = "done"
  let summary = "pi agent 已完成自动推进。"
  let blockers = extractAutoDriveBlockers(text)
  if (result.timedOut) {
    state = "failed"
    summary = "pi agent 运行超时，已被终止。"
    blockers = ["自动推进超过 1 小时上限，请人工检查该需求或重新启动。", ...blockers]
  } else if (result.exitCode !== 0) {
    state = "failed"
    summary = `pi agent 异常退出（exit ${result.exitCode ?? "unknown"}）。`
    if (blockers.length === 0) blockers = ["自动推进进程异常退出，请查看 stdout/stderr。"]
  } else if (/AUTO_DRIVE_STATUS\s*:\s*BLOCKED/i.test(text) || blockers.length > 0) {
    state = "blocked"
    summary = "pi agent 已暂停，等待人工审核或补充信息。"
  } else if (/AUTO_DRIVE_STATUS\s*:\s*DONE/i.test(text)) {
    state = "done"
  }
  return updateAutoDriveJob(jobId, {
    state,
    summary,
    blockers,
    stdout: result.stdout,
    stderr: result.stderr,
    exitCode: result.exitCode,
    timedOut: result.timedOut,
    queuedMs: result.queuedMs,
    durationMs: result.durationMs,
    doneAt: Date.now(),
  })
}

/** Test-only: override the persistent store path and clear memory. */
export function _resetAutoDriveJobsForTest(path: string): void {
  _storePath = path
  _jobs.clear()
}

/** Test-only: seed jobs from a serialized store without touching disk. */
export function _loadAutoDriveJobsForTest(json: string): void {
  _jobs.clear()
  try {
    const store = JSON.parse(json) as PersistedStore
    for (const job of store.jobs || []) _jobs.set(job.id, job)
  } catch {
    _jobs.clear()
  }
}
