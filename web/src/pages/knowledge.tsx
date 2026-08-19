import { motion } from "framer-motion"
import { Activity, AlertTriangle, CheckCircle2, Lightbulb, Library, RefreshCw } from "lucide-react"
import { useEffect, useMemo, useState } from "react"
import type { ExperienceSummaryDispatchPayload, ExperienceSummaryJobsPayload, KnowledgeDraft, KnowledgeItem, KnowledgeKind, KnowledgeListPayload, KnowledgeSavePayload, Requirement } from "../types"
import { fetchJson, postJson, useFetch } from "../lib/api"
import { joinList, relAge, splitCsvText } from "../lib/format"
import { EmptyCard, ErrorCard, KpiCard, LoadingCard, PageChrome, PanelHead } from "../components/ui"
import { experienceSummaryPill, experienceSummaryStageLabel, projectsOf, statusPill } from "../features/requirements/badges"

function emptyKnowledgeDraft(kind: KnowledgeKind): KnowledgeDraft {
  return {
    kind,
    title: "",
    domain: "general",
    project: "",
    scope: "project",
    status: "active",
    confidence: "medium",
    tags: [],
    triggerTerms: [],
    relatedSkills: [],
    relatedRepos: [],
    relatedTables: [],
    relatedApis: [],
    source: "manual",
    summary: "",
    details: kind === "businessKnowledge" ? "## 概述\n\n## 规则\n\n## 相关接口\n\n## 相关表\n\n## 注意事项" : "## 现象\n\n## 根因\n\n## 处理方式\n\n## 验证方式\n\n## 过期风险",
  }
}

function knowledgeStatusPill(status?: string) {
  const s = status || "active"
  const color = s === "active" ? "#22c55e" : s === "stale" ? "#f59e0b" : s === "deprecated" ? "#ef4444" : "#94a3b8"
  const soft = s === "active" ? "rgba(34,197,94,.14)" : s === "stale" ? "rgba(245,158,11,.14)" : s === "deprecated" ? "rgba(239,68,68,.14)" : "rgba(148,163,184,.14)"
  return <span className="react-status-pill" style={{ color, background: soft, borderColor: `${color}66` }}>{s}</span>
}

function confidencePill(confidence?: string) {
  const value = confidence || "medium"
  return <span className="react-effort-badge">confidence {value}</span>
}

export function KnowledgePage({ kind }: { kind: KnowledgeKind }) {
  if (kind === "experience") return <ExperiencesPage />
  return <KnowledgeEditorPage kind={kind} />
}

function KnowledgeEditorPage({ kind }: { kind: KnowledgeKind }) {
  const isBiz = kind === "businessKnowledge"
  const label = isBiz ? "业务知识" : "经验"
  const [query, setQuery] = useState("")
  const [domain, setDomain] = useState("")
  const [status, setStatus] = useState("")
  const [selectedId, setSelectedId] = useState<string | null>(null)
  const [draft, setDraft] = useState<KnowledgeDraft>(() => emptyKnowledgeDraft(kind))
  const [saving, setSaving] = useState(false)
  const [message, setMessage] = useState<string | null>(null)
  const params = new URLSearchParams({ kind, limit: "100" })
  if (query) params.set("q", query)
  if (domain) params.set("domain", domain)
  if (status) params.set("status", status)
  const list = useFetch<KnowledgeListPayload>(`/api/knowledge?${params.toString()}`, [kind, query, domain, status])
  const items = list.data?.items || []
  const domains = useMemo(() => Array.from(new Set(items.map((i) => i.domain).filter(Boolean) as string[])).sort(), [items])
  const selected = selectedId ? items.find((item) => item.id === selectedId) || null : null
  useEffect(() => { setDraft(emptyKnowledgeDraft(kind)); setSelectedId(null); setMessage(null) }, [kind])
  const editItem = async (item: KnowledgeItem) => {
    setSelectedId(item.id)
    try {
      const full = await fetchJson<{ item: KnowledgeItem }>(`/api/knowledge/item?id=${encodeURIComponent(item.id)}`)
      setDraft({ ...full.item, details: full.item.details || "" })
    } catch {
      setDraft({ ...item, details: item.summary || "" })
    }
  }
  const resetDraft = () => { setSelectedId(null); setDraft(emptyKnowledgeDraft(kind)); setMessage(null) }
  const save = async () => {
    if (!draft.title?.trim() || saving) return
    setSaving(true)
    setMessage(null)
    try {
      const payload = {
        id: selectedId || draft.id || undefined,
        kind,
        title: draft.title,
        domain: draft.domain || "general",
        project: draft.project || "",
        scope: draft.scope || "project",
        status: draft.status || "active",
        confidence: draft.confidence || "medium",
        tags: splitCsvText(draft.tags),
        triggerTerms: splitCsvText(draft.triggerTerms),
        relatedSkills: splitCsvText(draft.relatedSkills),
        relatedRepos: splitCsvText(draft.relatedRepos),
        relatedTables: splitCsvText(draft.relatedTables),
        relatedApis: splitCsvText(draft.relatedApis),
        source: draft.source || "manual",
        summary: draft.summary || "",
        details: draft.details || "",
      }
      const saved = await postJson<KnowledgeSavePayload>("/api/knowledge", payload)
      setSelectedId(saved.item.id)
      setDraft({ ...saved.item, details: saved.item.details || draft.details || "" })
      setMessage("已保存")
      list.refresh()
    } catch (err) {
      setMessage(`保存失败：${err instanceof Error ? err.message : String(err)}`)
    } finally {
      setSaving(false)
    }
  }
  const apiHint = `POST /api/agent/knowledge/query\n{\n  "kind": "${kind}",\n  "intent": "${query || (isBiz ? "状态流转 / 接口字段" : "排障现象 / 错误信息")}",\n  "domain": "${domain || "wms"}",\n  "limit": 5,\n  "tokensBudget": 2200\n}`
  return <PageChrome icon={isBiz ? <Library size={15} /> : <Lightbulb size={15} />} eyebrow={isBiz ? "Business Knowledge" : "Experiences"} title={label} description={isBiz ? "维护短保质期的业务事实、规则、接口和表关系；Agent 默认只查询摘要。" : "维护特化排障经验、踩坑记录和历史处理方式；成熟稳定后再提升为 skill。"} actions={<button onClick={list.refresh}><RefreshCw size={15} />刷新</button>}><section className="react-panel react-filter-panel"><div className="react-filter-grid"><label>关键词<input value={query} onChange={(e) => setQuery(e.target.value)} placeholder={isBiz ? "状态 / 接口 / 表 / 字段" : "现象 / 报错 / 模块 / 处理方式"} /></label><label>Domain<input list="knowledge-domains" value={domain} onChange={(e) => setDomain(e.target.value)} placeholder="wms / general" /><datalist id="knowledge-domains">{domains.map((d) => <option key={d} value={d} />)}</datalist></label><label>Status<select value={status} onChange={(e) => setStatus(e.target.value)}><option value="">全部</option><option value="active">active</option><option value="stale">stale</option><option value="deprecated">deprecated</option><option value="draft">draft</option></select></label><label>接口提示<textarea readOnly rows={4} value={apiHint} /></label></div><div className="react-actions"><button onClick={resetDraft}>新建{label}</button><a href={isBiz ? "/experiences" : "/business-knowledge"}>{isBiz ? "切到经验" : "切到业务知识"}</a></div></section>{list.error ? <ErrorCard error={list.error} /> : <div className="react-knowledge-layout"><section className="react-card-list">{list.loading ? <LoadingCard /> : items.length === 0 ? <EmptyCard>暂无{label}记录。</EmptyCard> : items.map((item, index) => <KnowledgeCard key={item.id} item={item} selected={item.id === selectedId} index={index} onEdit={() => editItem(item)} />)}</section><KnowledgeEditor kind={kind} draft={draft} setDraft={setDraft} saving={saving} selected={selected} message={message} onSave={save} onReset={resetDraft} /></div>}</PageChrome>
}

function KnowledgeCard({ item, selected, index, onEdit }: { item: KnowledgeItem; selected: boolean; index: number; onEdit: () => void }) {
  return <motion.article className={`react-list-card react-knowledge-card ${selected ? "active" : ""}`} initial={{ opacity: 0, y: 12 }} animate={{ opacity: 1, y: 0 }} transition={{ delay: Math.min(index, 16) * 0.025 }} whileHover={{ y: -3 }}><div><span className="react-card-id">{item.id}</span><h3>{item.title}</h3><p>{item.summary || "暂无摘要"}</p><div className="react-card-meta"><span>{item.domain || "general"}</span>{item.project ? <span>{item.project}</span> : null}<span>{item.scope || "project"}</span><span>更新 {item.updatedAt || "-"}</span>{item.whyMatched?.length ? <span>match: {item.whyMatched.join(" / ")}</span> : null}</div><div className="react-chip-list">{(item.tags || []).slice(0, 6).map((tag) => <span key={tag}>{tag}</span>)}</div></div><div className="react-card-side">{knowledgeStatusPill(item.status)}{confidencePill(item.confidence)}<button type="button" className="react-copy-link-btn" onClick={onEdit}>编辑</button></div></motion.article>
}

export function ExperiencesPage() {
  const params = new URLSearchParams(window.location.search)
  const initialTab = params.get("tab") === "knowledge" ? "knowledge" : "summary"
  const [tab, setTab] = useState<"summary" | "knowledge">(initialTab)
  const switchTab = (next: "summary" | "knowledge") => {
    setTab(next)
    const q = new URLSearchParams(window.location.search)
    if (next === "knowledge") q.set("tab", "knowledge"); else q.delete("tab")
    window.history.replaceState(null, "", `/experiences${q.toString() ? `?${q}` : ""}`)
  }
  return <PageChrome icon={<Lightbulb size={15} />} eyebrow="Experiences" title="经验" description="集中查看需求经验自动总结进度，也可维护长期经验库条目。"><div className="react-tab-row"><button className={tab === "summary" ? "active" : ""} onClick={() => switchTab("summary")}>需求总结</button><button className={tab === "knowledge" ? "active" : ""} onClick={() => switchTab("knowledge")}>经验库</button></div>{tab === "summary" ? <ExperienceSummaryJobsPage /> : <KnowledgeEditorPage kind="experience" />}</PageChrome>
}

function ExperienceSummaryJobsPage() {
  const [stage, setStage] = useState("all")
  const [refreshing, setRefreshing] = useState(false)
  const jobs = useFetch<ExperienceSummaryJobsPayload>(`/api/experience-summary/jobs${stage === "all" ? "" : `?status=${encodeURIComponent(stage)}`}`, [stage])
  const items = jobs.data?.items || []
  const stats = jobs.data?.stats
  const dispatchNow = async () => {
    setRefreshing(true)
    try {
      await postJson<ExperienceSummaryDispatchPayload>("/api/experience-summary/jobs/dispatch", {})
      jobs.refresh()
    } finally {
      setRefreshing(false)
    }
  }
  return <>
    <section className="react-kpi-grid">
      <KpiCard icon={<Lightbulb size={20} />} label="可总结" value={stats?.available ?? "-"} sub="需求已进入经验总结" tone="avg" />
      <KpiCard icon={<Activity size={20} />} label="总结中" value={stats?.running ?? "-"} sub={`并发 ${jobs.data?.config?.maxAgents ?? 3}`} tone="active" />
      <KpiCard icon={<CheckCircle2 size={20} />} label="已完成" value={stats?.completed ?? "-"} sub="可查看报告" tone="done" />
      <KpiCard icon={<AlertTriangle size={20} />} label="失败" value={stats?.failed ?? "-"} sub="可重试" tone="total" />
    </section>
    <section className="react-panel react-filter-panel"><PanelHead kicker="Auto Summary" title="自动经验总结队列" chip={jobs.data?.config?.enabled ? "ON" : "OFF"} /><p className="react-muted">当需求状态进入“经验总结”后，Agent Panel 会按设置中的模型和并发数自动派发 pi agent，总结完成后写入 experience-summary.md 并标记完成。</p><div className="react-tab-row"><button className={stage === "all" ? "active" : ""} onClick={() => setStage("all")}>全部</button>{["available", "running", "completed", "failed", "skipped"].map((s) => <button key={s} className={stage === s ? "active" : ""} onClick={() => setStage(s)}>{experienceSummaryStageLabel(s)}</button>)}<button onClick={dispatchNow} disabled={refreshing}><RefreshCw size={15} className={refreshing ? "react-spin" : ""} />立即派发</button><a href="/settings">配置</a></div></section>
    {jobs.error ? <ErrorCard error={jobs.error} /> : jobs.loading ? <LoadingCard /> : <div className="react-card-list">{items.length === 0 ? <EmptyCard>暂无可展示的需求总结任务。</EmptyCard> : items.map((item, index) => <ExperienceSummaryJobCard key={item.req.id} req={item.req} stage={item.stage} index={index} onChanged={jobs.refresh} />)}</div>}
  </>
}

function ExperienceSummaryJobCard({ req, stage, index, onChanged }: { req: Requirement; stage: string; index: number; onChanged: () => void }) {
  const [working, setWorking] = useState(false)
  const retry = async () => {
    setWorking(true)
    try {
      await postJson("/api/experience-summary/jobs/retry", { reqId: req.id, note: "UI 手动重试自动经验总结" })
      onChanged()
    } finally {
      setWorking(false)
    }
  }
  const job = req.experienceSummaryJob
  return <motion.article className={`react-list-card react-exp-job-card react-exp-job-${stage}`} initial={{ opacity: 0, y: 12 }} animate={{ opacity: 1, y: 0 }} transition={{ delay: Math.min(index, 16) * 0.025 }} whileHover={{ y: -3 }}><div><span className="react-card-id">{req.id}</span><h3><a href={`/requirement?id=${encodeURIComponent(req.id)}`}>{req.title}</a></h3><p>{job?.error || req.description || "暂无描述"}</p><div className="react-card-meta"><span>{projectsOf(req)}</span><span>{experienceSummaryStageLabel(stage)}</span>{job?.sessionId ? <span>session {job.sessionId.slice(0, 8)}…</span> : null}{job?.updatedAt ? <span>更新 {relAge(job.updatedAt)}</span> : <span>需求更新 {relAge(req.updatedAt)}</span>}{job?.attempts ? <span>attempts {job.attempts}</span> : null}</div>{job?.model ? <code className="react-command">{job.model}</code> : null}</div><div className="react-card-side">{experienceSummaryPill(req) || statusPill(req.status)}<a href={`/requirement-doc?id=${encodeURIComponent(req.id)}&doc=experience-summary&title=${encodeURIComponent("经验总结报告")}`}>查看报告</a>{job?.sessionId ? <a href={`/session?id=${encodeURIComponent(job.sessionId)}`}>总结 Agent</a> : null}<button onClick={retry} disabled={working}>{working ? "派发中…" : stage === "failed" ? "重试" : "重新总结"}</button></div></motion.article>
}

function KnowledgeEditor({ kind, draft, setDraft, saving, selected, message, onSave, onReset }: { kind: KnowledgeKind; draft: KnowledgeDraft; setDraft: (draft: KnowledgeDraft) => void; saving: boolean; selected: KnowledgeItem | null; message: string | null; onSave: () => void; onReset: () => void }) {
  const isBiz = kind === "businessKnowledge"
  const set = (patch: KnowledgeDraft) => setDraft({ ...draft, ...patch })
  return <section className="react-panel react-knowledge-editor"><PanelHead kicker={selected ? "Edit" : "Create"} title={selected ? selected.title : `新建${isBiz ? "业务知识" : "经验"}`} chip={draft.id || "new"} /><div className="react-knowledge-form"><label>标题<input value={draft.title || ""} onChange={(e) => set({ title: e.target.value })} placeholder={isBiz ? "例如：WMS 出库单状态流转" : "例如：波次取消后库存占用未释放排查"} /></label><label>Domain<input value={draft.domain || ""} onChange={(e) => set({ domain: e.target.value })} placeholder="wms" /></label><label>Project<input value={draft.project || ""} onChange={(e) => set({ project: e.target.value })} placeholder="可选" /></label><label>Scope<select value={draft.scope || "project"} onChange={(e) => set({ scope: e.target.value })}><option value="project">project</option><option value="global">global</option></select></label><label>Status<select value={draft.status || "active"} onChange={(e) => set({ status: e.target.value })}><option value="active">active</option><option value="stale">stale</option><option value="deprecated">deprecated</option><option value="draft">draft</option></select></label><label>Confidence<select value={draft.confidence || "medium"} onChange={(e) => set({ confidence: e.target.value })}><option value="low">low</option><option value="medium">medium</option><option value="high">high</option></select></label><label>Tags<input value={joinList(draft.tags)} onChange={(e) => set({ tags: splitCsvText(e.target.value) })} placeholder="逗号分隔" /></label><label>Trigger Terms<input value={joinList(draft.triggerTerms)} onChange={(e) => set({ triggerTerms: splitCsvText(e.target.value) })} placeholder="Agent 查询关键词" /></label><label>Related Skills<input value={joinList(draft.relatedSkills)} onChange={(e) => set({ relatedSkills: splitCsvText(e.target.value) })} placeholder="skill-name" /></label><label>Related Repos<input value={joinList(draft.relatedRepos)} onChange={(e) => set({ relatedRepos: splitCsvText(e.target.value) })} placeholder="yl-cwhsea-wms-web" /></label><label>Related Tables<input value={joinList(draft.relatedTables)} onChange={(e) => set({ relatedTables: splitCsvText(e.target.value) })} placeholder="shipment_header" /></label><label>Related APIs<input value={joinList(draft.relatedApis)} onChange={(e) => set({ relatedApis: splitCsvText(e.target.value) })} placeholder="POST /wms-web/... 或 api-* id" /></label><label>Source<input value={draft.source || ""} onChange={(e) => set({ source: e.target.value })} placeholder="manual / code / test / session" /></label><label className="react-knowledge-wide">摘要<textarea rows={3} value={draft.summary || ""} onChange={(e) => set({ summary: e.target.value })} placeholder="给 Agent 默认返回的一句话或短段摘要" /></label><label className="react-knowledge-wide">详情<textarea rows={14} value={draft.details || ""} onChange={(e) => set({ details: e.target.value })} placeholder="Markdown 正文" /></label></div><div className="react-actions"><button onClick={onSave} disabled={saving || !draft.title?.trim()}>{saving ? "保存中…" : "保存"}</button><button type="button" onClick={onReset}>重置</button></div>{message ? <p className={message.startsWith("保存失败") ? "react-effort-error" : "react-save-hint"}>{message}</p> : null}<p className="react-muted">保存后写入 <code>{isBiz ? ".agents/business-knowledge/" : ".agents/experiences/"}</code> 或全局 <code>~/.agents/</code> 对应目录，自动维护 created_at / updated_at。</p></section>
}
