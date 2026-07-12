/**
 * Tests for requirement auto-drive job classification and persistence helpers.
 *
 * Covers:
 *   - queued job creation and latest lookup
 *   - BLOCKED output detection for human-review gates
 *   - successful DONE output classification
 */

import { test } from "node:test"
import { strict as assert } from "node:assert"
import { mkdtempSync } from "node:fs"
import { tmpdir } from "node:os"
import { join } from "node:path"

import type { Requirement } from "../src/requirements.ts"
import {
  _resetAutoDriveJobsForTest,
  buildAutoDrivePrompt,
  createAutoDriveJob,
  extractAutoDriveBlockers,
  finalizeAutoDriveJobFromResult,
  getAutoDriveJobs,
  getLatestAutoDriveJobForRequirement,
} from "../src/requirementAutoDrive.ts"

function storePath(): string {
  return join(mkdtempSync(join(tmpdir(), "agent-panel-drive-")), "auto-drive.json")
}

function req(id = "REQ-1"): Requirement {
  return {
    id,
    title: "测试自动推进",
    status: "需求对齐",
    projects: ["WMS"],
    project: "WMS",
    groupPath: [],
    description: "",
    sessionIds: [],
    createdAt: 1,
    updatedAt: 1,
    reqDir: "/tmp/req",
  }
}

test("createAutoDriveJob stores a queued job and latest lookup finds it", () => {
  _resetAutoDriveJobsForTest(storePath())
  const job = createAutoDriveJob(req(), "ses_test", "notif_1")
  assert.equal(job.state, "queued")
  assert.equal(job.reqId, "REQ-1")
  assert.equal(getAutoDriveJobs().length, 1)
  assert.equal(getLatestAutoDriveJobForRequirement("REQ-1")?.id, job.id)
})

test("extractAutoDriveBlockers reads BLOCKERS bullet list", () => {
  const blockers = extractAutoDriveBlockers(`AUTO_DRIVE_STATUS: BLOCKED\nBLOCKERS:\n- 验收标准待确认\n- 测试覆盖需要人工审核\nNEXT_ACTIONS:\n- 让用户确认`)
  assert.deepEqual(blockers, ["验收标准待确认", "测试覆盖需要人工审核"])
})

test("finalizeAutoDriveJobFromResult marks blocked output as blocked", () => {
  _resetAutoDriveJobsForTest(storePath())
  const job = createAutoDriveJob(req(), "ses_test", null)
  const final = finalizeAutoDriveJobFromResult(job.id, {
    stdout: "AUTO_DRIVE_STATUS: BLOCKED\nBLOCKERS:\n- 需求范围不清楚",
    stderr: "",
    exitCode: 0,
    timedOut: false,
    durationMs: 1200,
    queuedMs: 100,
  })
  assert.equal(final?.state, "blocked")
  assert.equal(final?.blockers[0], "需求范围不清楚")
})

test("finalizeAutoDriveJobFromResult marks clean DONE output as done", () => {
  _resetAutoDriveJobsForTest(storePath())
  const job = createAutoDriveJob(req(), "ses_test", null)
  const final = finalizeAutoDriveJobFromResult(job.id, {
    stdout: "AUTO_DRIVE_STATUS: DONE\nSUMMARY:\n- 已完成可自动推进项\nBLOCKERS:\n- none",
    stderr: "",
    exitCode: 0,
    timedOut: false,
    durationMs: 1200,
    queuedMs: 100,
  })
  assert.equal(final?.state, "done")
  assert.deepEqual(final?.blockers, [])
})

test("buildAutoDrivePrompt includes mandatory human gates", () => {
  const prompt = buildAutoDrivePrompt(req())
  assert.match(prompt, /需求对齐/)
  assert.match(prompt, /测试覆盖/)
  assert.match(prompt, /AUTO_DRIVE_STATUS/)
})
