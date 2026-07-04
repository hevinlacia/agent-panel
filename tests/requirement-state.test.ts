/**
 * Regression tests for `src/requirementState.ts` and the
 * scanner's state.json integration.
 *
 * Covers:
 *   - extractHermesStatus / mapHermesStatusToReqStatus
 *   - readRequirementState: cold read with no state.json + no meta.md
 *   - readRequirementState: migrates `- Status: <eng>` from meta.md into state.json
 *   - writeRequirementStatus: appends a transition with `from` and timestamp
 *   - writeRequirementStatus: idempotent on same-status writes (no duplicate transition)
 *   - nextStatus: walks the chinese 7-stage vocabulary
 *   - scanHermesRequirements: a state.json on disk overrides meta.md's status
 */

import { test } from "node:test"
import { strict as assert } from "node:assert"
import { join } from "node:path"
import { mkdirSync, writeFileSync, existsSync, readFileSync } from "node:fs"
import { randomBytes } from "node:crypto"

import {
  _setReqDir,
  _setStorePath,
  scanHermesRequirements,
} from "../src/requirements.ts"
import {
  readRequirementState,
  writeRequirementStatus,
  nextStatus,
  extractHermesStatus,
  mapHermesStatusToReqStatus,
} from "../src/requirementState.ts"

function freshFixture(): string {
  const root = join("/tmp", "opencode", "test-req-state-" + randomBytes(6).toString("hex"))
  mkdirSync(root, { recursive: true })
  _setReqDir(root)
  _setStorePath(join(root, "associations.json"))
  return root
}

function writeMetaMd(dir: string, lines: string[]): void {
  mkdirSync(dir, { recursive: true })
  writeFileSync(join(dir, "meta.md"), lines.join("\n") + "\n", "utf-8")
}

test("extractHermesStatus: matches `- Status: <value>` markdown list line", () => {
  const text = [
    "# Foo",
    "## Summary",
    "- Title: Foo",
    "- Status: ready",
    "- Owner: unknown",
  ].join("\n")
  assert.equal(extractHermesStatus(text), "ready")
})

test("extractHermesStatus: returns null when no Status line exists", () => {
  assert.equal(extractHermesStatus("# Foo\nNo status here.\n"), null)
})

test("mapHermesStatusToReqStatus: maps known english labels", () => {
  assert.equal(mapHermesStatusToReqStatus("intake"), "需求对齐")
  assert.equal(mapHermesStatusToReqStatus("待设计"), "需求对齐")
  assert.equal(mapHermesStatusToReqStatus("ready"), "方案设计")
  assert.equal(mapHermesStatusToReqStatus("待开发"), "方案设计")
  assert.equal(mapHermesStatusToReqStatus("dev"), "开发中")
  assert.equal(mapHermesStatusToReqStatus("test"), "测试中")
  assert.equal(mapHermesStatusToReqStatus("done"), "已完成")
})

test("mapHermesStatusToReqStatus: unknown value falls back to 开发中", () => {
  assert.equal(mapHermesStatusToReqStatus("anything-weird"), "开发中")
  assert.equal(mapHermesStatusToReqStatus(""), "开发中")
})

test("nextStatus: walks forward through REQ_STATUSES", () => {
  assert.equal(nextStatus("需求对齐"), "方案设计")
  assert.equal(nextStatus("开发中"), "自测中")
  assert.equal(nextStatus("测试中"), "待上线")
  // Last stage has no next.
  assert.equal(nextStatus("已完成"), null)
})

test("readRequirementState: returns null when neither state.json nor meta.md exists", async () => {
  const root = freshFixture()
  const reqDir = join(root, "no-state-no-meta")
  mkdirSync(reqDir, { recursive: true })
  const state = await readRequirementState(reqDir)
  assert.equal(state, null)
})

test("readRequirementState: migrates `- Status: ready` from meta.md and writes state.json", async () => {
  const root = freshFixture()
  const reqDir = join(root, "0622-foo")
  writeMetaMd(reqDir, [
    "# 0622-foo Metadata",
    "## Summary",
    "- Title: Foo",
    "- Status: ready",
  ])
  const state = await readRequirementState(reqDir)
  assert.ok(state, "expected migrated state")
  assert.equal(state!.status, "方案设计")
  assert.equal(state!.history.length, 1)
  assert.equal(state!.history[0].from, null)
  assert.equal(typeof state!.history[0].at, "number")

  // Persisted to disk.
  const sp = join(reqDir, "state.json")
  assert.ok(existsSync(sp), "state.json should be created on migration")
  const onDisk = JSON.parse(readFileSync(sp, "utf-8"))
  assert.equal(onDisk.status, "方案设计")
})

test("writeRequirementStatus: appends a transition with from + at", async () => {
  const root = freshFixture()
  const reqDir = join(root, "0622-bar")
  writeMetaMd(reqDir, ["- Status: ready"])
  // Migrate first.
  await readRequirementState(reqDir)

  const next = await writeRequirementStatus(reqDir, "开发中", "started coding")
  assert.equal(next.status, "开发中")
  // 1 (migration) + 1 (this write) = 2 transitions.
  assert.equal(next.history.length, 2)
  const last = next.history[next.history.length - 1]
  assert.equal(last.status, "开发中")
  assert.equal(last.from, "方案设计")
  assert.equal(last.note, "started coding")
})

test("writeRequirementStatus: no duplicate transition when status is unchanged", async () => {
  const root = freshFixture()
  const reqDir = join(root, "0622-baz")
  mkdirSync(reqDir, { recursive: true })

  const first = await writeRequirementStatus(reqDir, "开发中")
  assert.equal(first.history.length, 1)
  const second = await writeRequirementStatus(reqDir, "开发中")
  // Same status — no new entry pushed.
  assert.equal(second.history.length, 1)
})

test("scanHermesRequirements: state.json overrides meta.md `- Status:`", async () => {
  const root = freshFixture()
  const reqDir = join(root, "WMS", "disaster-recovery", "mq-migration", "0622-mq-stub")
  writeMetaMd(reqDir, ["- Status: ready"])
  // Manually write state.json indicating a more advanced status.
  writeFileSync(
    join(reqDir, "state.json"),
    JSON.stringify({
      version: 1,
      status: "测试中",
      updatedAt: Date.now(),
      history: [
        { status: "测试中", from: "开发中", at: Date.now() },
      ],
    }, null, 2) + "\n",
    "utf-8",
  )
  const reqs = await scanHermesRequirements()
  const target = reqs.find((r) => r.id === "0622-mq-stub")
  assert.ok(target, "expected the seeded requirement in scan output")
  assert.equal(target!.status, "测试中")
  assert.deepEqual(target!.groupPath, ["disaster-recovery", "mq-migration"])
  assert.equal(target!.reqDir, reqDir)
})
