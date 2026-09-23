import { GitBranch, RefreshCw } from "lucide-react"
import { useState } from "react"
import type { BranchRegistrationPayload, BranchRoundsPayload, CodeReviewPayload, DiffSnapshotsPayload, MasterDiffPayload, Requirement, ReviewGatePayload, SyncBasePayload } from "../../types"
import { postForm, useFetch } from "../../lib/api"
import { reviewStats, unquoteGitPath } from "../../lib/diff"
import { formatDateTime } from "../../lib/format"
import { LoadingCard, PanelHead } from "../../components/ui"

export function CodeReviewPanel({ req, round, onRoundChange }: { req: Requirement; round: number; onRoundChange: (n: number) => void }) {
  // 分支登记轮次由父页面（需求详情页）持有，与生产 MR 卡片联动：
  // 1 = 原始 branches.json + 审查工作流（code-review.json 快照/门禁/备料）；
  // >=2 = 修复轮次（branches-round-n.json），scope 读 branch-registration?round=，
  // 差异走 master-diff 快照体系（每轮次独立快照栈），与分支差异页保持一致。
  // 快照/审查材料的生成由 agent 经 API 驱动（code-review / review-materials），卡片只做展示。
  const rounds = useFetch<BranchRoundsPayload>(`/api/requirement/branch-rounds?reqId=${encodeURIComponent(req.id)}`, [req.id])
  const hasMultiRounds = (rounds.data?.rounds?.length || 0) > 1 || round > 1
  const roundOneData = useFetch<CodeReviewPayload>(round <= 1 ? `/api/requirement/code-review?id=${encodeURIComponent(req.id)}` : null, [req.id, round])
  const roundScopeData = useFetch<BranchRegistrationPayload>(round >= 2 ? `/api/requirement/branch-registration?reqId=${encodeURIComponent(req.id)}&round=${round}` : null, [req.id, round])
  const roundSnapshots = useFetch<DiffSnapshotsPayload>(round >= 2 ? `/api/requirement/diff-snapshots?reqId=${encodeURIComponent(req.id)}&round=${round}` : null, [req.id, round])
  const gate = useFetch<ReviewGatePayload>(`/api/requirement/review-gate?id=${encodeURIComponent(req.id)}`, [req.id])
  const inventoryRisk = Boolean(gate.data?.gate?.inventoryRisk)
  const gateRiskTags = gate.data?.gate?.riskTags || []
  const staleRepos = gate.data?.gate?.staleRepos || []
  const gateWarnings = gate.data?.gate?.warnings || []
  const gateStale = gate.data?.gate?.status === "stale"
  const error = round <= 1 ? roundOneData.error : roundScopeData.error || roundSnapshots.error
  const loading = round <= 1 ? roundOneData.loading : roundScopeData.loading || roundSnapshots.loading
  const refresh = roundOneData.refresh
  const [actionError, setActionError] = useState<string | null>(null)
  const [syncing, setSyncing] = useState(false)
  const [syncPayload, setSyncPayload] = useState<SyncBasePayload | null>(null)
  const [generatingDiff, setGeneratingDiff] = useState(false)
  const scope = round <= 1 ? roundOneData.data?.branchScope || null : roundScopeData.data?.scope || null
  const review = round <= 1 ? roundOneData.data?.review || null : roundSnapshots.data?.snapshots?.[0] || null
  const stats = reviewStats(review)
  const canScan = Boolean(scope?.repos?.length)
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
  // 修复轮次（round >= 2）生成差异：走 master-diff（每轮次独立快照栈，与分支差异页共用）。
  const generateRoundDiff = async () => {
    if (!canScan || generatingDiff) return
    setGeneratingDiff(true)
    setActionError(null)
    try {
      await postForm<MasterDiffPayload>("/api/requirement/master-diff", { reqId: req.id, round: String(round) })
      roundSnapshots.refresh()
    } catch (err) {
      setActionError(err instanceof Error ? err.message : String(err))
    } finally {
      setGeneratingDiff(false)
    }
  }
  return <section id="code-review" className="react-panel react-code-review-panel"><PanelHead kicker="Code Diff" title="代码差异" />
    {round >= 2 ? <p className="react-muted">修复轮次 {round}：差异按该轮次登记的分支生成，与分支差异页共用每轮次快照；审查门禁与备料仅作用于轮次 1。</p> : null}
    {round <= 1 && gateRiskTags.length ? <div className="react-review-risk-tags"><strong>风险标签</strong>{gateRiskTags.map((tag) => <span key={tag} className="react-review-tag">{tag}</span>)}</div> : null}
    {round <= 1 && inventoryRisk ? <div className="react-review-gate react-review-gate-blocked"><strong>⚠ 库存高危风险</strong><span>本次改动命中库存相关文件/表，门禁强制要求库存账本专项评估：单据活跃/死亡、DB 库存(onHand/allocated/临时库位/回库单)、redis 可用量(建单-、真取消+、恢复-、回退保持占用)、重复释放、遗漏占用、幂等、验证证据(DB/redis/日志/单测)。未补充前即使 PASS 也不通过。</span></div> : null}
    {round <= 1 && gateStale ? <div className="react-drive-blockers"><strong>审查快照需刷新覆盖</strong>{staleRepos.length ? <ul>{staleRepos.map((repo) => <li key={`${repo.repoName}-${repo.branch}`}><code>{repo.repoName}</code> / <code>{repo.branch}</code>：{(repo.reviewedTargetCommit || "").slice(0, 12) || "reviewed?"} → {(repo.currentTargetCommit || "").slice(0, 12) || "current?"}</li>)}</ul> : null}<p>让 agent 加载 agent-panel-code-review skill 重新备料复审（POST /api/requirement/review-materials；发布就绪起默认增量，只审上次已审 commit → 当前 HEAD 的新增 diff；非线性历史自动回退全量）。</p></div> : null}
    {round <= 1 && gateWarnings.length ? <div className="react-drive-blockers"><strong>⚠ 审查放行但有警示</strong><ul>{gateWarnings.map((w, i) => <li key={i}>{w}</li>)}</ul></div> : null}
    {round <= 1 && gate.error ? <p className="react-effort-error">门禁加载失败：{gate.error}</p> : null}
    <div className="react-actions">{hasMultiRounds ? <select value={round} onChange={(e) => onRoundChange(Number(e.target.value))} title="切换分支登记轮次：每轮次每应用登记一个分支；轮次 1 为原始需求分支，≥2 为合入生产后的修复轮次；选中轮次对下方生产 MR 卡片同样生效">{(rounds.data?.rounds || []).map((r) => <option key={r.round} value={r.round}>轮次 {r.round}{r.round === 1 ? "（原始）" : "（修复）"}{r.sealed ? " · 已封版" : ""}</option>)}</select> : null}{round >= 2 ? <button onClick={generateRoundDiff} disabled={!canScan || generatingDiff} title="按当前轮次登记的分支生成差异并保存到该轮次快照栈（与分支差异页共用）"><RefreshCw size={15} className={generatingDiff ? "react-spin" : ""} />{review ? "刷新该轮次差异" : "生成代码差异"}</button> : null}<button onClick={syncBase} disabled={!canScan || syncing} title="fetch 远端生产分支并 reset 本地 master/production 到最新,工作区有改动时自动跳过"><RefreshCw size={15} className={syncing ? "react-spin" : ""} />{syncing ? "同步中…" : "同步生产基线"}</button><a href={`/requirement-diff?id=${encodeURIComponent(req.id)}&round=${round}&base=origin%2Fmaster`}><GitBranch size={15} />打开分支差异页</a></div>
    {syncPayload?.results?.length ? <details className="react-review-repo" open><summary><span><strong>生产基线同步</strong><em>{formatDateTime(syncPayload.generatedAt)}</em></span><span className="react-review-size">{syncPayload.results.filter((r) => r.ok).length}/{syncPayload.results.length} ok</span></summary><div className="react-table-wrap react-code-file-wrap"><table className="react-code-file-table"><thead><tr><th>应用</th><th>本地分支</th><th>状态</th><th>before</th><th>after</th><th>说明</th></tr></thead><tbody>{syncPayload.results.map((r) => <tr key={r.repoName}><td><strong>{r.repoName}</strong></td><td><code>{r.localBranch || r.baseRef || "-"}</code></td><td><span className={`react-merge-status ${r.ok ? "merged" : "conflict"}`}>{r.status}</span></td><td><code>{r.beforeCommit || "-"}</code></td><td><code>{r.afterCommit || "-"}</code></td><td>{r.message}{r.warnings?.length ? <em>{r.warnings.join("; ")}</em> : null}</td></tr>)}</tbody></table></div></details> : null}
    {error ? <p className="react-effort-error">加载失败：{error}</p> : null}{actionError ? <p className="react-effort-error">刷新失败：{actionError}</p> : null}
    {loading ? <LoadingCard label="正在加载代码差异…" /> : <>
      <div className="react-branch-scope">
        {scope?.repos?.length ? scope.repos.map((repo) => <div key={`${repo.repoName}-${repo.branches?.join("/")}`} className="react-branch-card"><strong>{repo.repoName}</strong><span>{repo.role || "repo"}</span><code>{repo.branches?.join(" / ") || "未指定分支"}</code><em>{repo.baseRef || (repo.role === "前端" ? "origin/production" : "origin/master")}</em></div>) : <p className="react-muted">未找到 <code>{round >= 2 ? `branches-round-${round}.json` : "branches.json"}</code>（轮次 {round}），无法生成代码差异；请先运行 <code>req-branches-update</code> 登记该轮次分支。</p>}
      </div>
      {review ? <div className="react-review-summary"><span>{stats.repoCount} repo/branch</span><span>{stats.fileCount} files</span><span className="react-review-add">+{stats.additions}</span><span className="react-review-del">-{stats.deletions}</span><span>更新 {formatDateTime(review.updatedAt)}</span></div> : <p className="react-muted">{round >= 2 ? "暂无该轮次差异快照；点击「生成代码差异」生成。" : <>暂无 <code>code-review.json</code> 快照；由 agent 备料/审查流程生成（<code>POST /api/requirement/review-materials</code>）。</>}</p>}
      {review?.repos?.map((repo, index) => <details key={`${repo.repoName}-${repo.branch}-${index}`} className="react-review-repo">
        <summary><span><strong>{repo.repoName}</strong><em>{repo.branch}</em></span><span className="react-review-size">+{repo.additions || 0} / -{repo.deletions || 0}</span></summary>
        <div className="react-card-meta"><span>base {repo.baseRef || review.baseRef}{repo.baseCommit ? ` @ ${repo.baseCommit.slice(0, 12)}` : ""}</span><span>target {repo.resolvedTargetRef || repo.branch}{repo.targetCommit ? ` @ ${repo.targetCommit.slice(0, 12)}` : ""}</span><span>current {repo.currentBranch || "-"}</span><span>{repo.dirty ? "工作区有未提交改动" : "工作区干净"}</span><span>{repo.projectPath || "path n/a"}</span></div>
        {repo.error ? <p className="react-effort-error">{repo.error}</p> : null}
        {repo.warnings?.length ? <div className="react-drive-blockers"><strong>Warnings</strong><ul>{repo.warnings.map((w) => <li key={w}>{w}</li>)}</ul></div> : null}
        {repo.commits?.length ? <details className="react-review-commits"><summary>提交列表（{repo.commits.length}）</summary><pre>{repo.commits.join("\n")}</pre></details> : null}
        {repo.files?.length ? <div className="react-table-wrap react-code-file-wrap"><table className="react-code-file-table"><thead><tr><th>文件</th><th>状态</th><th>增删</th><th>风险</th></tr></thead><tbody>{repo.files.map((file) => <tr key={file.path}><td><code>{unquoteGitPath(file.path)}</code></td><td>{file.status}</td><td><span className="react-review-add">+{file.additions}</span> / <span className="react-review-del">-{file.deletions}</span></td><td>{file.riskTags?.length ? file.riskTags.map((tag) => <span key={tag} className="react-review-tag" data-risk={tag}>{tag}</span>) : <span className="react-muted">-</span>}</td></tr>)}</tbody></table></div> : <p className="react-muted">没有文件级差异。</p>}
      </details>)}
    </>}
  </section>
}
