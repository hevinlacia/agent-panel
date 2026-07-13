/**
 * Tests for `src/fullSyncScheduler.ts`.
 */

import { test } from "node:test"
import { strict as assert } from "node:assert"
import { EventEmitter } from "node:events"
import { PassThrough, type Readable } from "node:stream"
import { mkdirSync, mkdtempSync, writeFileSync } from "node:fs"
import { tmpdir } from "node:os"
import { join } from "node:path"

import {
  FULL_SYNC_HOUR,
  FULL_SYNC_MINUTE,
  msUntilNextLocalTime,
  normalizeFullSyncTimes,
  triggerFullSync,
} from "../src/fullSyncScheduler.ts"

function fakeScriptPath(): string {
  const path = join(mkdtempSync(join(tmpdir(), "full-sync-script-")), "opencode-cron-sync.sh")
  writeFileSync(path, "#!/usr/bin/env bash\n", "utf-8")
  return path
}

function fakeSpawn(opts: { code: number | null; stdout?: string; stderr?: string; captured?: { argv?: string[]; env?: NodeJS.ProcessEnv } }) {
  return ((_bin: string, argv: string[], spawnOpts?: { env?: NodeJS.ProcessEnv }) => {
    opts.captured!.argv = argv
    opts.captured!.env = spawnOpts?.env
    const child = new EventEmitter() as EventEmitter & {
      stdout: Readable
      stderr: Readable
      kill: () => boolean
    }
    child.stdout = new PassThrough()
    child.stderr = new PassThrough()
    child.kill = () => true
    setTimeout(() => {
      if (opts.stdout) child.stdout.push(opts.stdout)
      if (opts.stderr) child.stderr.push(opts.stderr)
      child.stdout.push(null)
      child.stderr.push(null)
      child.emit("close", opts.code)
    }, 0)
    return child
  }) as any
}

test("msUntilNextLocalTime: computes same-day 20:30 delay", () => {
  const now = new Date(2026, 6, 1, 20, 0, 0, 0)
  assert.equal(msUntilNextLocalTime(FULL_SYNC_HOUR, FULL_SYNC_MINUTE, now), 30 * 60 * 1000)
})

test("msUntilNextLocalTime: rolls to next day after 20:30", () => {
  const now = new Date(2026, 6, 1, 20, 31, 0, 0)
  assert.equal(msUntilNextLocalTime(20, 30, now), (23 * 60 + 59) * 60 * 1000)
})

test("normalizeFullSyncTimes: keeps valid HH:mm values and falls back to defaults", () => {
  assert.deepEqual(normalizeFullSyncTimes(["7:05", "18:00", "bad", "24:00"]), ["07:05", "18:00"])
  assert.deepEqual(normalizeFullSyncTimes([]), ["12:00", "18:00", "20:30", "23:30"])
})

test("triggerFullSync: runs sync-all-to-github.sh with no special flags", async () => {
  const captured: { argv?: string[]; env?: NodeJS.ProcessEnv } = {}
  const result = await triggerFullSync({
    syncScript: fakeScriptPath(),
    spawnFn: fakeSpawn({ code: 0, stdout: "ok", captured }),
    nowFn: () => 1000,
  })
  assert.equal(result.ok, true)
  assert.deepEqual(captured.argv, [])
  assert.equal(captured.env?.OPENCODE_SYNC_SOURCE, "dashboard-full-sync")
})

test("triggerFullSync: pulls selected GitHub repos after sync", async () => {
  // Create a fake git repo dir so existsSync(.git) passes
  const fakeRepo = mkdtempSync(join(tmpdir(), "fake-repo-"))
  const fakeGitDir = join(fakeRepo, ".git")
  mkdirSync(fakeGitDir, { recursive: true })
  writeFileSync(join(fakeGitDir, "HEAD"), "ref: refs/heads/main")
  // Note: execFileSync will fail because it's not a real repo, but the test
  // verifies that sync-all runs with no --github-projects flag and the result is ok.
  const captured: { argv?: string[]; env?: NodeJS.ProcessEnv } = {}
  const result = await triggerFullSync({
    syncScript: fakeScriptPath(),
    githubRepos: [fakeRepo],
    spawnFn: fakeSpawn({ code: 0, stdout: "ok", captured }),
    nowFn: () => 1000,
  })
  assert.equal(result.ok, true)
  assert.deepEqual(captured.argv, [])
  // No more GITHUB_PROJECTS_CONFIG or GITHUB_PROJECTS_SYNC_SCRIPT env vars
  assert.ok(!captured.env?.GITHUB_PROJECTS_CONFIG)
  assert.ok(!captured.env?.GITHUB_PROJECTS_SYNC_SCRIPT)
  // The pull attempt should appear in stdout
  assert.match(result.stdout, /GitHub repos pull/)
})

test("triggerFullSync: returns failure when script is missing", async () => {
  const result = await triggerFullSync({ syncScript: "/tmp/opencode/missing-full-sync-script" })
  assert.equal(result.ok, false)
  assert.match(result.stderr, /not found/)
})
