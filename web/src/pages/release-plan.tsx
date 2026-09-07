/**
 * Role: 发布计划页 — 统计「发布就绪」状态的需求，顶部平铺全部发布就绪需求列表（默认展开，可折叠），再按 meta.md plan-release 登记日期分组（当天/已过期/未来/未登记）。
 * Public surface: ReleasePlanPage used by web/src/App.tsx route /release-plan.
 * Constraints: read-only view over /api/requirements; only includes status=发布就绪 (人工已检查代码、随时可发布) requirements.
 * Read-this-with: web/src/pages/projects.tsx for card/filter patterns and src/requirement_index.rs for the DTO source.
 */
import { motion } from "framer-motion"
import { CalendarClock, CheckCircle2, ChevronDown, ListChecks, Rocket } from "lucide-react"
import { useMemo, useState } from "react"
import type { Requirement } from "../types"
import { useFetch } from "../lib/api"
import { relAge } from "../lib/format"

import { onesBadge, projectsOf, statusPill } from "../features/requirements/badges"
import { EmptyCard, ErrorCard, KpiCard, LoadingCard, PageChrome, PanelHead } from "../components/ui"
import { readProjectFilter } from "../lib/preferences"

const DAY_MS = 24 * 60 * 60 * 1000
const WEEKDAYS = ["周日", "周一", "周二", "周三", "周四", "周五", "周六"]
/** 正常发版序列长度：下拉最多可选距现第 8 发版日。 */
const RELEASE_DAY_COUNT = 8

function ymd(d: Date): string {
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`
}

function weekdayOf(date: string): string {
  return WEEKDAYS[new Date(`${date}T00:00:00`).getDay()] || ""
}

/** 从 from（含当天）起的下一个周三：正常发版日固定为周三，当天是周三则返回当天。 */
function nextWednesday(from: Date): Date {
  const d = new Date(from)
  d.setDate(d.getDate() + ((3 - d.getDay() + 7) % 7))
  return d
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
  /** 顶部「全部发布就绪」平铺列表折叠态：默认展开，一眼看到哪些需求可发。 */
  const [readyOpen, setReadyOpen] = useState(true)
  /** 发版日筛选：null=全部（按日期分组）；0..7=距现第 N 发版日（正常周三序列，含当天）。 */
  const [releaseDay, setReleaseDay] = useState<number | null>(null)
  const reqs = data?.requirements || []

  // 距现第 1..8 发版日（周三）日期序列；紧急发版不属于正常序列，不在筛选范围内。
  const releaseDays = useMemo(() => {
    const first = nextWednesday(new Date(`${ymd(new Date())}T00:00:00`))
    return Array.from({ length: RELEASE_DAY_COUNT }, (_, i) => {
      const d = new Date(first)
      d.setDate(d.getDate() + i * 7)
      return ymd(d)
    })
  }, [])

  const effectiveProject = globalProject || project
  const readyReqs = useMemo(() => reqs.filter((r) => r.status === "发布就绪"), [reqs])
  const filtered = useMemo(() => {
    let base = readyReqs
    if (effectiveProject) base = base.filter((r) => (r.projects?.length ? r.projects : [r.project]).includes(effectiveProject))
    if (releaseDay !== null) base = base.filter((r) => (r.planRelease || "").trim() === releaseDays[releaseDay])
    return base
  }, [readyReqs, effectiveProject, releaseDay, releaseDays])

  // 全部发布就绪平铺列表：有 plan-release 的按日期升序在前，未登记/非法日期按更新时间倒序在后。
  const allReady = useMemo(() => {
    const dated: Requirement[] = []
    const undated: Requirement[] = []
    for (const r of filtered) {
      const plan = (r.planRelease || "").trim()
      if (plan && plan !== "unknown" && dayDiff(plan, selected) !== null) dated.push(r)
      else undated.push(r)
    }
    dated.sort((a, b) => (a.planRelease || "").localeCompare(b.planRelease || ""))
    undated.sort((a, b) => (b.updatedAt || 0) - (a.updatedAt || 0))
    return [...dated, ...undated]
  }, [filtered, selected])

  const allGroup = useMemo<ReleaseGroup>(() => ({
    key: "all",
    kicker: "Ready · All",
    title: "全部发布就绪需求",
    chip: `${allReady.length} 条`,
    reqs: allReady,
    collapsible: true,
  }), [allReady])

  const groups = useMemo<ReleaseGroup[]>(() => {
    // 发版日筛选生效时：只展示该正常发版日的需求，不再按当天/过期/未来分组。
    if (releaseDay !== null) {
      const day = releaseDays[releaseDay]
      return [{ key: "release-day", kicker: `Release ${day} ${weekdayOf(day)}`, title: `距现第${releaseDay + 1}发版日`, chip: `${filtered.length} 条`, reqs: filtered }]
    }
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
  }, [filtered, selected, releaseDay, releaseDays])

  const selectedCount = groups[0]?.reqs.length ?? 0
  const overdueCount = groups[1]?.reqs.length ?? 0
  const upcomingCount = groups[2]?.reqs.length ?? 0
  const unknownCount = groups[3]?.reqs.length ?? 0
  const readyCount = filtered.length
  const projects = useMemo(() => [...new Set(reqs.flatMap((r) => r.projects?.length ? r.projects : [r.project]).filter(Boolean))].sort(), [reqs])

  return <PageChrome icon={<Rocket size={15} />} eyebrow="Release Plan" title="发布计划" description="统计所有「发布就绪」状态的需求（人工已检查代码、随时可发布），按 plan-release 登记日期分组，发版当天快速确认；agent 已完成但未经人工检查的需求不在此列。">
    {releaseDay !== null ? <section className="react-kpi-grid">
      <KpiCard icon={<CheckCircle2 size={20} />} label={`距现第${releaseDay + 1}发版日`} value={readyCount} sub={`${releaseDays[releaseDay]} ${weekdayOf(releaseDays[releaseDay])} 正式发版`} tone="done" />
    </section> : <section className="react-kpi-grid-5">
      <KpiCard icon={<CheckCircle2 size={20} />} label="发布就绪" value={readyCount} sub="人工已检查可发布" tone="done" />
      <KpiCard icon={<Rocket size={20} />} label="当天发布" value={selectedCount} sub={`${selected} ${weekdayOf(selected)}`} tone="active" />
      <KpiCard icon={<CalendarClock size={20} />} label="已过期未发" value={overdueCount} sub={`${selected} 之前登记`} tone="avg" />
      <KpiCard icon={<ListChecks size={20} />} label="未来排期" value={upcomingCount} sub={`${selected} 之后登记`} tone="total" />
      <KpiCard icon={<CalendarClock size={20} />} label="未登记" value={unknownCount} sub="plan-release 为 unknown" tone="total" />
    </section>}
    <section className="react-panel react-filter-panel">
      <div className="react-filter-grid">
        <label>发版日<select value={releaseDay ?? ""} onChange={(e) => setReleaseDay(e.target.value === "" ? null : Number(e.target.value))}><option value="">全部（按日期分组）</option>{releaseDays.map((d, i) => <option key={d} value={i}>距现第{i + 1}发版日 · {d.slice(5)} {weekdayOf(d)}</option>)}</select></label>
        <label>查看日期<input type="date" value={selected} onChange={(e) => setSelected(e.target.value || ymd(new Date()))} /></label>
        {globalProject ? null : <label>项目<select value={project} onChange={(e) => setProject(e.target.value)}><option value="">全部项目</option>{projects.map((p) => <option key={p} value={p}>{p}</option>)}</select></label>}
      </div>
      <div className="react-actions"><span className="react-muted">统计范围：状态为「发布就绪」的需求；发版日按正常周三发版序列（含当天）过滤，紧急发版不在序列内；发布日期在需求编辑页 planRelease 字段维护，unknown 表示尚未排期。</span></div>
    </section>
    {error ? <ErrorCard error={error} /> : loading ? <LoadingCard /> : <>
      {releaseDay === null ? <GroupSection group={allGroup} selected={selected} collapsed={!readyOpen} onToggle={() => setReadyOpen((v) => !v)} /> : null}
      {groups.map((group) => (
        <GroupSection key={group.key} group={group} selected={releaseDay !== null ? releaseDays[releaseDay] : selected} collapsed={Boolean(group.collapsible) && !unknownOpen} onToggle={() => setUnknownOpen((v) => !v)} />
      ))}
    </>}
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
    {group.reqs.length === 0 ? <EmptyCard>{group.key === "all" ? "当前没有任何发布就绪需求。" : group.key === "selected" ? `${selected} 没有登记发布的发布就绪需求。` : group.key === "release-day" ? `${selected} 没有登记发布的发布就绪需求。` : group.key === "unknown" ? "没有未登记发布日期的发布就绪需求。" : "暂无发布就绪需求。"}</EmptyCard> : <div className="react-card-list">{group.reqs.map((req, index) => <ReleaseCard key={req.id} req={req} selected={selected} index={index} />)}</div>}
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
