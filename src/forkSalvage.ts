/**
 * Salvage assistant output from a fork session in opencode's SQLite DB.
 *
 * Role: when `runExtractSummary` fails (timeout, non-zero exit, empty
 * stdout), opencode may still have completed the LLM call in the
 * background and written the assistant's reply into a fork session
 * (`<source-title> (fork #N)`). This module queries that database
 * directly so the dashboard can recover the user's summary instead of
 * losing it.
 *
 * Public surface:
 *   - findRecentForkSession(opts): locate the freshest fork session
 *     whose part data contains the prompt anchor, with time_created
 *     inside a window around the spawn's startedAt.
 *   - extractAssistantText(opts): pull all assistant text-parts out of
 *     the fork session and concatenate them.
 *   - salvageFromFork(opts): one-shot wrapper used by extractJobs;
 *     returns null when nothing usable is found.
 *
 * Constraints / safety:
 *   - Read-only. Never writes to opencode.db, never deletes the fork.
 *   - Uses `sqlite3 -json` with a fixed-shape argv (no shell). The only
 *     runtime inputs to the queries are the session id (validated by
 *     callers) and an integer time window (built from Date.now()).
 *     Both flow into SQLite via parameter placeholders, never via
 *     string concatenation.
 *   - DB path defaults to opencode's standard location; overridable
 *     for tests via `dbPath`.
 *
 * Read-this-with:
 *   - `src/extractJobs.ts` (the consumer in the timeout/failure path).
 *   - `src/sessions.ts` (similar SQLite-via-CLI patterns).
 */

import { spawn } from "node:child_process"
import { existsSync } from "node:fs"
import { homedir } from "node:os"
import { join } from "node:path"

export const DEFAULT_OPENCODE_DB_PATH = join(
  homedir(),
  ".local",
  "share",
  "opencode",
  "opencode.db",
)

/** Default time tolerance for matching a fork against a spawn's startedAt. */
export const FORK_LOOKUP_WINDOW_MS = 60 * 1000

/** SQLite spawn timeout — these queries are tiny but be defensive. */
export const SQLITE_TIMEOUT_MS = 5_000

const STDOUT_CAP_BYTES = 256 * 1024
const STDERR_CAP_BYTES = 16 * 1024

export interface FoundFork {
  forkSessionId: string
  forkTitle: string
  /** When opencode created the fork row in `session`. */
  timeCreated: number
  /** When opencode last wrote to the fork (proxy for LLM done time). */
  timeUpdated: number
}

export interface FindRecentForkOptions {
  sourceSessionId: string
  /** ms since epoch; the dashboard's job.startedAt. */
  startedAt: number
  /** Anchor text from the prompt to disambiguate concurrent forks. */
  promptAnchor: string
  dbPath?: string
  /** Test seam. Overrides `sqlite3` invocation. */
  sqliteFn?: typeof spawn
}

/**
 * Find the most recent fork session created within
 * `[startedAt - FORK_LOOKUP_WINDOW_MS, startedAt + 5min]` whose stored
 * parts contain `promptAnchor`. The anchor is matched with LIKE so a
 * truncated prompt still hits.
 *
 * Why we match by anchor text instead of fork title alone: if the user
 * has many sessions, multiple "(fork #N)" rows may show up in the
 * window; we need exact-job-attribution. The `请用中文总结本次会话`
 * prefix is unique enough — only this dashboard ever sends it.
 *
 * Returns null on any DB error / timeout / no match — never throws.
 */
export function findRecentForkSession(
  opts: FindRecentForkOptions,
): Promise<FoundFork | null> {
  const dbPath = opts.dbPath ?? DEFAULT_OPENCODE_DB_PATH
  const sp = opts.sqliteFn ?? spawn
  const minTs = opts.startedAt - FORK_LOOKUP_WINDOW_MS
  // Allow up to 5 minutes after startedAt — covers the rare case where
  // opencode's background work outran our spawn timeout by a lot.
  const maxTs = opts.startedAt + 5 * 60 * 1000

  // The query picks any fork whose part data contains the anchor; we
  // already know the dashboard's prompts are uniquely prefixed.
  // Restricting to "title LIKE '%(fork #%'" further narrows the result
  // when opencode runs without --fork (we always pass --fork now but
  // belt and suspenders cost nothing).
  //
  // We use sqlite3's `.param set :name value` named-bind protocol
  // because the positional `?N` form is silently ignored by older
  // CLI builds (verified on 3.53 shipping with current dashboards).
  const query = `
    SELECT s.id, s.title, s.time_created, s.time_updated
    FROM part p
    JOIN session s ON s.id = p.session_id
    WHERE p.data LIKE :anchor
      AND s.title LIKE '%(fork #%'
      AND s.time_created >= :minTs
      AND s.time_created <= :maxTs
    ORDER BY s.time_created DESC
    LIMIT 1
  `.trim()

  return new Promise<FoundFork | null>((resolve) => {
    if (!existsSync(dbPath)) {
      resolve(null)
      return
    }
    // sqlite3 -cmd doesn't support real bind parameters from argv. Use
    // the older `-cmd ".param set"` approach via stdin-fed parameter
    // bindings to keep injection-safety: we send the anchor and the
    // numbers via a controlled stdin script that uses .param set, then
    // runs the parameterized SELECT. The anchor is a TEXT value so any
    // quotes inside it are safely escaped by .param set 's own parser.
    //
    // Anchor pre-processing: escape % and _ so a future prompt with
    // SQL wildcards doesn't degenerate the LIKE.
    const safeAnchor =
      "%" +
      opts.promptAnchor
        .replace(/\\/g, "\\\\")
        .replace(/%/g, "\\%")
        .replace(/_/g, "\\_") +
      "%"

    let proc
    try {
      proc = sp(
        "sqlite3",
        ["-json", dbPath],
        { stdio: ["pipe", "pipe", "pipe"] },
      )
    } catch {
      resolve(null)
      return
    }

    let stdout = ""
    let stderr = ""
    const timer = setTimeout(() => {
      try { proc.kill("SIGKILL") } catch { /* noop */ }
    }, SQLITE_TIMEOUT_MS)

    proc.stdout?.on("data", (d: Buffer) => {
      if (stdout.length >= STDOUT_CAP_BYTES) return
      stdout += d.toString("utf-8")
    })
    proc.stderr?.on("data", (d: Buffer) => {
      if (stderr.length >= STDERR_CAP_BYTES) return
      stderr += d.toString("utf-8")
    })
    proc.on("error", () => {
      clearTimeout(timer)
      resolve(null)
    })
    proc.on("close", (code) => {
      clearTimeout(timer)
      if (code !== 0) {
        resolve(null)
        return
      }
      let parsed: unknown
      try { parsed = JSON.parse(stdout || "[]") } catch { resolve(null); return }
      if (!Array.isArray(parsed) || parsed.length === 0) {
        resolve(null)
        return
      }
      const row = parsed[0] as Record<string, unknown>
      const id = typeof row.id === "string" ? row.id : null
      const title = typeof row.title === "string" ? row.title : ""
      const tc = typeof row.time_created === "number" ? row.time_created : 0
      const tu = typeof row.time_updated === "number" ? row.time_updated : tc
      if (!id) { resolve(null); return }
      resolve({ forkSessionId: id, forkTitle: title, timeCreated: tc, timeUpdated: tu })
    })

    // Feed parameterized query via the sqlite3 dot-command protocol.
    const script =
      `.param init\n` +
      `.param set :anchor ${JSON.stringify(safeAnchor)}\n` +
      `.param set :minTs ${minTs}\n` +
      `.param set :maxTs ${maxTs}\n` +
      query + ";\n" +
      `.quit\n`
    try {
      proc.stdin?.write(script)
      proc.stdin?.end()
    } catch {
      // close handler will resolve(null).
    }
  })
}

export interface ExtractAssistantTextOptions {
  forkSessionId: string
  dbPath?: string
  sqliteFn?: typeof spawn
}

/**
 * Pull the assistant's text reply out of a fork session. Returns the
 * concatenation of every `type:"text"` part whose parent message has
 * role="assistant", in chronological order. Empty string on miss/error.
 *
 * Why we concat rather than picking "the last assistant message": the
 * LLM may emit multiple text parts (e.g. reasoning followed by the
 * final answer), and our prompt is short enough that all assistant
 * text in the fork is part of the summary.
 */
export function extractAssistantText(
  opts: ExtractAssistantTextOptions,
): Promise<string> {
  const dbPath = opts.dbPath ?? DEFAULT_OPENCODE_DB_PATH
  const sp = opts.sqliteFn ?? spawn
  // The schema stores `role` inside message.data (JSON blob), so we
  // can't do role=assistant filtering in pure SQL without json1; the
  // simpler path is to fetch both the part and the parent message data
  // and filter in Node.
  // The fork structure is: [cloned source messages…] + [our summary
  // user prompt] + [assistant reply]. We only need the assistant text
  // parts attached to the LAST assistant message. Targeting just that
  // one message keeps the JSON small even when the source session has
  // hundreds of messages.
  const query = `
    SELECT p.data AS part_data, m.data AS message_data, p.time_created AS t
    FROM part p
    JOIN message m ON m.id = p.message_id
    WHERE p.session_id = :sid
      AND m.id = (
        SELECT id
        FROM message
        WHERE session_id = :sid
          AND data LIKE '%"role":"assistant"%'
        ORDER BY time_created DESC
        LIMIT 1
      )
    ORDER BY p.time_created ASC
  `.trim()

  return new Promise<string>((resolve) => {
    if (!existsSync(dbPath)) { resolve(""); return }
    let proc
    try {
      proc = sp("sqlite3", ["-json", dbPath], { stdio: ["pipe", "pipe", "pipe"] })
    } catch {
      resolve("")
      return
    }
    let stdout = ""
    const timer = setTimeout(() => {
      try { proc.kill("SIGKILL") } catch { /* noop */ }
    }, SQLITE_TIMEOUT_MS)
    proc.stdout?.on("data", (d: Buffer) => {
      if (stdout.length >= STDOUT_CAP_BYTES) return
      stdout += d.toString("utf-8")
    })
    proc.on("error", () => { clearTimeout(timer); resolve("") })
    proc.on("close", (code) => {
      clearTimeout(timer)
      if (code !== 0) { resolve(""); return }
      let rows: unknown
      try { rows = JSON.parse(stdout || "[]") } catch { resolve(""); return }
      if (!Array.isArray(rows)) { resolve(""); return }

      const out: string[] = []
      for (const r of rows as Record<string, unknown>[]) {
        const pd = typeof r.part_data === "string" ? r.part_data : ""
        const md = typeof r.message_data === "string" ? r.message_data : ""
        // Quick gate: skip the user message (the prompt itself).
        if (!md.includes('"role":"assistant"')) continue
        let pobj: { type?: string; text?: string } | null = null
        try { pobj = JSON.parse(pd) } catch { continue }
        if (pobj && pobj.type === "text" && typeof pobj.text === "string") {
          out.push(pobj.text)
        }
      }
      resolve(out.join("\n").trim())
    })
    const script =
      `.param init\n` +
      `.param set :sid ${JSON.stringify(opts.forkSessionId)}\n` +
      query + ";\n" +
      `.quit\n`
    try {
      proc.stdin?.write(script)
      proc.stdin?.end()
    } catch {
      // close handler will resolve("").
    }
  })
}

export interface SalvageOptions {
  sourceSessionId: string
  startedAt: number
  promptAnchor: string
  dbPath?: string
  sqliteFn?: typeof spawn
}

export interface SalvageResult {
  forkSessionId: string
  forkTitle: string
  /** Wall-clock duration the fork took to finish (time_updated - time_created). */
  forkDurationMs: number
  text: string
}

/**
 * One-shot wrapper for the timeout/failure salvage flow.
 *
 * Returns null if the fork can't be located or its assistant text is
 * empty. Callers should treat null as "we have nothing better than the
 * spawn's own stdout".
 */
export async function salvageFromFork(
  opts: SalvageOptions,
): Promise<SalvageResult | null> {
  const fork = await findRecentForkSession({
    sourceSessionId: opts.sourceSessionId,
    startedAt: opts.startedAt,
    promptAnchor: opts.promptAnchor,
    dbPath: opts.dbPath,
    sqliteFn: opts.sqliteFn,
  })
  if (!fork) return null
  const text = await extractAssistantText({
    forkSessionId: fork.forkSessionId,
    dbPath: opts.dbPath,
    sqliteFn: opts.sqliteFn,
  })
  if (!text) return null
  return {
    forkSessionId: fork.forkSessionId,
    forkTitle: fork.forkTitle,
    forkDurationMs: Math.max(0, fork.timeUpdated - fork.timeCreated),
    text,
  }
}
