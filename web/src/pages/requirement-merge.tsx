import { ArrowLeft, GitMerge, RefreshCw } from "lucide-react"
import { useEffect, useState } from "react"
import type { MergeBranchPayload, MergeTarget } from "../types"
import { fetchJson } from "../lib/api"
import { formatDateTime } from "../lib/format"
import { mergeStatusLabel } from "../features/requirements/branch-ops-panels"
import { EmptyCard, ErrorCard, LoadingCard, PageChrome, PanelHead } from "../components/ui"
import { RequirementsData } from "./projects"

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
