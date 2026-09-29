import { useState } from "react"
import { GitBranch, GitMerge, PackageCheck, RefreshCw, Rocket } from "lucide-react"
import type { ReleaseBranchEntry, ReleaseBranchListResult, ReleaseBranchOpResult } from "../../types"
import { fetchJson, postJson } from "../../lib/api"
import { PanelHead } from "../../components/ui"

const OP_URL: Record<string, string> = {
  create: "create",
  merge: "merge-sub",
  sync: "sync-prod",
  mr: "prod-mr",
  release: "mark-released",
}

/**
 * 整合需求视角：整合需求发布分支管理。
 * 发布分支以各仓生产分支为 base，子需求分支逐仓合入做预集成（提前解冲突）；
 * 发布 = 发布分支 → 生产分支 MR；多次发布各建一条（r1/r2…，命名自动去重）。
 */
export function ReleaseBranchesPanel({ req, onSaved }: { req: { id: string }; onSaved: () => void }) {
  const [prefix, setPrefix] = useState("")
  const [subReqId, setSubReqId] = useState("")
  const [busy, setBusy] = useState(false)
  const [lastOp, setLastOp] = useState<ReleaseBranchOpResult | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [list, setList] = useState<ReleaseBranchListResult | null>(null)
  const [loading, setLoading] = useState(false)

  const load = async () => {
    setLoading(true)
    try {
      const res = await fetchJson<ReleaseBranchListResult>(
        `/api/requirement/release-branch/list?id=${encodeURIComponent(req.id)}`
      )
      setList(res)
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err))
    } finally {
      setLoading(false)
    }
  }
  const run = async (kind: keyof typeof OP_URL, branchId?: string) => {
    if (busy) return
    if (kind === "merge") {
      if (!subReqId.trim()) {
        setError("请先在输入框填要合入的子需求 reqId")
        return
      }
      if (!window.confirm(`确认把子需求 ${subReqId.trim()} 合入发布分支（${branchId ?? "最新 active"}）？\n\n会修改并推送发布分支（多子需求共享的集成分支）；冲突时保留 merge worktree 在发布分支侧解决。`)) return
    }
    if (kind === "sync" && !window.confirm(`确认把各仓生产分支最新成果合入发布分支（${branchId ?? "最新 active"}）？冲突时保留 worktree 由人工在发布分支侧解决。`)) return
    if (kind === "release" && !window.confirm(`确认封版发布分支 ${branchId ?? ""}？\n\n前提：发布分支 → 生产分支的 MR 已全部人工合入；封版后该分支不可再合入，其覆盖的子需求自动推进「已发布」。`)) return
    setBusy(true)
    setError(null)
    try {
      const body: Record<string, unknown> = { reqId: req.id }
      if (branchId) body.branch = branchId
      if (kind === "create" && prefix.trim()) body.branchPrefix = prefix.trim()
      if (kind === "merge") body.subReqId = subReqId.trim()
      if (kind === "merge" || kind === "sync" || kind === "release") body.confirm = true
      const res = await postJson<ReleaseBranchOpResult>(`/api/requirement/release-branch/${OP_URL[kind]}`, body)
      setLastOp(res)
      if (kind === "create" || kind === "release") onSaved()
      if (kind !== "create") load()
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err))
    } finally {
      setBusy(false)
    }
  }
  const branches = list?.branches ?? []
  const renderBranch = (b: ReleaseBranchEntry) => (
    <div key={b.id} className="react-sub-op-repo" style={{ display: "block" }}>
      <div><code>{b.name}</code><strong className={`react-sub-op-status react-sub-op-${b.status === "active" ? "merged" : "upToDate"}`}>{b.status}</strong><span className="react-muted">{b.id}</span></div>
      {b.mergedSubs?.length ? (
        <div className="react-muted">已合入子需求：{b.mergedSubs.map((m) => <span key={m.reqId} className="react-linked-issue-chip"><a href={`/requirement?id=${encodeURIComponent(m.reqId)}`}>{m.reqId}</a>{m.status ? <em className="react-muted">{m.status}</em> : null}{m.repos && !m.repos.length ? <em className="react-muted">（纯配置）</em> : null}</span>)}</div>
      ) : <div className="react-muted">尚未合入子需求。</div>}
      {b.repos?.length ? (
        <div className="react-muted">{b.repos.length} 仓 · base={b.repos[0]?.baseBranch}{b.repos.some((r) => typeof r.diffFiles === "number") ? ` · 相对生产分支 diff ${b.repos.reduce((acc, r) => acc + (r.diffFiles ?? 0), 0)} files` : ""}</div>
      ) : null}
      {b.status === "active" ? (
        <div className="react-actions">
          <button type="button" onClick={() => run("merge", b.id)} disabled={busy}><GitMerge size={13} />合入子需求</button>
          <button type="button" onClick={() => run("sync", b.id)} disabled={busy}><RefreshCw size={13} />同步生产分支</button>
          <button type="button" onClick={() => run("mr", b.id)} disabled={busy}><Rocket size={13} />生成生产 MR</button>
          <button type="button" onClick={() => run("release", b.id)} disabled={busy}><PackageCheck size={13} />封版（已发布）</button>
        </div>
      ) : null}
    </div>
  )
  return (
    <section id="release-branches" className="react-panel">
      <PanelHead kicker="Release Branches" title="整合需求发布分支" chip={branches.length ? `${branches.filter((b) => b.status === "active").length} active / ${branches.length}` : "0"} />
      <p className="react-muted">整合多条子需求的发布载体：<strong>① 创建发布分支</strong>——以各仓生产分支（master/production）为 base 建 <code>&lt;前缀&gt;/release/&lt;整合需求&gt;-r&lt;n&gt;</code>（多次创建自动 r1/r2… 命名去重）；<strong>② 合入子需求</strong>——逐仓把子需求分支合入发布分支做预集成，子需求间冲突提前在发布分支侧解决；<strong>③ 生成生产 MR</strong>——发布分支 → 生产分支，人工在 GitLab 合入；<strong>④ 封版</strong>——MR 合入后标记 released，覆盖的子需求自动推进「已发布」。发布分支存在期间生产分支前进可用「同步生产分支」拉齐。</p>
      <div className="react-inline-form react-sub-create-form">
        <input value={prefix} onChange={(e) => setPrefix(e.target.value)} placeholder="分支名前缀（可选，默认从子需求分支推导，如 hevin.yang/release）" />
        <input value={subReqId} onChange={(e) => setSubReqId(e.target.value)} placeholder="子需求 reqId（合入用，如 WMS-144-S2-xxx）" />
        <button onClick={() => run("create")} disabled={busy}>{busy ? "处理中…" : <><GitBranch size={13} />创建发布分支</>}</button>
      </div>
      <div className="react-actions">
        <button type="button" onClick={load} disabled={busy || loading}><RefreshCw size={13} />{loading ? "刷新中…" : "刷新列表"}</button>
      </div>
      {error ? <p className="react-effort-error">{error}</p> : null}
      {branches.length ? <div className="react-sub-op-results">{branches.map(renderBranch)}</div> : <p className="react-muted">尚未创建发布分支；创建前需要子需求已登记分支（纯配置子需求可无分支，仅登记随分支上线）。</p>}
      {lastOp ? (
        <div className="react-sub-op-results">
          {lastOp.message ? <p className="react-muted">{lastOp.message}</p> : null}
          {lastOp.branch ? <p className="react-muted">目标分支：<code>{lastOp.branch}</code></p> : null}
          {lastOp.repos?.length ? lastOp.repos.map((r, i) => (
            <div key={`${r.repoName}-${i}`} className="react-sub-op-repo"><code>{r.repoName ?? "-"}</code><span className="react-muted">{r.sourceBranch || "-"} → {r.targetBranch || "-"}</span><strong className={`react-sub-op-status react-sub-op-${r.status}`}>{r.status}</strong>{r.message ? <span className="react-muted" title={r.message}>{r.message}</span> : null}{r.worktreePath ? <span className="react-muted" title={r.worktreePath}>worktree: {r.worktreePath}</span> : null}</div>
          )) : null}
          {lastOp.results?.length ? <p className="react-muted">生产 MR 结果 {lastOp.results.length} 条（链接已写入需求 notes）。</p> : null}
          {lastOp.advancedSubs?.length ? <p className="react-save-hint">已推进「已发布」：{lastOp.advancedSubs.join(", ")}</p> : null}
        </div>
      ) : null}
    </section>
  )
}
