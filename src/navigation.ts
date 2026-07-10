/**
 * Pure URL/navigation contracts for the dashboard nav and the sessions
 * dashboard's time-filter links. Kept in a side-effect-free module so
 * `tests/navigation.test.ts` can assert the contract without booting Hono.
 *
 * The order of `NAV_ITEMS` is the visual top-to-bottom order in the sidebar.
 * The status dashboard is the site home; `/dashboard` remains a direct alias.
 */

export const HOME_PATH = "/"
export const PROJECTS_PATH = "/projects"
export const PROJECTS_ALIAS_PATH = "/projects"
export const DASHBOARD_PATH = "/dashboard"
export const SESSIONS_PATH = "/sessions"
export const REPORTS_PATH = "/reports"
export const SCHEDULERS_PATH = "/schedulers"
export const SETTINGS_PATH = "/settings"
export const ENV_VARS_PATH = "/env-vars"

export type NavKey = "requirements" | "dashboard" | "sessions" | "reports" | "schedulers" | "settings" | "envvars"

export interface NavItem {
  key: NavKey
  label: string
  href: string
}

export const NAV_ITEMS: readonly NavItem[] = [
  { key: "dashboard", label: "/dashboard", href: DASHBOARD_PATH },
  { key: "requirements", label: "/projects", href: PROJECTS_PATH },
  { key: "sessions", label: "/sessions", href: SESSIONS_PATH },
  { key: "reports", label: "/reports", href: REPORTS_PATH },
  { key: "schedulers", label: "/schedulers", href: SCHEDULERS_PATH },
  { key: "envvars", label: "/env-vars", href: ENV_VARS_PATH },
  { key: "settings", label: "/settings", href: SETTINGS_PATH },
] as const

export function sessionsDaysPath(days: number): string {
  return `${SESSIONS_PATH}?days=${encodeURIComponent(String(days))}`
}
