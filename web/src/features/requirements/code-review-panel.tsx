import { GitBranch, RefreshCw } from "lucide-react"
import { useState } from "react"
import type { CodeReviewPayload, Requirement, ReviewGatePayload, ReviewMaterialsPayload, SyncBasePayload } from "../../types"
import { postForm, useFetch } from "../../lib/api"
import { reviewStats, unquoteGitPath } from "../../lib/diff"
import { formatDateTime } from "../../lib/format"
import { LoadingCard, PanelHead } from "../../components/ui"

export function CodeReviewPanel({ req }: { req: Requirement }) {
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
  return <section id="code-review" className="react-panel react-code-review-panel"><PanelHead kicker="Code Diff" title="代码差异" />
    {gateRiskTags.length ? <div className="react-review-risk-tags"><strong>风险标签</strong>{gateRiskTags.map((tag) => <span key={tag} className="react-review-tag">{tag}</span>)}</div> : null}
    {inventoryRisk ? <div className="react-review-gate react-review-gate-blocked"><strong>⚠ 库存高危风险</strong><span>本次改动命中库存相关文件/表，门禁强制要求库存账本专项评估：单据活跃/死亡、DB 库存(onHand/allocated/临时库位/回库单)、redis 可用量(建单-、真取消+、恢复-、回退保持占用)、重复释放、遗漏占用、幂等、验证证据(DB/redis/日志/单测)。未补充前即使 PASS 也不通过。</span></div> : null}
    {gateStale ? <div className="react-drive-blockers"><strong>审查快照需刷新覆盖</strong>{staleRepos.length ? <ul>{staleRepos.map((repo) => <li key={`${repo.repoName}-${repo.branch}`}><code>{repo.repoName}</code> / <code>{repo.branch}</code>：{(repo.reviewedTargetCommit || "").slice(0, 12) || "reviewed?"} → {(repo.currentTargetCommit || "").slice(0, 12) || "current?"}</li>)}</ul> : null}<p>优先生成增量审查包，只审上次已审 commit 到当前 HEAD 的新增 diff；非线性历史再回退全量审查。</p></div> : null}
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
