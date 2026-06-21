/**
 * Pure helpers for the terminal page's WebSocket URL and the auto-injection
 * gating semantics. Importable on machines without a working PTY toolchain
 * (no native binding imports, no side effects), so `tests/terminalUrl.test.ts`
 * can run anywhere.
 *
 * The `inject=1` query flag is the explicit signal that this session was just
 * created for a requirement and the requirement's context should be auto-fed
 * into the TUI once. Existing sessions that are merely opened with `?req=`
 * must NOT trigger auto-injection.
 */

/**
 * Returns true only when the search params contain literal `inject=1`.
 * Accepts a raw URL string, a `URL` instance, a `URLSearchParams`, or null/undefined.
 * Any other value of `inject` (e.g. `true`, `0`, missing) yields false.
 */
export function shouldAutoInjectRequirementContext(
  raw: string | URL | URLSearchParams | null | undefined,
): boolean {
  if (raw === null || raw === undefined) return false
  let params: URLSearchParams
  if (raw instanceof URLSearchParams) {
    params = raw
  } else if (raw instanceof URL) {
    params = raw.searchParams
  } else {
    const s = String(raw)
    try {
      // Accept full URLs and bare query strings alike.
      if (s.includes("://")) {
        params = new URL(s).searchParams
      } else {
        params = new URLSearchParams(s.startsWith("?") ? s.slice(1) : s)
      }
    } catch {
      return false
    }
  }
  return params.get("inject") === "1"
}

export interface BuildTerminalWebSocketUrlOptions {
  protocol: string
  host: string
  sessionId: string
  reqId?: string
  autoInject?: boolean
}

/**
 * Mirrors the inline construction in `public/terminal.js`. Kept here as a
 * contract so the gating semantics can be unit-tested without a browser.
 */
export function buildTerminalWebSocketUrl(options: BuildTerminalWebSocketUrlOptions): string {
  const proto = options.protocol === "https:" ? "wss" : "ws"
  let url = `${proto}://${options.host}/ws/session-terminal?id=${encodeURIComponent(options.sessionId)}`
  if (options.reqId) url += `&req=${encodeURIComponent(options.reqId)}`
  if (options.autoInject) url += "&inject=1"
  return url
}
