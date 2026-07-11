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

const REQ_STATUSES: ReqStatus[] = ["需求对齐", "方案设计", "开发中", "自测中", "测试中", "待上线", "已完成"]

const statusMeta: Record<string, { slug: string; color: string; soft: string }> = {
  需求对齐: { slug: "align", color: "#94a3b8", soft: "rgba(148, 163, 184, 0.14)" },
  方案设计: { slug: "design", color: "#f59e0b", soft: "rgba(245, 158, 11, 0.14)" },
  开发中: { slug: "dev", color: "#22d3ee", soft: "rgba(34, 211, 238, 0.14)" },
  自测中: { slug: "selftest", color: "#3b82f6", soft: "rgba(59, 130, 246, 0.14)" },
  测试中: { slug: "testing", color: "#a855f7", soft: "rgba(168, 85, 247, 0.14)" },
  待上线: { slug: "deploy", color: "#eab308", soft: "rgba(234, 179, 8, 0.14)" },
  已完成: { slug: "done", color: "#22c55e", soft: "rgba(34, 197, 94, 0.14)" },
}

interface Requirement {
  id: string
  title: string
  description?: string
  status: ReqStatus
  project: string
  projects?: string[]
  groupPath?: string[]
  createdAt: number
  updatedAt: number
  sessionIds: string[]
  reqDir?: string
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
  const params = new URLSearchParams(window.location.search)
  const [project, setProject] = useState(params.get("project") || "")
  const [subproject, setSubproject] = useState(params.get("subproject") || "")
  const [createdFrom, setCreatedFrom] = useState(params.get("createdFrom") || "")
  const [createdTo, setCreatedTo] = useState(params.get("createdTo") || "")
  const [statuses, setStatuses] = useState<string[]>(params.getAll("status"))
  const reqs = data?.requirements || []
  const projects = useMemo(() => [...new Set(reqs.flatMap((r) => r.projects?.length ? r.projects : [r.project]).filter(Boolean))].sort(), [reqs])
  const subprojects = useMemo(() => [...new Set(reqs.filter((r) => !project || (r.projects?.length ? r.projects : [r.project]).includes(project)).map((r) => r.groupPath?.[0] || "").filter(Boolean))].sort(), [reqs, project])
  const counts = useMemo(() => Object.fromEntries(REQ_STATUSES.map((s) => [s, reqs.filter((r) => r.status === s).length])), [reqs]) as Record<string, number>
  const filtered = useMemo(() => reqs.filter((r) => {
    if (statuses.length === 0 && r.status === "已完成") return false
    if (statuses.length && !statuses.includes(r.status)) return false
    if (project && !(r.projects?.length ? r.projects : [r.project]).includes(project)) return false
    if (subproject && r.groupPath?.[0] !== subproject) return false
    if (createdFrom && r.createdAt < new Date(`${createdFrom}T00:00:00`).getTime()) return false
    if (createdTo && r.createdAt > new Date(`${createdTo}T23:59:59`).getTime()) return false
    return true
  }).sort((a, b) => b.updatedAt - a.updatedAt), [reqs, statuses, project, subproject, createdFrom, createdTo])
  const apply = () => {
    const q = new URLSearchParams()
    if (createdFrom) q.set("createdFrom", createdFrom)
    if (createdTo) q.set("createdTo", createdTo)
    if (project) q.set("project", project)
    if (subproject) q.set("subproject", subproject)
    statuses.forEach((s) => q.append("status", s))
    window.location.href = `/projects${q.toString() ? `?${q}` : ""}`
  }
  return <PageChrome icon={<ListChecks size={15} />} eyebrow="Requirements" title="需求进度看板" description="按项目、状态和创建时间筛选需求，查看关联 session 和最近更新。">
    {error ? <ErrorCard error={error} /> : loading ? <LoadingCard /> : <>
      <section className="react-panel react-filter-panel"><PanelHead kicker="Filter" title="需求筛选" chip={`${filtered.length} tracked`} />
        <div className="react-filter-grid"><label>创建时间起<input type="date" value={createdFrom} onChange={(e) => setCreatedFrom(e.target.value)} /></label><label>创建时间止<input type="date" value={createdTo} onChange={(e) => setCreatedTo(e.target.value)} /></label><label>一级项目<select value={project} onChange={(e) => { setProject(e.target.value); setSubproject("") }}><option value="">全部项目</option>{projects.map((p) => <option key={p} value={p}>{p}</option>)}</select></label><label>二级项目<select value={subproject} onChange={(e) => setSubproject(e.target.value)} disabled={!project}><option value="">{project ? "全部二级项目" : "请先选择一级项目"}</option>{subprojects.map((p) => <option key={p} value={p}>{p}</option>)}</select></label></div>
        <div className="react-status-options">{REQ_STATUSES.map((s) => <label key={s} className={`react-status-option ${statuses.includes(s) ? "active" : ""}`}><input type="checkbox" checked={statuses.includes(s)} onChange={(e) => setStatuses((cur) => e.target.checked ? [...cur, s] : cur.filter((x) => x !== s))} /><span>{s}</span><strong>{counts[s] || 0}</strong></label>)}</div>
        <div className="react-actions"><button onClick={apply}>应用筛选</button><a href="/projects">重置</a></div>
      </section>
      <div className="react-card-list">{filtered.length === 0 ? <EmptyCard>没有符合当前筛选条件的需求。</EmptyCard> : filtered.map((req, index) => <RequirementCard key={req.id} req={req} index={index} />)}</div>
    </>}
  </PageChrome>
}

function RequirementCard({ req, index }: { req: Requirement; index: number }) {
  return <motion.article className="react-list-card" initial={{ opacity: 0, y: 12 }} animate={{ opacity: 1, y: 0 }} transition={{ delay: Math.min(index, 16) * 0.025 }} whileHover={{ y: -3 }}><div><span className="react-card-id">{req.id}</span><h3><a href={`/requirement?id=${encodeURIComponent(req.id)}`}>{req.title}</a></h3><p>{req.description || "暂无描述"}</p><div className="react-card-meta"><span>{projectsOf(req)}</span><span>{req.sessionIds?.length || 0} session(s)</span><span>更新 {relAge(req.updatedAt)}</span></div></div><div className="react-card-side">{statusPill(req.status)}<a href={`/requirement/review?id=${encodeURIComponent(req.id)}`}>Review</a><a href={`/requirement/release?id=${encodeURIComponent(req.id)}`}>Release</a></div></motion.article>
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

function RequirementPage({ tool }: { tool?: "review" | "release" | "extract" | "recall" | "auto-extract" }) {
  const id = new URLSearchParams(window.location.search).get("id") || new URLSearchParams(window.location.search).get("reqId") || ""
  const sessionId = new URLSearchParams(window.location.search).get("sessionId") || ""
  const { data, error, loading, refresh } = RequirementsData()
  const req = data?.requirements.find((r) => r.id === id)
  const [note, setNote] = useState("")
  const [status, setStatus] = useState<ReqStatus | "">("")
  const [command, setCommand] = useState("")
  const submitStatus = async () => { if (!req || !status) return; await postForm("/api/requirement/status", { reqId: req.id, status, note }); refresh() }
  const newSession = async () => { if (!req) return; const res = await postForm<any>("/api/requirement/new-session", { reqId: req.id }); setCommand(res.command || JSON.stringify(res)) }
  return <PageChrome icon={<GitBranch size={15} />} eyebrow="Requirement" title={tool ? `${tool} — ${req?.title || id}` : (req?.title || id || "Requirement")} description={req?.description || "需求详情、状态流转、关联 session 与工具入口。"}>{error ? <ErrorCard error={error} /> : loading ? <LoadingCard /> : !req ? <EmptyCard>需求不存在：{id}</EmptyCard> : <div className="react-detail-grid"><section className="react-panel"><PanelHead kicker="Overview" title="需求信息" chip={statusPill(req.status)} /><div className="react-meta-grid"><span>Req ID <code>{req.id}</code></span><span>项目 {projectsOf(req)}</span><span>创建 {formatDate(req.createdAt)}</span><span>更新 {relAge(req.updatedAt)}</span><span>目录 {req.reqDir || "-"}</span></div><p className="react-detail-desc">{req.description || "暂无描述"}</p><div className="react-tool-links"><a href={`/requirement?id=${req.id}`}>详情</a><a href={`/requirement/review?id=${req.id}`}>代码差异</a><a href={`/requirement/release?id=${req.id}`}>发版注意</a></div></section><section className="react-panel"><PanelHead kicker="Status" title="状态切换" /><div className="react-inline-form"><select value={status} onChange={(e) => setStatus(e.target.value as ReqStatus)}><option value="">选择状态</option>{REQ_STATUSES.map((s) => <option key={s} value={s}>{s}</option>)}</select><input value={note} onChange={(e) => setNote(e.target.value)} placeholder="备注" /><button onClick={submitStatus} disabled={!status}>保存状态</button></div></section><section className="react-panel"><PanelHead kicker="Sessions" title="关联 Session" chip={`${req.sessionIds?.length || 0}`} />{req.sessionIds?.length ? <div className="react-chip-list">{req.sessionIds.map((sid) => <a key={sid} href={`/session?id=${sid}`}>{sid}</a>)}</div> : <p className="react-muted">暂无关联 session。</p>}<div className="react-actions"><button onClick={newSession}>另开新 session</button></div>{command ? <code className="react-command">{command}</code> : null}</section>{tool ? <ToolPanel tool={tool} req={req} sessionId={sessionId} /> : null}</div>}</PageChrome>
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
  const { data, error, loading, refresh } = useFetch<ConfigPayload>("/api/config")
  const [draft, setDraft] = useState<ConfigPayload>({})
  useEffect(() => { if (data) setDraft(data) }, [data])
  const save = async () => { await postJson("/api/config", draft); refresh(); alert("已保存") }
  return <PageChrome icon={<Settings size={15} />} eyebrow="Settings" title="Settings" description="配置同步、智能提取、价值发现与模型参数。">{error ? <ErrorCard error={error} /> : loading ? <LoadingCard /> : <section className="react-panel"><PanelHead kicker="Config" title="运行配置" /><div className="react-settings-grid">{["autoExtract", "autoExtractSchedule", "fullSyncSchedule", "autoValuation"].map((key) => <label key={key} className="react-switch"><input type="checkbox" checked={Boolean(draft[key])} onChange={(e) => setDraft({ ...draft, [key]: e.target.checked })} /><span>{key}</span></label>)}<label>提取模型<input value={draft.extractModel || ""} onChange={(e) => setDraft({ ...draft, extractModel: e.target.value })} /></label><label>最小消息增量<input type="number" value={draft.minChangeMessages || 0} onChange={(e) => setDraft({ ...draft, minChangeMessages: Number(e.target.value) })} /></label><label>价值评分阈值<input type="number" value={draft.valuationThreshold || 0} onChange={(e) => setDraft({ ...draft, valuationThreshold: Number(e.target.value) })} /></label></div><div className="react-actions"><button onClick={save}>保存配置</button></div></section>}</PageChrome>
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
      : <NotFoundPage />}
  </motion.div></AnimatePresence>
}
