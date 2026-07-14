/**
 * Hot-deploy front proxy for agent-panel.
 *
 * Role: a lightweight, never-stopping HTTP + WebSocket reverse proxy that
 * listens on the public port (7331) and forwards to the active backend slot
 * (blue=7332 / green=7333), so backends can be rotated without dropping users.
 *
 * Public surface: none (run via `bun run proxy`); reads active-backend.json.
 * Constraints: no new dependencies (node:http + ws already vendored via
 * @fastify/websocket); reads the active slot on every request so a deploy's
 * active-slot switch takes effect immediately for new connections.
 * Read-this-with: systemd/agent-panel-proxy.service, bin/hot-deploy-agent-panel.sh.
 */

import http from "node:http"
import { existsSync, mkdirSync, readFileSync, renameSync, writeFileSync } from "node:fs"
import { homedir } from "node:os"
import { dirname, join } from "node:path"
import { WebSocket, WebSocketServer } from "ws"

const PROXY_PORT = parseInt(process.env.AGENT_PANEL_PROXY_PORT || "7331", 10)
const BACKENDS: Record<string, string> = {
  blue: process.env.AGENT_PANEL_BLUE_URL || "http://127.0.0.1:7332",
  green: process.env.AGENT_PANEL_GREEN_URL || "http://127.0.0.1:7333",
}
const DEFAULT_ACTIVE_FILE = join(homedir(), ".local", "state", "agent-panel", "active-backend.json")
const ACTIVE_FILE = process.env.AGENT_PANEL_ACTIVE_BACKEND_FILE || DEFAULT_ACTIVE_FILE

function readActiveSlot(): string {
  const fallback = process.env.AGENT_PANEL_DEFAULT_SLOT || "blue"
  if (!existsSync(ACTIVE_FILE)) return fallback
  try {
    const data = JSON.parse(readFileSync(ACTIVE_FILE, "utf-8"))
    const slot = data?.slot
    return typeof slot === "string" && slot in BACKENDS ? slot : fallback
  } catch {
    return fallback
  }
}

function writeActiveSlot(slot: string): boolean {
  if (!(slot in BACKENDS)) return false
  mkdirSync(dirname(ACTIVE_FILE), { recursive: true })
  const payload = {
    slot,
    base_url: BACKENDS[slot],
    updated_at: Math.floor(Date.now() / 1000),
  }
  const tmp = `${ACTIVE_FILE}.${process.pid}.tmp`
  writeFileSync(tmp, JSON.stringify(payload, null, 2) + "\n")
  renameSync(tmp, ACTIVE_FILE)
  return true
}

function backendUrl(slot: string): URL {
  return new URL(BACKENDS[slot])
}

const server = http.createServer((req, res) => {
  const url = req.url || "/"

  // --- proxy control plane ---
  if (url === "/_proxy/health") {
    res.writeHead(200, { "content-type": "application/json" })
    res.end(JSON.stringify({
      ok: true,
      active_slot: readActiveSlot(),
      backends: Object.keys(BACKENDS),
      active_backend_file: ACTIVE_FILE,
    }))
    return
  }

  if (req.method === "POST" && url.startsWith("/_proxy/active/")) {
    const slot = url.slice("/_proxy/active/".length).split("?")[0]
    const ok = writeActiveSlot(slot)
    res.writeHead(ok ? 200 : 400, { "content-type": "application/json" })
    res.end(JSON.stringify({ ok, active_slot: ok ? slot : readActiveSlot() }))
    return
  }

  // --- forward HTTP to the active backend ---
  // No failover once the body starts streaming: the active slot is always the
  // healthy one (hot-deploy health-checks before switching). On connect error,
  // return 502 so the caller knows to retry.
  const slot = readActiveSlot()
  const target = backendUrl(slot)
  const proxyReq = http.request(
    {
      hostname: target.hostname,
      port: Number(target.port),
      path: url,
      method: req.method,
      headers: { ...req.headers, host: target.host },
    },
    (proxyRes) => {
      res.writeHead(proxyRes.statusCode ?? 502, proxyRes.headers)
      proxyRes.pipe(res)
    },
  )
  proxyReq.on("error", () => {
    if (!res.headersSent) {
      res.writeHead(502, { "content-type": "application/json" })
      res.end(JSON.stringify({ error: `agent-panel backend '${slot}' unavailable` }))
    } else {
      res.destroy()
    }
  })
  req.pipe(proxyReq)
})

// --- WebSocket forwarding (per-connection, to the active slot at upgrade time) ---
const wss = new WebSocketServer({ noServer: true })

server.on("upgrade", (req, socket, head) => {
  const url = req.url || "/"
  if (url.startsWith("/_proxy/")) {
    socket.destroy()
    return
  }
  const slot = readActiveSlot()
  const target = backendUrl(slot)
  wss.handleUpgrade(req, socket, head, (ws) => {
    const upstreamUrl = `ws://${target.host}${url}`
    const upstream = new WebSocket(upstreamUrl)
    // Pipe both directions, preserving binary frames (terminal PTY bytes).
    upstream.on("message", (data, isBinary) => ws.send(data, { binary: isBinary }))
    ws.on("message", (data, isBinary) => upstream.send(data, { binary: isBinary }))
    upstream.on("close", (code, reason) => {
      try { ws.close(code, reason) } catch { /* noop */ }
    })
    ws.on("close", () => {
      try { upstream.close() } catch { /* noop */ }
    })
    upstream.on("error", () => {
      try { ws.close(1011, "upstream error") } catch { /* noop */ }
    })
    ws.on("error", () => {
      try { upstream.close() } catch { /* noop */ }
    })
  })
})

server.listen(PROXY_PORT, "0.0.0.0", () => {
  console.log(`Agent Panel proxy running at http://localhost:${PROXY_PORT} -> ${readActiveSlot()}`)
})
