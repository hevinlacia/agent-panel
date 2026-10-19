import { useEffect, useState } from "react"
import { getApiBase, getAuthHeaders } from "./connection"

/** 相对路径拼接当前连接的 baseURL；绝对地址（如 dsh url）原样返回。 */
function fullUrl(url: string): string {
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

export async function fetchJson<T>(url: string, init?: RequestInit): Promise<T> {
  const res = await fetch(fullUrl(url), { cache: "no-store", ...init, headers: mergeHeaders(init) })
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

export function useFetch<T>(url: string | null, deps: unknown[] = []) {
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

  return { data, error, loading, refresh: () => setNonce((v) => v + 1) }
}
