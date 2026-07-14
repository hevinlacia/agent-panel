/**
 * File-based scheduler ownership lock for blue/green backends.
 *
 * Role: ensure only one agent-panel backend instance runs the background
 * schedulers (autoExtract, autoSummary, autoValuation, fullSync) at any time,
 * even though two backends may both be serving HTTP/WS during a hot deploy.
 *
 * Public surface: acquireSchedulerLock(), releaseSchedulerLock().
 * Constraints: O_EXCL atomic create + PID liveness check; no native deps.
 * Read-this-with: src/server.tsx bootstrap (calls these on startup/shutdown).
 */

import { closeSync, existsSync, mkdirSync, openSync, readFileSync, unlinkSync, writeFileSync } from "node:fs"
import { homedir } from "node:os"
import { dirname, join } from "node:path"

const DEFAULT_LOCK_FILE = join(homedir(), ".local", "state", "agent-panel", "scheduler.lock")

function lockFile(): string {
  return process.env.AGENT_PANEL_SCHEDULER_LOCK_FILE || DEFAULT_LOCK_FILE
}

function pidAlive(pid: number): boolean {
  try {
    process.kill(pid, 0)
    return true
  } catch {
    return false
  }
}

/**
 * Try to acquire the scheduler lock. Returns true if this process now owns it.
 * If a stale lock from a dead holder exists, it is reclaimed.
 */
export function acquireSchedulerLock(): boolean {
  const file = lockFile()
  mkdirSync(dirname(file), { recursive: true })

  if (existsSync(file)) {
    try {
      const pid = parseInt(readFileSync(file, "utf-8").trim(), 10)
      // Held by another live backend: stay out.
      if (!Number.isNaN(pid) && pid !== process.pid && pidAlive(pid)) return false
    } catch {
      // corrupt lock file: fall through to reclaim
    }
    try {
      unlinkSync(file)
    } catch {
      // raced or unreadable; try create below
    }
  }

  try {
    const fd = openSync(file, "wx") // O_CREAT | O_EXCL | O_WRONLY
    writeFileSync(fd, String(process.pid))
    closeSync(fd)
    return true
  } catch {
    return false // lost the create race
  }
}

/** Release the lock if this process owns it. Safe to call when not the owner. */
export function releaseSchedulerLock(): void {
  const file = lockFile()
  try {
    const pid = parseInt(readFileSync(file, "utf-8").trim(), 10)
    if (pid === process.pid) unlinkSync(file)
  } catch {
    // already gone or unreadable: nothing to do
  }
}
