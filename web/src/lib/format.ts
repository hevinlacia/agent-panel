export function formatDuration(ms: number): string {
  const safe = Math.max(0, ms)
  const sec = Math.floor(safe / 1000)
  if (sec < 60) return `${sec}秒`
  const min = Math.floor(sec / 60)
  if (min < 60) return `${min}分钟`
  const hr = Math.floor(min / 60)
  const remainMin = min % 60
  if (hr < 24) return remainMin > 0 ? `${hr}小时${remainMin}分钟` : `${hr}小时`
  const day = Math.floor(hr / 24)
  const remainHr = hr % 24
  return remainHr > 0 ? `${day}天${remainHr}小时` : `${day}天`
}

export function formatDate(ms?: number): string {
  if (!ms) return "-"
  return new Date(ms).toLocaleDateString("zh-CN")
}

export function formatDateTime(ms?: number): string {
  if (!ms) return "-"
  return new Date(ms).toLocaleString("zh-CN")
}

export function relAge(ms?: number): string {
  if (!ms) return "-"
  const diff = Math.max(0, Date.now() - ms)
  if (diff < 60_000) return "刚刚"
  if (diff < 3_600_000) return `${Math.floor(diff / 60_000)}分钟前`
  if (diff < 86_400_000) return `${Math.floor(diff / 3_600_000)}小时前`
  return `${Math.floor(diff / 86_400_000)}天前`
}

export function parseOnesRef(raw?: string): { raw: string; url: string | null; label: string } | null {
  const value = (raw || "").trim()
  if (!value) return null
  // A pasted value may carry extra text before/around the ONES link
  // (e.g. "JTYC-1347611 上架策略新增指定库位 https://.../issue/JTYC-1347611"),
  // so search for the first http(s) URL anywhere instead of requiring it at the start.
  const urlMatch = value.match(/https?:\/\/[^\s<>"']+/i)
  if (urlMatch) {
    const url = urlMatch[0]
    let label = url
    try {
      const parsed = new URL(url)
      const issueCode = parsed.hash.match(/(?:^|\/)issue\/([^/?#]+)/i)?.[1]
      const pathSegment = parsed.pathname.split("/").filter(Boolean).pop()
      const segment = issueCode || pathSegment
      if (segment && segment.length <= 60) label = decodeURIComponent(segment)
    } catch { /* keep url as label */ }
    return { raw: value, url, label }
  }
  return { raw: value, url: null, label: value }
}

export function splitCsvText(value?: string | string[]): string[] {
  if (Array.isArray(value)) return value.map((v) => v.trim()).filter(Boolean)
  return (value || "").split(/[，,\n]/).map((v) => v.trim()).filter(Boolean)
}

export function joinList(value?: string[]): string {
  return (value || []).join(", ")
}
