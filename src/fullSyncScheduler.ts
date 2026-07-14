/**
 * Daily full-sync scheduler for local OpenCode configuration.
 *
 * Role: run the existing workstation-bootstrap full sync script at configured
 * local times from the dashboard process, including optional GitHub repo syncs.
 * Public surface: startFullSyncScheduler(), stopFullSyncScheduler(),
 * isFullSyncSchedulerRunning(), triggerFullSync(), msUntilNextLocalTime(),
 * normalizeFullSyncTimes().
 * Constraints: fixed argv only; never reads .env / secret files.
 * Read-this-with: src/server.tsx schedulers page and config.ts toggle.
 */

import { spawn } from "node:child_process"
import { execFileSync } from "node:child_process"
import { existsSync } from "node:fs"
import { homedir } from "node:os"
import { basename, join } from "node:path"

import { getConfig } from "./config.ts"

export const FULL_SYNC_HOUR = 20
export const FULL_SYNC_MINUTE = 30
export const DEFAULT_FULL_SYNC_TIMES = ["12:00", "18:00", "20:30", "23:30"] as const

export const FULL_SYNC_SCRIPT = join(
  homedir(),
  "Developer",
  "infra",
  "workstation-bootstrap",
  "scripts",
  "sync-all-to-github.sh",
)

export const POLL_INTERVAL_MS = 24 * 60 * 60 * 1000

export interface FullSyncResult {
  ok: boolean
  exitCode: number | null
  stdout: string
  stderr: string
  startedAt: number
  finishedAt: number
}

const OUTPUT_CAP_BYTES = 64 * 1024
let _timer: ReturnType<typeof setTimeout> | null = null
let _lastResult: FullSyncResult | null = null

/** Milliseconds until the next local HH:mm occurrence. */
export function msUntilNextLocalTime(
  hour: number,
  minute: number,
  now: Date = new Date(),
): number {
  const h = Math.max(0, Math.min(23, Math.floor(hour)))
  const m = Math.max(0, Math.min(59, Math.floor(minute)))
  const next = new Date(now)
  next.setHours(h, m, 0, 0)
  if (next.getTime() <= now.getTime()) {
    next.setDate(next.getDate() + 1)
  }
  return next.getTime() - now.getTime()
}

/** Normalize user-configured HH:mm sync times, falling back to the dashboard default schedule. */
export function normalizeFullSyncTimes(times: readonly string[] | null | undefined): string[] {
  const seen = new Set<string>()
  for (const raw of times ?? []) {
    const match = raw.trim().match(/^(\d{1,2}):(\d{2})$/)
    if (!match) continue
    const hour = Number(match[1])
    const minute = Number(match[2])
    if (!Number.isInteger(hour) || !Number.isInteger(minute) || hour < 0 || hour > 23 || minute < 0 || minute > 59) continue
    seen.add(`${String(hour).padStart(2, "0")}:${String(minute).padStart(2, "0")}`)
  }
  return seen.size > 0 ? [...seen].sort() : [...DEFAULT_FULL_SYNC_TIMES]
}

function msUntilNextConfiguredTime(times: readonly string[], now: Date = new Date()): number {
  return Math.min(...normalizeFullSyncTimes(times).map((time) => {
    const [hour, minute] = time.split(":").map(Number)
    return msUntilNextLocalTime(hour, minute, now)
  }))
}

/** Run one full sync via sync-all-to-github.sh, then optionally pull selected GitHub repos. */
export function triggerFullSync(opts?: {
  syncScript?: string
  githubRepos?: readonly string[]
  spawnFn?: typeof spawn
  nowFn?: () => number
}): Promise<FullSyncResult> {
  const syncScript = opts?.syncScript ?? FULL_SYNC_SCRIPT
  const sp = opts?.spawnFn ?? spawn
  const startedAt = opts?.nowFn ? opts.nowFn() : Date.now()

  return new Promise<FullSyncResult>((resolve) => {
    if (!existsSync(syncScript)) {
      const result = {
        ok: false,
        exitCode: null,
        stdout: "",
        stderr: `Sync script not found: ${syncScript}`,
        startedAt,
        finishedAt: opts?.nowFn ? opts.nowFn() : Date.now(),
      }
      _lastResult = result
      resolve(result)
      return
    }

    // sync-all-to-github.sh needs no special flags for default full sync.
    // It self-updates workstation-bootstrap, syncs Developer + ai-code-config +
    // all simple repos (personal/playground/tools), and pushes to GitHub.
    let child: ReturnType<typeof spawn>
    try {
      child = sp(syncScript, [], {
        stdio: ["ignore", "pipe", "pipe"],
        env: {
          ...process.env,
          OPENCODE_SYNC_SOURCE: "dashboard-full-sync",
        },
      })
    } catch (err) {
      const result = {
        ok: false,
        exitCode: null,
        stdout: "",
        stderr: err instanceof Error ? err.message : String(err),
        startedAt,
        finishedAt: opts?.nowFn ? opts.nowFn() : Date.now(),
      }
      _lastResult = result
      resolve(result)
      return
    }

    let stdout = ""
    let stderr = ""
    child.stdout?.on("data", (d: Buffer) => {
      if (stdout.length >= OUTPUT_CAP_BYTES) return
      stdout += d.toString("utf-8")
      if (stdout.length > OUTPUT_CAP_BYTES) stdout = stdout.slice(0, OUTPUT_CAP_BYTES)
    })
    child.stderr?.on("data", (d: Buffer) => {
      if (stderr.length >= OUTPUT_CAP_BYTES) return
      stderr += d.toString("utf-8")
      if (stderr.length > OUTPUT_CAP_BYTES) stderr = stderr.slice(0, OUTPUT_CAP_BYTES)
    })
    child.on("error", (err) => {
      stderr += (stderr ? "\n" : "") + (err instanceof Error ? err.message : String(err))
    })
    child.on("close", (code) => {
      // After sync-all completes, optionally pull selected third-party GitHub repos.
      const githubRepos = opts?.githubRepos ?? []
      if (githubRepos.length > 0) {
        const pullSummary = pullGithubRepos(githubRepos, sp)
        stdout += (stdout ? "\n" : "") + pullSummary
      }
      const result = {
        ok: code === 0,
        exitCode: code,
        stdout: stdout.trim(),
        stderr: stderr.trim(),
        startedAt,
        finishedAt: opts?.nowFn ? opts.nowFn() : Date.now(),
      }
      _lastResult = result
      resolve(result)
    })
  })
}

/** Run git pull --ff-only on selected repos, returning a summary string. */
function pullGithubRepos(repoPaths: readonly string[], _sp: typeof spawn): string {
  const lines: string[] = ["\n--- GitHub repos pull ---"]
  for (const repoPath of repoPaths) {
    if (!existsSync(join(repoPath, ".git"))) continue
    try {
      const out = execFileSync("git", ["-C", repoPath, "pull", "--ff-only"], {
        stdio: ["ignore", "pipe", "pipe"],
        timeout: 30_000,
      }).toString("utf-8").trim()
      lines.push(`${basename(repoPath)}: ${out || "up to date"}`)
    } catch (err) {
      lines.push(`${basename(repoPath)}: ERROR ${err instanceof Error ? err.message : String(err)}`)
    }
  }
  return lines.join("\n")
}

/** Start the configured full-sync scheduler. */
export function startFullSyncScheduler(): void {
  if (_timer) return
  void scheduleNextFullSync()
}

async function scheduleNextFullSync(): Promise<void> {
  const cfg = await getConfig().catch(() => null)
  const delay = msUntilNextConfiguredTime(cfg?.fullSyncTimes ?? DEFAULT_FULL_SYNC_TIMES)
  _timer = setTimeout(() => {
    void (async () => {
      const nextCfg = await getConfig()
      if (nextCfg.fullSyncSchedule) {
        await triggerFullSync({ githubRepos: nextCfg.fullSyncGithubRepos })
      }
    })()
      .catch(() => {})
      .finally(() => {
        _timer = null
        void scheduleNextFullSync()
      })
  }, delay)
  if (typeof _timer.unref === "function") _timer.unref()
}

/** Stop the daily full-sync scheduler. */
export function stopFullSyncScheduler(): void {
  if (!_timer) return
  clearTimeout(_timer)
  _timer = null
}

/** Whether the daily full-sync scheduler is currently scheduled. */
export function isFullSyncSchedulerRunning(): boolean {
  return _timer !== null
}

/** Last result from triggerFullSync(), if any in this process. */
export function getLastFullSyncResult(): FullSyncResult | null {
  return _lastResult
}
