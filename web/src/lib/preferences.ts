import type { ReqStatus } from "../types"
import { REQ_STATUSES } from "./requirements"

export const PROJECT_FILTER_KEY = "agent-panel.project"
export const PROJECT_DEFAULT_EXCLUDED_STATUSES_KEY = "agent-panel.projects.defaultExcludedStatuses"
export const SIDEBAR_COLLAPSED_KEY = "agent-panel.sidebarCollapsed"
export const FALLBACK_DEFAULT_EXCLUDED_STATUSES = ["已完成"]

export function readProjectFilter(): string {
  try { return localStorage.getItem(PROJECT_FILTER_KEY) || "" } catch { return "" }
}

export function readDefaultExcludedStatuses(): string[] {
  try {
    const raw = localStorage.getItem(PROJECT_DEFAULT_EXCLUDED_STATUSES_KEY)
    if (!raw) return FALLBACK_DEFAULT_EXCLUDED_STATUSES
    const parsed = JSON.parse(raw)
    if (!Array.isArray(parsed)) return FALLBACK_DEFAULT_EXCLUDED_STATUSES
    return parsed.filter((s) => typeof s === "string" && REQ_STATUSES.includes(s as ReqStatus))
  } catch {
    return FALLBACK_DEFAULT_EXCLUDED_STATUSES
  }
}

export function persistDefaultExcludedStatuses(statuses: string[]) {
  try { localStorage.setItem(PROJECT_DEFAULT_EXCLUDED_STATUSES_KEY, JSON.stringify(statuses)) } catch { /* ignore */ }
}

export function readSidebarCollapsed(): boolean {
  try { return localStorage.getItem(SIDEBAR_COLLAPSED_KEY) === "1" } catch { return false }
}

export function persistSidebarCollapsed(collapsed: boolean) {
  try { localStorage.setItem(SIDEBAR_COLLAPSED_KEY, collapsed ? "1" : "0") } catch { /* ignore */ }
}
