/**
 * Role: /connect 页 —— 多 Agent Panel 服务器连接管理（选择/添加/编辑/测试）。
 * Public surface: ConnectPage component。
 * Constraints: 连接配置持久化在 localStorage（web/src/lib/connection.ts）；
 *   token 输入用 password 形态可切换显示，不回显到任何日志。
 */
import { useState } from "react"
import { Check, Eye, EyeOff, Loader2, Pencil, Plus, Server, Trash2, Wifi, WifiOff } from "lucide-react"
import { EmptyCard, PageChrome, PanelHead } from "../components/ui"
import {
  activeConnectionLabel,
  getActiveId,
  listConnections,
  LOCAL_CONNECTION_ID,
  normalizeUrl,
  removeConnection,
  setActiveId,
  testConnection,
  upsertConnection,
  type ConnectionTestResult,
  type PanelConnection,
} from "../lib/connection"

type TestState = { status: "idle" | "testing" } & ({ result: null } | { result: ConnectionTestResult })

function idleTest(): TestState {
  return { status: "idle", result: null }
}

function ConnectionCard({ conn, active, onActivate, onEdit, onDelete, onTest, test }: {
  conn: PanelConnection
  active: boolean
  onActivate: () => void
  onEdit: () => void
  onDelete: () => void
  onTest: () => void
  test: TestState
}) {
  const isLocal = conn.id === LOCAL_CONNECTION_ID
  return (
    <article className={`react-connect-card ${active ? "active" : ""}`}>
      <div className="react-connect-card-head">
        <span className={`react-connect-dot ${active ? "on" : ""}`} aria-hidden="true" />
        <strong>{conn.name}</strong>
        {active ? <em className="react-connect-badge">当前连接</em> : null}
        {isLocal ? <em className="react-connect-badge muted">同源</em> : null}
      </div>
      <code className="react-connect-url">{conn.url || "（面板自身地址）"}</code>
      <div className="react-connect-meta">
        <span>{conn.token ? "已配置 token" : "无 token"}</span>
        {test.status === "testing" ? (
          <span className="react-connect-testing"><Loader2 size={13} className="react-spin" /> 测试中…</span>
        ) : test.result ? (
          <span className={test.result.ok ? "react-connect-ok" : "react-connect-fail"}>
            {test.result.ok ? <Wifi size={13} /> : <WifiOff size={13} />} {test.result.detail}
          </span>
        ) : null}
      </div>
      <div className="react-connect-actions">
        {active ? (
          <button type="button" className="react-ghost-btn" disabled><Check size={13} /> 使用中</button>
        ) : (
          <button type="button" onClick={onActivate} title="切换到该服务器；页面会刷新以加载新连接的数据">使用此连接</button>
        )}
        <button type="button" className="react-ghost-btn" onClick={onTest}>测试</button>
        <button type="button" className="react-ghost-btn" onClick={onEdit} title={isLocal ? "编辑本机 token" : "编辑名称/地址/token"}><Pencil size={13} /> 编辑</button>
        {isLocal ? null : (
          <button type="button" className="react-ghost-btn danger" onClick={onDelete} title="删除该连接配置（不影响服务器本身）"><Trash2 size={13} /></button>
        )}
      </div>
    </article>
  )
}

function ConnectionForm({ initial, onSaved, onCancel }: {
  initial: PanelConnection
  onSaved: (list: PanelConnection[]) => void
  onCancel: () => void
}) {
  const isLocal = initial.id === LOCAL_CONNECTION_ID
  const [name, setName] = useState(initial.name)
  const [url, setUrl] = useState(initial.url)
  const [token, setToken] = useState(initial.token)
  const [showToken, setShowToken] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const submit = () => {
    if (!isLocal) {
      if (!name.trim()) return setError("请填写名称")
      const normalized = normalizeUrl(url)
      if (!/^https?:\/\/.+/i.test(normalized)) return setError("地址需以 http:// 或 https:// 开头")
      onSaved(upsertConnection({ id: initial.id, name: name.trim(), url: normalized, token: token.trim() }))
      return
    }
    onSaved(upsertConnection({ id: LOCAL_CONNECTION_ID, name: "本机", url: "", token: token.trim() }))
  }

  return (
    <section className="react-panel react-connect-form">
      <PanelHead kicker={isLocal ? "Local Token" : initial.id ? "Edit Connection" : "Add Connection"} title={isLocal ? "本机 token" : initial.id ? "编辑连接" : "添加服务器"} />
      {isLocal ? (
        <p className="react-muted">本机即当前浏览器访问的面板自身；仅在服务端配置了 PANEL_AUTH_TOKEN 时需要填写对应 token。</p>
      ) : null}
      <label className="react-connect-field">
        <span>名称</span>
        <input value={name} onChange={(e) => setName(e.target.value)} disabled={isLocal} placeholder="如：家里服务器 / 公司电脑" />
      </label>
      {isLocal ? null : (
        <label className="react-connect-field">
          <span>地址</span>
          <input value={url} onChange={(e) => setUrl(e.target.value)} placeholder="https://box.xxxx.ts.net 或 http://192.168.1.10:7331" inputMode="url" autoCapitalize="off" spellCheck={false} />
        </label>
      )}
      <label className="react-connect-field">
        <span>Token（可选）</span>
        <div className="react-connect-token-row">
          <input type={showToken ? "text" : "password"} value={token} onChange={(e) => setToken(e.target.value)} placeholder="PANEL_AUTH_TOKEN" autoCapitalize="off" autoComplete="off" spellCheck={false} />
          <button type="button" className="react-ghost-btn" onClick={() => setShowToken((v) => !v)} aria-label={showToken ? "隐藏 token" : "显示 token"}>
            {showToken ? <EyeOff size={14} /> : <Eye size={14} />}
          </button>
        </div>
      </label>
      {error ? <p className="react-effort-error">{error}</p> : null}
      <div className="react-connect-actions">
        <button type="button" onClick={submit}><Check size={13} /> 保存</button>
        <button type="button" className="react-ghost-btn" onClick={onCancel}>取消</button>
      </div>
    </section>
  )
}

export function ConnectPage() {
  const [connections, setConnections] = useState<PanelConnection[]>(() => listConnections())
  const [activeId, setActiveIdState] = useState<string>(() => getActiveId())
  const [editing, setEditing] = useState<PanelConnection | null>(null)
  const [adding, setAdding] = useState(false)
  const [tests, setTests] = useState<Record<string, TestState>>({})

  const refresh = (list: PanelConnection[]) => {
    setConnections(list)
    setActiveIdState(getActiveId())
  }

  const activate = (id: string) => {
    setActiveId(id)
    // 切换连接后整页刷新，确保所有页面的 in-flight 数据/WS 都走新连接。
    window.location.href = "/dashboard"
  }

  const runTest = (conn: PanelConnection) => {
    setTests((prev) => ({ ...prev, [conn.id]: { status: "testing", result: null } }))
    void testConnection(conn.url).then((result) => {
      setTests((prev) => ({ ...prev, [conn.id]: { status: "idle", result } }))
    })
  }

  const del = (conn: PanelConnection) => {
    if (!window.confirm(`删除连接「${conn.name}」？仅移除本机配置，不影响服务器。`)) return
    refresh(removeConnection(conn.id))
  }

  return (
    <PageChrome
      icon={<Server size={15} />}
      eyebrow="Connection"
      title="服务器连接"
      description={`当前：${activeConnectionLabel()}。添加公司电脑（VPN 内网）或个人服务器（Tailscale）等多台 Agent Panel，点击切换；连接配置仅保存在本机浏览器。`}
    >
      {adding || editing ? (
        <ConnectionForm
          initial={editing ?? { id: `conn-${Date.now()}`, name: "", url: "", token: "" }}
          onSaved={(list) => { refresh(list); setEditing(null); setAdding(false) }}
          onCancel={() => { setEditing(null); setAdding(false) }}
        />
      ) : null}
      <section className="react-panel">
        <PanelHead
          kicker="Servers"
          title="连接列表"
          chip={String(connections.length)}
        />
        <div className="react-actions react-connect-toolbar">
          <button type="button" onClick={() => setAdding(true)}><Plus size={13} /> 添加服务器</button>
        </div>
        {connections.length === 0 ? <EmptyCard>暂无连接配置。</EmptyCard> : (
          <div className="react-connect-list">
            {connections.map((conn) => (
              <ConnectionCard
                key={conn.id}
                conn={conn}
                active={conn.id === activeId}
                onActivate={() => activate(conn.id)}
                onEdit={() => { setAdding(false); setEditing(conn) }}
                onDelete={() => del(conn)}
                onTest={() => runTest(conn)}
                test={tests[conn.id] ?? idleTest()}
              />
            ))}
          </div>
        )}
      </section>
    </PageChrome>
  )
}
