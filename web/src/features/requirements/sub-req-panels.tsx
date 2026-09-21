import { useState } from "react"
import { GitMerge, RefreshCw, Sparkles } from "lucide-react"
import type { Requirement, SubBranchOpResult, SubRepoOpResult } from "../../types"
import { postJson } from "../../lib/api"
import { statusPill } from "./badges"
import { PanelHead } from "../../components/ui"

/** 父需求视角：子需求列表 + 拆分子需求。子需求是并行执行单元，分支以父分支为 base，合回父分支。 */
export function SubRequirementsPanel({ req, onSaved }: { req: Requirement; onSaved: () => void }) {
  const subs = req.subReqs ?? []
  const [title, setTitle] = useState("")
  const [slug, setSlug] = useState("")
  const [summary, setSummary] = useState("")
  const [creating, setCreating] = useState(false)
  const [feedback, setFeedback] = useState<string | null>(null)
  const [open, setOpen] = useState(false)
  const merged = subs.filter((s) => s.merged).length
  const active = subs.filter((s) => s.found && !s.merged && !s.cancelled).length
  const create = async () => {
    if (creating || !title.trim()) return
    setCreating(true)
    setFeedback(null)
    try {
      const res = await postJson<{ ok: boolean; reqId: string; copiedDocs?: string[] }>("/api/requirement/create-sub", {
        parentReqId: req.id,
        title: title.trim(),
        slug: slug.trim() || undefined,
        summary: summary.trim() || undefined,
      })
      setFeedback(`已创建子需求 ${res.reqId}（复制了 ${res.copiedDocs?.length ?? 0} 份父需求文档作快照）；到「初始化分支」前仅是需求记录`)
      setTitle("")
      setSlug("")
      setSummary("")
      onSaved()
    } catch (err) {
      setFeedback(`创建失败：${err instanceof Error ? err.message : String(err)}`)
    } finally {
      setCreating(false)
    }
  }
  return <section id="sub-requirements" className="react-panel"><PanelHead kicker="Sub Requirements" title="子需求（并行拆分）" chip={subs.length ? `${merged}/${subs.length} 已合入` : "0"} /><p className="react-muted">大需求开发中需要提速时，可从本需求拆出子需求并行开发：子分支以本需求分支为 base（&lt;父分支&gt;-sub&lt;n&gt;），合回本需求分支后由本需求统一做 Review、test/UAT 集成和发布；子需求不绑 ONES/plan-release/issues。已合入 {merged} · 进行中 {active} · 已取消 {subs.filter((s) => s.cancelled).length}。</p>{subs.length ? <div className="react-card-meta">{subs.map((s) => <span key={s.reqId} className="react-linked-issue-chip"><a href={`/requirement?id=${encodeURIComponent(s.reqId)}`}>{s.reqId}</a>{s.title ? <em className="react-muted">{s.title}</em> : null}{s.found ? statusPill(s.status || "需求创建") : <em className="react-muted">未找到</em>}</span>)}{subs.some((s) => !s.found) ? <span className="react-muted">存在失效子需求引用（目录被手动删除？）</span> : null}</div> : <p className="react-muted">尚未拆分子需求。</p>}{open ? <div className="react-inline-form react-sub-create-form"><input value={title} onChange={(e) => setTitle(e.target.value)} placeholder="子需求标题（必填）" /><input value={slug} onChange={(e) => setSlug(e.target.value)} placeholder="slug（可选，如 fix-logging）" /><input value={summary} onChange={(e) => setSummary(e.target.value)} placeholder="范围摘要（可选，这个子需求负责什么）" /><button onClick={create} disabled={creating || !title.trim()}>{creating ? "创建中…" : "创建子需求"}</button></div> : null}{feedback ? <p className={feedback.startsWith("创建失败") ? "react-effort-error" : "react-save-hint"}>{feedback}</p> : null}{open ? <p className="react-muted">创建后仅生成需求记录（ID = 父票号-S&lt;n&gt;[-slug]，复制父需求 background/technical-plan/impact/test 文档作快照）；并行开发前在子需求详情页点「初始化分支」。</p> : null}<div className="react-actions"><button type="button" onClick={() => setOpen((v) => !v)}><Sparkles size={13} />{open ? "收起创建表单" : "拆分子需求"}</button></div></section>
}

/** 子需求视角：分支操作面板 —— 初始化分支（以父分支为 base）、同步主需求分支、合入主需求。 */
export function SubBranchPanel({ req, onSaved }: { req: Requirement; onSaved: () => void }) {
  const [busy, setBusy] = useState<"" | "init" | "sync" | "merge">("")
  const [results, setResults] = useState<SubBranchOpResult | null>(null)
  const [error, setError] = useState<string | null>(null)
  const run = async (kind: "init" | "sync" | "merge") => {
    if (busy) return
    if (kind === "merge" && !window.confirm(`确认把子需求 ${req.id} 的所有子分支合入父需求分支？\n\n这会修改并推送父需求分支（高风险操作）；合入后子需求自动推进为「已合入」，后续增量请拆新子需求。`)) return
    if (kind === "sync" && !window.confirm("确认同步主需求分支最新成果到子分支？冲突时会保留 merge worktree 由人工在子分支侧解决。")) return
    if (kind === "init" && !window.confirm("确认初始化子需求分支？将为父需求 branches.json 的每个仓库创建 <父分支>-sub<n> 分支（已存在则跳过）、push -u 并创建 worktree（<repo>/.worktrees/<子需求ID>）。")) return
    setBusy(kind)
    setError(null)
    try {
      const res = await postJson<SubBranchOpResult>(`/api/requirement/sub/${kind === "init" ? "init-branches" : kind === "sync" ? "sync-parent" : "merge-to-parent"}`, {
        reqId: req.id,
        ...(kind === "init" ? { createWorktree: true } : {}),
        ...(kind === "merge" ? { confirm: true } : {}),
      })
      setResults(res)
      onSaved()
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err))
    } finally {
      setBusy("")
    }
  }
  const renderRepo = (r: SubRepoOpResult) => <div key={`${r.repoName}-${r.sourceBranch}-${r.targetBranch}`} className="react-sub-op-repo"><code>{r.repoName}</code><span className="react-muted">{r.sourceBranch || "-"} → {r.targetBranch || "-"}</span><strong className={`react-sub-op-status react-sub-op-${r.status}`}>{r.status}</strong>{r.message ? <span className="react-muted" title={r.message}>{r.message}</span> : null}{r.worktreePath ? <span className="react-muted" title={r.worktreePath}>worktree: {r.worktreePath}</span> : null}</div>
  return <section id="sub-branch" className="react-panel"><PanelHead kicker="Sub Branch Ops" title="子需求分支操作" chip={req.status} /><p className="react-muted">并行开发三步：<strong>① 初始化分支</strong>——以父需求分支为 base 派生 <code>&lt;父分支&gt;-sub&lt;n&gt;</code>（每仓，baseRef 记为父分支，代码差异审查自动以父分支为基准）；<strong>② 同步主需求分支</strong>——兄弟子需求合入父分支后，把父分支最新成果合入子分支拉齐进度（冲突在子分支侧解决）；<strong>③ 合入主需求</strong>——逐仓把子分支合回父分支并推送，全部成功后自动推进「已合入」。子需求永不直接操作 test/UAT/生产分支，环境集成由父需求统一执行。</p><div className="react-actions"><button type="button" onClick={() => run("init")} disabled={busy !== ""}><GitMerge size={13} />{busy === "init" ? "初始化中…" : "① 初始化分支"}</button><button type="button" onClick={() => run("sync")} disabled={busy !== ""}><RefreshCw size={13} />{busy === "sync" ? "同步中…" : "② 同步主需求分支"}</button><button type="button" onClick={() => run("merge")} disabled={busy !== "" || req.status === "已合入" || req.status === "已取消"}><GitMerge size={13} />{busy === "merge" ? "合入中…" : "③ 合入主需求"}</button></div>{error ? <p className="react-effort-error">{error}</p> : null}{results ? <div className="react-sub-op-results">{results.repos?.length ? results.repos.map(renderRepo) : <p className="react-muted">无仓库结果。</p>}</div> : null}</section>
}
