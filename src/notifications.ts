/**
 * Persistent, generic notification store for the dashboard.
 *
 * Role: aggregate async events (extract-context jobs, system alerts,
 * errors) into a UI-facing notification center. Each notification is
 * persisted to disk at `~/.local/share/opencode-dashboard/notifications.json`
 * so the bell badge and dropdown survive a dashboard restart.
 *
 * Public surface:
 *   - createNotification(opts) → id
 *   - updateNotification(id, partial)
 *   - dismissNotification(id)
 *   - dismissAll()
 *   - markAllRead()
 *   - getNotifications() (non-dismissed, newest-first)
 *   - getUnreadCount()
 *   - _resetForTest(path) / _loadForTest(path) — test-only lifecycle
 *
 * Constraints / safety:
 *   - Persisted JSON write is synchronous on mutation (no batching).
 *     This is acceptable because notification writes are rare (≤1/s).
 *   - 7-day TTL: done/failed notifications older than 7d are silently
 *     dropped on load. Running notifications are never TTL'd.
 *   - Does NOT import node-pty, extractJobs, or any native binding.
 *
 * Read-this-with:
 *   - `src/extractJobs.ts` (the primary notification producer).
 *   - `src/server.tsx` (the API routes that consume the store).
 *   - `public/notifications.js` (the bell / dropdown UI).
 */

import { mkdir, readFile, writeFile } from "node:fs/promises"
import { existsSync } from "node:fs"
import { homedir } from "node:os"
import { dirname, join } from "node:path"
import { randomBytes } from "node:crypto"

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

export type NotificationType = "extract" | "system" | "error"
export type NotificationState = "running" | "done" | "failed"

export interface Notification {
  id: string
  type: NotificationType
  title: string
  subtitle: string
  state: NotificationState
  /** Id of the underlying job (e.g. extract-context jobId). */
  jobId: string | null
  reqId: string | null
  sessionId: string | null
  /** Link for the "查看预览" / "查看详情" action. */
  actionHref: string | null
  unread: boolean
  createdAt: number
  /** null = not yet dismissed. */
  dismissedAt: number | null
}

interface PersistedStore {
  version: 1
  notifications: Notification[]
}

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/** 7 days in ms. */
const TTL_MS = 7 * 24 * 60 * 60 * 1000

const DEFAULT_STORE_PATH = join(
  homedir(),
  ".local",
  "share",
  "opencode-dashboard",
  "notifications.json",
)

let _storePath: string = DEFAULT_STORE_PATH
let _notifs = new Map<string, Notification>()

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function newId(): string {
  return randomBytes(6).toString("hex")
}

// ---------------------------------------------------------------------------
// Persistence
// ---------------------------------------------------------------------------

async function ensureDir(): Promise<void> {
  const dir = dirname(_storePath)
  if (!existsSync(dir)) {
    await mkdir(dir, { recursive: true })
  }
}

async function loadFromDisk(): Promise<void> {
  _notifs.clear()
  if (!existsSync(_storePath)) return
  try {
    const raw = await readFile(_storePath, "utf-8")
    const store: PersistedStore = JSON.parse(raw)
    const now = Date.now()
    for (const n of store.notifications || []) {
      if (n.state !== "running" && n.dismissedAt === null && now - n.createdAt > TTL_MS) {
        // Auto-remove stale done/failed notifications.
        continue
      }
      _notifs.set(n.id, n)
    }
  } catch {
    // Corrupted file; start fresh.
    _notifs.clear()
  }
}

async function saveToDisk(): Promise<void> {
  await ensureDir()
  const now = Date.now()
  const all: Notification[] = []
  for (const n of _notifs.values()) {
    // Exclude done/failed notifications past TTL from the persisted file,
    // but keep running + recent + dismissed ones until explicitly cleaned.
    if (n.state !== "running" && n.dismissedAt === null && now - n.createdAt > TTL_MS) {
      continue
    }
    all.push(n)
  }
  const store: PersistedStore = { version: 1, notifications: all }
  await writeFile(_storePath, JSON.stringify(store, null, 2), "utf-8")
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/** Initialize (load from disk). Call once at server startup. */
export async function initNotifications(): Promise<void> {
  await loadFromDisk()
}

export function createNotification(opts: {
  type: NotificationType
  title: string
  subtitle?: string
  state?: NotificationState
  jobId?: string | null
  reqId?: string | null
  sessionId?: string | null
  actionHref?: string | null
}): string {
  const id = newId()
  const n: Notification = {
    id,
    type: opts.type,
    title: opts.title,
    subtitle: opts.subtitle ?? "",
    state: opts.state ?? "running",
    jobId: opts.jobId ?? null,
    reqId: opts.reqId ?? null,
    sessionId: opts.sessionId ?? null,
    actionHref: opts.actionHref ?? null,
    unread: true,
    createdAt: Date.now(),
    dismissedAt: null,
  }
  _notifs.set(id, n)
  saveToDisk().catch(() => {})
  return id
}

export function updateNotification(
  id: string,
  partial: Partial<Pick<Notification, "title" | "subtitle" | "state" | "actionHref" | "unread">>,
): void {
  const n = _notifs.get(id)
  if (!n) return
  if (partial.title !== undefined) n.title = partial.title
  if (partial.subtitle !== undefined) n.subtitle = partial.subtitle
  if (partial.state !== undefined) n.state = partial.state
  if (partial.actionHref !== undefined) n.actionHref = partial.actionHref
  if (partial.unread !== undefined) n.unread = partial.unread
  if (partial.state !== undefined && partial.state !== "running") {
    // Mark as unread when transitioning out of running.
    n.unread = true
  }
  saveToDisk().catch(() => {})
}

export function dismissNotification(id: string): void {
  const n = _notifs.get(id)
  if (!n) return
  n.dismissedAt = Date.now()
  n.unread = false
  saveToDisk().catch(() => {})
}

export function dismissAll(): void {
  const now = Date.now()
  for (const n of _notifs.values()) {
    n.dismissedAt = now
    n.unread = false
  }
  saveToDisk().catch(() => {})
}

export function markAllRead(): void {
  for (const n of _notifs.values()) {
    if (n.state !== "running") n.unread = false
  }
  saveToDisk().catch(() => {})
}

/**
 * Return non-dismissed notifications, newest-first.
 * Callers may filter further by state or type.
 */
export function getNotifications(includeDismissed = false): Notification[] {
  const out: Notification[] = []
  for (const n of _notifs.values()) {
    if (!includeDismissed && n.dismissedAt !== null) continue
    out.push(n)
  }
  out.sort((a, b) => b.createdAt - a.createdAt)
  return out
}

export function getUnreadCount(): number {
  let count = 0
  for (const n of _notifs.values()) {
    if (n.unread) count++
  }
  return count
}

export function getNotification(id: string): Notification | null {
  return _notifs.get(id) ?? null
}

// ---------------------------------------------------------------------------
// Test helpers
// ---------------------------------------------------------------------------

/** Override the persistent store path. Also clears the in-memory map. */
export function _resetForTest(path: string): void {
  _storePath = path
  _notifs.clear()
}

/** Load from the given JSON string (bypasses disk). */
export function _loadForTest(json: string): void {
  _notifs.clear()
  try {
    const store: PersistedStore = JSON.parse(json)
    for (const n of store.notifications || []) {
      _notifs.set(n.id, n)
    }
  } catch {
    _notifs.clear()
  }
}