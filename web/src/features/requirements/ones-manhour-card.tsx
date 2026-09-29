/**
 * Role: 需求详情页 ONES 工时卡片 — 展示/维护需求工时四联：
 * 人工预估（可编辑）、自动预估（effort-estimate，只读）、ONES 已登记（实时只读）、
 * agent 实际（可编辑），并支持快速发起单条工时登记（准备模式）。
 * Public surface: OnesManhourCard used by web/src/pages/requirement.tsx.
 * Constraints: 写仅限 POST /api/ones/manhour/save 与 /api/ones/manhour/log；
 * ONES 已登记工时来自工时报表缓存（窗口期由后端标注），只读不落盘。
 * Read-this-with: src/ones_manhour.rs、web/src/features/requirements/req-meta-panels.tsx（OnesBindModal）。
 */
import { useCallback, useEffect, useRef, useState } from "react"
import { Clock3, RefreshCw } from "lucide-react"
import type { OnesManhourInfo, Requirement } from "../../types"
import { fetchJson, postJson } from "../../lib/api"
import { relAge } from "../../lib/format"
import { PanelHead } from "../../components/ui"

function todayStr(): string {
  const d = new Date()
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`
}

const fmtH = (v?: number | null) => (typeof v === "number" ? `${v}h` : "-")

export function OnesManhourCard({ req, onOpenOnesBind, onSaved }: { req: Requirement; onOpenOnesBind: () => void; onSaved: () => void }) {
  const info = useOnesManhour(req.id)
  const [manual, setManual] = useState("")
  const [agent, setAgent] = useState("")
  const [agentNote, setAgentNote] = useState("")
  const [saving, setSaving] = useState(false)
  const [feedback, setFeedback] = useState<string | null>(null)
  const [logDate, setLogDate] = useState(todayStr())
  const [logHours, setLogHours] = useState("")
  const [logRemark, setLogRemark] = useState("")
  const [logging, setLogging] = useState(false)
  /** 输入框初值只同步一次（info 首次加载），避免覆盖用户正在编辑的值。 */
  const syncedRef = useRef(false)
  useEffect(() => {
    if (syncedRef.current || !info.data) return
    syncedRef.current = true
    setManual(info.data.manualHours != null ? String(info.data.manualHours) : "")
    setAgent(info.data.agentHours != null ? String(info.data.agentHours) : "")
    setAgentNote(info.data.agentHoursNote || "")
  }, [info.data])

  const save = async () => {
    if (saving) return
    setSaving(true)
    setFeedback(null)
    try {
      await postJson("/api/ones/manhour/save", {
        reqId: req.id,
        manualHours: manual.trim() === "" ? null : Number(manual),
        agentHours: agent.trim() === "" ? null : Number(agent),
        agentHoursNote: agentNote.trim() === "" ? null : agentNote,
      })
      setFeedback("工时档案已保存")
      onSaved()
      info.refresh()
    } catch (err) {
      setFeedback(`保存失败：${err instanceof Error ? err.message : String(err)}`)
    } finally {
      setSaving(false)
    }
  }

  const quickLog = async () => {
    if (logging || !logHours.trim()) return
    setLogging(true)
    setFeedback(null)
    try {
      const res = await postJson<{ mode: string; displayId?: string; results: { status: string; message?: string }[] }>("/api/ones/manhour/log", {
        reqId: req.id,
        execute: false,
        entries: [{ date: logDate, hours: Number(logHours), remark: logRemark }],
      })
      const first = res.results?.[0]
      setFeedback(first?.status === "prepared" ? `已生成登记计划（准备模式）：${res.displayId ?? ""} ${logDate} ${logHours}h，可打开 ONES 任务页登记` : `登记未完成：${first?.message ?? "未知原因"}`)
      setLogHours("")
      setLogRemark("")
      info.refresh()
    } catch (err) {
      setFeedback(`登记失败：${err instanceof Error ? err.message : String(err)}`)
    } finally {
      setLogging(false)
    }
  }

  const d = info.data
  const bound = Boolean(d?.displayId)
  return <section id="ones-manhour" className="react-panel"><PanelHead kicker="ONES Manhour" title="ONES 工时" chip={<>{bound ? <a href={d?.taskUrl || d?.ones?.url || "#"} target="_blank" rel="noopener noreferrer"><code>{d?.displayId}</code></a> : <em className="react-muted">未关联</em>}{d?.onesFetchedAt ? <span className="react-muted" title="工时报表缓存时间"> · 数据 {relAge(d.onesFetchedAt)}</span> : null}</>} />
    {info.error ? <p className="react-effort-error">工时信息加载失败：{info.error instanceof Error ? info.error.message : String(info.error)}</p> : info.loading && !d ? <p className="react-muted">加载中…</p> : !bound ? <div><p className="react-muted">本需求尚未关联 ONES 任务：关联后即可跟踪已登记工时、剩余工时并生成登记清单。</p><div className="react-actions"><button type="button" onClick={onOpenOnesBind}>登记 ONES 关联</button></div></div> : <><div className="react-meta-grid">
      <span title="用户自己觉得这个需求要花多久（可编辑，保存到 ones-manhour.json）"><strong className="react-field-edit" onClick={() => document.getElementById("ones-manual-hours")?.focus()}>人工预估</strong> <input id="ones-manual-hours" className="react-manhour-input" type="number" min="0" max="1000" step="0.5" value={manual} onChange={(e) => setManual(e.target.value)} placeholder="小时" /></span>
      <span title="agent 凭需求内容与历史工作量评估的预估（effort-estimate.json，agent 可通过 /api/requirement/effort-estimate 更新）">自动预估 {fmtH(d?.autoHours)}{d?.autoHoursUpdatedAt ? <em className="react-muted">（{relAge(d.autoHoursUpdatedAt)}评估）</em> : null}</span>
      <span title={`ONES 工时报表窗口内该任务已登记工时（窗口 ${d?.manhourWindowDays ?? 120} 天）`}>实际录入 <strong className="react-manhour-logged">{fmtH(d?.loggedHours)}</strong></span>
      <span title="agent 从开始开发到 UAT 回归完成实际投入工时（雏形阶段手动维护，后续可按 session 时间线自动估算）"><strong className="react-field-edit" onClick={() => document.getElementById("ones-agent-hours")?.focus()}>agent 工时</strong> <input id="ones-agent-hours" className="react-manhour-input" type="number" min="0" max="1000" step="0.5" value={agent} onChange={(e) => setAgent(e.target.value)} placeholder="小时" /></span>
      <span title="人工预估 − 实际录入">剩余 <strong>{fmtH(d?.remainingHours)}</strong>{d?.remainingHours != null && d.remainingHours <= 0 ? <em className="react-muted">（已录满）</em> : null}</span>
    </div>
    <div className="react-inline-form react-manhour-agent-note"><input value={agentNote} onChange={(e) => setAgentNote(e.target.value)} placeholder="agent 工时备注（如：含 2 轮 UAT 回归）" /></div>
    <div className="react-actions">
      <button type="button" onClick={save} disabled={saving}><Clock3 size={13} />{saving ? "保存中…" : "保存工时档案"}</button>
      <button type="button" className="react-ghost-btn" onClick={() => info.refresh(true)} disabled={info.refreshing} title="重新从 ONES 拉取已登记工时（回源，忽略缓存）"><RefreshCw size={13} />{info.refreshing ? "刷新中…" : "刷新 ONES 工时"}</button>
    </div>
    <div className="react-manhour-log-row">
      <span className="react-muted">快速登记：</span>
      <input type="date" value={logDate} onChange={(e) => setLogDate(e.target.value)} />
      <input className="react-manhour-input" type="number" min="0.5" max="24" step="0.5" value={logHours} onChange={(e) => setLogHours(e.target.value)} placeholder="小时" />
      <input value={logRemark} onChange={(e) => setLogRemark(e.target.value)} placeholder="备注（可选）" onKeyDown={(e) => { if (e.key === "Enter") quickLog() }} />
      <button type="button" onClick={quickLog} disabled={logging || !logHours.trim()}>{logging ? "提交中…" : "生成登记清单"}</button>
      <em className="react-muted" title="雏形：ONES 写入接口待抓包验证，当前生成清单并打开任务页人工确认">准备模式</em>
    </div>
    </>}
    {d?.warnings?.length ? d.warnings.map((w, i) => <p key={i} className="react-muted">⚠ {w}</p>) : null}
    {feedback ? <p className={feedback.startsWith("保存失败") || feedback.startsWith("登记失败") || feedback.startsWith("登记未完成") ? "react-effort-error" : "react-save-hint"}>{feedback}</p> : null}
    {d?.logHistory?.length ? <details className="react-manhour-history"><summary className="react-muted">登记历史（{d.logHistory.length}）</summary><div className="react-card-meta">{[...d.logHistory].reverse().slice(0, 10).map((h, i) => <span key={i} className="react-linked-issue-chip"><code>{h.date}</code> {h.hours}h <em className="react-muted">{h.status === "prepared" ? "已生成清单" : h.status === "logged" ? "已写入" : "失败"}</em>{h.taskUrl ? <a href={h.taskUrl} target="_blank" rel="noopener noreferrer">任务 ↗</a> : null}</span>)}</div></details> : null}
  </section>
}

/** ONES 工时信息 hook：GET /api/ones/manhour，支持强制回源刷新。 */
export function useOnesManhour(reqId: string) {
  const [data, setData] = useState<OnesManhourInfo | null>(null)
  const [error, setError] = useState<Error | null>(null)
  const [loading, setLoading] = useState(true)
  const [refreshing, setRefreshing] = useState(false)
  const aliveRef = useRef(true)
  useEffect(() => {
    aliveRef.current = true
    return () => { aliveRef.current = false }
  }, [reqId])
  const load = useCallback((refresh?: boolean) => {
    const force = refresh ?? false
    if (force) setRefreshing(true)
    setLoading(true)
    fetchJson<OnesManhourInfo>(`/api/ones/manhour?reqId=${encodeURIComponent(reqId)}${force ? "&refresh=true" : ""}`)
      .then((d) => { if (aliveRef.current) { setData(d); setError(null) } })
      .catch((err) => { if (aliveRef.current) setError(err instanceof Error ? err : new Error(String(err))) })
      .finally(() => { if (aliveRef.current) { setLoading(false); setRefreshing(false) } })
  }, [reqId])
  useEffect(() => { load(false) }, [load])
  return { data, error, loading, refreshing, refresh: load }
}
