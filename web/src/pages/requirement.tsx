import { AlertTriangle, ArrowLeft, Copy, FileCode2, GitBranch, GitMerge, Library, Lightbulb, List, MessageSquareText, Paperclip, Redo2, RefreshCw, Search, Undo2 } from "lucide-react"
import { useEffect, useMemo, useRef, useState } from "react"
import type { AnnotationsPayload, CodeAnnotations, CodeDiffSnapshot, CodeFileAnnotation, CodeReviewPayload, CodeReviewSnapshot, DiffSnapshotsPayload, HarnessCurrent, MasterDiffPayload, MergeBranchPayload, MergeKindOptions, MergeOptionsPayload, MergeRepoKind, MergeTarget, NewSessionPayload, PendingSessionPayload, ProdMrPayload, ProdMrResult, ReqCategory, ReqStatus, Requirement, RequirementAttachment, RequirementAttachmentsPayload, RequirementDocPayload, ReviewGatePayload, ReviewMaterialsPayload, SessionInfo, SyncBasePayload } from "../types"
import { fetchJson, postForm, postJson, putJson, useFetch } from "../lib/api"
import { copyRequirementSessionCommand } from "../features/requirements/session-command"
import { formatDate, formatDateTime, relAge } from "../lib/format"
import { ISSUE_STATUSES, REQ_CATEGORIES, REQ_FLOW_STATUSES, REQ_SOURCES } from "../lib/requirements"
import { compactPath, diffDomId, diffLineDomId, parseUnifiedDiffFiles, reviewStats, shortFileName, unquoteGitPath } from "../lib/diff"
import { annotationStaleRepos, buildAnnotationIndex, fileKeyOf, hunkOwners, matchHunkNotes } from "../lib/annotations"
import { AnnotationPanel } from "../features/requirements/annotation-panel"
import { Markdown } from "../markdown"
import { experienceSummaryPill, onesBadge, projectsOf, statusPill } from "../features/requirements/badges"
import { EmptyCard, ErrorCard, LoadingCard, PageChrome, PanelHead } from "../components/ui"
import { RequirementsData } from "./projects"
import { SessionChipList, SessionListModal } from "./sessions"

/** 关联线上问题面板：普通需求绑定线上问题（meta.md issues 字段），需求进入 >= 经验总结 时系统自动推进关联问题。 */
function LinkedIssuesPanel({ req, issues, onSaved }: { req: Requirement; issues: Requirement[]; onSaved: () => void }) {
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

function OnesPanel({ req, onSaved }: { req: Requirement; onSaved: () => void }) {
  const [ones, setOnes] = useState(req.ones || "")
  const [saving, setSaving] = useState(false)
  const [feedback, setFeedback] = useState<string | null>(null)
  const changed = ones.trim() !== (req.ones ?? "").trim()
  const submit = async () => {
    if (saving || !changed) return
    setSaving(true)
    try {
      await postForm("/api/requirement/ones", { reqId: req.id, ones })
      setFeedback("保存成功")
      onSaved()
    } catch (err) {
      setFeedback(`保存失败：${err instanceof Error ? err.message : String(err)}`)
    } finally {
      setSaving(false)
    }
  }
  return <section className="react-panel"><PanelHead kicker="ONES" title="ONES 任务关联" chip={onesBadge(req.ones)} /><p className="react-muted">粘贴 ONES 网址、编号，或直接从 ONES 复制的整段文本（编号 + 标题 + 链接），会自动识别为可点击引用；留空保存可清除关联。</p><div className="react-inline-form"><input value={ones} onChange={(e) => { setOnes(e.target.value); setFeedback(null) }} placeholder="ONES 网址 / 编号 / 带链接的复制文本" /><button onClick={submit} disabled={saving || !changed}>{saving ? "保存中…" : "保存"}</button></div>{feedback ? <p className="react-save-hint">{feedback}</p> : null}</section>
}

function PlanReleaseForm({ req, onSaved }: { req: Requirement; onSaved: () => void }) {
  const current = req.planRelease && req.planRelease !== "unknown" ? req.planRelease : ""
  const [planRelease, setPlanRelease] = useState(current)
  const [saving, setSaving] = useState(false)
  const [feedback, setFeedback] = useState<string | null>(null)
  useEffect(() => { setPlanRelease(current) }, [req.planRelease])
  const changed = planRelease !== current
  const submit = async (value: string) => {
    if (saving) return
    setSaving(true)
    try {
      await postForm("/api/requirement/update", { reqId: req.id, planRelease: value || "unknown" })
      setFeedback(value ? `预计发版已保存：${value}` : "已清除预计发版日期（unknown）")
      onSaved()
    } catch (err) {
      setFeedback(`保存失败：${err instanceof Error ? err.message : String(err)}`)
    } finally {
      setSaving(false)
    }
  }
  return <><div className="react-inline-form react-category-form"><label>预计发版</label><input type="date" value={planRelease} onChange={(e) => { setPlanRelease(e.target.value); setFeedback(null) }} /><button onClick={() => submit(planRelease)} disabled={saving || !changed}>{saving ? "保存中…" : "保存"}</button>{current ? <button type="button" onClick={() => { setPlanRelease(""); setFeedback(null); submit("") }}>清除</button> : null}</div>{feedback ? <p className="react-save-hint">{feedback}</p> : null}</>
}

function CodeReviewPanel({ req }: { req: Requirement }) {
  const { data, error, loading, refresh } = useFetch<CodeReviewPayload>(`/api/requirement/code-review?id=${encodeURIComponent(req.id)}`, [req.id])
  const gate = useFetch<ReviewGatePayload>(`/api/requirement/review-gate?id=${encodeURIComponent(req.id)}`, [req.id])
  const inventoryRisk = Boolean(gate.data?.gate?.inventoryRisk)
  const gateRiskTags = gate.data?.gate?.riskTags || []
  const staleRepos = gate.data?.gate?.staleRepos || []
  const gateStale = gate.data?.gate?.status === "stale"
  const incrementalReview = data?.incrementalReview || gate.data?.gate?.incrementalReview || null
  const incrementalStats = reviewStats(incrementalReview)
  const [refreshing, setRefreshing] = useState(false)
  const [refreshingIncremental, setRefreshingIncremental] = useState(false)
  const [showDiff, setShowDiff] = useState(false)
  const [actionError, setActionError] = useState<string | null>(null)
  const [syncing, setSyncing] = useState(false)
  const [syncPayload, setSyncPayload] = useState<SyncBasePayload | null>(null)
  const [preparingMaterials, setPreparingMaterials] = useState(false)
  const [materials, setMaterials] = useState<ReviewMaterialsPayload["materials"] | null>(null)
  const scope = data?.branchScope || null
  const review = data?.review || null
  const stats = reviewStats(review)
  const canScan = Boolean(scope?.repos?.length)
  const refreshScan = async () => {
    if (!canScan || refreshing) return
    setRefreshing(true)
    setActionError(null)
    try {
      await postForm<CodeReviewPayload>("/api/requirement/code-review", { reqId: req.id })
      refresh()
      gate.refresh()
    } catch (err) {
      setActionError(err instanceof Error ? err.message : String(err))
    } finally {
      setRefreshing(false)
    }
  }
  const refreshIncrementalScan = async () => {
    if (!canScan || refreshingIncremental) return
    setRefreshingIncremental(true)
    setActionError(null)
    try {
      await postForm<CodeReviewPayload>("/api/requirement/code-review/incremental", { reqId: req.id })
      refresh()
      gate.refresh()
    } catch (err) {
      setActionError(err instanceof Error ? err.message : String(err))
    } finally {
      setRefreshingIncremental(false)
    }
  }
  const syncBase = async () => {
    if (!canScan || syncing) return
    setSyncing(true)
    setActionError(null)
    try {
      const payload = await postForm<SyncBasePayload>("/api/requirement/sync-base", { reqId: req.id })
      setSyncPayload(payload)
      refresh()
      gate.refresh()
    } catch (err) {
      setActionError(err instanceof Error ? err.message : String(err))
    } finally {
      setSyncing(false)
    }
  }
  const prepareMaterials = async () => {
    if (!canScan || preparingMaterials) return
    setPreparingMaterials(true)
    setActionError(null)
    try {
      const payload = await postForm<ReviewMaterialsPayload>("/api/requirement/review-materials", { reqId: req.id })
      setMaterials(payload.materials)
      refresh()
      gate.refresh()
    } catch (err) {
      setActionError(err instanceof Error ? err.message : String(err))
    } finally {
      setPreparingMaterials(false)
    }
  }
  return <section id="code-review" className="react-panel react-code-review-panel"><PanelHead kicker="Code Review Gate" title="代码审查门禁" chip={gate.data?.gate?.label || (gate.loading ? "loading" : "gate")} />
    <div className={`react-review-gate react-review-gate-${gate.data?.gate?.status || "unknown"}`}><strong>{gate.data?.gate?.label || "读取中"}</strong><span>{gate.data?.gate?.reason || "自测中推进到测试中前必须完成代码审查门禁。"}</span>{gate.data?.gate?.source ? <em>source: {gate.data.gate.source}</em> : null}</div>
    {gateRiskTags.length ? <div className="react-review-risk-tags"><strong>风险标签</strong>{gateRiskTags.map((tag) => <span key={tag} className="react-review-tag">{tag}</span>)}</div> : null}
    {inventoryRisk ? <div className="react-review-gate react-review-gate-blocked"><strong>⚠ 库存高危风险</strong><span>本次改动命中库存相关文件/表，门禁强制要求库存账本专项评估：单据活跃/死亡、DB 库存(onHand/allocated/临时库位/回库单)、redis 可用量(建单-、真取消+、恢复-、回退保持占用)、重复释放、遗漏占用、幂等、验证证据(DB/redis/日志/单测)。未补充前即使 PASS 也不通过。</span></div> : null}
    {gateStale ? <div className="react-drive-blockers"><strong>审查快照需刷新覆盖</strong>{staleRepos.length ? <ul>{staleRepos.map((repo) => <li key={`${repo.repoName}-${repo.branch}`}><code>{repo.repoName}</code> / <code>{repo.branch}</code>：{(repo.reviewedTargetCommit || "").slice(0, 12) || "reviewed?"} → {(repo.currentTargetCommit || "").slice(0, 12) || "current?"}</li>)}</ul> : null}<p>优先生成增量审查包，只审上次已审 commit 到当前 HEAD 的新增 diff；非线性历史再回退全量审查。</p></div> : null}
    {gate.data?.gate?.actions?.length ? <div className="react-drive-blockers"><strong>门禁动作</strong><ul>{gate.data.gate.actions.map((item) => <li key={item}>{item}</li>)}</ul></div> : null}
    {gate.error ? <p className="react-effort-error">门禁加载失败：{gate.error}</p> : null}
    <div className="react-actions"><button onClick={refreshScan} disabled={!canScan || refreshing}><RefreshCw size={15} className={refreshing ? "react-spin" : ""} />{review ? "刷新全量差异" : "生成代码差异"}</button>{gateStale ? <button onClick={refreshIncrementalScan} disabled={!canScan || refreshingIncremental}><RefreshCw size={15} className={refreshingIncremental ? "react-spin" : ""} />生成增量审查包</button> : null}<button onClick={prepareMaterials} disabled={!canScan || preparingMaterials} title="一键备料：无快照生成全量、有漂移生成增量包、非线性历史回退全量；返回快照路径供 reviewer 直接 read，审查产出同步维护差异页说明栏（code-annotations.json）"><RefreshCw size={15} className={preparingMaterials ? "react-spin" : ""} />{preparingMaterials ? "备料中…" : "准备审查材料"}</button><button onClick={syncBase} disabled={!canScan || syncing} title="fetch 远端生产分支并 reset 本地 master/production 到最新,工作区有改动时自动跳过"><RefreshCw size={15} className={syncing ? "react-spin" : ""} />{syncing ? "同步中…" : "同步生产基线"}</button>{review ? <button onClick={() => setShowDiff((v) => !v)}>{showDiff ? "隐藏 unified diff" : "展示 unified diff"}</button> : null}<a href={`/requirement-diff?id=${encodeURIComponent(req.id)}&base=origin%2Fmaster`}><GitBranch size={15} />打开分支差异页</a></div>
    {syncPayload?.results?.length ? <details className="react-review-repo" open><summary><span><strong>生产基线同步</strong><em>{formatDateTime(syncPayload.generatedAt)}</em></span><span className="react-review-size">{syncPayload.results.filter((r) => r.ok).length}/{syncPayload.results.length} ok</span></summary><div className="react-table-wrap react-code-file-wrap"><table className="react-code-file-table"><thead><tr><th>应用</th><th>本地分支</th><th>状态</th><th>before</th><th>after</th><th>说明</th></tr></thead><tbody>{syncPayload.results.map((r) => <tr key={r.repoName}><td><strong>{r.repoName}</strong></td><td><code>{r.localBranch || r.baseRef || "-"}</code></td><td><span className={`react-merge-status ${r.ok ? "merged" : "conflict"}`}>{r.status}</span></td><td><code>{r.beforeCommit || "-"}</code></td><td><code>{r.afterCommit || "-"}</code></td><td>{r.message}{r.warnings?.length ? <em>{r.warnings.join("; ")}</em> : null}</td></tr>)}</tbody></table></div></details> : null}
    {materials ? <details className="react-review-repo" open><summary><span><strong>审查材料就绪</strong><em>{materials.mode} · {materials.materialFile}</em></span><span className="react-review-size">{materials.repos.length} repo</span></summary><p className="react-muted">{materials.reason}。reviewer 材料：<code>{materials.materialPath}</code></p>{materials.handoffHints?.length ? <ul className="react-muted">{materials.handoffHints.map((hint, i) => <li key={i}>{hint}</li>)}</ul> : null}{materials.repos.map((repo, i) => <div key={`${repo.repoName}-${repo.branch}-${i}`} className="react-branch-card"><strong>{repo.repoName}</strong><code>{repo.branch}</code><span>{repo.fromCommit?.slice(0, 12) || "?"} → {repo.toCommit?.slice(0, 12) || "?"}</span><em>{repo.linearHistory === false ? "非线性历史" : `+${repo.additions ?? 0} / -${repo.deletions ?? 0}`}</em></div>)}</details> : null}
    {error ? <p className="react-effort-error">加载失败：{error}</p> : null}{actionError ? <p className="react-effort-error">刷新失败：{actionError}</p> : null}
    {loading ? <LoadingCard label="正在加载代码差异…" /> : <>
      <div className="react-branch-scope">
        {scope?.repos?.length ? scope.repos.map((repo) => <div key={`${repo.repoName}-${repo.branches?.join("/")}`} className="react-branch-card"><strong>{repo.repoName}</strong><span>{repo.role || "repo"}</span><code>{repo.branches?.join(" / ") || "未指定分支"}</code><em>{repo.baseRef || (repo.role === "前端" ? "origin/production" : "origin/master")}</em></div>) : <p className="react-muted">未找到 <code>branches.json</code>，无法生成代码差异；请先运行 <code>req-branches-update</code>。</p>}
      </div>
      {incrementalReview ? <details className="react-review-repo" open><summary><span><strong>增量审查包</strong><em>{incrementalReview.baseDescription || "reviewed commit → current HEAD"}</em></span><span className="react-review-size">{incrementalStats.repoCount} repo / {incrementalStats.fileCount} files / +{incrementalStats.additions} / -{incrementalStats.deletions}</span></summary><p className="react-muted">供二次 AI 审查优先读取 <code>code-review-incremental.json</code>；审完后在 <code>code-review-ai.md</code> 或 <code>review.md</code> 注明增量覆盖范围并重新写明 Review Gate。</p>{incrementalReview.repos.map((repo, index) => <div key={`${repo.repoName}-${repo.branch}-${index}`} className="react-branch-card"><strong>{repo.repoName}</strong><code>{repo.branch}</code><span>{repo.coverageFromCommit?.slice(0, 12) || repo.baseCommit?.slice(0, 12) || "base?"} → {repo.coverageToCommit?.slice(0, 12) || repo.targetCommit?.slice(0, 12) || "head?"}</span><em>{repo.linearHistory === false ? "非线性历史：建议全量审查" : `+${repo.additions || 0} / -${repo.deletions || 0}`}</em></div>)}</details> : null}
      {review ? <div className="react-review-summary"><span>{stats.repoCount} repo/branch</span><span>{stats.fileCount} files</span><span className="react-review-add">+{stats.additions}</span><span className="react-review-del">-{stats.deletions}</span><span>更新 {formatDateTime(review.updatedAt)}</span></div> : <p className="react-muted">暂无 <code>code-review.json</code> 快照；点击“生成代码差异”后会读取本地 git diff 并写回需求目录。</p>}
      {review?.repos?.map((repo, index) => <details key={`${repo.repoName}-${repo.branch}-${index}`} className="react-review-repo" open={index === 0}>
        <summary><span><strong>{repo.repoName}</strong><em>{repo.branch}</em></span><span className="react-review-size">+{repo.additions || 0} / -{repo.deletions || 0}</span></summary>
        <div className="react-card-meta"><span>base {repo.baseRef || review.baseRef}{repo.baseCommit ? ` @ ${repo.baseCommit.slice(0, 12)}` : ""}</span><span>target {repo.resolvedTargetRef || repo.branch}{repo.targetCommit ? ` @ ${repo.targetCommit.slice(0, 12)}` : ""}</span><span>current {repo.currentBranch || "-"}</span><span>{repo.dirty ? "工作区有未提交改动" : "工作区干净"}</span><span>{repo.projectPath || "path n/a"}</span></div>
        {repo.error ? <p className="react-effort-error">{repo.error}</p> : null}
        {repo.warnings?.length ? <div className="react-drive-blockers"><strong>Warnings</strong><ul>{repo.warnings.map((w) => <li key={w}>{w}</li>)}</ul></div> : null}
        {repo.commits?.length ? <details className="react-review-commits"><summary>提交列表（{repo.commits.length}）</summary><pre>{repo.commits.join("\n")}</pre></details> : null}
        {repo.files?.length ? <div className="react-table-wrap react-code-file-wrap"><table className="react-code-file-table"><thead><tr><th>文件</th><th>状态</th><th>增删</th><th>风险</th></tr></thead><tbody>{repo.files.map((file) => <tr key={file.path}><td><code>{unquoteGitPath(file.path)}</code></td><td>{file.status}</td><td><span className="react-review-add">+{file.additions}</span> / <span className="react-review-del">-{file.deletions}</span></td><td>{file.riskTags?.length ? file.riskTags.map((tag) => <span key={tag} className="react-review-tag" data-risk={tag}>{tag}</span>) : <span className="react-muted">-</span>}</td></tr>)}</tbody></table></div> : <p className="react-muted">没有文件级差异。</p>}
        {showDiff && repo.diff ? <pre className="react-diff-preview">{repo.diff}{repo.diffTruncated ? "\n… diff 已截断" : ""}</pre> : null}
      </details>)}
    </>}
  </section>
}

function MergeBranchPanel({ req }: { req: Requirement }) {
  const optionData = useFetch<MergeOptionsPayload>(`/api/requirement/merge-options?id=${encodeURIComponent(req.id)}`, [req.id])
  const [payloads, setPayloads] = useState<MergeBranchPayload[]>([])
  const [frontendBranch, setFrontendBranch] = useState("")
  const [backendBranch, setBackendBranch] = useState("")
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  useEffect(() => {
    const options = optionData.data?.options
    if (!options) return
    setFrontendBranch(options.frontend?.defaultValue || "")
    setBackendBranch(options.backend?.defaultValue || "")
  }, [optionData.data?.generatedAt, req.id])
  const runMerge = async () => {
    if (loading) return
    const jobs: Array<{ kind: MergeRepoKind; branch: string; target: string }> = []
    const frontendOption = optionData.data?.options.frontend.options.find((item) => item.value === frontendBranch)
    const backendOption = optionData.data?.options.backend.options.find((item) => item.value === backendBranch)
    if (frontendBranch && frontendOption) jobs.push({ kind: "frontend", branch: frontendBranch, target: frontendOption.target })
    if (backendBranch && backendOption) jobs.push({ kind: "backend", branch: backendBranch, target: backendOption.target })
    if (!jobs.length) { setError("请先选择前端或后端目标分支"); return }
    setLoading(true)
    setError(null)
    try {
      const next: MergeBranchPayload[] = []
      for (const job of jobs) {
        next.push(await postForm<MergeBranchPayload>("/api/requirement/merge-branch", { reqId: req.id, repoKind: job.kind, targetBranch: job.branch, target: job.target }))
      }
      setPayloads(next)
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err))
    } finally {
      setLoading(false)
    }
  }
  const results = payloads.flatMap((item) => item.results || [])
  const conflictCount = results.filter((item) => item.status === "conflict").length
  const mergedCount = results.filter((item) => item.status === "merged" || item.status === "upToDate").length
  const latestAt = Math.max(0, ...payloads.map((item) => item.generatedAt || 0))
  const chip = payloads.length ? payloads.map((item) => item.status).join(" / ") : "选择环境分支"
  const selectBranch = (kind: MergeRepoKind, value: string, onChange: (v: string) => void, options?: MergeKindOptions) => <label className="react-merge-select"><span>{kind === "frontend" ? "前端分支" : "后端分支"}</span><select value={value} onChange={(e) => onChange(e.target.value)} disabled={loading || optionData.loading || !options?.options?.length}><option value="">不合并</option>{(options?.options || []).map((item) => <option key={`${kind}-${item.value}`} value={item.value}>{item.label}</option>)}</select></label>
  return <section className="react-panel react-merge-panel"><PanelHead kicker="Branch Merge" title="合并到测试 / UAT" chip={chip} />
    <p className="react-muted">选择要合并的环境分支后执行；未选择的前端/后端不会合并。自测中默认选 test，测试中默认选 UAT 分支，其他状态默认不选择。</p>
    {optionData.error ? <p className="react-effort-error">分支选项加载失败：{optionData.error}</p> : null}
    <div className="react-merge-select-grid">{selectBranch("frontend", frontendBranch, setFrontendBranch, optionData.data?.options.frontend)}{selectBranch("backend", backendBranch, setBackendBranch, optionData.data?.options.backend)}</div>
    <div className="react-actions"><button onClick={runMerge} disabled={loading || optionData.loading || (!frontendBranch && !backendBranch)}><GitMerge size={15} />{loading ? "合并中…" : "合并所选分支"}</button><a href={`/requirement-merge?id=${encodeURIComponent(req.id)}`}><AlertTriangle size={15} />查看冲突 / 合并状态</a>{latestAt ? <span className="react-muted">更新 {formatDateTime(latestAt)}</span> : null}</div>
    {error ? <p className="react-effort-error">合并失败：{error}</p> : null}
    {payloads.length ? <div className="react-review-summary"><span>{results.length} repo/branch</span><span className="react-review-add">{mergedCount} merged</span><span className={conflictCount ? "react-review-del" : undefined}>{conflictCount} conflict</span><span>{[frontendBranch, backendBranch].filter(Boolean).join(" / ") || "-"}</span></div> : null}
    {results.length ? <div className="react-table-wrap react-prod-mr-wrap"><table className="react-code-file-table react-prod-mr-table"><thead><tr><th>应用</th><th>源分支</th><th>目标</th><th>状态</th><th>冲突 / 位置</th></tr></thead><tbody>{results.map((item, index) => <tr key={`${item.repoName}-${item.sourceBranch}-${item.target}-${index}`}><td><strong>{item.repoName}</strong><span>{item.role || "repo"}</span></td><td><code>{item.sourceBranch}</code></td><td><code>{item.targetBranch || item.target}</code></td><td><span className={`react-merge-status ${item.status}`}>{mergeStatusLabel(item.status)}</span>{item.message ? <em>{item.message}</em> : null}</td><td>{item.status === "conflict" ? <a href={`/requirement-merge?id=${encodeURIComponent(req.id)}&target=${encodeURIComponent(String(item.target))}`}>{item.conflictFiles?.length || 0} 个冲突文件</a> : item.worktreePath ? <code>{item.worktreePath}</code> : <span className="react-muted">-</span>}</td></tr>)}</tbody></table></div> : payloads.length ? <p className="react-muted">未返回合并结果，请检查 <code>branches.json</code>。</p> : null}
  </section>
}

function mergeStatusLabel(status: string): string {
  if (status === "merged") return "已合并"
  if (status === "upToDate") return "已最新"
  if (status === "conflict") return "冲突"
  if (status === "skipped") return "跳过"
  if (status === "idle") return "空闲"
  if (status === "pending") return "待检查"
  if (status === "failed") return "失败"
  return status
}

function ProdMrPanel({ req }: { req: Requirement }) {
  const [payload, setPayload] = useState<ProdMrPayload | null>(null)
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [copiedKey, setCopiedKey] = useState<string | null>(null)
  const createMrs = async () => {
    if (loading) return
    setLoading(true)
    setError(null)
    try {
      const next = await postForm<ProdMrPayload>("/api/requirement/prod-mrs", { reqId: req.id })
      setPayload(next)
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err))
    } finally {
      setLoading(false)
    }
  }
  const copyMrLink = async (item: ProdMrResult, index: number) => {
    if (!item.webUrl) return
    const key = `${item.repoName}-${item.sourceBranch}-${index}`
    try {
      await navigator.clipboard.writeText(item.webUrl)
      setCopiedKey(key)
      window.setTimeout(() => setCopiedKey((current) => current === key ? null : current), 1600)
    } catch (err) {
      setError(err instanceof Error ? err.message : "复制失败，请手动复制链接")
    }
  }
  const results = payload?.results || []
  const okCount = results.filter((item) => item.webUrl).length
  const noDiffCount = results.filter((item) => item.status === "no_diff").length
  const chip = payload ? `${okCount}/${results.length} ready${noDiffCount ? ` · ${noDiffCount} 无差异` : ""}` : "test/uat → prod"
  return <section className="react-panel react-prod-mr-panel"><PanelHead kicker="Production MR" title="生产 MR" chip={chip} />
    <p className="react-muted">按 <code>branches.json</code> 中每个应用的每个需求分支创建生产 MR；后端目标 <code>master</code>，前端目标 <code>production</code>。<strong>仅创建合并请求，不会自动合入生产分支</strong>，合入需在 GitLab 上由组长审批后手动执行。生成前先比对需求分支与生产分支的差异，<strong>无差异的仓库自动跳过 MR</strong>。再次点击会重新扫描并复用已存在的 open MR。</p>
    <div className="react-actions"><button onClick={createMrs} disabled={loading}><GitBranch size={15} />{loading ? "生成中…" : payload ? "重新生成 / 复用 MR" : "生成生产 MR 链接"}</button>{payload?.generatedAt ? <span className="react-muted">更新 {formatDateTime(payload.generatedAt)}</span> : null}</div>
    {error ? <p className="react-effort-error">生成失败：{error}</p> : null}
    {results.length ? <div className="react-table-wrap react-prod-mr-wrap"><table className="react-code-file-table react-prod-mr-table"><thead><tr><th>应用</th><th>源分支</th><th>目标</th><th>状态</th><th>生产差异</th><th>MR</th><th>复制</th></tr></thead><tbody>{results.map((item, index) => { const key = `${item.repoName}-${item.sourceBranch}-${index}`; return <tr key={key}><td><strong>{item.repoName}</strong><span>{item.role || "repo"}</span></td><td><code>{item.sourceBranch}</code></td><td><code>{item.targetBranch}</code></td><td><span className={`react-prod-mr-status ${item.status}`}>{item.status === "created" ? "新建" : item.status === "reused" ? "复用" : item.status === "skipped" ? "跳过" : item.status === "no_diff" ? "无差异" : "失败"}</span>{item.error ? <em>{item.error}</em> : null}</td><td>{item.diffFiles != null ? (item.diffFiles === 0 ? <span className="react-muted">无</span> : <span><span className="react-review-add">+{item.diffAdditions ?? 0}</span> / <span className="react-review-del">-{item.diffDeletions ?? 0}</span> <span className="react-muted">({item.diffFiles})</span></span>) : <span className="react-muted">-</span>}</td><td>{item.webUrl ? <a href={item.webUrl} target="_blank" rel="noopener noreferrer">!{item.iid || "MR"}</a> : <span className="react-muted">-</span>}</td><td>{item.webUrl ? <button type="button" className="react-copy-link-btn" onClick={() => copyMrLink(item, index)}>{copiedKey === key ? "已复制" : "复制链接"}</button> : <span className="react-muted">-</span>}</td></tr> })}</tbody></table></div> : payload ? <p className="react-muted">未生成 MR，请检查 <code>branches.json</code> 是否包含应用和分支。</p> : null}
  </section>
}

const DOC_TITLES: Record<string, string> = {
  "background": "业务背景文档",
  "technical-plan": "技术方案",
  "release-manifest": "上线清单",
  "experience-summary": "经验总结闭环",
  "troubleshooting": "排查经验",
  "incident": "问题档案",
  "root-cause": "根因与修复决策",
  "review": "代码审查",
  "release-check": "发布预检",
  "test": "测试用例",
  "test-scenario": "测试场景文档",
  "notes": "执行笔记",
  "meta": "需求信息",
}

export function RequirementDocPage() {
  const params = new URLSearchParams(window.location.search)
  const id = params.get("id") || params.get("reqId") || ""
  const docType = params.get("doc") || params.get("file") || "background"
  const doc = useFetch<RequirementDocPayload>(id ? `/api/requirement/doc?id=${encodeURIComponent(id)}&file=${encodeURIComponent(docType)}` : null, [id, docType])
  const title = DOC_TITLES[docType] ?? docType
  return <PageChrome icon={<Library size={15} />} eyebrow="Requirement Doc" title={title} description={id ? `需求 ${id}` : undefined} actions={<><a href={id ? `/requirement?id=${encodeURIComponent(id)}` : "/projects"}><ArrowLeft size={15} />返回需求</a></>}>
    <section className="react-panel react-doc-page-panel">
      <PanelHead kicker="Document" title={title} chip={doc.loading ? "loading" : doc.data?.file || docType} />
      {doc.error ? <ErrorCard error={doc.error} /> : doc.loading ? <LoadingCard label="正在加载文档…" /> : !doc.data?.exists ? (
        <EmptyCard>暂无内容。可在需求澄清阶段生成或更新该文档。</EmptyCard>
      ) : <Markdown text={doc.data.content || ""} />}
    </section>
  </PageChrome>
}

export function RequirementDiffPage() {
  const params = new URLSearchParams(window.location.search)
  const reqId = params.get("id") || params.get("reqId") || ""
  const initialBase = params.get("base") || "origin/master"
  const requirements = RequirementsData()
  const req = requirements.data?.requirements.find((r) => r.id === reqId)
  const [baseRef, setBaseRef] = useState(initialBase)
  const [loadingDiff, setLoadingDiff] = useState(false)
  const [error, setError] = useState<string | null>(null)
  /** 快照对比模式：差异栈（新在前）+ 当前查看的下标；进入页面只读栈，点刷新才生成并入栈。 */
  const [snapshots, setSnapshots] = useState<CodeDiffSnapshot[]>([])
  const [viewIndex, setViewIndex] = useState(0)
  const review = snapshots[viewIndex] || null
  const files = useMemo(() => parseUnifiedDiffFiles(review), [review])
  const stats = reviewStats(review)
  const [activeKey, setActiveKey] = useState("")
  /** 行级联动状态：用户点击的 diff 行（key + 行下标），驱动右侧说明面板与 hunk 高亮。 */
  const [activeLine, setActiveLine] = useState<{ key: string; line: number } | null>(null)
  const [savingAnnotation, setSavingAnnotation] = useState(false)
  /** 顶部统一横向滚动条：diff 内容（所有文件卡片）放进同一个横向滚动容器，顶部 sticky 工具栏下方挂一条滚动条统一控制，替代每行超长代码各自的滚动条。 */
  const scrollBodyRef = useRef<HTMLDivElement | null>(null)
  const scrollBarRef = useRef<HTMLDivElement | null>(null)
  const [hScrollWidth, setHScrollWidth] = useState(0)
  useEffect(() => {
    const body = scrollBodyRef.current
    if (!body) return
    const measure = () => {
      const bar = scrollBarRef.current
      // 补偿顶部滚动条与内容区 clientWidth 的差值（边框等），保证两侧滚动范围一致、scrollLeft 可 1:1 同步。
      setHScrollWidth(body.scrollWidth + Math.max(0, (bar?.clientWidth ?? 0) - body.clientWidth))
    }
    measure()
    const ro = new ResizeObserver(measure)
    ro.observe(body)
    for (const child of Array.from(body.children)) ro.observe(child)
    window.addEventListener("resize", measure)
    document.fonts?.ready.then(measure).catch(() => {})
    return () => { ro.disconnect(); window.removeEventListener("resize", measure) }
  }, [files, loadingDiff])
  // 双向同步顶部滚动条与内容区；赋值触发的对侧 scroll 事件里两侧值已相等，不会来回抖动。
  const onHScrollBarScroll = () => {
    const body = scrollBodyRef.current
    const bar = scrollBarRef.current
    if (body && bar && body.scrollLeft !== bar.scrollLeft) body.scrollLeft = bar.scrollLeft
  }
  const onHScrollBodyScroll = () => {
    const body = scrollBodyRef.current
    const bar = scrollBarRef.current
    if (body && bar && bar.scrollLeft !== body.scrollLeft) bar.scrollLeft = body.scrollLeft
  }
  const annotations = useFetch<AnnotationsPayload>(reqId ? `/api/requirement/annotations?reqId=${encodeURIComponent(reqId)}` : null, [reqId])
  // 进入页面只加载已保存的快照栈，不自动生成；生成由「刷新代码差异」显式触发。
  useEffect(() => {
    if (!reqId) return
    let cancelled = false
    fetchJson<DiffSnapshotsPayload>(`/api/requirement/diff-snapshots?reqId=${encodeURIComponent(reqId)}`)
      .then((payload) => { if (!cancelled) { setSnapshots(payload.snapshots || []); setViewIndex(0) } })
      .catch(() => { if (!cancelled) setSnapshots([]) })
    return () => { cancelled = true }
  }, [reqId])
  // 生成新差异并入栈（后端保留最近 5 版）；base 不变时点刷新也会重新生成，旧版本保留可回退。
  const generateDiff = async (base = baseRef) => {
    if (!reqId) return
    setLoadingDiff(true)
    setError(null)
    try {
      const payload = await postForm<MasterDiffPayload>("/api/requirement/master-diff", { reqId, baseRef: base })
      setSnapshots(payload.snapshots || [])
      setViewIndex(0)
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err))
    } finally {
      setLoadingDiff(false)
    }
  }
  useEffect(() => { setActiveLine(null) }, [viewIndex])
  useEffect(() => {
    if (!files.length) { setActiveKey(""); return }
    const exists = files.some((item) => `${item.repo.repoName}:${item.file.path}` === activeKey)
    if (!exists) setActiveKey(`${files[0].repo.repoName}:${files[0].file.path}`)
  }, [files, activeKey])
  const activeIndex = Math.max(0, files.findIndex((item) => `${item.repo.repoName}:${item.file.path}` === activeKey))
  const activeView = files.find((item) => `${item.repo.repoName}:${item.file.path}` === activeKey) || null
  const annotationIndex = useMemo(() => buildAnnotationIndex(files, annotations.data?.annotations), [files, annotations.data])
  const staleRepos = useMemo(() => annotationStaleRepos(annotations.data?.annotations, files), [annotations.data, files])
  const activeAnnotation = annotationIndex.get(activeKey) || null
  const activeHunkOwners = useMemo(() => hunkOwners(activeView?.lines || []), [activeView])
  const hunkNotes = useMemo(() => matchHunkNotes(activeView?.lines || [], activeAnnotation?.notes), [activeView, activeAnnotation])
  const activeHunkLine = activeLine && activeLine.key === activeKey ? activeHunkOwners[activeLine.line] ?? -1 : -1
  const noteLocateMap = useMemo(() => {
    const map = new Map<number, number>()
    const notes = activeAnnotation?.notes || []
    for (const hit of hunkNotes.matched) {
      const idx = notes.indexOf(hit.note)
      if (idx >= 0) map.set(idx, hit.lineIndex)
    }
    return map
  }, [hunkNotes, activeAnnotation])
  const scrollToFile = (key: string) => {
    setActiveKey(key)
    setActiveLine(null)
    document.getElementById(diffDomId(key))?.scrollIntoView({ behavior: "smooth", block: "start" })
  }
  const locateNote = (noteIndex: number) => {
    const lineIndex = noteLocateMap.get(noteIndex)
    if (lineIndex == null) return
    setActiveLine({ key: activeKey, line: lineIndex })
    document.getElementById(diffLineDomId(activeKey, lineIndex))?.scrollIntoView({ behavior: "smooth", block: "center" })
  }
  const saveAnnotation = async (next: CodeFileAnnotation) => {
    if (!reqId) return
    setSavingAnnotation(true)
    try {
      const current: CodeAnnotations = { ...(annotations.data?.annotations || {}) }
      const list = [...(current.files || [])]
      const idx = list.findIndex((f) => f.repo === next.repo && f.path === next.path)
      if (idx >= 0) list[idx] = next
      else list.push(next)
      current.files = list
      await putJson("/api/requirement/annotations", { reqId, annotations: current })
      annotations.refresh()
    } finally {
      setSavingAnnotation(false)
    }
  }
  const changeBase = (next: string) => {
    setBaseRef(next)
    const q = new URLSearchParams(window.location.search)
    q.set("id", reqId)
    q.set("base", next)
    window.history.replaceState(null, "", `/requirement-diff?${q.toString()}`)
    // 切基准分支即以新基线生成并保存一版新快照，旧快照仍在栈内可回退。
    generateDiff(next)
  }
  const title = req?.title || reqId || "分支差异"
  return <PageChrome icon={<GitBranch size={15} />} eyebrow="Diff" title={title} description="快照对比：生成的代码差异保存为本地快照，点刷新生成新版（旧版保留可回退），右侧查看/编辑文件说明。" actions={<><a href={`/requirement?id=${encodeURIComponent(reqId)}`}><ArrowLeft size={15} />返回需求</a><button onClick={() => generateDiff()} disabled={loadingDiff || !reqId}><RefreshCw size={15} className={loadingDiff ? "react-spin" : ""} />{loadingDiff ? "生成中…" : snapshots.length ? "刷新代码差异" : "生成代码差异"}</button>{viewIndex < snapshots.length - 1 ? <button onClick={() => setViewIndex((i) => Math.min(i + 1, snapshots.length - 1))} disabled={loadingDiff}><Undo2 size={15} />回退上一版</button> : null}{viewIndex > 0 ? <button onClick={() => setViewIndex(0)} disabled={loadingDiff}><Redo2 size={15} />回到最新</button> : null}</>}>
    <section className="react-diff-shell">
      <aside className="react-diff-sidebar"><div className="react-diff-compare"><span>Compare</span><select value={baseRef} onChange={(e) => changeBase(e.target.value)}><option value="origin/master">origin/master</option><option value="origin/production">origin/production</option><option value="master">master</option><option value="production">production</option></select><em>and latest version</em></div><label className="react-diff-search"><Search size={14} /><input placeholder="Search files (Ctrl+P)" onChange={(e) => { const hit = files.find((f) => f.file.path.toLowerCase().includes(e.target.value.toLowerCase())); if (hit && e.target.value) scrollToFile(`${hit.repo.repoName}:${hit.file.path}`) }} /></label><div className="react-diff-file-list">{[...new Set(review?.repos?.map((r) => r.repoName) || [])].map((repoName) => {
          const repo = review?.repos?.find((r) => r.repoName === repoName)
          const repoFiles = files.filter((f) => f.repo.repoName === repoName)
          return <details key={repoName} className="react-diff-repo-group" open>
            <summary><strong>{repoName}</strong><em>base: {repo?.baseRef || baseRef}</em></summary>
            {repoFiles.length ? repoFiles.map((item) => { const key = `${item.repo.repoName}:${item.file.path}`; return <button key={key} className={key === activeKey ? "active" : ""} onClick={() => scrollToFile(key)}><FileCode2 size={14} /><span><strong>{shortFileName(item.file.path)}</strong><small>{compactPath(item.file.path)}</small></span><em>{annotationIndex.has(key) ? <MessageSquareText size={12} className="react-diff-annotated" /> : null}<b>+{item.file.additions}</b> <i>-{item.file.deletions}</i></em></button> }) : <p className="react-muted" style={{padding: '8px'}}>暂无文件差异</p>}
          </details>
        })}</div></aside>
      <main className="react-diff-main"><div className="react-diff-topbar"><div className="react-diff-toolbar"><div><strong>{stats.fileCount} files</strong><span className="react-review-add">+{stats.additions}</span><span className="react-review-del">-{stats.deletions}</span>{snapshots.length ? <span>快照 {viewIndex + 1}/{snapshots.length}</span> : null}{review?.savedAt ? <span>生成 {formatDateTime(review.savedAt)}</span> : review?.updatedAt ? <span>生成 {formatDateTime(review.updatedAt)}</span> : null}{viewIndex > 0 ? <span className="react-warn-note">⚠ 正在查看历史快照（base: {review?.baseRef || baseRef}），备注锚定以当前显示快照为准</span> : null}{review?.repos?.some((r) => r.diffTruncated) ? <span className="react-warn-note">⚠ 部分仓库 diff 超过输出上限被截断，缺失内容可分仓或减小差异后重试</span> : null}{staleRepos.length ? <span className="react-warn-note">⚠ {staleRepos.join(" / ")} 的备注基于旧 diff</span> : null}</div><span>{activeIndex + 1}/{Math.max(files.length, 1)}</span></div>{files.length ? <div className="react-diff-hscroll" ref={scrollBarRef} onScroll={onHScrollBarScroll}><div className="react-diff-hscroll-pad" style={{ width: hScrollWidth }} /></div> : null}</div>{requirements.error ? <ErrorCard error={requirements.error} /> : error ? <ErrorCard error={error} /> : loadingDiff ? <LoadingCard label="正在生成代码差异…" /> : files.length === 0 ? <EmptyCard>{snapshots.length === 0 ? <>暂无代码差异快照。点右上角「生成代码差异」首次生成并保存；之后每次刷新都会保留旧版本，可回退对比。</> : "该快照没有可展示的文件级差异。"}</EmptyCard> : <div className="react-diff-scroll-body" ref={scrollBodyRef} onScroll={onHScrollBodyScroll}>{files.map((item) => { const key = `${item.repo.repoName}:${item.file.path}`; const owners = hunkOwners(item.lines); const focusHunk = activeLine && activeLine.key === key ? owners[activeLine.line] ?? -1 : -1; return <article key={key} id={diffDomId(key)} className="react-diff-file-card"><header><div><FileCode2 size={16} /><strong>{item.repo.repoName}/{item.file.path}</strong><em className="react-diff-base-label">vs {item.repo.baseRef || review?.baseRef || "?"}</em></div><span><b>+{item.file.additions}</b><i>-{item.file.deletions}</i></span></header>{item.lines.length ? <table className="react-diff-code"><tbody>{item.lines.map((line, i) => <tr key={i} id={diffLineDomId(key, i)} className={`react-diff-line-${line.type}${focusHunk >= 0 && owners[i] === focusHunk ? " react-diff-line-focus" : ""}`} onClick={() => { setActiveKey(key); setActiveLine({ key, line: i }) }}><td>{line.oldNo}</td><td>{line.newNo}</td><td><code>{line.type === "add" ? "+" : line.type === "del" ? "-" : line.type === "hunk" ? "" : " "}{line.text || " "}</code></td></tr>)}</tbody></table> : <pre className="react-diff-preview">{item.diff || "该文件 diff 已截断或为空。"}</pre>}</article> })}</div>}</main>
      <AnnotationPanel
        fileLabel={activeView ? `${activeView.repo.repoName}/${activeView.file.path}` : ""}
        annotation={activeAnnotation}
        stale={Boolean(activeView && staleRepos.includes(activeView.repo.repoName))}
        activeNotes={hunkNotes.matched.filter((hit) => hit.lineIndex === activeHunkLine)}
        unmatchedNotes={hunkNotes.unmatched}
        onLocate={locateNote}
        onSave={saveAnnotation}
        saving={savingAnnotation}
 />
    </section>
  </PageChrome>
}

export function RequirementMergePage() {
  const params = new URLSearchParams(window.location.search)
  const reqId = params.get("id") || params.get("reqId") || ""
  const targetParam = params.get("target") || ""
  const targetFilter = targetParam === "test" || targetParam === "uat" ? targetParam : ""
  const requirements = RequirementsData()
  const req = requirements.data?.requirements.find((r) => r.id === reqId)
  const [target, setTarget] = useState<MergeTarget | "">(targetFilter as MergeTarget | "")
  const [payload, setPayload] = useState<MergeBranchPayload | null>(null)
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const loadStatus = async (nextTarget = target) => {
    if (!reqId) return
    setLoading(true)
    setError(null)
    try {
      const query = new URLSearchParams({ id: reqId })
      if (nextTarget) query.set("target", nextTarget)
      const next = await fetchJson<MergeBranchPayload>(`/api/requirement/merge-status?${query.toString()}`)
      setPayload(next)
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err))
    } finally {
      setLoading(false)
    }
  }
  useEffect(() => { loadStatus(targetFilter as MergeTarget | "") }, [reqId])
  const changeTarget = (next: MergeTarget | "") => {
    setTarget(next)
    const q = new URLSearchParams(window.location.search)
    q.set("id", reqId)
    if (next) q.set("target", next); else q.delete("target")
    window.history.replaceState(null, "", `/requirement-merge?${q.toString()}`)
    loadStatus(next)
  }
  const results = payload?.results || []
  const conflicts = results.filter((item) => item.status === "conflict")
  const title = req?.title || reqId || "分支合并"
  return <PageChrome icon={<GitMerge size={15} />} eyebrow="Merge" title={title} description="查看 Agent Panel 自动合并结果；冲突 worktree 会保留，人工或 agent 可按返回路径继续处理。" actions={<><a href={`/requirement?id=${encodeURIComponent(reqId)}`}><ArrowLeft size={15} />返回需求</a><button onClick={() => loadStatus()} disabled={loading}><RefreshCw size={15} className={loading ? "react-spin" : ""} />刷新状态</button></>}>
    <section className="react-panel react-merge-panel"><PanelHead kicker="Merge Status" title="环境合并状态" chip={payload?.status || "status"} />
      <div className="react-tab-row"><button className={target === "" ? "active" : ""} onClick={() => changeTarget("")}>全部</button><button className={target === "test" ? "active" : ""} onClick={() => changeTarget("test")}>test</button><button className={target === "uat" ? "active" : ""} onClick={() => changeTarget("uat")}>UAT</button></div>
      {requirements.error ? <ErrorCard error={requirements.error} /> : error ? <ErrorCard error={error} /> : loading ? <LoadingCard label="正在读取合并状态…" /> : results.length === 0 ? <EmptyCard>暂无合并状态；请先在需求详情页触发合并。</EmptyCard> : <>
        <div className="react-review-summary"><span>{results.length} repo/branch</span><span className="react-review-del">{conflicts.length} conflict</span><span>{payload?.generatedAt ? formatDateTime(payload.generatedAt) : "-"}</span></div>
        {conflicts.length ? <div className="react-drive-blockers"><strong>冲突处理提示</strong><ul><li>人工处理：进入下方 <code>worktreePath</code> 后解决冲突、提交并推送到目标分支。</li><li>Agent 处理：把接口返回的 <code>repoName / targetBranch / worktreePath / conflictFiles</code> 交给 agent，agent 可继续在该 worktree 解决冲突。</li></ul></div> : null}
        <div className="react-card-list react-merge-result-list">{results.map((item, index) => <article key={`${item.repoName}-${item.target}-${item.sourceBranch}-${index}`} className={`react-list-card react-merge-card react-merge-card-${item.status}`}><div><span className="react-card-id">{item.repoName} · {item.target}</span><h3>{item.sourceBranch} → {item.targetBranch || item.target}</h3><p>{item.message || mergeStatusLabel(item.status)}</p><div className="react-card-meta"><span>{mergeStatusLabel(item.status)}</span><span>{item.role || "repo"}</span><span>{item.projectPath || "path n/a"}</span></div>{item.worktreePath ? <code className="react-command">{item.worktreePath}</code> : null}{item.conflictFiles?.length ? <details className="react-review-commits" open><summary>冲突文件（{item.conflictFiles.length}）</summary><pre>{item.conflictFiles.join("\n")}</pre></details> : null}{item.warnings?.length ? <div className="react-drive-blockers"><strong>Warnings</strong><ul>{item.warnings.map((w) => <li key={w}>{w}</li>)}</ul></div> : null}</div><div className="react-card-side"><span className={`react-merge-status ${item.status}`}>{mergeStatusLabel(item.status)}</span><span className="react-muted">{item.targetBranch || "-"}</span></div></article>)}</div>
      </>}
    </section>
  </PageChrome>
}

function markdownPreview(content: string, expanded: boolean, max = 2600): { text: string; truncated: boolean } {
  const clean = (content || "").trim()
  if (!clean) return { text: "", truncated: false }
  if (expanded || clean.length <= max) return { text: clean, truncated: false }
  return { text: `${clean.slice(0, max)}\n…`, truncated: true }
}

function RequirementDocPanel({ id, req, docType, title, kicker, description, path, actions }: { id: string; req: Requirement; docType: string; title: string; kicker: string; description: string; path?: string; actions?: React.ReactNode }) {
  const [expanded, setExpanded] = useState(false)
  const doc = useFetch<RequirementDocPayload>(req.reqDir ? `/api/requirement/doc?id=${encodeURIComponent(req.id)}&file=${encodeURIComponent(docType)}` : null, [req.id, docType])
  const content = doc.data?.content || ""
  const preview = markdownPreview(content, expanded)
  const chip = doc.loading ? "loading" : doc.data?.exists ? doc.data.file : path ? "path only" : "missing"
  return <section id={id} className="react-panel react-doc-panel"><PanelHead kicker={kicker} title={title} chip={chip} />
    <p className="react-muted">{description}</p>
    <div className="react-actions">{actions}{path || doc.data?.path ? <code className="react-doc-path">{doc.data?.path || path}</code> : null}</div>
    {doc.error ? <p className="react-effort-error">加载失败：{doc.error}</p> : doc.loading ? <LoadingCard label="正在加载文档…" /> : preview.text ? <><pre className="react-doc-preview">{preview.text}</pre>{preview.truncated || expanded ? <div className="react-actions"><button type="button" onClick={() => setExpanded((v) => !v)}>{expanded ? "收起" : "展开全文"}</button></div> : null}</> : <p className="react-muted">暂无内容。可在需求澄清阶段生成或更新该文档。</p>}
  </section>
}

const TEXT_ATTACHMENT_EXTS = new Set(["sql", "txt", "md", "yaml", "yml", "json", "csv"])

function attachmentHumanBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`
}

function attachmentFormatTime(ms: number): string {
  if (!ms) return "-"
  const d = new Date(ms)
  const p = (n: number) => String(n).padStart(2, "0")
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())} ${p(d.getHours())}:${p(d.getMinutes())}:${p(d.getSeconds())}`
}

function RequirementAttachmentsPanel({ req }: { req: Requirement }) {
  const { data, error, loading } = useFetch<RequirementAttachmentsPayload>(
    req.reqDir ? `/api/requirement/attachments?id=${encodeURIComponent(req.id)}` : null,
    [req.id],
  )
  const rows = data?.attachments ?? []
  const [copiedKey, setCopiedKey] = useState<string | null>(null)
  const previewable = (row: RequirementAttachment) => TEXT_ATTACHMENT_EXTS.has(row.extension) && Boolean((row.sample || "").trim())
  const copyAttachment = async (row: RequirementAttachment) => {
    try {
      await navigator.clipboard.writeText(row.sample || "")
      setCopiedKey(row.filename)
      window.setTimeout(() => setCopiedKey((k) => (k === row.filename ? null : k)), 1600)
    } catch {
      setCopiedKey(null)
    }
  }
  const chip = loading ? "loading" : error ? "error" : `${rows.length} 个文件`
  return <section id="attachments" className="react-panel react-attachments-panel"><PanelHead kicker="Attachments" title="附件" chip={chip} />
    <p className="react-muted">展示需求目录 <code>attachments/</code> 下的非代码资产（排查数据、SQL、截图、导出件等），与上线清单相互独立；线上问题排查过程中沉淀的附件也在这里直接可见。</p>
    {error ? <p className="react-effort-error">附件加载失败：{error}</p> : loading ? <LoadingCard label="正在加载附件…" /> : rows.length === 0 ? <p className="react-muted">暂无附件。</p> : <>
      <div className="react-table-wrap react-attachment-wrap"><table className="react-code-file-table"><thead><tr><th>文件</th><th>类型/统计</th><th>大小</th><th>更新时间</th><th>路径</th></tr></thead><tbody>{rows.map((row) => <tr key={row.filename}><td><code>{row.filename}</code></td><td>{row.summary.length ? row.summary.join(" / ") : row.extension || "-"}</td><td>{attachmentHumanBytes(row.size)}</td><td>{attachmentFormatTime(row.mtime)}</td><td><code>{row.relativePath || row.path}</code></td></tr>)}</tbody></table></div>
      <div className="react-attachment-files">{rows.map((row) => {
        const canPreview = previewable(row)
        const summary = row.summary.length ? row.summary.join(" / ") : row.extension || "-"
        return <details key={row.filename} className="react-attachment-file"><summary><span className="react-attachment-name">{row.filename}</span><span className="react-attachment-badge">{summary}</span><span className="react-attachment-size">{attachmentHumanBytes(row.size)}</span>{canPreview ? null : <span className="react-attachment-note">二进制/不可预览</span>}</summary>{canPreview ? <div className="react-attachment-file-body"><div className="react-attachment-actions"><button type="button" className="react-copy-link-btn" onClick={() => copyAttachment(row)}>{copiedKey === row.filename ? "已复制" : "一键复制"}</button><code className="react-attachment-path">{row.path}</code></div><pre>{row.sample}</pre></div> : <div className="react-attachment-file-body"><code className="react-attachment-path">{row.path}</code><p className="react-muted">该文件为二进制或不可预览内容，请按路径打开核对。</p></div>}</details>
      })}</div>
    </>}
  </section>
}

function RequirementFilesPanel({ req }: { req: Requirement }) {
  const corePath = (file: string, path?: string) => path || (req.reqDir ? `${req.reqDir}/${file}` : "-")
  const existing = (file: string, path?: string): [string, string] | null => path ? [file, path] : null
  const groups: { title: string; note: string; rows: [string, string][] }[] = [
    { title: "核心文档", note: "新需求默认创建，Agent 执行过程中持续维护。", rows: [
      ["meta.md", corePath("meta.md", req.metaPath)],
      ["background.md", corePath("background.md", req.backgroundPath)],
      ["technical-plan.md", corePath("technical-plan.md", req.technicalPlanPath)],
      ["notes.md", corePath("notes.md", req.notesPath)],
    ] },
    { title: "按需阶段文件", note: "进入自测、发布、审查、经验总结等阶段后再创建。", rows: [
      existing("test.md", req.testPath),
      existing("test-scenario.md", req.testScenarioPath),
      existing("release-manifest.md", req.releaseManifestPath),
      existing("release-check.md", req.releaseCheckPath),
      existing("experience-summary.md", req.experienceSummaryPath),
      existing("troubleshooting.md", req.troubleshootingPath),
      existing("review.md", req.reviewPath),
    ].filter(Boolean) as [string, string][] },
    { title: "历史兼容文件", note: "旧需求存在时继续读取；新需求不再默认要求。", rows: [
      existing("alignment.md", req.alignmentPath),
      existing("impact.md", req.impactPath),
      existing("memory.md", req.memoryPath),
      existing("branch.md", req.branchPath),
      existing("config-changes.md", req.configPath),
    ].filter(Boolean) as [string, string][] },
  ]
  return <section className="react-panel"><PanelHead kicker="Files" title="需求文件" />
    {groups.map((group) => <div key={group.title} className="react-file-group"><strong>{group.title}</strong><p className="react-muted">{group.note}</p>{group.rows.length ? <div className="react-meta-grid">{group.rows.flatMap(([name, value]) => [<span key={`${group.title}-${name}-n`}>{name}</span>, <span key={`${group.title}-${name}-v`}>{value}</span>])}</div> : <p className="react-muted">暂无已创建文件。</p>}</div>)}
  </section>
}

export function RequirementPage() {
  const id = new URLSearchParams(window.location.search).get("id") || new URLSearchParams(window.location.search).get("reqId") || ""
  const { data, error, loading, refresh } = RequirementsData()
  const req = data?.requirements.find((r) => r.id === id)
  const harness = useFetch<HarnessCurrent>("/api/harness/current")
  const curHarness = harness.data?.harness ?? "pi"
  const isDsh = curHarness === "dsh-web" || curHarness === "dsh-tui"
  const isDshWeb = curHarness === "dsh-web"
  const resolved = useFetch<{ sessions: SessionInfo[]; missing: string[] }>(req?.sessionIds?.length ? `/api/sessions/resolve?ids=${encodeURIComponent(req.sessionIds.join(","))}` : null, [req?.id])
  /** In dsh mode only dsh sessions are shown; resolve returns each session's harness via `agent`. */
  const dshSessionIds = useMemo(() => {
    if (!isDsh || !req?.sessionIds?.length) return req?.sessionIds ?? []
    const agents = new Map((resolved.data?.sessions ?? []).map((s) => [s.id.replace(/^session-/, ""), s.agent]))
    return req.sessionIds.filter((sid) => agents.get(sid.replace(/^session-/, "")) === "dsh")
  }, [curHarness, req?.sessionIds, resolved.data])
  const [note, setNote] = useState("")
  const [status, setStatus] = useState<ReqStatus | "">("")
  const [category, setCategory] = useState<ReqCategory | "">("")
  const [source, setSource] = useState<string>("")
  const [savingStatus, setSavingStatus] = useState(false)
  const [savingCategory, setSavingCategory] = useState(false)
  const [savingSource, setSavingSource] = useState(false)
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
  const statusOptions = isIssueFamily ? ISSUE_STATUSES : REQ_FLOW_STATUSES
  const [fixReqId, setFixReqId] = useState<string | null>(null)
  const convertIssue = async () => {
    if (!req || req.category !== "线上问题") return
    try {
      const res = await postJson<{ ok: boolean; fixReqId?: string }>("/api/requirement/convert-issue", { reqId: req.id, note: note || "线上问题转代码修复需求" })
      setFixReqId(res.fixReqId || null)
      setStatusMessage(res.fixReqId ? `已创建代码修复需求 ${res.fixReqId} 并绑定本问题；需求进入经验总结后自动推进本问题到已修复` : "已转普通需求流程")
      refresh()
    } catch (err) {
      setStatusMessage(`转换失败：${err instanceof Error ? err.message : String(err)}`)
    }
  }
  const submitStatus = async () => {
    if (!req || !status || savingStatus) return
    setSavingStatus(true)
    setStatusMessage(null)
    try {
      await postForm("/api/requirement/status", { reqId: req.id, status, note })
      setStatusMessage("状态已保存")
      refresh()
    } catch (err) {
      setStatusMessage(`状态保存失败：${err instanceof Error ? err.message : String(err)}`)
    } finally {
      setSavingStatus(false)
    }
  }
  const submitCategory = async () => {
    if (!req || !category || savingCategory) return
    setSavingCategory(true)
    try { await postForm("/api/requirement/category", { reqId: req.id, category }); refresh() }
    finally { setSavingCategory(false) }
  }
  const submitSource = async () => {
    if (!req || !source || savingSource) return
    setSavingSource(true)
    try { await postForm("/api/requirement/update", { reqId: req.id, source }); setSource(""); refresh() }
    finally { setSavingSource(false) }
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
  return <PageChrome icon={<GitBranch size={15} />} eyebrow="Requirement" title={req?.title || id || "Requirement"} description={req?.description || "需求详情、状态流转、技术方案、上线清单、业务背景、经验总结与关联 session。"} actions={<><a href="/projects"><ArrowLeft size={15} />返回需求列表</a>{req ? <a href="#technical-plan"><FileCode2 size={15} />技术方案</a> : null}{req ? <a href="#release-manifest"><AlertTriangle size={15} />上线清单</a> : null}{req ? <a href="#attachments"><Paperclip size={15} />附件</a> : null}{req ? <a href={`/requirement-doc?id=${encodeURIComponent(req.id)}&doc=background&title=${encodeURIComponent("业务背景文档")}`}><Library size={15} />业务背景</a> : null}{req ? <a href="#experience-summary"><Lightbulb size={15} />经验总结</a> : null}{req ? <a href="#code-review"><GitBranch size={15} />代码差异</a> : null}</>}>
    {error ? <ErrorCard error={error} /> : loading ? <LoadingCard /> : !req ? <EmptyCard>需求不存在：{id}</EmptyCard> : <div className="react-detail-grid">
      <section className="react-panel"><PanelHead kicker="Overview" title="需求信息" chip={<>{statusPill(req.status)}{experienceSummaryPill(req)}</>} /><div className="react-meta-grid"><span>Req ID <code>{req.id}</code></span><span>项目 {projectsOf(req)}</span><span>创建 {formatDate(req.createdAt)}</span><span>更新 {relAge(req.updatedAt)}</span><span>预计发版 {req.planRelease || "unknown"}</span><span>目录 {req.reqDir || "-"}</span><span>类别 {req.category || "需求"}</span><span>来源 {req.source ?? "产品推动"}{req.source === "开发推动" ? "（需测试场景文档）" : ""}</span></div><PlanReleaseForm req={req} onSaved={refresh} /><p className="react-detail-desc">{req.description || "暂无描述"}</p></section>
      <RequirementDocPanel id="technical-plan" req={req} docType="technical-plan" title="技术方案" kicker="Implementation Plan" path={req.technicalPlanPath} description="Agent 执行需求过程中持续维护：先看总体实现路径、影响范围、风险、灰度/回滚和验证计划，再进入代码差异人工审查。" actions={<><a href={`/api/requirement/context?id=${encodeURIComponent(req.id)}&intent=design&tokens=req.technicalPlan,req.impact,req.branchScope,req.codeReview&budget=4000&format=html`} target="_blank" rel="noreferrer">方案上下文</a></>} />
      {req.source === "开发推动" && !isOnlineIssue ? <RequirementDocPanel id="test-scenario" req={req} docType="test-scenario" title="测试场景" kicker="Test Scenario" path={req.testScenarioPath} description="开发推动的需求，测试无法向产品确认测试范围，本档由开发负责：需求说明（这个需求是干嘛的）+ 开发评估的测试范围 + 测试覆盖场景，让测试自主评估与补充用例；进入「测试中」前强制校验。" actions={<><a href={`/requirement-doc?id=${encodeURIComponent(req.id)}&doc=test-scenario&title=${encodeURIComponent("测试场景文档")}`}><Library size={15} />整页查看</a><a href={`/api/requirement/context?id=${encodeURIComponent(req.id)}&intent=design&tokens=req.technicalPlan,req.branchScope&budget=4000&format=html`} target="_blank" rel="noreferrer">方案上下文</a></>} /> : null}
      <RequirementDocPanel id="release-manifest" req={req} docType="release-manifest" title="上线清单" kicker="Release Manifest" path={req.releaseManifestPath} description="贯穿需求全流程维护：集中展示 DB 表、配置、Topic/Group、Job、开关、接口和上线人工动作，避免发布时遗漏。" actions={<><a href={`/api/requirement/context?id=${encodeURIComponent(req.id)}&intent=release-check&tokens=req.releaseManifest,req.attachments,req.configChanges,req.branchScope&budget=5000&format=html`} target="_blank" rel="noreferrer">清单上下文</a></>} />
      <RequirementAttachmentsPanel req={req} />
      <section id="business-background" className="react-panel react-doc-panel"><PanelHead kicker="Business Context" title="业务背景文档" chip="新页面查看" /><p className="react-muted">给不熟悉业务的开发/测试快速理解背景，也作为后续经验总结的参考材料。点击下方按钮在独立页面查看渲染后的完整文档。</p><div className="react-actions"><a href={`/requirement-doc?id=${encodeURIComponent(req.id)}&doc=background&title=${encodeURIComponent("业务背景文档")}`}><Library size={15} />查看业务背景文档</a><a href="/business-knowledge"><Library size={15} />业务知识库</a><a href={`/api/requirement/context?id=${encodeURIComponent(req.id)}&intent=clarification&budget=3000&format=html`} target="_blank" rel="noreferrer">澄清上下文</a></div></section>
      <RequirementDocPanel id="experience-summary" req={req} docType="experience-summary" title="经验总结闭环" kicker="Capability Evolution" path={req.experienceSummaryPath} description="记录本次需求暴露出的业务知识、经验、skill 和流程改进，让下一次需求执行更快更稳。" actions={<><a href="/experiences"><Lightbulb size={15} />需求总结</a><a href={`/requirement-doc?id=${encodeURIComponent(req.id)}&doc=experience-summary&title=${encodeURIComponent("经验总结报告")}`}><Library size={15} />查看总结报告</a>{req.experienceSummaryJob?.sessionId ? <a href={`/session?id=${encodeURIComponent(req.experienceSummaryJob.sessionId)}`}>总结 Agent</a> : null}<button type="button" onClick={retrySummary} disabled={summaryWorking}>{summaryWorking ? "派发中…" : req.experienceSummaryJob?.status === "failed" ? "重试总结" : "重新总结"}</button><a href={`/api/requirement/experience-summary-context?id=${encodeURIComponent(req.id)}&limit=200`} target="_blank" rel="noreferrer">候选汇总</a><a href={`/api/requirement/context?id=${encodeURIComponent(req.id)}&intent=experience-summary&budget=3000&format=html`} target="_blank" rel="noreferrer">总结上下文</a></>} />
      {isIssueFamily ? <RequirementDocPanel id="incident" req={req} docType="incident" title="问题档案" kicker="Incident Profile" path={req.incidentPath} description="回答这个问题「是什么」：现象描述、发现渠道、环境、首次/最近发生时间窗口（带时区绝对时间）、影响范围（单量/仓库/租户/用户）、复现步骤和时间线；排查中创建并持续更新，是问题身份证。" actions={<><a href={`/requirement-doc?id=${encodeURIComponent(req.id)}&doc=incident&title=${encodeURIComponent("问题档案")}`}><Library size={15} />整页查看</a><a href={`/api/requirement/context?id=${encodeURIComponent(req.id)}&intent=progress&budget=3000&format=html`} target="_blank" rel="noreferrer">排查上下文</a></>} /> : null}
      {isIssueFamily ? <RequirementDocPanel id="root-cause" req={req} docType="root-cause" title="根因与修复决策" kicker="Root Cause" path={req.rootCausePath} description="回答「为什么」和「怎么办」：根因（直接+深层）、证据链（每条必须附可复核线索：日志=时间范围+tid/关键字，DB=验证 SQL，代码=应用+文件+可搜关键字，配置=环境+key）、影响面、修复路径决策、临时处置与回滚；线上问题推进到「已定位」前必须填写，测试问题建议按需维护。" actions={<><a href={`/requirement-doc?id=${encodeURIComponent(req.id)}&doc=root-cause&title=${encodeURIComponent("根因与修复决策")}`}><Library size={15} />整页查看</a><a href={`/api/requirement/context?id=${encodeURIComponent(req.id)}&intent=progress&budget=3000&format=html`} target="_blank" rel="noreferrer">排查上下文</a></>} /> : null}
      {isIssueFamily ? <RequirementDocPanel id="troubleshooting" req={req} docType="troubleshooting" title="排查经验" kicker="Troubleshooting" path={req.troubleshootingPath} description="沉淀本问题的排查路径与修复方案：怎么排查（证据链、定位步骤、工具/命令）、怎么修复（方案、验证）、根因与复用清单；线上问题推进到「已复盘」前必须填写并回填经验库条目 id，测试问题按需沉淀。" actions={<><a href="/experiences"><Lightbulb size={15} />经验库</a><a href={`/requirement-doc?id=${encodeURIComponent(req.id)}&doc=troubleshooting&title=${encodeURIComponent("排查经验")}`}><Library size={15} />整页查看</a><a href={`/api/requirement/context?id=${encodeURIComponent(req.id)}&intent=progress&budget=3000&format=html`} target="_blank" rel="noreferrer">排查上下文</a></>} /> : null}
      <CodeReviewPanel req={req} />
      <MergeBranchPanel req={req} />
      <ProdMrPanel req={req} />
      {!isIssueFamily ? <LinkedIssuesPanel req={req} issues={data?.requirements || []} onSaved={refresh} /> : null}
      <OnesPanel req={req} onSaved={refresh} />
      <section className="react-panel"><PanelHead kicker="Status" title={isOnlineIssue ? "线上问题状态" : isIssueFamily ? "测试问题状态" : "状态切换"} /><p className="react-muted">{isOnlineIssue ? "线上问题专用流程：排查中 → 已定位 → 已修复 → 已复盘；已定位→已修复 有两条路径：数据修复直接推进已修复；代码修复创建普通需求并在需求「关联线上问题」面板绑定本问题，需求进入经验总结后系统自动推进关联问题。有价值的问题在「排查经验」面板沉淀 troubleshooting.md（怎么排查 + 怎么修复），进入已复盘前强制校验；无沉淀价值直接已关闭。代码修复可点「创建修复需求」：自动创建并绑定本问题的普通需求，需求进入经验总结后本问题自动推进已修复。" : isIssueFamily ? "测试问题流程：排查中 → 已定位 → 已修复 → 已复盘，与线上问题共用轻量状态机，但硬门禁更轻：进入已定位/已复盘不强制 root-cause.md 和排查经验，小问题可直接修复后推进，有复用价值时按需沉淀。测试问题不绑定普通需求；需要常规开发承接时创建 category=需求 的需求跟进。" : "新版流程：需求澄清 → 开发中 → 自测中 → 测试中 → 发布就绪 → 经验总结 → 已完成。自测中推进到测试中前必须通过代码审查门禁，旧状态会自动兼容映射。开发推动（source=开发推动）的需求进入测试中前必须先完成「测试场景」文档，否则状态接口会拒绝。"}</p><div className="react-inline-form"><select value={status} onChange={(e) => { setStatus(e.target.value as ReqStatus); setStatusMessage(null) }}><option value="">选择状态</option>{statusOptions.map((s) => <option key={s} value={s}>{s}</option>)}</select><input value={note} onChange={(e) => setNote(e.target.value)} placeholder="备注" /><button onClick={submitStatus} disabled={!status || savingStatus}>{savingStatus ? "保存中…" : "保存状态"}</button>{isOnlineIssue ? <button type="button" onClick={convertIssue}>创建修复需求</button> : null}</div>{statusMessage ? <p className={statusMessage.startsWith("状态保存失败") || statusMessage.startsWith("转换失败") ? "react-effort-error" : "react-save-hint"}>{statusMessage}{fixReqId ? <> <a href={`/requirement?id=${encodeURIComponent(fixReqId)}`}>打开修复需求 →</a></> : null}</p> : null}<div className="react-inline-form react-category-form"><label>类别</label><select value={category} onChange={(e) => setCategory(e.target.value as ReqCategory)}><option value="">{req.category ?? "需求"}</option>{REQ_CATEGORIES.map((c) => <option key={c} value={c}>{c}</option>)}</select><button onClick={submitCategory} disabled={!category || savingCategory}>{savingCategory ? "保存中…" : "保存类别"}</button><label>推动方</label><select value={source} onChange={(e) => setSource(e.target.value)}><option value="">{req.source ?? "产品推动"}</option>{REQ_SOURCES.map((s) => <option key={s} value={s}>{s}</option>)}</select><button onClick={submitSource} disabled={!source || savingSource} title={req.source === "开发推动" ? "切回产品推动" : "切为开发推动：进入测试中前必须先完成测试场景文档"}>{savingSource ? "保存中…" : "保存推动方"}</button></div></section>
      <section className="react-panel"><PanelHead kicker="Sessions" title="关联 Session" chip={req.sessionIds?.length ? <button type="button" className="react-chip-count-btn" onClick={() => setShowSessions(true)} title="查看全部关联 session"><List size={13} />{isDsh ? dshSessionIds.length : req.sessionIds.length}</button> : "0"} />{isDsh ? dshSessionIds.length ? <SessionChipList sessionIds={dshSessionIds} /> : <p className="react-muted">暂无关联的 dsh session。</p> : req.sessionIds?.length ? <SessionChipList sessionIds={req.sessionIds} /> : <p className="react-muted">暂无关联 session。</p>}<div className="react-actions">{isDshWeb ? <button onClick={newDshWebSession}>为需求开启 dsh web session</button> : <><button type="button" onClick={() => copyLatestCommand(false)} title="复制最新终端命令；session 被使用后再次点击会自动换新 session id"><Copy size={13} />{copyFeedback === "copied" ? "已复制" : "复制命令"}</button><button type="button" onClick={() => copyLatestCommand(true)} title="无视使用状态强制更换 session id，并复制新命令；用于“是否已使用”判断失误时兜底"><RefreshCw size={13} />{copyFeedback === "refreshed" ? "已刷新并复制" : "强制刷新"}</button></>}</div>{isDshWeb ? <div className="react-command-wrap"><code className="react-command">/requirement-bind {req.id}</code><button type="button" className="react-copy-link-btn" onClick={copyBindCommand} title="复制绑定命令，到 dsh web 聊天框粘贴"><Copy size={13} />{bindCmdCopied ? "已复制" : "复制"}</button></div> : shownCommand ? <><div className="react-command-wrap"><code className="react-command">{shownCommand}</code><button type="button" className="react-copy-link-btn" onClick={copyShownCommand} title="再次复制当前显示的命令"><Copy size={13} />{copied ? "已复制" : "复制"}</button></div><p className="react-muted">{pendingSession.data?.pending ? (pendingSession.data.pending.used ? "该命令的 session 已被使用：再次点击“复制命令”会自动换新 session id。" : "重复点击“复制命令”复用同一 session id；命令被使用后自动换新。判断失误时用“强制刷新”。") : "尚未生成命令：点击“复制命令”会生成并复制；命令被使用后再次点击会自动换新。"}</p></> : null}{!isDshWeb && copyError ? <p className="react-effort-error">{copyError}</p> : null}{isDshWeb ? dshSession ? <div className="react-dsh-session"><p className="react-save-hint">已在 dsh web（{dshSession.url || "3080"}）创建并绑定 session：<code>{dshSession.sessionId}</code></p>{dshSession.url ? <a className="react-link" href={dshSession.url} target="_blank" rel="noreferrer">打开 dsh web GUI 继续会话 ↗</a> : null}<p className="react-muted">需求上下文会在下一条消息注入该 session；也可复制上面的绑定命令到任一 dsh session 聊天框。</p></div> : <p className="react-muted">复制上面的绑定命令，粘贴到 dsh web 聊天框（任意 session），即可把该 session 关联到本需求。</p> : null}{showSessions && (isDsh ? dshSessionIds.length : req.sessionIds?.length) ? <SessionListModal sessionIds={isDsh ? dshSessionIds : req.sessionIds} harness={curHarness} onClose={() => setShowSessions(false)} /> : null}</section>
      <RequirementFilesPanel req={req} />
    </div>}
  </PageChrome>
}
