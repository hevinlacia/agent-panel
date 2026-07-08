/**
 * Pi session scanner for dashboard harness compatibility.
 *
 * Role: read Pi's JSONL session files under `~/.pi/agent/sessions` and
 * normalize them into the dashboard's existing `SessionInfo` shape.
 * Public surface: scanPiSessions/getPiSession/getPiSessionsByIds,
 * clearPiSessionCache, isValidPiSessionId, PI_SESSION_ID_RE.
 * Constraints / safety: only reads Pi session JSONL files; session ids are
 * strict UUIDs before they are used by terminal spawning or association APIs.
 * Read-this-with: `src/sessions.ts` for the OpenCode scanner and
 * `src/terminal.ts` for harness-specific PTY spawning.
 */

import { readdir, readFile, stat } from "node:fs/promises"
import { existsSync } from "node:fs"
import { homedir } from "node:os"
import { join } from "node:path"

import { deriveWorktree, type SessionInfo } from "./sessions.ts"

export const PI_SESSION_ID_RE = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i
export const PI_SESSION_ROOT = join(homedir(), ".pi", "agent", "sessions")
export const PI_MAX_SESSIONS = 200
const CACHE_TTL_MS = 4_000

type PiHeader = {
  type?: unknown
  id?: unknown
  timestamp?: unknown
  cwd?: unknown
}

type PiEntry = {
  type?: unknown
  timestamp?: unknown
  provider?: unknown
  modelId?: unknown
  name?: unknown
  message?: unknown
}

let cache: { at: number; data: SessionInfo[] } | null = null

function toMs(value: unknown): number {
  if (typeof value !== "string") return 0
  const ms = Date.parse(value)
  return Number.isFinite(ms) ? ms : 0
}

function safeTruncate(value: string, max = 200): string {
  return value.length > max ? value.slice(0, max - 1) + "…" : value
}

function statusFromUpdated(updated: number, now = Date.now()): SessionInfo["status"] {
  if (!updated) return "stale"
  const ageMs = now - updated
  if (ageMs < 5 * 60_000) return "running"
  if (ageMs < 24 * 60 * 60_000) return "idle"
  return "stale"
}

function applyAgeFilter(sessions: SessionInfo[], maxAgeMs?: number): SessionInfo[] {
  if (!maxAgeMs || !Number.isFinite(maxAgeMs) || maxAgeMs <= 0) return sessions
  const cutoff = Date.now() - maxAgeMs
  return sessions.filter((s) => (s.updated || s.created || 0) >= cutoff)
}

function textFromUserMessage(message: unknown): string {
  if (!message || typeof message !== "object") return ""
  const msg = message as { role?: unknown; content?: unknown }
  if (msg.role !== "user" || !Array.isArray(msg.content)) return ""
  const parts: string[] = []
  for (const item of msg.content) {
    if (!item || typeof item !== "object") continue
    const block = item as { type?: unknown; text?: unknown }
    if (block.type === "text" && typeof block.text === "string") parts.push(block.text)
  }
  return safeTruncate(parts.join("\n").trim().replace(/\s+/g, " "))
}

async function readPiSessionFile(path: string): Promise<SessionInfo | null> {
  let st
  try {
    st = await stat(path)
  } catch {
    return null
  }
  let raw = ""
  try {
    raw = await readFile(path, "utf-8")
  } catch {
    return null
  }
  const lines = raw.split(/\r?\n/).filter(Boolean)
  if (lines.length === 0) return null

  let header: PiHeader
  try {
    header = JSON.parse(lines[0]) as PiHeader
  } catch {
    return null
  }
  if (header.type !== "session" || typeof header.id !== "string" || !isValidPiSessionId(header.id)) return null

  const cwd = typeof header.cwd === "string" ? header.cwd : ""
  const created = toMs(header.timestamp) || Math.floor(st.birthtimeMs || st.ctimeMs)
  const updated = Math.floor(st.mtimeMs || created)
  let title = ""
  let modelId = ""
  let modelProvider = ""

  for (const line of lines.slice(1, 160)) {
    let entry: PiEntry
    try {
      entry = JSON.parse(line) as PiEntry
    } catch {
      continue
    }
    if (!modelId && entry.type === "model_change") {
      if (typeof entry.modelId === "string") modelId = entry.modelId
      if (typeof entry.provider === "string") modelProvider = entry.provider
    }
    // Prefer the display name set via `pi --name` (stored in a
    // `session_info` entry) over the first user message.
    if (!title && entry.type === "session_info" && typeof entry.name === "string") {
      const n = entry.name.trim()
      if (n) title = safeTruncate(n)
    }
    if (!title && entry.type === "message") {
      title = textFromUserMessage(entry.message)
    }
    if (title && modelId) break
  }

  return {
    id: header.id,
    title: title || `pi ${header.id.slice(0, 8)}`,
    created,
    updated,
    projectId: "pi",
    directory: cwd,
    status: statusFromUpdated(updated || created),
    source: "fs",
    agent: "pi",
    worktree: deriveWorktree({ directory: cwd, path: null }),
    modelId: modelId || undefined,
    modelProvider: modelProvider || undefined,
  }
}

/** Scan all Pi session JSONL files and return dashboard-normalized rows. */
export async function scanPiSessions(force = false, maxAgeMs?: number): Promise<SessionInfo[]> {
  if (!force && cache && Date.now() - cache.at < CACHE_TTL_MS) {
    return applyAgeFilter(cache.data, maxAgeMs)
  }
  if (!existsSync(PI_SESSION_ROOT)) {
    cache = { at: Date.now(), data: [] }
    return []
  }
  let projectDirs
  try {
    projectDirs = await readdir(PI_SESSION_ROOT, { withFileTypes: true })
  } catch {
    cache = { at: Date.now(), data: [] }
    return []
  }

  const out: SessionInfo[] = []
  for (const dirent of projectDirs) {
    if (!dirent.isDirectory()) continue
    const dir = join(PI_SESSION_ROOT, dirent.name)
    let files
    try {
      files = await readdir(dir, { withFileTypes: true })
    } catch {
      continue
    }
    for (const file of files) {
      if (!file.isFile() || !file.name.endsWith(".jsonl")) continue
      const session = await readPiSessionFile(join(dir, file.name))
      if (session) out.push(session)
    }
  }
  out.sort((a, b) => b.updated - a.updated)
  cache = { at: Date.now(), data: out.slice(0, PI_MAX_SESSIONS) }
  return applyAgeFilter(cache.data, maxAgeMs)
}

export function clearPiSessionCache(): void {
  cache = null
}

export async function getPiSession(id: string): Promise<SessionInfo | null> {
  if (!isValidPiSessionId(id)) return null
  const all = await scanPiSessions()
  return all.find((s) => s.id === id) ?? null
}

export async function getPiSessionsByIds(ids: string[]): Promise<SessionInfo[]> {
  const valid = ids.filter(isValidPiSessionId)
  if (valid.length === 0) return []
  const set = new Set(valid)
  const all = await scanPiSessions(true)
  return all.filter((s) => set.has(s.id))
}

/** Validate a Pi session UUID before using it in argv or association APIs. */
export function isValidPiSessionId(id: string | null | undefined): id is string {
  return typeof id === "string" && PI_SESSION_ID_RE.test(id)
}
