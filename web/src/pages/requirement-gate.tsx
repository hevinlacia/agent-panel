import { ArrowLeft, RefreshCw, ShieldCheck } from "lucide-react"
import type { StatusGateDetailPayload } from "../types"
import { useFetch } from "../lib/api"
import { EmptyCard, ErrorCard, LoadingCard, PageChrome, PanelHead } from "../components/ui"
import { formatDateTime } from "../lib/format"

const STATE_TEXT: Record<string, string> = { passed: "已通过", failed: "未通过", unverified: "未校验" }
const STATE_MARK: Record<string, string> = { passed: "✓", failed: "✗", unverified: "○" }

function GateStateBanner({ state, label, reason, checkedAt }: { state: string; label: string; reason: string; checkedAt: number }) {
  return <div className={`react-statusflow-gate is-${state} react-gate-detail-state`}>
    <span className="react-statusflow-mark">{STATE_MARK[state] || "○"}</span>
    <strong>{label}</strong>
    <em>{STATE_TEXT[state] || state}</em>
    <span className="react-gate-detail-time">{formatDateTime(checkedAt)}</span>
  </div>
}

function ReviewGateDetail({ detail }: { detail: NonNullable<StatusGateDetailPayload["detail"]> }) {
  const staleRepos = detail.staleRepos || []
  return <>
    <section className="react-panel"><PanelHead kicker="Review Gate" title="代码审查门禁详情" chip={detail.label} />
      <div className="react-meta-grid">
        <span>审查结论 <code>{detail.label || "-"}</code></span>
        <span>结论来源 <code>{detail.source || "-"}</code></span>
        <span>review.md <code>{detail.reviewPath || "-"}</code></span>
        <span>code-review-ai.md <code>{detail.aiReviewPath || "-"}</code></span>
      </div>
      <p className="react-muted">门禁判定：{detail.reason || "-"}</p>
      {detail.riskTags?.length ? <div className="react-review-risk-tags"><strong>风险标签</strong>{detail.riskTags.map((tag) => <span key={tag} className="react-review-tag">{tag}</span>)}</div> : null}
      {detail.inventoryRisk ? <div className="react-review-gate react-review-gate-blocked"><strong>⚠ 库存高危风险</strong><span>本次改动命中库存相关文件/表，门禁强制要求库存账本专项评估：单据活跃/死亡、DB 库存(onHand/allocated/临时库位/回库单)、redis 可用量(建单-、真取消+、恢复-、回退保持占用)、重复释放、遗漏占用、幂等、验证证据(DB/redis/日志/单测)。未补充前即使 PASS 也不通过。</span></div> : null}
      {detail.checklist ? (() => {
        const c = detail.checklist
        if (!c.present) return <div className="react-drive-blockers"><strong>审查清单缺失</strong><p>{c.error || "review-checklist.json 不存在：门禁要求审查清单列出的每一项都有结论（pass/fail/na）才放行。用 PUT /api/requirement/review-checklist 写入清单。"}</p></div>
        const mark = (v?: string) => v === "pass" ? "✅ pass" : v === "fail" ? "❌ fail" : "➖ na"
        return <div className="react-drive-blockers"><strong>审查清单（{c.total} 项{c.failed ? `，${c.failed} 项 fail` : "，全部有结论"}）</strong><ul>{(c.items || []).map((item) => <li key={item.id}><code>{item.id}</code> {item.title} —— <strong>{mark(item.conclusion)}</strong>{item.note ? <span className="react-muted">（{item.note}）</span> : null}{item.evidence ? <span className="react-muted"> 证据：{item.evidence}</span> : null}</li>)}</ul></div>
      })() : null}
      {staleRepos.length ? <div className="react-drive-blockers"><strong>审查快照需刷新覆盖</strong><ul>{staleRepos.map((repo) => <li key={`${repo.repoName}-${repo.branch}`}><code>{repo.repoName}</code> / <code>{repo.branch}</code>：{(repo.reviewedTargetCommit || "").slice(0, 12) || "reviewed?"} → {(repo.currentTargetCommit || "").slice(0, 12) || "current?"}</li>)}</ul><p>优先生成增量审查包，只审上次已审 commit 到当前 HEAD 的新增 diff；非线性历史再回退全量审查。</p></div> : null}
      {detail.actions?.length ? <div className="react-drive-blockers"><strong>门禁动作</strong><ul>{detail.actions.map((item) => <li key={item}>{item}</li>)}</ul></div> : null}
      <p className="react-muted">生成/刷新差异与审查材料请到需求详情页「代码差异」卡片操作。</p>
    </section>
  </>
}

function SelftestGateDetail({ detail }: { detail: NonNullable<StatusGateDetailPayload["detail"]> }) {
  const problems = detail.problems || []
  return <section className="react-panel"><PanelHead kicker="Selftest Checklist" title="自测清单校验详情" />
    <p className="react-muted">校验目标：test.md「## 自测清单」表格——列出测试项目且每项有结果（通过/失败/无法测试），失败或无法测试的项必须写明具体原因。</p>
    {detail.hotfix ? <p className="react-save-hint">抢修模式（已关联线上问题）：自测门禁默认放行，速度优先；用户明确要求自测时再补 test.md 自测清单。</p> : null}
    {problems.length ? <div className="react-drive-blockers"><strong>当前问题（{problems.length}）</strong><ul>{problems.map((p, i) => <li key={i}>{p}</li>)}</ul></div> : <p className="react-save-hint">✓ 自测清单校验通过：每项都有测试结果，未通过项均附具体原因。</p>}
    <details className="react-review-repo"><summary><span><strong>期望格式</strong></span></summary><pre className="react-gate-detail-pre">{`| # | 自测项 | 结果 | 失败/无法测试原因 |
| --- | --- | --- | --- |
| 1 | 场景名 | 通过 | - |
| 2 | 场景名 | 无法测试 | test 环境 OMS 未订阅 topic，无法联调 |`}</pre></details>
  </section>
}

function DocGateDetail({ detail, title, file }: { detail: NonNullable<StatusGateDetailPayload["detail"]>; title: string; file: string }) {
  return <section className="react-panel"><PanelHead kicker="Doc Gate" title={title} />
    <div className="react-meta-grid">
      <span>适用范围 <code>{detail.applicable ? "生效" : "不生效（当前需求不适用此门禁，恒通过）"}</code></span>
      <span>类别 <code>{detail.category || "-"}</code></span>
    </div>
    {"rootCauseFilled" in detail ? <p className="react-muted">root-cause.md 已填写：{detail.rootCauseFilled ? "是" : "否"}；technical-plan.md 存量兼容：{detail.legacyPlanFilled ? "已填写" : "未填写"}</p> : null}
    {"filled" in detail && !("rootCauseFilled" in detail) ? <p className="react-muted">{file} 已填写：{detail.filled ? "是" : "否"}</p> : null}
    <p className="react-muted">未通过时请先在需求详情页补齐对应文档（至少一条非「待补充」记录），再由 agent 重新推进状态。</p>
  </section>
}

export function RequirementGatePage() {
  const params = new URLSearchParams(window.location.search)
  const id = params.get("id") || params.get("reqId") || ""
  const gateId = params.get("gate") || ""
  const detail = useFetch<StatusGateDetailPayload>(id && gateId ? `/api/requirement/status-gate-detail?id=${encodeURIComponent(id)}&gate=${encodeURIComponent(gateId)}` : null)
  const d = detail.data
  return <PageChrome icon={<ShieldCheck size={15} />} eyebrow="Gate" title={d?.label || gateId || "门禁详情"} description={d?.description || "门禁验证详情：当前校验状态、未通过原因与处理指引。"}
    actions={<><a href={`/requirement?id=${encodeURIComponent(id)}`}><ArrowLeft size={15} />返回需求</a><button onClick={detail.refresh} disabled={detail.loading}><RefreshCw size={15} className={detail.loading ? "react-spin" : ""} />刷新</button></>}>
    {detail.error ? <ErrorCard error={detail.error} /> : detail.loading ? <LoadingCard label="正在加载门禁校验…" /> : !d ? <EmptyCard>缺少 id 或 gate 参数：<code>/requirement-gate?id=&lt;reqId&gt;&amp;gate=&lt;gateId&gt;</code></EmptyCard> : <>
      <section className="react-panel"><PanelHead kicker="Gate Check" title="当前校验结果" chip={`agent 推进时${d.state === "failed" ? "会被拦截" : "放行"}`} />
        <GateStateBanner state={d.state} label={d.label} reason={d.reason} checkedAt={d.checkedAt} />
        <p className="react-muted">{d.reason}</p>
      </section>
      {d.gate === "review" && d.detail ? <ReviewGateDetail detail={d.detail} /> : null}
      {d.gate === "selftest-checklist" && d.detail ? <SelftestGateDetail detail={d.detail} /> : null}
      {d.gate === "test-scenario" && d.detail ? <DocGateDetail detail={d.detail} title="测试场景文档校验" file="test-scenario.md" /> : null}
      {d.gate === "issue-root-cause" && d.detail ? <DocGateDetail detail={d.detail} title="线上问题定位校验" file="root-cause.md" /> : null}
      {d.gate === "issue-troubleshooting" && d.detail ? <DocGateDetail detail={d.detail} title="线上问题复盘校验" file="troubleshooting.md" /> : null}
    </>}
  </PageChrome>
}
