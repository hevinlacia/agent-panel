import { AlertTriangle, ArrowLeft, Copy, FileCode2, GitBranch, GitMerge, Library, Lightbulb, List, RefreshCw, Search } from "lucide-react"
import { useEffect, useMemo, useState } from "react"
import type { CodeReviewPayload, CodeReviewSnapshot, MasterDiffPayload, MergeBranchPayload, MergeKindOptions, MergeOptionsPayload, MergeRepoKind, MergeTarget, ProdMrPayload, ProdMrResult, ReqCategory, ReqStatus, Requirement, RequirementDocPayload, ReviewGatePayload, SyncBasePayload } from "../types"
import { fetchJson, postForm, postJson, useFetch } from "../lib/api"
import { formatDate, formatDateTime, relAge } from "../lib/format"
import { ISSUE_STATUSES, REQ_CATEGORIES, REQ_FLOW_STATUSES } from "../lib/requirements"
import { compactPath, diffDomId, parseUnifiedDiffFiles, reviewStats, shortFileName } from "../lib/diff"
import { Markdown } from "../markdown"
import { experienceSummaryPill, onesBadge, projectsOf, statusPill } from "../features/requirements/badges"
import { EmptyCard, ErrorCard, LoadingCard, PageChrome, PanelHead } from "../components/ui"
import { RequirementsData } from "./projects"
import { SessionChipList, SessionListModal } from "./sessions"

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
  return <section id="code-review" className="react-panel react-code-review-panel"><PanelHead kicker="Code Review Gate" title="代码审查门禁" chip={gate.data?.gate?.label || (gate.loading ? "loading" : "gate")} />
    <div className={`react-review-gate react-review-gate-${gate.data?.gate?.status || "unknown"}`}><strong>{gate.data?.gate?.label || "读取中"}</strong><span>{gate.data?.gate?.reason || "自测中推进到测试中前必须完成代码审查门禁。"}</span>{gate.data?.gate?.source ? <em>source: {gate.data.gate.source}</em> : null}</div>
    {gateRiskTags.length ? <div className="react-review-risk-tags"><strong>风险标签</strong>{gateRiskTags.map((tag) => <span key={tag} className="react-review-tag">{tag}</span>)}</div> : null}
    {inventoryRisk ? <div className="react-review-gate react-review-gate-blocked"><strong>⚠ 库存高危风险</strong><span>本次改动命中库存相关文件/表，门禁强制要求库存账本专项评估：单据活跃/死亡、DB 库存(onHand/allocated/临时库位/回库单)、redis 可用量(建单-、真取消+、恢复-、回退保持占用)、重复释放、遗漏占用、幂等、验证证据(DB/redis/日志/单测)。未补充前即使 PASS 也不通过。</span></div> : null}
    {gateStale ? <div className="react-drive-blockers"><strong>审查快照需刷新覆盖</strong>{staleRepos.length ? <ul>{staleRepos.map((repo) => <li key={`${repo.repoName}-${repo.branch}`}><code>{repo.repoName}</code> / <code>{repo.branch}</code>：{(repo.reviewedTargetCommit || "").slice(0, 12) || "reviewed?"} → {(repo.currentTargetCommit || "").slice(0, 12) || "current?"}</li>)}</ul> : null}<p>优先生成增量审查包，只审上次已审 commit 到当前 HEAD 的新增 diff；非线性历史再回退全量审查。</p></div> : null}
    {gate.data?.gate?.actions?.length ? <div className="react-drive-blockers"><strong>门禁动作</strong><ul>{gate.data.gate.actions.map((item) => <li key={item}>{item}</li>)}</ul></div> : null}
    {gate.error ? <p className="react-effort-error">门禁加载失败：{gate.error}</p> : null}
    <div className="react-actions"><button onClick={refreshScan} disabled={!canScan || refreshing}><RefreshCw size={15} className={refreshing ? "react-spin" : ""} />{review ? "刷新全量差异" : "生成代码差异"}</button>{gateStale ? <button onClick={refreshIncrementalScan} disabled={!canScan || refreshingIncremental}><RefreshCw size={15} className={refreshingIncremental ? "react-spin" : ""} />生成增量审查包</button> : null}<button onClick={syncBase} disabled={!canScan || syncing} title="fetch 远端生产分支并 reset 本地 master/production 到最新,工作区有改动时自动跳过"><RefreshCw size={15} className={syncing ? "react-spin" : ""} />{syncing ? "同步中…" : "同步生产基线"}</button>{review ? <button onClick={() => setShowDiff((v) => !v)}>{showDiff ? "隐藏 unified diff" : "展示 unified diff"}</button> : null}<a href={`/requirement-diff?id=${encodeURIComponent(req.id)}&base=origin%2Fmaster`}><GitBranch size={15} />打开分支差异页</a></div>
    {syncPayload?.results?.length ? <details className="react-review-repo" open><summary><span><strong>生产基线同步</strong><em>{formatDateTime(syncPayload.generatedAt)}</em></span><span className="react-review-size">{syncPayload.results.filter((r) => r.ok).length}/{syncPayload.results.length} ok</span></summary><div className="react-table-wrap react-code-file-wrap"><table className="react-code-file-table"><thead><tr><th>应用</th><th>本地分支</th><th>状态</th><th>before</th><th>after</th><th>说明</th></tr></thead><tbody>{syncPayload.results.map((r) => <tr key={r.repoName}><td><strong>{r.repoName}</strong></td><td><code>{r.localBranch || r.baseRef || "-"}</code></td><td><span className={`react-merge-status ${r.ok ? "merged" : "conflict"}`}>{r.status}</span></td><td><code>{r.beforeCommit || "-"}</code></td><td><code>{r.afterCommit || "-"}</code></td><td>{r.message}{r.warnings?.length ? <em>{r.warnings.join("; ")}</em> : null}</td></tr>)}</tbody></table></div></details> : null}
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
        {repo.files?.length ? <div className="react-table-wrap react-code-file-wrap"><table className="react-code-file-table"><thead><tr><th>文件</th><th>状态</th><th>增删</th><th>风险</th></tr></thead><tbody>{repo.files.map((file) => <tr key={file.path}><td><code>{file.path}</code></td><td>{file.status}</td><td><span className="react-review-add">+{file.additions}</span> / <span className="react-review-del">-{file.deletions}</span></td><td>{file.riskTags?.length ? file.riskTags.map((tag) => <span key={tag} className="react-review-tag" data-risk={tag}>{tag}</span>) : <span className="react-muted">-</span>}</td></tr>)}</tbody></table></div> : <p className="react-muted">没有文件级差异。</p>}
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
    <p className="react-muted">按 <code>branches.json</code> 中每个应用的每个需求分支创建生产 MR；后端目标 <code>master</code>，前端目标 <code>production</code>。生成前先比对需求分支与生产分支的差异，<strong>无差异的仓库自动跳过 MR</strong>。再次点击会重新扫描并复用已存在的 open MR。</p>
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
  "review": "代码审查",
  "release-check": "发布预检",
  "test": "测试用例",
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
  const [review, setReview] = useState<CodeReviewSnapshot | null>(null)
  const files = useMemo(() => parseUnifiedDiffFiles(review), [review])
  const stats = reviewStats(review)
  const [activeKey, setActiveKey] = useState("")
  useEffect(() => {
    if (!reqId) return
    let cancelled = false
    setLoadingDiff(true)
    setError(null)
    postForm<MasterDiffPayload>("/api/requirement/master-diff", { reqId, baseRef })
      .then((payload) => { if (!cancelled) setReview(payload.review || null) })
      .catch((err) => { if (!cancelled) setError(err instanceof Error ? err.message : String(err)) })
      .finally(() => { if (!cancelled) setLoadingDiff(false) })
    return () => { cancelled = true }
  }, [reqId, baseRef])
  useEffect(() => {
    if (!files.length) { setActiveKey(""); return }
    const exists = files.some((item) => `${item.repo.repoName}:${item.file.path}` === activeKey)
    if (!exists) setActiveKey(`${files[0].repo.repoName}:${files[0].file.path}`)
  }, [files, activeKey])
  const activeIndex = Math.max(0, files.findIndex((item) => `${item.repo.repoName}:${item.file.path}` === activeKey))
  const scrollToFile = (key: string) => {
    setActiveKey(key)
    document.getElementById(diffDomId(key))?.scrollIntoView({ behavior: "smooth", block: "start" })
  }
  const changeBase = (next: string) => {
    setBaseRef(next)
    const q = new URLSearchParams(window.location.search)
    q.set("id", reqId)
    q.set("base", next)
    window.history.replaceState(null, "", `/requirement-diff?${q.toString()}`)
  }
  const title = req?.title || reqId || "分支差异"
  return <PageChrome icon={<GitBranch size={15} />} eyebrow="Diff" title={title} description="按需求分支和指定基准分支生成代码差异，左侧选择文件，中间查看改动内容。" actions={<><a href={`/requirement?id=${encodeURIComponent(reqId)}`}><ArrowLeft size={15} />返回需求</a><button onClick={() => changeBase(baseRef)} disabled={loadingDiff}><RefreshCw size={15} className={loadingDiff ? "react-spin" : ""} />刷新 diff</button></>}>
    <section className="react-diff-shell">
      <aside className="react-diff-sidebar"><div className="react-diff-compare"><span>Compare</span><select value={baseRef} onChange={(e) => changeBase(e.target.value)}><option value="origin/master">origin/master</option><option value="origin/production">origin/production</option><option value="master">master</option><option value="production">production</option></select><em>and latest version</em></div><label className="react-diff-search"><Search size={14} /><input placeholder="Search files (Ctrl+P)" onChange={(e) => { const hit = files.find((f) => f.file.path.toLowerCase().includes(e.target.value.toLowerCase())); if (hit && e.target.value) scrollToFile(`${hit.repo.repoName}:${hit.file.path}`) }} /></label><div className="react-diff-file-list">{[...new Set(review?.repos?.map((r) => r.repoName) || [])].map((repoName) => {
          const repo = review?.repos?.find((r) => r.repoName === repoName)
          const repoFiles = files.filter((f) => f.repo.repoName === repoName)
          return <details key={repoName} className="react-diff-repo-group" open>
            <summary><strong>{repoName}</strong><em>base: {repo?.baseRef || baseRef}</em></summary>
            {repoFiles.length ? repoFiles.map((item) => { const key = `${item.repo.repoName}:${item.file.path}`; return <button key={key} className={key === activeKey ? "active" : ""} onClick={() => scrollToFile(key)}><FileCode2 size={14} /><span><strong>{shortFileName(item.file.path)}</strong><small>{compactPath(item.file.path)}</small></span><em><b>+{item.file.additions}</b> <i>-{item.file.deletions}</i></em></button> }) : <p className="react-muted" style={{padding: '8px'}}>暂无文件差异</p>}
          </details>
        })}</div></aside>
      <main className="react-diff-main"><div className="react-diff-toolbar"><div><strong>{stats.fileCount} files</strong><span className="react-review-add">+{stats.additions}</span><span className="react-review-del">-{stats.deletions}</span>{review?.updatedAt ? <span>生成 {formatDateTime(review.updatedAt)}</span> : null}</div><span>{activeIndex + 1}/{Math.max(files.length, 1)}</span></div>{requirements.error ? <ErrorCard error={requirements.error} /> : error ? <ErrorCard error={error} /> : loadingDiff ? <LoadingCard label="正在生成分支差异…" /> : files.length === 0 ? <EmptyCard>没有可展示的文件级差异。</EmptyCard> : files.map((item) => { const key = `${item.repo.repoName}:${item.file.path}`; return <article key={key} id={diffDomId(key)} className="react-diff-file-card"><header><div><FileCode2 size={16} /><strong>{item.repo.repoName}/{item.file.path}</strong><em className="react-diff-base-label">vs {item.repo.baseRef || review?.baseRef || "?"}</em></div><span><b>+{item.file.additions}</b><i>-{item.file.deletions}</i></span></header>{item.lines.length ? <table className="react-diff-code"><tbody>{item.lines.map((line, i) => <tr key={i} className={`react-diff-line-${line.type}`}><td>{line.oldNo}</td><td>{line.newNo}</td><td><code>{line.type === "add" ? "+" : line.type === "del" ? "-" : line.type === "hunk" ? "" : " "}{line.text || " "}</code></td></tr>)}</tbody></table> : <pre className="react-diff-preview">{item.diff || "该文件 diff 已截断或为空。"}</pre>}</article> })}</main>
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
      existing("release-manifest.md", req.releaseManifestPath),
      existing("release-check.md", req.releaseCheckPath),
      existing("experience-summary.md", req.experienceSummaryPath),
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
  const [note, setNote] = useState("")
  const [status, setStatus] = useState<ReqStatus | "">("")
  const [category, setCategory] = useState<ReqCategory | "">("")
  const [savingStatus, setSavingStatus] = useState(false)
  const [savingCategory, setSavingCategory] = useState(false)
  const [summaryWorking, setSummaryWorking] = useState(false)
  const [statusMessage, setStatusMessage] = useState<string | null>(null)
  const [command, setCommand] = useState("")
  const [copied, setCopied] = useState(false)
  const [showSessions, setShowSessions] = useState(false)
  const statusOptions = req?.category === "线上问题" ? ISSUE_STATUSES : REQ_FLOW_STATUSES
  const isOnlineIssue = req?.category === "线上问题"
  const convertIssue = async () => {
    if (!req || req.category !== "线上问题") return
    await postForm("/api/requirement/convert-issue", { reqId: req.id, note: note || "线上问题转需求" })
    setStatusMessage("已转为普通需求流程")
    refresh()
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
  const newSession = async () => {
    if (!req) return
    const res = await postForm<{ command: string }>("/api/requirement/new-session", { reqId: req.id })
    setCommand(res.command)
    setCopied(false)
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
  const copyCommand = async () => {
    if (!command) return
    try {
      await navigator.clipboard.writeText(command)
      setCopied(true)
      window.setTimeout(() => setCopied(false), 1600)
    } catch (err) {
      setCopied(false)
      // eslint-disable-next-line no-console
      console.error("复制失败", err)
    }
  }
  return <PageChrome icon={<GitBranch size={15} />} eyebrow="Requirement" title={req?.title || id || "Requirement"} description={req?.description || "需求详情、状态流转、技术方案、上线清单、业务背景、经验总结与关联 pi session。"} actions={<><a href="/projects"><ArrowLeft size={15} />返回需求列表</a>{req ? <a href="#technical-plan"><FileCode2 size={15} />技术方案</a> : null}{req ? <a href="#release-manifest"><AlertTriangle size={15} />上线清单</a> : null}{req ? <a href={`/requirement-doc?id=${encodeURIComponent(req.id)}&doc=background&title=${encodeURIComponent("业务背景文档")}`}><Library size={15} />业务背景</a> : null}{req ? <a href="#experience-summary"><Lightbulb size={15} />经验总结</a> : null}{req ? <a href="#code-review"><GitBranch size={15} />代码差异</a> : null}</>}>
    {error ? <ErrorCard error={error} /> : loading ? <LoadingCard /> : !req ? <EmptyCard>需求不存在：{id}</EmptyCard> : <div className="react-detail-grid">
      <section className="react-panel"><PanelHead kicker="Overview" title="需求信息" chip={<>{statusPill(req.status)}{experienceSummaryPill(req)}</>} /><div className="react-meta-grid"><span>Req ID <code>{req.id}</code></span><span>项目 {projectsOf(req)}</span><span>创建 {formatDate(req.createdAt)}</span><span>更新 {relAge(req.updatedAt)}</span><span>目录 {req.reqDir || "-"}</span><span>类别 {req.category || "需求"}</span></div><p className="react-detail-desc">{req.description || "暂无描述"}</p></section>
      <RequirementDocPanel id="technical-plan" req={req} docType="technical-plan" title="技术方案" kicker="Implementation Plan" path={req.technicalPlanPath} description="Agent 执行需求过程中持续维护：先看总体实现路径、影响范围、风险、灰度/回滚和验证计划，再进入代码差异人工审查。" actions={<><a href={`/api/requirement/context?id=${encodeURIComponent(req.id)}&intent=design&tokens=req.technicalPlan,req.impact,req.branchScope,req.codeReview&budget=4000&format=html`} target="_blank" rel="noreferrer">方案上下文</a></>} />
      <RequirementDocPanel id="release-manifest" req={req} docType="release-manifest" title="上线清单" kicker="Release Manifest" path={req.releaseManifestPath} description="贯穿需求全流程维护：集中展示 DB 表、配置、Topic/Group、Job、开关、接口和上线人工动作，避免发布时遗漏。" actions={<><a href={`/api/requirement/context?id=${encodeURIComponent(req.id)}&intent=release-check&tokens=req.releaseManifest,req.attachments,req.configChanges,req.branchScope&budget=5000&format=html`} target="_blank" rel="noreferrer">清单上下文</a></>} />
      <section id="business-background" className="react-panel react-doc-panel"><PanelHead kicker="Business Context" title="业务背景文档" chip="新页面查看" /><p className="react-muted">给不熟悉业务的开发/测试快速理解背景，也作为后续经验总结的参考材料。点击下方按钮在独立页面查看渲染后的完整文档。</p><div className="react-actions"><a href={`/requirement-doc?id=${encodeURIComponent(req.id)}&doc=background&title=${encodeURIComponent("业务背景文档")}`}><Library size={15} />查看业务背景文档</a><a href="/business-knowledge"><Library size={15} />业务知识库</a><a href={`/api/requirement/context?id=${encodeURIComponent(req.id)}&intent=clarification&budget=3000&format=html`} target="_blank" rel="noreferrer">澄清上下文</a></div></section>
      <RequirementDocPanel id="experience-summary" req={req} docType="experience-summary" title="经验总结闭环" kicker="Capability Evolution" path={req.experienceSummaryPath} description="记录本次需求暴露出的业务知识、经验、skill 和流程改进，让下一次需求执行更快更稳。" actions={<><a href="/experiences"><Lightbulb size={15} />需求总结</a><a href={`/requirement-doc?id=${encodeURIComponent(req.id)}&doc=experience-summary&title=${encodeURIComponent("经验总结报告")}`}><Library size={15} />查看总结报告</a>{req.experienceSummaryJob?.sessionId ? <a href={`/session?id=${encodeURIComponent(req.experienceSummaryJob.sessionId)}`}>总结 Agent</a> : null}<button type="button" onClick={retrySummary} disabled={summaryWorking}>{summaryWorking ? "派发中…" : req.experienceSummaryJob?.status === "failed" ? "重试总结" : "重新总结"}</button><a href={`/api/requirement/experience-summary-context?id=${encodeURIComponent(req.id)}&limit=200`} target="_blank" rel="noreferrer">候选汇总</a><a href={`/api/requirement/context?id=${encodeURIComponent(req.id)}&intent=experience-summary&budget=3000&format=html`} target="_blank" rel="noreferrer">总结上下文</a></>} />
      <CodeReviewPanel req={req} />
      <MergeBranchPanel req={req} />
      <ProdMrPanel req={req} />
      <OnesPanel req={req} onSaved={refresh} />
      <section className="react-panel"><PanelHead kicker="Status" title={isOnlineIssue ? "线上问题状态" : "状态切换"} /><p className="react-muted">{isOnlineIssue ? "线上问题轻流程：排查中 → 已确认；用于记录排查过程，不强制需求阶段门禁。若确认需要代码修复，可一键转普通需求流程。" : "新版流程：需求澄清 → 开发中 → 自测中 → 测试中 → 经验总结 → 已完成。自测中推进到测试中前必须通过代码审查门禁，旧状态会自动兼容映射。"}</p><div className="react-inline-form"><select value={status} onChange={(e) => { setStatus(e.target.value as ReqStatus); setStatusMessage(null) }}><option value="">选择状态</option>{statusOptions.map((s) => <option key={s} value={s}>{s}</option>)}</select><input value={note} onChange={(e) => setNote(e.target.value)} placeholder="备注" /><button onClick={submitStatus} disabled={!status || savingStatus}>{savingStatus ? "保存中…" : "保存状态"}</button>{isOnlineIssue ? <button type="button" onClick={convertIssue}>转为普通需求</button> : null}</div>{statusMessage ? <p className={statusMessage.startsWith("状态保存失败") ? "react-effort-error" : "react-save-hint"}>{statusMessage}</p> : null}<div className="react-inline-form react-category-form"><label>类别</label><select value={category} onChange={(e) => setCategory(e.target.value as ReqCategory)}><option value="">{req.category ?? "需求"}</option>{REQ_CATEGORIES.map((c) => <option key={c} value={c}>{c}</option>)}</select><button onClick={submitCategory} disabled={!category || savingCategory}>{savingCategory ? "保存中…" : "保存类别"}</button></div></section>
      <section className="react-panel"><PanelHead kicker="Sessions" title="关联 Session" chip={req.sessionIds?.length ? <button type="button" className="react-chip-count-btn" onClick={() => setShowSessions(true)} title="查看全部关联 session"><List size={13} />{req.sessionIds.length}</button> : "0"} />{req.sessionIds?.length ? <SessionChipList sessionIds={req.sessionIds} /> : <p className="react-muted">暂无关联 session。</p>}<div className="react-actions"><button onClick={newSession}>生成新 pi session 命令</button></div>{command ? <div className="react-command-wrap"><code className="react-command">{command}</code><button type="button" className="react-copy-link-btn" onClick={copyCommand} title="复制命令到剪贴板"><Copy size={13} />{copied ? "已复制" : "复制"}</button></div> : null}{showSessions && req.sessionIds?.length ? <SessionListModal sessionIds={req.sessionIds} onClose={() => setShowSessions(false)} /> : null}</section>
      <RequirementFilesPanel req={req} />
    </div>}
  </PageChrome>
}
