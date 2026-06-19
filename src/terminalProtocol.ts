/**
 * Pure parser for the WebSocket terminal message contract.
 *
 * This module is side-effect-free and must NOT import `node-pty` (or any
 * other native binding) so it can be unit-tested on any platform without
 * a working PTY toolchain.
 *
 * Inbound WS messages:
 *   - raw string  -> { kind: "input", data }   (written to PTY stdin)
 *   - JSON {type:"resize", cols, rows} -> { kind: "resize", cols, rows }
 *   - JSON {type:"ping"}              -> { kind: "ignore" }
 *   - JSON object with any other `type` (including missing/null/undefined)
 *     -> treated as raw input, the JSON string is forwarded verbatim
 *   - non-JSON text starting with `{` that fails to parse -> raw input
 *
 * The function only inspects the leading `{` byte to decide whether to
 * attempt JSON parsing. Anything that doesn't look like a JSON object is
 * passed through as input.
 */

export type TerminalClientMessage =
  | { kind: "input"; data: string }
  | { kind: "resize"; cols: number; rows: number }
  | { kind: "ignore" }

/** Parse a WS text frame into either "input" or a typed control message. */
export function parseClientMessage(raw: string): TerminalClientMessage {
  if (!raw) return { kind: "ignore" }
  if (raw.charCodeAt(0) === 0x7b /* "{" */) {
    try {
      const obj = JSON.parse(raw) as { type?: unknown; cols?: unknown; rows?: unknown }
      if (obj && typeof obj === "object" && obj.type === "resize") {
        const cols = typeof obj.cols === "number" ? obj.cols : Number(obj.cols)
        const rows = typeof obj.rows === "number" ? obj.rows : Number(obj.rows)
        if (Number.isFinite(cols) && Number.isFinite(rows)) {
          return { kind: "resize", cols, rows }
        }
        return { kind: "ignore" }
      }
      if (obj && typeof obj === "object" && obj.type === "ping") {
        return { kind: "ignore" }
      }
    } catch {
      // fall through to treat as raw input
    }
  }
  return { kind: "input", data: raw }
}
