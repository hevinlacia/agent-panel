/**
 * Pure tests for the nav/URL contracts in `src/navigation.ts`.
 *
 * The dashboard intentionally has no route-level tests (no Hono server is
 * booted in tests), so these contracts guard against accidental regressions
 * in the nav order, the homepage path, and the sessions time-filter URLs.
 */

import { test } from "node:test"
import { strict as assert } from "node:assert"

import {
  DASHBOARD_PATH,
  HOME_PATH,
  NAV_ITEMS,
  PROJECTS_ALIAS_PATH,
  PROJECTS_PATH,
  REPORTS_PATH,
  SCHEDULERS_PATH,
  SETTINGS_PATH,
  SESSIONS_PATH,
  ENV_VARS_PATH,
  sessionsDaysPath,
} from "../src/navigation.ts"

test("NAV_ITEMS labels are dashboard, projects, sessions, reports, schedulers, env-vars, settings in that order", () => {
  assert.deepEqual(
    NAV_ITEMS.map((i) => i.label),
    ["/dashboard", "/projects", "/sessions", "/reports", "/schedulers", "/env-vars", "/settings"],
  )
})

test("NAV_ITEMS hrefs start with dashboard and projects", () => {
  assert.deepEqual(
    NAV_ITEMS.map((i) => i.href),
    ["/dashboard", "/projects", "/sessions", "/reports", "/schedulers", "/env-vars", "/settings"],
  )
})

test("NAV_ITEMS keys are dashboard, requirements, sessions, reports, schedulers, envvars, settings in that order", () => {
  assert.deepEqual(
    NAV_ITEMS.map((i) => i.key),
    ["dashboard", "requirements", "sessions", "reports", "schedulers", "envvars", "settings"],
  )
})

test("HOME_PATH is the dashboard home", () => {
  assert.equal(HOME_PATH, "/")
  assert.equal(DASHBOARD_PATH, "/dashboard")
})

test("projects uses /projects and is distinct from HOME_PATH", () => {
  assert.equal(PROJECTS_PATH, "/projects")
  assert.equal(PROJECTS_ALIAS_PATH, PROJECTS_PATH)
  assert.notEqual(PROJECTS_PATH, HOME_PATH)
})

test("route path constants", () => {
  assert.equal(SESSIONS_PATH, "/sessions")
  assert.equal(REPORTS_PATH, "/reports")
  assert.equal(SCHEDULERS_PATH, "/schedulers")
  assert.equal(SETTINGS_PATH, "/settings")
  assert.equal(ENV_VARS_PATH, "/env-vars")
})

test("sessionsDaysPath builds /sessions?days=<n>", () => {
  assert.equal(sessionsDaysPath(7), "/sessions?days=7")
  assert.equal(sessionsDaysPath(0), "/sessions?days=0")
  assert.equal(sessionsDaysPath(30), "/sessions?days=30")
  assert.equal(sessionsDaysPath(1), "/sessions?days=1")
})
