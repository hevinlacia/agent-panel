/**
 * Role: ONES 缺失统计页 — 统计所有未关联 ONES 任务（meta.md ones 字段为空/缺失）的需求，按状态分组展示。
 * Public surface: OnesMissingPage used by web/src/App.tsx route /ones-missing.
 * Constraints: read-only view over /api/requirements; unbound 判定与 features/requirements/badges.tsx onesBadge 保持一致（parseOnesRef 为空即未关联）。
 * Read-this-with: web/src/pages/release-plan.tsx for page patterns and web/src/lib/format.ts parseOnesRef.
 */
import { motion } from "framer-motion"
import { CheckCircle2, ChevronDown, Link2, Unlink2 } from "lucide-react"
import { useMemo, useState } from "react"
import type { Requirement } from "../types"
import { useFetch } from "../lib/api"
import { parseOnesRef, relAge } from "../lib/format"
import { ISSUE_STATUSES, REQ_FLOW_STATUSES } from "../lib/requirements"
import { projectsOf, statusPill } from "../features/requirements/badges"
import { EmptyCard, ErrorCard, KpiCard, LoadingCard, PageChrome, PanelHead } from "../components/ui"

/** 状态分组顺序：正常需求流 + 线上问题状态，未识别状态归入「其他」放最后。 */
const STATUS_ORDER: string[] = [...REQ_FLOW_STATUSES, ...ISSUE_STATUSES]

interface StatusGroup {
  status: string
  reqs: Requirement[]
  /** 折叠分组（如已完成）默认收起 */
  collapsible?: boolean
}

/** 未关联 ONES 判定：与 onesBadge 一致，ones 为空/缺失视为未关联。 */
function isOnesMissing(req: Requirement): boolean {
  return !parseOnesRef(req.ones)
}

export function OnesMissingPage({ globalProject }: { globalProject?: string }) {
  const { data, error, loading } = useFetch<{ requirements: Requirement[] }>("/api/requirements")
  const [project, setProject] = useState("")
  const [doneOpen, setDoneOpen] = useState(false)
  const reqs = data?.requirements || []

  const effectiveProject = globalProject || project
  const projects = useMemo(() => [...new Set(reqs.flatMap((r) => (r.projects?.length ? r.projects : [r.project])).filter(Boolean))].sort(), [reqs])

  const missing = useMemo(() => reqs.filter(isOnesMissing), [reqs])
  const filtered = useMemo(() => {
    if (!effectiveProject) return missing
    return missing.filter((r) => (r.projects?.length ? r.projects : [r.project]).includes(effectiveProject))
  }, [missing, effectiveProject])

  const groups = useMemo<StatusGroup[]>(() => {
    const byStatus = new Map<string, Requirement[]>()
    for (const r of filtered) {
      const key = STATUS_ORDER.includes(r.status) ? r.status : "其他"
      const list = byStatus.get(key) || []
      list.push(r)
      byStatus.set(key, list)
    }
    const order = [...STATUS_ORDER, "其他"]
    return order
      .filter((status) => byStatus.has(status))
      .map((status) => ({
        status,
        reqs: byStatus.get(status)!.sort((a, b) => b.updatedAt - a.updatedAt),
        collapsible: status === "已完成",
      }))
  }, [filtered])

  const doneReqs = groups.find((g) => g.status === "已完成")?.reqs || []
  const activeReqs = filtered.filter((r) => r.status !== "已完成")
  const linkedCount = reqs.length - missing.length

  return <PageChrome icon={<Unlink2 size={15} />} eyebrow="ONES Missing" title="ONES 缺失统计" description="统计所有未关联 ONES 任务的需求（meta.md ones 字段为空），按状态分组，点击需求进入详情补充 ONES 关联；判定口径与需求卡片「⚠ 未关联 ONES」徽标一致。">
    <section className="react-kpi-grid">
      <KpiCard icon={<Unlink2 size={20} />} label="未关联 ONES" value={filtered.length} sub="ones 字段为空或缺失" tone="avg" />
      <KpiCard icon={<Unlink2 size={20} />} label="进行中未关联" value={activeReqs.length} sub="未完成状态的需求" tone="active" />
      <KpiCard icon={<CheckCircle2 size={20} />} label="已完成未关联" value={doneReqs.length} sub="历史需求默认折叠" tone="done" />
      <KpiCard icon={<Link2 size={20} />} label="已关联 ONES" value={linkedCount} sub={`全部需求 ${reqs.length} 条`} tone="total" />
    </section>
    {globalProject ? null : <section className="react-panel react-filter-panel">
      <div className="react-filter-grid">
        <label>项目<select value={project} onChange={(e) => setProject(e.target.value)}><option value="">全部项目</option>{projects.map((p) => <option key={p} value={p}>{p}</option>)}</select></label>
      </div>
      <div className="react-actions"><span className="react-muted">统计范围：全部状态下未关联 ONES 的需求；「已完成」分组默认折叠，点击展开查看；ONES 关联在需求详情页 meta.md ones 字段维护。</span></div>
    </section>}
    {error ? <ErrorCard error={error} /> : loading ? <LoadingCard /> : groups.length === 0 ? <EmptyCard>没有未关联 ONES 的需求，全部需求都已关联。</EmptyCard> : groups.map((group) => group.collapsible && !doneOpen
      ? <section key={group.status} className="react-panel">
        <button type="button" className="react-filter-section-head react-collapse-head" onClick={() => setDoneOpen(true)} aria-expanded={false}>
          <span>已完成</span>
          <em className="react-collapse-summary">{group.reqs.length} 条 · 点击展开</em>
          <ChevronDown size={14} className="react-collapse-chevron" />
        </button>
      </section>
      : <StatusSection key={group.status} status={group.status} reqs={group.reqs} />)}
  </PageChrome>
}

function StatusSection({ status, reqs }: { status: string; reqs: Requirement[] }) {
  return <section className="react-panel">
    <PanelHead kicker="Ones Missing" title={status} chip={`${reqs.length} 条`} />
    {reqs.length === 0 ? <EmptyCard>该状态下没有未关联 ONES 的需求。</EmptyCard> : <div className="react-card-list">{reqs.map((req, index) => <MissingCard key={req.id} req={req} index={index} />)}</div>}
  </section>
}

function MissingCard({ req, index }: { req: Requirement; index: number }) {
  return <motion.article className="react-list-card react-req-card" initial={{ opacity: 0, y: 12 }} animate={{ opacity: 1, y: 0 }} transition={{ delay: Math.min(index, 16) * 0.025 }} whileHover={{ y: -3 }}>
    <div>
      <span className="react-card-id">{req.id}</span>
      <h3><a href={`/requirement?id=${encodeURIComponent(req.id)}`}>{req.title}</a></h3>
      <p>{req.description || "暂无描述"}</p>
      <div className="react-card-meta">
        <span>{projectsOf(req)}</span>
        <span>{req.sessionIds?.length || 0} session(s)</span>
        <span>更新 {relAge(req.updatedAt)}</span>
      </div>
    </div>
    <div className="react-card-side">
      {req.category === "线上问题" ? <span className="react-status-pill" style={{ color: "#f87171", background: "rgba(239, 68, 68, 0.14)", borderColor: "rgba(239, 68, 68, 0.4)" }}>线上问题</span> : null}
      {statusPill(req.status)}
      <span className="react-ones-badge react-ones-missing" title="未关联 ONES 任务">⚠ 未关联 ONES</span>
    </div>
  </motion.article>
}
