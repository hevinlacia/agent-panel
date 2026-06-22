/**
 * Spawn a local `opencode --session <id>` TUI in a node-pty pseudo-terminal
 * and pipe bytes to/from a WebSocket. The WS layer is owned by Hono; this
 * module only owns the PTY and the message contract.
 *
 * Inbound WS messages:
 *   - raw string  -> written directly to PTY stdin
 *   - JSON {type:"resize", cols, rows} -> pty.resize(cols, rows)
 *   - JSON {type:"ping"}              -> ignored (keepalive convenience)
 *
 * Outbound WS messages:
 *   - raw string: PTY stdout bytes
 *   - JSON: {type:"exit", code, signal} when the child exits
 *   - JSON: {type:"error", message}   when spawning fails
 */

import { existsSync } from "node:fs"
import { spawn as nodePtySpawn, type IPty } from "node-pty"

import { isValidSessionId, resolveCwd } from "./sessions.ts"
import { parseClientMessage } from "./terminalProtocol.ts"

export type { TerminalClientMessage } from "./terminalProtocol.ts"
export { parseClientMessage } from "./terminalProtocol.ts"

export type TerminalOutMessage =
  | { type: "exit"; code: number; signal?: number }
  | { type: "error"; message: string }
  | string // raw pty output

export type TerminalHandler = {
  onOutput: (chunk: string) => void
  onExit: (code: number, signal?: number) => void
  onError: (message: string) => void
}

export type TerminalSession = {
  id: string
  pty: IPty
  cols: number
  rows: number
  cwd: string
}

export type StartSessionOptions = {
  createNew?: boolean
  title?: string
}

const OPENCODE_BIN_CANDIDATES = ["/usr/bin/opencode", "opencode"]
const DEFAULT_COLS = 100
const DEFAULT_ROWS = 28

function pickOpencodeBin(): string {
  for (const candidate of OPENCODE_BIN_CANDIDATES) {
    if (candidate.startsWith("/")) {
      if (existsSync(candidate)) return candidate
    } else {
      // Rely on PATH; if it does not exist, spawn will fail and we will report.
      return candidate
    }
  }
  return "opencode"
}

export function startSession(
  id: string,
  directory: string | null | undefined,
  handler: TerminalHandler,
  options: StartSessionOptions = {}
): TerminalSession | { error: string } {
  const createNew = options.createNew === true
  if (!createNew && !isValidSessionId(id)) {
    return { error: "Invalid session id" }
  }
  if (createNew && id && !isValidSessionId(id)) {
    return { error: "Invalid session id" }
  }
  const bin = pickOpencodeBin()
  const args = createNew ? ["run", "-i"] : ["--session", id]
  if (createNew && options.title) {
    args.push("--title", options.title)
  }
  const cwd = resolveCwd(directory)

  let pty: IPty
  try {
    pty = nodePtySpawn(bin, args, {
      name: "xterm-256color",
      cols: DEFAULT_COLS,
      rows: DEFAULT_ROWS,
      cwd,
      env: { ...process.env, TERM: "xterm-256color" } as Record<string, string>,
    })
  } catch (err) {
    const message = err instanceof Error ? err.message : String(err)
    return { error: `Failed to spawn ${bin}: ${message}` }
  }

  const session: TerminalSession = { id, pty, cols: DEFAULT_COLS, rows: DEFAULT_ROWS, cwd }

  pty.onData((data) => {
    try {
      handler.onOutput(data)
    } catch {
      // Swallow handler errors so the PTY is not killed by an observer bug.
    }
  })

  pty.onExit(({ exitCode, signal }) => {
    try {
      handler.onExit(exitCode, signal)
    } catch {
      // ignore
    }
  })

  return session
}

export function writeToSession(session: TerminalSession, data: string): void {
  if (!data) return
  try {
    session.pty.write(data)
  } catch {
    // PTY already dead; ignore.
  }
}

export function resizeSession(session: TerminalSession, cols: number, rows: number): void {
  if (!Number.isFinite(cols) || !Number.isFinite(rows)) return
  const c = Math.max(2, Math.min(500, Math.floor(cols)))
  const r = Math.max(2, Math.min(200, Math.floor(rows)))
  if (c === session.cols && r === session.rows) return
  session.cols = c
  session.rows = r
  try {
    session.pty.resize(c, r)
  } catch {
    // ignore
  }
}

export function killSession(session: TerminalSession): void {
  try {
    session.pty.kill()
  } catch {
    // ignore
  }
}
