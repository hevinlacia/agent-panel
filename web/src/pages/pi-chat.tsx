import { motion } from "framer-motion"
import { ArrowLeft, Bot, OctagonX, CornerDownLeft, Zap } from "lucide-react"
import { useCallback, useEffect, useRef, useState } from "react"
import { fetchJson } from "../lib/api"
import { EmptyCard, ErrorCard, LoadingCard, PageChrome } from "../components/ui"

/**
 * Pi 会话聊天页：通过 /ws/pi-chat 连接 agent-panel 后端，
 * 后端 attach（复用或 spawn）`pi --mode rpc` 子进程并原样转发 JSONL 协议。
 * pi 进程挂在 systemd 服务下，与浏览器/SSH 生命周期解耦；
 * session JSONL 持久化在 ~/.pi/agent/sessions/，进程退出后重新 attach 即可恢复。
 */

type Block =
  | { kind: "text"; text: string }
  | { kind: "thinking"; text: string }
  | { kind: "tool"; id: string; name: string; args: string; status: "pending" | "running" | "done" }

type UIMessage = { role: "user" | "assistant"; blocks: Block[]; streaming: boolean }

type ApiOpen = { sessionId: string; isNew: boolean; title: string | null; cwd: string }

const uid = () => Math.random().toString(36).slice(2)

export function PiChatPage() {
  const params = new URLSearchParams(window.location.search)
  const reqId = params.get("req") || params.get("reqId") || ""
  const explicitSession = params.get("session") || params.get("sessionId") || ""

  const [open, setOpen] = useState<ApiOpen | null>(null)
  const [openError, setOpenError] = useState("")
  const [messages, setMessages] = useState<UIMessage[]>([])
  const [input, setInput] = useState("")
  const [busy, setBusy] = useState(false)
  const [status, setStatus] = useState<"connecting" | "live" | "closed" | "exited">("connecting")
  const [exitInfo, setExitInfo] = useState<{ code: number | null; stderrTail: string[] } | null>(null)
  const [modelLabel, setModelLabel] = useState("")

  const wsRef = useRef<WebSocket | null>(null)
  const bottomRef = useRef<HTMLDivElement | null>(null)

  // 解析默认 session（reqId → 需求最近关联 session；无则新建并绑定）。
  useEffect(() => {
    let cancelled = false
    const qs = explicitSession ? `sessionId=${encodeURIComponent(explicitSession)}`
      : reqId ? `reqId=${encodeURIComponent(reqId)}` : ""
    if (!qs) { setOpenError("缺少 req 或 session 参数，请从需求详情页进入。"); return }
    fetchJson<ApiOpen>(`/api/pi-chat/open?${qs}`)
      .then((d) => { if (!cancelled) setOpen(d) })
      .catch((e) => { if (!cancelled) setOpenError(String(e.message || e)) })
    return () => { cancelled = true }
  }, [reqId, explicitSession])

  // 消息列表自动滚底。
  useEffect(() => { bottomRef.current?.scrollIntoView({ behavior: "smooth" }) }, [messages])

  const patchLast = useCallback((fn: (m: UIMessage) => UIMessage) => {
    setMessages((prev) => (prev.length ? [...prev.slice(0, -1), fn(prev[prev.length - 1])] : prev))
  }, [])

  const appendTextToLast = useCallback((kind: "text" | "thinking", delta: string) => {
    patchLast((m) => {
      const blocks = [...m.blocks]
      const last = blocks[blocks.length - 1]
      const text = last && last.kind === kind ? last.text + delta : delta
      if (last && last.kind === kind) blocks[blocks.length - 1] = kind === "text" ? { kind: "text", text } : { kind: "thinking", text }
      else blocks.push({ kind, text })
      return { ...m, blocks }
    })
  }, [patchLast])

  const upsertToolBlock = useCallback((tool: { id?: string; name?: string; args?: unknown }) => {
    patchLast((m) => {
      const blocks = [...m.blocks]
      const idx = tool.id ? blocks.findIndex((b) => b.kind === "tool" && b.id === tool.id) : -1
      const entry: Block = {
        kind: "tool",
        id: tool.id || uid(),
        name: tool.name || "tool",
        args: tool.args ? JSON.stringify(tool.args, null, 2) : "",
        status: "running",
      }
      if (idx >= 0) {
        const b = blocks[idx]
        if (b.kind === "tool") blocks[idx] = { ...b, name: entry.name, args: entry.args || b.args }
      }
      else blocks.push(entry)
      return { ...m, blocks }
    })
  }, [patchLast])

  const markToolDone = useCallback((toolCallId: string | undefined) => {
    if (!toolCallId) return
    setMessages((prev) => prev.map((m) => ({
      ...m,
      blocks: m.blocks.map((b) => (b.kind === "tool" && b.id === toolCallId ? { ...b, status: "done" as const } : b)),
    })))
  }, [])

  // 把 RPC AgentMessage（历史 / message_end）转成 UI 消息。
  const renderAgentMessage = useCallback((raw: any): UIMessage | null => {
    const role = raw?.role === "user" ? "user" : raw?.role === "assistant" ? "assistant" : null
    if (!role) return null
    const blocks: Block[] = []
    const content: any[] = Array.isArray(raw?.content) ? raw.content : typeof raw?.content === "string" ? [{ type: "text", text: raw.content }] : []
    for (const c of content) {
      if (c?.type === "text" && c.text) blocks.push({ kind: "text", text: c.text })
      else if (c?.type === "thinking" && c.thinking) blocks.push({ kind: "thinking", text: c.thinking })
      else if (c?.type === "toolCall" && c.id) blocks.push({ kind: "tool", id: c.id, name: c.name || "tool", args: c.arguments ? JSON.stringify(c.arguments, null, 2) : "", status: "done" })
    }
    return blocks.length ? { role, blocks, streaming: false } : null
  }, [])

  // WebSocket 生命周期：open 解析成功后建立连接。
  useEffect(() => {
    if (!open) return
    let closedByUs = false
    const proto = window.location.protocol === "https:" ? "wss:" : "ws:"
    const ws = new WebSocket(`${proto}//${window.location.host}/ws/pi-chat?session=${encodeURIComponent(open.sessionId)}&cwd=${encodeURIComponent(open.cwd)}`)
    wsRef.current = ws
    setStatus("connecting")

    const onMessage = (ev: MessageEvent) => {
      let data: any
      try { data = JSON.parse(ev.data) } catch { return }
      switch (data.type) {
        case "panel_attached":
          setStatus("live")
          // 拉取状态与历史消息（get_messages 返回完整对话，用于恢复渲染）。
          ws.send(JSON.stringify({ type: "get_state" }))
          ws.send(JSON.stringify({ type: "get_messages" }))
          return
        case "panel_rpc_exit":
          setStatus("exited")
          setExitInfo({ code: data.exitCode ?? null, stderrTail: Array.isArray(data.stderrTail) ? data.stderrTail : [] })
          return
        case "response":
          if (data.command === "get_messages" && data.success) {
            const history = (data.data?.messages || []).map(renderAgentMessage).filter(Boolean)
            setMessages(history as UIMessage[])
          } else if (data.command === "get_state" && data.success) {
            const m = data.data?.model
            if (m) setModelLabel(`${m.provider || ""}/${m.modelId || m.id || ""}`.replace(/^\/+/, ""))
          }
          return
        case "message_start": {
          const msg = renderAgentMessage(data.message)
          if (msg?.role === "user") return // user 消息本地即时渲染，忽略 RPC 回显避免重复。
          if (msg) { setMessages((prev) => [...prev, { ...msg, streaming: true }]); return }
          setMessages((prev) => [...prev, { role: "assistant", blocks: [], streaming: true }])
          return
        }
        case "message_update": {
          const e = data.assistantMessageEvent || {}
          if (e.type === "text_delta") appendTextToLast("text", e.delta || "")
          else if (e.type === "thinking_delta") appendTextToLast("thinking", e.delta || "")
          else if (e.type === "toolcall_start") upsertToolBlock({ id: e.id, name: e.toolName })
          else if (e.type === "toolcall_end") upsertToolBlock({ id: e.toolCall?.id || e.id, name: e.toolCall?.name, args: e.toolCall?.arguments })
          return
        }
        case "message_end": {
          const msg = renderAgentMessage(data.message)
          if (!msg) { setMessages((prev) => (prev.length ? [...prev.slice(0, -1), { ...prev[prev.length - 1], streaming: false }] : prev)); return }
          if (msg.role === "user") return
          setMessages((prev) => {
            // 用完整 message 替换正在流式的最后一条 assistant 消息；无流式消息时追加。
            const last = prev[prev.length - 1]
            if (last && last.role === "assistant" && last.streaming) return [...prev.slice(0, -1), msg]
            return [...prev, msg]
          })
          return
        }
        case "tool_execution_end":
          markToolDone(data.toolCallId || data.id)
          return
        case "agent_start":
          setBusy(true)
          return
        case "agent_settled":
        case "agent_end":
          if (data.type === "agent_settled" || !data.willRetry) setBusy(false)
          return
        case "auto_retry_start":
          setBusy(true)
          return
        default:
          return
      }
    }

    ws.onmessage = onMessage
    ws.onopen = () => setStatus("live")
    ws.onclose = () => { if (!closedByUs) setStatus("closed") }
    ws.onerror = () => setStatus("closed")
    return () => {
      closedByUs = true
      ws.close()
      wsRef.current = null
    }
  }, [open, appendTextToLast, markToolDone, patchLast, renderAgentMessage, upsertToolBlock])

  const send = () => {
    const text = input.trim()
    if (!text || !wsRef.current || wsRef.current.readyState !== WebSocket.OPEN) return
    const payload = busy
      ? { type: "prompt", message: text, streamingBehavior: "followUp" }
      : { type: "prompt", message: text }
    wsRef.current.send(JSON.stringify(payload))
    setMessages((prev) => [...prev, { role: "user", blocks: [{ kind: "text", text }], streaming: false }])
    setInput("")
    setBusy(true)
  }

  const abort = () => wsRef.current?.send(JSON.stringify({ type: "abort" }))

  if (openError) {
    return <PageChrome icon={<Bot size={15} />} eyebrow="Pi Agent" title="会话无法打开"><ErrorCard error={openError} /><div className="react-actions"><a className="react-ghost-btn" href={reqId ? `/requirement?id=${encodeURIComponent(reqId)}` : "/sessions"}><ArrowLeft size={15} />返回</a></div></PageChrome>
  }
  if (!open) return <PageChrome icon={<Bot size={15} />} eyebrow="Pi Agent" title="Pi 会话"><LoadingCard label="正在解析会话…" /></PageChrome>

  const statusChip = status === "live" ? (busy ? "运行中" : "空闲") : status === "connecting" ? "连接中" : status === "exited" ? "进程退出" : "连接断开"

  return (
    <PageChrome
      icon={<Bot size={15} />}
      eyebrow="Pi Agent"
      title={open.title || `会话 ${open.sessionId.slice(0, 8)}`}
      description={`${open.isNew ? "已新建会话并绑定" : "最近使用的会话"} · cwd ${open.cwd}${reqId ? ` · 需求 ${reqId}` : ""}`}
      actions={<a className="react-ghost-btn" href={reqId ? `/requirement?id=${encodeURIComponent(reqId)}` : "/sessions"}><ArrowLeft size={15} />返回</a>}
    >
      <section className="react-panel react-pichat-panel">
        <div className="react-pichat-status">
          <span className={`react-pichat-dot ${status === "live" ? "on" : ""}`} /> {statusChip}
          {modelLabel ? <code>{modelLabel}</code> : null}
          {reqId ? <a href={`/requirement?id=${encodeURIComponent(reqId)}`}>需求 {reqId}</a> : null}
          <em>进程运行于 agent-panel 服务内，关闭页面 / SSH 断开不影响执行；重新打开本页继续同一会话。</em>
        </div>

        {status === "exited" ? (
          <div className="react-pichat-exit">
            <p>pi 进程已退出（exit code: {exitInfo?.code ?? "unknown"}）。关闭本页后重新打开会自动用同一会话重启进程，上下文不丢失。</p>
            {exitInfo?.stderrTail?.length ? <pre>{exitInfo.stderrTail.join("\n")}</pre> : null}
          </div>
        ) : null}
        {status === "closed" ? <p className="react-pichat-note">与后端连接断开，刷新页面重新接入（pi 进程仍在后台运行）。</p> : null}

        <div className="react-pichat-stream">
          {messages.length === 0 && status === "live" ? <EmptyCard>会话为空，输入第一条消息开始。</EmptyCard> : null}
          {messages.map((m, i) => (
            <motion.div key={i} initial={{ opacity: 0, y: 6 }} animate={{ opacity: 1, y: 0 }} className={`react-pichat-msg ${m.role}`}>
              <span className="react-pichat-role">{m.role === "user" ? "你" : "pi"}</span>
              <div className="react-pichat-body">
                {m.blocks.map((b, j) => {
                  if (b.kind === "text") return <p key={j} className="react-pichat-text">{b.text}</p>
                  if (b.kind === "thinking") return <details key={j} className="react-pichat-thinking"><summary>thinking</summary><pre>{b.text}</pre></details>
                  return (
                    <details key={j} className={`react-pichat-tool ${b.status}`} open={b.status !== "done"}>
                      <summary>⚙ {b.name} {b.status === "running" ? "…" : ""}</summary>
                      {b.args ? <pre>{b.args}</pre> : null}
                    </details>
                  )
                })}
                {m.streaming && !m.blocks.length ? <p className="react-pichat-text react-pichat-typing">…</p> : null}
              </div>
            </motion.div>
          ))}
          <div ref={bottomRef} />
        </div>

        <div className="react-pichat-input">
          <textarea
            value={input}
            onChange={(e) => setInput(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" && !e.shiftKey && !e.nativeEvent.isComposing) { e.preventDefault(); send() }
            }}
            placeholder={busy ? "pi 正在运行；输入内容将排队（followUp）…" : "输入消息，Enter 发送，Shift+Enter 换行"}
            rows={2}
          />
          <div className="react-pichat-input-actions">
            {busy ? <button type="button" className="react-ghost-btn" onClick={abort}><OctagonX size={15} />停止</button> : null}
            <button type="button" onClick={send} disabled={!input.trim() || status !== "live"}>
              {busy ? <><CornerDownLeft size={15} />排队发送</> : <><Zap size={15} />发送</>}
            </button>
          </div>
        </div>
      </section>
    </PageChrome>
  )
}

// fetchJson 复用 web/src/lib/api.ts 的实现（cache: no-store + 非 2xx 报错）。
