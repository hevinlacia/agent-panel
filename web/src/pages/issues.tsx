/** Role: 线上问题页 — 统计问题类记录（category=线上问题/测试问题），平铺列表 + 状态筛选（排查中→已定位→已修复→经验总结/已关闭）。
 * Public surface: IssuesPage used by web/src/App.tsx route /issues.
 * Constraints: read-only view over /api/requirements; 排查经验沉淀状态以 troubleshooting.md 是否存在判定；测试问题（WMS-TST-）承接 UAT 测试反馈中不属于常规需求的轻量问题。
 * Read-this-with: web/src/pages/ones-missing.tsx for page patterns and prompts/phase-online-issue.md for the state machine.
 */
import { motion } from "framer-motion"
import { BookOpenCheck, ChevronDown, Siren, TriangleAlert } from "lucide-react"
import { useMemo, useState } from "react"
import type { Requirement, ReqCategory } from "../types"
import { useFetch } from "../lib/api"
import { relAge } from "../lib/format"
import { ISSUE_STATUS_OPTIONS, REGION_LABELS } from "../lib/requirements"
import { ISSUES_FALLBACK_EXCLUDED_STATUSES, persistIssuesDefaultExcludedStatuses, readIssuesDefaultExcludedStatuses } from "../lib/preferences"
import { projectsOf, statusPill } from "../features/requirements/badges"
import { EmptyCard, ErrorCard, KpiCard, LoadingCard, PageChrome, PanelHead } from "../components/ui"

/** 状态筛选选项顺序；后端已收欦 issue 家族不允许常规需求流状态，无需伪状态兜底。 */
const STATUS_OPTIONS: string[] = ISSUE_STATUS_OPTIONS
/** 需要排查经验的阶段：已修复起应开始沉淀，经验总结必须已有。 */
const EXPERIENCE_REQUIRED: string[] = ["已修复", "经验总结"]

function hasTroubleshootingDoc(req: Requirement): boolean {
  return Boolean(req.troubleshootingPath)
}

export function IssuesPage({ globalProject }: { globalProject?: string }) {
  const { data, error, loading } = useFetch<{ requirements: Requirement[] }>("/api/requirements")
  const urlParams = new URLSearchParams(window.location.search)
  const [keyword, setKeyword] = useState(urlParams.get("q") || "")
  const [project, setProject] = useState("")
  // 创建时间筛选（天精度）：线上问题一般当天排查当天出结果，常用于看某天登记的问题（起止同一天）。
  const [createdFrom, setCreatedFrom] = useState(urlParams.get("createdFrom") || "")
  const [createdTo, setCreatedTo] = useState(urlParams.get("createdTo") || "")
  // 状态筛选：勾选后只显示选中状态；空 = 按默认排除过滤（与需求列表交互一致）。
  const [statuses, setStatuses] = useState<string[]>(urlParams.getAll("status").filter((s) => STATUS_OPTIONS.includes(s)))
  const toggleStatus = (s: string) => setStatuses((cur) => {
    const next = cur.includes(s) ? cur.filter((x) => x !== s) : [...cur, s]
    syncUrl({ status: next })
    return next
  })
  // 默认排除状态：与需求列表同款「草稿 + 保存按钮」模型；独立存储 key（状态集合与需求不同）。
  const [issuesDefaultExcluded, setIssuesDefaultExcludedState] = useState<string[]>(readIssuesDefaultExcludedStatuses)
  const [excludedDraft, setExcludedDraft] = useState<string[]>(() => readIssuesDefaultExcludedStatuses())
  const [excludedOpen, setExcludedOpen] = useState(false)
  const [excludedJustSaved, setExcludedJustSaved] = useState(false)
  const saveIssuesDefaultExcluded = (next: string[]) => {
    const cleaned = next.filter((s, i, arr) => STATUS_OPTIONS.includes(s) && arr.indexOf(s) === i)
    setIssuesDefaultExcludedState(cleaned)
    setExcludedDraft(cleaned)
    persistIssuesDefaultExcludedStatuses(cleaned)
  }
  const statusOrder = (list: string[]) => STATUS_OPTIONS.filter((s) => list.includes(s))
  const excludedDirty = JSON.stringify(statusOrder(excludedDraft)) !== JSON.stringify(statusOrder(issuesDefaultExcluded))
  const toggleExcludedOpen = () => {
    if (!excludedOpen) setExcludedDraft(issuesDefaultExcluded) // 展开时以已保存值为草稿
    setExcludedOpen(!excludedOpen)
  }
  const saveExcludedDraft = () => {
    saveIssuesDefaultExcluded(excludedDraft)
    setExcludedJustSaved(true)
    window.setTimeout(() => setExcludedJustSaved(false), 1600)
  }
  const urlType = urlParams.get("type")
  const [typeFilter, setTypeFilter] = useState<"" | ReqCategory>(urlType === "线上问题" || urlType === "测试问题" ? urlType : "")
  // 区域筛选：__none__ = 未登记；其它值为 meta.md region 字段（cn/sea，后续可扩展更多国家/区域）。
  const urlRegion = urlParams.get("region") || ""
  const [regionFilter, setRegionFilter] = useState(urlRegion)
  const reqs = data?.requirements || []

  // issue 家族：线上问题 + 测试问题（WMS-TST-，承接 UAT 测试反馈中不属于常规需求的轻量问题），共用轻量状态机；平铺列表按更新时间倒序。
  const issues = useMemo(() => reqs.filter((r) => r.category === "线上问题" || r.category === "测试问题").sort((a, b) => b.updatedAt - a.updatedAt), [reqs])
  const typedIssues = useMemo(() => (typeFilter ? issues.filter((r) => r.category === typeFilter) : issues), [issues, typeFilter])
  /** 区域选项：从现有数据 distinct 动态生成（含未登记），新增国家/区域零代码扩展。 */
  const regionValues = useMemo(() => [...new Set(issues.map((r) => r.region ?? ""))].filter(Boolean).sort(), [issues])
  const hasUnregisteredRegion = useMemo(() => issues.some((r) => !r.region), [issues])
  const regionIssues = useMemo(() => (
    regionFilter === "__none__" ? typedIssues.filter((r) => !r.region)
      : regionFilter ? typedIssues.filter((r) => r.region === regionFilter)
      : typedIssues
  ), [typedIssues, regionFilter])
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
  const setKeywordSync = (value: string) => {
    setKeyword(value)
    syncUrl({ q: value })
  }
  const changeType = (value: "" | ReqCategory) => {
    setTypeFilter(value)
    syncUrl({ type: value })
  }
  const changeRegion = (value: string) => {
    setRegionFilter(value)
    syncUrl({ region: value })
  }
  const changeCreatedFrom = (value: string) => {
    setCreatedFrom(value)
    syncUrl({ createdFrom: value })
  }
  const changeCreatedTo = (value: string) => {
    setCreatedTo(value)
    syncUrl({ createdTo: value })
  }
  const syncUrl = (patch: { q?: string; type?: string; region?: string; status?: string[]; createdFrom?: string; createdTo?: string }) => {
    const q = new URLSearchParams(window.location.search)
    if (patch.q !== undefined) { patch.q ? q.set("q", patch.q) : q.delete("q") }
    if (patch.type !== undefined) { patch.type ? q.set("type", patch.type) : q.delete("type") }
    if (patch.region !== undefined) { patch.region ? q.set("region", patch.region) : q.delete("region") }
    if (patch.createdFrom !== undefined) { patch.createdFrom ? q.set("createdFrom", patch.createdFrom) : q.delete("createdFrom") }
    if (patch.createdTo !== undefined) { patch.createdTo ? q.set("createdTo", patch.createdTo) : q.delete("createdTo") }
    if (patch.status !== undefined) {
      q.delete("status")
      for (const s of patch.status) q.append("status", s)
    }
    window.history.replaceState(null, "", `/issues${q.toString() ? `?${q}` : ""}`)
  }
  /** 匹配基数：类型/区域/项目/关键字命中（含精确编号快速路径），不含状态筛选。 */
  const matchBase = useMemo(() => {
    const tokens = keyword.trim().toLowerCase().split(/\s+/).filter(Boolean)
    const kw = tokens.length === 1 ? tokens[0] : ""
    // 精确编号查找：关键词形如问题 ID（wms-inc-112 / wms-tst-003 / inc-112 / tst-3）或纯数字序号（112）时，
    // 直接按 ID 命中（全等 / 前缀 / 序号段匹配），无视项目筛选；精确命中为空时回退到模糊查找。
    if (kw) {
      const exact = /^[a-z]+-((inc|tst)-)?\d+/.test(kw)
        ? regionIssues.filter((r) => r.id.toLowerCase() === kw || r.id.toLowerCase().startsWith(`${kw}-`))
        : /^\d+$/.test(kw)
          ? regionIssues.filter((r) => r.id.toLowerCase().split("-").includes(kw))
          : []
      if (exact.length) return exact
    }
    let base = effectiveProject
      ? regionIssues.filter((r) => (r.projects?.length ? r.projects : [r.project]).includes(effectiveProject))
      : regionIssues
    // 创建时间筛选（天精度）：与需求列表同口径，含首尾两天的完整 24h。
    if (createdFrom) base = base.filter((r) => r.createdAt >= new Date(`${createdFrom}T00:00:00`).getTime())
    if (createdTo) base = base.filter((r) => r.createdAt <= new Date(`${createdTo}T23:59:59`).getTime())
    if (tokens.length) {
      base = base.filter((r) => {
        const fix = fixReqOf.get(r.id)
        const haystack = [
          r.id,
          r.title,
          r.description || "",
          (r.projects?.length ? r.projects : [r.project]).filter(Boolean).join(" "),
          fix?.title || "",
        ].join(" ").toLowerCase()
        return tokens.every((t) => haystack.includes(t))
      })
    }
    return base
  }, [regionIssues, effectiveProject, keyword, fixReqOf, createdFrom, createdTo])
  // 状态筛选：勾选后只显示选中状态；未勾选时应用默认排除过滤。
  const filtered = useMemo(
    () => (statuses.length
      ? matchBase.filter((r) => statuses.includes(r.status))
      : matchBase.filter((r) => !issuesDefaultExcluded.includes(r.status))),
    [matchBase, statuses, issuesDefaultExcluded],
  )
  /** 状态计数：跨全部状态统计（与需求列表计数口径一致），跟随类型/项目/关键字筛选。 */
  const counts = useMemo(
    () => Object.fromEntries(STATUS_OPTIONS.map((s) => [s, matchBase.filter((r) => r.status === s).length])) as Record<string, number>,
    [matchBase],
  )
  const countBy = (status: string) => counts[status] ?? 0
  // KPI 指标用匹配基数（不含状态筛选）统计，避免切换状态勾选时指标跳动。
  const troubleshootingCount = matchBase.filter(hasTroubleshootingDoc).length
  const pendingExperience = matchBase.filter((r) => EXPERIENCE_REQUIRED.includes(r.status) && !hasTroubleshootingDoc(r)).length

  return <PageChrome icon={<Siren size={15} />} eyebrow="Online Issues" title="线上问题" description="统计问题类记录（线上问题 + 测试问题），平铺列表展示，状态可用复选框筛选：排查中 → 已定位 → 已修复 → 经验总结。测试问题（WMS-TST- 编号池）承接 UAT 测试反馈中不属于常规需求的轻量问题，门禁更轻。排查/复现需要改代码时可直接登记 branches.json 在需求分支开发，合入 test/UAT 环境分支验证，禁止合入生产分支；已定位→已修复 双路径：数据修复直接推进；正式生产修复创建普通需求并绑定本问题，需求进入经验总结后自动推进。有价值的问题沉淀排查经验（troubleshooting.md：怎么排查 + 怎么修复），无价值的直接已关闭。">
    <section className="react-kpi-grid-5">
      <KpiCard icon={<Siren size={20} />} label={typeFilter || "问题总数"} value={matchBase.length} sub={typeFilter ? `${typeFilter} · 全部状态` : "全部状态"} tone="total" />
      <KpiCard icon={<Siren size={20} />} label="排查中" value={countBy("排查中")} sub="收集证据验证假设" tone="avg" />
      <KpiCard icon={<Siren size={20} />} label="已定位" value={countBy("已定位")} sub="根因与方案明确" tone="active" />
      <KpiCard icon={<TriangleAlert size={20} />} label="待沉淀经验" value={pendingExperience} sub="已修复但排查经验未填" tone={pendingExperience > 0 ? "avg" : "total"} />
      <KpiCard icon={<BookOpenCheck size={20} />} label="经验总结" value={countBy("经验总结")} sub={`${troubleshootingCount} 条已有排查经验`} tone="done" />
    </section>
    {globalProject ? null : <section className="react-panel react-filter-panel">
      <div className="react-filter-grid">
        <label>类型<select value={typeFilter} onChange={(e) => changeType(e.target.value as "" | ReqCategory)}><option value="">全部（含测试问题）</option><option value="线上问题">线上问题</option><option value="测试问题">测试问题</option></select></label>
        <label>区域<select value={regionFilter} onChange={(e) => changeRegion(e.target.value)}><option value="">全部区域</option>{regionValues.map((r) => <option key={r} value={r}>{REGION_LABELS[r] ?? r}</option>)}{hasUnregisteredRegion ? <option value="__none__">未登记</option> : null}</select></label>
        <label>项目<select value={project} onChange={(e) => setProject(e.target.value)}><option value="">全部项目</option>{projects.map((p) => <option key={p} value={p}>{p}</option>)}</select></label>
        <label>创建开始<input type="date" value={createdFrom} onChange={(e) => changeCreatedFrom(e.target.value)} /></label>
        <label>创建结束<input type="date" value={createdTo} onChange={(e) => changeCreatedTo(e.target.value)} /></label>
        <label className="react-filter-grow">查找问题<input value={keyword} onChange={(e) => setKeywordSync(e.target.value)} placeholder="编号精确：WMS-INC-112 / WMS-TST-3 / 112；或关键字模糊：标题 / 描述 / 关联需求" /></label>
      </div>
      <div className="react-filter-section-head"><span>状态筛选</span><em>勾选后只显示选中状态（覆盖默认排除）；计数跟随类型/项目/关键字/日期跨全部状态统计</em></div>
      <div className="react-status-options">{STATUS_OPTIONS.map((s) => <label key={s} className={`react-status-option ${statuses.includes(s) ? "active" : ""}`}><input type="checkbox" checked={statuses.includes(s)} onChange={() => toggleStatus(s)} /><span>{s}</span><strong>{counts[s] || 0}</strong></label>)}</div>
      <button type="button" className={`react-filter-section-head react-collapse-head ${excludedOpen ? "open" : ""}`} onClick={toggleExcludedOpen} aria-expanded={excludedOpen}><span>默认排除状态</span><em className="react-collapse-summary">{excludedOpen ? "未勾选上方状态筛选时自动生效；勾选只改草稿，点「保存默认排除」才生效" : (issuesDefaultExcluded.length ? `默认排除：${issuesDefaultExcluded.join(" / ")}` : "默认不排除任何状态")}<ChevronDown size={14} className="react-collapse-chevron" /></em></button>
      {excludedOpen ? <div className="react-excluded-editor"><div className="react-status-options react-excluded-status-options">{STATUS_OPTIONS.map((s) => <label key={s} className={`react-status-option react-excluded-status-option ${excludedDraft.includes(s) ? "active" : ""}`}><input type="checkbox" checked={excludedDraft.includes(s)} onChange={(e) => setExcludedDraft((cur) => e.target.checked ? [...cur, s] : cur.filter((x) => x !== s))} /><span>{s}</span><strong>{counts[s] || 0}</strong></label>)}</div><div className="react-excluded-save-row"><button type="button" onClick={saveExcludedDraft} disabled={!excludedDirty}>{excludedJustSaved ? "已保存 ✓" : "保存默认排除"}</button><button type="button" onClick={() => saveIssuesDefaultExcluded(ISSUES_FALLBACK_EXCLUDED_STATUSES)} disabled={!excludedDirty && issuesDefaultExcluded.join() === ISSUES_FALLBACK_EXCLUDED_STATUSES.join()}>恢复默认</button><em>{excludedDirty ? "有未保存的修改，保存后作用于列表并记住" : "与已保存一致"}</em></div></div> : null}
      <div className="react-actions"><span className="react-muted">统计范围：全部状态下问题类记录（category=线上问题/测试问题，未强调测试环境的问题默认线上问题）；列表按更新时间倒序平铺展示。排查经验在需求详情页「排查经验」面板维护，线上问题进入经验总结前必须先填写（测试问题门禁更轻）。</span></div>
    </section>}
    {error ? <ErrorCard error={error} /> : loading ? <LoadingCard /> : filtered.length === 0 ? <EmptyCard>{keyword.trim() ? `没有匹配「${keyword.trim()}」的问题，试试其他关键字或完整编号。` : statuses.length ? "选中状态下没有问题，取消部分状态勾选试试。" : matchBase.length ? "当前默认排除设置下没有可见问题，可展开「默认排除状态」调整。" : "没有问题记录。创建时选择类别「线上问题」（未强调测试环境默认）或「测试问题」（UAT 测试反馈）即可进入本流程。"}</EmptyCard> : <section className="react-panel">
      <PanelHead kicker="Online Issues" title="问题列表" chip={`${filtered.length} 条`} />
      <div className="react-card-list">{filtered.map((req, index) => <IssueCard key={req.id} req={req} index={index} fixReq={fixReqOf.get(req.id)} showType={!typeFilter} />)}</div>
    </section>}
  </PageChrome>
}

function IssueCard({ req, index, fixReq, showType }: { req: Requirement; index: number; fixReq?: Requirement; showType: boolean }) {
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
      {req.region ? <span className="react-status-pill" title={`区域：${REGION_LABELS[req.region] ?? req.region}`} style={{ color: "#60a5fa", background: "rgba(96, 165, 250, 0.12)", borderColor: "rgba(96, 165, 250, 0.4)" }}>{REGION_LABELS[req.region] ?? req.region}</span> : null}
      {showType ? (req.category === "测试问题"
        ? <span className="react-status-pill" title="测试问题：UAT 测试反馈中不属于常规需求的轻量问题（WMS-TST- 编号池）" style={{ color: "#fbbf24", background: "rgba(251, 191, 36, 0.12)", borderColor: "rgba(251, 191, 36, 0.4)" }}>测试问题</span>
        : <span className="react-status-pill" style={{ color: "#f87171", background: "rgba(239, 68, 68, 0.14)", borderColor: "rgba(239, 68, 68, 0.4)" }}>线上问题</span>) : null}
      {statusPill(req.status)}
    </div>
  </motion.article>
}
