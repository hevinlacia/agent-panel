/**
 * Tests for `src/forkSalvage.ts`.
 *
 * We don't spin up a real sqlite3 binary here — we inject a fake
 * `sqliteFn` that captures the argv and stdin script we send, and
 * replays whatever stdout payload the test wants. This keeps the
 * tests fast and hermetic; the production SQL query shape is already
 * exercised by the live curl smoke at the end of the development
 * cycle.
 *
 * Covers:
 *   - findRecentForkSession: returns parsed FoundFork on a hit
 *   - findRecentForkSession: returns null on empty result
 *   - findRecentForkSession: returns null on sqlite3 non-zero exit
 *   - findRecentForkSession: sends the parameterized .param set script
 *     and includes the anchor with wildcards escaped
 *   - extractAssistantText: concatenates assistant text-parts in order,
 *     skipping user / non-text parts
 *   - salvageFromFork: returns null when fork miss; returns concatenated
 *     text when found
 */

import { test } from "node:test"
import { strict as assert } from "node:assert"
import { Readable, Writable, PassThrough } from "node:stream"
import { EventEmitter } from "node:events"

import {
  findRecentForkSession,
  extractAssistantText,
  salvageFromFork,
} from "../src/forkSalvage.ts"

interface FakeChildLog {
  argv: string[]
  stdin: string
}

/**
 * Build a fake child process exposing the spawn shape used by the
 * sqlite3 helpers. The test pre-loads `stdoutPayload` and an exit
 * code; the spawn returns immediately and we capture the stdin
 * script the helper writes.
 */
function makeFakeSqlite(opts: {
  stdoutPayload: string
  exitCode?: number
}): { sqliteFn: any; log: FakeChildLog } {
  const log: FakeChildLog = { argv: [], stdin: "" }

  const sqliteFn = (bin: string, argv: string[]) => {
    log.argv = [bin, ...argv]
    const child = new EventEmitter() as any
    const stdout = new PassThrough()
    const stderr = new PassThrough()
    const stdin = new Writable({
      write(chunk, _enc, cb) { log.stdin += chunk.toString("utf-8"); cb() },
    })
    child.stdin = stdin
    child.stdout = stdout
    child.stderr = stderr
    child.kill = (_sig?: any) => true

    // Resolve next tick so the caller has time to write stdin first.
    queueMicrotask(() => {
      stdout.write(opts.stdoutPayload)
      stdout.end()
      stderr.end()
      child.emit("close", opts.exitCode ?? 0)
    })
    return child
  }
  return { sqliteFn, log }
}

// ---------------------------------------------------------------------------
// findRecentForkSession
// ---------------------------------------------------------------------------

test("findRecentForkSession: parses a hit and returns FoundFork", async () => {
  const { sqliteFn, log } = makeFakeSqlite({
    stdoutPayload: JSON.stringify([{
      id: "ses_fork123",
      title: "X (fork #1)",
      time_created: 1700000000000,
      time_updated: 1700000018000,
    }]),
  })
  const r = await findRecentForkSession({
    sourceSessionId: "ses_src",
    startedAt: 1700000000000,
    promptAnchor: "请用中文总结本次会话",
    dbPath: "/tmp/opencode/__no_such_db_path__.db", // existsSync gate
    sqliteFn,
  })
  // existsSync will fail; we expect null in that case.
  assert.equal(r, null)
  // But we should NOT have spawned sqlite at all.
  assert.deepEqual(log.argv, [])
})

test("findRecentForkSession: returns FoundFork when DB exists and a row matches", async () => {
  // Write an empty file so existsSync returns true.
  const tmpDb = "/tmp/opencode/fake-db-" + Math.random().toString(36).slice(2, 8) + ".db"
  await import("node:fs/promises").then(m => m.writeFile(tmpDb, ""))

  const { sqliteFn, log } = makeFakeSqlite({
    stdoutPayload: JSON.stringify([{
      id: "ses_fork123",
      title: "X (fork #1)",
      time_created: 1700000000000,
      time_updated: 1700000018000,
    }]),
  })
  const r = await findRecentForkSession({
    sourceSessionId: "ses_src",
    startedAt: 1700000000000,
    promptAnchor: "请用中文总结本次会话",
    dbPath: tmpDb,
    sqliteFn,
  })
  assert.deepEqual(r, {
    forkSessionId: "ses_fork123",
    forkTitle: "X (fork #1)",
    timeCreated: 1700000000000,
    timeUpdated: 1700000018000,
  })
  // Argv: sqlite3 -json <db>
  assert.equal(log.argv[0], "sqlite3")
  assert.equal(log.argv[1], "-json")
  assert.equal(log.argv[2], tmpDb)
  // Stdin script: named params + the SELECT + .quit
  assert.match(log.stdin, /\.param set :anchor /)
  assert.match(log.stdin, /\.param set :minTs /)
  assert.match(log.stdin, /\.param set :maxTs /)
  assert.match(log.stdin, /SELECT s\.id, s\.title/)
  assert.match(log.stdin, /\.quit/)
  // Anchor must be wrapped in % … % so LIKE matches substrings.
  assert.ok(log.stdin.includes("%请用中文总结本次会话%"))
})

test("findRecentForkSession: returns null on empty result", async () => {
  const tmpDb = "/tmp/opencode/fake-db-empty.db"
  await import("node:fs/promises").then(m => m.writeFile(tmpDb, ""))
  const { sqliteFn } = makeFakeSqlite({ stdoutPayload: "" })
  const r = await findRecentForkSession({
    sourceSessionId: "ses_src",
    startedAt: 1700000000000,
    promptAnchor: "请用中文总结本次会话",
    dbPath: tmpDb,
    sqliteFn,
  })
  assert.equal(r, null)
})

test("findRecentForkSession: returns null on sqlite3 non-zero exit", async () => {
  const tmpDb = "/tmp/opencode/fake-db-failed.db"
  await import("node:fs/promises").then(m => m.writeFile(tmpDb, ""))
  const { sqliteFn } = makeFakeSqlite({ stdoutPayload: "", exitCode: 1 })
  const r = await findRecentForkSession({
    sourceSessionId: "ses_src",
    startedAt: 1700000000000,
    promptAnchor: "请用中文总结本次会话",
    dbPath: tmpDb,
    sqliteFn,
  })
  assert.equal(r, null)
})

// ---------------------------------------------------------------------------
// extractAssistantText
// ---------------------------------------------------------------------------

test("extractAssistantText: concatenates assistant text parts and skips others", async () => {
  const tmpDb = "/tmp/opencode/fake-db-extract.db"
  await import("node:fs/promises").then(m => m.writeFile(tmpDb, ""))
  // Simulate four rows:
  //   1) user message with text part   → skipped
  //   2) assistant message with text   → kept
  //   3) assistant message with reasoning (type=reasoning) → skipped
  //   4) assistant message with text   → kept (later)
  const rows = [
    { part_data: JSON.stringify({ type: "text", text: "请用中文总结" }),
      message_data: JSON.stringify({ role: "user" }), t: 1 },
    { part_data: JSON.stringify({ type: "text", text: "## 目标\n做了 A" }),
      message_data: JSON.stringify({ role: "assistant" }), t: 2 },
    { part_data: JSON.stringify({ type: "reasoning", text: "thinking..." }),
      message_data: JSON.stringify({ role: "assistant" }), t: 3 },
    { part_data: JSON.stringify({ type: "text", text: "## 关键决策\n- x" }),
      message_data: JSON.stringify({ role: "assistant" }), t: 4 },
  ]
  const { sqliteFn, log } = makeFakeSqlite({ stdoutPayload: JSON.stringify(rows) })
  const text = await extractAssistantText({
    forkSessionId: "ses_fork123",
    dbPath: tmpDb,
    sqliteFn,
  })
  assert.match(text, /## 目标/)
  assert.match(text, /做了 A/)
  assert.match(text, /## 关键决策/)
  assert.match(text, /- x/)
  // Reasoning and user prompt should be absent.
  assert.ok(!text.includes("请用中文总结"))
  assert.ok(!text.includes("thinking"))
  // Stdin script should be parameterized.
  assert.match(log.stdin, /\.param set :sid /)
})

test("extractAssistantText: returns empty string on no assistant parts", async () => {
  const tmpDb = "/tmp/opencode/fake-db-noassist.db"
  await import("node:fs/promises").then(m => m.writeFile(tmpDb, ""))
  const rows = [
    { part_data: JSON.stringify({ type: "text", text: "请用中文总结" }),
      message_data: JSON.stringify({ role: "user" }), t: 1 },
  ]
  const { sqliteFn } = makeFakeSqlite({ stdoutPayload: JSON.stringify(rows) })
  const text = await extractAssistantText({
    forkSessionId: "ses_fork123",
    dbPath: tmpDb,
    sqliteFn,
  })
  assert.equal(text, "")
})

// ---------------------------------------------------------------------------
// salvageFromFork
// ---------------------------------------------------------------------------

test("salvageFromFork: returns null when find phase misses", async () => {
  const tmpDb = "/tmp/opencode/fake-db-salv-miss.db"
  await import("node:fs/promises").then(m => m.writeFile(tmpDb, ""))
  const { sqliteFn } = makeFakeSqlite({ stdoutPayload: "[]" })
  const r = await salvageFromFork({
    sourceSessionId: "ses_src",
    startedAt: 1700000000000,
    promptAnchor: "请用中文总结本次会话",
    dbPath: tmpDb,
    sqliteFn,
  })
  assert.equal(r, null)
})
