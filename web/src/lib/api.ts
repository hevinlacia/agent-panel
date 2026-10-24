import { useEffect, useState } from "react"
import { getApiBase, getAuthHeaders } from "./connection"

/** 相对路径拼接当前连接的 baseURL；绝对地址（如 dsh url）原样返回。 */
export function fullUrl(url: string): string {
  if (/^https?:\/\//i.test(url)) return url
  return `${getApiBase()}${url}`
}

function mergeHeaders(init?: RequestInit): Record<string, string> {
  const merged: Record<string, string> = { ...getAuthHeaders() }
  const extra = init?.headers
  if (!extra) return merged
  if (extra instanceof Headers) {
    extra.forEach((value, key) => { merged[key] = value })
  } else if (Array.isArray(extra)) {
    for (const [key, value] of extra) merged[key] = value
  } else {
    Object.assign(merged, extra)
  }
  return merged
}

// ---- 访问 token（docs 模式容器 hub 必需；local 模式后端不启用鉴权，token 被忽略） ----
const TOKEN_STORAGE_KEY = "panelToken"

export function getPanelToken(): string {
  try {
    return localStorage.getItem(TOKEN_STORAGE_KEY) ?? ""
  } catch {
    return ""
  }
}

export function setPanelToken(token: string): void {
  try {
    localStorage.setItem(TOKEN_STORAGE_KEY, token)
  } catch {
    /* localStorage 不可用时忽略（如隐私模式） */
  }
}

let unauthorizedHandler: (() => void) | null = null

export function setUnauthorizedHandler(fn: (() => void) | null): void {
  unauthorizedHandler = fn
}

async function fetchWithToken(url: string, init?: RequestInit): Promise<Response> {
  const headers = new Headers(init?.headers)
  const token = getPanelToken()
  if (token) headers.set("X-Panel-Token", token)
  const withToken: RequestInit = { cache: "no-store", ...init, headers }
  // 连接层 baseURL + 连接级 Bearer（getAuthHeaders）与全局 X-Panel-Token 合并；显式 headers 优先。
  const res = await fetch(fullUrl(url), { ...withToken, headers: mergeHeaders(withToken) })
  if (res.status === 401) unauthorizedHandler?.()
  return res
}

export async function fetchJson<T>(url: string, init?: RequestInit): Promise<T> {
  const res = await fetchWithToken(url, init)
  if (!res.ok) {
    if (res.status === 401) {
      throw new Error("认证失败 (401)：token 无效或缺失，请在「连接」页检查当前服务器的 token 配置")
    }
    const text = await res.text().catch(() => "")
    throw new Error(text || `HTTP ${res.status}`)
  }
  return res.json() as Promise<T>
}

export async function postJson<T>(url: string, data: unknown): Promise<T> {
  return fetchJson<T>(url, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(data),
  })
}

export async function putJson<T>(url: string, data: unknown): Promise<T> {
  return fetchJson<T>(url, {
    method: "PUT",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(data),
  })
}

export async function patchJson<T>(url: string, data: unknown): Promise<T> {
  return fetchJson<T>(url, {
    method: "PATCH",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(data),
  })
}

export async function postForm<T>(url: string, data: Record<string, string>): Promise<T> {
  return fetchJson<T>(url, {
    method: "POST",
    headers: { "Content-Type": "application/x-www-form-urlencoded" },
    body: new URLSearchParams(data),
  })
}

export function useFetch<T>(url: string | null, deps: unknown[] = [], intervalMs?: number) {
  const [data, setData] = useState<T | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [loading, setLoading] = useState(Boolean(url))
  const [nonce, setNonce] = useState(0)

  useEffect(() => {
    if (!url) return
    let cancelled = false
    setLoading(true)
    fetchJson<T>(`${url}${url.includes("?") ? "&" : "?"}t=${Date.now()}`)
      .then((value) => { if (!cancelled) { setData(value); setError(null) } })
      .catch((err: Error) => { if (!cancelled) setError(err.message) })
      .finally(() => { if (!cancelled) setLoading(false) })
    return () => { cancelled = true }
  }, [url, nonce, ...deps])

  useEffect(() => {
    if (!url || !intervalMs) return
    const timer = setInterval(() => setNonce((v) => v + 1), intervalMs)
    return () => clearInterval(timer)
  }, [url, intervalMs])

  return { data, error, loading, refresh: () => setNonce((v) => v + 1) }
}
