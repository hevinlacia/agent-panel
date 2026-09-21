import { AlertTriangle, ArrowLeft, Copy, FileCode2, GitBranch, Library, Lightbulb, List, Paperclip, RefreshCw } from "lucide-react"
import { useMemo, useState } from "react"
import type { HarnessCurrent, NewSessionPayload, PendingSessionPayload, SessionInfo } from "../types"
import { postForm, postJson, useFetch } from "../lib/api"
import { copyRequirementSessionCommand } from "../features/requirements/session-command"
import { formatDate, parseOnesRef, relAge } from "../lib/format"
import { effectiveStatus, experienceSummaryPill, groupBadge, onesBadge, projectsOf, statusPill } from "../features/requirements/badges"
import { CodeReviewPanel } from "../features/requirements/code-review-panel"
import { MergeBranchPanel, ProdMrPanel } from "../features/requirements/branch-ops-panels"
import { RequirementDocPanel, RequirementPrdPanel } from "../features/requirements/req-doc-panels"
import { GroupMembersPanel, LinkedIssuesPanel, OnesBindModal, ReqFieldEditModal, type ReqEditField } from "../features/requirements/req-meta-panels"
import { RequirementAttachmentsPanel, RequirementFilesPanel } from "../features/requirements/req-files-panels"
import { StatusFlowCard } from "../features/requirements/status-flow-card"
import { SubBranchPanel, SubRequirementsPanel } from "../features/requirements/sub-req-panels"
import { EmptyCard, ErrorCard, LoadingCard, PageChrome, PanelHead } from "../components/ui"
import { RequirementsData } from "./projects"
import { SessionListModal } from "./sessions"

export function RequirementPage() {
  const id = new URLSearchParams(window.location.search).get("id") || new URLSearchParams(window.location.search).get("reqId") || ""
  const { data, error, loading, refresh } = RequirementsData()
  const req = data?.requirements.find((r) => r.id === id)
  const harness = useFetch<HarnessCurrent>("/api/harness/current")
  const curHarness = harness.data?.harness ?? "pi"
  const isDsh = curHarness === "dsh-web" || curHarness === "dsh-tui"
  const isDshWeb = curHarness === "dsh-web"
  const resolved = useFetch<{ sessions: SessionInfo[]; missing: string[] }>(req?.sessionIds?.length ? `/api/sessions/resolve?ids=${encodeURIComponent(req.sessionIds.join(","))}` : null, [req?.id])
  /** 引用式需求组：状态为派生值，不能手动设置，展示聚合状态。 */
  const isGroup = Boolean(req?.groupMembers?.length)
  /** In dsh mode only dsh sessions are shown; resolve returns each session's harness via `agent`. */
  const dshSessionIds = useMemo(() => {
    if (!isDsh || !req?.sessionIds?.length) return req?.sessionIds ?? []
    const agents = new Map((resolved.data?.sessions ?? []).map((s) => [s.id.replace(/^session-/, ""), s.agent]))
    return req.sessionIds.filter((sid) => agents.get(sid.replace(/^session-/, "")) === "dsh")
  }, [curHarness, req?.sessionIds, resolved.data])
  const [summaryWorking, setSummaryWorking] = useState(false)
  const [statusMessage, setStatusMessage] = useState<string | null>(null)
  const [command, setCommand] = useState("")
  const [copied, setCopied] = useState(false)
  /** 主按钮反馈：复制命令 / 强制刷新刚完成。 */
  const [copyFeedback, setCopyFeedback] = useState<"" | "copied" | "refreshed">("")
  const [copyError, setCopyError] = useState<string | null>(null)
  const [bindCmdCopied, setBindCmdCopied] = useState(false)
  /** dsh: the just-created session (appears in the dsh web GUI at `url`). */
  const [dshSession, setDshSession] = useState<{ sessionId: string; url: string } | null>(null)
  const [showSessions, setShowSessions] = useState(false)
  /** pi/dsh-tui：需求当前待使用的启动命令（无副作用读取，用于页面展示）。 */
  const pendingSession = useFetch<PendingSessionPayload>(req && !isDshWeb ? `/api/requirement/pending-session?id=${encodeURIComponent(req.id)}` : null, [req?.id, curHarness])
  /** 展示用命令：复制后优先展示新命令，否则展示服务端 pending 命令。 */
  const shownCommand = command || pendingSession.data?.pending?.command || ""
  const isIssueFamily = req?.category === "线上问题" || req?.category === "测试问题"
  const isOnlineIssue = req?.category === "线上问题"
  /** 子需求：独立轻量状态机（需求创建→开发中→已合入/已取消），无门禁。 */
  const isSub = Boolean(req?.isSubReq)
  /** ONES 关联：需求信息卡展示 + 弹窗登记/修改/清除。 */
  const onesRef = parseOnesRef(req?.ones)
  const [onesModal, setOnesModal] = useState(false)
  const [editField, setEditField] = useState<ReqEditField | null>(null)
  const [fixReqId, setFixReqId] = useState<string | null>(null)
  const convertIssue = async () => {
    if (!req || req.category !== "线上问题") return
    try {
      const res = await postJson<{ ok: boolean; fixReqId?: string }>("/api/requirement/convert-issue", { reqId: req.id, note: "线上问题转代码修复需求" })
      setFixReqId(res.fixReqId || null)
      setStatusMessage(res.fixReqId ? `已创建代码修复需求 ${res.fixReqId} 并绑定本问题；需求进入经验总结后自动推进本问题到已修复` : "已转普通需求流程")
      refresh()
    } catch (err) {
      setStatusMessage(`转换失败：${err instanceof Error ? err.message : String(err)}`)
    }
  }
  /** dsh-web 专用：直接创建并绑定一个 dsh web session（无粘贴命令形态）。 */
  const newDshWebSession = async () => {
    if (!req) return
    const res = await postForm<NewSessionPayload>("/api/requirement/new-session", { reqId: req.id })
    setDshSession(res.sessionId ? { sessionId: res.sessionId, url: res.url || "" } : null)
  }
  /** pi/dsh-tui：复制最新命令。复用未使用的 pending session id；force 强制换新。 */
  const copyLatestCommand = async (force: boolean) => {
    if (!req) return
    setCopyError(null)
    try {
      const res = await copyRequirementSessionCommand(req.id, { force })
      setCommand(res.command)
      setDshSession(null)
      setCopyFeedback(force ? "refreshed" : "copied")
      pendingSession.refresh()
      window.setTimeout(() => setCopyFeedback(""), 2000)
    } catch (err) {
      setCopyError(`复制命令失败：${err instanceof Error ? err.message : String(err)}`)
    }
  }
  /** 复制当前展示的命令文本（不重新请求，纯剪贴板操作）。 */
  const copyShownCommand = async () => {
    if (!shownCommand) return
    try {
      await navigator.clipboard.writeText(shownCommand)
      setCopied(true)
      window.setTimeout(() => setCopied(false), 1600)
    } catch (err) {
      setCopied(false)
      // eslint-disable-next-line no-console
      console.error("复制失败", err)
    }
  }
  const retrySummary = async () => {
    if (!req || summaryWorking) return
    setSummaryWorking(true)
    setStatusMessage(null)
    try {
      await postJson("/api/experience-summary/jobs/retry", { reqId: req.id, note: "需求详情页手动派发经验总结" })
      setStatusMessage("自动经验总结已派发")
      refresh()
    } catch (err) {
      setStatusMessage(`派发失败：${err instanceof Error ? err.message : String(err)}`)
    } finally {
      setSummaryWorking(false)
    }
  }
  const copyBindCommand = async () => {
    if (!req) return
    try {
      await navigator.clipboard.writeText(`/requirement-bind ${req.id}`)
      setBindCmdCopied(true)
      window.setTimeout(() => setBindCmdCopied(false), 1600)
    } catch (err) {
      setBindCmdCopied(false)
      // eslint-disable-next-line no-console
      console.error("复制失败", err)
    }
  }
  return <PageChrome icon={<GitBranch size={15} />} eyebrow="Requirement" title={req?.title || id || "Requirement"} description={req?.description || "需求详情、状态流转、技术方案、上线清单、业务背景、经验总结与关联 session。"} actions={<><a href="/projects"><ArrowLeft size={15} />返回需求列表</a>{req ? <a href="#technical-plan"><FileCode2 size={15} />技术方案</a> : null}{req && !isOnlineIssue && !isSub ? <a href="#release-manifest"><AlertTriangle size={15} />上线清单</a> : null}{req ? <a href="#attachments"><Paperclip size={15} />附件</a> : null}{req ? <a href={`/requirement-doc?id=${encodeURIComponent(req.id)}&doc=background&title=${encodeURIComponent("业务背景文档")}`}><Library size={15} />业务背景</a> : null}{req ? <a href="#experience-summary"><Lightbulb size={15} />经验总结</a> : null}{req && !isOnlineIssue ? <a href="#code-review"><GitBranch size={15} />代码差异</a> : null}</>}>
    {error ? <ErrorCard error={error} /> : loading ? <LoadingCard /> : !req ? <EmptyCard>需求不存在：{id}</EmptyCard> : <div className="react-detail-grid">
      <section className="react-panel"><PanelHead kicker="Overview" title="需求信息" chip={<>{statusPill(effectiveStatus(req))}{groupBadge(req)}{experienceSummaryPill(req)}</>} /><div className="react-meta-grid"><span>Req ID <code>{req.id}</code></span><span>项目 {projectsOf(req)}</span><span>创建 {formatDate(req.createdAt)}</span><span>更新 {relAge(req.updatedAt)}</span><span><strong className={!isSub ? "react-field-edit" : undefined} onClick={!isSub ? () => setEditField("planRelease") : undefined} title={!isSub ? "点击修改预计发版" : undefined}>预计发版</strong> {req.planRelease || "unknown"}</span><span>目录 {req.reqDir || "-"}</span><span><strong className={!isSub ? "react-field-edit" : undefined} onClick={!isSub ? () => setEditField("category") : undefined} title={!isSub ? "点击修改类别" : undefined}>类别</strong> {req.category || "需求"}</span><span><strong className="react-field-edit" onClick={() => setEditField("source")} title="点击修改推动方">来源</strong> {req.source ?? "产品推动"}{req.source === "开发推动" ? "（需测试场景文档）" : ""}</span>{!isSub ? <span><strong className="react-field-edit" onClick={() => setOnesModal(true)} title="点击登记 / 修改 / 清除 ONES 关联">ONES</strong> {onesRef ? (onesRef.url ? <a href={onesRef.url} target="_blank" rel="noopener noreferrer" title={onesRef.raw}>{onesRef.label}</a> : <code title={onesRef.raw}>{onesRef.label}</code>) : <em className="react-muted">未关联</em>}</span> : null}{req.memberOf?.length ? <span>所属组 {req.memberOf.map((g) => <a key={g} href={`/requirement?id=${encodeURIComponent(g)}`}>{g}</a>)}</span> : null}{req.isSubReq && req.parentReqId ? <span>父需求 <a href={`/requirement?id=${encodeURIComponent(req.parentReqId)}`}>{req.parentReqId}</a>（并行执行单元，合回父分支）</span> : null}</div>{isOnlineIssue ? <div className="react-inline-form"><button type="button" onClick={convertIssue} title="正式生产修复创建普通需求并绑定本问题">创建修复需求</button>{statusMessage ? <p className={statusMessage.startsWith("转换失败") ? "react-effort-error" : "react-save-hint"}>{statusMessage}{fixReqId ? <> <a href={`/requirement?id=${encodeURIComponent(fixReqId)}`}>打开修复需求 →</a></> : null}</p> : null}</div> : null}<p className="react-detail-desc">{req.description || "暂无描述"}</p></section>
      <StatusFlowCard req={req} onSaved={refresh} />
      <section className="react-panel react-sessions-panel"><PanelHead kicker="Sessions" title="关联 Session" chip={String(isDsh ? dshSessionIds.length : (req.sessionIds?.length ?? 0))} /><p className="react-muted">本需求绑定的终端 session；卡片不再平铺历史 session，点击「查看全部」弹窗展示完整列表，可一键复制 session id。</p><div className="react-actions">{(isDsh ? dshSessionIds.length : (req.sessionIds?.length ?? 0)) ? <button type="button" onClick={() => setShowSessions(true)} title="弹窗查看全部关联 session，可一键复制 session id"><List size={13} />查看全部 session（{isDsh ? dshSessionIds.length : (req.sessionIds?.length ?? 0)}）</button> : null}{isDshWeb ? <button onClick={newDshWebSession}>为需求开启 dsh web session</button> : <><button type="button" className="react-ghost-btn" onClick={() => copyLatestCommand(false)} title="复制最新终端命令；session 被使用后再次点击会自动换新 session id"><Copy size={13} />{copyFeedback === "copied" ? "已复制" : "复制命令"}</button><button type="button" className="react-ghost-btn" onClick={() => copyLatestCommand(true)} title="无视使用状态强制更换 session id，并复制新命令；用于“是否已使用”判断失误时兜底"><RefreshCw size={13} />{copyFeedback === "refreshed" ? "已刷新并复制" : "强制刷新"}</button></>}</div>{isDshWeb ? <div className="react-command-wrap"><code className="react-command">/requirement-bind {req.id}</code><button type="button" className="react-copy-link-btn" onClick={copyBindCommand} title="复制绑定命令，到 dsh web 聊天框粘贴"><Copy size={13} />{bindCmdCopied ? "已复制" : "复制"}</button></div> : shownCommand ? <><div className="react-command-wrap"><code className="react-command">{shownCommand}</code><button type="button" className="react-copy-link-btn" onClick={copyShownCommand} title="再次复制当前显示的命令"><Copy size={13} />{copied ? "已复制" : "复制"}</button></div><p className="react-muted">{pendingSession.data?.pending ? (pendingSession.data.pending.used ? "该命令的 session 已被使用：再次点击“复制命令”会自动换新 session id。" : "重复点击“复制命令”复用同一 session id；命令被使用后自动换新。判断失误时用“强制刷新”。") : "尚未生成命令：点击“复制命令”会生成并复制；命令被使用后再次点击会自动换新。"}</p></> : null}{!isDshWeb && copyError ? <p className="react-effort-error">{copyError}</p> : null}{isDshWeb ? dshSession ? <div className="react-dsh-session"><p className="react-save-hint">已在 dsh web（{dshSession.url || "3080"}）创建并绑定 session：<code>{dshSession.sessionId}</code></p>{dshSession.url ? <a className="react-link" href={dshSession.url} target="_blank" rel="noreferrer">打开 dsh web GUI 继续会话 ↗</a> : null}<p className="react-muted">需求上下文会在下一条消息注入该 session；也可复制上面的绑定命令到任一 dsh session 聊天框。</p></div> : <p className="react-muted">复制上面的绑定命令，粘贴到 dsh web 聊天框（任意 session），即可把该 session 关联到本需求。</p> : null}{showSessions && (isDsh ? dshSessionIds.length : req.sessionIds?.length) ? <SessionListModal sessionIds={isDsh ? dshSessionIds : req.sessionIds} harness={curHarness} onClose={() => setShowSessions(false)} /> : null}</section>
      {!isOnlineIssue ? <CodeReviewPanel req={req} /> : null}
      {req.prdPath ? <RequirementPrdPanel req={req} /> : null}
      <RequirementDocPanel id="technical-plan" req={req} docType="technical-plan" title="技术方案" kicker="Implementation Plan" path={req.technicalPlanPath} jumpOnly description="Agent 执行需求过程中持续维护：先看总体实现路径、影响范围、风险、灰度/回滚和验证计划，再进入代码差异人工审查。卡片不再内嵌概要，点击「方案上下文」在独立页面查看全文，页面左上角可返回需求详情。" actions={<><a href={`/requirement-doc?id=${encodeURIComponent(req.id)}&doc=technical-plan`}><Library size={15} />方案上下文</a></>} />
      {req.source === "开发推动" && !isOnlineIssue && !isSub ? <RequirementDocPanel id="test-scenario" req={req} docType="test-scenario" title="测试场景" kicker="Test Scenario" path={req.testScenarioPath} description="开发推动的需求，测试无法向产品确认测试范围，本档由开发负责：需求说明（这个需求是干嘛的）+ 开发评估的测试范围 + 测试覆盖场景，让测试自主评估与补充用例；进入「测试中」前强制校验。" actions={<><a href={`/requirement-doc?id=${encodeURIComponent(req.id)}&doc=test-scenario&title=${encodeURIComponent("测试场景文档")}`}><Library size={15} />整页查看</a><a href={`/api/requirement/context?id=${encodeURIComponent(req.id)}&intent=design&tokens=req.technicalPlan,req.branchScope&budget=4000&format=html`} target="_blank" rel="noreferrer">方案上下文</a></>} /> : null}
      {!isOnlineIssue && !isSub ? <RequirementDocPanel id="release-manifest" req={req} docType="release-manifest" title="上线清单" kicker="Release Manifest" path={req.releaseManifestPath} jumpOnly description="贯穿需求全流程维护：集中展示 DB 表、配置、Topic/Group、Job、开关、接口和上线人工动作，避免发布时遗漏。卡片不再内嵌摘要/详情，点击「整页查看」在独立页面查看全文。" actions={<><a href={`/requirement-doc?id=${encodeURIComponent(req.id)}&doc=release-manifest&title=${encodeURIComponent("上线清单")}`}><Library size={15} />整页查看</a><a href={`/api/requirement/context?id=${encodeURIComponent(req.id)}&intent=release-check&tokens=req.releaseManifest,req.attachments,req.configChanges,req.branchScope&budget=5000&format=html`} target="_blank" rel="noreferrer">清单上下文</a></>} /> : null}
      <RequirementAttachmentsPanel req={req} />
      <section id="business-background" className="react-panel react-doc-panel"><PanelHead kicker="Business Context" title="业务背景文档" chip="新页面查看" /><p className="react-muted">给不熟悉业务的开发/测试快速理解背景，也作为后续经验总结的参考材料。点击下方按钮在独立页面查看渲染后的完整文档。</p><div className="react-actions"><a href={`/requirement-doc?id=${encodeURIComponent(req.id)}&doc=background&title=${encodeURIComponent("业务背景文档")}`}><Library size={15} />查看业务背景文档</a><a href="/business-knowledge"><Library size={15} />业务知识库</a><a href={`/api/requirement/context?id=${encodeURIComponent(req.id)}&intent=clarification&budget=3000&format=html`} target="_blank" rel="noreferrer">澄清上下文</a></div></section>
      {!isSub ? <RequirementDocPanel id="experience-summary" req={req} docType="experience-summary" title="经验总结闭环" kicker="Capability Evolution" path={req.experienceSummaryPath} description="记录本次需求暴露出的业务知识、经验、skill 和流程改进，让下一次需求执行更快更稳。" actions={<><a href="/experiences"><Lightbulb size={15} />需求总结</a><a href={`/requirement-doc?id=${encodeURIComponent(req.id)}&doc=experience-summary&title=${encodeURIComponent("经验总结报告")}`}><Library size={15} />查看总结报告</a>{req.experienceSummaryJob?.sessionId ? <a href={`/session?id=${encodeURIComponent(req.experienceSummaryJob.sessionId)}`}>总结 Agent</a> : null}<button type="button" onClick={retrySummary} disabled={summaryWorking}>{summaryWorking ? "派发中…" : req.experienceSummaryJob?.status === "failed" ? "重试总结" : "重新总结"}</button><a href={`/api/requirement/experience-summary-context?id=${encodeURIComponent(req.id)}&limit=200`} target="_blank" rel="noreferrer">候选汇总</a><a href={`/api/requirement/context?id=${encodeURIComponent(req.id)}&intent=experience-summary&budget=3000&format=html`} target="_blank" rel="noreferrer">总结上下文</a></>} /> : null}
      {isIssueFamily ? <RequirementDocPanel id="incident" req={req} docType="incident" title="问题档案" kicker="Incident Profile" path={req.incidentPath} description="回答这个问题「是什么」：现象描述、发现渠道、环境、首次/最近发生时间窗口（带时区绝对时间）、影响范围（单量/仓库/租户/用户）、复现步骤和时间线；排查中创建并持续更新，是问题身份证。" actions={<><a href={`/requirement-doc?id=${encodeURIComponent(req.id)}&doc=incident&title=${encodeURIComponent("问题档案")}`}><Library size={15} />整页查看</a><a href={`/api/requirement/context?id=${encodeURIComponent(req.id)}&intent=progress&budget=3000&format=html`} target="_blank" rel="noreferrer">排查上下文</a></>} /> : null}
      {isIssueFamily ? <RequirementDocPanel id="root-cause" req={req} docType="root-cause" title="根因与修复决策" kicker="Root Cause" path={req.rootCausePath} description="回答「为什么」和「怎么办」：根因（直接+深层）、证据链（每条必须附可复核线索：日志=时间范围+tid/关键字，DB=验证 SQL，代码=应用+文件+可搜关键字，配置=环境+key）、影响面、修复路径决策、临时处置与回滚；线上问题推进到「已定位」前必须填写，测试问题建议按需维护。" actions={<><a href={`/requirement-doc?id=${encodeURIComponent(req.id)}&doc=root-cause&title=${encodeURIComponent("根因与修复决策")}`}><Library size={15} />整页查看</a><a href={`/api/requirement/context?id=${encodeURIComponent(req.id)}&intent=progress&budget=3000&format=html`} target="_blank" rel="noreferrer">排查上下文</a></>} /> : null}
      {isIssueFamily ? <RequirementDocPanel id="troubleshooting" req={req} docType="troubleshooting" title="排查经验" kicker="Troubleshooting" path={req.troubleshootingPath} description="沉淀本问题的排查路径与修复方案：怎么排查（证据链、定位步骤、工具/命令）、怎么修复（方案、验证）、根因与复用清单；线上问题推进到「已复盘」前必须填写并回填经验库条目 id，测试问题按需沉淀。" actions={<><a href="/experiences"><Lightbulb size={15} />经验库</a><a href={`/requirement-doc?id=${encodeURIComponent(req.id)}&doc=troubleshooting&title=${encodeURIComponent("排查经验")}`}><Library size={15} />整页查看</a><a href={`/api/requirement/context?id=${encodeURIComponent(req.id)}&intent=progress&budget=3000&format=html`} target="_blank" rel="noreferrer">排查上下文</a></>} /> : null}
      {isSub ? <SubBranchPanel req={req} onSaved={refresh} /> : <MergeBranchPanel req={req} />}
      {!isIssueFamily && !isSub ? <ProdMrPanel req={req} /> : null}
      {!isIssueFamily && !isSub ? <LinkedIssuesPanel req={req} issues={data?.requirements || []} onSaved={refresh} /> : null}
      {!isSub ? <GroupMembersPanel req={req} /> : null}
      {!isSub ? <SubRequirementsPanel req={req} onSaved={refresh} /> : null}
      <RequirementFilesPanel req={req} />
      {onesModal && req ? <OnesBindModal req={req} onClose={() => setOnesModal(false)} onSaved={refresh} /> : null}
      {editField && req ? <ReqFieldEditModal req={req} field={editField} onClose={() => setEditField(null)} onSaved={refresh} /> : null}
    </div>}
  </PageChrome>
}
