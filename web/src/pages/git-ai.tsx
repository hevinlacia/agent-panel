import { motion } from "framer-motion"
import { AlertTriangle, CheckCircle2, Clock3, GitBranch, RefreshCw } from "lucide-react"
import { useMemo, useState } from "react"
import type { GitAiCompanyStatus, GitAiFixResponse, GitAiHealthPayload, GitAiSuspectRecord, GitAiSuspectsPayload } from "../types"
import { postJson, useFetch } from "../lib/api"
import { relAge } from "../lib/format"
import { EmptyCard, ErrorCard, KpiCard, LoadingCard, PageChrome, PanelHead } from "../components/ui"

const gitAiStatusMeta: Record<GitAiCompanyStatus, { label: string; color: string; soft: string }> = {
  pending: { label: "待确认", color: "#f59e0b", soft: "rgba(245, 158, 11, .14)" },
  confirmed_ai: { label: "已标记", color: "#22c55e", soft: "rgba(34, 197, 94, .14)" },
  missing_ai: { label: "确认缺失", color: "#ef4444", soft: "rgba(239, 68, 68, .14)" },
  not_found: { label: "未找到", color: "#94a3b8", soft: "rgba(148, 163, 184, .14)" },
  check_failed: { label: "检查失败", color: "#f97316", soft: "rgba(249, 115, 22, .14)" },
}

function gitAiStatusPill(status: GitAiCompanyStatus) {
  const meta = gitAiStatusMeta[status] || gitAiStatusMeta.pending
  return <span className="react-status-pill" style={{ color: meta.color, background: meta.soft, borderColor: `${meta.color}66` }}>{meta.label}</span>
}

function authorShort(name?: string | null): string {
  return (name || "").split("<")[0].trim()
}

export function GitAiPage() {
  const feed = useFetch<GitAiSuspectsPayload>("/api/git-ai/suspects")
  const health = useFetch<GitAiHealthPayload>("/api/git-ai/health")
  const [status, setStatus] = useState<GitAiCompanyStatus | "all">("all")
  const [refreshingCompany, setRefreshingCompany] = useState(false)
  const [fixingId, setFixingId] = useState<string | null>(null)
  const [fixResults, setFixResults] = useState<Record<string, GitAiFixResponse>>({})
  const [author, setAuthor] = useState<string>("")
  const records = feed.data?.records || []
  const authors = useMemo(() => Array.from(new Set(records.map((r) => authorShort(r.authorName)).filter(Boolean))).sort(), [records])
  const filtered = (status === "all" ? records.filter((r) => r.companyStatus !== "confirmed_ai" && r.companyStatus !== "missing_ai") : records.filter((r) => r.companyStatus === status)).filter((r) => !author || authorShort(r.authorName) === author)
  const stats = feed.data?.stats
  const refreshCompany = async () => {
    setRefreshingCompany(true)
    try {
      await postJson<GitAiSuspectsPayload>("/api/git-ai/suspects/refresh", { limit: 200 })
      feed.refresh()
      health.refresh()
    } finally {
      setRefreshingCompany(false)
    }
  }
  const fixNote = async (record: GitAiSuspectRecord) => {
    if (!record.id || fixingId) return
    setFixingId(record.id)
    try {
      const res = await postJson<GitAiFixResponse>("/api/git-ai/suspects/fix-note", { id: record.id })
      setFixResults((cur) => ({ ...cur, [record.id]: res }))
      feed.refresh()
      health.refresh()
    } catch (err) {
      setFixResults((cur) => ({ ...cur, [record.id]: { ok: false, stillMissing: true, piAgent: { dispatched: false, message: err instanceof Error ? err.message : String(err) } } }))
    } finally {
      setFixingId(null)
    }
  }
  const canFix = (r: GitAiSuspectRecord) => r.companyStatus === "missing_ai" || r.companyStatus === "pending" || r.companyStatus === "not_found"
  return <PageChrome icon={<GitBranch size={15} />} eyebrow="Git AI" title="AI 标记漏标检查" description="刷新会调用公司 ai-stats/check-commit 接口；git-ai 是否打标以公司接口结果为准。" actions={<button onClick={refreshCompany} disabled={refreshingCompany}><RefreshCw size={15} />{refreshingCompany ? "公司检查中…" : "刷新公司检查"}</button>}><section className="react-kpi-grid"><KpiCard icon={<AlertTriangle size={20} />} label="疑似记录" value={stats?.total ?? "-"} sub="待判定" tone="avg" /><KpiCard icon={<Clock3 size={20} />} label="待确认" value={stats?.pending ?? "-"} sub="pending company check" tone="active" /><KpiCard icon={<AlertTriangle size={20} />} label="确认缺失" value={stats?.missingAi ?? "-"} sub="company says missing" tone="total" /><KpiCard icon={<CheckCircle2 size={20} />} label="已标记" value={stats?.confirmedAi ?? "-"} sub="company says tagged" tone="done" /></section><section className="react-panel"><PanelHead kicker="Health" title="git-ai 状态" chip={health.data?.cli?.daemonOk ? "ok" : (health.data?.piExtension?.status || "unknown")} />{health.error ? <ErrorCard error={health.error} /> : <div className="react-meta-grid"><span>Store</span><span><code>{health.data?.storePath || "-"}</code></span><span>git-ai binary</span><span><code>{health.data?.cli?.binaryPath || "missing"}</code></span><span>CLI version</span><span>{health.data?.cli?.version || "-"}</span><span>Daemon</span><span>{health.data?.cli?.daemonOk ? "running" : (health.data?.cli?.daemonMessage || "not running")}</span><span>Trace2 socket</span><span>{health.data?.cli?.trace2SocketExists ? "ok" : "missing"} <code>{health.data?.cli?.trace2Socket || "-"}</code></span><span>Hooks path</span><span><code>{health.data?.cli?.hooksPath || "-"}</code></span><span>post-commit hook</span><span>{health.data?.cli?.postCommitHook?.mode || "-"} · record={String(Boolean(health.data?.cli?.postCommitHook?.recordsToAgentPanel))}</span><span>pre-push hook</span><span>{health.data?.cli?.prePushHook?.mode || "-"} · record={String(Boolean(health.data?.cli?.prePushHook?.recordsToAgentPanel))}</span><span>Pi extension</span><span>{health.data?.piExtension?.status || "unknown"} · {health.data?.piExtension?.message || "-"}</span><span>Tracked tools</span><span>{health.data?.piExtension?.tracksTools?.join(" / ") || "-"}</span></div>}<div className="react-tab-row"><button className={status === "all" ? "active" : ""} onClick={() => setStatus("all")}>疑似待处理</button>{(Object.keys(gitAiStatusMeta) as GitAiCompanyStatus[]).map((s) => <button key={s} className={status === s ? "active" : ""} onClick={() => setStatus(s)}>{gitAiStatusMeta[s].label}</button>)}<select className="react-tab-select" value={author} onChange={(e) => setAuthor(e.target.value)} aria-label="按提交人筛选"><option value="">全部提交人</option>{authors.map((a) => <option key={a} value={a}>{a}</option>)}</select></div></section>{feed.error ? <ErrorCard error={feed.error} /> : feed.loading ? <LoadingCard /> : <div className="react-card-list">{filtered.length === 0 ? <EmptyCard>暂无符合条件的疑似漏标记录。</EmptyCard> : filtered.map((r, i) => <motion.article key={r.id || `${r.projectName}-${r.commitSha}`} className="react-list-card react-session-card react-gitai-card" initial={{ opacity: 0, y: 12 }} animate={{ opacity: 1, y: 0 }} transition={{ delay: Math.min(i, 16) * 0.025 }} whileHover={{ y: -3 }}><div><span className="react-card-id">{r.projectName} · {r.shortSha || r.commitSha?.slice(0, 12)}</span><h3>{r.commitWebUrl ? <a href={r.commitWebUrl} target="_blank" rel="noopener noreferrer">{r.commitTitle || r.subject || r.commitSha}</a> : (r.commitTitle || r.subject || r.commitSha)}</h3><p>{r.repoPath || r.remoteUrl || "repo path n/a"}</p><div className="react-card-meta"><span>{gitAiStatusPill(r.companyStatus || "pending")}</span>{r.authorName ? <span>提交人 {authorShort(r.authorName)}</span> : null}<span>hook: {(r.eventSources || []).join(" / ") || "-"}</span><span>本地 note: {r.localNoteState || "unknown"}</span><span>记录 {relAge(r.lastSeenAt)}</span>{r.companyCheckedAt ? <span>公司检查 {relAge(r.companyCheckedAt)}</span> : null}{typeof r.aiRate === "number" ? <span>AI rate {r.aiRate}%</span> : null}</div>{r.companyError ? <p className="react-error">{r.companyError}</p> : null}{fixResults[r.id] ? <div className="react-gitai-fix-result">{fixResults[r.id].pushSteps?.length ? <details className="react-review-commits"><summary>重推 notes 步骤（{fixResults[r.id].pushSteps!.filter((s) => s.ok).length}/{fixResults[r.id].pushSteps!.length} 成功）</summary><pre>{fixResults[r.id].pushSteps!.map((s) => `${s.ok ? "✓" : "✗"} ${s.label} — ${s.command}\n${s.stderr || s.stdout || ""}`).join("\n")}</pre></details> : null}{!fixResults[r.id].stillMissing ? <p className="react-effort-error react-fix-ok">✅ 重推 notes 后公司接口已确认标记。</p> : fixResults[r.id].piAgent ? <div className="react-gitai-agent-info"><span>{fixResults[r.id].piAgent!.dispatched ? "🤖" : "⚠️"} {fixResults[r.id].piAgent!.message}</span>{fixResults[r.id].piAgent!.sessionId ? <code>pi --session {fixResults[r.id].piAgent!.sessionId}</code> : null}</div> : null}</div> : null}</div><div className="react-card-side"><span className="react-effort-badge">{r.aiLines ?? 0} AI / {r.humanLines ?? 0} human</span><span className="react-muted">{r.branch || "branch n/a"}</span><code>{(r.commitSha || "").slice(0, 12)}</code>{canFix(r) ? <button className="react-fix-note-btn" onClick={() => fixNote(r)} disabled={!!fixingId}>{fixingId === r.id ? "补标中…" : "一键补标"}</button> : null}</div></motion.article>)}</div>}</PageChrome>
}
