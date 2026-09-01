import { motion } from "framer-motion"
import { Activity, CalendarRange, CheckCircle2, Clock3, Gauge, LayoutDashboard, RefreshCw, Rocket, Sparkles } from "lucide-react"
import { useMemo } from "react"
import type { DashboardStatsPayload, ReleaseDayCount, RequirementDuration, StatusCount } from "../types"
import { useFetch } from "../lib/api"
import { formatDate, formatDuration } from "../lib/format"
import { statusMeta } from "../lib/requirements"
import { cardVariants, EmptyCard, ErrorCard, KpiCard, LoadingCard, PanelHead } from "../components/ui"

const PIPELINE_STATUSES = ["需求澄清", "开发中", "自测中", "测试中"]
const WEEKDAYS = ["周日", "周一", "周二", "周三", "周四", "周五", "周六"]

function weekdayOf(date: string): string {
  return WEEKDAYS[new Date(`${date}T00:00:00`).getDay()] || ""
}

function PipelineBar({ item, index, color }: { item: StatusCount; index: number; color?: string }) {
  const meta = color
    ? { color, soft: `${color}22` }
    : statusMeta[item.status] || { color: "#94a3b8", soft: "rgba(148, 163, 184, 0.14)" }
  return <motion.div className="react-pipeline-row" initial={{ opacity: 0, x: -14 }} animate={{ opacity: 1, x: 0 }} transition={{ delay: 0.08 + index * 0.045 }}><div className="react-pipeline-label"><span className="react-pipeline-dot" style={{ background: meta.color, boxShadow: `0 0 14px ${meta.color}66` }} /><span>{item.status}</span></div><div className="react-pipeline-track"><motion.div className="react-pipeline-fill" style={{ background: `linear-gradient(90deg, ${meta.color}, ${meta.color}99)` }} initial={{ width: 0 }} animate={{ width: `${Math.max(1.5, item.percent)}%` }} transition={{ duration: 0.85, delay: 0.16 + index * 0.04 }} /></div><strong>{item.count}</strong><span>{item.percent}%</span></motion.div>
}

function ReleaseDayBar({ item, max, index }: { item: ReleaseDayCount; max: number; index: number }) {
  const meta = { color: "#22d3ee" }
  const todayMark = index === 0 ? " · 今天" : ""
  const pct = max > 0 ? (item.count / max) * 100 : 0
  const width = item.count > 0 ? Math.max(5, pct) : 0
  return <motion.div className="react-pipeline-row" initial={{ opacity: 0, x: -14 }} animate={{ opacity: 1, x: 0 }} transition={{ delay: 0.06 + index * 0.022 }}><div className="react-pipeline-label"><span className="react-pipeline-dot" style={{ background: meta.color, boxShadow: `0 0 14px ${meta.color}66` }} /><span>{item.date.slice(5)} {weekdayOf(item.date)}{todayMark}</span></div><div className="react-pipeline-track"><motion.div className="react-pipeline-fill" style={{ background: `linear-gradient(90deg, ${meta.color}, ${meta.color}99)` }} initial={{ width: 0 }} animate={{ width: `${width}%` }} transition={{ duration: 0.7, delay: 0.12 + index * 0.02 }} /></div><strong>{item.count}</strong><span>{item.count > 0 ? `${Math.round(pct)}%` : "-"}</span></motion.div>
}

function DurationRow({ item, max, index }: { item: RequirementDuration; max: number; index: number }) {
  const meta = statusMeta[item.req.status] || statusMeta["需求澄清"]
  const pct = Math.min(100, (item.durationMs / Math.max(max, 1)) * 100)
  return <motion.tr initial={{ opacity: 0, y: 8 }} animate={{ opacity: 1, y: 0 }} transition={{ delay: 0.16 + index * 0.035 }}><td><a href={`/requirement?id=${encodeURIComponent(item.req.id)}`}>{item.req.title}</a><div className="react-duration-id">{item.req.id}</div></td><td><span className="react-status-pill" style={{ color: meta.color, background: meta.soft, borderColor: `${meta.color}55` }}>{item.req.status}</span></td><td className="react-muted">{(item.req.projects?.length ? item.req.projects : [item.req.project]).filter(Boolean).join(" / ") || "-"}</td><td className="react-muted">{formatDate(item.req.createdAt)}</td><td className="react-duration-cell"><motion.div className="react-duration-fill" initial={{ width: 0 }} animate={{ width: `${pct}%` }} transition={{ duration: 0.8, delay: 0.2 + index * 0.025 }} /><span>{formatDuration(item.durationMs)}</span></td></motion.tr>
}

export function DashboardPage({ apiPath, project }: { apiPath: string; project: string }) {
  const url = project ? `${apiPath}${apiPath.includes("?") ? "&" : "?"}project=${encodeURIComponent(project)}` : apiPath
  const { data: payload, error, loading, refresh } = useFetch<DashboardStatsPayload>(url, [project])
  const stats = payload?.stats
  const completionRate = useMemo(() => (!stats || stats.total === 0) ? 0 : Math.round((stats.completedCount / stats.total) * 100), [stats])
  const activeRate = useMemo(() => (!stats || stats.total === 0) ? 0 : Math.round((stats.inProgressCount / stats.total) * 100), [stats])
  const pipelineRows = useMemo(() => {
    const rows: { item: StatusCount; color?: string }[] = (stats?.statusCounts || [])
      .filter((s) => PIPELINE_STATUSES.includes(s.status))
      .map((item) => ({ item }))
    if (stats?.nextRelease) {
      const nr = stats.nextRelease
      rows.push({
        item: {
          status: `最近发版日 ${nr.date.slice(5)}`,
          count: nr.count,
          percent: stats.total > 0 ? Math.round((nr.count / stats.total) * 1000) / 10 : 0,
        },
        color: "#f59e0b",
      })
    }
    return rows
  }, [stats])
  const forecastMax = useMemo(() => Math.max(0, ...(stats?.releaseSchedule || []).map((d) => d.count)), [stats])
  const forecastTotal = useMemo(() => (stats?.releaseSchedule || []).reduce((sum, d) => sum + d.count, 0), [stats])
  return <div className="react-dashboard"><motion.section className="react-hero" initial={{ opacity: 0, y: 16 }} animate={{ opacity: 1, y: 0 }} transition={{ duration: 0.48 }}><div className="react-hero-grid" aria-hidden="true" /><div className="react-hero-copy"><span className="react-eyebrow"><LayoutDashboard size={14} /> Agent Panel</span><h1>React + Rust 控制台</h1><p>前端由 React SPA 接管，后端已切到 Rust/Axum；OpenCode 旧报告链路和 PTY terminal 已移除。</p><div className="react-hero-actions"><button type="button" onClick={refresh} disabled={loading}><RefreshCw size={15} className={loading ? "react-spin" : ""} /> 刷新数据</button><a href="/projects"><Sparkles size={15} /> 进入需求看板</a></div></div><motion.div className="react-orb" animate={{ y: [0, -8, 0] }} transition={{ duration: 5, repeat: Infinity }}><strong>{stats?.total ?? "—"}</strong><span>REQS</span></motion.div></motion.section>{error ? <ErrorCard error={error} /> : !stats ? <LoadingCard label="正在加载 dashboard stats…" /> : <motion.div className="react-dashboard-body" initial="hidden" animate="show" transition={{ staggerChildren: 0.06 }}><motion.section className="react-kpi-grid" variants={{ hidden: {}, show: { transition: { staggerChildren: 0.07 } } }}><KpiCard icon={<Gauge size={20} />} label="需求总数" value={stats.total} sub="Tracked requirements" tone="total" /><KpiCard icon={<CheckCircle2 size={20} />} label="已完成" value={stats.completedCount} sub={`${completionRate}% complete`} tone="done" /><KpiCard icon={<Activity size={20} />} label="进行中" value={stats.inProgressCount} sub={`${activeRate}% active`} tone="active" /><KpiCard icon={<Clock3 size={20} />} label="平均交付时长" value={formatDuration(stats.avgDeliveryMs)} sub={`中位数 ${formatDuration(stats.medianDeliveryMs)} · 最长 ${formatDuration(stats.maxDeliveryMs)}`} tone="avg" /></motion.section><section className="react-content-grid"><motion.article className="react-panel" variants={cardVariants}><PanelHead kicker="Pipeline" title="需求状态分布" chip={`${pipelineRows.length} 项`} /><div className="react-pipeline-list">{pipelineRows.map((row, index) => <PipelineBar key={row.item.status} item={row.item} index={index} color={row.color} />)}</div></motion.article><motion.article className="react-panel react-delivery-panel" variants={cardVariants}><PanelHead kicker="Delivery" title="需求交付时长" chip={<><Clock3 size={13} /> Top durations</>} />{stats.durations.length === 0 ? <EmptyCard>暂无需求数据。</EmptyCard> : <DurationTable durations={stats.durations.slice(0, 18)} max={stats.maxDeliveryMs} />}</motion.article></section><motion.article className="react-panel react-forecast-panel" variants={cardVariants}><PanelHead kicker="Release Forecast" title="未来 14 天发版分布" chip={<><Rocket size={13} /> {forecastTotal} 条 · 横向按天</>} />{forecastMax === 0 ? <EmptyCard>未来 14 天没有登记发版需求；在需求详情页填写预计发版日期后这里会自动统计。</EmptyCard> : <div className="react-pipeline-list">{stats.releaseSchedule.map((item, index) => <ReleaseDayBar key={item.date} item={item} max={forecastMax} index={index} />)}</div>}</motion.article></motion.div>}</div>
}

function DurationTable({ durations, max }: { durations: RequirementDuration[]; max: number }) {
  return <div className="react-table-wrap"><table className="react-duration-table"><thead><tr><th>需求</th><th>状态</th><th>项目</th><th>创建时间</th><th>交付时长</th></tr></thead><tbody>{durations.map((item, index) => <DurationRow key={item.req.id} item={item} max={max} index={index} />)}</tbody></table></div>
}
