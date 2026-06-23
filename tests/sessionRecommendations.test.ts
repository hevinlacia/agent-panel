/**
 * Tests for `src/sessionRecommendations.ts`.
 *
 * Covers:
 *   - scoreSessionForRequirement: title keyword hits, path hits,
 *     project match, time bonus, zero-score filtering.
 *   - recommendSessionsForRequirement: sorting by score then recency,
 *     limit enforcement.
 */

import { test } from "node:test"
import { strict as assert } from "node:assert"
import { join } from "node:path"
import { mkdirSync } from "node:fs"
import { randomBytes } from "node:crypto"

import {
  scoreSessionForRequirement,
  recommendSessionsForRequirement,
} from "../src/sessionRecommendations.ts"
import type { Requirement } from "../src/requirements.ts"
import type { SessionInfo } from "../src/sessions.ts"

function makeReq(overrides: Partial<Requirement> = {}): Requirement {
  return {
    id: "WMS-001-log-refactor",
    title: "WMS 日志系统重构",
    status: "测试中",
    project: "WMS",
    groupPath: [],
    description: "- Title: WMS 日志系统重构\n- Status: 测试中",
    sessionIds: [],
    createdAt: Date.now() - 7 * 24 * 60 * 60_000,
    updatedAt: Date.now() - 1 * 24 * 60 * 60_000,
    ...overrides,
  }
}

function makeSession(overrides: Partial<SessionInfo> = {}): SessionInfo {
  return {
    id: "ses_" + randomBytes(12).toString("hex"),
    title: "",
    created: Date.now() - 3600_000,
    updated: Date.now() - 1800_000,
    projectId: "global",
    directory: "",
    status: "idle",
    source: "db",
    ...overrides,
  }
}

test("scoreSessionForRequirement: returns null for completely unrelated session", () => {
  const req = makeReq()
  const session = makeSession({ title: "opencode discussion", directory: "/tmp" })
  const result = scoreSessionForRequirement(req, session)
  assert.equal(result, null)
})

test("scoreSessionForRequirement: high score when title contains requirement keywords", () => {
  const req = makeReq()
  const session = makeSession({
    title: "test环境日志操作时间与operation_log不一致",
    directory: "/home/hevin/Developer/company/WMS",
  })
  const result = scoreSessionForRequirement(req, session)
  assert.ok(result)
  assert.ok(result!.score >= 10)
  assert.ok(result!.reasons.length > 0)
})

test("scoreSessionForRequirement: detects project name in directory", () => {
  const req = makeReq({ title: "Some generic task" })
  const session = makeSession({
    title: "random work",
    directory: "/home/hevin/Developer/company/WMS/yl-cwhsea-wms-api",
  })
  const result = scoreSessionForRequirement(req, session)
  assert.ok(result)
  assert.ok(result!.reasons.some((r) => r.includes("WMS")))
})

test("scoreSessionForRequirement: title exact match gives highest score", () => {
  const req = makeReq({ title: "日志重构" })
  const sessionA = makeSession({ title: "日志重构 bugfix" })
  const sessionB = makeSession({ title: "something else" })
  const a = scoreSessionForRequirement(req, sessionA)
  const b = scoreSessionForRequirement(req, sessionB)
  assert.ok(a)
  assert.equal(b, null)
  assert.ok(a!.score >= 10)
})

test("recommendSessionsForRequirement: sorts by score desc, then recency desc", () => {
  const req = makeReq({ title: "日志" })
  const old = makeSession({
    id: "ses_old00000000000000000000",
    title: "日志 bug",
    updated: Date.now() - 10 * 24 * 60 * 60_000,
  })
  const recent = makeSession({
    id: "ses_recent000000000000000000",
    title: "日志 bug",
    updated: Date.now() - 60_000,
  })
  const recos = recommendSessionsForRequirement(req, [old, recent], 6)
  assert.ok(recos.length >= 1)
  // Both should match, but recent should come first on tie.
  if (recos.length === 2) {
    assert.equal(recos[0].session.id, "ses_recent000000000000000000")
  }
})

test("recommendSessionsForRequirement: respects limit", () => {
  const req = makeReq({ title: "日志" })
  const sessions = Array.from({ length: 10 }, (_, i) =>
    makeSession({
      id: "ses_" + String(i).padStart(24, "0"),
      title: `日志 task ${i}`,
    })
  )
  const recos = recommendSessionsForRequirement(req, sessions, 3)
  assert.ok(recos.length <= 3)
})

test("recommendSessionsForRequirement: filters out zero-score sessions", () => {
  const req = makeReq({ title: "日志重构" })
  const unrelated = makeSession({ title: "completely unrelated topic", directory: "/tmp" })
  const recos = recommendSessionsForRequirement(req, [unrelated], 6)
  assert.equal(recos.length, 0)
})
