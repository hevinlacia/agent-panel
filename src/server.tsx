/** @jsxImportSource @kitajs/html */
/**
 * Role: wires Agent Panel pages, APIs, and browser assets into the Fastify server.
 * Public surface: the HTTP routes started at the bottom of this module.
 * Constraints: keep filesystem and session safety gates in their dedicated modules.
 * Read-this-with: src/requirements.ts, src/requirementBoard.ts, src/fastify/context.ts, and public/style.css.
 */
import Fastify from "fastify"
import fastifyStatic from "@fastify/static"
import fastifyFormbody from "@fastify/formbody"
import fastifyMultipart from "@fastify/multipart"
import fastifyWebsocket from "@fastify/websocket"
import fastifySwagger from "@fastify/swagger"
import fastifySwaggerUi from "@fastify/swagger-ui"
import { Type } from "@sinclair/typebox"
import { NAV_ITEMS, sessionsDaysPath, SESSIONS_PATH, DASHBOARD_PATH } from "./navigation.ts"
import { type FC, type Ctx, createRouter } from "./fastify/context.ts"
import { readFile, writeFile, appendFile, readdir, rename, mkdir } from "node:fs/promises"
import { existsSync } from "node:fs"
import { spawn } from "node:child_process"
import { randomUUID } from "node:crypto"
import { join, dirname, resolve } from "node:path"
import { fileURLToPath } from "node:url"
import { scanReports, getReport, saveConfirmation, getConfirmationStatus, type Confirmation, type ConfirmationStatus } from "./scanner.ts"
import type { Candidate, ParsedReport } from "./parser.ts"
import {
  scanSessions,
  summarizeSessions,
  groupSessionsByParent,
  isValidSessionId,
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
import { shouldAutoInjectRequirementContext } from "./terminalUrl.ts"
import { writeRequirementStatus, writeRequirementCategory, nextStatus, readRequirementState, type RequirementState } from "./requirementState.ts"
import {
  REQ_STATUSES,
  REQ_CATEGORIES,
  type ReqStatus,
  type ReqCategory,
  type Requirement,
  listRequirementsByProject,
  getRequirement,
  associateSession,
  dissociateSession,
  replaceAssociatedSession,
  getRequirementForSession,
  getAllAssociatedSessionIds,
  buildInjectionContext,
  writeInjectionContext,
  scanHermesRequirements,
  loadAssociations,
  DEFAULT_REQ_ID,
  DEFAULT_PROJECT_NAME,
} from "./requirements.ts"
import {
  buildRequirementBoardItems,
  parseRequirementDateBoundary,
  type RequirementBoardItem,
} from "./requirementBoard.ts"
import { buildRequirementStats, formatDuration } from "./dashboardStats.ts"
import {
  buildExtractPrompt,
  appendSummaryToNotes,
  runExtractStandalone,
} from "./sessionExtract.ts"
import {
  createExtractJob,
  getExtractJob,
  findRunningJobForSession,
  findRecentJobForSession,
  checkExtractGuard,
  EXTRACT_DEBOUNCE_MS,
  JobConflictError,
  type ExtractJob,
} from "./extractJobs.ts"
import { enqueueAutoExtract, getQueueStatus } from "./extractQueue.ts"
import {
  initNotifications,
  getNotifications,
  getNotification,
  getUnreadCount,
  dismissNotification,
  dismissAll,
  markAllRead,
  createNotification,
  updateNotification,
} from "./notifications.ts"
import {
  buildManagedEnv,
  getConfig,
  getSafeConfig,
  setConfig,
  initConfig,
  safeEnvVars,
  safeEnvVarsByFile,
  deleteEnvVar,
  ENV_VAR_CATALOG,
  type AppConfig,
  type EnvFileKind,
  type EnvFileGroup,
  type EnvVarEntry,
  upsertEnvVar,
} from "./config.ts"
import {
  getPiConfigFile,
  isPiConfigFileKey,
  readPiConfigSummary,
  savePiConfigFile,
  updatePiSettings,
} from "./piConfig.ts"
import {
  scanDashboardSessions,
  getDashboardSession,
  getDashboardSessionsByIds,
  clearDashboardSessionCache,
  isValidDashboardSessionId,
  extractDashboardSessionId,
  buildResumeCommand,
  harnessLabel,
  type DashboardHarness,
} from "./dashboardSessions.ts"
import { extractTokensFromCurl, type ExtractedToken } from "./tokenExtract.ts"
import {
  buildReleaseChecklist,
  type ReleaseChecklist,
  type ChecklistFiles,
} from "./releaseChecklist.ts"
import {
  readBranchScope,
  fallbackFromBranchMd,
  BRANCH_SCOPE_FILE,
  type BranchScope,
} from "./branchScope.ts"
import { buildBranchScopePrompt, runAiBranchScopeExtraction } from "./branchScopeExtract.ts"
import {
  ALIGNMENT_FILE,
  ALIGNMENT_TEMPLATE,
  PRD_FILE,
  PRD_TEMPLATE,
} from "./requirementAlignment.ts"
import {
  IMPACT_FILE,
  IMPACT_TEMPLATE,
  buildImpactAssessment,
  type ImpactAssessment,
} from "./impactAssessment.ts"
import {
  CODE_REVIEW_STATUSES,
  DEFAULT_CODE_REVIEW_BASE_REF,
  readCodeReviewSnapshot,
  runCodeReviewScan,
  saveCodeReviewVerdict,
  saveCodeReviewAiResult,
  runAiCodeReview,
  parseUnifiedDiff,
  type CodeReviewDiffLine,
  type CodeReviewFileDiff,
  type CodeReviewSnapshot,
  type CodeReviewStatus,
  type CodeReviewAiResult,
} from "./codeReview.ts"
import {
  detectHighlightLanguage,
  highlightDiffLines,
} from "./codeHighlight.ts"
import {
  buildAutoExtractPrompt,
  parseAutoExtractOutput,
  filterAllowed,
  type AutoExtractResult,
  type ContextFiles,
} from "./autoExtract.ts"
import {
  FORK_TITLE_RE,
  recommendSessionsForRequirement,
  type SessionRecommendation,
} from "./sessionRecommendations.ts"
import {
  getExtractHistoryForRequirement,
  getLastExtractForSession,
  type ExtractHistoryRecord,
} from "./extractHistory.ts"
import {
  initMarkers,
  markSession,
  unmarkSession,
  getMarker,
  listMarkers,
  type ExperienceMarker,
  type MarkerStatus,
} from "./experienceMarkers.ts"
import {
  startAutoSummaryWorker,
  stopAutoSummaryWorker,
  triggerExecutionForMarker,
  isAutoSummaryWorkerRunning,
} from "./experienceAutoSummary.ts"
import {
  startAutoExtractScheduler,
  stopAutoExtractScheduler,
  isAutoExtractSchedulerRunning,
  POLL_INTERVAL_MS as AUTO_EXTRACT_POLL_MS,
} from "./autoExtractScheduler.ts"
import {
  startAutoValuationWorker,
  stopAutoValuationWorker,
  isAutoValuationWorkerRunning,
  getValuationStats,
  getRecentCandidates,
  pollOnce as valuationPollOnce,
  POLL_INTERVAL_MS as VALUATION_POLL_MS,
  type ValuationStats,
} from "./autoValuation.ts"
import {
  buildRecallMarkdown,
  readSessionTranscript,
} from "./sessionTranscript.ts"
import {
  DEFAULT_FULL_SYNC_TIMES,
  getLastFullSyncResult,
  isFullSyncSchedulerRunning,
  POLL_INTERVAL_MS as FULL_SYNC_POLL_MS,
  startFullSyncScheduler,
  stopFullSyncScheduler,
} from "./fullSyncScheduler.ts"
import {
  acquireSchedulerLock,
  releaseSchedulerLock,
} from "./schedulerLock.ts"
import {
  getOpencodeProcessQueueStatus,
  runQueuedOpencodeProcess,
} from "./opencodeProcessQueue.ts"
import {
  buildAutoDriveJobName,
  buildAutoDrivePrompt,
  createAutoDriveJob,
  finalizeAutoDriveJobFromResult,
  getAutoDriveJobs,
  getLatestAutoDriveJobForRequirement,
  initAutoDriveJobs,
  updateAutoDriveJob,
  type AutoDriveJob,
} from "./requirementAutoDrive.ts"
import {
  ATTACHMENTS_DIR_NAME,
  listAttachments,
  writeAttachment,
  deleteAttachment,
  readAttachmentBuffer,
  resolveAttachmentPath,
  type AttachmentInfo,
} from "./requirementAttachments.ts"

const __filename = fileURLToPath(import.meta.url)
const __dirname = dirname(__filename)
const PROJECT_ROOT = resolve(__dirname, "..")
const PUBLIC_DIR = join(PROJECT_ROOT, "public")
const NODE_MODULES_DIR = join(PROJECT_ROOT, "node_modules")

// ---------------------------------------------------------------------------
// Layout
// ---------------------------------------------------------------------------

type Tab = "sessions" | "reports" | "requirements" | "dashboard" | "settings" | "schedulers" | "envvars"

const NAV_ICONS: Record<Tab, string> = {
  requirements: "PR",
  dashboard: "DB",
  sessions: "SE",
  reports: "RP",
  schedulers: "SC",
  settings: "ST",
  envvars: "EV",
}

const NAV_LABELS: Record<Tab, string> = {
  requirements: "需求看板",
  dashboard: "状态看板",
  sessions: "Sessions",
  reports: "Reports",
  schedulers: "Schedulers",
  settings: "Settings",
  envvars: "Env Vars",
}

/**
 * Ark-router-inspired application shell: fixed sidebar for primary sections,
 * compact top command row, and unchanged page body contracts for each route.
 */
const Layout: FC<{ title: string; active: Tab; children: any; wide?: boolean }> = ({ title, active, children, wide = false }) => (
  <html lang="zh-CN">
    <head>
      <meta charset="utf-8" />
      <meta name="viewport" content="width=device-width, initial-scale=1" />
      <title>{title} — Agent Panel</title>
      <link rel="stylesheet" href="/static/style.css?v=20260714-category-search" />
    </head>
    <body>
      <div class="op-shell">
        <aside class="op-sidebar">
          <a class="op-sidebar-brand" href="/" aria-label="Agent Panel home">
            <span class="op-brand-mark">AP</span>
            <span class="op-brand-copy">
              <span class="op-brand-name">Agent</span>
              <span class="op-brand-status">Panel</span>
            </span>
          </a>
          <nav class="op-sidebar-nav" aria-label="Primary navigation">
            {NAV_ITEMS.map((item) => {
              const key = item.key as Tab
              return (
                <a href={item.href} class={active === key ? "op-sidebar-link op-sidebar-link-active" : "op-sidebar-link"}>
                  <span class="op-sidebar-icon">{NAV_ICONS[key]}</span>
                  <span>{NAV_LABELS[key]}</span>
                </a>
              )
            })}
          </nav>
          <div class="op-sidebar-card">
            <span class="op-sidebar-card-k">LOCAL PANEL</span>
            <span class="op-sidebar-card-v">localhost:7331</span>
          </div>
        </aside>
        <div class="op-content-shell">
          <header class="op-topbar">
            <div class="op-topbar-row">
              <div class="op-title-block">
                <span class="op-title-eyebrow">{NAV_LABELS[active]}</span>
                <span class="op-title-main">{title}</span>
              </div>
              <div class="op-meta">
                <button type="button" class="op-meta-item op-refresh" id="op-force-refresh" title="强制刷新当前页面">Refresh</button>
                <div class="op-notify" id="op-notify">
                  <button type="button" class="op-notify-bell" id="op-notify-bell" aria-label="通知中心" aria-expanded="false">
                    <span class="op-notify-icon" aria-hidden="true">N</span>
                    <span class="op-notify-badge" id="op-notify-badge" hidden>0</span>
                  </button>
                  <div class="op-notify-panel" id="op-notify-panel" hidden role="dialog" aria-label="通知列表">
                    <div class="op-notify-panel-head">
                      <span class="op-notify-panel-title">通知中心</span>
                      <div class="op-notify-panel-actions">
                        <button type="button" class="op-notify-link" id="op-notify-mark-read">全部标记已读</button>
                        <button type="button" class="op-notify-link" id="op-notify-dismiss-all">全部清除</button>
                      </div>
                    </div>
                    <ul class="op-notify-list" id="op-notify-list"></ul>
                    <div class="op-notify-empty" id="op-notify-empty" hidden>暂无通知</div>
                  </div>
                </div>
              </div>
            </div>
          </header>
          <main class={`${(active === "sessions" || active === "requirements") ? "op-main op-main-sessions" : "op-main"}${wide ? " op-main-wide" : ""}`}>{children}</main>
        </div>
      </div>
      <div id="op-toast-host" class="op-toast-host" aria-live="polite" aria-atomic="false"></div>
      <script src="/static/notifications.js" defer></script>
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

const AGENT_BADGE_COLOR_CLASS: Record<string, string> = {
  orchestrator: "op-lane-child-agent-agent-orchestrator",
  "code-writer": "op-lane-child-agent-agent-code-writer",
  "code-reviewer": "op-lane-child-agent-agent-code-reviewer",
  "test-runner": "op-lane-child-agent-agent-test-runner",
  "code-explorer": "op-lane-child-agent-agent-code-explorer",
  debugger: "op-lane-child-agent-agent-debugger",
  general: "op-lane-child-agent-agent-general",
}

function childAgentBadgeClass(agent?: string): string {
  if (agent && Object.prototype.hasOwnProperty.call(AGENT_BADGE_COLOR_CLASS, agent)) {
    return AGENT_BADGE_COLOR_CLASS[agent]
  }
  return AGENT_BADGE_COLOR_CLASS.general
}

function childAgentDisplay(agent?: string): string {
  if (!agent) return "general"
  return agent
}

const ChildSessionCard: FC<{ child: SessionInfo }> = ({ child }) => {
  const badge = childAgentBadgeClass(child.agent)
  const label = childAgentDisplay(child.agent)
  return (
    <a class="op-lane-child" href={`/session?id=${encodeURIComponent(child.id)}`}>
      <StatusDot status={child.status} />
      <span class={`op-lane-child-agent ${badge}`}>{label}</span>
      <span class="op-lane-child-title" title={child.title}>{child.title}</span>
      <span class="op-lane-child-time">{formatRelAgo(child.updated || child.created)}</span>
    </a>
  )
}

const SessionLane: FC<{ session: SessionInfo; index: number; total: number; childSessions?: SessionInfo[] }> = ({ session, index, total, childSessions }) => {
  const updatedText = formatUpdated(session.updated || session.created)
  const relText = formatRelAgo(session.updated || session.created)
  const runTag = "RUN-LANE-" + String(total - index).padStart(3, "0")
  const titleLine = "Agent Run"
  const agentName = session.agent || "session"
  const subtitle = `Pi ${agentName} session is active.`
  const statusPhrase = session.status === "running"
    ? `Pi ${agentName} session is live — last touched ${relText}.`
    : session.status === "idle"
    ? `Pi ${agentName} session is paused — last touched ${relText}.`
    : `Pi ${agentName} session is stale — last touched ${relText}.`
  const worktree = session.worktree || "none"
  const totalTokens = (session.tokensInput || 0) + (session.tokensOutput || 0) + (session.tokensReasoning || 0) + (session.tokensCacheRead || 0) + (session.tokensCacheWrite || 0)
  const messageCount = session.messageCount ?? 0
  const childList = childSessions && childSessions.length > 0 ? childSessions : null
  return (
    <div class="op-lane">
      <a class="op-lane-main" href={`/session?id=${encodeURIComponent(session.id)}`}>
        <div class="op-lane-rail" aria-hidden="true" />
        <div class="op-lane-body">
          <div class="op-lane-head">
            <span class="op-lane-issue">ISSUE / {runTag}</span>
            <span class={`op-lane-status op-lane-status-${session.status}`}>
              <StatusDot status={session.status} /> {statusLabel(session.status)}
            </span>
          </div>
          <h2 class="op-lane-title">{titleLine} <span class="op-lane-title-sep">·</span> <span class="op-lane-title-name">{session.title}</span></h2>
          <p class="op-lane-subtitle">{subtitle}</p>
          <p class="op-lane-phrase">{statusPhrase}</p>
          <div class="op-lane-stats">
            <span class="op-stat"><span class="op-stat-k">MESSAGES</span><span class="op-stat-v">{formatTokens(messageCount)}</span></span>
            <span class="op-stat"><span class="op-stat-k">USER</span><span class="op-stat-v">{formatTokens(session.userMessageCount)}</span></span>
            <span class="op-stat"><span class="op-stat-k">ASSIST</span><span class="op-stat-v">{formatTokens(session.assistantMessageCount)}</span></span>
            <span class="op-stat"><span class="op-stat-k">TOOLS</span><span class="op-stat-v">{formatTokens(session.toolCallCount)}</span></span>
            <span class="op-stat"><span class="op-stat-k">TOKENS</span><span class="op-stat-v">{formatTokens(totalTokens)}</span></span>
          </div>
          <div class="op-lane-grid">
            <div class="op-grid-cell">
              <span class="op-grid-k">PI SESSION</span>
              <span class="op-grid-v mono">{shortSessionId(session.id)}</span>
            </div>
            <div class="op-grid-cell">
              <span class="op-grid-k">STATUS</span>
              <span class="op-grid-v mono">{statusLabel(session.status)} · updated {relText}</span>
            </div>
            <div class="op-grid-cell">
              <span class="op-grid-k">PROJECT DIR</span>
              <span class="op-grid-v mono" title={session.directory || ""}>{worktree}</span>
            </div>
            <div class="op-grid-cell">
              <span class="op-grid-k">MODEL</span>
              <span class="op-grid-v mono" title={session.modelProvider ? `${session.modelProvider}` : ""}>{modelDisplay(session)}</span>
            </div>
            <div class="op-grid-cell">
              <span class="op-grid-k">PROVIDER</span>
              <span class="op-grid-v mono">{session.modelProvider || "unknown"}</span>
            </div>
            <div class="op-grid-cell">
              <span class="op-grid-k">THINKING</span>
              <span class="op-grid-v mono">{session.thinkingLevel || "default"}</span>
            </div>
            <div class="op-grid-cell">
              <span class="op-grid-k">TOOL RESULTS</span>
              <span class="op-grid-v mono">{formatTokens(session.toolResultCount)}</span>
            </div>
            <div class="op-grid-cell">
              <span class="op-grid-k">UPDATED</span>
              <span class="op-grid-v mono">{updatedText}</span>
            </div>
          </div>
        </div>
      </a>
      {childList && (
        <details class="op-lane-children">
          <summary class="op-lane-children-toggle">
            <span class="op-lane-children-count">{childList.length}</span>
            <span>子 Session</span>
            <span class="op-lane-children-arrow" aria-hidden="true">{"▾"}</span>
          </summary>
          <div class="op-lane-children-list">
            {childList.map((c) => <ChildSessionCard child={c} />)}
          </div>
        </details>
      )}
    </div>
  )
}

const SessionsPage: FC<{ sessions: SessionInfo[]; summary: ReturnType<typeof summarizeSessions>; days: number; harness: DashboardHarness }> = ({ sessions, summary, days, harness }) => {
  const { top, childrenByParent } = groupSessionsByParent(sessions)
  // Flow strip and totals count only top-level sessions, not subagent children.
  const topSummary = summarizeSessions(top)
  const source = top[0]?.source ?? sessions[0]?.source ?? "db"
  const sourceText = harness === "pi"
    ? "pi jsonl store"
    : source === "db" ? "sqlite store" : source === "cli" ? "opencode CLI" : "fs fallback"
  return (
    <Layout title="Sessions" active="sessions">
      <section class="op-flow" aria-label="operator flow">
        <div class="op-flow-cell op-flow-backlog">
          <span class="op-flow-k">BACKLOG</span>
          <span class="op-flow-v">{topSummary.stale}</span>
          <span class="op-flow-hint">stale &gt; 24h</span>
        </div>
        <div class="op-flow-cell op-flow-running">
          <span class="op-flow-k">RUNNING</span>
          <span class="op-flow-v">{topSummary.running}</span>
          <span class="op-flow-hint">&lt; 5m touched</span>
        </div>
        <div class="op-flow-cell op-flow-repair">
          <span class="op-flow-k">REPAIR</span>
          <span class="op-flow-v">{topSummary.idle}</span>
          <span class="op-flow-hint">idle 5m–24h</span>
        </div>
        <div class="op-flow-cell op-flow-ready">
          <span class="op-flow-k">READY</span>
          <span class="op-flow-v">{topSummary.total}</span>
          <span class="op-flow-hint">total leased</span>
        </div>
      </section>

      <header class="op-section-head">
        <h1 class="op-section-title">RUNNING LANES</h1>
        <div class="op-section-meta">
          <details class="op-time-filter">
            <summary class="op-time-filter-toggle">
              <span class="op-time-filter-icon">◷</span>
              <span class="op-time-filter-label">{days === 0 ? "全部时间" : `近 ${days} 天`}</span>
            </summary>
            <div class="op-time-filter-menu">
              <a href={sessionsDaysPath(1)} class={days === 1 ? "op-time-filter-option active" : "op-time-filter-option"}>近 1 天</a>
              <a href={sessionsDaysPath(3)} class={days === 3 ? "op-time-filter-option active" : "op-time-filter-option"}>近 3 天</a>
              <a href={sessionsDaysPath(7)} class={days === 7 ? "op-time-filter-option active" : "op-time-filter-option"}>近 7 天</a>
              <a href={sessionsDaysPath(14)} class={days === 14 ? "op-time-filter-option active" : "op-time-filter-option"}>近 14 天</a>
              <a href={sessionsDaysPath(30)} class={days === 30 ? "op-time-filter-option active" : "op-time-filter-option"}>近 30 天</a>
              <a href={sessionsDaysPath(0)} class={days === 0 ? "op-time-filter-option active" : "op-time-filter-option"}>全部时间</a>
            </div>
          </details>
          <span class="op-section-meta-item">{topSummary.running} RUNNING · {topSummary.total} LEASED</span>
          <span class="op-section-meta-item muted">via {sourceText}</span>
        </div>
      </header>

      {top.length === 0 ? (
        <div class="op-empty">
          <p>No Pi sessions found.</p>
          <p class="muted small">No sessions in the selected time range. Try a wider range or <a href={sessionsDaysPath(0)}>view all</a>.</p>
          <p class="muted small">
            Start one with <code>pi</code> in any project, or ensure <code>~/.pi/agent/sessions</code> is readable.
          </p>
          <p class="muted small">
            Useful commands: <code>pi -c</code>, <code>pi -r</code>, <code>pi --session &lt;id&gt;</code>.
          </p>
        </div>
      ) : (
        <div class="op-lanes">
          {top.map((s, i) => <SessionLane session={s} index={i} total={top.length} childSessions={childrenByParent.get(s.id)} />)}
        </div>
      )}

      <section class="op-hints">
        <h2 class="op-hints-title">PI SESSION COMMANDS</h2>
        <ul class="op-hints-list">
          <li><code>pi -c</code> — continue the most recent session.</li>
          <li><code>pi -r</code> — browse and resume previous sessions.</li>
          <li><code>pi --session &lt;id&gt;</code> — resume a specific Pi session.</li>
        </ul>
        <p class="op-hints-note muted small">
          The dashboard's primary resume path is the embedded terminal — these commands are provided as a CLI fallback.
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

const ReportListPage: FC<{ reports: (Awaited<ReturnType<typeof scanReports>>[number] & { confirmedCount?: number; rejectedCount?: number })[] }> = ({ reports }) => (
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
          <a class={`report-card${r.confirmedCount ? " report-card-confirmed" : ""}`} href={`/report?path=${encodeURIComponent(r.reportPath)}`}>
            <div class="report-card-header">
              <span class="report-session">{r.session}</span>
              <span class="report-date">{r.generated || "unknown date"}</span>
            </div>
            <div class="report-card-body">
              <span class="stat stat-high">{r.highCount} 高</span>
              <span class="stat stat-medium">{r.mediumCount} 中</span>
              <span class="stat stat-total">{r.candidateCount} total</span>
              {r.confirmedCount ? <span class="stat stat-confirmed">✓ {r.confirmedCount} confirmed</span> : null}
              {r.rejectedCount ? <span class="stat stat-rejected">✗ {r.rejectedCount} rejected</span> : null}
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

const CandidateCard: FC<{ c: Candidate; confirmed?: boolean }> = ({ c, confirmed }) => (
  <div class={`candidate-card${confirmed ? " checked" : ""}`} data-cid={c.id}>
    <div class="candidate-header">
      <label class="candidate-check">
        <input type="checkbox" data-cid={c.id} checked={confirmed ?? false} />
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

const ReportDetailPage: FC<{ report: ParsedReport; reportPath: string; confirmation: ConfirmationStatus }> = ({ report, reportPath, confirmation }) => {
  const confirmedSet = new Set(confirmation.confirmedIds)
  return (
  <Layout title={`Report — ${report.meta.session}`} active="reports">
    <div class="page-header">
      <a href="/reports" class="back-link">← Back to reports</a>
      <h1>{report.meta.session || "Session"}</h1>
      <div class="meta-grid">
        {report.meta.scope && <div><span class="field-label">Scope</span> {report.meta.scope}</div>}
        {report.meta.generated && <div><span class="field-label">Generated</span> {report.meta.generated}</div>}
        {report.meta.artifact && <div><span class="field-label">Artifact</span> <code>{report.meta.artifact}</code></div>}
        {confirmation.confirmedIds.length > 0 && <div><span class="field-label">Confirmed</span> <span class="stat stat-confirmed">{confirmation.confirmedIds.length} candidate(s)</span></div>}
        {confirmation.rejectedIds.length > 0 && <div><span class="field-label">Rejected</span> <span class="stat stat-rejected">{confirmation.rejectedIds.length} candidate(s)</span></div>}
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
            .map((c) => <CandidateCard c={c} confirmed={confirmedSet.has(c.id)} />)}
        </div>

        {report.candidates.some((c) => c.category === "interaction") && (
          <>
            <h2 class="section-title">主/子 Agent 互动优化</h2>
            <div class="candidate-list">
              {report.candidates
                .filter((c) => c.category === "interaction")
                .map((c) => <CandidateCard c={c} confirmed={confirmedSet.has(c.id)} />)}
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

    <script>{`window.__REPORT_PATH__ = ${JSON.stringify(reportPath)}; window.__CONFIRMED_IDS__ = ${JSON.stringify(confirmation.confirmedIds)}; window.__REJECTED_IDS__ = ${JSON.stringify(confirmation.rejectedIds)};`}</script>
  </Layout>
  )
}

// ---------------------------------------------------------------------------
// Session detail (embedded terminal) page
// ---------------------------------------------------------------------------

const SessionTerminalPage: FC<{ session: SessionInfo; req?: Requirement | null; reqContext?: string; createNew?: boolean; harness: DashboardHarness }> = ({ session, req, reqContext, createNew, harness }) => {
  const updatedText = formatUpdated(session.updated || session.created)
  const worktree = session.worktree || "none"
  const model = modelDisplay(session)
  const reqId = req ? req.id : ""
  const ctx = reqContext ?? ""
  const descSnippet = req && req.description ? req.description.slice(0, 200) : ""
  const isNew = createNew === true
  const initJs = `window.__REQ_ID__ = ${JSON.stringify(reqId)}; window.__REQ_CONTEXT__ = ${JSON.stringify(ctx)}; window.__CREATE_NEW__ = ${JSON.stringify(isNew)}; window.__HARNESS__ = ${JSON.stringify(harness)};`
  const resumeCmd = buildResumeCommand(harness, session.id || "<id>")
  const terminalTitle = isNew
    ? (harness === "pi"
      ? (req ? `pi --name ${JSON.stringify(req.title).slice(1, -1)}` : "pi (new session)")
      : (req ? `opencode run -i --title ${JSON.stringify(req.title).slice(1, -1)}` : "opencode run -i"))
    : resumeCmd
  return (
    <Layout title={`Session ${session.id || "new"}`} active="sessions">
      <div class="page-header session-detail-header">
        <a href={SESSIONS_PATH} class="back-link">← All sessions</a>
        <h1 class="mono">{session.title || session.id || "New session"}</h1>
        <div class="meta-grid">
          <div><span class="field-label">Session</span> <code>{session.id || (isNew ? "(pending — opencode 创建中)" : "—")}</code></div>
          <div><span class="field-label">Status</span> <span class={`status-pill status-${session.status}`}>{statusLabel(session.status)}</span></div>
          <div><span class="field-label">Project</span> {session.projectId || "global"}</div>
          <div><span class="field-label">Agent</span> {session.agent || "—"}</div>
          <div><span class="field-label">Model</span> <code>{model}</code></div>
          <div><span class="field-label">Worktree</span> <code>{worktree}</code></div>
          <div><span class="field-label">Updated</span> {updatedText}</div>
          {session.directory ? <div><span class="field-label">Cwd</span> <code>{session.directory}</code></div> : null}
          <div><span class="field-label">Source</span> {sourceLabel(session.source)}</div>
          {req ? <div><span class="field-label">Requirement</span> <a href={`/requirement?id=${encodeURIComponent(req.id)}`}>{req.title}</a></div> : null}
        </div>
      </div>

      {req ? (
        <details class="req-context-panel" open>
          <summary>需求上下文 — {req.title} <span class={`req-status-badge req-status-${req.status}`}>{req.status}</span></summary>
          <div class="req-context-panel-body">
            {descSnippet ? <div><strong>描述：</strong><pre>{descSnippet}</pre></div> : null}
            <button id="inject-req-btn" type="button" class="btn btn-secondary">注入需求上下文</button>
          </div>
        </details>
      ) : null}

      <div class="terminal-wrap">
        <div class="terminal-header">
          <div class="terminal-header-left">
            <span class="dot dot-red" />
            <span class="dot dot-yellow" />
            <span class="dot dot-green" />
            <span class="terminal-title mono">{terminalTitle}</span>
          </div>
          <div class="terminal-header-right muted small">
            <span>WebSocket: /ws/session-terminal</span>
          </div>
        </div>
        <div class="terminal-host-shell">
          <div id="terminal" class="terminal-host" data-session-id={session.id} data-req-id={reqId} />
        </div>
        <div id="terminal-status" class="terminal-status muted small">connecting…</div>
      </div>

      <section class="hints-section">
        <h2>{harness === "pi" ? "Pi CLI hints" : "OpenCode CLI hints"}</h2>
        {harness === "pi" ? (
          <ul class="hints-list">
            <li><code>pi --session {session.id || "<id>"}</code> - 在你自己的终端里继续这个 session。</li>
            <li><code>pi --fork {session.id || "<id>"}</code> - 从当前 session 复制出一个新 session。</li>
            <li><code>pi -r</code> - 浏览选择历史 session 恢复；<code>pi -c</code> 继续最近一次。</li>
          </ul>
        ) : (
          <ul class="hints-list">
            <li><code>opencode web</code> - start OpenCode's own web interface in your browser.</li>
            <li><code>opencode serve --port 4096</code> - start a headless server, then attach with:</li>
            <li><code>opencode attach http://localhost:4096 --session {session.id || "<id>"}</code></li>
          </ul>
        )}
        <p class="muted small">
          This page runs an embedded <code>node-pty</code> terminal locally; it is independent of any
          remote <code>opencode serve</code> process.
        </p>
      </section>

      <script>{initJs}</script>
      <script
        type="module"
        src="/static/terminal.js"
        data-session-id={session.id}
        data-req-id={reqId}
        data-create-new={isNew ? "1" : ""}
      />
    </Layout>
  )
}

const SessionMissingPage: FC<{ id: string; backReqId?: string }> = ({ id, backReqId }) => (
  <Layout title={`Session ${id} not found`} active="sessions">
    <div class="page-header">
      <a href={SESSIONS_PATH} class="back-link">← All sessions</a>
      <h1>Session not available</h1>
      <p class="muted">
        <code>{id || "(empty)"}</code> 在 OpenCode 数据库里找不到。可能已归档，或这是一个曾经记在需求里、但 OpenCode 从未真正创建过的"幽灵 id"。
      </p>
      {backReqId ? (
        <p>
          <a class="btn btn-primary" href={`/requirement?id=${encodeURIComponent(backReqId)}`}>
            返回需求页面选择「新建」或「关联已有 session」 →
          </a>
        </p>
      ) : null}
    </div>
  </Layout>
)

// ---------------------------------------------------------------------------
// Requirement pages
// ---------------------------------------------------------------------------

const REQ_STATUS_SLUG: Record<ReqStatus, string> = {
  "需求对齐": "align",
  "方案设计": "design",
  "开发中": "dev",
  "自测中": "selftest",
  "测试中": "testing",
  "待上线": "deploy",
  "已完成": "done",
}

function reqStatusBadgeClass(status: ReqStatus): string {
  return `req-status-badge req-status-${REQ_STATUS_SLUG[status]}`
}

/** Flat requirement card with dedicated review, detail, and release actions. */
const RequirementBoardCard: FC<{ item: RequirementBoardItem }> = ({ item }) => {
  const r = item.requirement
  const snippet = (r.description || "").trim().slice(0, 180) || "暂无描述"
  const canOpenTools = !!r.reqDir && r.id !== DEFAULT_REQ_ID
  const reviewHref = `/requirement/review?id=${encodeURIComponent(r.id)}`
  const detailHref = `/requirement?id=${encodeURIComponent(r.id)}`
  const releaseHref = `/requirement/release?id=${encodeURIComponent(r.id)}`
  return (
    <article class="req-board-card">
      <div class="req-board-card-main">
        <div class="req-board-card-head">
          <div class="req-board-card-title-wrap">
            <span class="req-board-card-id">{r.id}</span>
            <h2 class="req-board-card-title">{r.title}</h2>
          </div>
          <div class="req-board-card-badges">
            {r.category === "线上问题" ? <span class="req-category-badge req-category-incident">线上问题</span> : null}
            <span class={reqStatusBadgeClass(r.status)}>{r.status}</span>
          </div>
        </div>
        <div class="req-board-card-path">{item.hierarchy || DEFAULT_PROJECT_NAME}</div>
        <p class="req-board-card-description">{snippet}</p>
        <div class="req-board-card-meta">
          <span>需求</span>
          <span>创建 {new Date(r.createdAt).toLocaleDateString("zh-CN")}</span>
          <span>{r.sessionIds.length} session(s)</span>
          <span>更新于 {formatRelAgo(r.updatedAt)}</span>
        </div>
      </div>
      <div class="req-board-card-actions" aria-label={`${r.title} 操作`}>
        {canOpenTools ? (
          <a class="req-board-action req-board-action-review" href={reviewHref} title="查看 AI 相比生产分支的代码改动">代码差异</a>
        ) : (
          <span class="req-board-action req-board-action-disabled" title="该需求没有可读取的本地需求目录">代码差异</span>
        )}
        <a class="req-board-action req-board-action-detail" href={detailHref}>需求详情</a>
        {canOpenTools ? (
          <a class="req-board-action req-board-action-release" href={releaseHref}>发版注意</a>
        ) : (
          <span class="req-board-action req-board-action-disabled" title="该需求没有可读取的本地需求目录">发版注意</span>
        )}
      </div>
    </article>
  )
}

/**
 * Search-as-you-type session picker built on the native HTML `<datalist>`
 * element. Each option's `value` is "ses_xxx — <title>" so the browser's
 * built-in matching works against both the id prefix and any fragment
 * of the title. The server-side handler extracts the `ses_...` portion
 * from whatever value the user submits.
 */
const SessionPicker: FC<{ candidates: SessionInfo[]; listId: string; placeholder?: string }> = ({ candidates, listId, placeholder }) => {
  return (
    <>
      <input
        type="text"
        name="sessionId"
        list={listId}
        autocomplete="off"
        spellcheck={false}
        placeholder={placeholder ?? "输入 ses_ 前缀或标题片段筛选…"}
        required
      />
      <datalist id={listId}>
        {candidates.map((s) => {
          const title = (s.title || "(untitled)").replace(/\s+/g, " ").trim()
          const label = `${s.id} — ${title}`
          return <option value={label} />
        })}
      </datalist>
    </>
  )
}

const ProjectsPage: FC<{
  items: RequirementBoardItem[]
  counts: Record<ReqStatus, number>
  selectedStatuses: ReqStatus[]
  projectFilter: string
  subprojectFilter: string
  createdFrom: string
  createdTo: string
  projectOptions: string[]
  subprojectOptions: string[]
  categoryFilter: string
  keyword: string
}> = ({ items, counts, selectedStatuses, projectFilter, subprojectFilter, createdFrom, createdTo, projectOptions, subprojectOptions, categoryFilter, keyword }) => (
  <Layout title="需求进度看板" active="requirements">
    <section class="req-board-filter-panel" aria-label="需求筛选">
      <div class="req-board-filter-heading">
        <div>
          <span class="op-section-title">需求筛选</span>
          <p class="muted small">按关键词、类别、创建时间、需求状态和所属项目组合筛选。状态未选择时默认隐藏已完成需求。</p>
        </div>
        <a class="req-filter-clear" href="/projects">清空筛选</a>
      </div>
      <form method="get" action="/projects" class="req-board-filter-form" id="req-board-filter-form">
        <label class="req-board-filter-field req-board-filter-field-grow">
          <span>关键词搜索</span>
          <input type="search" name="q" value={keyword} placeholder="搜索需求 ID、标题、描述或项目路径…" autocomplete="off" />
        </label>
        <label class="req-board-filter-field">
          <span>类别</span>
          <select name="category" id="req-board-category-filter">
            <option value="" selected={!categoryFilter}>全部类别</option>
            {REQ_CATEGORIES.map((cat) => <option value={cat} selected={cat === categoryFilter}>{cat}</option>)}
          </select>
        </label>
        <label class="req-board-filter-field">
          <span>创建时间起</span>
          <input type="date" name="createdFrom" value={createdFrom} />
        </label>
        <label class="req-board-filter-field">
          <span>创建时间止</span>
          <input type="date" name="createdTo" value={createdTo} />
        </label>
        <label class="req-board-filter-field">
          <span>一级项目</span>
          <select name="project" id="req-board-project-filter">
            <option value="">全部项目</option>
            {projectOptions.map((project) => <option value={project} selected={project === projectFilter}>{project}</option>)}
          </select>
        </label>
        <label class="req-board-filter-field">
          <span>二级项目</span>
          <select name="subproject" id="req-board-subproject-filter" disabled={!projectFilter}>
            <option value="">{projectFilter ? "全部二级项目" : "请先选择一级项目"}</option>
            {subprojectOptions.map((subproject) => <option value={subproject} selected={subproject === subprojectFilter}>{subproject}</option>)}
          </select>
        </label>
        <fieldset class="req-board-status-filter">
          <legend>需求状态（支持多选）</legend>
          <div class="req-board-status-options">
            {REQ_STATUSES.map((status) => (
              <label class={`req-board-status-option req-board-status-${REQ_STATUS_SLUG[status]}`}>
                <input type="checkbox" name="status" value={status} checked={selectedStatuses.includes(status)} />
                <span>{status}</span>
                <strong>{counts[status]}</strong>
              </label>
            ))}
          </div>
        </fieldset>
        <div class="req-board-filter-actions">
          <button type="submit" class="btn btn-primary">应用筛选</button>
          <a class="btn btn-secondary" href="/projects">重置</a>
        </div>
      </form>
    </section>

    <header class="op-section-head req-board-section-head">
      <div>
        <h1 class="op-section-title">当前需求进度</h1>
        <p class="muted small">父需求、子需求和普通需求统一平铺，按最近更新时间排序。</p>
      </div>
      <div class="op-section-meta"><span class="op-section-meta-item">{items.length} TRACKED</span></div>
    </header>

    {items.length === 0 ? (
      <div class="op-empty">
        <p>没有符合当前筛选条件的需求。</p>
        <p><a href="/projects">清空筛选并查看当前需求</a></p>
      </div>
    ) : (
      <div class="req-board-list">
        {items.map((item) => <RequirementBoardCard item={item} />)}
      </div>
    )}
    <script src="/static/requirements-board.js" defer></script>
  </Layout>
)

const HermesFileSection: FC<{ title: string; content?: string }> = ({ title, content }) => {
  if (!content) return null
  return (
    <section class="req-hermes-section">
      <h2 class="op-section-title">{title}</h2>
      <pre style="white-space: pre-wrap; max-height: 320px; overflow: auto; padding: 10px; border: 1px solid var(--op-border, #2a2a2a); border-radius: 4px; background: var(--op-bg-soft, #181818);">{content}</pre>
    </section>
  )
}

const ACTIVE_STATUSES: ReqStatus[] = ["开发中", "自测中", "测试中"]

function pickMostRecentSession(sessions: SessionInfo[]): SessionInfo | null {
  if (sessions.length === 0) return null
  return [...sessions].sort((a, b) => (b.updated || b.created || 0) - (a.updated || a.created || 0))[0]
}

function sortByLastUsedDesc(sessions: SessionInfo[]): SessionInfo[] {
  return [...sessions].sort((a, b) => (b.updated || b.created || 0) - (a.updated || a.created || 0))
}

/**
 * Release checklist card — shown only when status = "待上线".
 * Displays 4 sections parsed from the Hermes context files:
 * 涉及应用, 涉及分支, 数据库变更, Apollo/Nacos 配置变更.
 */
const ReleaseChecklistCard: FC<{ checklist: ReleaseChecklist }> = ({ checklist }) => {
  const hasData =
    checklist.applications.length > 0 ||
    checklist.branches.length > 0 ||
    checklist.dbChanges.length > 0 ||
    checklist.configChanges.length > 0 ||
    checklist.mqResources.length > 0 ||
    checklist.verificationChains.length > 0 ||
    checklist.reviewItems.length > 0 ||
    checklist.releaseNotes.length > 0
  return (
    <section class="release-checklist" aria-label="上线检查">
      <h2 class="op-section-title">📋 上线检查清单</h2>
      {!hasData ? (
        <p class="muted small">尚未从上下文文件中提取到上线信息。请先运行「智能提取」补充 branch.md / config-changes.md / test.md / review.md。</p>
      ) : (
        <div class="release-checklist-grid">
          {checklist.applications.length > 0 ? (
            <div class="release-checklist-section">
              <h3>涉及应用</h3>
              <ul>{checklist.applications.map((a) => <li><code>{a}</code></li>)}</ul>
            </div>
          ) : null}
          {checklist.branches.length > 0 ? (
            <div class="release-checklist-section">
              <h3>涉及分支</h3>
              <table class="release-checklist-table">
                <tbody>
                  {checklist.branches.map((b) => (
                    <tr><td class="muted small">{b.label}</td><td><code>{b.value}</code></td></tr>
                  ))}
                </tbody>
              </table>
            </div>
          ) : null}
          {checklist.dbChanges.length > 0 ? (
            <div class="release-checklist-section">
              <h3>数据库变更</h3>
              <ul>{checklist.dbChanges.map((d) => <li><code>{d}</code></li>)}</ul>
            </div>
          ) : null}
          {checklist.configChanges.length > 0 ? (
            <div class="release-checklist-section">
              <h3>Apollo / Nacos 配置变更</h3>
              <ul>{checklist.configChanges.map((c) => <li><code>{c}</code></li>)}</ul>
            </div>
          ) : null}
          {checklist.mqResources.length > 0 ? (
            <div class="release-checklist-section">
              <h3>Topic / Group / 云资源</h3>
              <ul>{checklist.mqResources.map((c) => <li><code>{c}</code></li>)}</ul>
            </div>
          ) : null}
          {checklist.verificationChains.length > 0 ? (
            <div class="release-checklist-section">
              <h3>上线前复验链路</h3>
              <ul>{checklist.verificationChains.map((c) => <li>{c}</li>)}</ul>
            </div>
          ) : null}
          {checklist.reviewItems.length > 0 ? (
            <div class="release-checklist-section">
              <h3>Code Review 结论</h3>
              <ul>{checklist.reviewItems.map((c) => <li>{c}</li>)}</ul>
            </div>
          ) : null}
          {checklist.releaseNotes.length > 0 ? (
            <div class="release-checklist-section release-checklist-notes">
              <h3>上线注意事项</h3>
              <ul>{checklist.releaseNotes.map((n) => <li>{n}</li>)}</ul>
            </div>
          ) : null}
        </div>
      )}
    </section>
  )
}

/**
 * Structured "代码改动范围" overview card - which repos the agent touched
 * and which feature branches it created in each. Sourced from
 * `branches.json` (authoritative) or a best-effort parse of `branch.md`
 * (flagged `scope.fallback`). Rendered for every non-parent requirement
 * with branch data, not only "待上线", so the change blast radius is
 * visible during dev. `branch.md` raw text stays available below as a
 * collapsed "完整分支记录".
 */
const BranchScopeCard: FC<{ req: Requirement; scope: BranchScope }> = ({ req, scope }) => {
  const repoCount = scope.repos.length
  const branchCount = scope.repos.reduce((n, r) => n + r.branches.length, 0)
  return (
    <section class="branch-scope" aria-label="代码改动范围">
      <div class="branch-scope-head">
        <h2 class="op-section-title">🗂 代码改动范围</h2>
        <span class="branch-scope-summary muted small">{repoCount} 仓库 · {branchCount} 分支</span>
        {req.reqDir ? (
          <form method="post" action="/api/requirement/generate-branch-scope" class="req-extract-trigger-form" data-extract-trigger="">
            <input type="hidden" name="reqId" value={req.id} />
            <button type="submit" class="btn btn-secondary branch-scope-gen-btn" title="让后台 agent 读取 branch.md 生成精确的 branches.json">
              {scope.fallback ? "🤖 生成 branches.json" : "🔄 重新生成"}
            </button>
          </form>
        ) : null}
      </div>
      {scope.fallback ? (
        <p class="branch-scope-warn muted small">自动从 branch.md 提取，可能不精确。生成 <code>branches.json</code> 可获得精确的应用↔分支映射。</p>
      ) : null}
      <div class="branch-scope-list">
        {scope.repos.map((r) => (
          <div class="branch-scope-repo">
            <div class="branch-scope-repo-head">
              <code class="branch-scope-repo-name">{r.repoName || "未关联仓库"}</code>
              {r.role ? <span class="branch-scope-role">{r.role}</span> : null}
            </div>
            {r.path ? <div class="branch-scope-path muted small">{r.path}</div> : null}
            {r.branches.length > 0 ? (
              <ul class="branch-scope-branches">
                {r.branches.map((b) => <li><code>{b}</code></li>)}
              </ul>
            ) : (
              <span class="muted small branch-scope-no-branch">无需求分支</span>
            )}
          </div>
        ))}
      </div>
    </section>
  )
}

const CODE_REVIEW_STATUS_LABELS: Record<CodeReviewStatus, string> = {
  not_started: "未开始",
  approved: "通过",
  changes_requested: "需修改",
  blocked: "阻塞",
}

interface CodeReviewFileView {
  key: string
  repoIndex: number
  fileIndex: number
  repoName: string
  projectPath?: string
  branch: string
  baseRef: string
  status: string
  path: string
  additions: number
  deletions: number
  riskTags: string[]
  diff?: CodeReviewFileDiff
}

const codeReviewFileName = (path: string): string => path.split("/").pop() || path
const codeReviewFileDir = (path: string): string => path.includes("/") ? path.slice(0, path.lastIndexOf("/")) : "."

const CodeReviewDiffRow: FC<{ line: CodeReviewDiffLine; innerHtml: string }> = ({ line, innerHtml }) => {
  const sign = line.kind === "addition" ? "+" : line.kind === "deletion" ? "−" : line.kind === "meta" ? "·" : ""
  return (
    <tr class={`code-review-line code-review-line-${line.kind}`}>
      <td class="code-review-line-no">{line.oldLine ?? ""}</td>
      <td class="code-review-line-no">{line.newLine ?? ""}</td>
      <td class="code-review-line-sign">{sign}</td>
      {/* innerHtml is pre-built by highlightDiffLines: highlight.js escapes
          every text fragment, so it is safe to inject raw here. */}
      <td class="code-review-line-code"><code>{innerHtml}</code></td>
    </tr>
  )
}

const CodeReviewFilePanel: FC<{ file: CodeReviewFileView; selected: boolean }> = ({ file, selected }) => {
  // One language per file; hunks share it. Determined by extension so short
  // diff snippets are never mis-detected by auto-guessing.
  const lang = detectHighlightLanguage(file.path)
  return (
  <article class="code-review-file-panel" data-review-file-panel={file.key} hidden={!selected}>
    <header class="code-review-file-head">
      <div class="code-review-file-title">
        <span class={`code-review-file-status code-review-file-status-${file.status.charAt(0).toLowerCase()}`}>{file.status}</span>
        <div>
          <strong>{file.path}</strong>
          <span>{file.repoName} · <code>{file.branch}</code> → <code>{file.baseRef}</code></span>
        </div>
      </div>
      <div class="code-review-file-metrics">
        <span class="code-review-additions">+{file.additions}</span>
        <span class="code-review-deletions">-{file.deletions}</span>
        {file.riskTags.map((tag) => <span class="code-review-risk-tag">{tag}</span>)}
      </div>
    </header>
    {file.diff && file.diff.hunks.length > 0 ? (
      <div class="code-review-hunks">
        {file.diff.hunks.map((hunk) => {
          // Tokenize the whole hunk at once so multi-line comments and
          // text blocks stay colored across line boundaries.
          const highlighted = highlightDiffLines(hunk.lines, lang)
          return (
          <section class="code-review-hunk">
            <div class="code-review-hunk-head"><code>{hunk.header}</code></div>
            <div class="code-review-table-wrap">
              <table class="code-review-table"><tbody>{hunk.lines.map((line, i) => <CodeReviewDiffRow line={line} innerHtml={highlighted[i]} />)}</tbody></table>
            </div>
          </section>
          )
        })}
      </div>
    ) : (
      <div class="code-review-no-diff">
        <strong>当前快照没有该文件的逐行 Diff。</strong>
        <span>可能是二进制文件、Diff 被截断，或扫描结果仅包含文件统计。可刷新 PRO Diff 后重试。</span>
      </div>
    )}
  </article>
  )
}

const CodeReviewWorkspace: FC<{ req: Requirement; scope?: BranchScope | null; snapshot?: CodeReviewSnapshot | null; redirectPath: string }> = ({ req, scope, snapshot, redirectPath }) => {
  const verdict = snapshot?.verdict
  const repoViews = (snapshot?.repos || []).map((repo, repoIndex) => {
    const parsed = parseUnifiedDiff(repo.diff)
    const byPath = new Map<string, CodeReviewFileDiff>()
    for (const file of parsed) {
      byPath.set(file.path, file)
      byPath.set(file.oldPath, file)
      byPath.set(file.newPath, file)
    }
    return {
      repo,
      files: repo.files.map((file, fileIndex): CodeReviewFileView => ({
        key: `review-file-${repoIndex}-${fileIndex}`,
        repoIndex,
        fileIndex,
        repoName: repo.repoName,
        projectPath: repo.projectPath,
        branch: repo.branch,
        baseRef: repo.baseRef,
        status: file.status,
        path: file.path,
        additions: file.additions,
        deletions: file.deletions,
        riskTags: file.riskTags,
        diff: byPath.get(file.path),
      })),
    }
  })
  const allFiles = repoViews.flatMap((view) => view.files)
  const files = allFiles.length
  const additions = snapshot?.repos.reduce((n, repo) => n + repo.additions, 0) ?? 0
  const deletions = snapshot?.repos.reduce((n, repo) => n + repo.deletions, 0) ?? 0
  const firstFile = allFiles[0]

  return (
    <section class="code-review-workspace" aria-label="代码差异审查工作区">
      <header class="code-review-toolbar">
        <div class="code-review-toolbar-copy">
          <div class="code-review-toolbar-title">
            <strong>生产分支差异</strong>
            <span class={`code-review-status code-review-status-${verdict?.status || "not_started"}`}>{CODE_REVIEW_STATUS_LABELS[verdict?.status || "not_started"]}</span>
          </div>
          <span>{snapshot ? `${snapshot.repos.length} 个仓库/分支 · ${files} 个文件 · 更新于 ${formatUpdated(snapshot.updatedAt)}` : "尚未生成代码差异快照"}</span>
        </div>
        <form method="post" action="/api/requirement/code-review/scan" class="code-review-scan-form">
          <input type="hidden" name="reqId" value={req.id} />
          <input type="hidden" name="redirect" value={redirectPath} />
          <label class="field-label" for={`code-review-base-${req.id}`}>生产基线</label>
          <input id={`code-review-base-${req.id}`} name="baseRef" value={snapshot?.baseRef || DEFAULT_CODE_REVIEW_BASE_REF} placeholder="origin/master" />
          <button type="submit" class="btn btn-primary" disabled={!scope || !req.reqDir}>刷新 PRO Diff</button>
        </form>
        <button type="button" id="branch-scope-ai-btn" class="btn btn-secondary" data-req-id={req.id} title="调用 AI 读取 branch.md 生成精确的 branches.json">🤖 提取 branches.json</button>
        <span id="branch-scope-ai-status" class="branch-scope-ai-status muted small"></span>
        <div class="code-review-total-metrics">
          <span>{files} files</span>
          <strong class="code-review-additions">+{additions}</strong>
          <strong class="code-review-deletions">-{deletions}</strong>
        </div>
      </header>

      {scope?.fallback ? <div class="code-review-banner is-warn">改动范围来自 branch.md 兜底解析，建议补充 branches.json 以准确标识项目和分支。</div> : null}
      {!scope ? <div class="code-review-banner is-warn">需要先补充分支范围（branches.json 或 branch.md），才能生成生产分支差异。</div> : null}

      {snapshot && allFiles.length > 0 ? (
        <div class="code-review-layout">
          <aside class="code-review-file-pane">
            <div class="code-review-pane-head">
              <strong>改动文件</strong>
              <span>{files}</span>
            </div>
            <label class="code-review-file-search">
              <span aria-hidden="true">⌕</span>
              <input id="code-review-file-search" type="search" placeholder="搜索文件或项目" autocomplete="off" />
            </label>
            <div class="code-review-file-groups">
              {repoViews.map(({ repo, files: repoFiles }) => (
                <section class="code-review-file-group" data-review-file-group>
                  <header>
                    <strong>{req.project} / {repo.repoName}</strong>
                    <span>{repoFiles.length}</span>
                    <small><code>{repo.branch}</code></small>
                    {repo.projectPath ? <small title={repo.projectPath}>{repo.projectPath}</small> : null}
                  </header>
                  <div>
                    {repoFiles.map((file, index) => (
                      <button
                        type="button"
                        class={`code-review-file-button${file.key === firstFile?.key ? " is-active" : ""}`}
                        data-review-file-button={file.key}
                        data-review-file-filter={`${req.project} ${file.repoName} ${file.path}`.toLowerCase()}
                        data-review-file-path={file.path}
                        data-review-file-repo={`${req.project} / ${file.repoName}`}
                        aria-pressed={file.key === firstFile?.key ? "true" : "false"}
                      >
                        <span class={`code-review-file-status code-review-file-status-${file.status.charAt(0).toLowerCase()}`}>{file.status}</span>
                        <span class="code-review-file-button-copy">
                          <strong>{codeReviewFileName(file.path)}</strong>
                          <small>{codeReviewFileDir(file.path)}</small>
                        </span>
                        <span class="code-review-file-button-metrics"><b>+{file.additions}</b><i>-{file.deletions}</i></span>
                      </button>
                    ))}
                  </div>
                </section>
              ))}
              <div id="code-review-file-empty" class="code-review-file-empty" hidden>没有匹配的改动文件。</div>
            </div>
          </aside>

          <main class="code-review-diff-pane">
            <div class="code-review-pane-head code-review-diff-pane-head">
              <div class="code-review-diff-file-info">
                <strong id="code-review-current-file">{firstFile?.path}</strong>
                <span id="code-review-current-repo">{firstFile ? `${req.project} / ${firstFile.repoName}` : ""}</span>
              </div>
              <div class="code-review-diff-head-right">
                <span>与 {snapshot.baseRef} 对比</span>
                <div class="code-review-fontsize" role="group" aria-label="代码字体大小">
                  <button type="button" class="code-review-fontsize-btn" data-fontsize="down" aria-label="缩小代码字体" title="缩小字体">A−</button>
                  <span class="code-review-fontsize-label" id="code-review-fontsize-label">100%</span>
                  <button type="button" class="code-review-fontsize-btn" data-fontsize="up" aria-label="放大代码字体" title="放大字体">A+</button>
                  <button type="button" class="code-review-fontsize-btn code-review-fontsize-reset" data-fontsize="reset" aria-label="恢复默认字体大小" title="恢复默认">↺</button>
                </div>
              </div>
            </div>
            <div class="code-review-file-panels">{allFiles.map((file) => <CodeReviewFilePanel file={file} selected={file.key === firstFile?.key} />)}</div>
          </main>

          <aside class="code-review-notes-pane">
            <div class="code-review-pane-head">
              <strong>代码备注</strong>
              <span>{CODE_REVIEW_STATUS_LABELS[verdict?.status || "not_started"]}</span>
            </div>
            <div class="code-review-current-context">
              <span>当前文件</span>
              <strong id="code-review-note-file">{firstFile?.path}</strong>
              <small id="code-review-note-repo">{firstFile ? `${req.project} / ${firstFile.repoName}` : ""}</small>
            </div>
            <form method="post" action="/api/requirement/code-review/verdict" class="code-review-verdict-form">
              <input type="hidden" name="reqId" value={req.id} />
              <input type="hidden" name="redirect" value={redirectPath} />
              <label><span class="field-label">结论</span><select name="status">{CODE_REVIEW_STATUSES.map((status) => <option value={status} selected={(verdict?.status || "not_started") === status}>{CODE_REVIEW_STATUS_LABELS[status]}</option>)}</select></label>
              <label><span class="field-label">Reviewer</span><input name="reviewer" value={verdict?.reviewer || ""} placeholder="你的名字" /></label>
              <label><span class="field-label">Review 摘要</span><textarea name="summary" rows={"5"} placeholder="记录整体实现、风险和结论">{verdict?.summary || ""}</textarea></label>
              <label><span class="field-label">待修复 / 关注项</span><textarea name="items" rows={"10"} placeholder="每行一条，例如：空库存时需保留原分配结果">{verdict?.items.join("\n") || ""}</textarea></label>
              <button type="submit" class="btn btn-primary">保存代码备注</button>
              <span class="muted small">同步写入 code-review.json 和 review.md。</span>
            </form>
            <details class="code-review-context-details">
              <summary>扫描上下文与告警</summary>
              {snapshot.repos.map((repo) => (
                <div class="code-review-context-repo">
                  <strong>{repo.repoName}</strong>
                  <span class={repo.baseUpdate.ok ? "is-ok" : "is-warn"}>{repo.baseUpdate.ok ? "生产基线已刷新" : "生产基线异常"}</span>
                  {repo.dirty ? <small class="is-warn">工作区有未提交改动</small> : null}
                  {repo.error ? <small class="is-warn">{repo.error}</small> : null}
                  {repo.warnings.map((warning) => <small class="is-warn">{warning}</small>)}
                </div>
              ))}
            </details>
          </aside>
        </div>
      ) : (
        <div class="code-review-empty-state">
          <strong>{snapshot ? "本次扫描没有发现代码文件变更" : "尚未生成代码差异"}</strong>
          <span>点击「刷新 PRO Diff」会先更新本地生产基线，再按需求分支生成逐文件差异。</span>
        </div>
      )}

      <section class="code-review-ai-panel" aria-label="AI 代码审查">
        <header class="code-review-ai-head">
          <div class="code-review-ai-head-copy">
            <h3>AI 代码审查</h3>
            <p class="muted small">基于代码差异 + 需求文件让 AI 给出 review 建议；模型在 <a href="/settings">Settings</a> 配置。人工 review 后可直接点下方按钮让 AI 再 review 一次。</p>
          </div>
          <div class="code-review-ai-head-meta">
            {snapshot?.aiReview ? (
              <>
                <span class="code-review-ai-model"><code>{snapshot.aiReview.model || "未知模型"}</code></span>
                <span class="muted small">更新于 {formatUpdated(snapshot.aiReview.updatedAt)}</span>
              </>
            ) : null}
          </div>
        </header>
        <div class="code-review-ai-actions">
          <button type="button" id="code-review-ai-btn" class="btn btn-primary" data-req-id={req.id} disabled={!snapshot || allFiles.length === 0} title={!snapshot || allFiles.length === 0 ? "请先生成代码差异" : "让 AI 根据差异和需求文件审查代码"}>🤖 AI 审查代码</button>
          <span id="code-review-ai-status" class="code-review-ai-status muted small"></span>
        </div>
        <textarea id="code-review-ai-result" class="code-review-ai-result" rows={"16"} readonly placeholder="点击「AI 审查代码」后，AI 给出的 review 建议会显示在这里。" spellcheck={false}>{snapshot?.aiReview?.content || ""}</textarea>
        {snapshot?.aiReview?.error ? <div class="code-review-ai-error">上次审查失败：{snapshot.aiReview.error}</div> : null}
      </section>
    </section>
  )
}

/** Dedicated review surface opened from the first action on each board card. */
const RequirementReviewPage: FC<{
  req: Requirement
  scope?: BranchScope | null
  snapshot?: CodeReviewSnapshot | null
}> = ({ req, scope, snapshot }) => {
  const redirectPath = `/requirement/review?id=${encodeURIComponent(req.id)}`
  return (
    <Layout title={`代码差异 — ${req.title}`} active="requirements" wide>
      <section class="req-tool-page code-review-page">
        <header class="req-tool-page-head code-review-page-head">
          <div>
            <a class="back-link" href="/projects">← 返回需求看板</a>
            <h1>AI 相比生产分支的代码改动</h1>
            <p class="muted">{req.project} · {req.title} · <code>{req.id}</code></p>
          </div>
          <nav class="req-tool-page-actions">
            <a class="btn btn-secondary" href={`/requirement?id=${encodeURIComponent(req.id)}`}>需求详情</a>
            <a class="btn btn-secondary" href={`/requirement/release?id=${encodeURIComponent(req.id)}`}>发版注意</a>
          </nav>
        </header>
        <script>{`(function(){try{var v=parseFloat(localStorage.getItem('agent-panel:code-review:font-scale'));if(isFinite(v)){document.documentElement.style.setProperty('--code-review-font-scale',String(Math.min(2,Math.max(0.6,v))))}}catch(e){}})();`}</script>
        <CodeReviewWorkspace req={req} scope={scope} snapshot={snapshot} redirectPath={redirectPath} />
      </section>
      <script src="/static/req-detail.js?v=20260714-branch-scope-ai" defer></script>
    </Layout>
  )
}

/** Dedicated release-attention surface opened from the third board action. */
const RequirementReleasePage: FC<{
  req: Requirement
  checklist: ReleaseChecklist
  branchContent?: string
  configContent?: string
  testContent?: string
  notesContent?: string
  reviewContent?: string
}> = ({ req, checklist, branchContent, configContent, testContent, notesContent, reviewContent }) => (
  <Layout title={`发版注意 — ${req.title}`} active="requirements">
    <section class="req-tool-page">
      <header class="req-tool-page-head">
        <div>
          <a class="back-link" href="/">← 返回需求看板</a>
          <h1>当前发版需要注意的事项</h1>
          <p class="muted">{req.title} · <code>{req.id}</code> · <span class={reqStatusBadgeClass(req.status)}>{req.status}</span></p>
        </div>
        <nav class="req-tool-page-actions">
          <a class="btn btn-secondary" href={`/requirement/review?id=${encodeURIComponent(req.id)}`}>代码差异</a>
          <a class="btn btn-secondary" href={`/requirement?id=${encodeURIComponent(req.id)}`}>需求详情</a>
        </nav>
      </header>
      {req.status !== "待上线" ? (
        <div class="req-release-stage-note">当前需求尚未处于「待上线」，这里展示的是已沉淀的发版信息，可提前检查并补齐。</div>
      ) : null}
      <ReleaseChecklistCard checklist={checklist} />
      <details class="req-release-sources">
        <summary>查看发版依据文件</summary>
        <HermesFileSection title="分支记录" content={branchContent} />
        <HermesFileSection title="配置变更" content={configContent} />
        <HermesFileSection title="测试范围" content={testContent} />
        <HermesFileSection title="开发笔记" content={notesContent} />
        <HermesFileSection title="上线 Review" content={reviewContent} />
      </details>
    </section>
  </Layout>
)

const ImpactAssessmentCard: FC<{ req: Requirement; assessment: ImpactAssessment }> = ({ req, assessment }) => {
  const missingText = assessment.missingSections.length > 0
    ? assessment.missingSections.join("、")
    : "已覆盖全部必填项"
  const statusText = assessment.complete ? "已完成" : assessment.exists ? "待补齐" : "未创建"
  return (
    <section class={`impact-card${assessment.complete ? " impact-card-complete" : " impact-card-incomplete"}`} aria-label="需求影响面评估">
      <div class="impact-card-head">
        <div>
          <h2 class="op-section-title">需求影响面评估</h2>
          <p class="muted small">编码前安全门：确认不会阻塞 WMS 入库、库存、出库、复核、发运、回传等核心链路。</p>
        </div>
        <span class={`impact-status ${assessment.complete ? "impact-status-ok" : "impact-status-warn"}`}>{statusText}</span>
      </div>
      <div class="impact-grid">
        <div class="impact-metric"><span class="field-label">风险等级</span><strong>{assessment.riskLevel}</strong></div>
        <div class="impact-metric"><span class="field-label">缺失项</span><span>{missingText}</span></div>
      </div>
      <div class="impact-columns">
        <div class="impact-list-block">
          <h3>核心链路</h3>
          {assessment.coreFlows.length > 0 ? <ul>{assessment.coreFlows.map((x) => <li>{x}</li>)}</ul> : <p class="muted small">尚未识别核心链路。</p>}
        </div>
        <div class="impact-list-block">
          <h3>阻塞风险</h3>
          {assessment.blockers.length > 0 ? <ul>{assessment.blockers.map((x) => <li>{x}</li>)}</ul> : <p class="muted small">尚未描述主流程阻塞、异常兜底或补偿风险。</p>}
        </div>
        <div class="impact-list-block">
          <h3>自测重点</h3>
          {assessment.testItems.length > 0 ? <ul>{assessment.testItems.map((x) => <li>{x}</li>)}</ul> : <p class="muted small">尚未沉淀自测/回归链路。</p>}
        </div>
      </div>
      {!assessment.complete ? (
        <div class="impact-actions">
          {req.reqDir ? (
            <form method="post" action="/api/requirement/impact-template">
              <input type="hidden" name="reqId" value={req.id} />
              <button type="submit" class="btn btn-primary">{assessment.exists ? "补齐模板" : "创建 impact.md 模板"}</button>
            </form>
          ) : null}
          <span class="muted small">建议在进入「开发中」编码前补齐，测试可直接按这里回归核心链路。</span>
        </div>
      ) : null}
    </section>
  )
}

const AlignmentCard: FC<{ req: Requirement; alignmentContent?: string; prdContent?: string }> = ({ req, alignmentContent, prdContent }) => {
  const hasAlignment = !!alignmentContent?.trim()
  const hasPrd = !!prdContent?.trim()
  const complete = hasAlignment && !/待补充|待确认/.test(alignmentContent || "")
  const statusText = complete ? "已完成" : hasAlignment ? "待补齐" : "未创建"
  return (
    <section class={`impact-card${complete ? " impact-card-complete" : " impact-card-incomplete"}`} aria-label="需求对齐">
      <div class="impact-card-head">
        <div>
          <h2 class="op-section-title">需求对齐</h2>
          <p class="muted small">业务对齐门：把产品/业务 PRD 或口述需求转成标准业务说明；后续阶段默认以 alignment.md 为准。</p>
        </div>
        <span class={`impact-status ${complete ? "impact-status-ok" : "impact-status-warn"}`}>{statusText}</span>
      </div>
      <div class="impact-grid">
        <div class="impact-metric"><span class="field-label">标准文档</span><strong>{hasAlignment ? "alignment.md" : "缺失"}</strong></div>
        <div class="impact-metric"><span class="field-label">PRD 来源</span><span>{hasPrd ? "prd.md 已记录" : "未记录"}</span></div>
      </div>
      <div class="impact-actions">
        {req.reqDir ? (
          <form method="post" action="/api/requirement/alignment-template">
            <input type="hidden" name="reqId" value={req.id} />
            <button type="submit" class="btn btn-primary">{hasAlignment ? "补齐需求对齐模板" : "创建需求对齐模板"}</button>
          </form>
        ) : null}
        <span class="muted small">如果用户提供飞书 PRD，先记录到 prd.md，再提炼成 alignment.md；PRD 后续只用于回溯。</span>
      </div>
    </section>
  )
}

/**
 * Format a byte count into a human-readable string (KB / MB).
 * Used only by the attachment card.
 */
function formatFileSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`
}

/**
 * Attachment card - lists files under `<reqDir>/attachments/` with
 * download and delete actions, plus an upload form (multipart).
 * Only rendered when the requirement has an on-disk directory.
 */
const AttachmentCard: FC<{ req: Requirement; attachments: AttachmentInfo[] }> = ({ req, attachments }) => {
  return (
    <section class="req-attachments" aria-label="附件文件">
      <h2 class="op-section-title">📎 附件文件（{attachments.length}）</h2>
      <p class="muted small" style="margin-bottom: 8px;">存放 SQL 数据、CSV 导出等需求相关文件。文件保存在 <code>{ATTACHMENTS_DIR_NAME}/</code> 子目录。</p>
      {attachments.length > 0 ? (
        <ul class="req-attachments-list">
          {attachments.map((a) => (
            <li class="req-attachments-item">
              <span class="req-attachments-name">📎 {a.filename}</span>
              <span class="req-attachments-size muted small">{formatFileSize(a.size)}</span>
              <span class="req-attachments-time muted small">{new Date(a.mtime).toLocaleString("zh-CN", { hour12: false })}</span>
              <a
                class="btn btn-sm btn-secondary"
                href={`/requirement/attachments/download?reqId=${encodeURIComponent(req.id)}&filename=${encodeURIComponent(a.filename)}`}
                download={a.filename}
              >
                下载
              </a>
              <form
                method="post"
                action="/api/requirement/attachments/delete"
                class="req-attachments-delete-form"
                onsubmit="return confirm(`确认删除附件 ${a.filename}？`);"
              >
                <input type="hidden" name="reqId" value={req.id} />
                <input type="hidden" name="filename" value={a.filename} />
                <input type="hidden" name="redirect" value={`/requirement?id=${encodeURIComponent(req.id)}`} />
                <button type="submit" class="btn btn-sm btn-reject" title="删除此附件">删除</button>
              </form>
            </li>
          ))}
        </ul>
      ) : (
        <p class="muted small" style="margin-bottom: 10px;">暂无附件。</p>
      )}
      <form
        method="post"
        action="/api/requirement/attachments/upload"
        enctype="multipart/form-data"
        class="req-attachments-upload-form"
      >
        <input type="hidden" name="reqId" value={req.id} />
        <input type="hidden" name="redirect" value={`/requirement?id=${encodeURIComponent(req.id)}`} />
        <input type="file" name="file" required />
        <button type="submit" class="btn btn-sm btn-primary">上传附件</button>
        <span class="muted small">同名文件将被覆盖</span>
      </form>
    </section>
  )
}

const RequirementDetailPage: FC<{
  req: Requirement
  associated: SessionInfo[]
  unassociated: SessionInfo[]
  recommendations: SessionRecommendation[]
  extractHistory: ExtractHistoryRecord[]
  backgroundContent?: string
  alignmentContent?: string
  prdContent?: string
  branchContent?: string
  notesContent?: string
  testContent?: string
  configContent?: string
  impactContent?: string
  memoryContent?: string
  reviewContent?: string
  impactAssessment: ImpactAssessment
  branchScope?: BranchScope | null
  codeReviewSnapshot?: CodeReviewSnapshot | null
  state?: RequirementState | null
  attachments?: AttachmentInfo[]
}> = ({ req, associated, unassociated, recommendations, extractHistory, backgroundContent, alignmentContent, prdContent, branchContent, notesContent, testContent, configContent, impactContent, memoryContent, reviewContent, impactAssessment, branchScope, codeReviewSnapshot, state, attachments = [] }) => {
  const currentIdx = REQ_STATUSES.indexOf(req.status)
  const description = (req.description || "").trim()
  const canSwitch = !!req.reqDir
  const next = nextStatus(req.status)
  const history = state?.history ?? []
  // Reverse-chronological display, but keep a stable copy.
  const historyDesc = [...history].sort((a, b) => b.at - a.at)
  return (
    <Layout title={`Requirement ${req.title}`} active="requirements">
      <div class="req-detail">
      <div class="page-header">
        <a href="/projects" class="back-link">← All requirements</a>
        <h1>
          {req.title}
          <span class={reqStatusBadgeClass(req.status)} style="margin-left: 8px;">{req.status}</span>
        </h1>
        <div class="meta-grid">
          <div><span class="field-label">项目</span> {(req.projects?.length ? req.projects : [req.project]).join(" / ")}{req.groupPath && req.groupPath.length > 0 ? <span class="muted small"> / {req.groupPath.join(" / ")}</span> : null}</div>
          <div><span class="field-label">Req ID</span> <code>{req.id}</code></div>
          <div><span class="field-label">更新于</span> {formatRelAgo(req.updatedAt)}</div>
        </div>
      </div>
      {(() => {
        const orderedAssociated = sortByLastUsedDesc(associated)
        const recent = orderedAssociated[0] ?? null
        const others = orderedAssociated.slice(1)
        const isActive = ACTIVE_STATUSES.includes(req.status)
        return (
          <section class="req-session-panel" aria-label="需求 Session 选择">
            {recent ? (
              <div class="req-session-panel-row">
                <a class="btn btn-primary" href={`/session?id=${encodeURIComponent(recent.id)}&req=${encodeURIComponent(req.id)}`}>
                  继续任务 →
                </a>
                <span class="muted small">
                  上次使用 session <code>{recent.id.slice(0, 16)}…</code> · {formatRelAgo(recent.updated || recent.created)}
                </span>
                <button
                  type="button"
                  class="btn btn-secondary req-copy-cmd-btn"
                  data-copy-cmd={`opencode -s ${recent.id}`}
                  title={`复制 \`opencode -s ${recent.id}\` 到剪贴板`}
                >
                  📋 复制命令
                </button>
                <form method="post" action="/api/requirement/extract-context" class="req-extract-trigger-form" data-extract-trigger="">
                  <input type="hidden" name="reqId" value={req.id} />
                  <input type="hidden" name="sessionId" value={recent.id} />
                  <button
                    type="submit"
                    class="btn btn-secondary req-extract-link"
                    title="让 opencode 后台总结这个 session 的对话，完成后弹出提示进入预览页"
                  >
                    从此 session 提取上下文 →
                  </button>
                </form>
                <form method="post" action="/api/requirement/auto-extract" class="req-extract-trigger-form" data-extract-trigger="">
                  <input type="hidden" name="reqId" value={req.id} />
                  <input type="hidden" name="sessionId" value={recent.id} />
                  <button
                    type="submit"
                    class="btn btn-secondary req-extract-link"
                    title="让 agent 读取需求上下文文件，根据 session 内容判断哪些文件需要更新"
                  >
                    🤖 智能提取上下文 →
                  </button>
                </form>
                <button type="button" class="btn btn-secondary req-new-session-btn" data-req-id={req.id} title="为该需求再创建一个 session">另开新 session</button>
                <span class="req-new-session-result" data-req-id={req.id}></span>
                <form method="post" action="/api/requirement/dissociate" class="req-dissociate-form" onsubmit="return confirm('确认解除此 session 与该需求的绑定？');">
                  <input type="hidden" name="reqId" value={req.id} />
                  <input type="hidden" name="sessionId" value={recent.id} />
                  <button type="submit" class="btn btn-secondary req-dissociate-btn" title="解除此 session 与该需求的绑定">解除绑定</button>
                </form>
              </div>
            ) : (
              <div class="req-session-panel-empty">
                <p class="req-session-panel-prompt">
                  该需求{isActive ? <> 当前状态 <span class={reqStatusBadgeClass(req.status)}>{req.status}</span>，</> : "尚"}未绑定任何 session。请选择：
                </p>
                <div class="req-session-panel-actions">
                  <div class="req-session-inline-form">
                    <button type="button" class="btn btn-primary req-new-session-btn" data-req-id={req.id}>新建并绑定 session</button>
                    <span class="req-new-session-result" data-req-id={req.id}></span>
                    <span class="muted small" style="margin-left: 8px;">将在后台运行 <code>opencode run</code> 并把新 session 关联到此需求</span>
                  </div>
                  {unassociated.length > 0 ? (
                    <form method="post" action="/api/requirement/associate" class="req-session-inline-form">
                      <input type="hidden" name="reqId" value={req.id} />
                      <SessionPicker
                        candidates={unassociated}
                        listId={`unbound-sessions-top-${req.id}`}
                        placeholder={`筛选 ${unassociated.length} 个孤儿 session…`}
                      />
                      <button type="submit" class="btn btn-secondary">绑定到此需求</button>
                    </form>
                  ) : (
                    <span class="muted small">（没有可关联的孤儿 session）</span>
                  )}
                </div>
              </div>
            )}
            {others.length > 0 ? (
              <details class="req-session-panel-others">
                <summary>其它已绑定的 session（{others.length}）</summary>
                <ul class="req-session-list">
                  {others.map((s) => (
                    <li>
                      <a href={`/session?id=${encodeURIComponent(s.id)}&req=${encodeURIComponent(req.id)}`}>
                        <code>{s.id}</code>
                      </a>
                      <span class="muted small">{s.title || ""}</span>
                      <span class="muted small">{formatRelAgo(s.updated || s.created)}</span>
                      <button
                        type="button"
                        class="req-copy-cmd-inline"
                        data-copy-cmd={`opencode -s ${s.id}`}
                        title={`复制 \`opencode -s ${s.id}\` 到剪贴板`}
                      >
                        📋 复制
                      </button>
                      <form
                        method="post"
                        action="/api/requirement/extract-context"
                        class="req-extract-trigger-form req-extract-trigger-inline"
                        data-extract-trigger=""
                      >
                        <input type="hidden" name="reqId" value={req.id} />
                        <input type="hidden" name="sessionId" value={s.id} />
                        <button
                          type="submit"
                          class="muted small req-extract-link-inline"
                          title="让 opencode 后台总结这个 session 的对话，完成后顶部提示进入预览页"
                        >
                          提取上下文 →
                        </button>
                      </form>
                      <form
                        method="post"
                        action="/api/requirement/auto-extract"
                        class="req-extract-trigger-form req-extract-trigger-inline"
                        data-extract-trigger=""
                      >
                        <input type="hidden" name="reqId" value={req.id} />
                        <input type="hidden" name="sessionId" value={s.id} />
                        <button
                          type="submit"
                          class="muted small req-extract-link-inline"
                          title="让 agent 读取需求上下文文件，根据 session 内容判断哪些文件需要更新"
                        >
                          🤖 智能提取 →
                        </button>
                      </form>
                      <form
                        method="post"
                        action="/api/requirement/dissociate"
                        class="req-dissociate-form req-dissociate-inline"
                        onsubmit="return confirm('确认解除此 session 与该需求的绑定？');"
                      >
                        <input type="hidden" name="reqId" value={req.id} />
                        <input type="hidden" name="sessionId" value={s.id} />
                        <button
                          type="submit"
                          class="muted small req-dissociate-link-inline"
                          title="解除此 session 与该需求的绑定"
                        >
                          解除绑定
                        </button>
                      </form>
                      <a class="muted small req-extract-link-inline" href={`/requirement/recall?reqId=${encodeURIComponent(req.id)}&sessionId=${encodeURIComponent(s.id)}`} title="只读召回这个历史 session 的文本上下文">
                        召回历史 →
                      </a>
                    </li>
                  ))}
                </ul>
              </details>
            ) : null}
          </section>
        )
      })()}

      {recommendations.length > 0 ? (
        <section class="req-recommendations" aria-label="疑似相关 session">
          <h2 class="op-section-title">疑似相关 Session（{recommendations.length}）</h2>
          <p class="muted small" style="margin-bottom: 8px;">根据标题、路径和关键词匹配推荐，点击右侧按钮一键绑定。</p>
          <ul class="req-reco-list">
            {recommendations.map((reco) => (
              <li class="req-reco-item">
                <div class="req-reco-info">
                  <a href={`/session?id=${encodeURIComponent(reco.session.id)}`}>
                    <code>{reco.session.id.slice(0, 20)}…</code>
                  </a>
                  <span class="req-reco-title">{reco.session.title || ""}</span>
                  <span class="muted small">{formatRelAgo(reco.session.updated || reco.session.created)}</span>
                </div>
                <div class="req-reco-meta">
                  <span class="req-reco-score muted small">{reco.score} 分</span>
                  <span class="req-reco-reasons muted small">{reco.reasons.slice(0, 3).join(" · ")}</span>
                  <form method="post" action="/api/requirement/associate" class="req-extract-trigger-form">
                    <input type="hidden" name="reqId" value={req.id} />
                    <input type="hidden" name="sessionId" value={reco.session.id} />
                    <button type="submit" class="btn btn-secondary req-reco-bind" title="绑定此 session 到当前需求">绑定</button>
                  </form>
                </div>
              </li>
            ))}
          </ul>
        </section>
      ) : null}

      <AlignmentCard req={req} alignmentContent={alignmentContent} prdContent={prdContent} />

      <div class="req-status-flow">
        {REQ_STATUSES.map((s, i) => {
          const cls = i === currentIdx ? "req-flow-step active" : i < currentIdx ? "req-flow-step done" : "req-flow-step"
          return <span class={cls}>{s}</span>
        })}
      </div>

      {canSwitch ? (
        <section class="req-status-switcher" aria-label="切换需求状态">
          <div class="req-status-switcher-row">
            <form method="post" action="/api/requirement/category" class="req-status-form req-category-form">
              <input type="hidden" name="reqId" value={req.id} />
              <input type="hidden" name="redirect" value={`/requirement?id=${encodeURIComponent(req.id)}`} />
              <label class="field-label" for={`req-category-select-${req.id}`}>类别</label>
              <select id={`req-category-select-${req.id}`} name="category" required>
                {REQ_CATEGORIES.map((cat) => (
                  <option value={cat} selected={cat === (req.category ?? "需求")}>{cat}</option>
                ))}
              </select>
              <button type="submit" class="btn btn-secondary">应用</button>
            </form>
            <form method="post" action="/api/requirement/status" class="req-status-form">
              <input type="hidden" name="reqId" value={req.id} />
              <input type="hidden" name="redirect" value={`/requirement?id=${encodeURIComponent(req.id)}`} />
              <label class="field-label" for={`req-status-select-${req.id}`}>切换到</label>
              <select id={`req-status-select-${req.id}`} name="status" required>
                {REQ_STATUSES.map((s) => (
                  <option value={s} selected={s === req.status}>{s}</option>
                ))}
              </select>
              <input type="text" name="note" placeholder="备注（可选）" class="req-status-note" maxlength={200} />
              <button type="submit" class="btn btn-secondary">应用</button>
            </form>
            {next ? (
              <form method="post" action="/api/requirement/status" class="req-status-form req-status-next">
                <input type="hidden" name="reqId" value={req.id} />
                <input type="hidden" name="status" value={next} />
                <input type="hidden" name="redirect" value={`/requirement?id=${encodeURIComponent(req.id)}`} />
                <button type="submit" class="btn btn-primary" title={`从 ${req.status} 推进到 ${next}`}>
                  推进到「{next}」 →
                </button>
              </form>
            ) : (
              <span class="muted small">已是末态</span>
            )}
          </div>
          {historyDesc.length > 0 ? (
            <details class="req-status-history">
              <summary>状态变更历史（{historyDesc.length}）</summary>
              <ol class="req-status-history-list">
                {historyDesc.map((h) => (
                  <li>
                    <span class="muted small mono">{new Date(h.at).toLocaleString("zh-CN", { hour12: false })}</span>
                    <span class={reqStatusBadgeClass(h.status)} style="margin-left: 8px;">{h.status}</span>
                    {h.from ? <span class="muted small" style="margin-left: 6px;">← {h.from}</span> : null}
                    {h.note ? <span class="muted small" style="margin-left: 8px;">— {h.note}</span> : null}
                  </li>
                ))}
              </ol>
            </details>
          ) : null}
        </section>
      ) : (
        <p class="muted small" style="margin: 6px 0 0;">合成的默认需求不支持状态切换。</p>
      )}

      {description ? (
        <section class="req-hermes-section">
          <h2 class="op-section-title">描述</h2>
          <pre style="white-space: pre-wrap; padding: 10px; border: 1px solid var(--op-border, #2a2a2a); border-radius: 4px; background: var(--op-bg-soft, #181818);">{description}</pre>
        </section>
      ) : null}

      <ImpactAssessmentCard req={req} assessment={impactAssessment} />

      {branchScope && branchScope.repos.length > 0 ? <BranchScopeCard req={req} scope={branchScope} /> : null}

      {req.reqDir ? (
        <nav class="req-detail-tool-links" aria-label="需求专项视图">
          <a class="btn btn-secondary" href={`/requirement/review?id=${encodeURIComponent(req.id)}`}>查看代码差异</a>
          <a class="btn btn-secondary" href={`/requirement/release?id=${encodeURIComponent(req.id)}`}>查看发版注意事项</a>
        </nav>
      ) : null}

      <HermesFileSection title="需求记忆" content={memoryContent} />
      <HermesFileSection title="需求对齐" content={alignmentContent} />
      <HermesFileSection title="PRD 来源" content={prdContent} />
      <HermesFileSection title="需求背景" content={backgroundContent} />
      {branchContent ? (
        <details class="req-hermes-section req-branch-md-details">
          <summary class="op-section-title">完整分支记录（branch.md）</summary>
          <pre style="white-space: pre-wrap; max-height: 480px; overflow: auto; padding: 10px; border: 1px solid var(--op-border, #2a2a2a); border-radius: 4px; background: var(--op-bg-soft, #181818);">{branchContent}</pre>
        </details>
      ) : null}
      <HermesFileSection title="开发笔记" content={notesContent} />
      <HermesFileSection title="影响面评估" content={impactContent} />
      <HermesFileSection title="测试范围" content={testContent} />
      <HermesFileSection title="配置变更" content={configContent} />
      <HermesFileSection title="上线 Review" content={reviewContent} />

      {req.reqDir ? <AttachmentCard req={req} attachments={attachments} /> : null}

      {extractHistory.length > 0 ? (
        <section class="req-extract-history" aria-label="上下文提取历史">
          <h2 class="op-section-title">提取历史（最近 {extractHistory.length} 次）</h2>
          <ol class="req-extract-history-list">
            {extractHistory.map((h) => (
              <li class={`req-extract-history-item req-extract-history-${h.state}`}>
                <span class="muted small mono">{new Date(h.doneAt).toLocaleString("zh-CN", { hour12: false })}</span>
                <span class={`req-status-badge req-status-${h.state === "done" ? "done" : "testing"}`} style="margin-left: 6px;">
                  {h.state === "done" ? "✓" : "✗"} {h.mode === "auto" ? "智能提取" : "摘要"}
                </span>
                <span class="muted small" style="margin-left: 6px;">{h.sessionId.slice(0, 16)}…</span>
                {h.salvagedFromFork ? <span class="muted small" style="margin-left: 4px;">（fork 救回）</span> : null}
                {h.summary ? <div class="req-extract-history-summary muted small">{h.summary}</div> : null}
                {h.errorMessage ? <div class="req-extract-history-error muted small">{h.errorMessage}</div> : null}
              </li>
            ))}
          </ol>
        </section>
      ) : null}

      <section class="req-sessions">
        <h2 class="op-section-title">关联 Sessions ({associated.length})</h2>
        {associated.length === 0 ? (
          <p class="muted small">暂无关联的 session。</p>
        ) : (
          <ul class="req-session-list">
            {sortByLastUsedDesc(associated).map((s, i) => (
              <li>
                <a href={`/session?id=${encodeURIComponent(s.id)}&req=${encodeURIComponent(req.id)}`}>
                  <code>{s.id}</code>
                </a>
                {i === 0 ? <span class="req-session-badge">上次使用</span> : null}
                <span class="muted small">{s.title || ""}</span>
                <span class="muted small">{formatRelAgo(s.updated || s.created)}</span>
                <button
                  type="button"
                  class="req-copy-cmd-inline"
                  data-copy-cmd={`opencode -s ${s.id}`}
                  title={`复制 \`opencode -s ${s.id}\` 到剪贴板`}
                >
                  📋 复制
                </button>
                <form
                  method="post"
                  action="/api/requirement/extract-context"
                  class="req-extract-trigger-form req-extract-trigger-inline"
                  data-extract-trigger=""
                >
                  <input type="hidden" name="reqId" value={req.id} />
                  <input type="hidden" name="sessionId" value={s.id} />
                  <button
                    type="submit"
                    class="muted small req-extract-link-inline"
                    title="让 opencode 后台总结这个 session 的对话，完成后顶部提示进入预览页"
                  >
                    提取上下文 →
                  </button>
                </form>
                <a class="muted small req-extract-link-inline" href={`/requirement/recall?reqId=${encodeURIComponent(req.id)}&sessionId=${encodeURIComponent(s.id)}`} title="只读召回这个历史 session 的文本上下文">
                  召回历史 →
                </a>
              </li>
            ))}
          </ul>
        )}

        <div class="req-form-actions" style="margin-top: 12px;">
          <button type="button" class="btn btn-primary req-new-session-btn" data-req-id={req.id}>新建 Session</button>
          <span class="req-new-session-result" data-req-id={req.id}></span>
        </div>

        {unassociated.length > 0 ? (
          <form method="post" action="/api/requirement/associate" class="req-form-actions" style="margin-top: 12px;">
            <input type="hidden" name="reqId" value={req.id} />
            <SessionPicker
              candidates={unassociated}
              listId={`unbound-sessions-bottom-${req.id}`}
              placeholder={`筛选 ${unassociated.length} 个孤儿 session…`}
            />
            <button type="submit" class="btn btn-secondary">关联已有 Session</button>
          </form>
        ) : null}
      </section>
      </div>
      <script src="/static/req-detail.js" defer></script>
    </Layout>
  )
}

/**
 * Preview page for the "extract context from session" flow.
 *
 * The page is rendered in three modes, driven by `job`:
 *   - `job === null`            : "no in-flight job" placeholder with
 *     a "回到需求页" button. Reachable when the user opens a stale URL.
 *   - `job.state === "running"` : "still working" card; the inline JS
 *     polls /api/extract/job/:id and reloads the page when state flips.
 *   - `job.state === "done"`    : the editable textarea + commit form.
 *   - `job.state === "failed"`  : a read-only error block with stderr
 *     snippet + a "retry" button (POSTs a fresh start through the
 *     existing detail-page button rather than spawning here).
 *
 * Why a dedicated page: we want a human-in-the-loop checkpoint between
 * "opencode generated a summary" and "the summary is committed to
 * notes.md". The body lives in an editable <textarea> so the user can
 * trim or rewrite before committing.
 */
const RequirementExtractPreviewPage: FC<{
  req: Requirement
  sessionId: string
  job: ExtractJob | null
}> = ({ req, sessionId, job }) => {
  const backHref = `/requirement?id=${encodeURIComponent(req.id)}`
  const elapsedMs = job ? (job.doneAt ?? Date.now()) - job.startedAt : 0
  return (
    <Layout title={`提取上下文 — ${req.title}`} active="requirements">
      <div class="req-extract">
        <div class="page-header">
          <a href={backHref} class="back-link">← 返回需求 {req.title}</a>
          <h1>从 session 提取上下文</h1>
          <div class="meta-grid">
            <div><span class="field-label">需求</span> {req.title} <span class={reqStatusBadgeClass(req.status)} style="margin-left: 6px;">{req.status}</span></div>
            <div><span class="field-label">Session</span> <code>{sessionId}</code></div>
            {job ? <div><span class="field-label">耗时</span> {(elapsedMs / 1000).toFixed(1)}s</div> : null}
          </div>
        </div>

        {job === null ? (
          <section class="req-extract-error" aria-label="无任务">
            <p class="req-extract-error-msg">
              <strong>找不到任务</strong>：可能已超过 30 分钟被自动清理，或服务重启后任务丢失。
            </p>
            <div class="req-extract-actions">
              <a href={backHref} class="btn btn-secondary">返回需求</a>
              <span class="muted small">回到需求页后重新点击「提取上下文」即可重启一次。</span>
            </div>
          </section>
        ) : job.state === "running" ? (
          <section class="req-extract-running" aria-label="生成中" data-job-id={job.id} data-req-id={req.id}>
            <p>
              <span class="req-extract-spinner" aria-hidden="true"></span>
              <strong>opencode 正在生成摘要…</strong>
            </p>
            <p class="muted small">
              已运行 <span class="js-extract-elapsed">{(elapsedMs / 1000).toFixed(0)}</span> 秒。完成后此页会自动刷新；你也可以关闭页面，稍后通过需求页顶部 toast 进入。
            </p>
            <div class="req-extract-actions">
              <a href={backHref} class="btn btn-secondary">返回需求页等待</a>
            </div>
          </section>
        ) : job.state === "done" ? (
          <section class="req-extract-preview" aria-label="摘要预览">
            {job.salvagedFromFork ? (
              <div class="req-extract-salvage-banner" role="status">
                <strong>已从 fork session 救回摘要</strong>
                ：opencode 子进程虽未正常退出，但 LLM 已在副本会话里写完了内容。下面的文本直接取自该 fork。
                {job.forkSessionId ? (
                  <>
                    {" "}副本：<code>{job.forkSessionId}</code>
                    {job.forkTitle ? <span class="muted small"> · {job.forkTitle}</span> : null}
                    {" "}<a href={`/session?id=${encodeURIComponent(job.forkSessionId)}`} class="op-toast-btn" target="_blank" rel="noopener">打开 fork session</a>
                  </>
                ) : null}
              </div>
            ) : null}
            <p class="muted small">
              下面是 <code>opencode</code> 生成的摘要。<strong>不会自动写入</strong> notes.md —
              你可以直接编辑文本框内的内容，确认后点击「合并到 notes.md」。如不满意，直接「取消」即可。
            </p>
            <form method="post" action="/api/requirement/extract-context/commit" class="req-extract-form">
              <input type="hidden" name="reqId" value={req.id} />
              <input type="hidden" name="sessionId" value={sessionId} />
              <textarea
                name="body"
                class="req-extract-body"
                rows={"24"}
                spellcheck={false}
                aria-label="摘要正文"
              >{job.stdout}</textarea>
              <div class="req-extract-actions">
                <button type="submit" class="btn btn-primary">合并到 notes.md</button>
                <a href={backHref} class="btn btn-secondary">取消</a>
                <span class="muted small">
                  追加到需求目录下的 <code>notes.md</code>，附时间戳与 session id 标题。
                </span>
              </div>
            </form>
          </section>
        ) : (
          <section class="req-extract-error" aria-label="摘要失败">
            <p class="req-extract-error-msg">
              <strong>生成失败</strong>：{job.errorMessage || "未知错误"}
            </p>
            <dl class="req-extract-error-detail">
              <dt>退出码</dt><dd>{String(job.exitCode)}</dd>
              <dt>超时</dt><dd>{job.timedOut ? "是" : "否"}</dd>
              <dt>已捕获</dt><dd>{job.stdout.length} 字节</dd>
              {job.stderr ? (<>
                <dt>stderr 摘要</dt>
                <dd><pre class="req-extract-stderr">{job.stderr.slice(0, 2000)}</pre></dd>
              </>) : null}
            </dl>
            {/*
              Salvage branch: when the LLM already wrote markdown before
              we killed the process, let the user keep it. The same
              commit endpoint is used; we just exit the read-only error
              card into an editable form.
            */}
            {job.stdout.length > 0 ? (
              <div class="req-extract-salvage" aria-label="抢救已捕获的摘要">
                <p class="muted small">
                  虽然 opencode 没有按时退出，但 stdout 里已经有一段可用的摘要文本。下面是抢救出来的部分；你可以编辑后照常合并到 notes.md。
                </p>
                <form method="post" action="/api/requirement/extract-context/commit" class="req-extract-form">
                  <input type="hidden" name="reqId" value={req.id} />
                  <input type="hidden" name="sessionId" value={sessionId} />
                  <textarea
                    name="body"
                    class="req-extract-body"
                    rows={"18"}
                    spellcheck={false}
                    aria-label="抢救摘要正文"
                  >{job.stdout}</textarea>
                  <div class="req-extract-actions">
                    <button type="submit" class="btn btn-primary">合并已捕获文本到 notes.md</button>
                  </div>
                </form>
              </div>
            ) : null}
            <div class="req-extract-actions">
              <form method="post" action="/api/requirement/extract-context" class="req-extract-retry-form">
                <input type="hidden" name="reqId" value={req.id} />
                <input type="hidden" name="sessionId" value={sessionId} />
                <button type="submit" class="btn btn-secondary">重试</button>
              </form>
              <a href={backHref} class="btn btn-secondary">返回需求</a>
            </div>
          </section>
        )}
      </div>
      <script src="/static/req-detail.js" defer></script>
    </Layout>
  )
}

const RequirementRecallPage: FC<{
  req: Requirement
  sessionId: string
  markdown: string
  partCount: number
}> = ({ req, sessionId, markdown, partCount }) => {
  const backHref = `/requirement?id=${encodeURIComponent(req.id)}`
  return (
    <Layout title={`召回历史 — ${req.title}`} active="requirements">
      <div class="req-extract">
        <div class="page-header">
          <a href={backHref} class="back-link">← 返回需求 {req.title}</a>
          <h1>历史 Session 召回</h1>
          <div class="meta-grid">
            <div><span class="field-label">需求</span> {req.title}</div>
            <div><span class="field-label">Session</span> <code>{sessionId}</code></div>
            <div><span class="field-label">Text parts</span> {partCount}</div>
          </div>
        </div>
        <section class="req-extract-preview" aria-label="历史 session 召回内容">
          <p class="muted small">
            这是从 OpenCode SQLite 直接读取的只读文本片段；已过滤 reasoning、tool、step 和非文本 part。用于人工或 AI 按需追溯，不会自动写入需求文件。
          </p>
          {markdown ? (
            <pre class="req-extract-body" style="white-space: pre-wrap; overflow: auto; max-height: 70vh;">{markdown}</pre>
          ) : (
            <div class="auto-extract-empty">
              <p>没有读到可召回的文本片段。可能该 session 不在本机 SQLite 中，或只有工具/流程片段。</p>
              <a href={backHref} class="btn btn-secondary">返回需求</a>
            </div>
          )}
        </section>
      </div>
    </Layout>
  )
}

// ---------------------------------------------------------------------------
// Fastify app + plugins
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Control-center dashboard page
// ---------------------------------------------------------------------------

const ReactAppPage: FC<{ title: string; active: Tab }> = ({ title, active }) => (
  <Layout title={title} active={active}>
    <div id="dashboard-root" data-api="/api/dashboard/stats">
      <div class="react-dashboard-fallback">正在加载 React app…</div>
    </div>
    <link rel="stylesheet" href="/static/dashboard-react/dashboard.css?v=20260714-category-search" />
    <script type="module" src="/static/dashboard-react/dashboard.js?v=20260714-category-search"></script>
  </Layout>
)

function reactPageMeta(path: string): { title: string; active: Tab } | null {
  if (path === "/" || path === DASHBOARD_PATH) return { title: "状态看板", active: "dashboard" }
  if (path === "/projects" || path === "/requirements") return { title: "需求进度看板", active: "requirements" }
  if (path === "/sessions" || path === "/sessions/refresh" || path === "/session") return { title: "Sessions", active: "sessions" }
  if (path === "/reports" || path === "/report") return { title: "Reports", active: "reports" }
  if (path === "/requirement") return { title: "Requirement", active: "requirements" }
  if (path === "/env-vars") return { title: "Env Vars", active: "envvars" }
  return null
}

/**
 * Parse the `days` query parameter for the sessions page time filter.
 * - missing / invalid / negative -> default 7 days
 * - 0 -> "all time" (no filter)
 * - positive integer -> that many days
 */
function parseDaysParam(raw: string | undefined): number {
  if (!raw) return 7
  const n = Number(raw)
  if (!Number.isFinite(n) || n < 0) return 7
  return Math.floor(n)
}

// ---------------------------------------------------------------------------
// Fastify app + plugins
// ---------------------------------------------------------------------------

const fastify = await Fastify({
  logger: { level: process.env.LOG_LEVEL || "info" },
  bodyLimit: 100 * 1024 * 1024,
})

await fastify.register(fastifyFormbody)
await fastify.register(fastifyMultipart, { attachFieldsToBody: "keyValues" })
await fastify.register(fastifyStatic, {
  root: PUBLIC_DIR,
  prefix: "/static/",
  cacheControl: false,
})
await fastify.register(fastifyWebsocket)
await fastify.register(fastifySwagger, {
  openapi: {
    info: {
      title: "Agent Panel API",
      description: "Local web control panel for coding agent sessions and requirements.",
      version: "0.2.0",
    },
  },
})
await fastify.register(fastifySwaggerUi, { routePrefix: "/docs" })

// Native Fastify health route with TypeBox schema.
fastify.get("/health", { schema: { response: { 200: Type.Object({ ok: Type.Boolean(), ts: Type.Number() }) } } }, async () => {
  return { ok: true, ts: Date.now() }
})

// Vendor xterm assets from node_modules (hardcoded safe paths).
for (const [route, pkg, rel, contentType] of [
  ["/vendor/xterm/xterm.css", "@xterm/xterm", "css/xterm.css", "text/css"],
  ["/vendor/xterm/xterm.js", "@xterm/xterm", "lib/xterm.js", "application/javascript"],
  ["/vendor/xterm-addon-fit/addon-fit.js", "@xterm/addon-fit", "lib/addon-fit.js", "application/javascript"],
] as const) {
  fastify.get(route, async (_req, reply) => {
    const filePath = join(NODE_MODULES_DIR, pkg, rel)
    try {
      const content = await readFile(filePath)
      reply.type(contentType).header("Cache-Control", "public, max-age=3600").send(content)
    } catch {
      reply.code(404).send("Not found")
    }
  })
}

fastify.addHook("preHandler", async (request, reply) => {
  if (request.method !== "GET") return
  const path = request.url.split("?")[0] || "/"
  const meta = reactPageMeta(path)
  if (!meta) return
  reply.type("text/html; charset=utf-8").send(<ReactAppPage title={meta.title} active={meta.active} />)
})

const app = createRouter(fastify)

// Projects (requirements) page — available from the sidebar after the status dashboard.
async function renderProjectsPage(c: Ctx) {
  const url = new URL(c.req.url)
  const selectedStatuses = url.searchParams
    .getAll("status")
    .filter((status): status is ReqStatus => (REQ_STATUSES as string[]).includes(status))
  const projectFilter = (url.searchParams.get("project") || "").trim()
  const subprojectFilter = (url.searchParams.get("subproject") || "").trim()
  const createdFrom = url.searchParams.get("createdFrom") || ""
  const createdTo = url.searchParams.get("createdTo") || ""
  const rawCategory = (url.searchParams.get("category") || "").trim()
  const categoryFilter = (REQ_CATEGORIES as string[]).includes(rawCategory) ? (rawCategory as ReqCategory) : ""
  const keyword = (url.searchParams.get("q") || "").trim()
  const groups = await listRequirementsByProject()
  const counts: Record<ReqStatus, number> = {
    "需求对齐": 0,
    "方案设计": 0,
    "开发中": 0,
    "自测中": 0,
    "测试中": 0,
    "待上线": 0,
    "已完成": 0,
  }
  for (const g of groups) {
    for (const r of g.requirements) counts[r.status] += 1
  }
  const projectOptions = groups.map((g) => g.project).sort()
  const subprojectOptions = projectFilter
    ? [...new Set(groups
        .flatMap((g) => g.requirements)
        .filter((r) => (r.projects?.length ? r.projects : [r.project]).includes(projectFilter))
        .map((r) => r.groupPath[0] || "")
        .filter(Boolean))].sort()
    : []
  const normalizedSubproject = subprojectOptions.includes(subprojectFilter) ? subprojectFilter : ""
  const items = buildRequirementBoardItems(groups, {
    statuses: selectedStatuses,
    project: projectFilter,
    subproject: normalizedSubproject,
    category: categoryFilter || undefined,
    keyword: keyword || undefined,
    createdFrom: parseRequirementDateBoundary(createdFrom),
    createdTo: parseRequirementDateBoundary(createdTo, true),
  })
  return c.html(
    <ProjectsPage
      items={items}
      counts={counts}
      selectedStatuses={selectedStatuses}
      projectFilter={projectFilter}
      subprojectFilter={normalizedSubproject}
      createdFrom={createdFrom}
      createdTo={createdTo}
      projectOptions={projectOptions}
      subprojectOptions={subprojectOptions}
      categoryFilter={categoryFilter}
      keyword={keyword}
    />,
  )
}

async function readDashboardStatsPayload() {
  const groups = await listRequirementsByProject()
  const requirements = [...new Map(groups.flatMap((g) => g.requirements).map((r) => [r.id, r])).values()]
  return { generatedAt: Date.now(), stats: buildRequirementStats(requirements) }
}

async function renderDashboardPage(c: Ctx) {
  return c.html(<ReactAppPage title="状态看板" active="dashboard" />)
}

app.get("/api/dashboard/stats", async (c) => c.json(await readDashboardStatsPayload()))
app.get(DASHBOARD_PATH, async (c) => renderDashboardPage(c))
app.get("/", async (c) => renderDashboardPage(c))

// Sessions landing page (was previously at "/")
app.get("/sessions", async (c) => {
  const { harness } = await getConfig()
  const days = parseDaysParam(c.req.query("days"))
  const maxAgeMs = days > 0 ? days * 24 * 60 * 60 * 1000 : undefined
  const sessions = await scanDashboardSessions(harness, false, maxAgeMs)
  const summary = summarizeSessions(sessions)
  return c.html(<SessionsPage sessions={sessions} summary={summary} days={days} harness={harness} />)
})

// Refresh cache: re-scan sessions.
app.get("/sessions/refresh", async (c) => {
  const { harness } = await getConfig()
  const days = parseDaysParam(c.req.query("days"))
  const maxAgeMs = days > 0 ? days * 24 * 60 * 60 * 1000 : undefined
  const sessions = await scanDashboardSessions(harness, true, maxAgeMs)
  const summary = summarizeSessions(sessions)
  return c.html(<SessionsPage sessions={sessions} summary={summary} days={days} harness={harness} />)
})

// Reports list (the original / path moved here)
app.get("/reports", async (c) => {
  const reports = await scanReports()
  const enriched = await Promise.all(
    reports
      .filter((r) => r.highCount > 0 || r.mediumCount > 0)
      .map(async (r) => {
        const status = await getConfirmationStatus(r.reportPath)
        return { ...r, confirmedCount: status.confirmedIds.length, rejectedCount: status.rejectedIds.length }
      }),
  )
  return c.html(<ReportListPage reports={enriched} />)
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
  const confirmation = await getConfirmationStatus(reportPath)
  return c.html(<ReportDetailPage report={report} reportPath={reportPath} confirmation={confirmation} />)
})

// Embedded terminal page
app.get("/session", async (c) => {
  const { harness } = await getConfig()
  const id = c.req.query("id")
  const reqIdParam = c.req.query("req")
  const newMode = c.req.query("new") === "1"

  // In "new" mode we don't require an id - the harness will create a real
  // session id when the PTY starts, and we'll push it back to the page.
  if (!newMode) {
    if (!id) {
      return c.text("Missing session id", 400)
    }
    if (!isValidDashboardSessionId(harness, id)) {
      return c.text("Invalid session id", 400)
    }
  } else if (id && !isValidDashboardSessionId(harness, id)) {
    return c.text("Invalid session id", 400)
  }

  let session: SessionInfo | null = id ? await getDashboardSession(harness, id) : null
  let req: Requirement | null = null
  if (reqIdParam) {
    req = await getRequirement(reqIdParam)
  }
  if (!session && newMode) {
    // "new" mode: synthesize a placeholder row so the terminal page can
    // render before opencode has created the underlying session row.
    // The WS handler will spawn `opencode run -i` and push back the
    // real id once OpenCode persists it.
    const now = Date.now()
    session = {
      id: id ?? "",
      title: "New session",
      status: "running",
      source: "fs",
      created: now,
      updated: now,
      projectId: "",
      directory: "",
    }
  }
  if (!session) {
    // Either no id was given (already handled above) OR the given id is
    // a "ghost" — not present in the OpenCode store. Refuse to spawn
    // `opencode --session <ghost>` because OpenCode will exit with
    // "Session not found". Direct the user back to the requirement so
    // they can pick "新建" or "关联已有 session" explicitly.
    return c.html(<SessionMissingPage id={id ?? ""} backReqId={req?.id} />, 404)
  }
  // If req param wasn't supplied, fall back to the requirement that
  // already owns this session (so the panel renders even without a
  // ?req= query string).
  if (!req && id) {
    req = await getRequirementForSession(id)
  }
  const reqContext = req ? await buildInjectionContext(req.id) : ""
  return c.html(<SessionTerminalPage session={session} req={req} reqContext={reqContext} createNew={newMode} harness={harness} />)
})

// ---------------------------------------------------------------------------
// Requirement routes
// ---------------------------------------------------------------------------

app.get("/projects", async (c) => renderProjectsPage(c))

app.get("/requirements", (c) => c.redirect("/projects", 302))

async function readOptionalRequirementFile(path?: string): Promise<string | undefined> {
  if (!path || !existsSync(path)) return undefined
  return readFile(path, "utf-8").catch(() => undefined)
}

async function loadRequirementBranchScope(req: Requirement, branchContent?: string): Promise<BranchScope | null> {
  if (!req.reqDir) return null
  const structured = await readBranchScope(req.reqDir)
  if (structured) return structured
  if (!branchContent) return null
  const repos = fallbackFromBranchMd(branchContent)
  if (repos.length === 0) return null
  return { version: 1, updatedAt: req.updatedAt, repos, fallback: true }
}

app.get("/requirement/review", async (c) => {
  const id = c.req.query("id")
  if (!id) return c.text("Missing requirement id", 400)
  const req = await getRequirement(id)
  if (!req) return c.text("Requirement not found", 404)
  if (!req.reqDir) return c.text("Requirement has no on-disk directory", 400)
  const branchContent = await readOptionalRequirementFile(req.branchPath)
  const [scope, snapshot] = await Promise.all([
    loadRequirementBranchScope(req, branchContent),
    readCodeReviewSnapshot(req.reqDir),
  ])
  return c.html(<RequirementReviewPage req={req} scope={scope} snapshot={snapshot} />)
})

app.get("/requirement/release", async (c) => {
  const id = c.req.query("id")
  if (!id) return c.text("Missing requirement id", 400)
  const req = await getRequirement(id)
  if (!req) return c.text("Requirement not found", 404)
  if (!req.reqDir) return c.text("Requirement has no on-disk directory", 400)
  const [metaContent, branchContent, configContent, testContent, notesContent, reviewContent] = await Promise.all([
    readOptionalRequirementFile(req.metaPath),
    readOptionalRequirementFile(req.branchPath),
    readOptionalRequirementFile(req.configPath),
    readOptionalRequirementFile(req.testPath),
    readOptionalRequirementFile(req.notesPath),
    readOptionalRequirementFile(req.reviewPath),
  ])
  const checklist = buildReleaseChecklist({
    meta: metaContent,
    branch: branchContent,
    config: configContent,
    test: testContent,
    notes: notesContent,
    review: reviewContent,
  })
  return c.html(
    <RequirementReleasePage
      req={req}
      checklist={checklist}
      branchContent={branchContent}
      configContent={configContent}
      testContent={testContent}
      notesContent={notesContent}
      reviewContent={reviewContent}
    />,
  )
})

app.get("/requirement", async (c) => {
  const id = c.req.query("id")
  if (!id) return c.text("Missing requirement id", 400)
  const req = await getRequirement(id)
  if (!req) return c.text("Requirement not found", 404)
  const { harness } = await getConfig()

  // Do NOT auto-create a session here, even when the requirement is in
  // an active stage with no bound sessions. The detail page renders an
  // explicit "新建" / "关联已有 Session" choice and only acts on a
  // direct user submit.

  const readFileSafe = async (path?: string): Promise<string | undefined> => {
    if (!path || !existsSync(path)) return undefined
    try {
      const raw = await readFile(path, "utf-8")
      return raw
    } catch {
      return undefined
    }
  }
  const [backgroundContent, alignmentContent, prdContent, branchContent, notesContent, testContent, configContent, impactContent, memoryContent, reviewContent] = await Promise.all([
    readFileSafe(req.backgroundPath),
    readFileSafe(req.alignmentPath),
    readFileSafe(req.prdPath),
    readFileSafe(req.branchPath),
    readFileSafe(req.notesPath),
    readFileSafe(req.testPath),
    readFileSafe(req.configPath),
    readFileSafe(req.impactPath),
    readFileSafe(req.memoryPath),
    readFileSafe(req.reviewPath),
  ])
  const impactAssessment = buildImpactAssessment(impactContent)
  const codeReviewSnapshot = req.reqDir ? await readCodeReviewSnapshot(req.reqDir) : null
  const attachments = req.reqDir ? await listAttachments(req.reqDir) : []

  // Branch scope: prefer the authoritative `branches.json`; fall back to a
  // best-effort parse of `branch.md` so pre-existing requirements still
  // get an overview. `fallback: true` flags the heuristic source to the UI.
  let branchScope: BranchScope | null = null
  if (req.reqDir) {
    const fromJson = await readBranchScope(req.reqDir)
    if (fromJson) {
      branchScope = fromJson
    } else if (branchContent) {
      branchScope = {
        version: 1,
        updatedAt: req.updatedAt,
        repos: fallbackFromBranchMd(branchContent),
        fallback: true,
      }
    }
  }

  const [sessions, associated] = await Promise.all([
    scanDashboardSessions(harness),
    req.sessionIds.length > 0 ? getDashboardSessionsByIds(harness, req.sessionIds) : Promise.resolve([]),
  ])
  const associatedAll = await getAllAssociatedSessionIds()
  const unassociated = sessions.filter(
    (s) =>
      !s.parentId &&
      !FORK_TITLE_RE.test(s.title || "") &&
      !associatedAll.has(s.id) &&
      !req.sessionIds.includes(s.id)
  )
  const state = req.reqDir ? await readRequirementState(req.reqDir) : null

  const recommendations = req.id !== DEFAULT_REQ_ID
    ? recommendSessionsForRequirement(req, unassociated, 6)
    : []
  const extractHistory = req.id !== DEFAULT_REQ_ID
    ? await getExtractHistoryForRequirement(req.id, 6)
    : []

  return c.html(
    <RequirementDetailPage
      req={req}
      associated={associated}
      unassociated={unassociated}
      backgroundContent={backgroundContent}
      alignmentContent={alignmentContent}
      prdContent={prdContent}
      branchContent={branchContent}
      notesContent={notesContent}
      testContent={testContent}
      configContent={configContent}
      impactContent={impactContent}
      memoryContent={memoryContent}
      reviewContent={reviewContent}
      impactAssessment={impactAssessment}
      branchScope={branchScope}
      codeReviewSnapshot={codeReviewSnapshot}
      state={state}
      recommendations={recommendations}
      extractHistory={extractHistory}
      attachments={attachments}
    />
  )
})

const AUTO_DRIVE_TIMEOUT_MS = 60 * 60 * 1000

async function launchRequirementAutoDrive(req: Requirement): Promise<AutoDriveJob> {
  if (!req.reqDir) throw new Error("Requirement has no on-disk directory")
  const sessionId = randomUUID()
  const actionHref = `/requirement?id=${encodeURIComponent(req.id)}`
  const name = buildAutoDriveJobName(req)
  const ctx = await buildInjectionContext(req.id)
  const ctxFile = await writeInjectionContext(sessionId, ctx)
  try { await associateSession(req.id, sessionId) } catch { /* best effort */ }

  const notificationId = createNotification({
    type: "system",
    title: `自动推进：${req.title}`,
    subtitle: "已加入 pi agent 自动推进队列。",
    state: "running",
    reqId: req.id,
    sessionId,
    actionHref,
  })
  const job = createAutoDriveJob(req, sessionId, notificationId)
  const env = await buildManagedEnv()

  void runQueuedOpencodeProcess({
    bin: "pi",
    args: ["--session-id", sessionId, "--name", name, "--append-system-prompt", ctxFile, "-p", buildAutoDrivePrompt(req)],
    spawnOptions: { stdio: ["ignore", "pipe", "pipe"], cwd: req.reqDir },
    env,
    timeoutMs: AUTO_DRIVE_TIMEOUT_MS,
    onQueued: (position) => {
      updateAutoDriveJob(job.id, { state: "queued", summary: `等待 pi agent 执行（队列位置 ${position}）。` })
      updateNotification(notificationId, { subtitle: `等待 pi agent 执行（队列位置 ${position}）`, actionHref })
    },
    onSpawn: () => {
      updateAutoDriveJob(job.id, { state: "running", startedAt: Date.now(), summary: "pi agent 正在自动推进需求。" })
      updateNotification(notificationId, { subtitle: "pi agent 正在自动推进需求。", actionHref })
    },
  }).then((result) => {
    const final = finalizeAutoDriveJobFromResult(job.id, result)
    if (!final) return
    if (final.state === "blocked") {
      updateNotification(notificationId, {
        title: `自动推进需要人工确认：${req.title}`,
        subtitle: final.blockers.slice(0, 3).join("；") || final.summary,
        state: "failed",
        actionHref,
      })
    } else if (final.state === "failed") {
      updateNotification(notificationId, {
        title: `自动推进失败：${req.title}`,
        subtitle: final.summary,
        state: "failed",
        actionHref,
      })
    } else {
      updateNotification(notificationId, {
        title: `自动推进完成：${req.title}`,
        subtitle: final.summary,
        state: "done",
        actionHref,
      })
    }
  }).catch((err: unknown) => {
    const message = err instanceof Error ? err.message : String(err)
    updateAutoDriveJob(job.id, {
      state: "failed",
      summary: `启动或运行 pi agent 失败：${message}`,
      blockers: ["自动推进进程启动失败，请检查 pi 命令和模型配置。"],
      stderr: message,
      doneAt: Date.now(),
    })
    updateNotification(notificationId, {
      title: `自动推进失败：${req.title}`,
      subtitle: message,
      state: "failed",
      actionHref,
    })
  })

  return job
}

/**
 * POST /api/requirement/new-session
 *
 * Spawn a detached background process that creates a new session for the
 * requirement, then poll the session store for the new id. Once we have
 * it, associate the new session with the requirement and return
 * `{ sessionId, command }` as JSON.
 *
 * OpenCode: spawn `opencode run "<injection-context>" --title "<title>"` and
 * poll for the new id (15s timeout).
 * Pi:       pre-assign a UUID session-id, return
 * `pi --session-id <id> --name "<title>"` immediately (no spawn, no wait);
 * the session is created when the user runs the command.
 *
 * The user copies the returned command and pastes it into their terminal.
 *
 * Errors:
 *   400 - missing reqId
 *   404 - requirement not found
 *   504 - harness did not register a new session within 15s
 */
app.post("/api/requirement/new-session", async (c) => {
  const form = await c.req.formData()
  const reqId = String(form.get("reqId") || "")
  if (!reqId) return c.json({ error: "Missing reqId" }, 400)
  const req = await getRequirement(reqId)
  if (!req) return c.json({ error: "Requirement not found" }, 404)

  const { harness } = await getConfig()
  const title = req.title || reqId

  if (harness === "pi") {
    // Pi: return a copyable command immediately. Pre-assign a UUID
    // session-id so the requirement binding is recorded now; when the
    // user runs the command, pi creates the session with this exact id
    // and the dashboard scan picks it up. No background spawn, no wait.
    const sessionId = randomUUID()
    const name = `${req.id} ${title}`.slice(0, 100)
    const ctx = await buildInjectionContext(reqId)
    const ctxFile = await writeInjectionContext(sessionId, ctx)
    try { await associateSession(reqId, sessionId) } catch { /* noop */ }
    return c.json({
      sessionId,
      command: `pi --session-id ${sessionId} --name ${JSON.stringify(name)} --append-system-prompt ${ctxFile}`,
    })
  }

  // OpenCode: spawn `opencode run "<ctx>" --title <title>` as a detached
  // background process, then poll for the new session id.
  const ctx = await buildInjectionContext(reqId)
  const startMs = Date.now()
  const env = await buildManagedEnv()
  void runQueuedOpencodeProcess({
    bin: "opencode",
    args: ["run", ctx, "--title", title],
    spawnOptions: { stdio: ["ignore", "pipe", "pipe"] },
    env,
  }).catch(() => {})

  // Poll for the newly created session id. clearDashboardSessionCache forces
  // the next scan to re-read the store so we see the new row the moment the
  // harness commits it.
  clearDashboardSessionCache(harness)
  const deadline = Date.now() + 15_000
  let sessionId = ""
  while (!sessionId && Date.now() < deadline) {
    await new Promise((r) => setTimeout(r, 500))
    clearDashboardSessionCache(harness)
    const list = await scanDashboardSessions(harness, true)
    const candidate = list.find(
      (s) => (s.created || 0) >= startMs,
    )
    if (candidate) {
      sessionId = candidate.id
      break
    }
  }

  if (!sessionId) {
    return c.json(
      { error: `Session creation timed out - ${harnessLabel(harness)} may still be starting. Check the sessions list in a moment.` },
      504,
    )
  }

  // Best-effort association: do not fail the response if persistence
  // hiccups; the user can re-bind manually.
  try {
    await associateSession(reqId, sessionId)
  } catch { /* noop */ }

  return c.json({ sessionId, command: buildResumeCommand(harness, sessionId) })
})

app.get("/api/requirement/auto-drive", async (c) => {
  const reqId = String(c.req.query("reqId") || "")
  const jobs = getAutoDriveJobs(reqId && reqId !== DEFAULT_REQ_ID ? { reqId } : {})
  return c.json({
    jobs,
    active: jobs.filter((job) => job.state === "queued" || job.state === "running").length,
    blocked: jobs.filter((job) => job.state === "blocked" || job.state === "failed").length,
    queue: getOpencodeProcessQueueStatus(),
  })
})

app.post("/api/requirement/auto-drive", async (c) => {
  const contentType = c.req.header("content-type") || ""
  let reqIds: string[] = []
  if (contentType.includes("application/json")) {
    const body = await c.req.json().catch(() => null) as Record<string, unknown> | null
    const raw = body?.reqIds
    reqIds = Array.isArray(raw) ? raw.map((x) => String(x)) : []
  } else {
    const form = await c.req.formData()
    reqIds = form.getAll("reqIds").map((x) => String(x))
    const single = String(form.get("reqId") || "")
    if (single) reqIds.push(single)
  }
  reqIds = [...new Set(reqIds.map((x) => x.trim()).filter(Boolean))]
  if (reqIds.length === 0) return c.json({ error: "Missing reqIds" }, 400)

  const jobs: AutoDriveJob[] = []
  const errors: Array<{ reqId: string; error: string }> = []
  for (const reqId of reqIds) {
    const req = await getRequirement(reqId)
    if (!req) {
      errors.push({ reqId, error: "Requirement not found" })
      continue
    }
    if (!req.reqDir || req.id === DEFAULT_REQ_ID) {
      errors.push({ reqId, error: "Requirement has no on-disk directory" })
      continue
    }
    const existing = getLatestAutoDriveJobForRequirement(req.id)
    if (existing && (existing.state === "queued" || existing.state === "running")) {
      jobs.push(existing)
      continue
    }
    try {
      jobs.push(await launchRequirementAutoDrive(req))
    } catch (err) {
      const message = err instanceof Error ? err.message : String(err)
      errors.push({ reqId, error: message })
    }
  }
  return c.json({ ok: errors.length === 0, jobs, errors })
})

app.post("/api/requirement/associate", async (c) => {
  const form = await c.req.formData()
  const reqId = String(form.get("reqId") || "")
  const raw = String(form.get("sessionId") || "")
  if (!reqId || !raw) return c.text("Missing reqId or sessionId", 400)
  const { harness } = await getConfig()
  // Extract a session id from the input value. The datalist-backed search
  // field stores values like "<id> - title …" so users can search by either
  // id prefix or title fragment; we accept either as long as a valid session
  // id for the active harness appears anywhere in the string.
  const sessionId = extractDashboardSessionId(harness, raw)
  if (!isValidDashboardSessionId(harness, sessionId)) {
    return c.text(`Invalid session id: ${raw}`, 400)
  }
  const exists = await getRequirement(reqId)
  if (!exists) return c.text("Requirement not found", 404)
  await associateSession(reqId, sessionId)
  // Support React/XHR callers that prefer JSON over a redirect.
  const accept = c.req.header("accept") || ""
  if (accept.includes("application/json")) {
    return c.json({ ok: true, reqId, sessionId })
  }
  return c.redirect(`/requirement?id=${encodeURIComponent(reqId)}`, 303)
})

/**
 * POST /api/requirement/dissociate
 * Body: reqId, sessionId
 *
 * Removes a session from a requirement's association list. The session
 * becomes an orphan (visible in the default requirement) unless re-associated
 * elsewhere. Used by the "解除绑定" buttons on the requirement detail page.
 */
app.post("/api/requirement/dissociate", async (c) => {
  const form = await c.req.formData()
  const reqId = String(form.get("reqId") || "")
  const sessionId = String(form.get("sessionId") || "")
  if (!reqId || !sessionId) return c.text("Missing reqId or sessionId", 400)
  const { harness } = await getConfig()
  if (!isValidDashboardSessionId(harness, sessionId)) {
    return c.text("Invalid session id", 400)
  }
  const exists = await getRequirement(reqId)
  if (!exists) return c.text("Requirement not found", 404)
  await dissociateSession(reqId, sessionId)
  return c.redirect(`/requirement?id=${encodeURIComponent(reqId)}`, 303)
})

/**
 * Shared validation for both job-start and preview routes.
 *
 * Returns either a ready-to-use {req, sessionId} pair or an HTTP error
 * carrying the appropriate 4xx text. The session must already be
 * associated with the requirement — otherwise any caller could spam any
 * requirement's notes.md with any session's summary.
 */
async function resolveExtractTarget(
  reqId: string,
  sessionId: string,
): Promise<{ ok: true; req: Requirement } | { ok: false; status: 400 | 403 | 404; message: string }> {
  if (!reqId || !sessionId) return { ok: false, status: 400, message: "Missing reqId or sessionId" }
  if (!isValidSessionId(sessionId)) return { ok: false, status: 400, message: "Invalid sessionId" }
  const req = await getRequirement(reqId)
  if (!req) return { ok: false, status: 404, message: "Requirement not found" }
  if (!req.sessionIds.includes(sessionId)) {
    return { ok: false, status: 403, message: "Session is not associated with this requirement" }
  }
  if (!req.reqDir) {
    return { ok: false, status: 400, message: "This requirement has no on-disk directory; cannot extract." }
  }
  return { ok: true, req }
}

/**
 * Serialize an `ExtractJob` for the polling endpoint.
 *
 * We do NOT include the full stdout/stderr while the job is still
 * running (they're empty anyway) and we clip stderr to 2KB on the
 * client-facing payload so a runaway opencode log can't bloat polling.
 */
function jobToJson(j: ExtractJob): Record<string, unknown> {
  return {
    id: j.id,
    reqId: j.reqId,
    sessionId: j.sessionId,
    state: j.state,
    mode: j.mode,
    model: j.model,
    startedAt: j.startedAt,
    doneAt: j.doneAt,
    exitCode: j.exitCode,
    timedOut: j.timedOut,
    errorMessage: j.errorMessage,
    stdoutLength: j.stdout.length,
    stderrSnippet: j.stderr.slice(0, 2048),
    elapsedMs: (j.doneAt ?? Date.now()) - j.startedAt,
    // Fork-salvage hints surfaced to the toast / preview page.
    forkSessionId: j.forkSessionId,
    forkTitle: j.forkTitle,
    salvagedFromFork: j.salvagedFromFork,
    // Auto-extract result summary (file counts only; full content
    // is on the preview page).
    autoFileCount: j.autoResult
      ? j.autoResult.updates.length + j.autoResult.appends.length
      : 0,
  }
}

/**
 * POST /api/requirement/extract-context
 * Body: reqId, sessionId
 *
 * Kicks off a background extract job and returns 202 with `{ jobId }`.
 * If a job for the same sessionId is already in-flight, returns 409
 * with the existing jobId so the UI can re-attach.
 */
app.post("/api/requirement/extract-context", async (c) => {
  const form = await c.req.formData()
  const reqId = String(form.get("reqId") || "")
  const sessionId = String(form.get("sessionId") || "")
  const guard = await resolveExtractTarget(reqId, sessionId)
  if (!guard.ok) return c.text(guard.message, guard.status)
  const prompt = buildExtractPrompt(guard.req)
  const cfg = await getConfig()
  try {
    const job = createExtractJob({ reqId, sessionId, prompt, model: cfg.extractModel })
    return c.json({ jobId: job.id, state: job.state }, 202)
  } catch (err) {
    if (err instanceof JobConflictError) {
      return c.json({ error: "conflict", jobId: err.existingJobId }, 409)
    }
    const msg = err instanceof Error ? err.message : String(err)
    return c.text(`Failed to start job: ${msg}`, 500)
  }
})

/**
 * GET /api/extract/job/:id
 * Returns the current job snapshot as JSON. 404 if missing or evicted.
 */
app.get("/api/extract/job/:id", (c) => {
  const id = c.req.param("id")
  const job = getExtractJob(id)
  if (!job) return c.json({ error: "not found" }, 404)
  return c.json(jobToJson(job))
})

/**
 * GET /requirement/extract?jobId=<id>
 *   or
 * GET /requirement/extract?reqId=<r>&sessionId=<s>
 *
 * Renders the preview page using a completed job's stdout. The two
 * accepted query shapes:
 *   - jobId  : the toast on the detail page links here after polling
 *     reports state ∈ {done, failed}.
 *   - reqId+sessionId : back-compat / direct deep link. If a finished
 *     job for this (sid) is in the store, we use it; if a running one
 *     exists, we render a "still working" preview that auto-redirects
 *     once it finishes (handled client-side). If neither, we surface a
 *     "no job" failure card with a "start one" button.
 */
app.get("/requirement/extract", async (c) => {
  const jobIdParam = c.req.query("jobId")
  let job: ExtractJob | null = null

  if (jobIdParam) {
    job = getExtractJob(jobIdParam)
    if (!job) return c.text("Job not found or expired", 404)
  } else {
    const reqId = String(c.req.query("reqId") || "")
    const sessionId = String(c.req.query("sessionId") || "")
    const guard = await resolveExtractTarget(reqId, sessionId)
    if (!guard.ok) return c.text(guard.message, guard.status)
    job = findRunningJobForSession(sessionId)
    // If none in-flight, we don't auto-spawn — the user is expected to
    // arrive here only via the toast. Render a minimal "no job" card
    // with a back link.
    if (!job) {
      return c.html(
        <RequirementExtractPreviewPage
          req={guard.req}
          sessionId={sessionId}
          job={null}
        />,
      )
    }
  }

  const req = await getRequirement(job.reqId)
  if (!req) return c.text("Requirement not found", 404)

  return c.html(
    <RequirementExtractPreviewPage
      req={req}
      sessionId={job.sessionId}
      job={job}
    />,
  )
})

app.get("/requirement/recall", async (c) => {
  const reqId = String(c.req.query("reqId") || "")
  const sessionId = String(c.req.query("sessionId") || "")
  const guard = await resolveExtractTarget(reqId, sessionId)
  if (!guard.ok) return c.text(guard.message, guard.status)
  const parts = await readSessionTranscript({ sessionId, limitParts: 240, maxTextChars: 6_000 })
  const markdown = buildRecallMarkdown(parts)
  return c.html(<RequirementRecallPage req={guard.req} sessionId={sessionId} markdown={markdown} partCount={parts.length} />)
})

app.get("/api/session/transcript", async (c) => {
  const sessionId = String(c.req.query("id") || "")
  if (!isValidSessionId(sessionId)) return c.json({ error: "Invalid session id" }, 400)
  const parts = await readSessionTranscript({ sessionId })
  return c.json({ sessionId, parts, markdown: buildRecallMarkdown(parts) })
})

// POST /api/requirement/extract-context/commit
// Append the (user-edited) summary body to <reqDir>/notes.md and
// redirect back to the requirement page. Same validation as GET above.
app.post("/api/requirement/extract-context/commit", async (c) => {
  const form = await c.req.formData()
  const reqId = String(form.get("reqId") || "")
  const sessionId = String(form.get("sessionId") || "")
  const body = String(form.get("body") || "")
  const guard = await resolveExtractTarget(reqId, sessionId)
  if (!guard.ok) return c.text(guard.message, guard.status)
  if (!body.trim()) return c.text("Body is empty; refusing to commit.", 400)
  const notesPath = guard.req.notesPath ?? join(guard.req.reqDir!, "notes.md")
  try {
    await appendSummaryToNotes(notesPath, sessionId, body)
  } catch (err) {
    const msg = err instanceof Error ? err.message : String(err)
    return c.text(`Failed to write notes.md: ${msg}`, 500)
  }
  return c.redirect(`/requirement?id=${encodeURIComponent(reqId)}`, 303)
})

// Switch a requirement's status. Writes <reqDir>/state.json atomically
// and appends a history entry. Refuses synthetic / non-Hermes requirements
// (DEFAULT_REQ_ID has no reqDir).
app.post("/api/requirement/status", async (c) => {
  const form = await c.req.formData()
  const reqId = String(form.get("reqId") || "")
  const rawStatus = String(form.get("status") || "")
  const note = String(form.get("note") || "")
  const redirectBack = String(form.get("redirect") || "") || `/requirement?id=${encodeURIComponent(reqId)}`
  if (!reqId) return c.text("Missing reqId", 400)
  if (!(REQ_STATUSES as readonly string[]).includes(rawStatus)) {
    return c.text(`Invalid status: ${rawStatus}`, 400)
  }
  const status = rawStatus as ReqStatus
  const req = await getRequirement(reqId)
  if (!req) return c.text("Requirement not found", 404)
  if (!req.reqDir) {
    return c.text("Requirement has no on-disk directory (synthetic default cannot be updated)", 400)
  }
  try {
    await writeRequirementStatus(req.reqDir, status, note || undefined)
  } catch (err) {
    const message = err instanceof Error ? err.message : String(err)
    return c.text(`Failed to write state: ${message}`, 500)
  }
  // Tolerate fetch/XHR callers that prefer JSON; default to redirect.
  const accept = c.req.header("accept") || ""
  if (accept.includes("application/json")) {
    return c.json({ ok: true, status })
  }
  return c.redirect(redirectBack, 303)
})

/**
 * POST /api/requirement/category
 * Sets the requirement category ("需求" | "线上问题"). When switched to
 * "线上问题", pre-development statuses are auto-advanced to "开发中".
 * Mirrors the status endpoint's JSON/redirect dual-response contract.
 */
app.post("/api/requirement/category", async (c) => {
  const form = await c.req.formData()
  const reqId = String(form.get("reqId") || "")
  const rawCategory = String(form.get("category") || "")
  const redirectBack = String(form.get("redirect") || "") || `/requirement?id=${encodeURIComponent(reqId)}`
  if (!reqId) return c.text("Missing reqId", 400)
  if (!(REQ_CATEGORIES as readonly string[]).includes(rawCategory)) {
    return c.text(`Invalid category: ${rawCategory}`, 400)
  }
  const category = rawCategory as ReqCategory
  const req = await getRequirement(reqId)
  if (!req) return c.text("Requirement not found", 404)
  if (!req.reqDir) {
    return c.text("Requirement has no on-disk directory (synthetic default cannot be updated)", 400)
  }
  try {
    const state = await writeRequirementCategory(req.reqDir, category)
    const accept = c.req.header("accept") || ""
    if (accept.includes("application/json")) {
      return c.json({ ok: true, category: state.category, status: state.status })
    }
    return c.redirect(redirectBack, 303)
  } catch (err) {
    const message = err instanceof Error ? err.message : String(err)
    return c.text(`Failed to write category: ${message}`, 500)
  }
})

/**
 * POST /api/requirement/code-review/scan
 * Refreshes the production base branch in every scoped repo before building
 * a persisted diff snapshot for human review.
 */
app.post("/api/requirement/code-review/scan", async (c) => {
  const form = await c.req.formData()
  const reqId = String(form.get("reqId") || "")
  const baseRef = String(form.get("baseRef") || DEFAULT_CODE_REVIEW_BASE_REF).trim() || DEFAULT_CODE_REVIEW_BASE_REF
  const redirectBack = String(form.get("redirect") || "") || `/requirement?id=${encodeURIComponent(reqId)}`
  if (!reqId) return c.text("Missing reqId", 400)
  if (/\s/.test(baseRef) || baseRef.length > 120) return c.text("Invalid baseRef", 400)
  const req = await getRequirement(reqId)
  if (!req) return c.text("Requirement not found", 404)
  if (!req.reqDir) return c.text("Requirement has no on-disk directory", 400)

  let scope = await readBranchScope(req.reqDir)
  if (!scope && req.branchPath && existsSync(req.branchPath)) {
    const branchMd = await readFile(req.branchPath, "utf-8").catch(() => "")
    const repos = fallbackFromBranchMd(branchMd)
    if (repos.length > 0) scope = { version: 1, updatedAt: Date.now(), repos, fallback: true }
  }
  if (!scope || scope.repos.length === 0) {
    return c.text("No branch scope found. Generate branches.json or fill branch.md first.", 400)
  }

  try {
    await runCodeReviewScan(req.reqDir, req.id, scope, { baseRef })
  } catch (err) {
    const msg = err instanceof Error ? err.message : String(err)
    return c.text(`Failed to scan code review diff: ${msg}`, 500)
  }
  return c.redirect(redirectBack, 303)
})

/**
 * POST /api/requirement/code-review/verdict
 * Saves the human review decision into code-review.json and mirrors a
 * managed summary block into review.md for release-checklist consumption.
 */
app.post("/api/requirement/code-review/verdict", async (c) => {
  const form = await c.req.formData()
  const reqId = String(form.get("reqId") || "")
  const rawStatus = String(form.get("status") || "not_started")
  const redirectBack = String(form.get("redirect") || "") || `/requirement?id=${encodeURIComponent(reqId)}`
  if (!reqId) return c.text("Missing reqId", 400)
  if (!CODE_REVIEW_STATUSES.includes(rawStatus as CodeReviewStatus)) {
    return c.text(`Invalid review status: ${rawStatus}`, 400)
  }
  const req = await getRequirement(reqId)
  if (!req) return c.text("Requirement not found", 404)
  if (!req.reqDir) return c.text("Requirement has no on-disk directory", 400)
  const snapshot = await readCodeReviewSnapshot(req.reqDir)
  if (!snapshot) return c.text("No code review snapshot. Run scan first.", 400)

  const items = String(form.get("items") || "")
    .split(/\r?\n/)
    .map((s) => s.trim().replace(/^[-*]\s+/, ""))
    .filter(Boolean)
  await saveCodeReviewVerdict(req.reqDir, snapshot, {
    status: rawStatus as CodeReviewStatus,
    reviewer: String(form.get("reviewer") || "").trim() || "未填写",
    summary: String(form.get("summary") || "").trim(),
    items,
    updatedAt: Date.now(),
  })
  return c.redirect(redirectBack, 303)
})

/**
 * POST /api/requirement/code-review/ai
 * Runs an AI code review against the existing diff snapshot + requirement
 * files, persists the Markdown suggestions into code-review.json, and
 * returns them as JSON for the review-page button. The model/endpoint/key
 * come from dashboard config (Settings page); the key is read server-side
 * only and never echoed back.
 */
app.post("/api/requirement/code-review/ai", async (c) => {
  const body = await c.req.json().catch(() => null) ?? {}
  const reqId = String(body.reqId || "")
  if (!reqId) return c.json({ ok: false, error: "Missing reqId" }, 400)
  const req = await getRequirement(reqId)
  if (!req) return c.json({ ok: false, error: "Requirement not found" }, 404)
  if (!req.reqDir) return c.json({ ok: false, error: "Requirement has no on-disk directory" }, 400)
  const snapshot = await readCodeReviewSnapshot(req.reqDir)
  if (!snapshot) return c.json({ ok: false, error: "请先点击「刷新 PRO Diff」生成代码差异，再进行 AI 审查。" }, 400)
  if (snapshot.repos.every((r) => !r.diff && r.files.length === 0)) {
    return c.json({ ok: false, error: "当前代码差异为空，无可审查内容。" }, 400)
  }

  const cfg = await getConfig()
  if (!cfg.codeReviewBaseUrl.trim() || !cfg.codeReviewModel.trim() || !cfg.codeReviewApiKey.trim()) {
    return c.json({ ok: false, error: "尚未配置 AI 代码审查模型，请在 Settings 页面填写 Base URL / API Key / Model。" }, 400)
  }

  const aiReview = await runAiCodeReview(req.reqDir, snapshot, {
    baseUrl: cfg.codeReviewBaseUrl,
    apiKey: cfg.codeReviewApiKey,
    model: cfg.codeReviewModel,
  })
  if (aiReview.error) {
    // Still persist a failed attempt so the UI can surface the error.
    await saveCodeReviewAiResult(req.reqDir, snapshot, aiReview)
    return c.json({ ok: false, error: aiReview.error, aiReview }, 200)
  }
  const next = await saveCodeReviewAiResult(req.reqDir, snapshot, aiReview)
  return c.json({ ok: true, aiReview: next.aiReview })
})

/**
 * POST /api/requirement/impact-template
 * Creates `impact.md` with the standard pre-coding safety template.
 * Existing files are preserved by appending only missing template sections.
 */
app.post("/api/requirement/impact-template", async (c) => {
  const form = await c.req.formData()
  const reqId = String(form.get("reqId") || "")
  if (!reqId) return c.text("Missing reqId", 400)
  const req = await getRequirement(reqId)
  if (!req) return c.text("Requirement not found", 404)
  if (!req.reqDir) return c.text("Requirement has no on-disk directory", 400)

  const impactPath = join(req.reqDir, IMPACT_FILE)
  const existing = existsSync(impactPath) ? await readFile(impactPath, "utf-8").catch(() => "") : ""
  const current = buildImpactAssessment(existing)
  const missingSections = current.missingSections

  if (!existing.trim()) {
    await writeFile(impactPath, IMPACT_TEMPLATE, "utf-8")
  } else if (missingSections.length > 0) {
    const templateSections = buildImpactAssessment(IMPACT_TEMPLATE).sections
    const additions = missingSections
      .map((section) => `## ${section}\n${templateSections[section] ?? "- 待补充"}`)
      .join("\n\n")
    await appendFile(impactPath, `\n\n${additions}\n`, "utf-8")
  }

  return c.redirect(`/requirement?id=${encodeURIComponent(reqId)}`, 303)
})

/**
 * POST /api/requirement/alignment-template
 * Creates the business-only alignment.md and PRD source-trace template.
 * Existing files are preserved; templates are appended only when absent.
 */
app.post("/api/requirement/alignment-template", async (c) => {
  const form = await c.req.formData()
  const reqId = String(form.get("reqId") || "")
  if (!reqId) return c.text("Missing reqId", 400)
  const req = await getRequirement(reqId)
  if (!req) return c.text("Requirement not found", 404)
  if (!req.reqDir) return c.text("Requirement has no on-disk directory", 400)

  const alignmentPath = join(req.reqDir, ALIGNMENT_FILE)
  const prdPath = join(req.reqDir, PRD_FILE)
  const existingAlignment = existsSync(alignmentPath) ? await readFile(alignmentPath, "utf-8").catch(() => "") : ""
  const existingPrd = existsSync(prdPath) ? await readFile(prdPath, "utf-8").catch(() => "") : ""
  if (!existingAlignment.trim()) {
    await writeFile(alignmentPath, ALIGNMENT_TEMPLATE, "utf-8")
  } else if (!existingAlignment.includes("## 9. PRD 转化记录")) {
    await appendFile(alignmentPath, "\n\n" + ALIGNMENT_TEMPLATE.split("## 9. PRD 转化记录")[1]!.replace(/^/, "## 9. PRD 转化记录") + "\n", "utf-8")
  }
  if (!existingPrd.trim()) {
    await writeFile(prdPath, PRD_TEMPLATE, "utf-8")
  }

  return c.redirect(`/requirement?id=${encodeURIComponent(reqId)}`, 303)
})

// ---------------------------------------------------------------------------
// Schedulers page
// ---------------------------------------------------------------------------

const DEVELOPER_ROOT = join(process.env.HOME || "", "Developer")

async function listDeveloperGithubRepos(): Promise<{ path: string; label: string; remote: string }[]> {
  const roots = ["github", "personal", "infra", "playground", "tools"].map((name) => join(DEVELOPER_ROOT, name))
  const repos: { path: string; label: string; remote: string }[] = []
  for (const root of roots) {
    if (!existsSync(root)) continue
    const entries = await readdir(root, { withFileTypes: true }).catch(() => [])
    for (const entry of entries) {
      if (!entry.isDirectory()) continue
      const repoPath = join(root, entry.name)
      if (!existsSync(join(repoPath, ".git"))) continue
      const remote = await readGitRemote(repoPath)
      if (!remote.includes("github.com")) continue
      repos.push({
        path: repoPath,
        label: repoPath.startsWith(`${DEVELOPER_ROOT}/`) ? repoPath.slice(DEVELOPER_ROOT.length + 1) : repoPath,
        remote,
      })
    }
  }
  return repos.sort((a, b) => a.label.localeCompare(b.label))
}

function readGitRemote(repoPath: string): Promise<string> {
  return new Promise((resolveRemote) => {
    const child = spawn("git", ["-C", repoPath, "remote", "get-url", "origin"], { stdio: ["ignore", "pipe", "ignore"] })
    let out = ""
    child.stdout?.on("data", (d: Buffer) => { out += d.toString("utf-8") })
    child.on("close", () => resolveRemote(out.trim()))
    child.on("error", () => resolveRemote(""))
  })
}

const SchedulerConfigPanel: FC<{ config: AppConfig; githubRepos: { path: string; label: string; remote: string }[] }> = ({ config, githubRepos }) => (
  <section class="sched-config-panel" aria-label="定时任务配置">
    <div class="sched-config-panel-head">
      <div>
        <p class="sched-eyebrow">CONFIGURATION</p>
        <h2 class="op-section-title">定时任务设置</h2>
        <p class="muted small">集中配置全量同步、智能提取和 session 价值发现；Schedulers 面板仅展示任务运行状态。</p>
      </div>
    </div>

    <form id="config-form" class="sched-config-form">
      <div class="sched-config-grid">
        <article class="sched-config-card">
          <div class="sched-config-card-top">
            <span class="sched-config-pill">SYNC</span>
            <div>
              <h3>配置 / GitHub 全量同步</h3>
              <p class="muted small">按配置时间运行 <code>sync-all-to-github.sh</code>，同步所有自有仓库到 GitHub；选中的第三方仓库追加 <code>git pull</code>。</p>
            </div>
          </div>
          <label class="sched-switch-row">
            <span>
              <strong>启用全量同步</strong>
              <small>执行 <code>sync-all-to-github.sh</code>，自更新脚本后同步所有自有仓库（Developer、ai-code-config、workstation-bootstrap、personal/playground/tools）。选中仓库追加 <code>git pull</code>。</small>
            </span>
            <input type="checkbox" name="fullSyncSchedule" id="cfg-full-sync-schedule" checked={config.fullSyncSchedule} />
          </label>
          <label class="settings-field">
            <span class="settings-label">同步时间</span>
            <input type="text" id="cfg-full-sync-times" name="fullSyncTimes" value={(config.fullSyncTimes?.length ? config.fullSyncTimes : [...DEFAULT_FULL_SYNC_TIMES]).join(", ")} class="settings-input" placeholder="12:00, 18:00, 20:30, 23:30" spellcheck={false} />
          </label>
          <div class="sched-github-repos">
            <span class="settings-label">同步 GitHub 仓库</span>
            <p class="muted small">勾选后在全量同步完成后对对应仓库执行 <code>git pull --ff-only</code>。</p>
            <div class="sched-repo-list">
              {githubRepos.length === 0 ? (
                <div class="sched-repo-empty muted small">未发现 <code>~/Developer</code> 下的 GitHub remote 仓库。</div>
              ) : githubRepos.map((repo) => (
                <label class="sched-repo-row">
                  <input type="checkbox" class="cfg-github-repo" value={repo.path} checked={config.fullSyncGithubRepos.includes(repo.path)} />
                  <span>
                    <strong>{repo.label}</strong>
                    <small>{repo.remote}</small>
                  </span>
                </label>
              ))}
            </div>
          </div>
        </article>

        <article class="sched-config-card sched-config-card-wide">
          <div class="sched-config-card-top">
            <span class="sched-config-pill">EXTRACT</span>
            <div>
              <h3>上下文提取策略</h3>
              <p class="muted small">控制 idle 自动提取和每天 00:00 的定时智能提取。</p>
            </div>
          </div>
          <div class="sched-switch-stack">
            <label class="sched-switch-row">
              <span>
                <strong>自动提取模式</strong>
                <small>关联 session idle 且消息增量超过阈值时触发。</small>
              </span>
              <input type="checkbox" name="autoExtract" id="cfg-auto-extract" checked={config.autoExtract} />
            </label>
            <label class="sched-switch-row">
              <span>
                <strong>定时智能提取</strong>
                <small>每天 00:00 扫描近 24 小时有变化的需求 session。</small>
              </span>
              <input type="checkbox" name="autoExtractSchedule" id="cfg-auto-extract-schedule" checked={config.autoExtractSchedule} />
            </label>
          </div>
          <div class="sched-config-controls">
            <label class="settings-field">
              <span class="settings-label">提取模型</span>
              <input type="text" id="cfg-model" name="extractModel" value={config.extractModel} class="settings-input" spellcheck={false} />
            </label>
            <label class="settings-field sched-config-number">
              <span class="settings-label">最小消息增量</span>
              <input type="number" id="cfg-min-change" name="minChangeMessages" value={String(config.minChangeMessages)} min={"1"} max={"100"} class="settings-input settings-input-narrow" />
            </label>
          </div>
        </article>

        <article class="sched-config-card">
          <div class="sched-config-card-top">
            <span class="sched-config-pill">VALUE</span>
            <div>
              <h3>Session 价值发现</h3>
              <p class="muted small">每 10 分钟扫描近 48h session，识别可沉淀经验。</p>
            </div>
          </div>
          <label class="sched-switch-row">
            <span>
              <strong>自动标记高价值 session</strong>
              <small>关闭时仅展示候选，不自动进入总结流程。</small>
            </span>
            <input type="checkbox" name="autoValuation" id="cfg-auto-valuation" checked={config.autoValuation} />
          </label>
          <label class="settings-field sched-config-number">
            <span class="settings-label">价值评分阈值</span>
            <input type="number" id="cfg-valuation-threshold" name="valuationThreshold" value={String(config.valuationThreshold)} min={"1"} max={"100"} class="settings-input settings-input-narrow" />
          </label>
        </article>
      </div>

      <div class="sched-config-actions">
        <button type="submit" class="btn btn-primary">保存定时任务设置</button>
        <span id="config-saved" class="settings-saved muted small" hidden>✓ 已保存</span>
      </div>
    </form>
  </section>
)

const SchedulersPage: FC<{
  schedulers: {
    name: string
    running: boolean
    pollIntervalMs: number | null
    pollIntervalLabel: string
    enabled: boolean
    description: string
    details: { label: string; value: string }[]
  }[]
  extractQueues: { reqId: string; queueLength: number; nextAvailableAt: number }[]
  valuationCandidates: { sessionId: string; score: number; reasons: string[]; signals: string[] }[]
  valuationStats: { lastPollAt: number | null; sessionsScanned: number; candidatesFound: number; threshold: number }
}> = ({ schedulers, extractQueues, valuationCandidates, valuationStats }) => {
  return (
    <Layout title="Schedulers" active="schedulers">
      <header class="op-section-head">
        <div>
          <h1 class="op-section-title">BACKGROUND SCHEDULERS</h1>
          <p class="muted small">展示后台任务运行状态、最近结果和队列信息；配置入口已移到 Settings。</p>
        </div>
        <div class="op-section-meta">
          <span class="op-section-meta-item">{schedulers.filter((s) => s.running).length} / {schedulers.length} RUNNING</span>
          <a class="op-section-meta-item" href="/settings">Settings</a>
        </div>
      </header>

      <div class="sched-list">
        {schedulers.map((s) => (
          <div class={`sched-card${s.running ? " sched-card-running" : ""}`}>
            <div class="sched-card-head">
              <span class={`sched-dot sched-dot-${s.running ? "on" : "off"}`}></span>
              <span class="sched-card-name">{s.name}</span>
              <span class="sched-card-status">{s.running ? "running" : "stopped"}</span>
            </div>
            <div class="sched-card-body">
              <p class="sched-card-desc muted small">{s.description}</p>
              <div class="sched-card-meta">
                <span class="sched-meta-item">间隔 <code>{s.pollIntervalLabel}</code></span>
                <span class="sched-meta-item">配置 <code>{s.enabled ? "enabled" : "disabled"}</code></span>
              </div>
              {s.details.length > 0 ? (
                <dl class="sched-card-details">
                  {s.details.map((d) => (
                    <div class="sched-detail-row">
                      <dt class="sched-detail-k muted small">{d.label}</dt>
                      <dd class="sched-detail-v">{d.value}</dd>
                    </div>
                  ))}
                </dl>
              ) : null}
            </div>
          </div>
        ))}
      </div>

      {valuationCandidates.length > 0 ? (
        <section class="sched-queues">
          <h2 class="op-section-title" style="font-size: 0.9rem; margin-top: 20px;">VALUATION CANDIDATES</h2>
          <p class="muted small">自动发现的高价值 session 候选（score ≥ {valuationStats.threshold}），点击 session ID 可查看详情。</p>
          <table class="sched-queue-table">
            <thead>
              <tr><th>Session</th><th>Score</th><th>Signals</th><th>Reasons</th><th>操作</th></tr>
            </thead>
            <tbody>
              {valuationCandidates.map((c) => (
                <tr>
                  <td><a href={`/session?id=${encodeURIComponent(c.sessionId)}`}><code>{c.sessionId.slice(0, 16)}…</code></a></td>
                  <td><strong>{c.score}</strong></td>
                  <td>{c.signals.join(", ")}</td>
                  <td class="muted small" style="max-width: 400px">{c.reasons.slice(0, 3).join("； ")}</td>
                  <td>
                    <form method="post" action="/api/valuation/mark" style="display:inline">
                      <input type="hidden" name="sessionId" value={c.sessionId} />
                      <button type="submit" class="btn btn-sm btn-primary">标记</button>
                    </form>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </section>
      ) : null}

      {extractQueues.length > 0 ? (
        <section class="sched-queues">
          <h2 class="op-section-title" style="font-size: 0.9rem; margin-top: 20px;">EXTRACT QUEUES</h2>
          <p class="muted small">同需求智能提取延时队列，每个需求排队中的任务间隔 5 分钟。</p>
          <table class="sched-queue-table">
            <thead>
              <tr><th>需求 ID</th><th>排队数</th><th>下一个可用时间</th></tr>
            </thead>
            <tbody>
              {extractQueues.map((q) => (
                <tr>
                  <td><code>{q.reqId}</code></td>
                  <td>{q.queueLength}</td>
                  <td>{q.nextAvailableAt > Date.now() ? formatRelAgo(q.nextAvailableAt) : "现在"}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </section>
      ) : null}
    </Layout>
  )
}

app.get("/schedulers", async (c) => {
  const cfg = await getConfig()
  const allMarkers = listMarkers()
  const processable = allMarkers.filter((m) => m.status === "marked")
  const summarizing = allMarkers.filter((m) => m.status === "summarizing")
  const summarized = allMarkers.filter((m) => m.status === "summarized")
  const failed = allMarkers.filter((m) => m.status === "failed")

  // Collect active extract queues from all requirements.
  const extractQueues: { reqId: string; queueLength: number; nextAvailableAt: number }[] = []

  const schedulers = [
    {
      name: "Developer 全量同步",
      running: isFullSyncSchedulerRunning(),
      pollIntervalMs: FULL_SYNC_POLL_MS,
      pollIntervalLabel: (cfg.fullSyncTimes?.length ? cfg.fullSyncTimes : [...DEFAULT_FULL_SYNC_TIMES]).join(", "),
      enabled: cfg.fullSyncSchedule,
      description: "按配置时间触发全量同步：自更新脚本 -> sync-all-to-github.sh（Developer + ai-code-config + workstation-bootstrap + 所有自有仓库），选中仓库追加 git pull。",
      details: (() => {
        const last = getLastFullSyncResult()
        return [
          { label: "配置开关", value: cfg.fullSyncSchedule ? "✅ fullSyncSchedule = true" : "❌ fullSyncSchedule = false" },
          { label: "同步时间", value: (cfg.fullSyncTimes?.length ? cfg.fullSyncTimes : [...DEFAULT_FULL_SYNC_TIMES]).join(", ") },
          { label: "GitHub 仓库", value: cfg.fullSyncGithubRepos.length > 0 ? `${cfg.fullSyncGithubRepos.length} 个已选择（同步后 pull）` : "未选择（仅同步自有仓库）" },
          { label: "上次结果", value: last ? (last.ok ? "success" : `failed: ${last.stderr || last.exitCode}`) : "本进程尚未执行" },
        ]
      })(),
    },
    {
      name: "定时智能提取",
      running: isAutoExtractSchedulerRunning(),
      pollIntervalMs: AUTO_EXTRACT_POLL_MS,
      pollIntervalLabel: "00:00",
      enabled: cfg.autoExtractSchedule,
      description: "每天本地 00:00 触发一次：只检查最近 24 小时内创建或更新过的需求 session；首次未提取或有新增内容时生成智能提取预览。",
      details: [
        { label: "配置开关", value: cfg.autoExtractSchedule ? "✅ autoExtractSchedule = true" : "❌ autoExtractSchedule = false" },
        { label: "提取模型", value: cfg.extractModel || "(default)" },
      ],
    },
    {
      name: "经验自动总结",
      running: isAutoSummaryWorkerRunning(),
      pollIntervalMs: 24 * 60 * 60 * 1000,
      pollIntervalLabel: "01:00",
      enabled: true,
      description: "每天本地 01:00 触发一次：只检查最近 24 小时内创建或更新过、且已空闲 ≥1 小时的已标记 session，自动 fork 生成经验报告。",
      details: [
        { label: "待处理标记", value: `${processable.length} 个（status=marked）` },
        { label: "总结中", value: `${summarizing.length} 个（status=summarizing）` },
        { label: "已完成", value: `${summarized.length} 个（status=summarized）` },
        { label: "失败", value: `${failed.length} 个（status=failed）` },
        { label: "总计标记", value: `${allMarkers.length} 个` },
      ],
    },
    {
      name: "智能提取延时队列",
      running: extractQueues.length > 0,
      pollIntervalMs: 5 * 60 * 1000,
      pollIntervalLabel: "on-demand",
      enabled: true,
      description: "同一需求的多个 session 智能提取按 5 分钟间隔排队执行，避免并发写入冲突。",
      details: extractQueues.length > 0
        ? extractQueues.map((q) => ({
            label: q.reqId,
            value: `${q.queueLength} 个排队中，下一个 ${q.nextAvailableAt > Date.now() ? formatRelAgo(q.nextAvailableAt) : "现在"}`,
          }))
        : [{ label: "状态", value: "空闲（无排队中的任务）" }],
    },
  ]

  // Valuation worker stats (always collected for display, even when disabled).
  const valStats = getValuationStats()
  const valCandidates = getRecentCandidates(10)

  const valuationScheduler = {
    name: "Session 价值发现",
    running: isAutoValuationWorkerRunning(),
    pollIntervalMs: VALUATION_POLL_MS,
    pollIntervalLabel: "10 min",
    enabled: cfg.autoValuation,
    description: "每 10 分钟扫描近 48h 的 session，通过元数据 + SQLite 内容两层评分识别有经验总结价值的 session（日志/DB 验证、skill 发现、经验纠错等）。开启后自动标记超阈值的 session 进入经验总结流程。",
    details: [
      { label: "自动标记", value: cfg.autoValuation ? "✅ autoValuation = true" : "❌ autoValuation = false（仅发现，不自动标记）" },
      { label: "阈值", value: `${cfg.valuationThreshold ?? 25}` },
      { label: "上次扫描", value: valStats.lastPollAt ? formatRelAgo(valStats.lastPollAt) : "未运行" },
      { label: "新扫描", value: `${valStats.sessionsScanned} 个` },
      { label: "内容评分", value: `${valStats.contentScored} 个` },
      { label: "候选发现", value: `${valStats.candidatesFound} 个` },
      { label: "已自动标记", value: `${valStats.autoMarked} 个` },
      { label: "已有标记跳过", value: `${valStats.alreadyMarked} 个` },
    ],
  }

  schedulers.push(valuationScheduler)

  return c.html(<SchedulersPage schedulers={schedulers} extractQueues={extractQueues} valuationCandidates={valCandidates} valuationStats={valStats} />)
})

// ---------------------------------------------------------------------------
// Settings page + config API
// ---------------------------------------------------------------------------

const CodeReviewConfigPanel: FC<{ config: AppConfig & { codeReviewApiKeySet: boolean } }> = ({ config }) => (
  <section class="sched-config-panel code-review-config-panel" aria-label="AI 代码审查配置">
    <div class="sched-config-panel-head">
      <div>
        <p class="sched-eyebrow">AI CODE REVIEW</p>
        <h2 class="op-section-title">AI 代码审查</h2>
        <p class="muted small">配置需求代码差异页面「AI 审查代码」使用的 OpenAI 兼容接口。API Key 仅保存在本地 config.json，不会回显到页面。</p>
      </div>
    </div>
    <form id="code-review-config-form" class="sched-config-form">
      <div class="sched-config-grid">
        <article class="sched-config-card sched-config-card-wide">
          <div class="sched-config-card-top">
            <span class="sched-config-pill">LLM</span>
            <div>
              <h3>模型接入</h3>
              <p class="muted small">Base URL 需为 OpenAI 兼容的 <code>/v1</code> 端点，例如 <code>https://api.deepseek.com/v1</code>；代码差异页面会调用 <code>{`{baseUrl}/chat/completions`}</code>。</p>
            </div>
          </div>
          <div class="sched-config-controls">
            <label class="settings-field">
              <span class="settings-label">Base URL</span>
              <input type="text" id="cfg-code-review-base" name="codeReviewBaseUrl" value={config.codeReviewBaseUrl} class="settings-input" placeholder="https://api.deepseek.com/v1" spellcheck={false} autocomplete="off" />
            </label>
            <label class="settings-field">
              <span class="settings-label">Model</span>
              <input type="text" id="cfg-code-review-model" name="codeReviewModel" value={config.codeReviewModel} class="settings-input" placeholder="deepseek-chat" spellcheck={false} autocomplete="off" />
            </label>
          </div>
          <label class="settings-field">
            <span class="settings-label">API Key {config.codeReviewApiKeySet ? <span class="settings-saved">✓ 已设置</span> : <span class="is-warn">未设置</span>}</span>
            <input type="password" id="cfg-code-review-key" name="codeReviewApiKey" class="settings-input" placeholder={config.codeReviewApiKeySet ? "已设置，输入新值覆盖（留空保持不变）" : "粘贴 API Key"} autocomplete="off" spellcheck={false} />
          </label>
          <label class="settings-field">
            <span class="settings-label">branches.json 提取模型 <span class="muted small">（留空则复用上方 Model）</span></span>
            <input type="text" id="cfg-branch-scope-model" name="branchScopeModel" value={config.branchScopeModel} class="settings-input" placeholder="留空复用 code review Model" spellcheck={false} autocomplete="off" />
          </label>
        </article>
      </div>
      <div class="sched-config-actions">
        <button type="submit" class="btn btn-primary">保存 AI 代码审查设置</button>
        <span id="code-review-config-saved" class="settings-saved muted small" hidden>✓ 已保存</span>
      </div>
    </form>
  </section>
)

const SettingsPage: FC<{ config: AppConfig & { codeReviewApiKeySet: boolean }; githubRepos: { path: string; label: string; remote: string }[] }> = ({ config, githubRepos }) => {
  return (
  <Layout title="Settings" active="settings">
    <div class="settings-page">
      <div class="page-header">
        <a href="/projects" class="back-link">← Back to projects</a>
        <h1>Dashboard 设置</h1>
        <p class="muted">这里集中管理定时任务设置；Schedulers 页面只展示后台任务运行状态和队列。</p>
      </div>

      <SchedulerConfigPanel config={config} githubRepos={githubRepos} />
      <CodeReviewConfigPanel config={config} />
    </div>
    <script src="/static/config.js" defer></script>
  </Layout>
  )
}

app.get("/settings", async (c) => {
  const [cfg, githubRepos] = await Promise.all([getSafeConfig(), listDeveloperGithubRepos()])
  return c.html(<SettingsPage config={cfg} githubRepos={githubRepos} />)
})

// ---------------------------------------------------------------------------
// Env vars page + env API
// ---------------------------------------------------------------------------

const EnvVarsPage: FC<{ groups: EnvFileGroup[] }> = ({ groups }) => {
  const catalogByName = new Map(ENV_VAR_CATALOG.map((entry) => [entry.name, entry]))
  const totalVars = groups.reduce((sum, g) => sum + g.variables.length, 0)
  const setVars = groups.reduce((sum, g) => sum + g.variables.filter((v) => v.hasValue).length, 0)
  return (
  <Layout title="Env Vars" active="envvars">
    <div class="settings-page env-vars-page">
      <div class="page-header">
        <a href="/projects" class="back-link">← Back to projects</a>
        <h1>环境变量管理</h1>
        <p class="muted">管理 OpenCode skill 所需的环境变量，按文件分组。修改直接写入 <code>~/.config/opencode/</code> 下的原始 env 文件，SOPS 同步会加密敏感文件。</p>
        <span class="settings-env-count">{setVars} / {totalVars} SET</span>
      </div>

      <section class="settings-section env-extract-section">
        <div class="settings-section-head">
          <div>
            <h2 class="op-section-title">一键提取 Token</h2>
            <p class="muted small">粘贴从浏览器 DevTools 复制的 curl 命令，自动提取 Ylops access / refresh token 并填充到环境变量。</p>
          </div>
        </div>
        <div class="env-extract-form">
          <textarea id="env-extract-curl" class="settings-input env-extract-textarea" placeholder="粘贴 curl 命令（含 Authorization header 和 Cookie）" rows={"4"} spellcheck={false}></textarea>
          <div class="env-extract-actions">
            <button type="button" id="env-extract-btn" class="btn btn-primary">提取 Token</button>
            <button type="button" id="env-extract-clear" class="btn btn-secondary">清空</button>
            <span id="env-extract-status" class="env-extract-status muted small"></span>
          </div>
        </div>
      </section>

      <section class="settings-section env-add-section">
        <div class="settings-section-head">
          <div>
            <h2 class="op-section-title">添加 / 覆盖变量</h2>
            <p class="muted small">填写变量名和值，选择目标文件后保存。值不会回显，只显示脱敏预览。</p>
          </div>
        </div>
        <form id="env-form" class="settings-env-form">
          <div class="settings-env-grid env-vars-grid">
            <label class="settings-field">
              <span class="settings-label">变量名</span>
              <input id="env-name" class="settings-input" name="name" list="env-known-vars" placeholder="OPENCODE_AI_ARK_HEVIN_API_KEY" autocomplete="off" spellcheck={false} pattern="[A-Za-z_][A-Za-z0-9_]*" required />
              <datalist id="env-known-vars">
                {ENV_VAR_CATALOG.map((entry) => <option value={entry.name} label={entry.requiredBy} />)}
              </datalist>
            </label>
            <label class="settings-field settings-env-value-field">
              <span class="settings-label">值</span>
              <input id="env-value" class="settings-input" name="value" type="password" placeholder="粘贴新值覆盖旧值" autocomplete="off" spellcheck={false} required />
            </label>
            <label class="settings-field">
              <span class="settings-label">说明</span>
              <input id="env-note" class="settings-input" name="note" placeholder="如：WMS UAT X-Token" autocomplete="off" spellcheck={false} />
            </label>
            <label class="settings-field">
              <span class="settings-label">目标文件</span>
              <select id="env-file" class="settings-input env-file-select" name="file">
                {groups.map((g) => (
                  <option value={g.file} selected={g.file === "secrets"}>
                    {g.label}{g.sensitive ? " (sensitive)" : ""}
                  </option>
                ))}
              </select>
            </label>
          </div>
          <div class="settings-env-actions">
            <button type="submit" class="btn btn-primary">保存变量</button>
            <button type="button" id="env-clear" class="btn btn-secondary">清空输入</button>
            <span id="env-saved" class="settings-saved muted small" hidden>✓ 已保存</span>
          </div>
        </form>
      </section>

      {groups.map((group) => {
        const setCount = group.variables.filter((v) => v.hasValue).length
        return (
        <section class="settings-section settings-section-env env-file-group" data-file={group.file}>
          <div class="settings-section-head">
            <div>
              <h2 class="op-section-title">
                {group.label}
                {group.sensitive ? <span class="env-file-badge env-file-badge-sensitive">sensitive</span> : <span class="env-file-badge env-file-badge-safe">plaintext</span>}
              </h2>
              <p class="muted small env-file-path">{group.path}</p>
            </div>
            <span class="settings-env-count">{setCount} / {group.variables.length} SET</span>
          </div>

          <div class="settings-env-list" id={`env-list-${group.file}`}>
            {group.variables.length === 0 ? (
              <div class="settings-env-empty">此文件中暂无已知变量。</div>
            ) : group.variables.map((entry) => {
              const placeholder = catalogByName.get(entry.name)?.placeholder ?? "Paste token / cookie / key"
              return (
              <div class="settings-env-row" data-name={entry.name} data-file={group.file}>
                <div class="settings-env-main">
                  <code>{entry.name}</code>
                  <span class={`settings-env-source settings-env-source-${entry.source}`}>{entry.source === "managed" ? "file" : entry.source === "process" ? "process env" : "missing"}</span>
                  <span class="settings-env-preview">{entry.preview}</span>
                  {entry.note ? <span class="settings-env-note">{entry.note}</span> : null}
                  <span class="settings-env-desc">{entry.requiredBy} · {entry.description}</span>
                </div>
                <div class="settings-env-row-actions">
                  <button type="button" class="btn btn-sm btn-secondary env-edit" data-name={entry.name} data-note={entry.note} data-file={group.file} data-placeholder={placeholder}>覆盖</button>
                  <button type="button" class="btn btn-sm btn-reject env-delete" data-name={entry.name} data-file={group.file}>删除</button>
                </div>
              </div>
            )})}
          </div>
        </section>
        )
      })}
    </div>
    <script src="/static/env-vars.js" defer></script>
  </Layout>
  )
}

app.get("/env-vars", async (c) => {
  const groups = await safeEnvVarsByFile()
  return c.html(<EnvVarsPage groups={groups} />)
})

app.get("/api/env-vars", async (c) => {
  return c.json({ groups: await safeEnvVarsByFile() })
})

/**
 * POST /api/env-vars/extract-tokens
 *
 * Parse a pasted curl command and extract known JWT tokens
 * (currently Ylops access + refresh). Returns the extracted tokens
 * with redacted previews so the browser can show a confirmation dialog
 * without exposing full values server-side.
 *
 * The actual write happens via the existing POST /api/config/env endpoint
 * once the user confirms in the dialog.
 */
app.post("/api/env-vars/extract-tokens", async (c) => {
  const body = await c.req.json().catch(() => null) ?? {}
  const curlText = typeof body.curl === "string" ? body.curl : ""
  if (!curlText.trim()) return c.json({ error: "Missing curl text" }, 400)

  const tokens = extractTokensFromCurl(curlText)
  if (tokens.length === 0) {
    return c.json({ error: "No recognised tokens found in the provided curl text" }, 422)
  }

  // Return redacted previews; full values are kept for the client to send
  // back to /api/config/env on confirmation.
  const preview = tokens.map((t) => ({
    name: t.name,
    file: t.file,
    source: t.source,
    preview: t.value.length > 12
      ? `${t.value.slice(0, 6)}…${t.value.slice(-4)}`
      : "******",
    value: t.value,
  }))

  return c.json({ tokens: preview })
})

app.get("/api/config", async (c) => {
  const config = await getSafeConfig()
  return c.json({ ...config, envVars: await safeEnvVars(config) })
})

app.post("/api/config", async (c) => {
  const body = await c.req.json().catch(() => null) ?? {}
  const partial: Partial<AppConfig> = {}
  if (body.harness === "pi" || body.harness === "opencode") partial.harness = body.harness
  if (typeof body.autoExtract === "boolean") partial.autoExtract = body.autoExtract
  if (typeof body.autoExtractSchedule === "boolean") partial.autoExtractSchedule = body.autoExtractSchedule
  const shouldRestartFullSyncScheduler = typeof body.fullSyncSchedule === "boolean" || Array.isArray(body.fullSyncTimes) || Array.isArray(body.fullSyncGithubRepos)
  if (typeof body.fullSyncSchedule === "boolean") partial.fullSyncSchedule = body.fullSyncSchedule
  if (Array.isArray(body.fullSyncTimes)) partial.fullSyncTimes = body.fullSyncTimes
  if (Array.isArray(body.fullSyncGithubRepos)) partial.fullSyncGithubRepos = body.fullSyncGithubRepos
  if (typeof body.extractModel === "string" && body.extractModel.trim()) partial.extractModel = body.extractModel.trim()
  if (typeof body.minChangeMessages === "number" && body.minChangeMessages > 0) partial.minChangeMessages = Math.floor(body.minChangeMessages)
  if (typeof body.autoValuation === "boolean") partial.autoValuation = body.autoValuation
  if (typeof body.valuationThreshold === "number" && body.valuationThreshold > 0) partial.valuationThreshold = Math.floor(body.valuationThreshold)
  // AI code review: empty apiKey means "keep existing" (handled in setConfig).
  if (typeof body.codeReviewBaseUrl === "string") partial.codeReviewBaseUrl = body.codeReviewBaseUrl.trim()
  if (typeof body.codeReviewModel === "string") partial.codeReviewModel = body.codeReviewModel.trim()
  if (typeof body.branchScopeModel === "string") partial.branchScopeModel = body.branchScopeModel.trim()
  if (typeof body.codeReviewApiKey === "string" && body.codeReviewApiKey.trim()) partial.codeReviewApiKey = body.codeReviewApiKey.trim()
  const next = await setConfig(partial)
  if (shouldRestartFullSyncScheduler) {
    stopFullSyncScheduler()
    startFullSyncScheduler()
  }
  const safe = await getSafeConfig()
  return c.json({ ...safe, envVars: await safeEnvVars(next) })
})

app.get("/api/pi-config", async (c) => {
  try {
    return c.json(await readPiConfigSummary())
  } catch (error) {
    return c.json({ error: (error as Error).message }, 500)
  }
})

app.post("/api/pi-config/settings", async (c) => {
  const body = await c.req.json().catch(() => null) ?? {}
  try {
    return c.json(await updatePiSettings({
      defaultProvider: typeof body.defaultProvider === "string" ? body.defaultProvider : undefined,
      defaultModel: typeof body.defaultModel === "string" ? body.defaultModel : undefined,
      defaultThinkingLevel: typeof body.defaultThinkingLevel === "string" ? body.defaultThinkingLevel : undefined,
      enabledModels: Array.isArray(body.enabledModels) ? body.enabledModels : undefined,
      theme: typeof body.theme === "string" ? body.theme : undefined,
    }))
  } catch (error) {
    return c.json({ error: (error as Error).message }, 400)
  }
})

app.get("/api/pi-config/file", async (c) => {
  const file = c.req.query("file")
  if (!isPiConfigFileKey(file)) return c.json({ error: "Invalid Pi config file" }, 400)
  try {
    return c.json(await getPiConfigFile(file))
  } catch (error) {
    return c.json({ error: (error as Error).message }, 500)
  }
})

app.post("/api/pi-config/file", async (c) => {
  const body = await c.req.json().catch(() => null) ?? {}
  if (!isPiConfigFileKey(body.file)) return c.json({ error: "Invalid Pi config file" }, 400)
  if (typeof body.content !== "string") return c.json({ error: "Missing config content" }, 400)
  try {
    return c.json(await savePiConfigFile(body.file, body.content))
  } catch (error) {
    return c.json({ error: (error as Error).message }, 400)
  }
})

app.post("/api/config/env", async (c) => {
  const body = await c.req.json().catch(() => null) ?? {}
  const action = typeof body.action === "string" ? body.action : "upsert"
  const name = typeof body.name === "string" ? body.name.trim().toUpperCase() : ""
  if (!/^[A-Z_][A-Z0-9_]{0,79}$/.test(name)) return c.json({ error: "Invalid variable name" }, 400)

  const config = await getConfig()
  const envVars = [...config.envVars]
  const existing = envVars.findIndex((entry) => entry.name === name)

  if (action === "delete") {
    await deleteEnvVar(name)
    if (existing >= 0) envVars.splice(existing, 1)
    const next = await setConfig({ envVars })
    return c.json({ envVars: await safeEnvVars(next), groups: await safeEnvVarsByFile(next) })
  } else {
    const value = typeof body.value === "string" ? body.value : ""
    if (!value && existing < 0) return c.json({ error: "Missing value" }, 400)
    const note = typeof body.note === "string" ? body.note : ""
    const file = body.file === "config" || body.file === "internal" || body.file === "secrets" ? body.file as EnvFileKind : "secrets"
    await upsertEnvVar(name, value || envVars[existing]?.value || "", file)
    const nextEntry: EnvVarEntry = {
      name,
      value: "",
      note,
      updatedAt: Date.now(),
    }
    if (existing >= 0) envVars[existing] = nextEntry
    else envVars.push(nextEntry)
  }

  const next = await setConfig({ envVars })
  return c.json({ envVars: await safeEnvVars(next), groups: await safeEnvVarsByFile(next) })
})

// ---------------------------------------------------------------------------
// Auto-extract: reads all context files, asks agent to produce per-file diffs
// ---------------------------------------------------------------------------

/**
 * Read all Hermes context files from a requirement directory.
 * Returns undefined for missing files.
 */
async function readContextFiles(reqDir: string): Promise<ContextFiles> {
  const readSafe = async (name: string): Promise<string | undefined> => {
    const p = join(reqDir, name)
    if (!existsSync(p)) return undefined
    try {
      return await readFile(p, "utf-8")
    } catch {
      return undefined
    }
  }
  const [meta, memory, alignment, prd, branch, config, impact, test, notes, review] = await Promise.all([
    readSafe("meta.md"),
    readSafe("memory.md"),
    readSafe(ALIGNMENT_FILE),
    readSafe(PRD_FILE),
    readSafe("branch.md"),
    readSafe("config-changes.md"),
    readSafe(IMPACT_FILE),
    readSafe("test.md"),
    readSafe("notes.md"),
    readSafe("review.md"),
  ])
  return { meta, memory, alignment, prd, branch, config, impact, test, notes, review }
}

/**
 * POST /api/requirement/auto-extract
 * Body: reqId, sessionId
 *
 * Kicks off a background auto-extract job that reads all context files,
 * builds a rich prompt, and asks the agent to produce per-file diffs.
 */
app.post("/api/requirement/auto-extract", async (c) => {
  const form = await c.req.formData()
  const reqId = String(form.get("reqId") || "")
  const sessionId = String(form.get("sessionId") || "")
  const guard = await resolveExtractTarget(reqId, sessionId)
  if (!guard.ok) return c.text(guard.message, guard.status)

  // Debounce + no-new-content guard: prevent rapid re-triggering and
  // redundant extracts when the session has no new conversation.
  const recentJob = findRecentJobForSession(sessionId, EXTRACT_DEBOUNCE_MS)
  const lastExtract = await getLastExtractForSession(sessionId)
  const sessions = await scanSessions(true)
  const sessionInfo = sessions.find((s) => s.id === sessionId)
  const sessionUpdated = sessionInfo?.updated || sessionInfo?.created || 0
  const guardResult = checkExtractGuard({
    recentJob,
    lastExtract,
    sessionUpdated,
    now: Date.now(),
  })
  if (!guardResult.ok) {
    return c.json({ error: guardResult.reason, message: guardResult.message }, 409)
  }

  const files = await readContextFiles(guard.req.reqDir!)
  const prompt = buildAutoExtractPrompt(guard.req, files)

  const cfg = await getConfig()

  try {
      const result = enqueueAutoExtract({
        reqId,
        sessionId,
        prompt,
        model: cfg.extractModel,
        autoAdopt: false,
        reqDir: guard.req.reqDir,
      })
    if (result.status === "immediate") {
      return c.json({ jobId: result.jobId, state: "running" }, 202)
    }
    // Queued — return 202 with scheduled time so the client can show
    // an estimated start time in the toast.
    return c.json({
      queued: true,
      scheduledAt: result.scheduledAt,
      delayMs: result.delayMs,
      queuePosition: result.queuePosition,
      sessionId,
    }, 202)
  } catch (err) {
    if (err instanceof JobConflictError) {
      return c.json({ error: "conflict", jobId: err.existingJobId }, 409)
    }
    const msg = err instanceof Error ? err.message : String(err)
    return c.text(`Failed to start job: ${msg}`, 500)
  }
})

/**
 * POST /api/requirement/generate-branch-scope
 * Body: reqId
 *
 * Kicks off a background job that reads `branch.md` and asks the agent
 * to emit a structured `branches.json`. Uses `runExtractStandalone`
 * (no `--session`/`--fork`) so the agent sees only the prompt - a
 * forked session's history would mislead it into "no update needed".
 * The prompt uses the `===UPDATE: branches.json===` delimiter protocol,
 * so finalizeJob's autoAdopt path writes the JSON straight to
 * `<req-dir>/branches.json`. No sessionId needed: the synthetic id
 * `branchscope-<reqId>` doubles as the per-requirement concurrency key.
 * The detail page's existing `data-extract-trigger` toast handles polling.
 */
app.post("/api/requirement/generate-branch-scope", async (c) => {
  const form = await c.req.formData()
  const reqId = String(form.get("reqId") || "")
  const req = await getRequirement(reqId)
  if (!req) return c.text("Requirement not found", 404)
  if (!req.reqDir) return c.text("This requirement has no on-disk directory", 400)

  const branchMd =
    req.branchPath && existsSync(req.branchPath)
      ? await readFile(req.branchPath, "utf-8")
      : ""
  if (!branchMd.trim()) {
    return c.json(
      { error: "no-branch-md", message: "需求没有 branch.md，无法生成 branches.json" },
      400,
    )
  }

  const prompt = buildBranchScopePrompt(req, branchMd)
  const cfg = await getConfig()

  try {
    const job = createExtractJob({
      reqId,
      sessionId: `branchscope-${reqId}`,
      prompt,
      mode: "auto",
      model: cfg.extractModel,
      autoAdopt: true,
      reqDir: req.reqDir,
      runFn: runExtractStandalone,
    })
    return c.json({ jobId: job.id, state: "running" }, 202)
  } catch (err) {
    if (err instanceof JobConflictError) {
      return c.json({ error: "conflict", jobId: err.existingJobId }, 409)
    }
    const msg = err instanceof Error ? err.message : String(err)
    return c.text(`Failed to start job: ${msg}`, 500)
  }
})

/**
 * POST /api/requirement/extract-branch-scope-ai
 * Body (JSON): { reqId }
 *
 * Direct LLM call that reads `branch.md`, asks an OpenAI-compatible model
 * to produce a structured `branches.json`, and persists it to
 * `<req-dir>/branches.json`. Unlike the background fork-job endpoint
 * above, this runs synchronously and returns the result as JSON so the
 * code-diff page button can show immediate feedback. Reuses the code-review
 * base URL / API key; the model defaults to branchScopeModel, falling
 * back to codeReviewModel.
 */
app.post("/api/requirement/extract-branch-scope-ai", async (c) => {
  const body = await c.req.json().catch(() => null) ?? {}
  const reqId = String(body.reqId || "")
  if (!reqId) return c.json({ ok: false, error: "Missing reqId" }, 400)
  const req = await getRequirement(reqId)
  if (!req) return c.json({ ok: false, error: "Requirement not found" }, 404)
  if (!req.reqDir) return c.json({ ok: false, error: "Requirement has no on-disk directory" }, 400)

  const branchMd =
    req.branchPath && existsSync(req.branchPath)
      ? await readFile(req.branchPath, "utf-8").catch(() => "")
      : ""
  if (!branchMd.trim()) {
    return c.json({ ok: false, error: "需求没有 branch.md，无法提取分支信息" }, 400)
  }

  const cfg = await getConfig()
  if (!cfg.codeReviewBaseUrl.trim() || !cfg.codeReviewApiKey.trim()) {
    return c.json({ ok: false, error: "尚未配置 AI 模型接入（Base URL / API Key），请在 Settings 页面填写。" }, 400)
  }

  const result = await runAiBranchScopeExtraction(req, branchMd, {
    baseUrl: cfg.codeReviewBaseUrl,
    apiKey: cfg.codeReviewApiKey,
    model: cfg.branchScopeModel,
    fallbackModel: cfg.codeReviewModel,
  })
  if (result.error) {
    return c.json({ ok: false, error: result.error, model: result.model }, 200)
  }

  // Persist branches.json atomically.
  const scope = {
    version: 2,
    updatedAt: Date.now(),
    repos: result.repos,
  }
  const branchesPath = join(req.reqDir, BRANCH_SCOPE_FILE)
  const tmp = `${branchesPath}.tmp.${process.pid}.${Date.now()}`
  try {
    const dir = dirname(branchesPath)
    if (!existsSync(dir)) await mkdir(dir, { recursive: true })
    await writeFile(tmp, JSON.stringify(scope, null, 2) + "\n", "utf-8")
    await rename(tmp, branchesPath)
  } catch (err) {
    const msg = err instanceof Error ? err.message : String(err)
    return c.json({ ok: false, error: `写入 branches.json 失败：${msg}`, model: result.model }, 500)
  }

  return c.json({
    ok: true,
    model: result.model,
    repoCount: result.repos.length,
    branchCount: result.repos.reduce((n, r) => n + r.branches.length, 0),
  })
})

/**
 * GET /requirement/auto-extract?jobId=<id>
 *
 * Preview page for auto-extract results. Shows per-file diffs with
 * accept/reject controls.
 */
app.get("/requirement/auto-extract", async (c) => {
  const jobIdParam = c.req.query("jobId")
  if (!jobIdParam) return c.text("Missing jobId", 400)
  const job = getExtractJob(jobIdParam)
  if (!job) return c.text("Job not found or expired", 404)
  const req = await getRequirement(job.reqId)
  if (!req) return c.text("Requirement not found", 404)

  // Read current file contents for diff display
  const currentFiles = req.reqDir ? await readContextFiles(req.reqDir) : {}

  return c.html(
    <AutoExtractPreviewPage req={req} sessionId={job.sessionId} job={job} currentFiles={currentFiles} />,
  )
})

const AutoExtractPreviewPage: FC<{
  req: Requirement
  sessionId: string
  job: ExtractJob
  currentFiles: ContextFiles
}> = ({ req, sessionId, job }) => {
  const backHref = `/requirement?id=${encodeURIComponent(req.id)}`
  const elapsedMs = (job.doneAt ?? Date.now()) - job.startedAt
  const autoResult = job.autoResult

  return (
    <Layout title={`智能提取 — ${req.title}`} active="requirements">
      <div class="req-extract">
        <div class="page-header">
          <a href={backHref} class="back-link">← 返回需求 {req.title}</a>
          <h1>智能上下文提取</h1>
          <div class="meta-grid">
            <div><span class="field-label">需求</span> {req.title}</div>
            <div><span class="field-label">Session</span> <code>{sessionId}</code></div>
            <div><span class="field-label">耗时</span> {(elapsedMs / 1000).toFixed(1)}s</div>
          </div>
        </div>

        {job.state === "running" ? (
          <section class="req-extract-running" data-job-id={job.id}>
            <p><span class="req-extract-spinner" aria-hidden="true"></span> <strong>agent 正在分析会话和上下文文件…</strong></p>
            <p class="muted small">已运行 <span class="js-extract-elapsed">{Math.round(elapsedMs / 1000)}</span> 秒。完成后此页自动刷新。</p>
          </section>
        ) : job.state === "done" && autoResult ? (
          <section class="auto-extract-result">
            {autoResult.summary ? (
              <div class="auto-extract-summary">
                <strong>变更说明：</strong> {autoResult.summary}
              </div>
            ) : null}

            {autoResult.updates.length === 0 && autoResult.appends.length === 0 ? (
              <div class="auto-extract-empty">
                <p>Agent 判断本次会话无需更新上下文文件。</p>
                <a href={backHref} class="btn btn-secondary">返回需求</a>
              </div>
            ) : (
              <form method="post" action="/api/requirement/auto-extract/commit" class="auto-extract-form">
                <input type="hidden" name="reqId" value={req.id} />
                <input type="hidden" name="sessionId" value={sessionId} />

                {autoResult.updates.map((u, i) => (
                  <div class="auto-extract-file" data-filename={u.filename}>
                    <div class="auto-extract-file-head">
                      <label class="auto-extract-accept">
                        <input type="checkbox" name={`update_${i}`} value={u.filename} checked />
                        <span>更新 <code>{u.filename}</code></span>
                      </label>
                      <details class="auto-extract-original">
                        <summary class="muted small">查看现有内容</summary>
                        <pre class="auto-extract-diff">{(job as any)._originalFiles?.[u.filename] ?? "(文件不存在)"}</pre>
                      </details>
                    </div>
                    <textarea
                      name={`update_content_${i}`}
                      class="req-extract-body auto-extract-textarea"
                      rows={String(Math.min(24, u.content.split("\n").length + 2))}
                      spellcheck={false}
                    >{u.content}</textarea>
                  </div>
                ))}

                {autoResult.appends.map((a, i) => (
                  <div class="auto-extract-file" data-filename={a.filename}>
                    <div class="auto-extract-file-head">
                      <label class="auto-extract-accept">
                        <input type="checkbox" name={`append_${i}`} value={a.filename} checked />
                        <span>追加到 <code>{a.filename}</code></span>
                      </label>
                    </div>
                    <textarea
                      name={`append_content_${i}`}
                      class="req-extract-body auto-extract-textarea"
                      rows={String(Math.min(20, a.content.split("\n").length + 2))}
                      spellcheck={false}
                    >{a.content}</textarea>
                  </div>
                ))}

                <div class="req-extract-actions">
                  <button type="submit" class="btn btn-primary">提交已接受的变更</button>
                  <a href={backHref} class="btn btn-secondary">全部取消</a>
                </div>
              </form>
            )}
          </section>
        ) : (
          <section class="req-extract-error">
            <p class="req-extract-error-msg"><strong>分析失败</strong>：{job.errorMessage || "未知错误"}</p>
            {job.stderr ? <pre class="req-extract-stderr">{job.stderr.slice(0, 2000)}</pre> : null}
            <div class="req-extract-actions">
              <a href={backHref} class="btn btn-secondary">返回需求</a>
            </div>
          </section>
        )}
      </div>
    </Layout>
  )
}

/**
 * POST /api/requirement/auto-extract/commit
 *
 * Writes the accepted file updates and appends to the requirement
 * directory. Each update replaces the file; each append adds content.
 */
app.post("/api/requirement/auto-extract/commit", async (c) => {
  const form = await c.req.formData()
  const reqId = String(form.get("reqId") || "")
  const sessionId = String(form.get("sessionId") || "")
  const guard = await resolveExtractTarget(reqId, sessionId)
  if (!guard.ok) return c.text(guard.message, guard.status)
  if (!guard.req.reqDir) return c.text("Requirement has no directory", 400)

  const reqDir = guard.req.reqDir
  const allowedFiles = new Set(["memory.md", ALIGNMENT_FILE, PRD_FILE, "branch.md", "config-changes.md", IMPACT_FILE, "test.md", "notes.md", "review.md", "meta.md"])
  let written = 0

  // Process updates and appends from form fields
  const entries = [...form.entries()]
  for (const [key, value] of entries) {
    const updateMatch = key.match(/^update_(\d+)$/)
    if (updateMatch) {
      const idx = updateMatch[1]
      const filename = String(value)
      if (!allowedFiles.has(filename)) continue
      const content = String(form.get(`update_content_${idx}`) || "")
      if (!content.trim()) continue
      const filePath = join(reqDir, filename)
      // Safety: ensure the resolved path is still inside reqDir
      if (!filePath.startsWith(reqDir + "/") && filePath !== reqDir) continue
      await writeFile(filePath, content, "utf-8")
      written++
      continue
    }

    const appendMatch = key.match(/^append_(\d+)$/)
    if (appendMatch) {
      const idx = appendMatch[1]
      const filename = String(value)
      if (!allowedFiles.has(filename)) continue
      const content = String(form.get(`append_content_${idx}`) || "")
      if (!content.trim()) continue
      const filePath = join(reqDir, filename)
      if (!filePath.startsWith(reqDir + "/") && filePath !== reqDir) continue
      await appendFile(filePath, "\n\n" + content + "\n", "utf-8")
      written++
    }
  }

  return c.redirect(`/requirement?id=${encodeURIComponent(reqId)}`, 303)
})

// ---------------------------------------------------------------------------
// Requirement attachments - list / upload / download / delete
// ---------------------------------------------------------------------------

/**
 * Resolve a reqId to a validated {req, reqDir} pair, or return an HTTP
 * error response. Shared by the four attachment routes below.
 */
async function resolveAttachmentReq(
  reqId: string,
): Promise<{ ok: true; req: Requirement } | { ok: false; status: 400 | 404; message: string }> {
  if (!reqId) return { ok: false, status: 400, message: "Missing reqId" }
  const req = await getRequirement(reqId)
  if (!req) return { ok: false, status: 404, message: "Requirement not found" }
  if (!req.reqDir) return { ok: false, status: 400, message: "Requirement has no on-disk directory" }
  return { ok: true, req }
}

/**
 * GET /api/requirement/attachments?reqId=<id>
 *
 * Returns a JSON array of attachment metadata for the requirement.
 */
app.get("/api/requirement/attachments", async (c) => {
  const reqId = String(c.req.query("reqId") || "")
  const guard = await resolveAttachmentReq(reqId)
  if (!guard.ok) return c.text(guard.message, guard.status)
  const attachments = await listAttachments(guard.req.reqDir!)
  return c.json({ attachments })
})

/**
 * POST /api/requirement/attachments/upload
 *
 * Multipart form: reqId, redirect (optional), file.
 * Writes the uploaded file into `<reqDir>/attachments/`. Unsafe filenames
 * (path traversal, null bytes) are rejected with 400. Same-name files
 * are overwritten.
 */
app.post("/api/requirement/attachments/upload", async (c) => {
  const form = await c.req.formData()
  const reqId = String(form.get("reqId") || "")
  const redirect = String(form.get("redirect") || "")
  const guard = await resolveAttachmentReq(reqId)
  if (!guard.ok) return c.text(guard.message, guard.status)

  const file = form.get("file")
  if (!(file instanceof File)) return c.text("Missing or invalid file", 400)

  // Use the uploaded filename's basename; reject if it fails the safe-path
  // gate (covers traversal, null bytes, path separators).
  const filename = file.name || "upload"
  const safePath = resolveAttachmentPath(guard.req.reqDir!, filename)
  if (!safePath) return c.text(`Unsafe filename: ${filename}`, 400)

  const buffer = Buffer.from(await file.arrayBuffer())
  await writeAttachment(guard.req.reqDir!, filename, buffer)

  if (redirect) return c.redirect(redirect, 303)
  return c.redirect(`/requirement?id=${encodeURIComponent(reqId)}`, 303)
})

/**
 * GET /requirement/attachments/download?reqId=<id>&filename=<name>
 *
 * Streams the raw attachment bytes with a Content-Disposition header so
 * the browser downloads it. Unsafe names are rejected with 400.
 */
fastify.get("/requirement/attachments/download", async (request, reply) => {
  const reqId = String((request.query as Record<string, string>).reqId || "")
  const filename = String((request.query as Record<string, string>).filename || "")
  const guard = await resolveAttachmentReq(reqId)
  if (!guard.ok) return reply.code(guard.status).send(guard.message)

  const buffer = await readAttachmentBuffer(guard.req.reqDir!, filename)
  if (!buffer) return reply.code(404).send("File not found")

  const encoded = encodeURIComponent(filename)
  reply
    .type("application/octet-stream")
    .header("Content-Disposition", `attachment; filename="${encoded}"; filename*=UTF-8''${encoded}`)
    .header("Content-Length", String(buffer.length))
    .send(buffer)
})

/**
 * POST /api/requirement/attachments/delete
 *
 * Form body: reqId, filename, redirect (optional). Deletes the file from
 * `<reqDir>/attachments/`. Unsafe names return 400 (not found = 400 too,
 * to avoid leaking which names exist).
 */
app.post("/api/requirement/attachments/delete", async (c) => {
  const form = await c.req.formData()
  const reqId = String(form.get("reqId") || "")
  const filename = String(form.get("filename") || "")
  const redirect = String(form.get("redirect") || "")
  const guard = await resolveAttachmentReq(reqId)
  if (!guard.ok) return c.text(guard.message, guard.status)

  const deleted = await deleteAttachment(guard.req.reqDir!, filename)
  if (!deleted) return c.text("File not found", 404)

  if (redirect) return c.redirect(redirect, 303)
  return c.redirect(`/requirement?id=${encodeURIComponent(reqId)}`, 303)
})

app.get("/api/requirements", async (c) => {
  const groups = await listRequirementsByProject()
  const requirements = groups.flatMap((g) => g.requirements)
  return c.json({ requirements })
})

/**
 * GET /api/requirement/recommendations?id=<reqId>
 *
 * Scores unbound sessions against the requirement and returns the top
 * matches with score + reasons, so the React detail page can surface
 * "疑似相关 Session" without duplicating the server-side scoring logic.
 */
app.get("/api/requirement/recommendations", async (c) => {
  const id = c.req.query("id")
  if (!id) return c.json({ error: "Missing id" }, 400)
  const req = await getRequirement(id)
  if (!req) return c.json({ error: "Requirement not found" }, 404)
  if (req.id === DEFAULT_REQ_ID) return c.json({ recommendations: [] })

  const { harness } = await getConfig()
  const [sessions, associatedAll] = await Promise.all([
    scanDashboardSessions(harness),
    getAllAssociatedSessionIds(),
  ])
  const unassociated = sessions.filter(
    (s) =>
      !s.parentId &&
      !FORK_TITLE_RE.test(s.title || "") &&
      !associatedAll.has(s.id) &&
      !req.sessionIds.includes(s.id)
  )
  const recommendations = recommendSessionsForRequirement(req, unassociated, 6)
  return c.json({
    recommendations: recommendations.map((reco) => ({
      session: reco.session,
      score: reco.score,
      reasons: reco.reasons,
    })),
  })
})

// ---------------------------------------------------------------------------
// Notification center routes
// ---------------------------------------------------------------------------

/**
 * GET /api/notifications
 *
 * Returns { unreadCount, notifications: [...] } for the bell panel.
 * Includes only non-dismissed notifications, newest-first.
 */
app.get("/api/notifications", (c) => {
  const notifications = getNotifications(false)
  return c.json({
    unreadCount: getUnreadCount(),
    notifications,
  })
})

/**
 * GET /api/notifications/unread-count
 *
 * Lightweight counter for the bell badge poll. Returns `{ count }`.
 */
app.get("/api/notifications/unread-count", (c) => {
  return c.json({ count: getUnreadCount() })
})

/**
 * POST /api/notifications/dismiss
 * Body: id=<notificationId> | all=1
 */
app.post("/api/notifications/dismiss", async (c) => {
  const form = await c.req.formData()
  const all = String(form.get("all") || "") === "1"
  if (all) {
    dismissAll()
    return c.json({ ok: true })
  }
  const id = String(form.get("id") || "")
  if (!id) return c.text("Missing id", 400)
  if (!getNotification(id)) return c.text("Notification not found", 404)
  dismissNotification(id)
  return c.json({ ok: true })
})

/**
 * POST /api/notifications/mark-read
 *
 * Mark all non-running notifications as read. Running ones stay unread
 * because they represent in-flight work the user hasn't seen the
 * outcome of yet.
 */
app.post("/api/notifications/mark-read", (c) => {
  markAllRead()
  return c.json({ ok: true })
})

// ---------------------------------------------------------------------------
// Experience marker routes (manual session marking for auto-summary)
// ---------------------------------------------------------------------------

/**
 * POST /api/experience/mark
 * Body (JSON): { sessionId, note? }
 *
 * Mark a session for auto experience summarization. The background
 * worker will fork the session and generate a report once it has been
 * idle for ≥1 hour.
 */
app.post("/api/experience/mark", async (c) => {
  const body = await c.req.json().catch(() => null) ?? {}
  const sessionId = String(body.sessionId || "")
  if (!sessionId) return c.json({ error: "Missing sessionId" }, 400)
  if (!isValidSessionId(sessionId)) return c.json({ error: "Invalid sessionId" }, 400)
  const note = typeof body.note === "string" ? body.note : undefined
  const marker = await markSession(sessionId, { note })
  return c.json({ ok: true, marker })
})

/**
 * POST /api/experience/unmark
 * Body (JSON): { sessionId }
 *
 * Remove a marker. No-op if the session was not marked.
 */
app.post("/api/experience/unmark", async (c) => {
  const body = await c.req.json().catch(() => null) ?? {}
  const sessionId = String(body.sessionId || "")
  if (!sessionId) return c.json({ error: "Missing sessionId" }, 400)
  if (!isValidSessionId(sessionId)) return c.json({ error: "Invalid sessionId" }, 400)
  const removed = await unmarkSession(sessionId)
  return c.json({ ok: true, removed })
})

/**
 * GET /api/experience/markers
 *
 * List all markers, optionally filtered by `?status=<status>`.
 */
app.get("/api/experience/markers", (c) => {
  const statusParam = c.req.query("status") as MarkerStatus | undefined
  const markers = listMarkers(statusParam)
  return c.json({ markers })
})

// ---------------------------------------------------------------------------
// Session valuation API
// ---------------------------------------------------------------------------

/**
 * GET /api/valuation/candidates
 * Returns recent valuation candidates (score ≥ threshold), newest/highest first.
 */
app.get("/api/valuation/candidates", (c) => {
  const limit = parseInt(c.req.query("limit") || "20", 10)
  const candidates = getRecentCandidates(limit)
  const stats = getValuationStats()
  return c.json({ candidates, stats })
})

/**
 * POST /api/valuation/mark
 * Body (JSON or form-encoded): { sessionId }
 * Manually mark a session from the valuation candidate list.
 * Also accepts form-encoded POST from the schedulers page table.
 */
app.post("/api/valuation/mark", async (c) => {
  const contentType = c.req.header("content-type") || ""
  let sessionId = ""
  if (contentType.includes("application/json")) {
    const body = await c.req.json().catch(() => null) ?? {}
    sessionId = String(body.sessionId || "")
  } else {
    const form = await c.req.formData().catch(() => null)
    sessionId = String(form?.get("sessionId") || "")
  }
  if (!sessionId) return c.json({ error: "Missing sessionId" }, 400)
  if (!isValidSessionId(sessionId)) return c.json({ error: "Invalid sessionId" }, 400)
  const marker = await markSession(sessionId, { note: "manual: from valuation candidates" })
  // Redirect back to /schedulers for form POSTs.
  if (!contentType.includes("application/json")) {
    return c.redirect("/schedulers")
  }
  return c.json({ ok: true, marker })
})

/**
 * POST /api/valuation/poll
 * Manually trigger a valuation poll cycle (useful for testing/debugging).
 */
app.post("/api/valuation/poll", async (c) => {
  await valuationPollOnce()
  const stats = getValuationStats()
  return c.json({ ok: true, stats })
})

// API: confirm or reject candidates.
// Extended: if the confirmed report has an associated marker (i.e. it
// was auto-generated from a marked session), trigger the execution fork
// for the confirmed candidate IDs so the user's accepted items get
// implemented without leaving the dashboard.
app.post("/api/confirm", async (c) => {
  const body = await c.req.json() as Confirmation
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

  // If this report came from a marked session and the user confirmed
  // candidates, trigger the execution fork. The fork runs in the
  // background; the marker's status tracks progress.
  let executionTriggered = false
  if (confirmation.mode === "confirm" && confirmation.confirmedIds.length > 0) {
    // Find a marker whose reportPath matches this report.
    const allMarkers = listMarkers("summarized")
    const matched = allMarkers.find((m) => m.reportPath === reportPath)
    if (matched) {
      // Fire and forget — the marker store tracks the fork's progress.
      void triggerExecutionForMarker(matched.sessionId, confirmation.confirmedIds).catch(() => {})
      executionTriggered = true
    }
  }

  return c.json({ ok: true, savedPath, executionTriggered })
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
  const { harness } = await getConfig()
  const days = parseDaysParam(c.req.query("days"))
  const maxAgeMs = days > 0 ? days * 24 * 60 * 60 * 1000 : undefined
  const sessions = await scanDashboardSessions(harness, false, maxAgeMs)
  return c.json({ summary: summarizeSessions(sessions), sessions, harness, days })
})

// API: get a single session
app.get("/api/session", async (c) => {
  const { harness } = await getConfig()
  const id = c.req.query("id")
  if (!id) return c.json({ error: "Missing id" }, 400)
  const session = await getDashboardSession(harness, id)
  if (!session) return c.json({ error: "Not found" }, 404)
  return c.json(session)
})

// ---------------------------------------------------------------------------
// WebSocket: /ws/session-terminal?id=...
// ---------------------------------------------------------------------------

/**
 * Terminal WebSocket handler. Attaches message/close/error listeners
 * synchronously (per @fastify/websocket requirement) then runs async open
 * logic. The socket is already connected when this handler is called.
 */
async function handleTerminalSocket(
  ws: { send(data: string): void; close(code?: number, reason?: string): void },
  query: Record<string, string>,
  state: { session: TerminalSession | null; exited: boolean },
): Promise<void> {
  const { harness } = await getConfig()
  const id = query.id ?? ""
  const createNew = query.new === "1"
  const reqId = query.req ?? ""
  const autoInject = shouldAutoInjectRequirementContext(
    new URL(`ws://localhost/ws?req=${encodeURIComponent(reqId)}&inject=${query.inject ?? ""}`),
  )

  let directory: string | null = null
  let title: string | undefined
  if (!createNew) {
    const sessionInfo = await getDashboardSession(harness, id)
    directory = sessionInfo?.directory ?? null
  } else if (reqId) {
    const req = await getRequirement(reqId)
    if (req) title = req.title || undefined
  }

  const startMs = Date.now()
  const env = await buildManagedEnv()
  const result = startSession(id, directory, {
    onOutput: (chunk) => {
      if (state.exited) return
      try { ws.send(chunk) } catch { /* ignore */ }
    },
    onExit: (code, signal) => {
      state.exited = true
      try { ws.send(JSON.stringify({ type: "exit", code, signal: signal ?? null })) } catch { /* ignore */ }
      try { ws.close(1000, "process exited") } catch { /* noop */ }
    },
    onError: (message) => {
      try { ws.send(JSON.stringify({ type: "error", message })) } catch { /* ignore */ }
      try { ws.close(1011, "spawn error") } catch { /* noop */ }
    },
  }, { createNew, title, env })

  if ("error" in result) {
    try { ws.send(JSON.stringify({ type: "error", message: result.error })) } catch { /* noop */ }
    try { ws.close(1008, result.error) } catch { /* noop */ }
    return
  }
  state.session = result
  try { ws.send(JSON.stringify({ type: "ready", id: result.id, cols: result.cols, rows: result.rows })) } catch { /* noop */ }

  let discoveredId = ""
  if (createNew) {
    const cwd = result.cwd
    const deadline = Date.now() + 10_000
    while (!discoveredId && Date.now() < deadline && !state.exited) {
      await new Promise((r) => setTimeout(r, 500))
      clearDashboardSessionCache(harness)
      const list = await scanDashboardSessions(harness, true)
      const candidate = list.find((s) => s.directory === cwd && (s.created || 0) >= startMs)
      if (candidate) { discoveredId = candidate.id; break }
    }
    if (discoveredId) {
      if (reqId) {
        try { await replaceAssociatedSession(reqId, id, discoveredId) } catch { /* noop */ }
      }
      try { ws.send(JSON.stringify({ type: "session", id: discoveredId })) } catch { /* noop */ }
    }
  }

  if (reqId && autoInject) {
    try {
      const req = await getRequirement(reqId)
      if (req) {
        const ctx = await buildInjectionContext(req.id)
        setTimeout(() => {
          if (state.exited || !state.session) return
          try { writeToSession(state.session!, ctx + "\r"); ws.send(JSON.stringify({ type: "injected" })) } catch { /* noop */ }
        }, 3000)
      }
    } catch { /* noop */ }
  }
}

// Track live terminal WebSockets for graceful drain during hot-deploy shutdown.
const liveTerminals = new Set<unknown>()

fastify.get("/ws/session-terminal", { websocket: true }, (socket, request) => {
  liveTerminals.add(socket)
  const state = { session: null as TerminalSession | null, exited: false }

  // Attach listeners synchronously before async work (per @fastify/websocket).
  socket.on("message", (data: unknown) => {
    if (!state.session) return
    const str = typeof data === "string" ? data : data instanceof Buffer ? data.toString() : ""
    if (!str) return
    const msg = parseClientMessage(str)
    if (msg.kind === "input") writeToSession(state.session, msg.data)
    else if (msg.kind === "resize") resizeSession(state.session, msg.cols, msg.rows)
  })

  socket.on("close", () => {
    liveTerminals.delete(socket)
    if (state.session) { killSession(state.session); state.session = null }
    state.exited = true
  })

  socket.on("error", () => {
    liveTerminals.delete(socket)
    if (state.session) { killSession(state.session); state.session = null }
    state.exited = true
  })

  const query = request.query as Record<string, string>
  handleTerminalSocket(socket, query, state).catch((err: unknown) => {
    const message = err instanceof Error ? err.message : String(err)
    try { socket.send(JSON.stringify({ type: "error", message })) } catch { /* noop */ }
    try { socket.close(1011, "open error") } catch { /* noop */ }
  })
})

// ---------------------------------------------------------------------------
// Server bootstrap
// ---------------------------------------------------------------------------

const port = parseInt(process.env.PORT || "7331", 10)

await initNotifications()
await initConfig()
await initAutoDriveJobs()
await initMarkers()

// Blue/green: only the backend holding the scheduler lock runs background
// workers. The inactive slot serves HTTP/WS only and polls until it can
// acquire the lock (the old backend releases it during a deploy's drain).
let ownsScheduler = false
let schedulerLockRetry: ReturnType<typeof setInterval> | null = null

function startSchedulersIfOwned(): void {
  if (ownsScheduler) return
  if (acquireSchedulerLock()) {
    ownsScheduler = true
    startAutoSummaryWorker()
    startAutoExtractScheduler()
    startAutoValuationWorker()
    startFullSyncScheduler()
    console.log("[scheduler] lock acquired, schedulers started")
  }
}

function stopSchedulers(): void {
  if (!ownsScheduler) return
  stopAutoSummaryWorker()
  stopAutoExtractScheduler()
  stopAutoValuationWorker()
  stopFullSyncScheduler()
  releaseSchedulerLock()
  ownsScheduler = false
  console.log("[scheduler] lock released, schedulers stopped")
}

startSchedulersIfOwned()
schedulerLockRetry = setInterval(startSchedulersIfOwned, 2000)
if (typeof schedulerLockRetry.unref === "function") schedulerLockRetry.unref()

await fastify.listen({ port, host: "0.0.0.0" })
console.log(`Agent Panel backend running at http://localhost:${port}`)

// Graceful shutdown (SIGTERM from systemd stop / hot-deploy). Drain live
// terminal WebSockets up to 5 min, then force-close and exit.
let shuttingDown = false
const DRAIN_TIMEOUT_MS = 5 * 60 * 1000

async function gracefulShutdown(signal: string): Promise<void> {
  if (shuttingDown) return
  shuttingDown = true
  console.log(`[shutdown] received ${signal}, draining...`)
  if (schedulerLockRetry) clearInterval(schedulerLockRetry)
  stopSchedulers()

  const deadline = Date.now() + DRAIN_TIMEOUT_MS
  while (liveTerminals.size > 0 && Date.now() < deadline) {
    await new Promise((r) => setTimeout(r, 1000))
  }
  for (const ws of liveTerminals) {
    try {
      ;(ws as { close: (code?: number, reason?: string) => void }).close(1001, "server shutting down")
    } catch {
      // already closed
    }
  }
  liveTerminals.clear()

  await fastify.close()
  console.log("[shutdown] complete")
  process.exit(0)
}

process.on("SIGTERM", () => { void gracefulShutdown("SIGTERM") })
process.on("SIGINT", () => { void gracefulShutdown("SIGINT") })
