/**
 * Pure URL/navigation contracts for the dashboard nav and the sessions
 * dashboard's time-filter links. Kept in a side-effect-free module so
 * `tests/navigation.test.ts` can assert the contract without booting Hono.
 *
 * The order of `NAV_ITEMS` is the visual left-to-right order in the topbar.
 * `HOME_PATH === PROJECTS_PATH` because the projects view is the site home.
 */

export const HOME_PATH = "/"
export const PROJECTS_PATH = "/"
export const PROJECTS_ALIAS_PATH = "/projects"
export const SESSIONS_PATH = "/sessions"
export const REPORTS_PATH = "/reports"

export type NavKey = "requirements" | "sessions" | "reports"

export interface NavItem {
  key: NavKey
  label: string
  href: string
}

export const NAV_ITEMS: readonly NavItem[] = [
  { key: "requirements", label: "/projects", href: PROJECTS_PATH },
  { key: "sessions", label: "/sessions", href: SESSIONS_PATH },
  { key: "reports", label: "/reports", href: REPORTS_PATH },
] as const

export function sessionsDaysPath(days: number): string {
  return `${SESSIONS_PATH}?days=${encodeURIComponent(String(days))}`
}
