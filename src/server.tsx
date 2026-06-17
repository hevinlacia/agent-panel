/** @jsxImportSource hono/jsx */
import { Hono } from "hono"
import { html } from "hono/html"
import { type FC } from "hono/jsx"
import { serve } from "@hono/node-server"
import { join } from "node:path"
import { scanReports, getReport, saveConfirmation, type Confirmation } from "./scanner.ts"
import type { Candidate, ParsedReport } from "./parser.ts"

// ---------------------------------------------------------------------------
// Layout
// ---------------------------------------------------------------------------

const Layout: FC<{ title: string; children: any }> = ({ title, children }) => (
  <html lang="zh-CN">
    <head>
      <meta charset="utf-8" />
      <meta name="viewport" content="width=device-width, initial-scale=1" />
      <title>{title} — OpenCode Dashboard</title>
      <link rel="stylesheet" href="/static/style.css" />
    </head>
    <body>
      <header class="topbar">
        <a href="/" class="logo">OpenCode Dashboard</a>
        <nav>
          <a href="/">Reports</a>
        </nav>
      </header>
      <main>{children}</main>
      <script src="/static/app.js" defer></script>
    </body>
  </html>
)

// ---------------------------------------------------------------------------
// Value rating badge
// ---------------------------------------------------------------------------

const RatingBadge: FC<{ rating: string }> = ({ rating }) => {
  const cls = rating === "高" ? "badge badge-high" : rating === "中" ? "badge badge-medium" : "badge badge-low"
  return <span class={cls}>{rating}</span>
}

// ---------------------------------------------------------------------------
// Report list page
// ---------------------------------------------------------------------------

const ReportListPage: FC<{ reports: Awaited<ReturnType<typeof scanReports>> }> = ({ reports }) => (
  <Layout title="Reports">
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
  <Layout title={`Report — ${report.meta.session}`}>
    <div class="page-header">
      <a href="/" class="back-link">← Back to reports</a>
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
// Hono app
// ---------------------------------------------------------------------------

const app = new Hono()

// Static files (CSS/JS)
app.get("/static/*", async (c) => {
  const path = c.req.path.replace("/static/", "")
  const filePath = join(import.meta.dirname, "..", "public", path)
  try {
    const { readFile } = await import("node:fs/promises")
    const content = await readFile(filePath)
    const ext = path.split(".").pop()
    const contentType = ext === "css" ? "text/css" : ext === "js" ? "application/javascript" : "text/plain"
    return new Response(content, { headers: { "Content-Type": contentType } })
  } catch {
    return c.text("Not found", 404)
  }
})

// Report list
app.get("/", async (c) => {
  const reports = await scanReports()
  return c.html(<ReportListPage reports={reports} />)
})

// Report detail
app.get("/report", async (c) => {
  const reportPath = c.req.query("path")
  if (!reportPath) return c.text("Missing path parameter", 400)

  // Prevent path traversal
  if (!reportPath.startsWith("/tmp/opencode/handoff/")) {
    return c.text("Forbidden path", 403)
  }

  const report = await getReport(reportPath)
  if (!report) return c.text("Report not found", 404)

  return c.html(<ReportDetailPage report={report} reportPath={reportPath} />)
})

// API: confirm or reject candidates
app.post("/api/confirm", async (c) => {
  const body = await c.req.json<Confirmation>()
  if (!body.reportPath || !body.reportPath.startsWith("/tmp/opencode/handoff/")) {
    return c.json({ error: "Invalid reportPath" }, 400)
  }

  const confirmation: Confirmation = {
    reportPath: body.reportPath,
    confirmedIds: body.confirmedIds || [],
    rejectedIds: body.rejectedIds || [],
    mode: body.mode || "confirm",
    timestamp: new Date().toISOString(),
  }

  const savedPath = await saveConfirmation(confirmation)
  return c.json({ ok: true, savedPath })
})

// API: list reports (JSON)
app.get("/api/reports", async (c) => {
  const reports = await scanReports()
  return c.json(reports)
})

// API: get report detail (JSON)
app.get("/api/report", async (c) => {
  const reportPath = c.req.query("path")
  if (!reportPath || !reportPath.startsWith("/tmp/opencode/handoff/")) {
    return c.json({ error: "Invalid path" }, 400)
  }
  const report = await getReport(reportPath)
  if (!report) return c.json({ error: "Not found" }, 404)
  return c.json(report)
})

const port = parseInt(process.env.PORT || "7331", 10)

serve({ fetch: app.fetch, port }, (info) => {
  console.log(`OpenCode Dashboard running at http://localhost:${info.port}`)
})
