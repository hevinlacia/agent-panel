/** @jsxImportSource hono/jsx */
import { Hono } from "hono"
import { type FC } from "hono/jsx"
import { serve } from "@hono/node-server"
import { upgradeWebSocket } from "@hono/node-server"
import { WebSocketServer } from "ws"
import { readFile } from "node:fs/promises"
import { existsSync } from "node:fs"
import { join, dirname, resolve } from "node:path"
import { fileURLToPath } from "node:url"
import { scanReports, getReport, saveConfirmation, type Confirmation } from "./scanner.ts"
import type { Candidate, ParsedReport } from "./parser.ts"
import {
  scanSessions,
  getSession,
  summarizeSessions,
  type SessionInfo,
} from "./sessions.ts"
import {
  startSession,
  writeToSession,
  resizeSession,
  killSession,
  parseClientMessage,
  type TerminalSession,
} from "./terminal.ts"
import { resolveHandoffPath } from "./paths.ts"

const __filename = fileURLToPath(import.meta.url)
const __dirname = dirname(__filename)
const PROJECT_ROOT = resolve(__dirname, "..")
const PUBLIC_DIR = join(PROJECT_ROOT, "public")
const NODE_MODULES_DIR = join(PROJECT_ROOT, "node_modules")

// ---------------------------------------------------------------------------
// Layout
// ---------------------------------------------------------------------------

type Tab = "sessions" | "reports"

/**
 * Operator-style topbar: thin console header with a logo block, optional
 * status badge, and a route strip below it. Both Sessions and Reports nav
 * still work — the route strip keeps them visible in the new style.
 */
const Layout: FC<{ title: string; active: Tab; children: any }> = ({ title, active, children }) => (
  <html lang="zh-CN">
    <head>
      <meta charset="utf-8" />
      <meta name="viewport" content="width=device-width, initial-scale=1" />
      <title>{title} — OpenCode Dashboard</title>
      <link rel="stylesheet" href="/static/style.css" />
    </head>
    <body>
      <header class="op-topbar">
        <div class="op-topbar-row">
          <div class="op-brand">
            <span class="op-brand-name">OpenCode Operator</span>
            <span class="op-brand-sep">|</span>
            <span class="op-brand-status">SNAPSHOT READY</span>
          </div>
          <div class="op-meta">
            <span class="op-meta-item">SYSTEM</span>
            <span class="op-meta-item">LIGHT</span>
            <span class="op-meta-item">DARK</span>
            <a class="op-meta-item op-refresh" href="/sessions/refresh" title="Force refresh">REFRESH</a>
          </div>
        </div>
        <div class="op-topbar-row op-topbar-routes">
          <nav class="op-routes">
            <a href="/" class={active === "sessions" ? "op-route op-route-active" : "op-route"}>/sessions</a>
            <a href="/reports" class={active === "reports" ? "op-route op-route-active" : "op-route"}>/reports</a>
            <a href="/api/sessions" class="op-route">/api/sessions</a>
            <a href="/api/reports" class="op-route">/api/reports</a>
          </nav>
          <span class="op-embedded">embedded web terminal · {title}</span>
        </div>
      </header>
      <main class={active === "sessions" ? "op-main op-main-sessions" : "op-main"}>{children}</main>
      <script src="/static/app.js" defer></script>
    </body>
  </html>
)

// ---------------------------------------------------------------------------
// Sessions dashboard
// ---------------------------------------------------------------------------

const StatusDot: FC<{ status: string }> = ({ status }) => (
  <span class={`status-dot status-${status}`} title={status} aria-hidden="true" />
)

const statusLabel = (status: SessionInfo["status"]): string => {
  if (status === "running") return "RUNNING"
  if (status === "idle") return "IDLE"
  return "STALE"
}

const shortSessionId = (id: string): string => {
  // "ses_12512136bffeLb0e0B1Z8epxiX" -> "1251 2136 BFFE LB0E"
  const core = id.startsWith("ses_") ? id.slice(4) : id
  if (core.length <= 16) return core.toUpperCase()
  return (core.slice(0, 4) + " " + core.slice(4, 8) + " " + core.slice(8, 12) + " " + core.slice(12, 16)).toUpperCase()
}

const formatUpdated = (ms: number): string => {
  if (!ms) return "—"
  const d = new Date(ms)
  if (!isFinite(d.getTime())) return "—"
  // Operator-style: 2026-06-18 16:32 UTC
  return d.toISOString().replace("T", " ").slice(0, 16) + " UTC"
}

const formatRelAgo = (ms: number, now = Date.now()): string => {
  if (!ms) return "—"
  const age = Math.max(0, now - ms)
  const sec = Math.floor(age / 1000)
  if (sec < 60) return `${sec}s ago`
  const min = Math.floor(sec / 60)
  if (min < 60) return `${min}m ago`
  const hr = Math.floor(min / 60)
  if (hr < 24) return `${hr}h ago`
  const d = Math.floor(hr / 24)
  return `${d}d ago`
}

const formatTokens = (n?: number): string => {
  if (!n) return "0"
  if (n < 1000) return String(n)
  if (n < 1_000_000) return (n / 1000).toFixed(n < 10000 ? 1 : 0) + "K"
  return (n / 1_000_000).toFixed(2) + "M"
}

const modelDisplay = (s: SessionInfo): string => {
  if (s.modelId) {
    const variant = s.modelVariant && s.modelVariant !== "default" ? ` · ${s.modelVariant}` : ""
    return s.modelId + variant
  }
  return "unknown model"
}

const sourceLabel = (source: SessionInfo["source"]): string => {
  if (source === "db") return "SQLITE"
  if (source === "cli") return "CLI"
  return "FS"
}

const SessionLane: FC<{ session: SessionInfo; index: number; total: number }> = ({ session, index, total }) => {
  const updatedText = formatUpdated(session.updated || session.created)
  const relText = formatRelAgo(session.updated || session.created)
  const runTag = "RUN-LANE-" + String(total - index).padStart(3, "0")
  const titleLine = "Agent Run"
  const agentName = session.agent || "session"
  const subtitle = `OpenCode ${agentName} thread is active.`
  const statusPhrase = session.status === "running"
    ? `OpenCode ${agentName} thread is live — last touched ${relText}.`
    : session.status === "idle"
    ? `OpenCode ${agentName} thread is paused — last touched ${relText}.`
    : `OpenCode ${agentName} thread is stale — last touched ${relText}.`
  const worktree = session.worktree || "none"
  const branch = session.directory ? session.directory.split("/").filter(Boolean).pop() || worktree : worktree
  const totalTokens = (session.tokensInput || 0) + (session.tokensOutput || 0) + (session.tokensCacheRead || 0)
  return (
    <a class="op-lane" href={`/session?id=${encodeURIComponent(session.id)}`}>
      <div class="op-lane-rail" aria-hidden="true" />
      <div class="op-lane-body">
        <div class="op-lane-head">
          <span class="op-lane-issue">ISSUE / {runTag}</span>
          <span class={`op-lane-status op-lane-status-${session.status}`}>
            <StatusDot status={session.status} /> {statusLabel(session.status)}
          </span>
        </div>
        <h2 class="op-lane-title">{titleLine}</h2>
        <p class="op-lane-subtitle">{subtitle}</p>
        <p class="op-lane-phrase">{statusPhrase}</p>
        <div class="op-lane-stats">
          <span class="op-stat"><span class="op-stat-k">INPUT</span><span class="op-stat-v">{formatTokens(session.tokensInput)}</span></span>
          <span class="op-stat"><span class="op-stat-k">OUTPUT</span><span class="op-stat-v">{formatTokens(session.tokensOutput)}</span></span>
          <span class="op-stat"><span class="op-stat-k">CACHE&nbsp;R</span><span class="op-stat-v">{formatTokens(session.tokensCacheRead)}</span></span>
          <span class="op-stat"><span class="op-stat-k">REASON</span><span class="op-stat-v">{formatTokens(session.tokensReasoning)}</span></span>
          <span class="op-stat"><span class="op-stat-k">TOTAL</span><span class="op-stat-v">{formatTokens(totalTokens)}</span></span>
        </div>
        <div class="op-lane-grid">
          <div class="op-grid-cell">
            <span class="op-grid-k">CODEX THREAD</span>
            <span class="op-grid-v mono">{shortSessionId(session.id)}</span>
          </div>
          <div class="op-grid-cell">
            <span class="op-grid-k">THREAD FLAGS</span>
            <span class="op-grid-v mono">{statusLabel(session.status)} · {sourceLabel(session.source)}</span>
          </div>
          <div class="op-grid-cell">
            <span class="op-grid-k">PROTOCOL EVENT</span>
            <span class="op-grid-v mono">opencode.pty.start</span>
          </div>
          <div class="op-grid-cell">
            <span class="op-grid-k">BRANCH</span>
            <span class="op-grid-v mono" title={session.directory}>{branch}</span>
          </div>
          <div class="op-grid-cell">
            <span class="op-grid-k">WORKTREE</span>
            <span class="op-grid-v mono" title={session.directory || ""}>{worktree}</span>
          </div>
          <div class="op-grid-cell">
            <span class="op-grid-k">BACKLOG OWNERSHIP</span>
            <span class="op-grid-v mono">{session.projectId || "global"}</span>
          </div>
          <div class="op-grid-cell">
            <span class="op-grid-k">MODEL</span>
            <span class="op-grid-v mono" title={session.modelProvider ? `${session.modelProvider}` : ""}>{modelDisplay(session)}</span>
          </div>
          <div class="op-grid-cell">
            <span class="op-grid-k">NEXT RETRY</span>
            <span class="op-grid-v mono">{updatedText} · {relText}</span>
          </div>
        </div>
      </div>
    </a>
  )
}

const SessionsPage: FC<{ sessions: SessionInfo[]; summary: ReturnType<typeof summarizeSessions> }> = ({ sessions, summary }) => {
  const source = sessions[0]?.source ?? "db"
  const sourceText = source === "db" ? "sqlite store" : source === "cli" ? "opencode CLI" : "fs fallback"
  return (
    <Layout title="Sessions" active="sessions">
      <section class="op-flow" aria-label="operator flow">
        <div class="op-flow-cell op-flow-backlog">
          <span class="op-flow-k">BACKLOG</span>
          <span class="op-flow-v">{summary.stale}</span>
          <span class="op-flow-hint">stale &gt; 24h</span>
        </div>
        <div class="op-flow-cell op-flow-running">
          <span class="op-flow-k">RUNNING</span>
          <span class="op-flow-v">{summary.running}</span>
          <span class="op-flow-hint">&lt; 5m touched</span>
        </div>
        <div class="op-flow-cell op-flow-repair">
          <span class="op-flow-k">REPAIR</span>
          <span class="op-flow-v">{summary.idle}</span>
          <span class="op-flow-hint">idle 5m–24h</span>
        </div>
        <div class="op-flow-cell op-flow-ready">
          <span class="op-flow-k">READY</span>
          <span class="op-flow-v">{summary.total}</span>
          <span class="op-flow-hint">total leased</span>
        </div>
      </section>

      <header class="op-section-head">
        <h1 class="op-section-title">RUNNING LANES</h1>
        <div class="op-section-meta">
          <span class="op-section-meta-item">{summary.running} RUNNING · {summary.total} LEASED</span>
          <span class="op-section-meta-item muted">via {sourceText}</span>
        </div>
      </header>

      {sessions.length === 0 ? (
        <div class="op-empty">
          <p>No OpenCode sessions found.</p>
          <p class="muted small">
            Start one with <code>opencode</code> in any project, or ensure{" "}
            <code>~/.local/share/opencode/opencode.db</code> is readable.
          </p>
          <p class="muted small">
            Useful commands: <code>opencode web</code>, <code>opencode serve --port 4096</code>,
            <code>opencode attach http://localhost:4096 --session &lt;id&gt;</code>
          </p>
        </div>
      ) : (
        <div class="op-lanes">
          {sessions.map((s, i) => <SessionLane session={s} index={i} total={sessions.length} />)}
        </div>
      )}

      <section class="op-hints">
        <h2 class="op-hints-title">OPENCODE WEB / SERVE / ATTACH</h2>
        <ul class="op-hints-list">
          <li><code>opencode web</code> — start server and open web interface in your browser.</li>
          <li><code>opencode serve --port 4096</code> — run a headless server on port 4096.</li>
          <li><code>opencode attach http://localhost:4096 --session &lt;id&gt;</code> — attach a TTY client to a running server.</li>
        </ul>
        <p class="op-hints-note muted small">
          The dashboard's primary attach path is the embedded terminal — those commands are provided as a fallback.
        </p>
      </section>
    </Layout>
  )
}

// ---------------------------------------------------------------------------
// Report list page
// ---------------------------------------------------------------------------

const RatingBadge: FC<{ rating: string }> = ({ rating }) => {
  const cls = rating === "高" ? "badge badge-high" : rating === "中" ? "badge badge-medium" : "badge badge-low"
  return <span class={cls}>{rating}</span>
}

const ReportListPage: FC<{ reports: Awaited<ReturnType<typeof scanReports>> }> = ({ reports }) => (
  <Layout title="Reports" active="reports">
    <div class="page-header">
      <h1>Experience Reports</h1>
      <p class="muted">{reports.length} report(s) found in /tmp/opencode/handoff/</p>
    </div>

    {reports.length === 0 ? (
      <div class="empty-state">
        <p>No reports yet.</p>
        <p class="muted">Run <code>/experience-summary</code> in OpenCode to generate a report.</p>
      </div>
    ) : (
      <div class="report-grid">
        {reports.map((r) => (
          <a class="report-card" href={`/report?path=${encodeURIComponent(r.reportPath)}`}>
            <div class="report-card-header">
              <span class="report-session">{r.session}</span>
              <span class="report-date">{r.generated || "unknown date"}</span>
            </div>
            <div class="report-card-body">
              <span class="stat stat-high">{r.highCount} 高</span>
              <span class="stat stat-medium">{r.mediumCount} 中</span>
              <span class="stat stat-total">{r.candidateCount} total</span>
            </div>
            <div class="report-card-footer muted">{r.scope}</div>
          </a>
        ))}
      </div>
    )}
  </Layout>
)

// ---------------------------------------------------------------------------
// Report detail page
// ---------------------------------------------------------------------------

const CandidateCard: FC<{ c: Candidate }> = ({ c }) => (
  <div class="candidate-card" data-cid={c.id}>
    <div class="candidate-header">
      <label class="candidate-check">
        <input type="checkbox" data-cid={c.id} />
        <span class="cid">[{c.id}]</span>
      </label>
      <span class="candidate-title">{c.title}</span>
      <RatingBadge rating={c.valueRating} />
    </div>
    <div class="candidate-body">
      {c.valueReason && <div class="field"><span class="field-label">理由</span><span>{c.valueReason}</span></div>}
      {c.evidenceDetail && <div class="field"><span class="field-label">验证依据</span><span>{c.evidenceDetail}</span></div>}
      {c.source && <div class="field"><span class="field-label">来源</span><span>{c.source}</span></div>}
      {c.targetFile && <div class="field"><span class="field-label">目标</span><code>{c.targetFile}</code></div>}
      {c.changeSummary && <div class="field"><span class="field-label">变更</span><span>{c.changeSummary}</span></div>}
      {c.followUpSkill && <div class="field"><span class="field-label">Skill</span><span>{c.followUpSkill}</span></div>}
      {c.keyEvidence && <div class="field"><span class="field-label">证据</span><span class="muted">{c.keyEvidence}</span></div>}
      {c.executionNotes && <div class="field"><span class="field-label">注意</span><span class="muted">{c.executionNotes}</span></div>}
    </div>
  </div>
)

const ReportDetailPage: FC<{ report: ParsedReport; reportPath: string }> = ({ report, reportPath }) => (
  <Layout title={`Report — ${report.meta.session}`} active="reports">
    <div class="page-header">
      <a href="/reports" class="back-link">← Back to reports</a>
      <h1>{report.meta.session || "Session"}</h1>
      <div class="meta-grid">
        {report.meta.scope && <div><span class="field-label">Scope</span> {report.meta.scope}</div>}
        {report.meta.generated && <div><span class="field-label">Generated</span> {report.meta.generated}</div>}
        {report.meta.artifact && <div><span class="field-label">Artifact</span> <code>{report.meta.artifact}</code></div>}
      </div>
    </div>

    {report.candidates.length === 0 ? (
      <div class="empty-state"><p>No candidates in this report.</p></div>
    ) : (
      <>
        <div class="action-bar" id="action-bar">
          <span class="muted" id="selection-info">0 selected</span>
          <button class="btn btn-primary" id="btn-confirm">Confirm Selected</button>
          <button class="btn btn-reject" id="btn-reject">Reject Selected</button>
          <button class="btn btn-secondary" id="btn-select-all">Select All</button>
          <button class="btn btn-secondary" id="btn-deselect-all">Deselect All</button>
        </div>

        <div class="candidate-list">
          {report.candidates
            .filter((c) => c.category === "candidate")
            .map((c) => <CandidateCard c={c} />)}
        </div>

        {report.candidates.some((c) => c.category === "interaction") && (
          <>
            <h2 class="section-title">主/子 Agent 互动优化</h2>
            <div class="candidate-list">
              {report.candidates
                .filter((c) => c.category === "interaction")
                .map((c) => <CandidateCard c={c} />)}
            </div>
          </>
        )}
      </>
    )}

    {report.risksGaps && (
      <div class="risks-section">
        <h2>Risks / Gaps</h2>
        <pre>{report.risksGaps}</pre>
      </div>
    )}

    <script dangerouslySetInnerHTML={{
      __html: `window.__REPORT_PATH__ = ${JSON.stringify(reportPath)};`,
    }} />
  </Layout>
)

// ---------------------------------------------------------------------------
// Session detail (embedded terminal) page
// ---------------------------------------------------------------------------

const SessionTerminalPage: FC<{ session: SessionInfo }> = ({ session }) => {
  const updatedText = formatUpdated(session.updated || session.created)
  const worktree = session.worktree || "none"
  const model = modelDisplay(session)
  return (
    <Layout title={`Session ${session.id}`} active="sessions">
      <div class="page-header session-detail-header">
        <a href="/" class="back-link">← All sessions</a>
        <h1 class="mono">{session.title || session.id}</h1>
        <div class="meta-grid">
          <div><span class="field-label">Session</span> <code>{session.id}</code></div>
          <div><span class="field-label">Status</span> <span class={`status-pill status-${session.status}`}>{statusLabel(session.status)}</span></div>
          <div><span class="field-label">Project</span> {session.projectId || "global"}</div>
          <div><span class="field-label">Agent</span> {session.agent || "—"}</div>
          <div><span class="field-label">Model</span> <code>{model}</code></div>
          <div><span class="field-label">Worktree</span> <code>{worktree}</code></div>
          <div><span class="field-label">Updated</span> {updatedText}</div>
          {session.directory ? <div><span class="field-label">Cwd</span> <code>{session.directory}</code></div> : null}
          <div><span class="field-label">Source</span> {sourceLabel(session.source)}</div>
        </div>
      </div>

      <div class="terminal-wrap">
        <div class="terminal-header">
          <div class="terminal-header-left">
            <span class="dot dot-red" />
            <span class="dot dot-yellow" />
            <span class="dot dot-green" />
            <span class="terminal-title mono">opencode --session {session.id}</span>
          </div>
          <div class="terminal-header-right muted small">
            <span>WebSocket: /ws/session-terminal</span>
          </div>
        </div>
        <div class="terminal-host-shell">
          <div id="terminal" class="terminal-host" data-session-id={session.id} />
        </div>
        <div id="terminal-status" class="terminal-status muted small">connecting…</div>
      </div>

      <section class="hints-section">
        <h2>OpenCode CLI hints</h2>
        <ul class="hints-list">
          <li><code>opencode web</code> — start OpenCode's own web interface in your browser.</li>
          <li><code>opencode serve --port 4096</code> — start a headless server, then attach with:</li>
          <li><code>opencode attach http://localhost:4096 --session {session.id}</code></li>
        </ul>
        <p class="muted small">
          This page runs an embedded <code>node-pty</code> terminal locally; it is independent of any
          remote <code>opencode serve</code> process.
        </p>
      </section>

      <script
        type="module"
        src="/static/terminal.js"
        data-session-id={session.id}
        dangerouslySetInnerHTML={undefined}
      />
    </Layout>
  )
}

const SessionMissingPage: FC<{ id: string }> = ({ id }) => (
  <Layout title={`Session ${id} not found`} active="sessions">
    <div class="page-header">
      <a href="/" class="back-link">← All sessions</a>
      <h1>Session not available</h1>
      <p class="muted">
        <code>{id}</code> was not found in the OpenCode session list. It may have been archived.
      </p>
    </div>
  </Layout>
)

// ---------------------------------------------------------------------------
// Hono app
// ---------------------------------------------------------------------------

const app = new Hono()

// Static files under /static/* (public/ dir).
app.get("/static/*", async (c) => {
  const path = c.req.path.replace("/static/", "")
  // Refuse to escape public/ via "..".
  if (path.includes("..") || path.startsWith("/")) return c.text("Forbidden", 403)
  const filePath = join(PUBLIC_DIR, path)
  try {
    const content = await readFile(filePath)
    const ext = path.split(".").pop()?.toLowerCase() ?? ""
    const contentType =
      ext === "css" ? "text/css" :
      ext === "js" ? "application/javascript" :
      ext === "mjs" ? "application/javascript" :
      "text/plain"
    return new Response(content, { headers: { "Content-Type": contentType, "Cache-Control": "no-cache" } })
  } catch {
    return c.text("Not found", 404)
  }
})

// Vendor xterm browser assets out of node_modules without copying.
function vendorFile(pkg: string, rel: string, contentType: string) {
  return async (c: any) => {
    const safeRel = rel.replace(/^\/+/, "")
    if (safeRel.includes("..") || safeRel.startsWith("/")) return c.text("Forbidden", 403)
    const filePath = join(NODE_MODULES_DIR, pkg, safeRel)
    if (!existsSync(filePath)) return c.text("Not found", 404)
    try {
      const content = await readFile(filePath)
      return new Response(content, {
        headers: {
          "Content-Type": contentType,
          "Cache-Control": "public, max-age=3600",
        },
      })
    } catch {
      return c.text("Not found", 404)
    }
  }
}

app.get("/vendor/xterm/xterm.css", vendorFile("@xterm/xterm", "css/xterm.css", "text/css"))
app.get("/vendor/xterm/xterm.js", vendorFile("@xterm/xterm", "lib/xterm.js", "application/javascript"))
app.get("/vendor/xterm-addon-fit/addon-fit.js", vendorFile("@xterm/addon-fit", "lib/addon-fit.js", "application/javascript"))

// Sessions landing page
app.get("/", async (c) => {
  const sessions = await scanSessions()
  const summary = summarizeSessions(sessions)
  return c.html(<SessionsPage sessions={sessions} summary={summary} />)
})

// Refresh cache: re-scan sessions.
app.get("/sessions/refresh", async (c) => {
  const sessions = await scanSessions(true)
  const summary = summarizeSessions(sessions)
  return c.html(<SessionsPage sessions={sessions} summary={summary} />)
})

// Reports list (the original / path moved here)
app.get("/reports", async (c) => {
  const reports = await scanReports()
  return c.html(<ReportListPage reports={reports} />)
})

// Backwards-compatible redirect: /report (no s) -> /reports
app.get("/report", async (c) => {
  const rawPath = c.req.query("path")
  if (!rawPath) {
    return c.redirect("/reports", 302)
  }
  const reportPath = resolveHandoffPath(rawPath)
  if (!reportPath) {
    return c.text("Forbidden path", 403)
  }
  const report = await getReport(reportPath)
  if (!report) return c.text("Report not found", 404)
  return c.html(<ReportDetailPage report={report} reportPath={reportPath} />)
})

// Embedded terminal page
app.get("/session", async (c) => {
  const id = c.req.query("id")
  if (!id) {
    return c.text("Missing session id", 400)
  }
  const session = await getSession(id)
  if (!session) {
    return c.html(<SessionMissingPage id={id} />, 404)
  }
  return c.html(<SessionTerminalPage session={session} />)
})

// API: confirm or reject candidates (unchanged)
app.post("/api/confirm", async (c) => {
  const body = await c.req.json<Confirmation>()
  const reportPath = resolveHandoffPath(body.reportPath)
  if (!reportPath) {
    return c.json({ error: "Invalid reportPath" }, 400)
  }

  const confirmation: Confirmation = {
    reportPath,
    confirmedIds: body.confirmedIds || [],
    rejectedIds: body.rejectedIds || [],
    mode: body.mode || "confirm",
    timestamp: new Date().toISOString(),
  }

  const savedPath = await saveConfirmation(confirmation)
  return c.json({ ok: true, savedPath })
})

// API: list reports (JSON, unchanged)
app.get("/api/reports", async (c) => {
  const reports = await scanReports()
  return c.json(reports)
})

// API: get report detail (JSON, unchanged)
app.get("/api/report", async (c) => {
  const reportPath = resolveHandoffPath(c.req.query("path"))
  if (!reportPath) {
    return c.json({ error: "Invalid path" }, 400)
  }
  const report = await getReport(reportPath)
  if (!report) return c.json({ error: "Not found" }, 404)
  return c.json(report)
})

// API: list sessions (JSON)
app.get("/api/sessions", async (c) => {
  const sessions = await scanSessions()
  return c.json({ summary: summarizeSessions(sessions), sessions })
})

// API: get a single session
app.get("/api/session", async (c) => {
  const id = c.req.query("id")
  if (!id) return c.json({ error: "Missing id" }, 400)
  const session = await getSession(id)
  if (!session) return c.json({ error: "Not found" }, 404)
  return c.json(session)
})

// ---------------------------------------------------------------------------
// WebSocket: /ws/session-terminal?id=...
// ---------------------------------------------------------------------------

const wss = new WebSocketServer({ noServer: true })

app.get(
  "/ws/session-terminal",
  upgradeWebSocket(() => {
    let session: TerminalSession | null = null
    let exited = false
    return {
      onOpen: async (_evt, ws) => {
        try {
          const url = ws.url ? new URL(ws.url) : null
          const id = url?.searchParams.get("id") ?? ""
          const sessionInfo = await getSession(id)
          const directory = sessionInfo?.directory ?? null
          const result = startSession(id, directory, {
            onOutput: (chunk) => {
              if (exited) return
              try {
                ws.send(chunk)
              } catch {
                // ignore send failures (closed)
              }
            },
            onExit: (code, signal) => {
              exited = true
              try {
                ws.send(JSON.stringify({ type: "exit", code, signal: signal ?? null }))
              } catch {
                // ignore
              }
              try { ws.close(1000, "process exited") } catch { /* noop */ }
            },
            onError: (message) => {
              try {
                ws.send(JSON.stringify({ type: "error", message }))
              } catch {
                // ignore
              }
              try { ws.close(1011, "spawn error") } catch { /* noop */ }
            },
          })
          if ("error" in result) {
            try {
              ws.send(JSON.stringify({ type: "error", message: result.error }))
            } catch { /* noop */ }
            try { ws.close(1008, result.error) } catch { /* noop */ }
            return
          }
          session = result
          try {
            ws.send(JSON.stringify({ type: "ready", id: result.id, cols: result.cols, rows: result.rows }))
          } catch { /* noop */ }
        } catch (err) {
          const message = err instanceof Error ? err.message : String(err)
          try {
            ws.send(JSON.stringify({ type: "error", message }))
          } catch { /* noop */ }
          try { ws.close(1011, "open error") } catch { /* noop */ }
        }
      },
      onMessage: (evt, ws) => {
        if (!session) return
        const data = typeof evt.data === "string" ? evt.data : ""
        if (!data) return
        const msg = parseClientMessage(data)
        if (msg.kind === "input") {
          writeToSession(session, msg.data)
        } else if (msg.kind === "resize") {
          resizeSession(session, msg.cols, msg.rows)
        }
      },
      onClose: () => {
        if (session) {
          killSession(session)
          session = null
        }
        exited = true
      },
      onError: () => {
        if (session) {
          killSession(session)
          session = null
        }
        exited = true
      },
    }
  })
)

// ---------------------------------------------------------------------------
// Server bootstrap
// ---------------------------------------------------------------------------

const port = parseInt(process.env.PORT || "7331", 10)

serve({ fetch: app.fetch, websocket: { server: wss }, port }, (info) => {
  console.log(`OpenCode Dashboard running at http://localhost:${info.port}`)
})
