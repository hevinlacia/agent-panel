/**
 * In-memory job store for asynchronous `opencode run` extract jobs.
 *
 * Role: track "extract context from session" tasks that run in the
 * background after the user clicks the button. The detail page polls
 * `/api/extract/job/:id` until the job is `done` or `failed`, then
 * navigates to the preview page which reads the stdout out of this
 * same store.
 *
 * Public surface:
 *   - createExtractJob({reqId,sessionId,prompt,model}) → starts the spawn and
 *     returns the freshly-stored job (state="running"). Throws if a job
 *     for the same sessionId is already running globally.
 *   - getExtractJob(jobId) → snapshot of the job, or null when missing
 *     (e.g. evicted by TTL).
 *   - findRunningJobForSession(sessionId) → for the mutex/UI restore.
 *   - _resetExtractJobs() → test-only reset of the singleton state.
 *
 * Constraints / safety:
 *   - Process-local Map; on dashboard restart all jobs are lost. We
 *     intentionally do NOT persist to disk — these tasks are short
 *     (≤120s) and re-runnable.
 *   - createExtractJob does its own spawn; it does NOT take a spawn fn
 *     parameter beyond a test injection point because we want one
 *     well-defined integration with `runExtractSummary`.
 *
 * Read-this-with:
 *   - `src/sessionExtract.ts` (the underlying spawn / prompt logic).
 *   - `src/server.tsx` routes `/api/requirement/extract-context` and
 *     `/api/extract/job/:id`.
 */

import { randomBytes } from "node:crypto"

import {
  runExtractSummary,
  DEFAULT_EXTRACT_TIMEOUT_MS,
  EXTRACT_MODEL,
  type ExtractResult,
  type RunExtractOptions,
} from "./sessionExtract.ts"
import {
  salvageFromFork,
  type SalvageResult,
} from "./forkSalvage.ts"
import {
  createNotification,
  updateNotification,
} from "./notifications.ts"
import {
  parseAutoExtractOutput,
  filterAllowed,
} from "./autoExtract.ts"
import {
  appendExtractHistory,
  buildExtractHistoryRecord,
} from "./extractHistory.ts"

export type JobState = "running" | "done" | "failed"
export type JobMode = "summary" | "auto"

export interface ExtractJob {
  id: string
  reqId: string
  sessionId: string
  state: JobState
  /** "summary" = plain markdown extract; "auto" = structured per-file diff. */
  mode: JobMode
  /** Model passed to `opencode run -m` for this extract job. */
  model: string
  startedAt: number
  doneAt: number | null
  /** Available once state !== "running". Empty string until then. */
  stdout: string
  stderr: string
  exitCode: number | null
  timedOut: boolean
  /** Human-readable error message when state==="failed". */
  errorMessage: string | null
  /**
   * Set when we recovered the assistant's text from a fork session in
   * opencode's SQLite (typically after a spawn timeout). The preview
   * page surfaces this so the user can either merge the salvaged text
   * to notes.md or open the fork to see the full thread.
   */
  forkSessionId: string | null
  forkTitle: string | null
  salvagedFromFork: boolean
  /**
   * Parsed result for mode="auto" jobs. Populated by finalizeJob
   * after the spawn completes and the output is parsed.
   */
  autoResult: import("./autoExtract.ts").AutoExtractResult | null
  /**
   * Internal: anchor text stored on the job at creation time so the
   * salvage step in `finalizeJob` can identify the right fork in the
   * database. Not serialized to the API.
   */
  _promptAnchor: string
  /**
   * Internal: salvage implementation (test seam). Not serialized.
   */
  _salvageFn: ((opts: { sourceSessionId: string; startedAt: number; promptAnchor: string }) => Promise<SalvageResult | null>) | null
  /**
   * Internal: notification id in the notification center so finalizeJob
   * can update the bell badge when state transitions. Not serialized.
   */
  _notificationId: string | null
}

/** TTL after which a finished job is evicted from memory. */
export const JOB_TTL_MS = 30 * 60 * 1000

const _jobs = new Map<string, ExtractJob>()

/** Generate a short, URL-safe job id (12 hex chars). */
function newJobId(): string {
  return randomBytes(6).toString("hex")
}

/** Drop done/failed jobs older than JOB_TTL_MS. Runs on every access. */
function evictStale(now: number): void {
  for (const [id, j] of _jobs) {
    if (j.state === "running") continue
    if (j.doneAt && now - j.doneAt > JOB_TTL_MS) {
      _jobs.delete(id)
    }
  }
}

/**
 * Return the in-flight job for `sessionId` if one exists.
 *
 * Why this exists: the UI policy is "one extract per session id at a
 * time" — we use this to (a) refuse duplicate start requests, and (b)
 * let a freshly-loaded page re-attach to an already-running job (e.g.
 * the user navigated away mid-run and came back).
 */
export function findRunningJobForSession(sessionId: string): ExtractJob | null {
  evictStale(Date.now())
  for (const j of _jobs.values()) {
    if (j.state === "running" && j.sessionId === sessionId) return j
  }
  return null
}

export function getExtractJob(jobId: string): ExtractJob | null {
  evictStale(Date.now())
  return _jobs.get(jobId) ?? null
}

export interface CreateExtractJobOptions {
  reqId: string
  sessionId: string
  prompt: string
  /** "summary" (default) or "auto" for structured per-file diff. */
  mode?: JobMode
  /** Model passed to `opencode run -m`. Defaults to EXTRACT_MODEL. */
  model?: string
  /** Test-only override to bypass real opencode spawn. */
  runFn?: (opts: RunExtractOptions) => Promise<ExtractResult>
  /** Test-only override for the wall-clock used in startedAt. */
  nowFn?: () => number
  /**
   * Test-only override for the SQLite salvage step. Production code
   * uses the real `salvageFromFork`.
   */
  salvageFn?: typeof salvageFromFork
  /**
   * First N characters of the prompt that uniquely identify our
   * dashboard's request in opencode's `part.data`. Defaults to the
   * first 30 chars of `prompt`. Exposed for tests so they can match
   * against a stub.
   */
  promptAnchor?: string
}

export class JobConflictError extends Error {
  constructor(public existingJobId: string) {
    super(`A job for this session is already running: ${existingJobId}`)
    this.name = "JobConflictError"
  }
}

/**
 * Start an extract job. Returns the seeded job record (state=running)
 * synchronously; the underlying `runExtractSummary` runs in the
 * background and mutates the same record on completion.
 *
 * Throws `JobConflictError` if another job for the same `sessionId` is
 * already running. Callers translate this into a user-visible 409.
 */
export function createExtractJob(opts: CreateExtractJobOptions): ExtractJob {
  const now = opts.nowFn ? opts.nowFn() : Date.now()
  evictStale(now)
  const conflict = findRunningJobForSession(opts.sessionId)
  if (conflict) throw new JobConflictError(conflict.id)

  const promptAnchor =
    opts.promptAnchor ?? opts.prompt.slice(0, 30)
  const model = opts.model && opts.model.trim() ? opts.model.trim() : EXTRACT_MODEL

  const job: ExtractJob = {
    id: newJobId(),
    reqId: opts.reqId,
    sessionId: opts.sessionId,
    state: "running",
    mode: opts.mode ?? "summary",
    model,
    startedAt: now,
    doneAt: null,
    stdout: "",
    stderr: "",
    exitCode: null,
    timedOut: false,
    errorMessage: null,
    forkSessionId: null,
    forkTitle: null,
    salvagedFromFork: false,
    autoResult: null,
    _promptAnchor: promptAnchor,
    _salvageFn: opts.salvageFn ?? salvageFromFork,
    _notificationId: null,
  }
  _jobs.set(job.id, job)

  // Add a "running" notification card for this job. Subsequent state
  // transitions (done/failed/salvaged) are pushed via updateNotification.
  const notifId = createNotification({
    type: "extract",
    title: "正在生成会话摘要…",
    subtitle: `session ${opts.sessionId}`,
    state: "running",
    jobId: job.id,
    reqId: opts.reqId,
    sessionId: opts.sessionId,
    actionHref: `/requirement/extract?jobId=${encodeURIComponent(job.id)}`,
  })
  job._notificationId = notifId

  const runner = opts.runFn ?? runExtractSummary
  // Fire and forget; the promise updates the job in-place on resolve.
  runner({ sessionId: opts.sessionId, prompt: opts.prompt, model })
    .then((result) => { void finalizeJob(job.id, result) })
    .catch((err: unknown) => {
      void finalizeJob(job.id, {
        stdout: "",
        stderr: err instanceof Error ? err.message : String(err),
        exitCode: null,
        durationMs: Date.now() - job.startedAt,
        timedOut: false,
      })
    })

  return { ...job }
}

async function persistJobHistory(job: ExtractJob): Promise<void> {
  const record = buildExtractHistoryRecord(job)
  if (!record) return
  await appendExtractHistory(record)
}

/**
 * Finalize a job once `runExtractSummary` resolves.
 *
 * The "happy path" (exit 0, non-empty stdout, no timeout) is trivial.
 * Anything else triggers a salvage attempt: we ask `salvageFromFork`
 * whether opencode wrote a fork session that contains the assistant
 * reply for our prompt. If it did, we promote the job to `state=done`
 * with the salvaged text — the user gets their summary even though
 * our spawn was killed. The fork id and title are recorded on the job
 * so the preview page can link there.
 *
 * Async because salvage spawns `sqlite3`. We `void` the returned
 * promise at the call site because nothing awaits it; the dashboard's
 * polling endpoint will see the updated job once finalize resolves.
 */
async function finalizeJob(jobId: string, result: ExtractResult): Promise<void> {
  const j = _jobs.get(jobId)
  if (!j) return
  j.stdout = result.stdout
  j.stderr = result.stderr
  j.exitCode = result.exitCode
  j.timedOut = result.timedOut
  j.doneAt = Date.now()

  // Happy path: opencode handed us the body directly.
  if (!result.timedOut && result.exitCode === 0 && result.stdout.length > 0) {
    j.state = "done"
    j.errorMessage = null
    // For "auto" mode, parse the structured output into per-file diffs.
    if (j.mode === "auto") {
      const parsed = parseAutoExtractOutput(result.stdout)
      j.autoResult = filterAllowed(parsed)
    }
    if (j._notificationId) {
      const dur = ((j.doneAt - j.startedAt) / 1000).toFixed(1)
      const title = j.mode === "auto"
        ? `✓ 上下文分析完成（${dur}s）`
        : `✓ 摘要生成完成（${dur}s）`
      const subtitle = j.mode === "auto"
        ? `session ${j.sessionId} · 进入预览页查看文件变更建议`
        : `session ${j.sessionId} · 进入预览页确认后写入 notes.md`
      updateNotification(j._notificationId, {
        title,
        subtitle,
        state: "done",
        actionHref: j.mode === "auto"
          ? `/requirement/auto-extract?jobId=${encodeURIComponent(j.id)}`
          : `/requirement/extract?jobId=${encodeURIComponent(j.id)}`,
      })
    }
    await persistJobHistory(j)
    return
  }

  // Attempt to salvage from the fork session opencode may have written
  // before our spawn was killed. The salvage is opportunistic; on any
  // error we fall through to the regular failure path.
  let salvage: SalvageResult | null = null
  if (j._salvageFn) {
    try {
      salvage = await j._salvageFn({
        sourceSessionId: j.sessionId,
        startedAt: j.startedAt,
        promptAnchor: j._promptAnchor,
      })
    } catch {
      // Treat any salvage error as "no salvage" — never throw out of
      // the background runner.
      salvage = null
    }
  }

  if (salvage && salvage.text.length > 0) {
    j.stdout = salvage.text
    j.forkSessionId = salvage.forkSessionId
    j.forkTitle = salvage.forkTitle
    j.salvagedFromFork = true
    j.state = "done"
    j.errorMessage = null
    if (j._notificationId) {
      const dur = ((j.doneAt - j.startedAt) / 1000).toFixed(1)
      updateNotification(j._notificationId, {
        title: `✓ 已从 fork 救回摘要（${dur}s）`,
        subtitle: `session ${j.sessionId} · 进程超时但 LLM 已写完`,
        state: "done",
      })
    }
    await persistJobHistory(j)
    return
  }

  j.state = "failed"
  if (result.timedOut) {
    j.errorMessage = describeTimeout(result)
  } else if (result.exitCode !== 0) {
    j.errorMessage = `opencode 退出码 ${result.exitCode ?? "null"}`
  } else {
    j.errorMessage = "opencode 没有输出"
  }
  if (j._notificationId) {
    updateNotification(j._notificationId, {
      title: "✗ 生成失败",
      subtitle: j.errorMessage || "未知错误",
      state: "failed",
    })
  }
  await persistJobHistory(j)
}

/**
 * Build a precise timeout description.
 *
 * We hit SIGKILL when wall-clock exceeds `DEFAULT_EXTRACT_TIMEOUT_MS`,
 * but "timeout" can mean very different things:
 *
 *   - **CLI start stall**: stdout is empty and stderr has < 1 line.
 *     opencode never got to load the session or invoke the model.
 *     Likely a wrong --model, missing provider key, or session id
 *     drift between SQLite and the running daemon.
 *   - **Model truly took too long**: stdout already started streaming
 *     markdown (we usually see "## 目标" within the first 3-10s once
 *     generation begins). The model is too slow on this much input.
 *   - **CLI post-processing stuck**: stdout has a complete-looking
 *     summary AND stderr shows a tokens/cost summary line, but the
 *     process didn't exit before the timeout. This is the one that
 *     burned us on minimax-latest-auto with 86k input tokens — the
 *     LLM finished but opencode kept the pipe open. The fix is to
 *     either raise the timeout further or salvage the partial stdout
 *     in the preview page (we already keep stdout in the job record).
 *
 * In all three cases we still hand the captured stdout/stderr to the
 * preview page; this string only adjusts the headline so the user
 * doesn't have to guess "did the model fail or did I just kill a
 * working process".
 */
function describeTimeout(result: ExtractResult): string {
  const seconds = Math.round(result.durationMs / 1000)
  const limit = Math.round(DEFAULT_EXTRACT_TIMEOUT_MS / 1000)
  const stdoutHasMarkdown = /(^|\n)#{1,3}\s/.test(result.stdout)
  const stderrMentionsFinish =
    /tokens?\b|cost\b|usage\b|finish/i.test(result.stderr) ||
    /\b(stop|completed|done)\b/i.test(result.stderr)

  if (result.stdout.length === 0 && result.stderr.length < 200) {
    return `opencode 在 ${seconds}s 内没有任何输出就被强制中断（上限 ${limit}s）。可能是模型加载、provider 鉴权或 session 找不到。`
  }
  if (stdoutHasMarkdown && stderrMentionsFinish) {
    return `LLM 已生成完毕（捕获到 ${result.stdout.length} 字节摘要），但 opencode 子进程 ${seconds}s 仍未退出，被强制中断（上限 ${limit}s）。摘要文本仍可在预览页中合并到 notes.md。`
  }
  if (stdoutHasMarkdown) {
    return `LLM 正在生成中（已捕获 ${result.stdout.length} 字节），但 ${seconds}s 仍未完成，被强制中断（上限 ${limit}s）。可在预览页中合并已有的部分文本，或重试。`
  }
  return `opencode 在 ${seconds}s 内未完成，被强制中断（上限 ${limit}s）。`
}

/** Test-only. Drops all in-memory jobs. */
export function _resetExtractJobs(): void {
  _jobs.clear()
}
