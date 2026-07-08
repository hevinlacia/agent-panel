/**
 * Tests for `src/notifications.ts`.
 *
 * Covers:
 *   - create / get / unread count basics
 *   - update transitions (running → done sets unread=true)
 *   - dismiss single / dismiss all
 *   - markAllRead leaves running notifications unread
 *   - persistence: writes to disk, reloads on init
 *   - TTL: notifications older than 7 days are evicted on load
 */

import { test } from "node:test"
import { strict as assert } from "node:assert"
import { mkdtempSync, rmSync, existsSync, writeFileSync, readFileSync } from "node:fs"
import { tmpdir } from "node:os"
import { join } from "node:path"

import {
  createNotification,
  updateNotification,
  dismissNotification,
  dismissAll,
  markAllRead,
  getNotifications,
  getUnreadCount,
  getNotification,
  initNotifications,
  _resetForTest,
  _loadForTest,
} from "../src/notifications.ts"

function newTmpStorePath(): string {
  const dir = mkdtempSync(join(tmpdir(), "agent-panel-notif-"))
  return join(dir, "notifications.json")
}

test("createNotification returns an id and adds an unread entry", () => {
  const p = newTmpStorePath()
  _resetForTest(p)
  const id = createNotification({
    type: "extract",
    title: "test",
    state: "running",
  })
  assert.ok(id.length > 0)
  const all = getNotifications()
  assert.equal(all.length, 1)
  assert.equal(all[0].id, id)
  assert.equal(all[0].unread, true)
  assert.equal(getUnreadCount(), 1)
})

test("updateNotification transitions state and re-flags unread", () => {
  const p = newTmpStorePath()
  _resetForTest(p)
  const id = createNotification({
    type: "extract",
    title: "t",
    state: "running",
  })
  // Caller may decide to peek the notification (mark as read) without
  // dismissing — to simulate that, manually mark it read.
  markAllRead()
  // markAllRead doesn't affect a running notification.
  assert.equal(getUnreadCount(), 1)

  updateNotification(id, { state: "done", title: "✓ done" })
  const n = getNotification(id)
  assert.ok(n)
  assert.equal(n!.state, "done")
  assert.equal(n!.title, "✓ done")
  assert.equal(n!.unread, true)
})

test("dismissNotification hides it from the default list", () => {
  const p = newTmpStorePath()
  _resetForTest(p)
  const id = createNotification({ type: "system", title: "x" })
  dismissNotification(id)
  assert.equal(getNotifications().length, 0)
  assert.equal(getNotifications(true).length, 1)
  assert.equal(getUnreadCount(), 0)
})

test("dismissAll hides everything", () => {
  const p = newTmpStorePath()
  _resetForTest(p)
  createNotification({ type: "extract", title: "a" })
  createNotification({ type: "extract", title: "b" })
  dismissAll()
  assert.equal(getNotifications().length, 0)
  assert.equal(getUnreadCount(), 0)
})

test("markAllRead does not affect running notifications", () => {
  const p = newTmpStorePath()
  _resetForTest(p)
  const running = createNotification({ type: "extract", title: "in-flight", state: "running" })
  const done = createNotification({ type: "extract", title: "done", state: "done" })
  assert.equal(getUnreadCount(), 2)
  markAllRead()
  assert.equal(getUnreadCount(), 1) // only the running one remains unread
  assert.equal(getNotification(running)!.unread, true)
  assert.equal(getNotification(done)!.unread, false)
})

test("getNotifications is newest-first", async () => {
  const p = newTmpStorePath()
  _resetForTest(p)
  createNotification({ type: "extract", title: "first" })
  await new Promise((r) => setTimeout(r, 5))
  createNotification({ type: "extract", title: "second" })
  await new Promise((r) => setTimeout(r, 5))
  createNotification({ type: "extract", title: "third" })
  const all = getNotifications()
  assert.equal(all[0].title, "third")
  assert.equal(all[1].title, "second")
  assert.equal(all[2].title, "first")
})

test("persistence: createNotification writes to disk; initNotifications reloads", async () => {
  const p = newTmpStorePath()
  _resetForTest(p)
  createNotification({ type: "extract", title: "persisted", state: "done" })
  // Wait for the async saveToDisk to flush.
  await new Promise((r) => setTimeout(r, 20))
  assert.ok(existsSync(p))
  const raw = readFileSync(p, "utf-8")
  assert.match(raw, /persisted/)

  // Reset memory but keep the file, then re-init from disk.
  _resetForTest(p)
  assert.equal(getNotifications().length, 0)
  await initNotifications()
  const reloaded = getNotifications()
  assert.equal(reloaded.length, 1)
  assert.equal(reloaded[0].title, "persisted")
})

test("TTL: notifications older than 7 days are evicted on load (but running and dismissed survive)", async () => {
  const p = newTmpStorePath()
  _resetForTest(p)
  const eightDaysAgo = Date.now() - 8 * 24 * 60 * 60 * 1000
  const stale = {
    id: "old1",
    type: "extract",
    title: "stale done",
    subtitle: "",
    state: "done",
    jobId: null,
    reqId: null,
    sessionId: null,
    actionHref: null,
    unread: false,
    createdAt: eightDaysAgo,
    dismissedAt: null,
  }
  const staleRunning = {
    id: "old2",
    type: "extract",
    title: "very old running",
    subtitle: "",
    state: "running",
    jobId: null,
    reqId: null,
    sessionId: null,
    actionHref: null,
    unread: true,
    createdAt: eightDaysAgo,
    dismissedAt: null,
  }
  const staleDismissed = {
    id: "old3",
    type: "extract",
    title: "stale but dismissed",
    subtitle: "",
    state: "done",
    jobId: null,
    reqId: null,
    sessionId: null,
    actionHref: null,
    unread: false,
    createdAt: eightDaysAgo,
    dismissedAt: eightDaysAgo + 1,
  }
  const fresh = {
    id: "fresh1",
    type: "extract",
    title: "fresh",
    subtitle: "",
    state: "done",
    jobId: null,
    reqId: null,
    sessionId: null,
    actionHref: null,
    unread: true,
    createdAt: Date.now(),
    dismissedAt: null,
  }
  writeFileSync(p, JSON.stringify({
    version: 1,
    notifications: [stale, staleRunning, staleDismissed, fresh],
  }))

  _resetForTest(p)
  await initNotifications()
  const all = getNotifications(true)
  const ids = all.map((n) => n.id).sort()
  // - stale done (no dismissal): dropped
  // - very old running: kept (running survives TTL)
  // - stale dismissed: kept (already dismissed, survives until manual cleanup)
  // - fresh: kept
  assert.deepEqual(ids, ["fresh1", "old2", "old3"])
})

test("_loadForTest seeds the in-memory store without touching disk", () => {
  const p = newTmpStorePath()
  _resetForTest(p)
  _loadForTest(JSON.stringify({
    version: 1,
    notifications: [
      {
        id: "abc",
        type: "extract",
        title: "from json",
        subtitle: "",
        state: "running",
        jobId: null,
        reqId: null,
        sessionId: null,
        actionHref: null,
        unread: true,
        createdAt: Date.now(),
        dismissedAt: null,
      },
    ],
  }))
  const n = getNotification("abc")
  assert.ok(n)
  assert.equal(n!.title, "from json")
})

test("integration: extract-context job creates a running notification", async () => {
  // This test imports the live extractJobs module — make sure both share
  // the same notifications store file before we kick off a job.
  const p = newTmpStorePath()
  _resetForTest(p)

  const { createExtractJob, _resetExtractJobs } = await import("../src/extractJobs.ts")
  _resetExtractJobs()
  const job = createExtractJob({
    reqId: "req1",
    sessionId: "ses_abcdef",
    prompt: "test prompt",
    // Avoid actually spawning opencode.
    runFn: async () => ({ stdout: "", stderr: "", exitCode: 0, durationMs: 1, timedOut: false }),
    salvageFn: async () => null,
  })
  assert.ok(job.id)
  const all = getNotifications()
  // Exactly one notification was created for this job.
  const forJob = all.filter((n) => n.jobId === job.id)
  assert.equal(forJob.length, 1)
  assert.equal(forJob[0].sessionId, "ses_abcdef")
  assert.equal(forJob[0].reqId, "req1")
  assert.equal(forJob[0].type, "extract")
  // Cleanup so cross-suite state doesn't bleed.
  _resetExtractJobs()
})

// Cleanup tmp dirs at process exit (best-effort).
process.on("exit", () => {
  const t = tmpdir()
  try {
    // No-op: each test makes its own dir; OS will clean /tmp eventually.
  } catch {
    // ignore
  }
  void t
  void rmSync
})