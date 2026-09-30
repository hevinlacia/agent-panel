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

export const PROJECT_SHOW_SUB_REQS_KEY = "agent-panel.projects.showSubReqs"
export const DIFF_INSPECTOR_COLLAPSED_KEY = "agent-panel.diffInspectorCollapsed"

export function readDiffInspectorCollapsed(): boolean {
  try { return localStorage.getItem(DIFF_INSPECTOR_COLLAPSED_KEY) === "1" } catch { return false }
}

export function persistDiffInspectorCollapsed(collapsed: boolean) {
  try { localStorage.setItem(DIFF_INSPECTOR_COLLAPSED_KEY, collapsed ? "1" : "0") } catch { /* ignore */ }
}

export function readShowSubReqs(): boolean {
  // 默认显示子需求（树形挂在父需求下方）；仅当用户显式关闭过（存 "0"）才隐藏。
  try { return localStorage.getItem(PROJECT_SHOW_SUB_REQS_KEY) !== "0" } catch { return true }
}

export function persistShowSubReqs(show: boolean) {
  try { localStorage.setItem(PROJECT_SHOW_SUB_REQS_KEY, show ? "1" : "0") } catch { /* ignore */ }
}

/** 默认折叠子需求：列表页整合需求树、详情页子需求面板共享的初始折叠状态，切换即记住。 */
export const SUB_REQS_COLLAPSED_KEY = "agent-panel.subReqsCollapsed"

export function readSubReqsCollapsed(): boolean {
  try { return localStorage.getItem(SUB_REQS_COLLAPSED_KEY) === "1" } catch { return false }
}

export function persistSubReqsCollapsed(collapsed: boolean) {
  try { localStorage.setItem(SUB_REQS_COLLAPSED_KEY, collapsed ? "1" : "0") } catch { /* ignore */ }
}
