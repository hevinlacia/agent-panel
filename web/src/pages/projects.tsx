import { motion } from "framer-motion"
import { ChevronDown, Copy, ListChecks } from "lucide-react"
import { useEffect, useMemo, useState } from "react"
import type { HarnessCurrent, ReqStatus, Requirement } from "../types"
import { useFetch } from "../lib/api"
import { parseOnesRef, relAge } from "../lib/format"
import { REQ_FLOW_STATUSES, REQ_SOURCES } from "../lib/requirements"
import { FALLBACK_DEFAULT_EXCLUDED_STATUSES, persistDefaultExcludedStatuses, readDefaultExcludedStatuses, readProjectFilter } from "../lib/preferences"
import { copyRequirementSessionCommand } from "../features/requirements/session-command"
import { experienceSummaryPill, onesBadge, projectsOf, statusPill } from "../features/requirements/badges"
import { EmptyCard, ErrorCard, LoadingCard, PageChrome } from "../components/ui"

function billableWindow(): { from: number; to: number; label: string } {
  const now = new Date()
  const y = now.getFullYear()
  const m = now.getMonth()
  const to = new Date(y, m, 20, 23, 59, 59, 999)
  const from = new Date(y, m - 1, 20, 0, 0, 0, 0)
  const fmt = (d: Date) => `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`
  return { from: from.getTime(), to: to.getTime(), label: `${fmt(from)} ~ ${fmt(to)}` }
}

export function ProjectsPage({ globalProject }: { globalProject: string }) {
  const { data, error, loading } = useFetch<{ requirements: Requirement[] }>("/api/requirements")
  const harness = useFetch<HarnessCurrent>("/api/harness/current")
  // 仅 pi / dsh-tui 有可粘贴的终端启动命令；dsh-web 无命令形态，不展示复制按钮。
  const canCopyCommand = (harness.data?.harness ?? "pi") !== "dsh-web"
  const params = new URLSearchParams(window.location.search)
  const [project, setProject] = useState(params.get("project") || readProjectFilter())
  useEffect(() => {
    if (!globalProject) return
    setProject(globalProject)
    const q = new URLSearchParams(window.location.search)
    q.set("project", globalProject)
    window.history.replaceState(null, "", `${window.location.pathname}?${q.toString()}`)
  }, [globalProject])
  const [createdFrom, setCreatedFrom] = useState(params.get("createdFrom") || "")
  const [createdTo, setCreatedTo] = useState(params.get("createdTo") || "")
  const [statuses, setStatuses] = useState<string[]>(params.getAll("status"))
  const [defaultExcludedStatuses, setDefaultExcludedStatusesState] = useState<string[]>(readDefaultExcludedStatuses)
  const [keyword, setKeyword] = useState(params.get("q") || "")
  const [billableOnly, setBillableOnly] = useState(false)
  const [noOnesOnly, setNoOnesOnly] = useState(false)
  const [source, setSource] = useState(params.get("source") || "")
  const [excludedOpen, setExcludedOpen] = useState(false)
  const reqs = data?.requirements || []
  const win = useMemo(() => billableWindow(), [])
  // 需求看板只统计普通需求（category=需求）；线上问题有独立侧边栏页 /issues。
  const requirementOnly = useMemo(() => reqs.filter((r) => (r.category ?? "需求") === "需求"), [reqs])
  const projects = useMemo(() => [...new Set(requirementOnly.flatMap((r) => r.projects?.length ? r.projects : [r.project]).filter(Boolean))].sort(), [requirementOnly])
  const counts = useMemo(() => Object.fromEntries(REQ_FLOW_STATUSES.map((s) => [s, requirementOnly.filter((r) => r.status === s).length])), [requirementOnly]) as Record<string, number>
  const setDefaultExcludedStatuses = (updater: (current: string[]) => string[]) => {
    setDefaultExcludedStatusesState((current) => {
      const next = updater(current).filter((s, i, arr) => REQ_FLOW_STATUSES.includes(s as ReqStatus) && arr.indexOf(s) === i)
      persistDefaultExcludedStatuses(next)
      return next
    })
  }
  const resetDefaultExcludedStatuses = () => setDefaultExcludedStatuses(() => FALLBACK_DEFAULT_EXCLUDED_STATUSES)
  const filtered = useMemo(() => requirementOnly.filter((r) => {
    // 多关键词搜索：空格分隔的关键字 AND 匹配（全部命中才保留）；单个关键词保留原有精确 ID 快速命中行为。
    const tokens = keyword.trim().toLowerCase().split(/\s+/).filter(Boolean)
    const kw = tokens.length === 1 ? tokens[0] : ""
    // 精确需求 ID 查找：单个关键词形如需求 ID（如 WMS-080 / WMS-080-xxx）且命中某需求 ID（含前缀）时，
    // 无视状态/项目/时间等筛选条件直接命中，属于精确查找（类别已在 requirementOnly 硬过滤）。
    if (kw && /^[a-z]+-\d+/.test(kw) && (r.id.toLowerCase() === kw || r.id.toLowerCase().startsWith(`${kw}-`))) return true
    if (!billableOnly && statuses.length === 0 && defaultExcludedStatuses.includes(r.status)) return false
    if (statuses.length && !statuses.includes(r.status)) return false
    if (project && !(r.projects?.length ? r.projects : [r.project]).includes(project)) return false
    if (source && (r.source || "产品推动") !== source) return false
    if (createdFrom && r.createdAt < new Date(`${createdFrom}T00:00:00`).getTime()) return false
    if (createdTo && r.createdAt > new Date(`${createdTo}T23:59:59`).getTime()) return false
    if (tokens.length) {
      const haystack = [r.id, r.title, r.description || "", projectsOf(r)].join(" ").toLowerCase()
      if (!tokens.every((t) => haystack.includes(t))) return false
    }
    if (billableOnly) {
      if (parseOnesRef(r.ones)) return false
      if (r.status === "已完成") {
        const doneAt = r.completedAt ?? r.updatedAt
        if (doneAt < win.from || doneAt > win.to) return false
      }
    }
    if (noOnesOnly && parseOnesRef(r.ones)) return false
    return true
  }).sort((a, b) => b.updatedAt - a.updatedAt), [requirementOnly, statuses, defaultExcludedStatuses, project, source, createdFrom, createdTo, keyword, billableOnly, noOnesOnly, win])
  const billableHours = useMemo(() => filtered.reduce((sum, r) => sum + (r.effortEstimate?.estimatedHours || 0), 0), [filtered])
  const apply = () => {
    const q = new URLSearchParams()
    if (createdFrom) q.set("createdFrom", createdFrom)
    if (createdTo) q.set("createdTo", createdTo)
    if (project) q.set("project", project)
    if (source) q.set("source", source)
    if (keyword) q.set("q", keyword)
    for (const s of statuses) q.append("status", s)
    window.location.href = `/projects${q.toString() ? `?${q}` : ""}`
  }
  return <PageChrome icon={<ListChecks size={15} />} eyebrow="Requirements" title="需求进度看板" description="按项目、推动方、状态和创建时间筛选需求，查看关联 pi session 和最近更新；线上问题在独立侧边栏页查看。"><section className="react-panel react-filter-panel"><div className="react-filter-grid"><label>项目<select value={project} onChange={(e) => setProject(e.target.value)}><option value="">全部项目</option>{projects.map((p) => <option key={p} value={p}>{p}</option>)}</select></label><label>推动方<select value={source} onChange={(e) => setSource(e.target.value)} title="筛选：产品推动 / 开发推动的需求"><option value="">全部推动方</option>{REQ_SOURCES.map((s) => <option key={s} value={s}>{s}</option>)}</select></label><label>创建开始<input type="date" value={createdFrom} onChange={(e) => setCreatedFrom(e.target.value)} /></label><label>创建结束<input type="date" value={createdTo} onChange={(e) => setCreatedTo(e.target.value)} /></label><label className="react-filter-grow">关键词<input value={keyword} onChange={(e) => setKeyword(e.target.value)} placeholder="标题 / req id / 描述；空格分隔多个关键字" /></label></div><div className="react-filter-section-head"><span>状态筛选</span><em>勾选后只显示选中状态，会覆盖默认排除</em></div><div className="react-status-options">{REQ_FLOW_STATUSES.map((s) => <label key={s} className={`react-status-option ${statuses.includes(s) ? "active" : ""}`}><input type="checkbox" checked={statuses.includes(s)} onChange={(e) => setStatuses((cur) => e.target.checked ? [...cur, s] : cur.filter((x) => x !== s))} /><span>{s}</span><strong>{counts[s] || 0}</strong></label>)}</div><button type="button" className={`react-filter-section-head react-collapse-head ${excludedOpen ? "open" : ""}`} onClick={() => setExcludedOpen((v) => !v)} aria-expanded={excludedOpen}><span>默认排除状态</span><em className="react-collapse-summary">{excludedOpen ? "未选择上方状态筛选时自动生效，勾选后立即保存" : (defaultExcludedStatuses.length ? `默认排除：${defaultExcludedStatuses.join(" / ")}` : "默认不排除任何状态")}<ChevronDown size={14} className="react-collapse-chevron" /></em></button>{excludedOpen ? <div className="react-status-options react-excluded-status-options">{REQ_FLOW_STATUSES.map((s) => <label key={s} className={`react-status-option react-excluded-status-option ${defaultExcludedStatuses.includes(s) ? "active" : ""}`}><input type="checkbox" checked={defaultExcludedStatuses.includes(s)} onChange={(e) => setDefaultExcludedStatuses((cur) => e.target.checked ? [...cur, s] : cur.filter((x) => x !== s))} /><span>{s}</span><strong>{counts[s] || 0}</strong></label>)}</div> : null}<div className="react-actions"><button onClick={apply}>应用筛选</button><a href="/projects">重置</a><button type="button" onClick={resetDefaultExcludedStatuses}>默认排除恢复默认</button><button type="button" className={`react-toggle-btn ${noOnesOnly ? "active" : ""}`} onClick={() => setNoOnesOnly((v) => !v)} title="筛选：未关联 ONES 任务的需求，可与其他筛选条件叠加">未关联 ONES</button><button type="button" className={`react-toggle-btn ${billableOnly ? "active" : ""}`} onClick={() => setBillableOnly((v) => !v)} title="筛选：未关联 ONES、完成时间在上月20到本月20之间或未完成">本月可结算业务工时</button>{defaultExcludedStatuses.length ? <span className="react-muted">默认排除：{defaultExcludedStatuses.join(" / ")}</span> : <span className="react-muted">默认不排除任何状态</span>}</div></section>{error ? <ErrorCard error={error} /> : loading ? <LoadingCard /> : <>{billableOnly ? <div className="react-billable-summary"><span>本月可结算业务工时</span><strong>{billableHours.toFixed(1)}h</strong><span>{filtered.length} 条需求</span><span className="react-muted">{win.label}</span></div> : null}<div className="react-card-list">{filtered.length === 0 ? <EmptyCard>暂无符合条件的需求。</EmptyCard> : filtered.map((req, index) => <RequirementCard key={req.id} req={req} index={index} canCopyCommand={canCopyCommand} />)}</div></>}</PageChrome>
}

function RequirementCard({ req, index, canCopyCommand }: { req: Requirement; index: number; canCopyCommand: boolean }) {
  const [copied, setCopied] = useState(false)
  const [copying, setCopying] = useState(false)
  const [copyError, setCopyError] = useState<string | null>(null)
  const copyCommand = async () => {
    if (copying) return
    setCopying(true)
    setCopyError(null)
    try {
      await copyRequirementSessionCommand(req.id)
      setCopied(true)
      window.setTimeout(() => setCopied(false), 1600)
    } catch (err) {
      setCopyError(err instanceof Error ? err.message : String(err))
      window.setTimeout(() => setCopyError(null), 4000)
    } finally {
      setCopying(false)
    }
  }
  return <motion.article className="react-list-card react-req-card" initial={{ opacity: 0, y: 12 }} animate={{ opacity: 1, y: 0 }} transition={{ delay: Math.min(index, 16) * 0.025 }} whileHover={{ y: -3 }}><div><span className="react-card-id">{req.id}</span><h3><a href={`/requirement?id=${encodeURIComponent(req.id)}`}>{req.title}</a></h3><p>{req.description || "暂无描述"}</p><div className="react-card-meta"><span>{projectsOf(req)}</span>{req.issues?.length ? <span title="绑定的线上问题，需求进入经验总结后自动推进">🔗 {req.issues.length} 线上问题</span> : null}<span>{req.sessionIds?.length || 0} session(s)</span><span>更新 {relAge(req.updatedAt)}</span></div></div><div className="react-card-side">{canCopyCommand ? <button type="button" className="react-card-copy-btn" onClick={copyCommand} disabled={copying} title="复制最新终端启动命令（pi/dsh-tui）；命令被使用后再次点击自动换新 session id"><Copy size={13} />{copying ? "复制中…" : copied ? "已复制" : "复制命令"}</button> : null}{copyError ? <span className="react-card-copy-error" title={copyError}>复制失败</span> : null}{req.effortEstimate ? <span className="react-effort-badge">{req.effortEstimate.estimatedHours}h</span> : null}{req.source === "开发推动" ? <span className="react-status-pill" title="开发推动：进入测试中前必须有测试场景文档（test-scenario.md）" style={{ color: "#fbbf24", background: "rgba(251, 191, 36, 0.12)", borderColor: "rgba(251, 191, 36, 0.4)" }}>开发推动</span> : null}{req.category === "线上问题" ? <span className="react-status-pill" style={{ color: "#f87171", background: "rgba(239, 68, 68, 0.14)", borderColor: "rgba(239, 68, 68, 0.4)" }}>线上问题</span> : null}{experienceSummaryPill(req)}{statusPill(req.status)}{onesBadge(req.ones)}</div></motion.article>
}

export function RequirementsData() { return useFetch<{ requirements: Requirement[] }>("/api/requirements") }
