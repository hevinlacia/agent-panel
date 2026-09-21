import { Fragment, useState } from "react"
import type { Requirement, StatusFlowPayload, StatusFlowTransition } from "../../types"
import { postForm, useFetch } from "../../lib/api"
import { PanelHead } from "../../components/ui"

function statusFlowNodeState(i: number, currentIndex: number | null): "done" | "current" | "todo" {
  if (currentIndex == null || i > currentIndex) return "todo"
  return i === currentIndex ? "current" : "done"
}

function StatusFlowJunction({ reqId, transition }: { reqId: string; transition: StatusFlowTransition }) {
  const gates = transition?.gates || []
  return <div className="react-statusflow-link">
    <span className="react-statusflow-line" />
    {gates.length ? <div className="react-statusflow-gates">
      {gates.map((g) => <a key={g.id} className={`react-statusflow-gate is-${g.state}`} title={g.reason} href={`/requirement-gate?id=${encodeURIComponent(reqId)}&gate=${encodeURIComponent(g.id)}`}>
        <span className="react-statusflow-mark">{g.state === "passed" ? "✓" : g.state === "failed" ? "✗" : "○"}</span>
        <span className="react-statusflow-gate-label">{g.label}</span>
        <em>{g.state === "passed" ? "已通过" : g.state === "failed" ? "未通过" : "未校验"}</em>
      </a>)}</div> : null}
  </div>
}

function StatusFlowCard({ req, onSaved }: { req: Requirement; onSaved?: () => void }) {
  const flow = useFetch<StatusFlowPayload>(req.id ? `/api/requirement/status-flow?id=${encodeURIComponent(req.id)}` : null)
  const data = flow.data
  /** 需求组状态为派生值（min 成员状态），不能手动设置，节点不可点击。 */
  const isGroup = Boolean(req.groupMembers?.length)
  /** 待确认的目标状态：点击状态节点先弹确认，确认后 via=ui 强制修改（人工修改跳过状态门禁）。 */
  const [pending, setPending] = useState<string | null>(null)
  const [saving, setSaving] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const openConfirm = (status: string) => {
    if (isGroup || saving || status === req.status) return
    setError(null)
    setPending(status)
  }
  /** 待确认流转上的门禁配置（仅相邻流转挂门禁；多跳进入无门禁含义）。 */
  const pendingGates = pending && data
    ? data.transitions.find((t) => t.from === req.status && t.to === pending)?.gates ?? []
    : []
  const closeConfirm = () => {
    if (saving) return
    setPending(null)
    setError(null)
  }
  const confirmChange = async () => {
    if (!pending || saving) return
    setSaving(true)
    setError(null)
    try {
      await postForm("/api/requirement/status", { reqId: req.id, status: pending, via: "ui", note: "状态流转卡点击修改" })
      setPending(null)
      flow.refresh()
      onSaved?.()
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err))
    } finally {
      setSaving(false)
    }
  }
  return <section id="status-flow" className="react-panel react-statusflow-panel">
    <PanelHead kicker="Status Flow" title="状态流转与门禁" chip={data ? `当前 ${data.currentStatus}` : flow.loading ? "loading" : "-"} />
    <p className="react-muted">一长条展示需求全流程状态（已完成 / 当前 / 待推进）；相邻状态之间竖排该流转配置的门禁：✓ 已通过（流转时通过，或门禁材料当前满足）、✗ 未通过（会拦住 agent 自动推进）、○ 未校验（人工/系统流转且门禁材料当前不满足，悬停看原因）。点击具体门禁可跳转门禁验证详情页。<strong>点击任意非当前状态节点，弹窗确认后即强制修改状态（via=ui，跳过门禁）</strong>；需求组状态为派生值不可点击。</p>
    {flow.error ? <p className="react-effort-error">{flow.error}</p> : !data ? <p className="react-muted">加载中…</p> : <div className="react-statusflow-strip">
      {data.statuses.map((s, i) => <Fragment key={s}>
        {i > 0 ? <StatusFlowJunction reqId={req.id} transition={data.transitions[i - 1]} /> : null}
        <div
          className={`react-statusflow-node is-${statusFlowNodeState(i, data.currentIndex)}${!isGroup && s !== req.status ? " is-clickable" : ""}`}
          title={isGroup
            ? "需求组状态为派生值（min 成员状态），不能手动设置；请在成员需求上修改"
            : s === req.status ? "当前状态" : `点击修改状态为「${s}」（确认后强制修改，跳过门禁）`}
          onClick={() => openConfirm(s)}
        >
          <span className="react-statusflow-dot">{statusFlowNodeState(i, data.currentIndex) === "done" ? "✓" : ""}</span>
          <span>{s}</span>
          {statusFlowNodeState(i, data.currentIndex) === "current" ? <em>当前</em> : null}
        </div>
      </Fragment>)}</div>}
    {pending ? <div className="react-modal-overlay" onClick={closeConfirm}>
      <div className="react-modal-card" onClick={(e) => e.stopPropagation()}>
        <h3>确认修改需求状态？</h3>
        <p>需求 <strong>{req.id}</strong></p>
        <p>状态将从 <strong>{req.status}</strong> 修改为 <strong>{pending}</strong>；人工修改跳过状态门禁，操作会记入状态历史（via=ui）。</p>
        {pendingGates.length ? <p>该流转配置了门禁（{pendingGates.map((g) => g.label).join("、")}）：人工进入会记录为人工流转；只要门禁材料当前满足，卡片仍显示 ✓ 已通过。</p> : null}
        {error ? <p className="react-effort-error">{error}</p> : null}
        <div className="react-modal-actions">
          <button type="button" onClick={closeConfirm}>取消</button>
          <button type="button" onClick={confirmChange} disabled={saving}>{saving ? "保存中…" : "确认修改"}</button>
        </div>
      </div>
    </div> : null}
  </section>
}

export { StatusFlowCard }
