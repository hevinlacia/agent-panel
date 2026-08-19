import { motion } from "framer-motion"
import { createPortal } from "react-dom"
import { ArrowLeft, Copy, RefreshCw, Server } from "lucide-react"
import { useEffect, useMemo, useState } from "react"
import type { ApiSessions, SessionInfo, SessionLogEntry, SessionLogPayload } from "../types"
import { fetchJson, useFetch } from "../lib/api"
import { formatDateTime, relAge } from "../lib/format"
import { EmptyCard, ErrorCard, LoadingCard, PageChrome, PanelHead } from "../components/ui"
import { statusPill } from "../features/requirements/badges"

export function SessionsPage() {
  const days = new URLSearchParams(window.location.search).get("days") || "7"
  const { data, error, loading, refresh } = useFetch<ApiSessions>(`/api/sessions?days=${encodeURIComponent(days)}`, [days])
  const sessions = data?.sessions || []
  return <PageChrome icon={<Server size={15} />} eyebrow="Pi Sessions" title="Sessions" description="只读浏览本机 pi session；PTY / terminal 已从新版移除。" actions={<button onClick={refresh}><RefreshCw size={15} />刷新</button>}><div className="react-tab-row">{[1, 3, 7, 14, 30, 0].map((d) => <a key={d} className={String(d) === days ? "active" : ""} href={`/sessions?days=${d}`}>{d === 0 ? "全部时间" : `近 ${d} 天`}</a>)}</div>{error ? <ErrorCard error={error} /> : loading ? <LoadingCard /> : <div className="react-card-list">{sessions.length === 0 ? <EmptyCard>暂无 pi session。</EmptyCard> : sessions.map((s, index) => <SessionCard key={s.id} session={s} index={index} />)}</div>}</PageChrome>
}

function SessionCard({ session, index }: { session: SessionInfo; index: number }) {
  return <motion.article className="react-list-card react-session-card" initial={{ opacity: 0, y: 12 }} animate={{ opacity: 1, y: 0 }} transition={{ delay: Math.min(index, 16) * 0.025 }} whileHover={{ y: -3 }}><div><span className="react-card-id">{session.id}</span><h3><a href={`/session?id=${encodeURIComponent(session.id)}`}>{session.title || session.id}</a></h3><p>{session.directory || "-"}</p><div className="react-card-meta"><span>{session.agent || "pi"}</span><span>{session.model || session.modelId || session.provider || "model n/a"}</span><span>更新 {relAge(session.updated || session.created)}</span><span>{session.messageCount || 0} messages</span></div></div><div className="react-card-side">{statusPill(session.status)}<span className="react-muted">{session.worktree || "-"}</span></div></motion.article>
}

export function SessionPage() {
  const id = new URLSearchParams(window.location.search).get("id") || ""
  const { data, error, loading } = useFetch<{ session: SessionInfo | null; terminalRemoved?: boolean }>(id ? `/api/session?id=${encodeURIComponent(id)}` : null, [id])
  const session = data?.session
  return <PageChrome icon={<Server size={15} />} eyebrow="Session" title={session?.title || id || "Session"} description="只读实时查看 pi session JSONL；关闭页面不会影响后台 agent。" actions={<a href="/sessions"><ArrowLeft size={15} />返回 Sessions</a>}>{error ? <ErrorCard error={error} /> : loading ? <LoadingCard /> : !session ? <EmptyCard>Session not found.</EmptyCard> : <div className="react-detail-grid"><section className="react-panel"><PanelHead kicker="Overview" title="Session 信息" chip={statusPill(session.status)} /><div className="react-meta-grid"><span>ID <code>{session.id}</code></span><span>Agent {session.agent || "pi"}</span><span>Model {session.model || session.modelId || "-"}</span><span>Updated {formatDateTime(session.updated || session.created)}</span><span>Worktree {session.worktree || "-"}</span><span>Messages {session.messageCount || 0}</span></div><p className="react-detail-desc">{session.directory || "-"}</p></section><section className="react-panel"><PanelHead kicker="Read Only" title="打开终端（可选）" /><p className="react-muted">页面日志是只读的，不会启动第二个 pi 进程。若你需要手动接管，可复制命令打开；关闭该终端不会影响 Agent Panel 已派发的后台总结进程。</p><code className="react-command">pi --session {session.id}</code></section><SessionLogPanel sessionId={session.id} /></div>}</PageChrome>
}

function SessionLogPanel({ sessionId }: { sessionId: string }) {
  const [entries, setEntries] = useState<SessionLogEntry[]>([])
  const [cursor, setCursor] = useState(0)
  const [total, setTotal] = useState(0)
  const [path, setPath] = useState("")
  const [live, setLive] = useState(true)
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [copied, setCopied] = useState(false)
  const load = async (reset = false) => {
    if (!sessionId || loading) return
    setLoading(true)
    setError(null)
    const nextCursor = reset ? 0 : cursor
    try {
      const payload = await fetchJson<SessionLogPayload>(`/api/session/log?id=${encodeURIComponent(sessionId)}&cursor=${nextCursor}&limit=120`)
      setPath(payload.path)
      setTotal(payload.total)
      setCursor(payload.cursor)
      setEntries((cur) => reset ? payload.entries : [...cur, ...payload.entries.filter((e) => !cur.some((x) => x.line === e.line))])
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err))
    } finally {
      setLoading(false)
    }
  }
  useEffect(() => { setEntries([]); setCursor(0); setTotal(0); setPath(""); setError(null); setLive(true) }, [sessionId])
  useEffect(() => { if (sessionId) load(true) }, [sessionId])
  useEffect(() => {
    if (!live || !sessionId) return
    const timer = window.setInterval(() => { load(false) }, 2000)
    return () => window.clearInterval(timer)
  }, [live, sessionId, cursor, loading])
  useEffect(() => {
    if (!live) return
    const el = document.getElementById("session-log-bottom")
    el?.scrollIntoView({ block: "end" })
  }, [entries.length, live])
  const copyCommand = async () => {
    await navigator.clipboard.writeText(`pi --session ${sessionId}`)
    setCopied(true)
    window.setTimeout(() => setCopied(false), 1600)
  }
  return <section className="react-panel react-session-log-panel"><PanelHead kicker="Live Log" title="实时过程（只读）" chip={live ? "LIVE" : "PAUSED"} /><p className="react-muted">从 pi session JSONL 增量读取，不向 session 写入任何内容；适合观察自动经验总结 agent 的完整过程。</p><div className="react-actions"><button onClick={() => load(false)} disabled={loading}><RefreshCw size={15} className={loading ? "react-spin" : ""} />刷新</button><button type="button" onClick={() => setLive((v) => !v)}>{live ? "暂停实时" : "继续实时"}</button><button type="button" onClick={() => load(true)}>重新读取</button><button type="button" onClick={copyCommand}><Copy size={13} />{copied ? "已复制" : "复制终端命令"}</button>{path ? <code className="react-doc-path">{path}</code> : null}</div>{error ? <ErrorCard error={error} /> : null}<div className="react-session-log"><div className="react-card-meta"><span>{entries.length}/{total} entries</span><span>cursor {cursor}</span></div>{entries.length === 0 && loading ? <LoadingCard label="正在读取 session 日志…" /> : entries.length === 0 ? <EmptyCard>暂无日志内容。</EmptyCard> : entries.map((entry) => <SessionLogEntryView key={entry.line} entry={entry} />)}<div id="session-log-bottom" /></div></section>
}

function SessionLogEntryView({ entry }: { entry: SessionLogEntry }) {
  const kind = entry.type || "event"
  const text = (entry.text || "").trim()
  return <article className={`react-session-log-entry react-session-log-${kind}`}><header><span>{kind}</span><strong>{entry.title || kind}</strong><em>#{entry.line}{entry.timestamp ? ` · ${formatDateTime(entry.timestamp)}` : ""}</em></header>{entry.tools?.length ? <div className="react-chip-list">{entry.tools.map((tool, i) => <span key={`${entry.line}-${i}-${tool.id || tool.name}`}>{tool.kind === "result" ? "✓" : "↪"} {tool.name || "tool"}</span>)}</div> : null}{text ? <pre>{text}</pre> : <p className="react-muted">无文本内容</p>}</article>
}

export function SessionChipList({ sessionIds }: { sessionIds: string[] }) {
  return <div className="react-chip-list">{sessionIds.map((sid) => <a key={sid} href={`/session?id=${encodeURIComponent(sid)}`}>{sid.slice(0, 8)}…</a>)}</div>
}

export function SessionListModal({ sessionIds, onClose }: { sessionIds: string[]; onClose: () => void }) {
  const { data, loading, error } = useFetch<{ sessions: SessionInfo[]; missing: string[] }>(sessionIds.length ? `/api/sessions/resolve?ids=${encodeURIComponent(sessionIds.join(","))}` : null, [sessionIds.join(",")])
  const byId = useMemo(() => {
    const map = new Map<string, SessionInfo>()
    for (const s of data?.sessions || []) map.set(s.id, s)
    return map
  }, [data])
  const [copiedId, setCopiedId] = useState<string | null>(null)
  const copyOpen = async (sid: string) => {
    try {
      await navigator.clipboard.writeText(`pi --session ${sid}`)
      setCopiedId(sid)
      window.setTimeout(() => setCopiedId((cur) => cur === sid ? null : cur), 1600)
    } catch {
      setCopiedId(null)
    }
  }
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => { if (e.key === "Escape") onClose() }
    window.addEventListener("keydown", onKey)
    return () => window.removeEventListener("keydown", onKey)
  }, [onClose])
  const sessions = sessionIds.map((sid) => byId.get(sid) ?? { id: sid, title: "(未知 session)", status: "" })
  return createPortal(
    <div className="react-modal-backdrop" onClick={onClose}>
      <div className="react-modal" role="dialog" aria-modal="true" onClick={(e) => e.stopPropagation()}>
        <div className="react-modal-head">
          <div><span>Sessions</span><h3>关联 Session（{sessions.length}）</h3></div>
          <button type="button" className="react-modal-close" onClick={onClose} aria-label="关闭">×</button>
        </div>
        {loading ? <LoadingCard label="正在加载 session 列表…" /> : error ? <ErrorCard error={error} /> : sessions.length === 0 ? <div className="react-modal-body"><p className="react-muted">暂无关联 session。</p></div> : (
          <ul className="react-session-list">
            {sessions.map((s) => (
              <li key={s.id} className="react-session-row">
                <div className="react-session-info">
                  <strong title={s.title}>{s.title || "(无标题)"}</strong>
                  <code>{s.id}</code>
                </div>
                <div className="react-session-actions">
                  <a href={`/session?id=${encodeURIComponent(s.id)}`} title="查看 session 详情">详情</a>
                  <button type="button" className="react-copy-link-btn" onClick={() => copyOpen(s.id)} title="复制打开命令到剪贴板"><Copy size={13} />{copiedId === s.id ? "已复制" : "复制命令"}</button>
                </div>
              </li>
            ))}
          </ul>
        )}
      </div>
    </div>,
    document.body,
  )
}
