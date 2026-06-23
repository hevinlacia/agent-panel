/**
 * Tests for `src/extractHistory.ts`.
 *
 * Covers:
 *   - buildExtractHistoryRecord: builds record from completed job
 *   - buildExtractHistoryRecord: returns null for running job
 *   - appendExtractHistory + getExtractHistoryForRequirement: round-trip
 *   - getExtractHistoryForRequirement: filters by reqId, sorts by doneAt desc
 *   - appendExtractHistory: replaces existing record by job id
 */

import { test } from "node:test"
import { strict as assert } from "node:assert"
import { join } from "node:path"
import { mkdirSync } from "node:fs"
import { randomBytes } from "node:crypto"

import {
  buildExtractHistoryRecord,
  appendExtractHistory,
  getExtractHistoryForRequirement,
  _resetExtractHistoryForTest,
  type ExtractHistoryRecord,
} from "../src/extractHistory.ts"

function freshPath(): string {
  const dir = join("/tmp", "opencode", "test-extract-hist-" + randomBytes(6).toString("hex"))
  mkdirSync(dir, { recursive: true })
  const p = join(dir, "extract-history.json")
  _resetExtractHistoryForTest(p)
  return p
}

function makeJobLike(overrides: Record<string, unknown> = {}): ExtractHistoryRecord {
  return {
    id: "job_" + randomBytes(4).toString("hex"),
    reqId: "REQ-TEST-001",
    sessionId: "ses_test0000000000000000000",
    mode: "summary",
    state: "done",
    model: "litellm-local/deepseek-v4-flash-auto",
    startedAt: Date.now() - 5000,
    doneAt: Date.now(),
    exitCode: 0,
    timedOut: false,
    errorMessage: null,
    salvagedFromFork: false,
    forkSessionId: null,
    forkTitle: null,
    summary: "测试摘要",
    stdoutSnippet: "## 目标\n做了点事",
    stderrSnippet: "",
    autoFileCount: 0,
    ...overrides,
  } as ExtractHistoryRecord
}

test("buildExtractHistoryRecord: returns null for running state", () => {
  const job = {
    id: "j1",
    reqId: "r1",
    sessionId: "s1",
    mode: "summary" as const,
    state: "running" as const,
    model: "m",
    startedAt: 100,
    doneAt: null,
    stdout: "",
    stderr: "",
    exitCode: null,
    timedOut: false,
    errorMessage: null,
    salvagedFromFork: false,
    forkSessionId: null,
    forkTitle: null,
    autoResult: null,
  }
  const record = buildExtractHistoryRecord(job)
  assert.equal(record, null)
})

test("buildExtractHistoryRecord: builds record for done job with summary", () => {
  const job = {
    id: "j2",
    reqId: "r2",
    sessionId: "s2",
    mode: "summary" as const,
    state: "done" as const,
    model: "m",
    startedAt: 100,
    doneAt: 200,
    stdout: "## 目标\n做了点事\n\n## 关键决策\n- a",
    stderr: "",
    exitCode: 0,
    timedOut: false,
    errorMessage: null,
    salvagedFromFork: false,
    forkSessionId: null,
    forkTitle: null,
    autoResult: null,
  }
  const record = buildExtractHistoryRecord(job)
  assert.ok(record)
  assert.equal(record!.state, "done")
  assert.ok(record!.summary.length > 0)
  assert.ok(record!.stdoutSnippet.includes("## 目标"))
})

test("appendExtractHistory + getExtractHistoryForRequirement: round-trip", async () => {
  freshPath()
  const record = makeJobLike({ reqId: "REQ-RT-001" })
  await appendExtractHistory(record)
  const history = await getExtractHistoryForRequirement("REQ-RT-001")
  assert.equal(history.length, 1)
  assert.equal(history[0].id, record.id)
})

test("getExtractHistoryForRequirement: filters by reqId", async () => {
  freshPath()
  await appendExtractHistory(makeJobLike({ id: "job_a", reqId: "REQ-A" }))
  await appendExtractHistory(makeJobLike({ id: "job_b", reqId: "REQ-B" }))
  await appendExtractHistory(makeJobLike({ id: "job_c", reqId: "REQ-A" }))

  const aHistory = await getExtractHistoryForRequirement("REQ-A")
  assert.equal(aHistory.length, 2)
  assert.ok(aHistory.every((h) => h.reqId === "REQ-A"))

  const bHistory = await getExtractHistoryForRequirement("REQ-B")
  assert.equal(bHistory.length, 1)
  assert.equal(bHistory[0].id, "job_b")
})

test("getExtractHistoryForRequirement: sorts by doneAt desc", async () => {
  freshPath()
  await appendExtractHistory(makeJobLike({ id: "job_old", reqId: "REQ-S", doneAt: 1000 }))
  await appendExtractHistory(makeJobLike({ id: "job_new", reqId: "REQ-S", doneAt: 2000 }))

  const history = await getExtractHistoryForRequirement("REQ-S")
  assert.equal(history.length, 2)
  assert.equal(history[0].id, "job_new")
  assert.equal(history[1].id, "job_old")
})

test("appendExtractHistory: replaces existing record by job id", async () => {
  freshPath()
  await appendExtractHistory(makeJobLike({ id: "job_dup", state: "done", summary: "first" }))
  await appendExtractHistory(makeJobLike({ id: "job_dup", state: "failed", summary: "second" }))

  const history = await getExtractHistoryForRequirement("REQ-TEST-001")
  assert.equal(history.length, 1)
  assert.equal(history[0].state, "failed")
  assert.equal(history[0].summary, "second")
})

test("appendExtractHistory: respects limit in getExtractHistoryForRequirement", async () => {
  freshPath()
  for (let i = 0; i < 5; i++) {
    await appendExtractHistory(makeJobLike({ id: `job_${i}`, reqId: "REQ-L", doneAt: 1000 + i }))
  }
  const history = await getExtractHistoryForRequirement("REQ-L", 3)
  assert.equal(history.length, 3)
  // Should be newest first.
  assert.equal(history[0].id, "job_4")
})
