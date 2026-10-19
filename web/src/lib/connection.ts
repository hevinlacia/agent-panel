/**
 * Role: 多服务器连接配置（localStorage 持久化）与 API/WS 地址推导。
 * Public surface: listConnections / upsertConnection / removeConnection /
 *   getActiveConnection / setActiveId / getApiBase / getAuthHeaders /
 *   buildWsUrl / activeConnectionLabel / testConnection
 * Constraints: 纯浏览器 localStorage；url 为空字符串 = 同源直连（桌面现状默认）；
 *   token 仅存本地并注入请求头/query，禁止写入日志或错误信息。
 */

export interface PanelConnection {
  id: string
  name: string
  /** 空字符串 = 同源（面板自身源）；远程示例 https://box.xxxx.ts.net 或 http://192.168.1.10:7331 */
  url: string
  /** 服务端 PANEL_AUTH_TOKEN 对应值，可为空 */
  token: string
}

export const LOCAL_CONNECTION_ID = "local"

const LS_CONNECTIONS = "panel.connections.v1"
const LS_ACTIVE = "panel.activeConnection.v1"

function isBrowser(): boolean {
  return typeof window !== "undefined" && typeof localStorage !== "undefined"
}

function defaultLocal(): PanelConnection {
  return { id: LOCAL_CONNECTION_ID, name: "本机", url: "", token: "" }
}

export function normalizeUrl(url: string): string {
  return url.trim().replace(/\/+$/, "")
}

export function listConnections(): PanelConnection[] {
  if (!isBrowser()) return [defaultLocal()]
  const list: PanelConnection[] = [defaultLocal()]
  try {
    const raw = localStorage.getItem(LS_CONNECTIONS)
    if (raw) {
      const parsed = JSON.parse(raw) as unknown
      if (Array.isArray(parsed)) {
        for (const item of parsed) {
          if (item && typeof item === "object" && typeof (item as PanelConnection).id === "string" && typeof (item as PanelConnection).name === "string") {
            const conn = item as PanelConnection
            list.push({ id: conn.id, name: conn.name, url: normalizeUrl(conn.url || ""), token: conn.token || "" })
          }
        }
      }
    }
  } catch {
    /* 配置损坏按空处理 */
  }
  return list
}

function saveRemote(list: PanelConnection[]): void {
  if (!isBrowser()) return
  const remote = list.filter((c) => c.id !== LOCAL_CONNECTION_ID)
  localStorage.setItem(LS_CONNECTIONS, JSON.stringify(remote))
}

export function upsertConnection(conn: PanelConnection): PanelConnection[] {
  const list = listConnections().filter((c) => c.id !== conn.id)
  if (conn.id === LOCAL_CONNECTION_ID) {
    // 本机记录只更新 token，url 恒为同源。
    const local = list.find((c) => c.id === LOCAL_CONNECTION_ID)
    if (local) local.token = conn.token
  } else {
    list.push({ ...conn, url: normalizeUrl(conn.url), token: conn.token })
  }
  saveRemote(list)
  return listConnections()
}

export function removeConnection(id: string): PanelConnection[] {
  const list = listConnections().filter((c) => c.id !== id || c.id === LOCAL_CONNECTION_ID)
  saveRemote(list)
  if (getActiveId() === id) setActiveId(LOCAL_CONNECTION_ID)
  return listConnections()
}

export function getActiveId(): string {
  if (!isBrowser()) return LOCAL_CONNECTION_ID
  return localStorage.getItem(LS_ACTIVE) || LOCAL_CONNECTION_ID
}

export function getActiveConnection(): PanelConnection {
  const id = getActiveId()
  return listConnections().find((c) => c.id === id) ?? defaultLocal()
}

export function setActiveId(id: string): void {
  if (!isBrowser()) return
  localStorage.setItem(LS_ACTIVE, id)
}

export function getApiBase(): string {
  return getActiveConnection().url
}

export function getAuthHeaders(): Record<string, string> {
  const token = getActiveConnection().token
  return token ? { Authorization: `Bearer ${token}` } : {}
}

/** WS 升级请求无法携带自定义 header，统一走 query token（服务端 auth 接缝同时接受两种形态）。 */
export function buildWsUrl(pathWithQuery: string): string {
  const conn = getActiveConnection()
  const base = conn.url.replace(/^http/, "ws")
  const sep = pathWithQuery.includes("?") ? "&" : "?"
  const auth = conn.token ? `${sep}token=${encodeURIComponent(conn.token)}` : ""
  return `${base}${pathWithQuery}${auth}`
}

export function activeConnectionLabel(): string {
  const conn = getActiveConnection()
  return conn.url ? conn.name : `${conn.name}（同源）`
}

export interface ConnectionTestResult {
  ok: boolean
  /** 成功 = 往返毫秒；失败 = 错误摘要（不含 token） */
  detail: string
}

/** 用 /health 测连通性与延迟；/health 在服务端 auth 豁免名单内，无需 token。 */
export async function testConnection(url: string): Promise<ConnectionTestResult> {
  const base = normalizeUrl(url)
  const started = performance.now()
  try {
    const res = await fetch(`${base}/health`, { cache: "no-store" })
    if (!res.ok) return { ok: false, detail: `HTTP ${res.status}` }
    const body = (await res.json().catch(() => null)) as { ok?: boolean } | null
    if (!body?.ok) return { ok: false, detail: "响应异常" }
    return { ok: true, detail: `${Math.round(performance.now() - started)} ms` }
  } catch (err) {
    return { ok: false, detail: err instanceof Error ? err.message.slice(0, 120) : "网络错误" }
  }
}
