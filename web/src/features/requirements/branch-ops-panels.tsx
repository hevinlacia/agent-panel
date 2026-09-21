import { AlertTriangle, GitBranch, GitMerge } from "lucide-react"
import { useEffect, useState } from "react"
import type { MergeBranchPayload, MergeKindOptions, MergeOptionsPayload, MergeRepoKind, ProdMrPayload, ProdMrResult, Requirement } from "../../types"
import { postForm, useFetch } from "../../lib/api"
import { formatDateTime } from "../../lib/format"
import { PanelHead } from "../../components/ui"

export function MergeBranchPanel({ req }: { req: Requirement }) {
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
  const isIssue = req.category === "线上问题" || req.category === "测试问题"
  return <section className="react-panel react-merge-panel"><PanelHead kicker="Branch Merge" title="合并到测试 / UAT" chip={chip} />
    <p className="react-muted">选择要合并的环境分支后执行；未选择的前端/后端不会合并。自测中默认选 test，测试中默认选 UAT 分支，其他状态默认不选择。{isIssue ? "线上问题/测试问题：排查/复现代码仅允许合入 test/UAT 环境分支，后端 master、前端 production 等生产分支会被后端拦截；正式生产修复请用状态面板的「创建修复需求」。" : "生产分支合入走下方「生产 MR」，由 GitLab 审批后手动合入。"}</p>
    {optionData.error ? <p className="react-effort-error">分支选项加载失败：{optionData.error}</p> : null}
    <div className="react-merge-select-grid">{selectBranch("frontend", frontendBranch, setFrontendBranch, optionData.data?.options.frontend)}{selectBranch("backend", backendBranch, setBackendBranch, optionData.data?.options.backend)}</div>
    <div className="react-actions"><button onClick={runMerge} disabled={loading || optionData.loading || (!frontendBranch && !backendBranch)}><GitMerge size={15} />{loading ? "合并中…" : "合并所选分支"}</button><a href={`/requirement-merge?id=${encodeURIComponent(req.id)}`}><AlertTriangle size={15} />查看冲突 / 合并状态</a>{latestAt ? <span className="react-muted">更新 {formatDateTime(latestAt)}</span> : null}</div>
    {error ? <p className="react-effort-error">合并失败：{error}</p> : null}
    {payloads.length ? <div className="react-review-summary"><span>{results.length} repo/branch</span><span className="react-review-add">{mergedCount} merged</span><span className={conflictCount ? "react-review-del" : undefined}>{conflictCount} conflict</span><span>{[frontendBranch, backendBranch].filter(Boolean).join(" / ") || "-"}</span></div> : null}
    {results.length ? <div className="react-table-wrap react-prod-mr-wrap"><table className="react-code-file-table react-prod-mr-table"><thead><tr><th>应用</th><th>源分支</th><th>目标</th><th>状态</th><th>冲突 / 位置</th></tr></thead><tbody>{results.map((item, index) => <tr key={`${item.repoName}-${item.sourceBranch}-${item.target}-${index}`}><td><strong>{item.repoName}</strong><span>{item.role || "repo"}</span></td><td><code>{item.sourceBranch}</code></td><td><code>{item.targetBranch || item.target}</code></td><td><span className={`react-merge-status ${item.status}`}>{mergeStatusLabel(item.status)}</span>{item.message ? <em>{item.message}</em> : null}</td><td>{item.status === "conflict" ? <a href={`/requirement-merge?id=${encodeURIComponent(req.id)}&target=${encodeURIComponent(String(item.target))}`}>{item.conflictFiles?.length || 0} 个冲突文件</a> : item.worktreePath ? <code>{item.worktreePath}</code> : <span className="react-muted">-</span>}</td></tr>)}</tbody></table></div> : payloads.length ? <p className="react-muted">未返回合并结果，请检查 <code>branches.json</code>。</p> : null}
  </section>
}

export function mergeStatusLabel(status: string): string {
  if (status === "merged") return "已合并"
  if (status === "upToDate") return "已最新"
  if (status === "conflict") return "冲突"
  if (status === "skipped") return "跳过"
  if (status === "idle") return "空闲"
  if (status === "pending") return "待检查"
  if (status === "failed") return "失败"
  return status
}

export function ProdMrPanel({ req }: { req: Requirement }) {
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
