/**
 * Role: 发布计划页 — 按 meta.md plan-release 登记日期分组查看需求发布安排。
 * Public surface: ReleasePlanPage used by web/src/App.tsx route /release-plan.
 * Constraints: read-only view over /api/requirements; release-day quick check page.
 * Read-this-with: web/src/pages/projects.tsx for card/filter patterns and src/requirement_index.rs for the DTO source.
 */
import { motion } from "framer-motion"
import { CalendarClock, ChevronDown, ListChecks, Rocket } from "lucide-react"
import { useMemo, useState } from "react"
import type { Requirement } from "../types"
import { useFetch } from "../lib/api"
import { relAge } from "../lib/format"

import { onesBadge, projectsOf, statusPill } from "../features/requirements/badges"
import { EmptyCard, ErrorCard, KpiCard, LoadingCard, PageChrome, PanelHead } from "../components/ui"
import { readProjectFilter } from "../lib/preferences"

const DAY_MS = 24 * 60 * 60 * 1000
const WEEKDAYS = ["周日", "周一", "周二", "周三", "周四", "周五", "周六"]

function ymd(d: Date): string {
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`
}

function weekdayOf(date: string): string {
  return WEEKDAYS[new Date(`${date}T00:00:00`).getDay()] || ""
}

/** 相对选中日期的天数差：负数=已过期，0=当天，正数=未来。planRelease 非法时返回 null。 */
function dayDiff(planRelease: string, selected: string): number | null {
  const plan = new Date(`${planRelease}T00:00:00`)
  if (Number.isNaN(plan.getTime())) return null
  return Math.round((plan.getTime() - new Date(`${selected}T00:00:00`).getTime()) / DAY_MS)
}

function dayHint(diff: number): string {
  if (diff === 0) return "当天发布"
  if (diff > 0) return `还有 ${diff} 天`
  return `已过期 ${-diff} 天`
}

interface ReleaseGroup {
  key: string
  kicker: string
  title: string
  chip: string
  reqs: Requirement[]
  /** 折叠分组（如未登记）默认收起 */
  collapsible?: boolean
}

export function ReleasePlanPage({ globalProject }: { globalProject?: string }) {
  const { data, error, loading } = useFetch<{ requirements: Requirement[] }>("/api/requirements")
  const [selected, setSelected] = useState(() => ymd(new Date()))
  const [project, setProject] = useState("")
  const [unknownOpen, setUnknownOpen] = useState(false)
  const reqs = data?.requirements || []

  const effectiveProject = globalProject || project
  const filtered = useMemo(() => {
    if (!effectiveProject) return reqs
    return reqs.filter((r) => (r.projects?.length ? r.projects : [r.project]).includes(effectiveProject))
  }, [reqs, effectiveProject])

  const groups = useMemo<ReleaseGroup[]>(() => {
    const onSelected: Requirement[] = []
    const overdue: Requirement[] = []
    const upcoming: Requirement[] = []
    const unknown: Requirement[] = []
    for (const r of filtered) {
      const plan = (r.planRelease || "").trim()
      if (!plan || plan === "unknown") {
        unknown.push(r)
        continue
      }
      const diff = dayDiff(plan, selected)
      if (diff === null) {
        unknown.push(r)
      } else if (diff === 0) {
        onSelected.push(r)
      } else if (diff < 0) {
        overdue.push(r)
      } else {
        upcoming.push(r)
      }
    }
    overdue.sort((a, b) => (b.planRelease || "").localeCompare(a.planRelease || ""))
    upcoming.sort((a, b) => (a.planRelease || "").localeCompare(b.planRelease || ""))
    const groups: ReleaseGroup[] = [
      { key: "selected", kicker: `Planned ${selected} ${weekdayOf(selected)}`, title: selected === ymd(new Date()) ? "今天发布" : "选中日期发布", chip: `${onSelected.length} 条`, reqs: onSelected },
      { key: "overdue", kicker: "Overdue", title: "已过期未发", chip: `${overdue.length} 条`, reqs: overdue },
      { key: "upcoming", kicker: "Upcoming", title: "未来排期", chip: `${upcoming.length} 条`, reqs: upcoming },
      { key: "unknown", kicker: "Unplanned", title: "未登记发布日期", chip: `${unknown.length} 条`, reqs: unknown, collapsible: true },
    ]
    return groups
  }, [filtered, selected])

  const selectedCount = groups[0]?.reqs.length ?? 0
  const overdueCount = groups[1]?.reqs.length ?? 0
  const upcomingCount = groups[2]?.reqs.length ?? 0
  const unknownCount = groups[3]?.reqs.length ?? 0
  const projects = useMemo(() => [...new Set(reqs.flatMap((r) => r.projects?.length ? r.projects : [r.project]).filter(Boolean))].sort(), [reqs])

  return <PageChrome icon={<Rocket size={15} />} eyebrow="Release Plan" title="发布计划" description="按需求 meta.md 登记的 plan-release 日期分组，发版当天快速确认今天要发哪些需求；数据在需求编辑页维护。">
    <section className="react-kpi-grid">
      <KpiCard icon={<Rocket size={20} />} label="当天发布" value={selectedCount} sub={`${selected} ${weekdayOf(selected)}`} tone="done" />
      <KpiCard icon={<CalendarClock size={20} />} label="已过期未发" value={overdueCount} sub={`${selected} 之前登记`} tone="avg" />
      <KpiCard icon={<ListChecks size={20} />} label="未来排期" value={upcomingCount} sub={`${selected} 之后登记`} tone="active" />
      <KpiCard icon={<CalendarClock size={20} />} label="未登记" value={unknownCount} sub="plan-release 为 unknown" tone="total" />
    </section>
    <section className="react-panel react-filter-panel">
      <div className="react-filter-grid">
        <label>查看日期<input type="date" value={selected} onChange={(e) => setSelected(e.target.value || ymd(new Date()))} /></label>
        {globalProject ? null : <label>项目<select value={project} onChange={(e) => setProject(e.target.value)}><option value="">全部项目</option>{projects.map((p) => <option key={p} value={p}>{p}</option>)}</select></label>}
      </div>
      <div className="react-actions"><span className="react-muted">发布日期在需求编辑页的 planRelease 字段维护，unknown 表示尚未排期。</span></div>
    </section>
    {error ? <ErrorCard error={error} /> : loading ? <LoadingCard /> : groups.map((group) => (
      <GroupSection key={group.key} group={group} selected={selected} collapsed={Boolean(group.collapsible) && !unknownOpen} onToggle={() => setUnknownOpen((v) => !v)} />
    ))}
  </PageChrome>
}

function GroupSection({ group, selected, collapsed, onToggle }: { group: ReleaseGroup; selected: string; collapsed: boolean; onToggle: () => void }) {
  if (group.collapsible && collapsed) {
    return <section className="react-panel">
      <button type="button" className="react-filter-section-head react-collapse-head" onClick={onToggle} aria-expanded={false}>
        <span>{group.title}</span>
        <em className="react-collapse-summary">{group.chip} · 点击展开</em>
        <ChevronDown size={14} className="react-collapse-chevron" />
      </button>
    </section>
  }
  return <section className="react-panel">
    <PanelHead kicker={group.kicker} title={group.title} chip={group.chip} />
    {group.reqs.length === 0 ? <EmptyCard>{group.key === "selected" ? `${selected} 没有登记发布需求。` : "暂无需求。"}</EmptyCard> : <div className="react-card-list">{group.reqs.map((req, index) => <ReleaseCard key={req.id} req={req} selected={selected} index={index} />)}</div>}
  </section>
}

function ReleaseCard({ req, selected, index }: { req: Requirement; selected: string; index: number }) {
  const plan = (req.planRelease || "").trim()
  const diff = plan ? dayDiff(plan, selected) : null
  return <motion.article className="react-list-card react-req-card" initial={{ opacity: 0, y: 12 }} animate={{ opacity: 1, y: 0 }} transition={{ delay: Math.min(index, 16) * 0.025 }} whileHover={{ y: -3 }}>
    <div>
      <span className="react-card-id">{req.id}{diff !== null ? ` · ${dayHint(diff)}` : ""}</span>
      <h3><a href={`/requirement?id=${encodeURIComponent(req.id)}`}>{req.title}</a></h3>
      <p>{req.description || "暂无描述"}</p>
      <div className="react-card-meta">
        {plan && plan !== "unknown" ? <span>{plan} {weekdayOf(plan)}</span> : null}
        <span>{projectsOf(req)}</span>
        <span>{req.sessionIds?.length || 0} session(s)</span>
        <span>更新 {relAge(req.updatedAt)}</span>
      </div>
    </div>
    <div className="react-card-side">
      {req.releaseManifestPath ? <a className="react-effort-badge" href={`/requirement?id=${encodeURIComponent(req.id)}#release-manifest`}>上线清单</a> : null}
      {req.category === "线上问题" ? <span className="react-status-pill" style={{ color: "#f87171", background: "rgba(239, 68, 68, 0.14)", borderColor: "rgba(239, 68, 68, 0.4)" }}>线上问题</span> : null}
      {statusPill(req.status)}
      {onesBadge(req.ones)}
    </div>
  </motion.article>
}
