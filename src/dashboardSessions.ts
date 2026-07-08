/**
 * Harness-aware session facade for dashboard routes.
 *
 * Role: route session reads, cache clearing, and id validation to either
 * the existing OpenCode scanner or the Pi JSONL scanner.
 * Public surface: DashboardHarness plus scan/get/validate helpers used by
 * `src/server.tsx` and terminal routes.
 * Constraints / safety: keeps OpenCode's `ses_...` validation unchanged and
 * adds a separate strict UUID validator for Pi sessions.
 * Read-this-with: `src/sessions.ts`, `src/piSessions.ts`, `src/terminal.ts`.
 */

import {
  scanSessions,
  getSession,
  getSessionsByIds,
  summarizeSessions,
  clearSessionCache,
  isValidSessionId,
  type SessionInfo,
} from "./sessions.ts"
import {
  scanPiSessions,
  getPiSession,
  getPiSessionsByIds,
  clearPiSessionCache,
  isValidPiSessionId,
} from "./piSessions.ts"
import type { DashboardHarness } from "./config.ts"

export type { DashboardHarness }
export { summarizeSessions }

export function normalizeHarness(value: unknown): DashboardHarness {
  return value === "pi" ? "pi" : "opencode"
}

export function harnessLabel(harness: DashboardHarness): string {
  return harness === "pi" ? "Pi" : "OpenCode"
}

export async function scanDashboardSessions(harness: DashboardHarness, force = false, maxAgeMs?: number): Promise<SessionInfo[]> {
  return harness === "pi" ? scanPiSessions(force, maxAgeMs) : scanSessions(force, maxAgeMs)
}

export async function getDashboardSession(harness: DashboardHarness, id: string): Promise<SessionInfo | null> {
  return harness === "pi" ? getPiSession(id) : getSession(id)
}

export async function getDashboardSessionsByIds(harness: DashboardHarness, ids: string[]): Promise<SessionInfo[]> {
  return harness === "pi" ? getPiSessionsByIds(ids) : getSessionsByIds(ids)
}

export function clearDashboardSessionCache(harness: DashboardHarness): void {
  if (harness === "pi") clearPiSessionCache()
  else clearSessionCache()
}

export function isValidDashboardSessionId(harness: DashboardHarness, id: string | null | undefined): id is string {
  return harness === "pi" ? isValidPiSessionId(id) : isValidSessionId(id)
}

export function extractDashboardSessionId(harness: DashboardHarness, raw: string): string {
  const value = raw.trim()
  if (harness === "pi") {
    const match = value.match(/[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}/i)
    return match ? match[0] : value
  }
  const match = value.match(/ses_[A-Za-z0-9]+/)
  return match ? match[0] : value
}

export function buildResumeCommand(harness: DashboardHarness, sessionId: string): string {
  return harness === "pi" ? `pi --session ${sessionId}` : `opencode -s ${sessionId}`
}
