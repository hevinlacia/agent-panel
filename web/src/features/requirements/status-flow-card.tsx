import { Fragment } from "react"
import type { Requirement, StatusFlowPayload, StatusFlowTransition } from "../../types"
import { useFetch } from "../../lib/api"
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

export function StatusFlowCard({ req }: { req: Requirement }) {
  const flow = useFetch<StatusFlowPayload>(req.id ? `/api/requirement/status-flow?id=${encodeURIComponent(req.id)}` : null)
  const data = flow.data
  return <section id="status-flow" className="react-panel react-statusflow-panel">
    <PanelHead kicker="Status Flow" title="状态流转与门禁" chip={data ? `当前 ${data.currentStatus}` : flow.loading ? "loading" : "-"} />
    <p className="react-muted">一长条展示需求全流程状态（已完成 / 当前 / 待推进）；相邻状态之间竖排该流转配置的门禁：✓ 已通过、✗ 未通过（会拦住 agent 自动推进）、○ 未校验（人工在面板上改状态跳过门禁，悬停看原因）。点击具体门禁可跳转门禁验证详情页。门禁只在 agent 推进状态时强制校验，规则在 Settings 页配置。</p>
    {flow.error ? <p className="react-effort-error">{flow.error}</p> : !data ? <p className="react-muted">加载中…</p> : <div className="react-statusflow-strip">
      {data.statuses.map((s, i) => <Fragment key={s}>
        {i > 0 ? <StatusFlowJunction reqId={req.id} transition={data.transitions[i - 1]} /> : null}
        <div className={`react-statusflow-node is-${statusFlowNodeState(i, data.currentIndex)}`}>
          <span className="react-statusflow-dot">{statusFlowNodeState(i, data.currentIndex) === "done" ? "✓" : ""}</span>
          <span>{s}</span>
          {statusFlowNodeState(i, data.currentIndex) === "current" ? <em>当前</em> : null}
        </div>
      </Fragment>)}</div>}
  </section>
}
