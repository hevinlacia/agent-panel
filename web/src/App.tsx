/**
 * Role: React SPA for Agent Panel pages, mounted inside the Fastify shell.
 * Public surface: App component consumed by web/src/main.tsx.
 * Constraints: browser UI only; all filesystem, PTY, and secret-safe writes stay behind server APIs.
 * Read-this-with: src/server.tsx route/API contracts and web/src/styles.css.
 */
import { motion, AnimatePresence } from "framer-motion"
import {
  Activity,
  AlertTriangle,
  ArrowLeft,
  Bell,
  CheckCircle2,
  Clock3,
  FileText,
  Gauge,
  GitBranch,
  LayoutDashboard,
  ListChecks,
  RefreshCw,
  Search,
  Server,
  Settings,
  Sparkles,
  TerminalSquare,
  TimerReset,
} from "lucide-react"
import { useEffect, useMemo, useRef, useState } from "react"
import { Terminal } from "@xterm/xterm"
import { FitAddon } from "@xterm/addon-fit"
import "@xterm/xterm/css/xterm.css"
import type { DashboardStatsPayload, RequirementDuration, StatusCount } from "./types"

interface AppProps { apiPath: string }

type ReqStatus = "需求对齐" | "方案设计" | "开发中" | "自测中" | "测试中" | "待上线" | "已完成"

type ReqCategory = "需求" | "线上问题"

const REQ_STATUSES: ReqStatus[] = ["需求对齐", "方案设计", "开发中", "自测中", "测试中", "待上线", "已完成"]

const REQ_CATEGORIES: ReqCategory[] = ["需求", "线上问题"]

const statusMeta: Record<string, { slug: string; color: string; soft: string }> = {
  需求对齐: { slug: "align", color: "#94a3b8", soft: "rgba(148, 163, 184, 0.14)" },
  方案设计: { slug: "design", color: "#f59e0b", soft: "rgba(245, 158, 11, 0.14)" },
  开发中: { slug: "dev", color: "#22d3ee", soft: "rgba(34, 211, 238, 0.14)" },
  自测中: { slug: "selftest", color: "#3b82f6", soft: "rgba(59, 130, 246, 0.14)" },
  测试中: { slug: "testing", color: "#a855f7", soft: "rgba(168, 85, 247, 0.14)" },
  待上线: { slug: "deploy", color: "#eab308", soft: "rgba(234, 179, 8, 0.14)" },
  已完成: { slug: "done", color: "#22c55e", soft: "rgba(34, 197, 94, 0.14)" },
}

interface EffortFactor { name: string; score: number; reason: string }
interface EffortEstimate {
  version: number
  coefficient: number
  baseHours: number
  estimatedHours: number
  factors: EffortFactor[]
  summary: string
  model: string
  updatedAt: number
}

interface Requirement {
  id: string
  title: string
  description?: string
  status: ReqStatus
  category?: ReqCategory
  project: string
  projects?: string[]
  groupPath?: string[]
  createdAt: number
  updatedAt: number
  sessionIds: string[]
  reqDir?: string
  effortEstimate?: EffortEstimate
  /** ONES task reference: a full URL (clickable) or a bare task id (display-only). */
  ones?: string
}

interface SessionInfo {
  id: string
  title?: string
  status: "running" | "idle" | "stale" | string
  agent?: string
  model?: string
  provider?: string
  directory?: string
  updated?: number
  created?: number
  parentId?: string
  source?: string
  tokens?: number
  inputTokens?: number
  outputTokens?: number
}

interface ApiSessions { summary: Record<string, number>; sessions: SessionInfo[]; harness?: string; days?: number }
interface ReportListItem { path: string; meta?: { session?: string; date?: string }; confirmedCount?: number; rejectedCount?: number; [key: string]: unknown }
interface EnvGroup { file: string; variables: Array<Record<string, any>> }
interface ConfigPayload { [key: string]: any }
type PiConfigFileKey = "settings" | "models" | "agents"
interface PiModelOption { providerId: string; modelId: string; label: string; name?: string; contextWindow?: number; maxTokens?: number; reasoning?: boolean; thinkingLevels: string[] }
interface PiProviderSummary { id: string; api?: string; baseUrl?: string; modelCount: number; hasApiKey: boolean; models: PiModelOption[] }
interface PiSettingsSummary { path: string; exists: boolean; defaultProvider: string; defaultModel: string; defaultThinkingLevel: string; enabledModels: string[]; theme: string }
interface PiConfigFileMeta { file: PiConfigFileKey; label: string; path: string; sensitive: boolean; description: string }
interface PiConfigSummary { settings: PiSettingsSummary; providers: PiProviderSummary[]; files: PiConfigFileMeta[]; thinkingLevels: string[] }
interface PiConfigFileSnapshot extends PiConfigFileMeta { content: string; updatedAt: number | null }
interface PiSettingsDraft { defaultProvider: string; defaultModel: string; defaultThinkingLevel: string; theme: string; enabledModelsText: string }
type AutoDriveState = "queued" | "running" | "blocked" | "done" | "failed"
interface AutoDriveJob {
  id: string
  reqId: string
  reqTitle: string
  reqStatus: ReqStatus
  reqDir: string | null
  state: AutoDriveState
  phase: ReqStatus
  sessionId: string | null
  notificationId: string | null
  summary: string
  blockers: string[]
  stdout: string
  stderr: string
  exitCode: number | null
  timedOut: boolean
  queuedMs: number
  durationMs: number
  createdAt: number
  updatedAt: number
  startedAt: number | null
  doneAt: number | null
}
interface AutoDrivePayload { jobs: AutoDriveJob[]; active: number; blocked: number; queue: { active: number; queued: number } }

type GitAiCompanyStatus = "pending" | "confirmed_ai" | "missing_ai" | "not_found" | "check_failed"
interface GitAiSuspectRecord {
  id: string
  projectName: string
  commitSha: string
  shortSha: string
  gitlabProjectId: string | null
  repoPath: string | null
  remoteUrl: string | null
  branch: string | null
  subject: string | null
  authorName: string | null
  eventSources: string[]
  localNoteState: "complete" | "missing" | "unknown"
  companyStatus: GitAiCompanyStatus
  companyCheckedAt: number | null
  companyError: string | null
  commitWebUrl: string | null
  commitTitle: string | null
  committedAt: string | null
  originBranch: string | null
  additions: number | null
  deletions: number | null
  aiRate: number | null
  aiLines: number | null
  humanLines: number | null
  firstSeenAt: number
  lastSeenAt: number
}
interface GitAiSuspectStats { total: number; pending: number; confirmedAi: number; missingAi: number; notFound: number; checkFailed: number }
interface GitAiSuspectsPayload { records: GitAiSuspectRecord[]; stats: GitAiSuspectStats; generatedAt: number }
type HealthTone = "ok" | "warn" | "error" | "unknown"
interface GitAiHookHealth { path: string | null; exists: boolean; mode: string; recordsToAgentPanel: boolean; executable: boolean }
interface GitAiHealthPayload {
  generatedAt: number
  storePath: string
  cli: {
    binaryPath: string | null
    installed: boolean
    version: string | null
    daemonOk: boolean
    daemonMessage: string | null
    trace2Target: string | null
    trace2Socket: string | null
    trace2SocketExists: boolean
    hooksPath: string | null
    postCommitHook: GitAiHookHealth
    prePushHook: GitAiHookHealth
  }
  piExtension: {
    globalPath: string
    sourcePath: string
    globalExists: boolean
    sourceExists: boolean
    sourceMatchesGlobal: boolean
    autoDiscoveryPath: boolean
    gitAiBinaryExistsForExtension: boolean
    registersStatus: boolean
    tracksTools: string[]
    status: HealthTone
    message: string
  }
}

const cardVariants = {
  hidden: { opacity: 0, y: 18, scale: 0.98 },
  show: { opacity: 1, y: 0, scale: 1 },
}

function useLocationKey() {
  const [key, setKey] = useState(() => window.location.pathname + window.location.search)
  useEffect(() => {
    const onPop = () => setKey(window.location.pathname + window.location.search)
    window.addEventListener("popstate", onPop)
    return () => window.removeEventListener("popstate", onPop)
  }, [])
  return key
}

async function fetchJson<T>(url: string, init?: RequestInit): Promise<T> {
  const res = await fetch(url, { cache: "no-store", ...init })
  if (!res.ok) throw new Error(`HTTP ${res.status}`)
  return res.json() as Promise<T>
}

async function postJson<T>(url: string, data: unknown): Promise<T> {
  return fetchJson<T>(url, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(data),
  })
}

async function postForm<T>(url: string, data: Record<string, string>): Promise<T> {
  return fetchJson<T>(url, {
    method: "POST",
    headers: { "Content-Type": "application/x-www-form-urlencoded" },
    body: new URLSearchParams(data),
  })
}

function useFetch<T>(url: string | null, deps: unknown[] = []) {
  const [data, setData] = useState<T | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [loading, setLoading] = useState(Boolean(url))
  const [nonce, setNonce] = useState(0)
  useEffect(() => {
    if (!url) return
    let cancelled = false
    setLoading(true)
    fetchJson<T>(`${url}${url.includes("?") ? "&" : "?"}t=${Date.now()}`)
      .then((value) => { if (!cancelled) { setData(value); setError(null) } })
      .catch((err: Error) => { if (!cancelled) setError(err.message) })
      .finally(() => { if (!cancelled) setLoading(false) })
    return () => { cancelled = true }
  }, [url, nonce, ...deps])
  return { data, error, loading, refresh: () => setNonce((v) => v + 1) }
}

function formatDuration(ms: number): string {
  const safe = Math.max(0, ms)
  const sec = Math.floor(safe / 1000)
  if (sec < 60) return `${sec}秒`
  const min = Math.floor(sec / 60)
  if (min < 60) return `${min}分钟`
  const hr = Math.floor(min / 60)
  const remainMin = min % 60
  if (hr < 24) return remainMin > 0 ? `${hr}小时${remainMin}分钟` : `${hr}小时`
  const day = Math.floor(hr / 24)
  const remainHr = hr % 24
  return remainHr > 0 ? `${day}天${remainHr}小时` : `${day}天`
}

function formatDate(ms?: number): string {
  if (!ms) return "-"
  return new Date(ms).toLocaleDateString("zh-CN")
}

function formatDateTime(ms?: number): string {
  if (!ms) return "-"
  return new Date(ms).toLocaleString("zh-CN")
}

function relAge(ms?: number): string {
  if (!ms) return "-"
  const diff = Math.max(0, Date.now() - ms)
  if (diff < 60_000) return "刚刚"
  if (diff < 3_600_000) return `${Math.floor(diff / 60_000)}分钟前`
  if (diff < 86_400_000) return `${Math.floor(diff / 3_600_000)}小时前`
  return `${Math.floor(diff / 86_400_000)}天前`
}

function statusPill(status: string) {
  const meta = statusMeta[status] || statusMeta["需求对齐"]
  return <span className="react-status-pill" style={{ color: meta.color, background: meta.soft, borderColor: `${meta.color}55` }}>{status}</span>
}

function projectsOf(req: Requirement): string {
  return (req.projects?.length ? req.projects : [req.project]).filter(Boolean).join(" / ") || "-"
}

/**
 * Parse a stored `ones` value into a display-ready reference for the UI.
 * ONES issue links use hash routing, so `#/.../issue/<code>` takes priority
 * over the final pathname segment. Empty or missing values return null.
 */
function parseOnesRef(raw?: string): { raw: string; url: string | null; label: string } | null {
  const value = (raw || "").trim()
  if (!value) return null
  if (/^https?:\/\//i.test(value)) {
    let label = value
    try {
      const url = new URL(value)
      const issueCode = url.hash.match(/(?:^|\/)issue\/([^/?#]+)/i)?.[1]
      const pathSegment = url.pathname.split("/").filter(Boolean).pop()
      const segment = issueCode || pathSegment
      if (segment && segment.length <= 60) label = decodeURIComponent(segment)
    } catch { /* keep full value as label */ }
    return { raw: value, url: value, label }
  }
  return { raw: value, url: null, label: value }
}

/** Compact ONES status badge for the board card: warning when missing,
 * clickable link when a URL is stored, plain pill for a bare id. */
function onesBadge(ones?: string) {
  const ref = parseOnesRef(ones)
  if (!ref) return <span className="react-ones-badge react-ones-missing" title="未关联 ONES 任务，请联系产品在 ONES 上登记">⚠ 未关联 ONES</span>
  if (ref.url) return <a className="react-ones-badge react-ones-linked" href={ref.url} target="_blank" rel="noopener noreferrer" title={`ONES 任务：${ref.raw}`}>🔗 ONES</a>
  return <span className="react-ones-badge react-ones-id" title={`ONES 任务编号：${ref.label}`}>ONES</span>
}

const driveStateLabel: Record<AutoDriveState, string> = {
  queued: "排队中",
  running: "推进中",
  blocked: "有阻塞",
  done: "已完成",
  failed: "失败",
}

function driveStateBadge(job: AutoDriveJob) {
  return <span className={`react-drive-badge react-drive-${job.state}`}>{driveStateLabel[job.state]}</span>
}

function latestDriveJobsByReq(jobs: AutoDriveJob[]): Map<string, AutoDriveJob> {
  const map = new Map<string, AutoDriveJob>()
  for (const job of [...jobs].sort((a, b) => b.updatedAt - a.updatedAt)) {
    if (!map.has(job.reqId)) map.set(job.reqId, job)
  }
  return map
}

function PageChrome({ icon, eyebrow, title, description, actions, children }: { icon: React.ReactNode; eyebrow: string; title: string; description?: string; actions?: React.ReactNode; children: React.ReactNode }) {
  return (
    <div className="react-page">
      <motion.section className="react-hero react-page-hero" initial={{ opacity: 0, y: 16 }} animate={{ opacity: 1, y: 0 }} transition={{ duration: 0.42 }}>
        <div className="react-hero-grid" aria-hidden="true" />
        <div className="react-hero-copy">
          <span className="react-eyebrow">{icon} {eyebrow}</span>
          <h1>{title}</h1>
          {description ? <p>{description}</p> : null}
          {actions ? <div className="react-hero-actions">{actions}</div> : null}
        </div>
      </motion.section>
      {children}
    </div>
  )
}

function LoadingCard({ label = "正在加载…" }: { label?: string }) {
  return <div className="react-loading">{label}</div>
}

function ErrorCard({ error }: { error: string }) {
  return <div className="react-error">加载失败：{error}</div>
}

function EmptyCard({ children }: { children: React.ReactNode }) {
  return <div className="react-empty">{children}</div>
}

function KpiCard({ icon, label, value, sub, tone }: { icon: React.ReactNode; label: string; value: string | number; sub: string; tone: string }) {
  return (
    <motion.article className={`react-kpi react-kpi-${tone}`} variants={cardVariants} whileHover={{ y: -5, scale: 1.01 }} transition={{ type: "spring", stiffness: 260, damping: 24 }}>
      <div className="react-kpi-icon">{icon}</div>
      <span className="react-kpi-label">{label}</span>
      <motion.strong className="react-kpi-value" layout>{value}</motion.strong>
      <span className="react-kpi-sub">{sub}</span>
    </motion.article>
  )
}

function PipelineBar({ item, index }: { item: StatusCount; index: number }) {
  const meta = statusMeta[item.status] || { color: "#94a3b8", soft: "rgba(148, 163, 184, 0.14)", slug: "unknown" }
  return (
    <motion.div className="react-pipeline-row" initial={{ opacity: 0, x: -14 }} animate={{ opacity: 1, x: 0 }} transition={{ delay: 0.08 + index * 0.045 }}>
      <div className="react-pipeline-label"><span className="react-pipeline-dot" style={{ background: meta.color, boxShadow: `0 0 14px ${meta.color}66` }} /><span>{item.status}</span></div>
      <div className="react-pipeline-track"><motion.div className="react-pipeline-fill" style={{ background: `linear-gradient(90deg, ${meta.color}, ${meta.color}99)` }} initial={{ width: 0 }} animate={{ width: `${Math.max(1.5, item.percent)}%` }} transition={{ duration: 0.85, delay: 0.16 + index * 0.04, ease: [0.22, 1, 0.36, 1] }} /></div>
      <strong>{item.count}</strong><span>{item.percent}%</span>
    </motion.div>
  )
}

function DurationRow({ item, max, index }: { item: RequirementDuration; max: number; index: number }) {
  const meta = statusMeta[item.req.status] || statusMeta["需求对齐"]
  const pct = Math.min(100, (item.durationMs / Math.max(max, 1)) * 100)
  return (
    <motion.tr initial={{ opacity: 0, y: 8 }} animate={{ opacity: 1, y: 0 }} transition={{ delay: 0.16 + index * 0.035 }}>
      <td><a href={`/requirement?id=${encodeURIComponent(item.req.id)}`}>{item.req.title}</a><div className="react-duration-id">{item.req.id}</div></td>
      <td><span className="react-status-pill" style={{ color: meta.color, background: meta.soft, borderColor: `${meta.color}55` }}>{item.req.status}</span></td>
      <td className="react-muted">{(item.req.projects?.length ? item.req.projects : [item.req.project]).filter(Boolean).join(" / ") || "-"}</td>
      <td className="react-muted">{formatDate(item.req.createdAt)}</td>
      <td className="react-duration-cell"><motion.div className="react-duration-fill" initial={{ width: 0 }} animate={{ width: `${pct}%` }} transition={{ duration: 0.8, delay: 0.2 + index * 0.025 }} /><span>{formatDuration(item.durationMs)}</span></td>
    </motion.tr>
  )
}

function DashboardPage({ apiPath }: { apiPath: string }) {
  const { data: payload, error, loading, refresh } = useFetch<DashboardStatsPayload>(apiPath)
  const stats = payload?.stats
  const completionRate = useMemo(() => (!stats || stats.total === 0) ? 0 : Math.round((stats.completedCount / stats.total) * 100), [stats])
  const activeRate = useMemo(() => (!stats || stats.total === 0) ? 0 : Math.round((stats.inProgressCount / stats.total) * 100), [stats])

  return (
    <div className="react-dashboard">
      <motion.section className="react-hero" initial={{ opacity: 0, y: 16 }} animate={{ opacity: 1, y: 0 }} transition={{ duration: 0.48, ease: [0.22, 1, 0.36, 1] }}>
        <div className="react-hero-grid" aria-hidden="true" />
        <div className="react-hero-copy">
          <span className="react-eyebrow"><LayoutDashboard size={14} /> Agent Panel Control Center</span>
          <h1>需求状态与交付健康度</h1>
          <p>统一 React 控制台：动态 KPI、状态分布动画、交付周期表格和丝滑页面切换，所有主要页面都由 React 接管。</p>
          <div className="react-hero-actions"><button type="button" onClick={refresh} disabled={loading}><RefreshCw size={15} className={loading ? "react-spin" : ""} /> 刷新数据</button><a href="/projects"><Sparkles size={15} /> 进入需求看板</a></div>
        </div>
        <motion.div className="react-orb" animate={{ y: [0, -8, 0], rotate: [0, 2, 0] }} transition={{ duration: 5, repeat: Infinity, ease: "easeInOut" }}><strong>{stats?.total ?? "—"}</strong><span>REQS</span></motion.div>
      </motion.section>
      {error ? <ErrorCard error={error} /> : !stats ? <LoadingCard label="正在加载 dashboard stats…" /> : (
        <motion.div className="react-dashboard-body" initial="hidden" animate="show" transition={{ staggerChildren: 0.06 }}>
          <motion.section className="react-kpi-grid" variants={{ hidden: {}, show: { transition: { staggerChildren: 0.07 } } }}>
            <KpiCard icon={<Gauge size={20} />} label="需求总数" value={stats.total} sub="Tracked requirements" tone="total" />
            <KpiCard icon={<CheckCircle2 size={20} />} label="已完成" value={stats.completedCount} sub={`${completionRate}% complete`} tone="done" />
            <KpiCard icon={<Activity size={20} />} label="进行中" value={stats.inProgressCount} sub={`${activeRate}% active`} tone="active" />
            <KpiCard icon={<Clock3 size={20} />} label="平均交付时长" value={formatDuration(stats.avgDeliveryMs) || "-"} sub={`中位数 ${formatDuration(stats.medianDeliveryMs)} · 最长 ${formatDuration(stats.maxDeliveryMs)}`} tone="avg" />
          </motion.section>
          <section className="react-content-grid">
            <motion.article className="react-panel" variants={cardVariants}><PanelHead kicker="Pipeline" title="需求状态分布" chip={`${stats.statusCounts.length} stages`} /><div className="react-pipeline-list">{stats.statusCounts.map((item, index) => <PipelineBar key={item.status} item={item} index={index} />)}</div></motion.article>
            <motion.article className="react-panel react-delivery-panel" variants={cardVariants}><PanelHead kicker="Delivery" title="需求交付时长" chip={<><TimerReset size={13} /> Top durations</>} />{stats.durations.length === 0 ? <EmptyCard>暂无需求数据。</EmptyCard> : <DurationTable durations={stats.durations.slice(0, 18)} max={stats.maxDeliveryMs} />}</motion.article>
          </section>
        </motion.div>
      )}
    </div>
  )
}

function PanelHead({ kicker, title, chip }: { kicker: string; title: string; chip?: React.ReactNode }) {
  return <div className="react-panel-head"><div><span>{kicker}</span><h2>{title}</h2></div>{chip ? <em>{chip}</em> : null}</div>
}

function DurationTable({ durations, max }: { durations: RequirementDuration[]; max: number }) {
  return <div className="react-table-wrap"><table className="react-duration-table"><thead><tr><th>需求</th><th>状态</th><th>项目</th><th>创建时间</th><th>交付时长</th></tr></thead><tbody>{durations.map((item, index) => <DurationRow key={item.req.id} item={item} max={max} index={index} />)}</tbody></table></div>
}

function ProjectsPage() {
  const { data, error, loading } = useFetch<{ requirements: Requirement[] }>("/api/requirements")
  const drive = useFetch<AutoDrivePayload>("/api/requirement/auto-drive")
  const params = new URLSearchParams(window.location.search)
  const [project, setProject] = useState(params.get("project") || "")
  const [subproject, setSubproject] = useState(params.get("subproject") || "")
  const [createdFrom, setCreatedFrom] = useState(params.get("createdFrom") || "")
  const [createdTo, setCreatedTo] = useState(params.get("createdTo") || "")
  const [statuses, setStatuses] = useState<string[]>(params.getAll("status"))
  const [category, setCategory] = useState<string>(params.get("category") || "")
  const [keyword, setKeyword] = useState(params.get("q") || "")
  const [selectedReqIds, setSelectedReqIds] = useState<string[]>([])
  const [driving, setDriving] = useState(false)
  const [driveHint, setDriveHint] = useState<string | null>(null)
  const reqs = data?.requirements || []
  const driveByReq = useMemo(() => latestDriveJobsByReq(drive.data?.jobs || []), [drive.data])
  const projects = useMemo(() => [...new Set(reqs.flatMap((r) => r.projects?.length ? r.projects : [r.project]).filter(Boolean))].sort(), [reqs])
  const subprojects = useMemo(() => [...new Set(reqs.filter((r) => !project || (r.projects?.length ? r.projects : [r.project]).includes(project)).map((r) => r.groupPath?.[0] || "").filter(Boolean))].sort(), [reqs, project])
  const counts = useMemo(() => Object.fromEntries(REQ_STATUSES.map((s) => [s, reqs.filter((r) => r.status === s).length])), [reqs]) as Record<string, number>
  const filtered = useMemo(() => reqs.filter((r) => {
    if (statuses.length === 0 && r.status === "已完成") return false
    if (statuses.length && !statuses.includes(r.status)) return false
    if (category && (r.category ?? "需求") !== category) return false
    if (project && !(r.projects?.length ? r.projects : [r.project]).includes(project)) return false
    if (subproject && r.groupPath?.[0] !== subproject) return false
    if (createdFrom && r.createdAt < new Date(`${createdFrom}T00:00:00`).getTime()) return false
    if (createdTo && r.createdAt > new Date(`${createdTo}T23:59:59`).getTime()) return false
    if (keyword.trim()) {
      const kw = keyword.trim().toLowerCase()
      const haystack = [r.id, r.title, r.description || "", projectsOf(r)].join(" ").toLowerCase()
      if (!haystack.includes(kw)) return false
    }
    return true
  }).sort((a, b) => b.updatedAt - a.updatedAt), [reqs, statuses, category, project, subproject, createdFrom, createdTo, keyword])
  const selectableIds = useMemo(() => filtered.filter((r) => r.reqDir && r.id !== "__default__").map((r) => r.id), [filtered])
  const allVisibleSelected = selectableIds.length > 0 && selectableIds.every((id) => selectedReqIds.includes(id))
  const apply = () => {
    const q = new URLSearchParams()
    if (createdFrom) q.set("createdFrom", createdFrom)
    if (createdTo) q.set("createdTo", createdTo)
    if (project) q.set("project", project)
    if (subproject) q.set("subproject", subproject)
    if (category) q.set("category", category)
    if (keyword.trim()) q.set("q", keyword.trim())
    statuses.forEach((s) => q.append("status", s))
    window.location.href = `/projects${q.toString() ? `?${q}` : ""}`
  }
  const toggleSelected = (reqId: string, checked: boolean) => setSelectedReqIds((cur) => checked ? [...new Set([...cur, reqId])] : cur.filter((id) => id !== reqId))
  const toggleAllVisible = (checked: boolean) => setSelectedReqIds((cur) => checked ? [...new Set([...cur, ...selectableIds])] : cur.filter((id) => !selectableIds.includes(id)))
  const autoDriveSelected = async () => {
    if (selectedReqIds.length === 0) return
    setDriving(true)
    setDriveHint(null)
    try {
      const res = await postJson<{ jobs: AutoDriveJob[]; errors: Array<{ reqId: string; error: string }> }>("/api/requirement/auto-drive", { reqIds: selectedReqIds })
      setDriveHint(`已派发 ${res.jobs.length} 个需求${res.errors?.length ? `，${res.errors.length} 个失败` : ""}`)
      setSelectedReqIds([])
      drive.refresh()
    } catch (err) {
      setDriveHint(err instanceof Error ? err.message : String(err))
    } finally {
      setDriving(false)
    }
  }
  return <PageChrome icon={<ListChecks size={15} />} eyebrow="Requirements" title="需求进度看板" description="按项目、状态和创建时间筛选需求，查看关联 session 和最近更新。" actions={<><button onClick={drive.refresh}><RefreshCw size={15} />刷新自动推进</button>{drive.data?.blocked ? <a href="/requirement?id=__default__"><AlertTriangle size={15} />{drive.data.blocked} 个阻塞</a> : null}</>}>
    {error ? <ErrorCard error={error} /> : loading ? <LoadingCard /> : <>
      <section className="react-panel react-filter-panel"><PanelHead kicker="Filter" title="需求筛选" chip={`${filtered.length} tracked`} />
        <div className="react-filter-grid">
          <label className="react-filter-grow">关键词搜索<input type="search" value={keyword} onChange={(e) => setKeyword(e.target.value)} onKeyDown={(e) => { if (e.key === "Enter") apply() }} placeholder="搜索 ID、标题、描述或项目…" /></label>
          <label>类别<select value={category} onChange={(e) => setCategory(e.target.value)}><option value="">全部类别</option>{REQ_CATEGORIES.map((c) => <option key={c} value={c}>{c}</option>)}</select></label>
          <label>创建时间起<input type="date" value={createdFrom} onChange={(e) => setCreatedFrom(e.target.value)} /></label><label>创建时间止<input type="date" value={createdTo} onChange={(e) => setCreatedTo(e.target.value)} /></label><label>一级项目<select value={project} onChange={(e) => { setProject(e.target.value); setSubproject("") }}><option value="">全部项目</option>{projects.map((p) => <option key={p} value={p}>{p}</option>)}</select></label><label>二级项目<select value={subproject} onChange={(e) => setSubproject(e.target.value)} disabled={!project}><option value="">{project ? "全部二级项目" : "请先选择一级项目"}</option>{subprojects.map((p) => <option key={p} value={p}>{p}</option>)}</select></label>
        </div>
        <div className="react-status-options">{REQ_STATUSES.map((s) => <label key={s} className={`react-status-option ${statuses.includes(s) ? "active" : ""}`}><input type="checkbox" checked={statuses.includes(s)} onChange={(e) => setStatuses((cur) => e.target.checked ? [...cur, s] : cur.filter((x) => x !== s))} /><span>{s}</span><strong>{counts[s] || 0}</strong></label>)}</div>
        <div className="react-actions"><button onClick={apply}>应用筛选</button><a href="/projects">重置</a></div>
      </section>
      <section className="react-panel react-drive-toolbar"><PanelHead kicker="Auto Drive" title="批量自动推进" chip={`${selectedReqIds.length} selected`} />
        <div className="react-drive-controls"><label className="react-check-inline"><input type="checkbox" checked={allVisibleSelected} disabled={selectableIds.length === 0} onChange={(e) => toggleAllVisible(e.target.checked)} />选择当前筛选可推进需求</label><button onClick={autoDriveSelected} disabled={selectedReqIds.length === 0 || driving}>{driving ? "派发中…" : "自动推进需求"}</button><button onClick={() => setSelectedReqIds([])} disabled={!selectedReqIds.length}>清空选择</button></div>
        <p className="react-muted">自动推进会创建 pi agent 任务；遇到需求对齐、测试覆盖、上线风险等人工门禁会停止，并在通知中心与需求卡片标记阻塞。</p>
        {driveHint ? <p className="react-drive-hint">{driveHint}</p> : null}
        {drive.error ? <ErrorCard error={drive.error} /> : null}
        {drive.data ? <div className="react-drive-summary"><span>运行/排队 {drive.data.active}</span><span>阻塞/失败 {drive.data.blocked}</span><span>进程队列 active {drive.data.queue.active} · queued {drive.data.queue.queued}</span></div> : null}
      </section>
      <div className="react-card-list">{filtered.length === 0 ? <EmptyCard>没有符合当前筛选条件的需求。</EmptyCard> : filtered.map((req, index) => <RequirementCard key={req.id} req={req} index={index} driveJob={driveByReq.get(req.id)} selected={selectedReqIds.includes(req.id)} onToggle={toggleSelected} />)}</div>
    </>}
  </PageChrome>
}

function RequirementCard({ req, index, driveJob, selected, onToggle }: { req: Requirement; index: number; driveJob?: AutoDriveJob; selected: boolean; onToggle: (reqId: string, checked: boolean) => void }) {
  const selectable = Boolean(req.reqDir && req.id !== "__default__")
  const blocked = driveJob?.state === "blocked" || driveJob?.state === "failed"
  return <motion.article className={`react-list-card react-req-card ${blocked ? "react-drive-blocked-card" : ""}`} initial={{ opacity: 0, y: 12 }} animate={{ opacity: 1, y: 0 }} transition={{ delay: Math.min(index, 16) * 0.025 }} whileHover={{ y: -3 }}><label className="react-card-select"><input type="checkbox" checked={selected} disabled={!selectable} onChange={(e) => onToggle(req.id, e.target.checked)} /><span>选择</span></label><div><span className="react-card-id">{req.id}</span><h3><a href={`/requirement?id=${encodeURIComponent(req.id)}`}>{req.title}</a></h3><p>{req.description || "暂无描述"}</p><div className="react-card-meta"><span>{projectsOf(req)}</span><span>{req.sessionIds?.length || 0} session(s)</span><span>更新 {relAge(req.updatedAt)}</span>{driveJob ? driveStateBadge(driveJob) : null}{driveJob?.blockers?.length ? <span className="react-drive-blocker">阻塞 {driveJob.blockers.length}</span> : null}</div></div><div className="react-card-side">{req.effortEstimate ? <span className="react-effort-badge" title={`系数 ${req.effortEstimate.coefficient}× · ${req.effortEstimate.estimatedHours}h`}>{req.effortEstimate.estimatedHours}h</span> : null}{req.category === "线上问题" ? <span className="react-status-pill" style={{ color: "#f87171", background: "rgba(239, 68, 68, 0.14)", borderColor: "rgba(239, 68, 68, 0.4)" }}>线上问题</span> : null}{statusPill(req.status)}{onesBadge(req.ones)}<a href={`/requirement/review?id=${encodeURIComponent(req.id)}`}>Review</a><a href={`/requirement/release?id=${encodeURIComponent(req.id)}`}>Release</a></div></motion.article>
}

function SessionsPage() {
  const params = new URLSearchParams(window.location.search)
  const days = params.get("days") || "7"
  const { data, error, loading, refresh } = useFetch<ApiSessions>(`/api/sessions?days=${encodeURIComponent(days)}`, [days])
  const sessions = data?.sessions || []
  const top = sessions.filter((s) => !s.parentId)
  const summary = {
    total: top.length,
    running: top.filter((s) => s.status === "running").length,
    idle: top.filter((s) => s.status === "idle").length,
    stale: top.filter((s) => s.status === "stale").length,
  }
  return <PageChrome icon={<TerminalSquare size={15} />} eyebrow="Sessions" title="Sessions" description="浏览本机 agent session，按最近活跃度识别 running / idle / stale。" actions={<button onClick={refresh}><RefreshCw size={15} />刷新</button>}>
    {error ? <ErrorCard error={error} /> : loading ? <LoadingCard /> : <>
      <section className="react-kpi-grid"><KpiCard icon={<AlertTriangle size={20} />} label="Backlog" value={summary.stale} sub="stale > 24h" tone="stale" /><KpiCard icon={<Activity size={20} />} label="Running" value={summary.running} sub="< 5m touched" tone="done" /><KpiCard icon={<Clock3 size={20} />} label="Idle" value={summary.idle} sub="5m–24h" tone="avg" /><KpiCard icon={<Server size={20} />} label="Total" value={summary.total} sub={data?.harness || "harness"} tone="active" /></section>
      <div className="react-tab-row">{[1,3,7,14,30,0].map((d) => <a key={d} className={String(d) === days ? "active" : ""} href={`/sessions?days=${d}`}>{d === 0 ? "全部时间" : `近 ${d} 天`}</a>)}</div>
      <div className="react-card-list">{top.length === 0 ? <EmptyCard>No sessions in selected range.</EmptyCard> : top.map((s, i) => <SessionCard key={s.id} session={s} childSessions={sessions.filter((c) => c.parentId === s.id)} index={i} />)}</div>
    </>}
  </PageChrome>
}

function SessionCard({ session, childSessions, index }: { session: SessionInfo; childSessions: SessionInfo[]; index: number }) {
  const meta = session.status === "running" ? statusMeta["已完成"] : session.status === "idle" ? statusMeta["方案设计"] : statusMeta["需求对齐"]
  return <motion.article className="react-list-card react-session-card" initial={{ opacity: 0, y: 12 }} animate={{ opacity: 1, y: 0 }} transition={{ delay: Math.min(index, 16) * 0.025 }} whileHover={{ y: -3 }}><div><span className="react-card-id">{session.id}</span><h3><a href={`/session?id=${encodeURIComponent(session.id)}`}>{session.title || "Untitled session"}</a></h3><p>{session.directory || "No directory"}</p><div className="react-card-meta"><span style={{ color: meta.color }}>{session.status}</span><span>{session.model || session.provider || "model n/a"}</span><span>更新 {relAge(session.updated || session.created)}</span>{childSessions.length ? <span>{childSessions.length} child</span> : null}</div></div><div className="react-card-side"><a href={`/session?id=${encodeURIComponent(session.id)}`}>Open terminal</a></div></motion.article>
}

function ReportsPage() {
  const { data, error, loading } = useFetch<ReportListItem[]>("/api/reports")
  return <PageChrome icon={<FileText size={15} />} eyebrow="Reports" title="Experience Reports" description="查看经验总结候选，进入详情页确认或拒绝候选项。">{error ? <ErrorCard error={error} /> : loading ? <LoadingCard /> : <div className="react-grid-list">{(data || []).length === 0 ? <EmptyCard>暂无报告。</EmptyCard> : (data || []).map((r, i) => <motion.a className="react-tile-card" key={r.path} href={`/report?path=${encodeURIComponent(r.path)}`} initial={{ opacity: 0, y: 12 }} animate={{ opacity: 1, y: 0 }} transition={{ delay: i * 0.025 }}><span className="react-card-id">{r.meta?.session || "report"}</span><h3>{r.path.split("/").pop()}</h3><p>{r.meta?.date || r.path}</p></motion.a>)}</div>}</PageChrome>
}

function ReportDetailPage() {
  const path = new URLSearchParams(window.location.search).get("path") || ""
  const { data, error, loading } = useFetch<any>(path ? `/api/report?path=${encodeURIComponent(path)}` : null, [path])
  const [selected, setSelected] = useState<string[]>([])
  const candidates = collectCandidates(data)
  const submit = async (mode: "confirm" | "reject") => {
    await postJson("/api/confirm", { reportPath: path, confirmedIds: mode === "confirm" ? selected : [], rejectedIds: mode === "reject" ? selected : [], mode })
    alert("已提交")
  }
  return <PageChrome icon={<FileText size={15} />} eyebrow="Report" title="Report Detail" description={path}>{error ? <ErrorCard error={error} /> : loading ? <LoadingCard /> : !data ? <EmptyCard>没有找到报告。</EmptyCard> : <><div className="react-actions"><button disabled={!selected.length} onClick={() => submit("confirm")}>确认选中</button><button disabled={!selected.length} onClick={() => submit("reject")}>拒绝选中</button></div><div className="react-card-list">{candidates.length ? candidates.map((c, i) => <label key={c.id || i} className="react-list-card react-check-card"><input type="checkbox" checked={selected.includes(c.id)} onChange={(e) => setSelected((cur) => e.target.checked ? [...cur, c.id] : cur.filter((x) => x !== c.id))} /><div><span className="react-card-id">{c.id || `candidate-${i+1}`}</span><h3>{c.title || c.summary || "Candidate"}</h3><p>{c.reason || c.description || JSON.stringify(c).slice(0, 220)}</p></div></label>) : <pre className="react-json-preview">{JSON.stringify(data, null, 2)}</pre>}</div></>}</PageChrome>
}

function collectCandidates(report: any): any[] {
  if (!report) return []
  if (Array.isArray(report.candidates)) return report.candidates.map((c: any, i: number) => ({ id: c.id || c.cid || String(i + 1), ...c }))
  const buckets = [report.high, report.medium, report.low, report.items].filter(Array.isArray).flat()
  return buckets.map((c: any, i: number) => ({ id: c.id || c.cid || String(i + 1), ...c }))
}

function SessionPage() {
  const params = new URLSearchParams(window.location.search)
  const id = params.get("id") || ""
  const { data, error, loading } = useFetch<SessionInfo>(id ? `/api/session?id=${encodeURIComponent(id)}` : null, [id])
  return <PageChrome icon={<TerminalSquare size={15} />} eyebrow="Terminal" title={data?.title || id || "Session"} description={data?.directory || "Embedded terminal session"}>{error ? <ErrorCard error={error} /> : loading ? <LoadingCard /> : !data ? <EmptyCard>Session not found.</EmptyCard> : <><section className="react-panel"><div className="react-meta-grid"><span>ID <code>{data.id}</code></span><span>Status {data.status}</span><span>Model {data.model || data.provider || "-"}</span><span>Updated {formatDateTime(data.updated || data.created)}</span></div></section><TerminalPane sessionId={data.id} /></>}</PageChrome>
}

function TerminalPane({ sessionId }: { sessionId: string }) {
  const hostRef = useRef<HTMLDivElement | null>(null)
  const [status, setStatus] = useState("initializing…")
  useEffect(() => {
    const host = hostRef.current
    if (!host) return
    const term = new Terminal({ cursorBlink: true, fontSize: 13, fontFamily: '"Noto Sans Mono CJK SC", "JetBrains Mono", monospace', theme: { background: "#0a0d12", foreground: "#d4dae3", cursor: "#22d3ee" } })
    const fit = new FitAddon()
    term.loadAddon(fit)
    term.open(host)
    setTimeout(() => fit.fit(), 0)
    const proto = window.location.protocol === "https:" ? "wss" : "ws"
    const ws = new WebSocket(`${proto}://${window.location.host}/ws/session-terminal?id=${encodeURIComponent(sessionId)}`)
    ws.addEventListener("open", () => setStatus("connected"))
    ws.addEventListener("message", (evt) => {
      const raw = typeof evt.data === "string" ? evt.data : ""
      if (raw.startsWith("{")) {
        try {
          const msg = JSON.parse(raw)
          if (msg.type === "ready") { setStatus(`ready: ${msg.id || sessionId}`); return }
          if (msg.type === "exit") { setStatus(`exited: ${msg.code}`); return }
          if (msg.type === "error") { setStatus(`error: ${msg.message}`); return }
        } catch { /* fall through */ }
      }
      term.write(raw)
    })
    ws.addEventListener("close", () => setStatus("disconnected"))
    term.onData((data) => { if (ws.readyState === WebSocket.OPEN) ws.send(data) })
    const onResize = () => { fit.fit(); if (ws.readyState === WebSocket.OPEN) ws.send(JSON.stringify({ type: "resize", cols: term.cols, rows: term.rows })) }
    window.addEventListener("resize", onResize)
    return () => { window.removeEventListener("resize", onResize); ws.close(); term.dispose() }
  }, [sessionId])
  return <section className="react-terminal"><div className="react-terminal-head"><span /><span /><span /><strong>{status}</strong></div><div ref={hostRef} className="react-terminal-host" /></section>
}

function RequirementsData() {
  return useFetch<{ requirements: Requirement[] }>("/api/requirements")
}

function AutoDrivePanel({ req }: { req: Requirement }) {
  const url = req.id === "__default__" ? "/api/requirement/auto-drive" : `/api/requirement/auto-drive?reqId=${encodeURIComponent(req.id)}`
  const drive = useFetch<AutoDrivePayload>(url, [req.id])
  const [busy, setBusy] = useState(false)
  const [hint, setHint] = useState<string | null>(null)
  const canStart = Boolean(req.reqDir && req.id !== "__default__")
  const start = async () => {
    if (!canStart) return
    setBusy(true)
    setHint(null)
    try {
      const res = await postJson<{ jobs: AutoDriveJob[]; errors: Array<{ reqId: string; error: string }> }>("/api/requirement/auto-drive", { reqIds: [req.id] })
      setHint(res.errors?.length ? res.errors.map((e) => `${e.reqId}: ${e.error}`).join("；") : "已派发自动推进 agent")
      drive.refresh()
    } catch (err) {
      setHint(err instanceof Error ? err.message : String(err))
    } finally {
      setBusy(false)
    }
  }
  const jobs = drive.data?.jobs || []
  return <section className="react-panel react-drive-panel"><PanelHead kicker="Auto Drive" title="自动推进 agent 状态" chip={drive.data ? `${drive.data.active} active / ${drive.data.blocked} blocked` : undefined} />
    <div className="react-actions"><button onClick={start} disabled={!canStart || busy}>{busy ? "派发中…" : "自动推进本需求"}</button><button onClick={drive.refresh}><RefreshCw size={15} />刷新状态</button></div>
    {!canStart ? <p className="react-muted">当前是默认/虚拟需求或没有磁盘目录，只展示全部自动推进任务状态。</p> : null}
    {hint ? <p className="react-drive-hint">{hint}</p> : null}
    {drive.error ? <ErrorCard error={drive.error} /> : drive.loading ? <LoadingCard label="正在读取自动推进状态…" /> : jobs.length === 0 ? <p className="react-muted">暂无自动推进任务。</p> : <div className="react-drive-job-list">{jobs.map((job) => <AutoDriveJobCard key={job.id} job={job} />)}</div>}
  </section>
}

function AutoDriveJobCard({ job }: { job: AutoDriveJob }) {
  return <details className={`react-drive-job react-drive-job-${job.state}`} open={job.state === "blocked" || job.state === "failed" || job.state === "running"}><summary><span>{driveStateBadge(job)}<strong>{job.reqTitle}</strong></span><em>{relAge(job.updatedAt)}</em></summary><div className="react-drive-job-body"><div className="react-meta-grid"><span>Job <code>{job.id}</code></span><span>Req <code>{job.reqId}</code></span><span>Session {job.sessionId ? <a href={`/session?id=${encodeURIComponent(job.sessionId)}`}>{job.sessionId}</a> : "-"}</span><span>耗时 {job.durationMs ? formatDuration(job.durationMs) : "-"}</span></div><p>{job.summary}</p>{job.blockers?.length ? <div className="react-drive-blockers"><strong>阻塞 / 待人工确认</strong><ul>{job.blockers.map((b, i) => <li key={i}>{b}</li>)}</ul></div> : null}{job.stdout || job.stderr ? <pre className="react-json-preview">{[job.stdout, job.stderr].filter(Boolean).join("\n\n--- stderr ---\n")}</pre> : null}</div></details>
}

function AttachmentPanel({ req }: { req: Requirement }) {
  const { data, error, loading } = useFetch<{ attachments: Array<{ filename: string; size: number; mtime: number }> }>(`/api/requirement/attachments?reqId=${encodeURIComponent(req.id)}`, [req.id])
  const attachments = data?.attachments || []
  if (loading) return null
  if (error) return null
  if (attachments.length === 0) return null
  return <section className="react-panel"><PanelHead kicker="Files" title="附件" chip={`${attachments.length}`} />
    <div className="react-attach-list">{attachments.map((a) => <div key={a.filename} className="react-attach-item"><span className="react-attach-name">📎 {a.filename}</span><span className="react-muted react-attach-size">{(a.size / 1024).toFixed(1)}KB</span><span className="react-muted react-attach-time">{relAge(a.mtime)}</span><div className="react-attach-actions"><a href={`/requirement/attachments/view?reqId=${encodeURIComponent(req.id)}&filename=${encodeURIComponent(a.filename)}`} target="_blank" rel="noopener noreferrer">查看</a><a href={`/requirement/attachments/download?reqId=${encodeURIComponent(req.id)}&filename=${encodeURIComponent(a.filename)}`} download={a.filename}>下载</a></div></div>)}</div>
  </section>
}

function EffortEstimatePanel({ req, onUpdated }: { req: Requirement; onUpdated: () => void }) {
  const estimate = req.effortEstimate
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const run = async () => {
    setLoading(true)
    setError(null)
    try {
      const res = await fetch("/api/requirement/effort-estimate", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ reqId: req.id }),
      })
      const data = await res.json()
      if (data.ok) {
        onUpdated()
      } else {
        setError(data.error || "评估失败")
      }
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err))
    } finally {
      setLoading(false)
    }
  }
  return <section className="react-panel react-effort-panel"><PanelHead kicker="Effort" title="工时评估" chip={estimate ? `${estimate.estimatedHours}h` : undefined} />
    {estimate ? <div className="react-effort-result"><div className="react-effort-headline"><div className="react-effort-coeff"><strong>{estimate.coefficient}×</strong><span>系数</span></div><div className="react-effort-hours"><strong>{estimate.estimatedHours}h</strong><span>预估工时</span></div><div className="react-effort-base"><span>基数 {estimate.baseHours}h</span></div></div><p className="react-muted">{estimate.summary}</p>{estimate.factors.length > 0 ? <div className="react-effort-factors">{estimate.factors.map((f) => <div key={f.name} className="react-effort-factor"><div className="react-effort-factor-head"><span>{f.name}</span><span className={`react-effort-score react-effort-score-${f.score}`}>{"★".repeat(f.score)}{"☆".repeat(5 - f.score)}</span></div>{f.reason ? <p className="react-muted">{f.reason}</p> : null}</div>)}</div> : null}<div className="react-effort-meta"><span>模型 {estimate.model}</span><span>评估于 {formatDateTime(estimate.updatedAt)}</span></div><div className="react-actions"><button onClick={run} disabled={loading}>{loading ? "评估中…" : "重新评估"}</button></div></div> : <div><p className="react-muted">点击「评估工时」调用 AI 分析需求文件，生成相对工时系数。最终工时 = 基础工时 × 系数。</p><div className="react-actions"><button onClick={run} disabled={loading}>{loading ? "评估中…（10-30 秒）" : "评估工时"}</button></div></div>}
    {error ? <p className="react-effort-error">{error}</p> : null}
  </section>
}

function RecommendationPanel({ req, onBound }: { req: Requirement; onBound: () => void }) {
  const { data, error, loading } = useFetch<{ recommendations: Array<{ session: SessionInfo; score: number; reasons: string[] }> }>(`/api/requirement/recommendations?id=${encodeURIComponent(req.id)}`, [req.id])
  const recos = data?.recommendations || []
  const [binding, setBinding] = useState<string | null>(null)
  const bind = async (sessionId: string) => {
    setBinding(sessionId)
    try {
      await fetch("/api/requirement/associate", {
        method: "POST",
        headers: { "Content-Type": "application/x-www-form-urlencoded", Accept: "application/json" },
        body: new URLSearchParams({ reqId: req.id, sessionId }),
      })
      onBound()
    } finally {
      setBinding(null)
    }
  }
  if (loading || error || recos.length === 0) return null
  return <section className="react-panel react-reco-panel"><PanelHead kicker="Recommend" title="疑似相关 Session" chip={`${recos.length}`} /><p className="react-muted">根据标题、路径和关键词匹配推荐，点击绑定关联到当前需求。</p><div className="react-reco-list">{recos.map((reco) => <div key={reco.session.id} className="react-reco-item"><div className="react-reco-info"><a href={`/session?id=${encodeURIComponent(reco.session.id)}`}><code>{reco.session.id.slice(0, 20)}…</code></a><span className="react-reco-title">{reco.session.title || ""}</span><span className="react-muted">{relAge(reco.session.updated || reco.session.created)}</span></div><div className="react-reco-meta"><span className="react-reco-score">{reco.score} 分</span><span className="react-muted react-reco-reasons">{reco.reasons.slice(0, 3).join(" · ")}</span><button onClick={() => bind(reco.session.id)} disabled={binding === reco.session.id}>{binding === reco.session.id ? "绑定中…" : "绑定"}</button></div></div>)}</div></section>
}

function SessionChipList({ sessionIds }: { sessionIds: string[] }) {
  const COLLAPSE_THRESHOLD = 4
  const [expanded, setExpanded] = useState(false)
  const needsCollapse = sessionIds.length > COLLAPSE_THRESHOLD
  const visible = needsCollapse && !expanded ? sessionIds.slice(0, COLLAPSE_THRESHOLD) : sessionIds
  const hiddenCount = sessionIds.length - COLLAPSE_THRESHOLD
  return <div className="react-chip-list react-chip-list-collapsible">{visible.map((sid) => <a key={sid} href={`/session?id=${sid}`}>{sid}</a>)}{needsCollapse ? <button type="button" className="react-chip-toggle" onClick={() => setExpanded((v) => !v)}>{expanded ? `收起` : `展开全部 (${hiddenCount} 更多)`}</button> : null}</div>
}

function OnesPanel({ req }: { req: Requirement }) {
  const [ones, setOnes] = useState(req.ones ?? "")
  const [saving, setSaving] = useState(false)
  const [feedback, setFeedback] = useState<{ tone: "success" | "error"; message: string } | null>(null)
  useEffect(() => { setOnes(req.ones ?? "") }, [req.ones])
  const ref = parseOnesRef(req.ones)
  const draftRef = parseOnesRef(ones)
  const changed = ones.trim() !== (req.ones ?? "").trim()
  const recognizedCode = draftRef && (draftRef.url === null || draftRef.label !== draftRef.raw) ? draftRef.label : null
  const submit = async () => {
    if (saving || !changed) return
    setSaving(true)
    setFeedback(null)
    try {
      const result = await postForm<{ ones: string; ref: { label: string } | null }>("/api/requirement/ones", { reqId: req.id, ones })
      const message = result.ref
        ? `保存成功，ONES 编码为 ${result.ref.label}。正在刷新页面…`
        : "ONES 关联已清除。正在刷新页面…"
      setFeedback({ tone: "success", message })
      window.setTimeout(() => window.location.reload(), 1_800)
    } catch (err) {
      setFeedback({ tone: "error", message: `保存失败：${err instanceof Error ? err.message : String(err)}` })
      setSaving(false)
    }
  }
  return <section className="react-panel"><PanelHead kicker="ONES" title="ONES 任务关联" chip={ref ? (ref.url ? <a className="react-ones-badge react-ones-linked" href={ref.url} target="_blank" rel="noopener noreferrer" title={ref.raw}>🔗 {ref.label}</a> : <span className="react-ones-badge react-ones-id" title="ONES 任务编号（无链接）">ONES: {ref.label}</span>) : <span className="react-ones-badge react-ones-missing" title="未关联 ONES 任务">⚠ 未关联</span>} /><p className="react-muted">关联产品在 ONES 上登记的任务，便于跳转查看和填写工时。粘贴 ONES 网址会自动提取任务编码；只填编号则仅展示不跳转。留空保存可清除关联。</p><div className="react-inline-form"><input value={ones} onChange={(e) => { setOnes(e.target.value); setFeedback(null) }} placeholder="ONES 任务网址或编号，如 https://ones.example.com/project/#/team/.../issue/JTYC-123 或 JTYC-123" spellCheck={false} autoComplete="off" /><button onClick={submit} disabled={saving || !changed}>{saving ? "保存中…" : "保存"}</button></div>{recognizedCode && changed ? <p className="react-ones-recognized">已识别 ONES 编码：<code>{recognizedCode}</code></p> : null}<AnimatePresence>{feedback ? <motion.div className={`react-save-feedback react-save-feedback-${feedback.tone}`} role="status" aria-live="polite" initial={{ opacity: 0, y: 10 }} animate={{ opacity: 1, y: 0 }} exit={{ opacity: 0, y: 6 }}>{feedback.tone === "success" ? <CheckCircle2 size={18} /> : <AlertTriangle size={18} />}<span>{feedback.message}</span></motion.div> : null}</AnimatePresence></section>
}

function RequirementPage({ tool }: { tool?: "review" | "release" | "extract" | "recall" | "auto-extract" }) {
  const id = new URLSearchParams(window.location.search).get("id") || new URLSearchParams(window.location.search).get("reqId") || ""
  const sessionId = new URLSearchParams(window.location.search).get("sessionId") || ""
  const { data, error, loading, refresh } = RequirementsData()
  const req = data?.requirements.find((r) => r.id === id)
  const [note, setNote] = useState("")
  const [status, setStatus] = useState<ReqStatus | "">("")
  const [savedStatus, setSavedStatus] = useState<ReqStatus | undefined>(undefined)
  const [savingStatus, setSavingStatus] = useState(false)
  const [category, setCategory] = useState<ReqCategory | "">("")
  const [savingCategory, setSavingCategory] = useState(false)
  const [command, setCommand] = useState("")
  useEffect(() => { if (req) setSavedStatus(req.status) }, [req?.status])
  const submitStatus = async () => { if (!req || !status || savingStatus) return; setSavingStatus(true); try { await postForm("/api/requirement/status", { reqId: req.id, status, note }); setSavedStatus(status); refresh() } finally { setSavingStatus(false) } }
  const submitCategory = async () => { if (!req || !category || savingCategory) return; setSavingCategory(true); try { await postForm("/api/requirement/category", { reqId: req.id, category }); refresh() } finally { setSavingCategory(false) } }
  const newSession = async () => { if (!req) return; const res = await postForm<any>("/api/requirement/new-session", { reqId: req.id }); setCommand(res.command || JSON.stringify(res)) }
  return <PageChrome icon={<GitBranch size={15} />} eyebrow="Requirement" title={tool ? `${tool} — ${req?.title || id}` : (req?.title || id || "Requirement")} description={req?.description || "需求详情、状态流转、关联 session 与工具入口。"} actions={<a href="/projects"><ArrowLeft size={15} />返回需求列表</a>}>{error ? <ErrorCard error={error} /> : loading ? <LoadingCard /> : !req ? <EmptyCard>需求不存在：{id}</EmptyCard> : <div className="react-detail-grid"><section className="react-panel"><PanelHead kicker="Overview" title="需求信息" chip={statusPill(req.status)} /><div className="react-meta-grid"><span>Req ID <code>{req.id}</code></span><span>项目 {projectsOf(req)}</span><span>创建 {formatDate(req.createdAt)}</span><span>更新 {relAge(req.updatedAt)}</span><span>目录 {req.reqDir || "-"}</span></div><p className="react-detail-desc">{req.description || "暂无描述"}</p><div className="react-tool-links"><a href={`/requirement/review?id=${req.id}`}>代码差异</a><a href={`/requirement/release?id=${req.id}`}>发版注意</a></div></section><OnesPanel req={req} /><section className="react-panel"><PanelHead kicker="Status" title="状态切换" /><div className="react-inline-form"><select value={status} onChange={(e) => setStatus(e.target.value as ReqStatus)}><option value="">选择状态</option>{REQ_STATUSES.map((s) => <option key={s} value={s}>{s}</option>)}</select><input value={note} onChange={(e) => setNote(e.target.value)} placeholder="备注" /><button onClick={submitStatus} disabled={!status || savingStatus || status === savedStatus}>{savingStatus ? "保存中…" : "保存状态"}</button></div><div className="react-inline-form react-category-form"><label>类别</label><select value={category} onChange={(e) => setCategory(e.target.value as ReqCategory)}><option value="">{req.category ?? "需求"}</option>{REQ_CATEGORIES.map((c) => <option key={c} value={c}>{c}</option>)}</select><button onClick={submitCategory} disabled={!category || savingCategory}>{savingCategory ? "保存中…" : "保存类别"}</button></div></section><section className="react-panel"><PanelHead kicker="Sessions" title="关联 Session" chip={`${req.sessionIds?.length || 0}`} />{req.sessionIds?.length ? <SessionChipList sessionIds={req.sessionIds} /> : <p className="react-muted">暂无关联 session。</p>}<div className="react-actions"><button onClick={newSession}>另开新 session</button></div>{command ? <code className="react-command">{command}</code> : null}</section><RecommendationPanel req={req} onBound={refresh} /><EffortEstimatePanel req={req} onUpdated={refresh} /><AttachmentPanel req={req} /><AutoDrivePanel req={req} />{tool ? <ToolPanel tool={tool} req={req} sessionId={sessionId} /> : null}</div>}</PageChrome>
}

function ToolPanel({ tool, req, sessionId }: { tool: string; req: Requirement; sessionId: string }) {
  const [result, setResult] = useState<string>("")
  const run = async () => {
    const endpoint = tool === "review" ? "/api/requirement/code-review/scan" : tool === "auto-extract" ? "/api/requirement/auto-extract" : ""
    if (!endpoint) { setResult("该工具页已迁移为 React 展示壳；具体内容请从需求详情入口继续。") ; return }
    const res = await postForm<any>(endpoint, { reqId: req.id, sessionId })
    setResult(JSON.stringify(res, null, 2))
  }
  return <section className="react-panel"><PanelHead kicker="Tool" title={`${tool} 工具`} /><p className="react-muted">React 已接管该工具页面。涉及文件写入、扫描和长任务的动作仍通过后端 API 执行。</p><div className="react-actions"><button onClick={run}>执行/刷新</button></div>{result ? <pre className="react-json-preview">{result}</pre> : null}</section>
}

function SettingsPage() {
  const dashboard = useFetch<ConfigPayload>("/api/config")
  const piConfig = useFetch<PiConfigSummary>("/api/pi-config")
  const [draft, setDraft] = useState<ConfigPayload>({})
  const [piDraft, setPiDraft] = useState<PiSettingsDraft>({ defaultProvider: "", defaultModel: "", defaultThinkingLevel: "", theme: "", enabledModelsText: "" })
  const [selectedFile, setSelectedFile] = useState<PiConfigFileKey>("settings")
  const [fileSnapshot, setFileSnapshot] = useState<PiConfigFileSnapshot | null>(null)
  const [fileContent, setFileContent] = useState("")
  const [fileError, setFileError] = useState<string | null>(null)
  const [savingFile, setSavingFile] = useState(false)
  const [savedHint, setSavedHint] = useState<string | null>(null)
  const [scanRootsText, setScanRootsText] = useState("")

  useEffect(() => { if (dashboard.data) setDraft(dashboard.data) }, [dashboard.data])
  useEffect(() => { if (dashboard.data) setScanRootsText((dashboard.data.requirementScanRoots || []).join("\n")) }, [dashboard.data])
  useEffect(() => {
    const s = piConfig.data?.settings
    if (!s) return
    setPiDraft({
      defaultProvider: s.defaultProvider,
      defaultModel: s.defaultModel,
      defaultThinkingLevel: s.defaultThinkingLevel || "off",
      theme: s.theme,
      enabledModelsText: s.enabledModels.join("\n"),
    })
  }, [piConfig.data])
  useEffect(() => {
    let cancelled = false
    setFileError(null)
    fetchJson<PiConfigFileSnapshot>(`/api/pi-config/file?file=${encodeURIComponent(selectedFile)}`)
      .then((snapshot) => { if (!cancelled) { setFileSnapshot(snapshot); setFileContent(snapshot.content) } })
      .catch((err: Error) => { if (!cancelled) setFileError(err.message) })
    return () => { cancelled = true }
  }, [selectedFile, savedHint])

  const saveDashboard = async () => { await postJson("/api/config", draft); dashboard.refresh(); setSavedHint("Dashboard 配置已保存") }
  const saveScanRoots = async () => { await postJson("/api/config", { requirementScanRoots: scanRootsText.split(/[\n,]/).map((v) => v.trim()).filter(Boolean) }); dashboard.refresh(); setSavedHint("扫描目录已保存") }
  const saveAiModel = async () => {
    await postJson("/api/config", {
      codeReviewBaseUrl: draft.codeReviewBaseUrl || "",
      codeReviewModel: draft.codeReviewModel || "",
      branchScopeModel: draft.branchScopeModel || "",
      effortEstimateModel: draft.effortEstimateModel || "",
      effortEstimateBaseHours: Number(draft.effortEstimateBaseHours) || 4,
      codeReviewApiKey: aiApiKey,
    })
    dashboard.refresh()
    setAiApiKey("")
    setSavedHint("AI 模型配置已保存")
  }
  const [aiApiKey, setAiApiKey] = useState("")
  const savePiSettings = async () => {
    await postJson("/api/pi-config/settings", {
      defaultProvider: piDraft.defaultProvider,
      defaultModel: piDraft.defaultModel,
      defaultThinkingLevel: piDraft.defaultThinkingLevel,
      theme: piDraft.theme,
      enabledModels: piDraft.enabledModelsText.split(/[\n,]/).map((v) => v.trim()).filter(Boolean),
    })
    piConfig.refresh()
    setSavedHint("Pi 默认模型配置已保存")
  }
  const savePiFile = async () => {
    setSavingFile(true)
    setFileError(null)
    try {
      const next = await postJson<PiConfigFileSnapshot>("/api/pi-config/file", { file: selectedFile, content: fileContent })
      setFileSnapshot(next)
      setFileContent(next.content)
      piConfig.refresh()
      setSavedHint(`${next.label} 已保存`)
    } catch (err) {
      setFileError((err as Error).message)
    } finally {
      setSavingFile(false)
    }
  }

  const providers = piConfig.data?.providers || []
  const selectedProvider = providers.find((p) => p.id === piDraft.defaultProvider) || providers[0]
  const modelOptions = selectedProvider?.models || []
  const fileMeta = piConfig.data?.files || []

  return <PageChrome icon={<Settings size={15} />} eyebrow="Settings" title="Settings" description="配置同步、智能提取、价值发现、Pi 默认模型和 Pi 配置文件。">{dashboard.error ? <ErrorCard error={dashboard.error} /> : dashboard.loading ? <LoadingCard /> : <div className="react-settings-layout"><section className="react-panel"><PanelHead kicker="Dashboard" title="运行配置" chip={savedHint || undefined} /><div className="react-settings-grid">{["autoExtract", "autoExtractSchedule", "fullSyncSchedule", "autoValuation"].map((key) => <label key={key} className="react-switch"><input type="checkbox" checked={Boolean(draft[key])} onChange={(e) => setDraft({ ...draft, [key]: e.target.checked })} /><span>{key}</span></label>)}<label>提取模型<input value={draft.extractModel || ""} onChange={(e) => setDraft({ ...draft, extractModel: e.target.value })} /></label><label>最小消息增量<input type="number" value={draft.minChangeMessages || 0} onChange={(e) => setDraft({ ...draft, minChangeMessages: Number(e.target.value) })} /></label><label>价值评分阈值<input type="number" value={draft.valuationThreshold || 0} onChange={(e) => setDraft({ ...draft, valuationThreshold: Number(e.target.value) })} /></label></div><div className="react-actions"><button onClick={saveDashboard}>保存 Dashboard 配置</button></div></section><section className="react-panel"><PanelHead kicker="Scan Roots" title="需求扫描目录" chip={savedHint || undefined} /><p className="react-muted">每个扫描目录会自动查找其下的 <code>.agents/req/</code> 或 <code>req/</code> 文件夹中的需求，不再从 <code>~/.agents/req/</code> 读取。一行一个目录路径（绝对路径、<code>~/</code> 前缀或 <code>~/Developer</code> 下的相对路径）。</p><label className="react-editor-label">扫描目录<textarea value={scanRootsText} onChange={(e) => setScanRootsText(e.target.value)} rows={4} placeholder={"/home/hevin/Developer/company/WMS\n~/Developer/tools/agent-panel"} /></label><div className="react-actions"><button onClick={saveScanRoots}>保存扫描目录</button></div></section><section className="react-panel"><PanelHead kicker="AI Model" title="AI 模型接入" chip={savedHint || undefined} /><p className="react-muted">配置 OpenAI 兼容接口，用于代码差异页面的「提取 branches.json」和「AI 审查代码」。API Key 仅保存在本地，不回显。</p><div className="react-settings-grid"><label>Base URL<input value={draft.codeReviewBaseUrl || ""} onChange={(e) => setDraft({ ...draft, codeReviewBaseUrl: e.target.value })} placeholder="https://api.deepseek.com/v1" /></label><label>Model (代码审查)<input value={draft.codeReviewModel || ""} onChange={(e) => setDraft({ ...draft, codeReviewModel: e.target.value })} placeholder="deepseek-chat" /></label><label>branches.json 提取模型<input value={draft.branchScopeModel || ""} onChange={(e) => setDraft({ ...draft, branchScopeModel: e.target.value })} placeholder="留空复用代码审查 Model" /></label><label>工时评估模型<input value={draft.effortEstimateModel || ""} onChange={(e) => setDraft({ ...draft, effortEstimateModel: e.target.value })} placeholder="留空复用代码审查 Model" /></label></div><label>基础工时（小时）<input type="number" value={draft.effortEstimateBaseHours || 4} onChange={(e) => setDraft({ ...draft, effortEstimateBaseHours: Number(e.target.value) })} placeholder="4" /></label><label>API Key {dashboard.data?.codeReviewApiKeySet ? <span className="react-saved-pill">✓ 已设置</span> : <span className="react-warn-pill">未设置</span>}<input type="password" value={aiApiKey} onChange={(e) => setAiApiKey(e.target.value)} placeholder={dashboard.data?.codeReviewApiKeySet ? "已设置，输入新值覆盖（留空保持不变）" : "粘贴 API Key"} /></label><div className="react-actions"><button onClick={saveAiModel}>保存 AI 模型配置</button></div></section><section className="react-panel"><PanelHead kicker="Pi Model" title="Pi 默认模型" chip={piConfig.loading ? "loading" : piConfig.error || `${providers.length} providers`} />{piConfig.error ? <ErrorCard error={piConfig.error} /> : <><div className="react-settings-grid"><label>Provider<select value={piDraft.defaultProvider} onChange={(e) => { const provider = providers.find((p) => p.id === e.target.value); setPiDraft({ ...piDraft, defaultProvider: e.target.value, defaultModel: provider?.models[0]?.modelId || piDraft.defaultModel }) }}><option value="">手动 / 内置 provider</option>{providers.map((p) => <option key={p.id} value={p.id}>{p.id} ({p.modelCount})</option>)}</select></label><label>Model<select value={piDraft.defaultModel} onChange={(e) => setPiDraft({ ...piDraft, defaultModel: e.target.value })}><option value={piDraft.defaultModel}>{piDraft.defaultModel || "选择模型"}</option>{modelOptions.map((m) => <option key={`${m.providerId}/${m.modelId}`} value={m.modelId}>{m.modelId}</option>)}</select></label><label>Thinking<select value={piDraft.defaultThinkingLevel} onChange={(e) => setPiDraft({ ...piDraft, defaultThinkingLevel: e.target.value })}>{(piConfig.data?.thinkingLevels || ["off", "minimal", "low", "medium", "high", "xhigh", "max"]).map((level) => <option key={level} value={level}>{level}</option>)}</select></label><label>Theme<input value={piDraft.theme} onChange={(e) => setPiDraft({ ...piDraft, theme: e.target.value })} placeholder="high-contrast" /></label></div><label className="react-editor-label">Enabled models<textarea value={piDraft.enabledModelsText} onChange={(e) => setPiDraft({ ...piDraft, enabledModelsText: e.target.value })} rows={4} placeholder="每行一个模型 pattern，例如 llm-provider-router/*" /></label><div className="react-actions"><button onClick={savePiSettings}>保存 Pi 默认模型</button><a href="/session?new=1">打开新 Pi session 测试</a></div><div className="react-model-list">{providers.map((p) => <div key={p.id} className="react-model-card"><strong>{p.id}</strong><span>{p.api || "api n/a"} · {p.modelCount} models · key {p.hasApiKey ? "set" : "missing"}</span><p>{p.models.slice(0, 4).map((m) => m.modelId).join(" / ")}{p.models.length > 4 ? " …" : ""}</p></div>)}</div></>}</section><section className="react-panel react-config-editor"><PanelHead kicker="Pi Files" title="配置文件编辑器" chip={fileSnapshot?.sensitive ? "secret placeholders" : fileSnapshot?.label} /><div className="react-file-tabs">{fileMeta.map((file) => <button key={file.file} className={file.file === selectedFile ? "active" : ""} onClick={() => setSelectedFile(file.file)}>{file.label}</button>)}</div>{fileSnapshot ? <p className="react-muted"><code>{fileSnapshot.path}</code> · {fileSnapshot.description}</p> : null}{fileError ? <ErrorCard error={fileError} /> : null}<textarea className="react-code-textarea" value={fileContent} onChange={(e) => setFileContent(e.target.value)} spellCheck={false} /><div className="react-actions"><button onClick={savePiFile} disabled={savingFile}>{savingFile ? "保存中…" : `保存 ${fileSnapshot?.label || selectedFile}`}</button><button onClick={() => setFileContent(fileSnapshot?.content || "")} disabled={!fileSnapshot}>恢复未保存修改</button></div>{fileSnapshot?.sensitive ? <p className="react-save-hint">敏感字段会显示为 <code>__AGENT_PANEL_SECRET__</code>；保存时后端会自动恢复原值，除非你手动改成新值。</p> : null}</section></div>}</PageChrome>
}

function EnvVarsPage() {
  const { data, error, loading, refresh } = useFetch<{ groups: EnvGroup[] }>("/api/env-vars")
  const [form, setForm] = useState({ name: "", value: "", note: "", file: "secrets" })
  const save = async () => { await postJson("/api/config/env", { action: "upsert", ...form }); setForm({ name: "", value: "", note: "", file: "secrets" }); refresh() }
  const del = async (name: string) => { if (!confirm(`删除环境变量 ${name}？`)) return; await postJson("/api/config/env", { action: "delete", name }); refresh() }
  return <PageChrome icon={<Settings size={15} />} eyebrow="Env Vars" title="环境变量" description="只显示安全预览；变量值通过后端写入，不在前端回显。">{error ? <ErrorCard error={error} /> : loading ? <LoadingCard /> : <><section className="react-panel"><PanelHead kicker="Add" title="新增 / 覆盖变量" /><div className="react-filter-grid"><label>变量名<input value={form.name} onChange={(e) => setForm({ ...form, name: e.target.value })} /></label><label>值<input type="password" value={form.value} onChange={(e) => setForm({ ...form, value: e.target.value })} /></label><label>说明<input value={form.note} onChange={(e) => setForm({ ...form, note: e.target.value })} /></label><label>文件<select value={form.file} onChange={(e) => setForm({ ...form, file: e.target.value })}><option value="secrets">secrets</option><option value="env">env</option></select></label></div><div className="react-actions"><button onClick={save} disabled={!form.name || !form.value}>保存</button></div></section>{(data?.groups || []).map((g) => <section key={g.file} className="react-panel"><PanelHead kicker="File" title={g.file} chip={`${g.variables.length} vars`} /><div className="react-card-list">{g.variables.map((v) => <div key={v.name} className="react-env-row"><div><code>{v.name}</code><p>{v.preview || v.source || "missing"} · {v.note || v.description || ""}</p></div><button onClick={() => del(v.name)}>删除</button></div>)}</div></section>)}</>}</PageChrome>
}

function SchedulersPage() {
  const config = useFetch<ConfigPayload>("/api/config")
  const valuation = useFetch<any>("/api/valuation/candidates?limit=20")
  const markers = useFetch<any>("/api/experience/markers")
  const poll = async () => { await postJson("/api/valuation/poll", {}); valuation.refresh() }
  return <PageChrome icon={<Bell size={15} />} eyebrow="Schedulers" title="Schedulers" description="自动同步、智能提取、经验总结和价值发现任务状态。"><section className="react-kpi-grid"><KpiCard icon={<Activity size={20} />} label="Auto Extract" value={config.data?.autoExtract ? "ON" : "OFF"} sub="extract scheduler" tone="active" /><KpiCard icon={<Sparkles size={20} />} label="Valuation" value={config.data?.autoValuation ? "ON" : "OFF"} sub="session scoring" tone="done" /><KpiCard icon={<Bell size={20} />} label="Markers" value={markers.data?.markers?.length || 0} sub="experience markers" tone="avg" /><KpiCard icon={<Search size={20} />} label="Candidates" value={valuation.data?.candidates?.length || 0} sub="recent value hits" tone="total" /></section><section className="react-panel"><PanelHead kicker="Valuation" title="高价值 Session 候选" chip={<button onClick={poll}>手动扫描</button>} /><div className="react-card-list">{(valuation.data?.candidates || []).map((c: any) => <div className="react-env-row" key={c.sessionId}><div><a href={`/session?id=${c.sessionId}`}>{c.sessionId}</a><p>score {c.score} · {(c.reasons || []).join(" / ")}</p></div></div>)}</div></section></PageChrome>
}

const gitAiStatusMeta: Record<GitAiCompanyStatus, { label: string; color: string; soft: string }> = {
  pending: { label: "待公司接口确认", color: "#f59e0b", soft: "rgba(245, 158, 11, .14)" },
  confirmed_ai: { label: "公司确认已标", color: "#22c55e", soft: "rgba(34, 197, 94, .14)" },
  missing_ai: { label: "公司确认缺失", color: "#ef4444", soft: "rgba(239, 68, 68, .14)" },
  not_found: { label: "公司接口未找到", color: "#94a3b8", soft: "rgba(148, 163, 184, .14)" },
  check_failed: { label: "检查失败", color: "#f97316", soft: "rgba(249, 115, 22, .14)" },
}

function gitAiStatusPill(status: GitAiCompanyStatus) {
  const meta = gitAiStatusMeta[status]
  return <span className="react-status-pill" style={{ color: meta.color, background: meta.soft, borderColor: `${meta.color}66` }}>{meta.label}</span>
}

const gitAiHealthMeta: Record<HealthTone, { label: string; color: string; soft: string }> = {
  ok: { label: "OK", color: "#22c55e", soft: "rgba(34, 197, 94, .14)" },
  warn: { label: "WARN", color: "#f59e0b", soft: "rgba(245, 158, 11, .14)" },
  error: { label: "ERROR", color: "#ef4444", soft: "rgba(239, 68, 68, .14)" },
  unknown: { label: "UNKNOWN", color: "#94a3b8", soft: "rgba(148, 163, 184, .14)" },
}

function tonePill(tone: HealthTone, label?: string) {
  const meta = gitAiHealthMeta[tone]
  return <span className="react-status-pill" style={{ color: meta.color, background: meta.soft, borderColor: `${meta.color}66` }}>{label || meta.label}</span>
}

function healthTone(ok: boolean, warn = false): HealthTone {
  return ok ? (warn ? "warn" : "ok") : "error"
}

function GitAiHealthPanel({ health, loading, error, refresh }: { health: GitAiHealthPayload | null; loading: boolean; error: string | null; refresh: () => void }) {
  const cliTone = health ? healthTone(Boolean(health.cli.installed && health.cli.daemonOk && health.cli.trace2SocketExists && health.cli.postCommitHook.recordsToAgentPanel && health.cli.prePushHook.recordsToAgentPanel)) : "unknown"
  const piTone = health?.piExtension.status || "unknown"
  return <section className="react-panel"><PanelHead kicker="Health" title="git-ai / Pi Extension 状态" chip={<button onClick={refresh}>刷新状态</button>} />{error ? <ErrorCard error={error} /> : loading ? <LoadingCard label="正在检查 git-ai 状态…" /> : !health ? <EmptyCard>暂无健康状态。</EmptyCard> : <><div className="react-kpi-grid"><KpiCard icon={<Gauge size={20} />} label="git-ai CLI" value={health.cli.version || (health.cli.installed ? "installed" : "missing")} sub={health.cli.binaryPath || "binary n/a"} tone={cliTone === "ok" ? "done" : cliTone === "warn" ? "active" : "total"} /><KpiCard icon={<Activity size={20} />} label="Daemon" value={health.cli.daemonOk ? "running" : "check"} sub={health.cli.daemonMessage || "daemon status"} tone={health.cli.daemonOk ? "done" : "total"} /><KpiCard icon={<GitBranch size={20} />} label="Hooks" value={health.cli.postCommitHook.mode + " / " + health.cli.prePushHook.mode} sub="post-commit / pre-push" tone={health.cli.postCommitHook.recordsToAgentPanel && health.cli.prePushHook.recordsToAgentPanel ? "done" : "total"} /><KpiCard icon={<Sparkles size={20} />} label="Pi Extension" value={gitAiHealthMeta[piTone].label} sub={health.piExtension.globalExists ? "auto-discovery path" : "missing"} tone={piTone === "ok" ? "done" : piTone === "warn" ? "active" : "total"} /></div><div className="react-meta-grid"><span>Trace2 {tonePill(health.cli.trace2SocketExists ? "ok" : "error", health.cli.trace2SocketExists ? "socket ok" : "socket missing")}</span><span><code>{health.cli.trace2Socket || health.cli.trace2Target || "trace2 n/a"}</code></span><span>post-commit {tonePill(health.cli.postCommitHook.recordsToAgentPanel ? "ok" : "error", health.cli.postCommitHook.mode)}</span><span><code>{health.cli.postCommitHook.path || "hook n/a"}</code></span><span>pre-push {tonePill(health.cli.prePushHook.recordsToAgentPanel ? "ok" : "error", health.cli.prePushHook.mode)}</span><span><code>{health.cli.prePushHook.path || "hook n/a"}</code></span><span>Pi source match {tonePill(health.piExtension.sourceMatchesGlobal ? "ok" : "warn", health.piExtension.sourceMatchesGlobal ? "synced" : "differs")}</span><span><code>{health.piExtension.globalPath}</code></span><span>Pi tools</span><span>{health.piExtension.tracksTools.length ? health.piExtension.tracksTools.join(" / ") : "none detected"}</span><span>Pi message</span><span>{health.piExtension.message}</span><span>Suspect store</span><span><code>{health.storePath}</code></span></div></>}</section>
}

function GitAiPage() {
  const feed = useFetch<GitAiSuspectsPayload>("/api/git-ai/suspects")
  const health = useFetch<GitAiHealthPayload>("/api/git-ai/health")
  const [status, setStatus] = useState<GitAiCompanyStatus | "all">("all")
  const [refreshing, setRefreshing] = useState(false)
  const records = feed.data?.records || []
  const filtered = status === "all" ? records : records.filter((r) => r.companyStatus === status)
  const stats = feed.data?.stats
  const refreshCompany = async () => {
    setRefreshing(true)
    try { await postJson("/api/git-ai/suspects/refresh", { limit: 200 }); feed.refresh() }
    finally { setRefreshing(false) }
  }
  return <PageChrome icon={<GitBranch size={15} />} eyebrow="Git AI" title="AI 标记疑似缺失" description="hook 只记录疑似缺失，不再阻断 commit/push；最终是否缺标以公司 ai-stats/check-commit 接口为准。" actions={<button onClick={refreshCompany} disabled={refreshing}>{refreshing ? "公司接口检查中…" : "刷新公司接口状态"}</button>}>
    <GitAiHealthPanel health={health.data} loading={health.loading} error={health.error} refresh={health.refresh} />
    <section className="react-kpi-grid"><KpiCard icon={<AlertTriangle size={20} />} label="疑似记录" value={stats?.total ?? "—"} sub="hook captured" tone="avg" /><KpiCard icon={<Clock3 size={20} />} label="待确认" value={stats?.pending ?? "—"} sub="pending company check" tone="active" /><KpiCard icon={<AlertTriangle size={20} />} label="确认缺失" value={stats?.missingAi ?? "—"} sub="company says missing" tone="total" /><KpiCard icon={<CheckCircle2 size={20} />} label="确认已标" value={stats?.confirmedAi ?? "—"} sub="company says tagged" tone="done" /></section>
    <section className="react-panel"><PanelHead kicker="Source of truth" title="公司接口判定" chip={feed.data ? `更新 ${relAge(feed.data.generatedAt)}` : undefined} /><p className="react-muted">本页不会把本地 git notes 当最终结果。git-ai 可能延迟生成标记；点击刷新时会用 <code>project_name</code> + <code>commit_sha</code> + 可选 <code>gitlab_project_id</code> 调公司接口复核。</p><div className="react-tab-row"><button className={status === "all" ? "active" : ""} onClick={() => setStatus("all")}>全部</button>{(Object.keys(gitAiStatusMeta) as GitAiCompanyStatus[]).map((s) => <button key={s} className={status === s ? "active" : ""} onClick={() => setStatus(s)}>{gitAiStatusMeta[s].label}</button>)}</div></section>
    {feed.error ? <ErrorCard error={feed.error} /> : feed.loading ? <LoadingCard /> : <div className="react-card-list">{filtered.length === 0 ? <EmptyCard>暂无符合条件的疑似缺标 commit。</EmptyCard> : filtered.map((r, i) => <motion.article key={r.id} className="react-list-card react-session-card" initial={{ opacity: 0, y: 12 }} animate={{ opacity: 1, y: 0 }} transition={{ delay: Math.min(i, 16) * 0.025 }} whileHover={{ y: -3 }}><div><span className="react-card-id">{r.projectName} · {r.shortSha}</span><h3>{r.commitWebUrl ? <a href={r.commitWebUrl} target="_blank" rel="noopener noreferrer">{r.commitTitle || r.subject || r.commitSha}</a> : (r.commitTitle || r.subject || r.commitSha)}</h3><p>{r.repoPath || r.remoteUrl || "repo path n/a"}</p><div className="react-card-meta"><span>{gitAiStatusPill(r.companyStatus)}</span><span>hook: {r.eventSources.join(" / ") || "-"}</span><span>本地 note: {r.localNoteState}</span><span>记录 {relAge(r.lastSeenAt)}</span><span>公司检查 {r.companyCheckedAt ? relAge(r.companyCheckedAt) : "未检查"}</span>{r.aiRate !== null ? <span>AI rate {r.aiRate}%</span> : null}{r.gitlabProjectId ? <span>GitLab {r.gitlabProjectId}</span> : null}</div>{r.companyError ? <p className="react-error">{r.companyError}</p> : null}</div><div className="react-card-side"><span className="react-effort-badge">{r.aiLines ?? 0} AI / {r.humanLines ?? 0} human</span><span className="react-muted">{r.originBranch || r.branch || "branch n/a"}</span><code>{r.commitSha.slice(0, 12)}</code></div></motion.article>)}</div>}
  </PageChrome>
}

function NotFoundPage() { return <PageChrome icon={<AlertTriangle size={15} />} eyebrow="Not Found" title="页面不存在"><EmptyCard>当前路由没有匹配的 React 页面。</EmptyCard></PageChrome> }

export function App({ apiPath }: AppProps) {
  const key = useLocationKey()
  const path = window.location.pathname
  return <AnimatePresence mode="wait"><motion.div key={key} initial={{ opacity: 0, y: 10 }} animate={{ opacity: 1, y: 0 }} exit={{ opacity: 0, y: -6 }} transition={{ duration: 0.22 }}>
    {path === "/" || path === "/dashboard" ? <DashboardPage apiPath={apiPath} />
      : path === "/projects" || path === "/requirements" ? <ProjectsPage />
      : path === "/sessions" ? <SessionsPage />
      : path === "/reports" ? <ReportsPage />
      : path === "/report" ? <ReportDetailPage />
      : path === "/session" ? <SessionPage />
      : path === "/requirement" ? <RequirementPage />
      : path === "/requirement/review" ? <RequirementPage tool="review" />
      : path === "/requirement/release" ? <RequirementPage tool="release" />
      : path === "/requirement/extract" ? <RequirementPage tool="extract" />
      : path === "/requirement/recall" ? <RequirementPage tool="recall" />
      : path === "/requirement/auto-extract" ? <RequirementPage tool="auto-extract" />
      : path === "/settings" ? <SettingsPage />
      : path === "/env-vars" ? <EnvVarsPage />
      : path === "/schedulers" ? <SchedulersPage />
      : path === "/git-ai" ? <GitAiPage />
      : <NotFoundPage />}
  </motion.div></AnimatePresence>
}
