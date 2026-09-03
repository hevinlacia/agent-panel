/**
 * Role: 线上问题页 — 只统计 category=线上问题 的记录，按线上问题专用状态机（排查中→已定位→已修复→已复盘/已关闭）分组。
 * Public surface: IssuesPage used by web/src/App.tsx route /issues.
 * Constraints: read-only view over /api/requirements; 排查经验沉淀状态以 troubleshooting.md 是否存在判定。
 * Read-this-with: web/src/pages/ones-missing.tsx for page patterns and prompts/phase-online-issue.md for the state machine.
 */
import { motion } from "framer-motion"
import { BookOpenCheck, ChevronDown, Siren, TriangleAlert } from "lucide-react"
import { useMemo, useState } from "react"
import type { Requirement } from "../types"
import { useFetch } from "../lib/api"
import { relAge } from "../lib/format"
import { ISSUE_STATUSES } from "../lib/requirements"
import { projectsOf, statusPill } from "../features/requirements/badges"
import { EmptyCard, ErrorCard, KpiCard, LoadingCard, PageChrome, PanelHead } from "../components/ui"

/** 状态分组顺序；未识别状态归入「其他」放最后。 */
const ISSUE_ORDER: string[] = [...ISSUE_STATUSES]
/** 需要排查经验的阶段：已修复起应开始沉淀，已复盘必须已有。 */
const EXPERIENCE_REQUIRED: string[] = ["已修复", "已复盘"]

interface IssueGroup {
  status: string
  reqs: Requirement[]
  /** 折叠分组（如已关闭）默认收起 */
  collapsible?: boolean
}

function hasTroubleshootingDoc(req: Requirement): boolean {
  return Boolean(req.troubleshootingPath)
}

export function IssuesPage({ globalProject }: { globalProject?: string }) {
  const { data, error, loading } = useFetch<{ requirements: Requirement[] }>("/api/requirements")
  const [project, setProject] = useState("")
  const [closedOpen, setClosedOpen] = useState(false)
  const reqs = data?.requirements || []

  const issues = useMemo(() => reqs.filter((r) => r.category === "线上问题"), [reqs])
  /** 反查修复需求：绑定该线上问题的普通需求（meta.md issues 字段）。 */
  const fixReqOf = useMemo(() => {
    const map = new Map<string, Requirement>()
    for (const r of reqs) {
      if ((r.category ?? "需求") !== "需求") continue
      for (const id of r.issues || []) {
        if (!map.has(id)) map.set(id, r)
      }
    }
    return map
  }, [reqs])
  const projects = useMemo(() => [...new Set(issues.flatMap((r) => (r.projects?.length ? r.projects : [r.project])).filter(Boolean))].sort(), [issues])
  const effectiveProject = globalProject || project
  const filtered = useMemo(() => {
    if (!effectiveProject) return issues
    return issues.filter((r) => (r.projects?.length ? r.projects : [r.project]).includes(effectiveProject))
  }, [issues, effectiveProject])

  const groups = useMemo<IssueGroup[]>(() => {
    const byStatus = new Map<string, Requirement[]>()
    for (const r of filtered) {
      const key = ISSUE_ORDER.includes(r.status) ? r.status : "其他"
      const list = byStatus.get(key) || []
      list.push(r)
      byStatus.set(key, list)
    }
    const order = [...ISSUE_ORDER, "其他"]
    return order
      .filter((status) => byStatus.has(status))
      .map((status) => ({
        status,
        reqs: byStatus.get(status)!.sort((a, b) => b.updatedAt - a.updatedAt),
        collapsible: status === "已关闭",
      }))
  }, [filtered])

  const countBy = (status: string) => groups.find((g) => g.status === status)?.reqs.length ?? 0
  const troubleshootingCount = filtered.filter(hasTroubleshootingDoc).length
  const pendingExperience = filtered.filter((r) => EXPERIENCE_REQUIRED.includes(r.status) && !hasTroubleshootingDoc(r)).length

  return <PageChrome icon={<Siren size={15} />} eyebrow="Online Issues" title="线上问题" description="只统计线上问题（category=线上问题），按专用状态机分组：排查中 → 已定位 → 已修复 → 已复盘。已定位→已修复 双路径：数据修复直接推进；代码修复创建普通需求并绑定本问题，需求进入经验总结后自动推进。有价值的问题沉淀排查经验（troubleshooting.md：怎么排查 + 怎么修复），无价值的直接已关闭。">
    <section className="react-kpi-grid-5">
      <KpiCard icon={<Siren size={20} />} label="线上问题" value={filtered.length} sub="全部状态" tone="total" />
      <KpiCard icon={<Siren size={20} />} label="排查中" value={countBy("排查中")} sub="收集证据验证假设" tone="avg" />
      <KpiCard icon={<Siren size={20} />} label="已定位" value={countBy("已定位")} sub="根因与方案明确" tone="active" />
      <KpiCard icon={<TriangleAlert size={20} />} label="待沉淀经验" value={pendingExperience} sub="已修复但排查经验未填" tone={pendingExperience > 0 ? "avg" : "total"} />
      <KpiCard icon={<BookOpenCheck size={20} />} label="已复盘" value={countBy("已复盘")} sub={`${troubleshootingCount} 条已有排查经验`} tone="done" />
    </section>
    {globalProject ? null : <section className="react-panel react-filter-panel">
      <div className="react-filter-grid">
        <label>项目<select value={project} onChange={(e) => setProject(e.target.value)}><option value="">全部项目</option>{projects.map((p) => <option key={p} value={p}>{p}</option>)}</select></label>
      </div>
      <div className="react-actions"><span className="react-muted">统计范围：全部状态下 category=线上问题 的记录；「已关闭」分组默认折叠，点击展开；排查经验在需求详情页「排查经验」面板维护，进入已复盘前必须先填写。</span></div>
    </section>}
    {error ? <ErrorCard error={error} /> : loading ? <LoadingCard /> : groups.length === 0 ? <EmptyCard>没有线上问题记录。创建需求时选择类别「线上问题」即可进入本流程。</EmptyCard> : groups.map((group) => group.collapsible && !closedOpen
      ? <section key={group.status} className="react-panel">
        <button type="button" className="react-filter-section-head react-collapse-head" onClick={() => setClosedOpen(true)} aria-expanded={false}>
          <span>已关闭</span>
          <em className="react-collapse-summary">{group.reqs.length} 条 · 点击展开</em>
          <ChevronDown size={14} className="react-collapse-chevron" />
        </button>
      </section>
      : <IssueSection key={group.status} status={group.status} reqs={group.reqs} fixReqOf={fixReqOf} />)}
  </PageChrome>
}

function IssueSection({ status, reqs, fixReqOf }: { status: string; reqs: Requirement[]; fixReqOf: Map<string, Requirement> }) {
  return <section className="react-panel">
    <PanelHead kicker="Online Issue" title={status} chip={`${reqs.length} 条`} />
    {reqs.length === 0 ? <EmptyCard>该状态下没有线上问题。</EmptyCard> : <div className="react-card-list">{reqs.map((req, index) => <IssueCard key={req.id} req={req} index={index} fixReq={fixReqOf.get(req.id)} />)}</div>}
  </section>
}

function IssueCard({ req, index, fixReq }: { req: Requirement; index: number; fixReq?: Requirement }) {
  const needExperience = EXPERIENCE_REQUIRED.includes(req.status) && !hasTroubleshootingDoc(req)
  const waitingForReq = Boolean(fixReq) && ["排查中", "已定位"].includes(req.status)
  return <motion.article className="react-list-card react-req-card" initial={{ opacity: 0, y: 12 }} animate={{ opacity: 1, y: 0 }} transition={{ delay: Math.min(index, 16) * 0.025 }} whileHover={{ y: -3 }}>
    <div>
      <span className="react-card-id">{req.id}</span>
      <h3><a href={`/requirement?id=${encodeURIComponent(req.id)}`}>{req.title}</a></h3>
      <p>{req.description || "暂无描述"}</p>
      <div className="react-card-meta">
        <span>{projectsOf(req)}</span>
        <span>{req.sessionIds?.length || 0} session(s)</span>
        <span>更新 {relAge(req.updatedAt)}</span>
        {fixReq ? <span>修复需求 <a href={`/requirement?id=${encodeURIComponent(fixReq.id)}`}>{fixReq.id}</a>{waitingForReq ? "（等待需求进入经验总结）" : ""}</span> : null}
      </div>
    </div>
    <div className="react-card-side">
      {hasTroubleshootingDoc(req) ? <a className="react-effort-badge" href={`/requirement?id=${encodeURIComponent(req.id)}#troubleshooting`} title="查看排查经验（怎么排查 + 怎么修复）">排查经验</a> : null}
      {needExperience ? <span className="react-ones-badge react-ones-missing" title="已修复但排查经验（troubleshooting.md）未沉淀，复盘前必须填写">⚠ 待沉淀经验</span> : null}
      {statusPill(req.status)}
    </div>
  </motion.article>
}
