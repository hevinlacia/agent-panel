import { Fragment, useState } from "react"
import type { Requirement, StatusFlowPayload, StatusFlowTransition } from "../../types"
import { postForm, useFetch } from "../../lib/api"
import { PanelHead } from "../../components/ui"
import { SUSPEND_STATUS } from "../../lib/requirements"

function statusFlowNodeState(i: number, currentIndex: number | null): "done" | "current" | "todo" {
  if (currentIndex == null || i > currentIndex) return "todo"
  return i === currentIndex ? "current" : "done"
}

function StatusFlowJunction({ reqId, transition }: { reqId: string; transition: StatusFlowTransition }) {
  const gates = transition?.gates || []
  return <div className="react-statusflow-link">
    <span className="react-statusflow-line" />
    {gates.length ? <div className="react-statusflow-gates">
      {gates.map((g) => {
        const warned = Boolean(g.warnings?.length)
        return <a key={g.id} className={`react-statusflow-gate is-${g.state}${warned ? " is-warning" : ""}`} title={warned ? `${g.reason}\n⚠ ${g.warnings!.join("\n⚠ ")}` : g.reason} href={`/requirement-gate?id=${encodeURIComponent(reqId)}&gate=${encodeURIComponent(g.id)}`}>
          <span className="react-statusflow-mark">{g.state === "passed" ? (warned ? "⚠" : "✓") : g.state === "failed" ? "✗" : "○"}</span>
          <span className="react-statusflow-gate-label">{g.label}</span>
          <em>{g.state === "passed" ? (warned ? "已通过 · 有警示" : "已通过") : g.state === "failed" ? "未通过" : "未校验"}</em>
        </a>
      })}</div> : null}
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
  const pendingGates = pending && data && pending !== SUSPEND_STATUS
    ? data.transitions.find((t) => t.from === req.status && t.to === pending)?.gates ?? []
    : []
  /** 打回返工轮次（loops 明细优先，向后兼容 rounds 字段）。 */
  const reworkRounds = data?.rework ? (data.rework.loops?.rework?.rounds ?? data.rework.rounds) : 0
  /** 发布就绪小循环轮次。 */
  const fastFixRounds = data?.rework?.releaseReadyRounds ?? 0
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
      await postForm("/api/requirement/status", { reqId: req.id, status: pending, via: "ui", note: pending === SUSPEND_STATUS ? "状态流转卡点击挂起" : req.status === SUSPEND_STATUS ? "状态流转卡取消挂起" : "状态流转卡点击修改" })
      setPending(null)
      flow.refresh()
      onSaved?.()
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err))
    } finally {
      setSaving(false)
    }
  }
  const suspended = Boolean(data?.suspended) || req.status === SUSPEND_STATUS
  const resumeStatus = data?.resumeStatus || null
  /** 子需求走独立轻量状态机（后端拒绝挂起）；需求组状态为派生值不可改。 */
  const canSuspend = !isGroup && !req.isSubReq
  return <section id="status-flow" className="react-panel react-statusflow-panel">
    <PanelHead kicker="Status Flow" title="状态流转与门禁" chip={data ? `当前 ${data.currentStatus}` : flow.loading ? "loading" : "-"} />
    <p className="react-muted">一长条展示需求全流程状态（已完成 / 当前 / 待推进）；相邻状态之间竖排该流转配置的门禁：✓ 已通过（流转时通过，或门禁材料当前满足）、✗ 未通过（会拦住 agent 自动推进）、○ 未校验（人工/系统流转且门禁材料当前不满足，悬停看原因）。点击具体门禁可跳转门禁验证详情页。<strong>点击任意非当前状态节点，弹窗确认后即强制修改状态（via=ui，跳过门禁）</strong>；需求组状态为派生值不可点击。「挂起」是流水外标记状态：任何状态都可挂起，不影响流水位置，适合需求暂时不做的场景；再次点击按钮即取消挂起，回到挂起前状态；列表页可按挂起筛选。</p>
    {canSuspend ? <div className="react-statusflow-suspend-actions">
      {suspended
        ? <button type="button" className="react-toggle-btn is-suspending" disabled={saving || !resumeStatus} onClick={() => resumeStatus && openConfirm(resumeStatus)} title={resumeStatus ? `取消挂起，恢复到挂起前状态「${resumeStatus}」（确认后强制修改，跳过门禁）` : "状态历史缺失，无法自动恢复；请在上方状态条点击目标状态手动恢复"}>▶ 取消挂起（恢复到 {resumeStatus ?? "…"}）</button>
        : <button type="button" className="react-toggle-btn" disabled={saving} onClick={() => openConfirm(SUSPEND_STATUS)} title="挂起是流水外标记状态：任何状态可挂起，不影响流水位置，恢复时回到挂起前状态；列表页可按挂起筛选">⏸ 挂起需求</button>}
    </div> : null}
    {flow.error ? <p className="react-effort-error">{flow.error}</p> : !data ? <p className="react-muted">加载中…</p> : <>
      <div className="react-statusflow-strip">
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
      </Fragment>)}
      </div>
      {data.rework ? <div className="react-statusflow-rework">
        <span className="react-statusflow-rework-loop">↺ 回到开发中的循环</span>
        <span>打回返工：{(data.rework.loops?.rework?.fromStatuses ?? data.rework.fromStatuses.filter((s) => s !== "发布就绪")).join(" / ") || "人工核查 / 测试中"} → 开发中</span>
        {reworkRounds > 0
          ? <em className={`react-statusflow-rework-count${reworkRounds >= 3 ? " is-high" : ""}`}>已打回 {reworkRounds} 次（R{reworkRounds}）</em>
          : <em>尚未发生打回</em>}
        <span>发布就绪小循环：发布就绪 → 开发中</span>
        {fastFixRounds > 0
          ? <em className="react-statusflow-rework-count">已进行 {fastFixRounds} 轮（F{fastFixRounds}）</em>
          : <em>尚未发生</em>}
        <span className="react-statusflow-rework-hint react-muted">打回是测试/产品的人工判断：点击「开发中」节点确认后即回退（via=ui），回退计入返工轮次；agent 会加载返工变体提示词做增量修复，重新走 自测 → 测试 门禁链路。打回≥3 次建议回头补需求澄清而非继续磨。从「发布就绪」点击「开发中」= 进入上线前快速小循环：小改动、默认免单测、只部署 UAT（CN+SEA 成对）并在 UAT 测试，agent 加载快速修复变体提示词，增量审查通过后直通发布就绪（门禁记录见门禁详情页的循环审查记录）。</span>
      </div> : null}
    </>}
    {pending ? <div className="react-modal-overlay" onClick={closeConfirm}>
      <div className="react-modal-card" onClick={(e) => e.stopPropagation()}>
        <h3>{pending === SUSPEND_STATUS ? "确认挂起需求？" : req.status === SUSPEND_STATUS ? "确认取消挂起？" : "确认修改需求状态？"}</h3>
        <p>需求 <strong>{req.id}</strong></p>
        {pending === SUSPEND_STATUS
          ? <p>状态将从 <strong>{req.status}</strong> 标记为 <strong>挂起</strong>；挂起是流水外标记（任何状态可挂起），不影响流水进度，再次点击按钮即取消挂起并回到挂起前状态。操作会记入状态历史（via=ui）。</p>
          : req.status === SUSPEND_STATUS
            ? <p>将取消挂起，状态从 <strong>挂起</strong> 恢复为 <strong>{pending}</strong>（挂起前的流水状态或你选择的目标状态）；人工修改跳过状态门禁，操作会记入状态历史（via=ui）。</p>
            : <p>状态将从 <strong>{req.status}</strong> 修改为 <strong>{pending}</strong>；人工修改跳过状态门禁，操作会记入状态历史（via=ui）。</p>}
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
