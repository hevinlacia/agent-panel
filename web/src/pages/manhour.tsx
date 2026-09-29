/**
 * Role: 工时录入页 — 按条件筛选可录入工时的需求（状态/创建时间/剩余工时/已绑 ONES），
 * 勾选并填写录入明细后一键生成登记（雏形为准备模式：ONES 写入接口待验证，
 * 生成登记清单 + 任务链接，人工确认后写入）。
 * Public surface: ManhourPage used by web/src/App.tsx route /manhour.
 * Constraints: 只读 /api/requirements + /api/ones/tasks；写仅走 /api/ones/manhour/log（逐需求调用）。
 * Read-this-with: src/ones_manhour.rs（log API 契约）、web/src/pages/projects.tsx（RequirementsData）、
 * web/src/lib/requirements.ts（状态常量）。
 */
import { CheckCircle2, Clock3, RefreshCw } from "lucide-react"
import { useMemo, useState } from "react"
import type { OnesTasksResponse, Requirement } from "../types"
import { postJson, useFetch } from "../lib/api"
import { parseOnesRef } from "../lib/format"
import { REQ_FLOW_STATUSES } from "../lib/requirements"
import { projectsOf, statusPill } from "../features/requirements/badges"
import { EmptyCard, ErrorCard, LoadingCard, PageChrome, PanelHead } from "../components/ui"
import { RequirementsData } from "./projects"

interface LogRow {
  req: Requirement
  displayId: string | null
  taskUrl: string | null
  manualHours: number | null
  autoHours: number | null
  loggedHours: number | null
  remainingHours: number | null
  agentHours: number | null
}

interface LogOutcome {
  reqId: string
  displayId?: string
  ok: boolean
  status?: string
  message?: string
  taskUrl?: string
  detail: string
}

/** 与后端 extract_display_id 一致：从 ones 字段提取任务编号。 */
function reqDisplayId(req: Requirement): string | null {
  const raw = req.ones || ""
  const m = raw.match(/[A-Za-z][A-Za-z0-9_]*-\d+/)
  return m ? m[0].toUpperCase() : null
}

function todayStr(): string {
  const d = new Date()
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`
}

const fmtH = (v: number | null | undefined) => (typeof v === "number" ? `${v}h` : "-")

export function ManhourPage({ globalProject }: { globalProject?: string }) {
  const { data, error, loading, refresh } = RequirementsData()
  const ones = useFetch<OnesTasksResponse>("/api/ones/tasks")
  const [project, setProject] = useState(globalProject || "")
  const [statuses, setStatuses] = useState<string[]>([])
  const [createdFrom, setCreatedFrom] = useState("")
  const [createdTo, setCreatedTo] = useState("")
  const [keyword, setKeyword] = useState("")
  const [remainingOnly, setRemainingOnly] = useState(true)
  const [boundOnly, setBoundOnly] = useState(true)
  const [issueIncluded, setIssueIncluded] = useState(false)
  /** 每行录入明细：reqId → { hours, date, remark }。 */
  const [drafts, setDrafts] = useState<Record<string, { hours: string; date: string; remark: string }>>({})
  const [selected, setSelected] = useState<Set<string>>(new Set())
  const [logging, setLogging] = useState(false)
  const [outcomes, setOutcomes] = useState<LogOutcome[] | null>(null)
  const [logError, setLogError] = useState<string | null>(null)

  const hoursMap = useMemo(() => {
    const map = new Map<string, { hours: number; uuid: string; url: string }>()
    for (const c of ones.data?.candidates || []) {
      map.set(c.displayId.toUpperCase(), { hours: c.actualHours ?? 0, uuid: c.taskUuid || "", url: c.url })
    }
    return map
  }, [ones.data])

  const rows: LogRow[] = useMemo(() => {
    const kw = keyword.trim().toLowerCase()
    const from = createdFrom ? new Date(`${createdFrom}T00:00:00`).getTime() : 0
    const to = createdTo ? new Date(`${createdTo}T23:59:59`).getTime() : Number.MAX_SAFE_INTEGER
    return (data?.requirements || [])
      .filter((r) => (issueIncluded ? true : (r.category ?? "需求") === "需求"))
      .filter((r) => (project ? (r.projects || [r.project]).includes(project) : true))
      .filter((r) => (statuses.length ? statuses.includes(r.status) : true))
      .filter((r) => r.createdAt >= from && r.createdAt <= to)
      .filter((r) => (kw ? `${r.id} ${r.title} ${r.description || ""}`.toLowerCase().includes(kw) : true))
      .map((r) => {
        const did = reqDisplayId(r)
        const hit = did ? hoursMap.get(did) : undefined
        const manual = r.onesManhour?.manualHours ?? null
        const logged = did && hit ? hit.hours : null
        return {
          req: r,
          displayId: did,
          taskUrl: hit?.url ?? (did ? parseOnesRef(r.ones)?.url ?? null : null),
          manualHours: manual,
          autoHours: r.effortEstimate?.estimatedHours ?? null,
          loggedHours: logged,
          remainingHours: manual != null ? Math.max(0, Math.round(((manual - (logged ?? 0)) * 100))) / 100 : null,
          agentHours: r.onesManhour?.agentHours ?? null,
        }
      })
      .filter((row) => (boundOnly ? Boolean(row.displayId) : true))
      .filter((row) => (remainingOnly ? (row.manualHours ?? 0) > 0 && (row.remainingHours ?? 0) > 0 : true))
  }, [data, hoursMap, project, statuses, createdFrom, createdTo, keyword, remainingOnly, boundOnly, issueIncluded])

  const draftOf = (reqId: string) => drafts[reqId] || { hours: "", date: todayStr(), remark: "" }
  const setDraft = (reqId: string, patch: Partial<{ hours: string; date: string; remark: string }>) =>
    setDrafts((prev) => ({ ...prev, [reqId]: { ...draftOf(reqId), ...patch } }))

  const toggle = (reqId: string) =>
    setSelected((prev) => {
      const next = new Set(prev)
      if (next.has(reqId)) next.delete(reqId)
      else next.add(reqId)
      return next
    })
  const allSelected = rows.length > 0 && rows.every((r) => selected.has(r.req.id))

  /** 按剩余工时预填选中行：min(剩余, 8)，至少 0.5。 */
  const prefillSelected = () => {
    setDrafts((prev) => {
      const next = { ...prev }
      for (const r of rows) {
        if (!selected.has(r.req.id)) continue
        const remain = r.remainingHours
        if (remain != null && remain > 0) {
          const cur = Number(next[r.req.id]?.hours || 0)
          if (!cur) next[r.req.id] = { ...draftOf(r.req.id), hours: String(Math.min(remain, 8)) }
        }
      }
      return next
    })
  }

  const submit = async () => {
    if (logging) return
    const picked = rows.filter((r) => selected.has(r.req.id) && Number(draftOf(r.req.id).hours) > 0)
    if (!picked.length) return
    setLogging(true)
    setLogError(null)
    setOutcomes(null)
    const results: LogOutcome[] = []
    try {
      for (const r of picked) {
        const d = draftOf(r.req.id)
        try {
          const res = await postJson<{ displayId?: string; results: { status: string; message?: string; taskUrl?: string }[] }>(
            "/api/ones/manhour/log",
            { reqId: r.req.id, execute: false, entries: [{ date: d.date, hours: Number(d.hours), remark: d.remark }] },
          )
          const first = res.results?.[0]
          results.push({
            reqId: r.req.id,
            displayId: res.displayId,
            ok: first?.status === "prepared",
            status: first?.status,
            message: first?.message,
            taskUrl: first?.taskUrl,
            detail: `${d.date} ${d.hours}h${d.remark ? ` · ${d.remark}` : ""}`,
          })
        } catch (err) {
          results.push({ reqId: r.req.id, ok: false, detail: `${d.date} ${d.hours}h · ${err instanceof Error ? err.message : String(err)}` })
        }
      }
    } finally {
      setOutcomes(results)
      setLogging(false)
    }
  }

  const copyPlan = async () => {
    if (!outcomes?.length) return
    const lines = [`工时登记清单 ${todayStr()}`]
    for (const o of outcomes) lines.push(`${o.displayId ?? "?"}\t${o.detail}\t${o.taskUrl ?? ""}`)
    try {
      await navigator.clipboard.writeText(lines.join("\n"))
    } catch {
      /* 剪贴板失败静默，清单已在页面展示 */
    }
  }

  const totalHours = rows
    .filter((r) => selected.has(r.req.id))
    .reduce((sum, r) => sum + (Number(draftOf(r.req.id).hours) || 0), 0)

  return <PageChrome icon={<Clock3 size={15} />} eyebrow="Manhour" title="工时录入" description="筛选今天可录入工时的需求：限制需求状态、创建时间、剩余工时（预估 > 已录入）；勾选并填写明细后一键生成登记清单（ONES 写入接口验证前为准备模式）。">
    {error ? <ErrorCard error={error} /> : loading ? <LoadingCard /> : <>
      <section className="react-panel"><PanelHead kicker="Filters" title="筛选条件" chip={`${rows.length} 条`} />
        <div className="react-manhour-filters">
          <label>项目 <select value={project} onChange={(e) => setProject(e.target.value)}><option value="">全部</option><option value="WMS">WMS</option></select></label>
          <label>创建从 <input type="date" value={createdFrom} onChange={(e) => setCreatedFrom(e.target.value)} /></label>
          <label>到 <input type="date" value={createdTo} onChange={(e) => setCreatedTo(e.target.value)} /></label>
          <label>关键字 <input value={keyword} onChange={(e) => setKeyword(e.target.value)} placeholder="标题 / req id" /></label>
          <label className="react-manhour-check"><input type="checkbox" checked={remainingOnly} onChange={(e) => setRemainingOnly(e.target.checked)} />仅剩剩余工时</label>
          <label className="react-manhour-check"><input type="checkbox" checked={boundOnly} onChange={(e) => setBoundOnly(e.target.checked)} />仅已绑 ONES</label>
          <label className="react-manhour-check"><input type="checkbox" checked={issueIncluded} onChange={(e) => setIssueIncluded(e.target.checked)} />含线上/测试问题</label>
          <button type="button" className="react-ghost-btn" onClick={() => { refresh(); ones.refresh() }}><RefreshCw size={13} />刷新</button>
        </div>
        <details className="react-manhour-status-filter"><summary className="react-muted">按状态筛选（{statuses.length ? `已选 ${statuses.length}` : "不限"}）</summary><div className="react-card-meta">{REQ_FLOW_STATUSES.map((s) => <label key={s} className="react-manhour-check react-linked-issue-chip"><input type="checkbox" checked={statuses.includes(s)} onChange={() => setStatuses((prev) => (prev.includes(s) ? prev.filter((x) => x !== s) : [...prev, s]))} />{s}</label>)}</div></details>
      </section>

      <section className="react-panel">
        <PanelHead kicker="Candidates" title="可录入需求" chip={`${selected.size} 选 / 合计 ${Math.round(totalHours * 100) / 100}h`} />
        {ones.error ? <p className="react-effort-error">ONES 工时数据获取失败：{ones.error}（可检查 Chrome 登录态）</p> : null}
        {!rows.length ? <EmptyCard>没有满足筛选条件的需求：调整筛选（如关闭「仅剩剩余工时」）或先在需求详情页填写人工预估工时。</EmptyCard> : <div className="react-table-wrap"><table className="react-manhour-table">
          <thead><tr><th></th><th>需求</th><th>ONES</th><th>人工预估</th><th>自动预估</th><th>已录入</th><th>剩余</th><th>agent</th><th>本次录入</th></tr></thead>
          <tbody>{rows.map((row) => {
            const d = draftOf(row.req.id)
            const checked = selected.has(row.req.id)
            return <tr key={row.req.id} className={checked ? "react-manhour-row-on" : undefined}>
              <td><input type="checkbox" checked={checked} onChange={() => toggle(row.req.id)} /></td>
              <td><a href={`/requirement?id=${encodeURIComponent(row.req.id)}`}><code>{row.req.id}</code></a> <span title={row.req.title}>{row.req.title.slice(0, 24)}{row.req.title.length > 24 ? "…" : ""}</span> {statusPill(row.req.status)}<em className="react-muted"> {projectsOf(row.req)}</em></td>
              <td>{row.displayId ? <a href={row.taskUrl || "#"} target="_blank" rel="noopener noreferrer"><code>{row.displayId}</code></a> : <em className="react-muted">未绑</em>}</td>
              <td>{fmtH(row.manualHours)}</td>
              <td title="effort-estimate.json">{fmtH(row.autoHours)}</td>
              <td title={row.loggedHours == null ? "ONES 工时报表窗口内未匹配到该任务（可能无登记或超出窗口）" : undefined}>{row.loggedHours == null ? <em className="react-muted">?</em> : fmtH(row.loggedHours)}</td>
              <td>{row.remainingHours != null && row.remainingHours <= 0 ? <em className="react-muted">已录满</em> : fmtH(row.remainingHours)}</td>
              <td>{fmtH(row.agentHours)}</td>
              <td><div className="react-manhour-log-row react-manhour-log-row-plain"><input type="number" min="0.5" max="24" step="0.5" value={d.hours} onChange={(e) => setDraft(row.req.id, { hours: e.target.value })} placeholder="h" /><input type="date" value={d.date} onChange={(e) => setDraft(row.req.id, { date: e.target.value })} /><input value={d.remark} onChange={(e) => setDraft(row.req.id, { remark: e.target.value })} placeholder="备注" /></div></td>
            </tr>
          })}</tbody>
        </table></div>}
        <div className="react-actions">
          <button type="button" onClick={() => setSelected(allSelected ? new Set() : new Set(rows.map((r) => r.req.id)))} disabled={!rows.length}>{allSelected ? "清空选择" : "全选"}</button>
          <button type="button" onClick={prefillSelected} disabled={!selected.size}>按剩余预填选中行</button>
          <button type="button" onClick={submit} disabled={logging || !selected.size || ![...selected].some((id) => Number(draftOf(id).hours) > 0)}><CheckCircle2 size={13} />{logging ? "提交中…" : `一键录入（${selected.size} 需求 / ${Math.round(totalHours * 100) / 100}h）`}</button>
        </div>
        <p className="react-muted">准备模式：雏形阶段 ONES 写入接口尚未抓包验证，提交后生成逐任务登记清单与任务链接，人工到 ONES 确认；接口验证后此处将直接写入。</p>
      </section>

      {logError ? <ErrorCard error={logError} /> : null}
      {outcomes ? <section className="react-panel"><PanelHead kicker="Result" title="登记结果" chip={`${outcomes.filter((o) => o.ok).length}/${outcomes.length} 成功`} />
        <div className="react-card-meta">{outcomes.map((o) => <span key={o.reqId} className="react-linked-issue-chip"><a href={`/requirement?id=${encodeURIComponent(o.reqId)}`}>{o.reqId}</a><code>{o.displayId ?? "未绑"}</code><span>{o.detail}</span><em className={o.ok ? "react-save-hint" : "react-effort-error"}>{o.ok ? "已生成清单" : o.message || "失败"}</em>{o.taskUrl ? <a href={o.taskUrl} target="_blank" rel="noopener noreferrer">任务 ↗</a> : null}</span>)}</div>
        <div className="react-actions"><button type="button" onClick={copyPlan}>复制登记清单</button></div>
      </section> : null}
    </>}
  </PageChrome>
}
