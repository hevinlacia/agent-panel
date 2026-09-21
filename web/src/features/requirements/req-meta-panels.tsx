import { useState } from "react"
import type { Requirement } from "../../types"
import { postForm, postJson } from "../../lib/api"
import { statusPill } from "./badges"
import { REQ_CATEGORIES, REQ_SOURCES } from "../../lib/requirements"
import { PanelHead } from "../../components/ui"

export function LinkedIssuesPanel({ req, issues, onSaved }: { req: Requirement; issues: Requirement[]; onSaved: () => void }) {
  const [input, setInput] = useState("")
  const [saving, setSaving] = useState(false)
  const [feedback, setFeedback] = useState<string | null>(null)
  const bound = req.issues || []
  const boundReqs = bound.map((id) => issues.find((r) => r.id === id)).filter(Boolean) as Requirement[]
  const add = async () => {
    const id = input.trim()
    if (saving || !id) return
    if (bound.includes(id)) { setFeedback("该线上问题已绑定"); return }
    setSaving(true)
    setFeedback(null)
    try {
      const res = await postJson<{ ok: boolean; autoAdvancedIssues?: string[] }>("/api/requirement/update", { reqId: req.id, issues: [...bound, id] })
      setInput("")
      setFeedback(res.autoAdvancedIssues?.length ? `已绑定；需求状态已达经验总结，自动推进：${res.autoAdvancedIssues.join("、")}` : "绑定成功")
      onSaved()
    } catch (err) {
      setFeedback(`绑定失败：${err instanceof Error ? err.message : String(err)}`)
    } finally {
      setSaving(false)
    }
  }
  const remove = async (id: string) => {
    if (saving) return
    setSaving(true)
    setFeedback(null)
    try {
      await postJson("/api/requirement/update", { reqId: req.id, issues: bound.filter((x) => x !== id) })
      setFeedback("已解除绑定")
      onSaved()
    } catch (err) {
      setFeedback(`解绑失败：${err instanceof Error ? err.message : String(err)}`)
    } finally {
      setSaving(false)
    }
  }
  return <section id="linked-issues" className="react-panel"><PanelHead kicker="Linked Issues" title="关联线上问题" chip={`${bound.length}`} /><p className="react-muted">绑定由本需求修复的线上问题（category=线上问题，支持多个）；需求进入 经验总结 及之后状态时，系统自动把仍处于排查中/已定位的关联线上问题推进到已修复。若需求尚未进入经验总结，优先先推进需求，不要手动改关联问题状态。</p><div className="react-inline-form"><input value={input} onChange={(e) => { setInput(e.target.value); setFeedback(null) }} placeholder="线上问题 req id（如 WMS-088-fix-xxx）" onKeyDown={(e) => { if (e.key === "Enter") add() }} /><button onClick={add} disabled={saving || !input.trim()}>{saving ? "保存中…" : "绑定"}</button></div>{bound.length ? <div className="react-card-meta">{boundReqs.map((issue) => <span key={issue.id} className="react-linked-issue-chip"><a href={`/requirement?id=${encodeURIComponent(issue.id)}`}>{issue.id}</a>{statusPill(issue.status)}<button type="button" className="react-copy-link-btn" onClick={() => remove(issue.id)} title="解除绑定">✕</button></span>)}{bound.filter((id) => !issues.some((r) => r.id === id)).map((id) => <span key={id} className="react-linked-issue-chip"><code>{id}</code><em className="react-muted">未找到</em><button type="button" className="react-copy-link-btn" onClick={() => remove(id)}>✕</button></span>)}</div> : <p className="react-muted">暂未绑定线上问题。</p>}{feedback ? <p className={feedback.startsWith("绑定失败") || feedback.startsWith("解绑失败") ? "react-effort-error" : "react-save-hint"}>{feedback}</p> : null}</section>
}

export function GroupMembersPanel({ req }: { req: Requirement }) {
  const members = req.groupMembers ?? []
  if (!members.length) return null
  const missing = members.filter((m) => !m.found).length
  const bottleneck = members.find((m) => m.reqId === req.groupBottleneck)
  return <section id="group-members" className="react-panel"><PanelHead kicker="Requirement Group" title="需求组成员" chip={<>{members.length} 成员 · {req.groupPolicy === "together" ? "整体发布" : "独立发布"}</>} /><p className="react-muted">引用式需求组（group.json）：成员是独立需求，各自维护需求文件、分支、session 和发布；组状态为派生值 = min(成员状态)，不能手动设置，推进最慢成员即推进组进度。session 绑定组时自动绑定所有成员，解绑同理。</p><div className="react-card-meta">{members.map((m) => <span key={m.reqId} className="react-linked-issue-chip" title={[m.title, m.note].filter(Boolean).join(" · ") || undefined}>{m.found ? <a href={`/requirement?id=${encodeURIComponent(m.reqId)}`}>{m.reqId}</a> : <code>{m.reqId}</code>}{m.status ? statusPill(m.status) : <em className="react-muted">未找到</em>}{m.nested ? <em className="react-muted" title="不支持组嵌套">嵌套组</em> : null}</span>)}</div>{bottleneck ? <p className="react-muted">当前瓶颈：<a href={`/requirement?id=${encodeURIComponent(bottleneck.reqId)}`}>{bottleneck.reqId}</a>{bottleneck.status ? <>（{bottleneck.status}）</> : null}，组进度 {req.groupStatus ?? "-"}</p> : null}{missing ? <p className="react-effort-error">{missing} 个失效引用：成员需求不存在（可能已删除或改名），请修正需求目录下的 group.json。</p> : null}</section>
}

/** ONES 登记弹窗：需求列表「未关联 ONES」徽章与详情页「需求信息」卡的 ONES 项共用。 */
export function OnesBindModal({ req, onClose, onSaved }: { req: Requirement; onClose: () => void; onSaved: () => void }) {
  const [ones, setOnes] = useState(req.ones || "")
  const [saving, setSaving] = useState(false)
  const [feedback, setFeedback] = useState<string | null>(null)
  const changed = ones.trim() !== (req.ones ?? "").trim()
  const submit = async () => {
    if (saving || !changed) return
    setSaving(true)
    setFeedback(null)
    try {
      await postForm("/api/requirement/ones", { reqId: req.id, ones })
      setFeedback(ones.trim() ? "保存成功" : "已清除关联")
      onSaved()
      onClose()
    } catch (err) {
      setFeedback(`保存失败：${err instanceof Error ? err.message : String(err)}`)
    } finally {
      setSaving(false)
    }
  }
  return <div className="react-modal-backdrop" onClick={onClose}><div className="react-modal react-ones-modal" onClick={(e) => e.stopPropagation()}><div className="react-modal-head"><div><span>ONES</span><h3>登记 ONES 任务关联</h3></div><button type="button" className="react-modal-close" onClick={onClose} title="关闭">✕</button></div><div className="react-modal-form"><p className="react-muted"><code>{req.id}</code> {req.title}</p><p className="react-muted">粘贴 ONES 网址、编号，或直接从 ONES 复制的整段文本（编号 + 标题 + 链接），会自动识别为可点击引用；留空保存可清除关联。</p><div className="react-inline-form"><input autoFocus value={ones} onChange={(e) => { setOnes(e.target.value); setFeedback(null) }} placeholder="ONES 网址 / 编号 / 带链接的复制文本" onKeyDown={(e) => { if (e.key === "Enter") submit() }} /><button onClick={submit} disabled={saving || !changed}>{saving ? "保存中…" : "保存"}</button>{req.ones ? <button type="button" onClick={() => { setOnes(""); }} disabled={saving}>清空输入</button> : null}</div>{feedback ? <p className={feedback.startsWith("保存失败") ? "react-effort-error" : "react-save-hint"}>{feedback}</p> : null}</div></div></div>
}

/** 需求信息卡字段编辑弹窗：类别 / 推动方 / 预计发版。 */
export type ReqEditField = "category" | "source" | "planRelease"

const FIELD_META: Record<ReqEditField, { title: string; kicker: string }> = {
  category: { title: "修改类别", kicker: "Category" },
  source: { title: "修改推动方", kicker: "Source" },
  planRelease: { title: "修改预计发版", kicker: "Plan Release" },
}

export function ReqFieldEditModal({ req, field, onClose, onSaved }: { req: Requirement; field: ReqEditField; onClose: () => void; onSaved: () => void }) {
  const initial = field === "category" ? (req.category || "需求") : field === "source" ? (req.source ?? "产品推动") : (req.planRelease && req.planRelease !== "unknown" ? req.planRelease : "")
  const [value, setValue] = useState(initial)
  const [saving, setSaving] = useState(false)
  const [feedback, setFeedback] = useState<string | null>(null)
  const changed = value.trim() !== initial.trim()
  const submit = async () => {
    if (saving || !changed) return
    setSaving(true)
    setFeedback(null)
    try {
      if (field === "category") await postForm("/api/requirement/category", { reqId: req.id, category: value })
      else if (field === "source") await postForm("/api/requirement/update", { reqId: req.id, source: value })
      else await postForm("/api/requirement/update", { reqId: req.id, planRelease: value.trim() || "unknown" })
      onSaved()
      onClose()
    } catch (err) {
      setFeedback(`保存失败：${err instanceof Error ? err.message : String(err)}`)
    } finally {
      setSaving(false)
    }
  }
  const clearRelease = async () => {
    if (saving) return
    setSaving(true)
    setFeedback(null)
    try {
      await postForm("/api/requirement/update", { reqId: req.id, planRelease: "unknown" })
      onSaved()
      onClose()
    } catch (err) {
      setFeedback(`清除失败：${err instanceof Error ? err.message : String(err)}`)
      setSaving(false)
    }
  }
  const meta = FIELD_META[field]
  return <div className="react-modal-backdrop" onClick={onClose}><div className="react-modal" onClick={(e) => e.stopPropagation()}><div className="react-modal-head"><div><span>{meta.kicker}</span><h3>{meta.title}</h3></div><button type="button" className="react-modal-close" onClick={onClose} title="关闭">✕</button></div><div className="react-modal-form"><p className="react-muted"><code>{req.id}</code> {req.title}</p>{field === "planRelease" ? <div className="react-inline-form"><input type="date" autoFocus value={value} onChange={(e) => { setValue(e.target.value); setFeedback(null) }} /></div> : <div className="react-inline-form"><select autoFocus value={value} onChange={(e) => { setValue(e.target.value); setFeedback(null) }}>{field === "category" ? REQ_CATEGORIES.map((c) => <option key={c} value={c}>{c}</option>) : REQ_SOURCES.map((src) => <option key={src} value={src}>{src}</option>)}</select></div>}{field === "source" ? <p className="react-muted">开发推动：进入测试中前必须先完成测试场景文档（test-scenario.md）。</p> : null}<div className="react-inline-form"><button onClick={submit} disabled={saving || !changed}>{saving ? "保存中…" : "保存"}</button>{field === "planRelease" ? <button type="button" onClick={clearRelease} disabled={saving || !initial}>清除预计发版</button> : null}</div>{feedback ? <p className={feedback.startsWith("保存失败") || feedback.startsWith("清除失败") ? "react-effort-error" : "react-save-hint"}>{feedback}</p> : null}</div></div></div>
}
