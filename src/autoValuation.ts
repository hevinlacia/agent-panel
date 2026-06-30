/**
 * Background worker for auto-discovering valuable sessions.
 *
 * Role: poll recent OpenCode sessions, score them for experience-value
 * potential using `src/sessionValuation.ts`, and auto-mark sessions
 * whose score exceeds the configured threshold. Auto-marking feeds
 * directly into the existing experience-summary pipeline
 * (`src/experienceAutoSummary.ts`), so the user's review/confirm
 * workflow is unchanged — the worker just pre-fills the candidate list.
 *
 * Design:
 *   - Poll every 10 minutes (configurable via POLL_INTERVAL_MS).
 *   - Scan sessions updated within the last 48 h.
 *   - Skip fork sessions (parentId set) and sessions already marked.
 *   - Cache valuation results for 24 h to avoid re-scoring.
 *   - When a session scores ≥ threshold, call markSession() with an
 *     auto-generated note and create a notification.
 *
 * Public surface:
 *   - startAutoValuationWorker(): start the periodic poll loop
 *   - stopAutoValuationWorker(): stop the poll loop
 *   - isAutoValuationWorkerRunning(): check if active
 *   - pollOnce(): run one poll cycle (exported for testing)
 *   - getValuationStats(): return last-cycle stats for the schedulers UI
 *   - getRecentCandidates(): return cached candidate list for the UI
 *
 * Constraints / safety:
 *   - Only `node:` built-ins + sibling modules.
 *   - Timer is `unref()`-ed so it doesn't keep the process alive.
 *   - Never reads or writes `.env` / secret files.
 *
 * Read-this-with:
 *   - `src/sessionValuation.ts` (the scorer).
 *   - `src/experienceMarkers.ts` (markSession — auto-marking target).
 *   - `src/experienceAutoSummary.ts` (the downstream summary worker).
 *   - `src/sessions.ts` (scanSessions for session discovery).
 *   - `src/notifications.ts` (createNotification for user alerting).
 */

import { scanSessions, type SessionInfo } from "./sessions.ts"
import { scoreSession, type ValuationResult } from "./sessionValuation.ts"
import { markSession, getMarker } from "./experienceMarkers.ts"
import { createNotification } from "./notifications.ts"
import { getConfig } from "./config.ts"

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

export const POLL_INTERVAL_MS = 10 * 60 * 1000 // 10 minutes
const SCAN_WINDOW_MS = 48 * 60 * 60 * 1000     // 48 hours
const CACHE_TTL_MS = 24 * 60 * 60 * 1000       // 24 hours

// ---------------------------------------------------------------------------
// Cache: session id → valuation result + timestamp
// ---------------------------------------------------------------------------

interface CachedValuation {
  result: ValuationResult
  cachedAt: number
}

const _cache = new Map<string, CachedValuation>()

function evictExpiredCache(now: number): void {
  for (const [sid, entry] of _cache) {
    if (now - entry.cachedAt > CACHE_TTL_MS) {
      _cache.delete(sid)
    }
  }
}

// ---------------------------------------------------------------------------
// Stats (for schedulers page UI)
// ---------------------------------------------------------------------------

export interface ValuationStats {
  lastPollAt: number | null
  sessionsScanned: number
  contentScored: number
  candidatesFound: number
  autoMarked: number
  alreadyMarked: number
  threshold: number
}

let _stats: ValuationStats = {
  lastPollAt: null,
  sessionsScanned: 0,
  contentScored: 0,
  candidatesFound: 0,
  autoMarked: 0,
  alreadyMarked: 0,
  threshold: 0,
}

/** Return stats from the last poll cycle for the schedulers page. */
export function getValuationStats(): ValuationStats {
  return { ..._stats }
}

/** Return cached valuation candidates (score ≥ threshold), newest first. */
export function getRecentCandidates(limit = 20): ValuationResult[] {
  const now = Date.now()
  const candidates: ValuationResult[] = []
  for (const entry of _cache.values()) {
    if (now - entry.cachedAt > CACHE_TTL_MS) continue
    if (entry.result.score >= (_stats.threshold || 25)) {
      candidates.push(entry.result)
    }
  }
  candidates.sort((a, b) => b.score - a.score)
  return candidates.slice(0, limit)
}

// ---------------------------------------------------------------------------
// Test seam: allow injecting a custom scorer for unit tests
// ---------------------------------------------------------------------------

let _scoreFn: typeof scoreSession = scoreSession

/** Override the scoring function (test-only). */
export function _setScoreFn(fn: typeof scoreSession): void {
  _scoreFn = fn
}

/** Reset test overrides. */
export function _resetTestState(): void {
  _scoreFn = scoreSession
  _cache.clear()
  _stats = {
    lastPollAt: null,
    sessionsScanned: 0,
    contentScored: 0,
    candidatesFound: 0,
    autoMarked: 0,
    alreadyMarked: 0,
    threshold: 0,
  }
}

// ---------------------------------------------------------------------------
// Poll cycle
// ---------------------------------------------------------------------------

/**
 * Run one valuation poll cycle:
 *   1. Evict expired cache entries.
 *   2. Scan sessions updated within the last 48 h.
 *   3. For each session not already cached/marked, run the two-tier scorer.
 *   4. Auto-mark sessions whose score ≥ threshold.
 *   5. Update stats.
 *
 * Exported for testing.
 */
export async function pollOnce(): Promise<void> {
  const now = Date.now()
  evictExpiredCache(now)

  const cfg = await getConfig()
  const enabled = cfg.autoValuation ?? false
  const threshold = cfg.valuationThreshold ?? 25

  // Always scan and score (even when disabled) so the schedulers page
  // can show candidate counts. Auto-marking is gated by `enabled`.
  const sessions = await scanSessions(false, SCAN_WINDOW_MS)

  let scanned = 0
  let contentScored = 0
  let candidatesFound = 0
  let autoMarked = 0
  let alreadyMarked = 0

  for (const session of sessions) {
    // Skip fork sessions.
    if (session.parentId) continue

    // Skip sessions already in the marker store (any status).
    const existingMarker = getMarker(session.id)
    if (existingMarker) {
      alreadyMarked++
      continue
    }

    // Skip sessions already cached (within 24 h).
    const cached = _cache.get(session.id)
    if (cached && now - cached.cachedAt < CACHE_TTL_MS) {
      if (cached.result.score >= threshold) candidatesFound++
      continue
    }

    scanned++

    // Score the session.
    const result = await _scoreFn(session)
    _cache.set(session.id, { result, cachedAt: now })
    if (result.contentScored) contentScored++

    if (result.score >= threshold) {
      candidatesFound++

      // Auto-mark if enabled and session is idle (not currently running).
      if (enabled && session.status !== "running") {
        try {
          const note = `auto: score=${result.score} signals=[${result.signals.join(",")}]`
          await markSession(session.id, { note })
          autoMarked++

          // Notify the user.
          createNotification({
            type: "system",
            title: "自动发现高价值 session",
            subtitle: `${session.title.slice(0, 60)} (score: ${result.score})`,
            state: "done",
            sessionId: session.id,
            actionHref: `/session?id=${encodeURIComponent(session.id)}`,
          })
        } catch {
          // markSession can throw on invalid session id; skip silently.
        }
      }
    }
  }

  _stats = {
    lastPollAt: now,
    sessionsScanned: scanned,
    contentScored,
    candidatesFound,
    autoMarked,
    alreadyMarked,
    threshold,
  }
}

// ---------------------------------------------------------------------------
// Background worker
// ---------------------------------------------------------------------------

let _timer: ReturnType<typeof setInterval> | null = null

/**
 * Start the background worker. Polls every `POLL_INTERVAL_MS`.
 * Safe to call multiple times — if already running, does nothing.
 */
export function startAutoValuationWorker(): void {
  if (_timer) return
  _timer = setInterval(() => {
    void pollOnce().catch(() => {})
  }, POLL_INTERVAL_MS)
  if (typeof _timer.unref === "function") _timer.unref()
  // Run one poll after a short delay (let other workers initialize first).
  setTimeout(() => {
    void pollOnce().catch(() => {})
  }, 30_000).unref?.()
}

/** Stop the background worker. */
export function stopAutoValuationWorker(): void {
  if (_timer) {
    clearInterval(_timer)
    _timer = null
  }
}

/** Whether the worker interval is currently active. */
export function isAutoValuationWorkerRunning(): boolean {
  return _timer !== null
}
