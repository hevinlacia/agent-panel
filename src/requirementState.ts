/**
 * Per-requirement state file at `<req-dir>/state.json`.
 *
 * Status is intentionally stored in a machine-friendly JSON file so the
 * dashboard, hermes skills and any other agent can read/write it without
 * parsing Markdown. The companion `meta.md` stays human-readable and is
 * NOT modified by the dashboard once state.json exists.
 *
 * Schema (version 1):
 *   {
 *     "version": 1,
 *     "status": "<one of REQ_STATUSES>",
 *     "updatedAt": <epoch ms>,
 *     "history": [
 *       { "status": "<new>", "from": "<old|null>", "at": <epoch ms>, "note": "<optional>" },
 *       ...
 *     ]
 *   }
 *
 * Migration: if state.json is missing but `<req-dir>/meta.md` contains a
 * line like `- Status: <english-value>` (hermes legacy format), the
 * english value is mapped to the chinese 7-stage vocabulary and a
 * state.json file is created. After migration, state.json wins.
 *
 * Only `node:` built-ins are used. Never touches `.env` / secret files.
 */

import { readFile, writeFile, rename, mkdir } from "node:fs/promises"
import { existsSync } from "node:fs"
import { dirname, join } from "node:path"

import { REQ_STATUSES, type ReqStatus } from "./requirements.ts"

export const STATE_FILE = "state.json"
const STATE_VERSION = 1
const MAX_HISTORY = 50

export interface StateTransition {
  status: ReqStatus
  from: ReqStatus | null
  at: number
  note?: string
}

export interface RequirementState {
  version: number
  status: ReqStatus
  updatedAt: number
  history: StateTransition[]
}

/**
 * Map a hermes meta.md `- Status: <value>` english label to the
 * dashboard's chinese 7-stage vocabulary. Unknown values map to
 * `开发中` (the dashboard's "in progress" default).
 *
 * Kept inline as a string table so it is easy to extend; not exported
 * as a Map because we want exhaustive case matching.
 */
export function mapHermesStatusToReqStatus(raw: string): ReqStatus {
  const v = raw.trim().toLowerCase()
  switch (v) {
    case "intake":
    case "design":
    case "designing":
      return "待设计"
    case "ready":
    case "planned":
    case "pending":
      return "待开发"
    case "dev":
    case "developing":
    case "in-progress":
    case "wip":
      return "开发中"
    case "selftest":
    case "self-test":
    case "self_testing":
      return "自测中"
    case "test":
    case "testing":
    case "qa":
      return "测试中"
    case "deploy":
    case "release":
    case "ready-to-release":
      return "待上线"
    case "done":
    case "completed":
    case "released":
      return "已完成"
    default:
      return "开发中"
  }
}

/**
 * Extract `- Status: <value>` from a meta.md text block. Returns null
 * if no such line is present. The match is case-insensitive on the
 * leading "Status" key but value is kept verbatim for the caller to
 * map.
 */
export function extractHermesStatus(metaText: string): string | null {
  const lines = metaText.replace(/\r\n/g, "\n").split("\n")
  for (const line of lines) {
    const m = line.match(/^\s*-\s*Status\s*:\s*(.+?)\s*$/i)
    if (m) return m[1]
  }
  return null
}

function isReqStatus(v: unknown): v is ReqStatus {
  return typeof v === "string" && (REQ_STATUSES as string[]).includes(v)
}

function statePath(reqDir: string): string {
  return join(reqDir, STATE_FILE)
}

function makeInitialState(status: ReqStatus, note?: string): RequirementState {
  const now = Date.now()
  return {
    version: STATE_VERSION,
    status,
    updatedAt: now,
    history: [{ status, from: null, at: now, note }],
  }
}

async function atomicWrite(path: string, body: string): Promise<void> {
  const dir = dirname(path)
  if (!existsSync(dir)) {
    await mkdir(dir, { recursive: true })
  }
  const tmp = `${path}.tmp.${process.pid}.${Date.now()}`
  await writeFile(tmp, body, "utf-8")
  await rename(tmp, path)
}

/**
 * Read state.json from `reqDir`. If absent, attempt a one-shot migration
 * from `<reqDir>/meta.md`'s `- Status:` line. Returns null only when no
 * state can be derived (no state.json AND no meta.md/Status line).
 */
export async function readRequirementState(reqDir: string): Promise<RequirementState | null> {
  const sp = statePath(reqDir)
  if (existsSync(sp)) {
    try {
      const raw = await readFile(sp, "utf-8")
      const parsed = JSON.parse(raw) as unknown
      if (parsed && typeof parsed === "object") {
        const o = parsed as Record<string, unknown>
        const status = isReqStatus(o.status) ? o.status : null
        if (status) {
          const updatedAt = typeof o.updatedAt === "number" ? o.updatedAt : Date.now()
          const history: StateTransition[] = []
          if (Array.isArray(o.history)) {
            for (const h of o.history) {
              if (!h || typeof h !== "object") continue
              const hr = h as Record<string, unknown>
              if (!isReqStatus(hr.status)) continue
              history.push({
                status: hr.status,
                from: isReqStatus(hr.from) ? hr.from : null,
                at: typeof hr.at === "number" ? hr.at : 0,
                note: typeof hr.note === "string" && hr.note ? hr.note : undefined,
              })
            }
          }
          return {
            version: STATE_VERSION,
            status,
            updatedAt,
            history,
          }
        }
      }
    } catch {
      // Fall through to migration / null below.
    }
  }

  // Migration: read meta.md if present.
  const metaPath = join(reqDir, "meta.md")
  if (existsSync(metaPath)) {
    try {
      const raw = await readFile(metaPath, "utf-8")
      const hermesRaw = extractHermesStatus(raw)
      if (hermesRaw) {
        const mapped = mapHermesStatusToReqStatus(hermesRaw)
        const state = makeInitialState(mapped, `migrated from meta.md (- Status: ${hermesRaw})`)
        // Persist so subsequent reads are cheap and writes are diff-safe.
        try {
          await atomicWrite(sp, JSON.stringify(state, null, 2) + "\n")
        } catch {
          // Read-only filesystem or perms — return the in-memory state anyway.
        }
        return state
      }
    } catch {
      // ignore
    }
  }

  return null
}

/**
 * Set the requirement's status. Writes state.json atomically and appends
 * a transition entry. Returns the new state. If state.json doesn't yet
 * exist, falls through to readRequirementState first so we still capture
 * the previous status from meta.md migration before overwriting.
 */
export async function writeRequirementStatus(
  reqDir: string,
  newStatus: ReqStatus,
  note?: string,
): Promise<RequirementState> {
  if (!isReqStatus(newStatus)) {
    throw new Error(`Invalid status: ${String(newStatus)}`)
  }
  const previous = await readRequirementState(reqDir)
  const now = Date.now()
  let history: StateTransition[] = previous?.history ? [...previous.history] : []
  if (!previous) {
    history = []
  }
  if (!previous || previous.status !== newStatus) {
    history.push({
      status: newStatus,
      from: previous?.status ?? null,
      at: now,
      note: note && note.trim() ? note.trim() : undefined,
    })
  }
  // Cap history so a long-lived requirement file doesn't grow unbounded.
  if (history.length > MAX_HISTORY) {
    history = history.slice(history.length - MAX_HISTORY)
  }
  const next: RequirementState = {
    version: STATE_VERSION,
    status: newStatus,
    updatedAt: now,
    history,
  }
  await atomicWrite(statePath(reqDir), JSON.stringify(next, null, 2) + "\n")
  return next
}

/**
 * Compute the "next" status one step forward in REQ_STATUSES (e.g.
 * `开发中 → 自测中`). Returns null when already at the final stage.
 */
export function nextStatus(current: ReqStatus): ReqStatus | null {
  const i = REQ_STATUSES.indexOf(current)
  if (i < 0 || i >= REQ_STATUSES.length - 1) return null
  return REQ_STATUSES[i + 1]
}
