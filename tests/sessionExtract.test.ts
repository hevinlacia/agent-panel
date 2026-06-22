/**
 * Tests for `src/sessionExtract.ts`.
 *
 * Covers:
 *   - buildExtractPrompt: includes title, status, and required headings;
 *     stays out of the way when title is empty.
 *   - runExtractSummary: spawns the requested command with the right
 *     argv shape, returns stdout/stderr/exitCode; respects timeout.
 *   - appendSummaryToNotes: creates a new file with the top heading,
 *     and appends a timestamped section to an existing file.
 */

import { test } from "node:test"
import { strict as assert } from "node:assert"
import { join } from "node:path"
import { mkdirSync, readFileSync, writeFileSync, existsSync } from "node:fs"
import { randomBytes } from "node:crypto"
import { Readable, PassThrough } from "node:stream"
import { EventEmitter } from "node:events"

import {
  buildExtractPrompt,
  runExtractSummary,
  appendSummaryToNotes,
} from "../src/sessionExtract.ts"

function freshDir(): string {
  const root = join("/tmp", "opencode", "test-extract-" + randomBytes(6).toString("hex"))
  mkdirSync(root, { recursive: true })
  return root
}

// ---------------------------------------------------------------------------
// buildExtractPrompt
// ---------------------------------------------------------------------------

test("buildExtractPrompt: includes title and status in the header", () => {
  const p = buildExtractPrompt({ id: "req-001", title: "迁移队列 wms-handover-push", status: "测试中" })
  assert.ok(p.includes("迁移队列 wms-handover-push"))
  assert.ok(p.includes("测试中"))
})

test("buildExtractPrompt: emits all five required section headings", () => {
  const p = buildExtractPrompt({ id: "req-001", title: "X", status: "开发中" })
  for (const heading of ["## 目标", "## 关键决策", "## 影响的文件/模块", "## 已完成的验证", "## 待办 / 风险"]) {
    assert.ok(p.includes(heading), `missing heading: ${heading}`)
  }
})

test("buildExtractPrompt: falls back to id when title is empty", () => {
  // Cast `status` to bypass the ReqStatus literal union — the helper accepts
  // a Pick<Requirement,…> so we can pass realistic edge-case inputs.
  const p = buildExtractPrompt({
    id: "req-fallback",
    title: "",
    status: "" as unknown as "开发中",
  })
  assert.ok(p.includes("req-fallback"))
  assert.ok(p.includes("未知"))
})

// ---------------------------------------------------------------------------
// runExtractSummary
// ---------------------------------------------------------------------------

/** Build a minimal fake child process compatible with spawn's return type. */
function makeFakeChild(opts: {
  stdoutChunks?: string[]
  stderrChunks?: string[]
  exitCode?: number | null
  delayMs?: number
}): { child: EventEmitter & { stdout: Readable; stderr: Readable; kill: (sig?: NodeJS.Signals | number) => boolean }; argv: string[] } {
  const child = new EventEmitter() as any
  const stdout = new PassThrough()
  const stderr = new PassThrough()
  child.stdout = stdout
  child.stderr = stderr
  child.kill = (_sig?: NodeJS.Signals | number) => {
    // Simulate kill: emit close immediately.
    queueMicrotask(() => child.emit("close", null))
    return true
  }

  const delay = opts.delayMs ?? 0
  setTimeout(() => {
    for (const c of opts.stdoutChunks ?? []) stdout.write(c)
    for (const c of opts.stderrChunks ?? []) stderr.write(c)
    stdout.end()
    stderr.end()
    child.emit("close", opts.exitCode ?? 0)
  }, delay)

  return { child, argv: [] }
}

test("runExtractSummary: spawns with --fork and -m EXTRACT_MODEL to avoid polluting the source session", async () => {
  let capturedArgs: { bin: string; argv: string[] } | null = null
  const fakeSpawn: any = (bin: string, argv: string[]) => {
    capturedArgs = { bin, argv }
    return makeFakeChild({ stdoutChunks: ["## 目标\n做了点事"], exitCode: 0 }).child
  }
  const r = await runExtractSummary({
    sessionId: "ses_abc123",
    prompt: "please summarize",
    opencodeBin: "fake-opencode",
    spawnFn: fakeSpawn,
  })
  const captured = capturedArgs as unknown as { bin: string; argv: string[] } | null
  assert.ok(captured)
  assert.equal(captured!.bin, "fake-opencode")
  // Exactly: ["run", "--session", "<sid>", "--fork", "-m", "<model>", "<prompt>"]
  // --fork is non-negotiable: without it, opencode would mutate the
  // original session by appending the prompt + reply as new messages.
  // -m pins the summarization model so we don't depend on opencode's
  // auto-pick and don't hit timeouts with a heavier daily-driver model.
  assert.deepEqual(captured!.argv, [
    "run",
    "--session",
    "ses_abc123",
    "--fork",
    "-m",
    "litellm-local/deepseek-v4-flash-auto",
    "please summarize",
  ])
  assert.equal(r.exitCode, 0)
  assert.equal(r.timedOut, false)
  assert.ok(r.stdout.includes("## 目标"))
})

test("runExtractSummary: surfaces non-zero exit and stderr without throwing", async () => {
  const fakeSpawn: any = () =>
    makeFakeChild({
      stdoutChunks: [],
      stderrChunks: ["opencode: session not found\n"],
      exitCode: 2,
    }).child
  const r = await runExtractSummary({
    sessionId: "ses_missing",
    prompt: "x",
    spawnFn: fakeSpawn,
  })
  assert.equal(r.exitCode, 2)
  assert.ok(r.stderr.includes("session not found"))
  assert.equal(r.stdout, "")
})

test("runExtractSummary: times out and reports timedOut=true", async () => {
  const fakeSpawn: any = () =>
    // Take 5s; we'll set timeoutMs=20.
    makeFakeChild({ stdoutChunks: ["late"], exitCode: 0, delayMs: 5_000 }).child
  const r = await runExtractSummary({
    sessionId: "ses_slow",
    prompt: "x",
    spawnFn: fakeSpawn,
    timeoutMs: 20,
  })
  assert.equal(r.timedOut, true)
  // exitCode is null because the fake child reports null on kill.
  assert.equal(r.exitCode, null)
})

// ---------------------------------------------------------------------------
// appendSummaryToNotes
// ---------------------------------------------------------------------------

test("appendSummaryToNotes: creates notes.md with top heading when missing", async () => {
  const dir = freshDir()
  const notesPath = join(dir, "notes.md")
  await appendSummaryToNotes(notesPath, "ses_111", "body line 1\nbody line 2", new Date(2026, 5, 22, 14, 30))
  const content = readFileSync(notesPath, "utf-8")
  assert.ok(content.startsWith("# Session 摘要 / 上下文沉淀\n"))
  assert.ok(content.includes("## Session ses_111 摘要 (2026-06-22 14:30)"))
  assert.ok(content.includes("body line 1"))
  assert.ok(content.includes("body line 2"))
})

test("appendSummaryToNotes: appends to an existing file without rewriting it", async () => {
  const dir = freshDir()
  const notesPath = join(dir, "notes.md")
  writeFileSync(notesPath, "# 0622 Notes\n\n## 来源\n- a\n", "utf-8")

  await appendSummaryToNotes(notesPath, "ses_222", "summary here", new Date(2026, 5, 22, 9, 15))
  const content = readFileSync(notesPath, "utf-8")

  // Original content is preserved verbatim at the top.
  assert.ok(content.startsWith("# 0622 Notes\n\n## 来源\n- a\n"))
  // New section is at the end with the timestamped heading.
  assert.ok(content.includes("## Session ses_222 摘要 (2026-06-22 09:15)"))
  assert.ok(content.trim().endsWith("summary here"))
})

test("appendSummaryToNotes: a second append for the same session id stacks (no dedupe)", async () => {
  const dir = freshDir()
  const notesPath = join(dir, "notes.md")
  await appendSummaryToNotes(notesPath, "ses_333", "first", new Date(2026, 5, 22, 10, 0))
  await appendSummaryToNotes(notesPath, "ses_333", "second", new Date(2026, 5, 22, 11, 0))
  const content = readFileSync(notesPath, "utf-8")
  assert.ok(content.includes("## Session ses_333 摘要 (2026-06-22 10:00)"))
  assert.ok(content.includes("## Session ses_333 摘要 (2026-06-22 11:00)"))
  assert.ok(content.includes("first"))
  assert.ok(content.includes("second"))
})

test("appendSummaryToNotes: creates the parent dir if missing", async () => {
  const dir = freshDir()
  const notesPath = join(dir, "nested", "deeper", "notes.md")
  assert.equal(existsSync(notesPath), false)
  await appendSummaryToNotes(notesPath, "ses_444", "x", new Date(2026, 5, 22, 0, 0))
  assert.equal(existsSync(notesPath), true)
})
