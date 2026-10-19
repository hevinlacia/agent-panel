/**
 * Role: 视口断点检测 hook（移动布局切换）。
 * Public surface: useIsMobile()。
 * Constraints: 纯 CSS 断点镜像（820px）；不做 UA 嗅探，窗口跨断点实时响应。
 */
import { useEffect, useState } from "react"

const QUERY = "(max-width: 820px)"

export function useIsMobile(): boolean {
  const [isMobile, setIsMobile] = useState(() => typeof window !== "undefined" && window.matchMedia(QUERY).matches)
  useEffect(() => {
    const mq = window.matchMedia(QUERY)
    const onChange = (e: MediaQueryListEvent) => setIsMobile(e.matches)
    mq.addEventListener("change", onChange)
    return () => mq.removeEventListener("change", onChange)
  }, [])
  return isMobile
}
