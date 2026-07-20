/**
 * Tests for git-ai suspect commit store and company-check refresh behavior.
 *
 * The real company ai-stats endpoint is intentionally not called here; tests
 * inject a checker so only persistence, merge semantics, and status mapping are
 * covered.
 */

import { test } from "node:test"
import { strict as assert } from "node:assert"
import { mkdtempSync, existsSync, readFileSync } from "node:fs"
import { tmpdir } from "node:os"
import { join } from "node:path"

import {
  _resetGitAiSuspectsForTest,
  initGitAiSuspects,
  recordGitAiSuspect,
  listGitAiSuspects,
  buildGitAiSuspectStats,
  refreshGitAiSuspects,
} from "../src/gitAiSuspects.ts"

function storePath(): string {
  return join(mkdtempSync(join(tmpdir(), "agent-panel-git-ai-")), "suspects.json")
}

test("recordGitAiSuspect persists and merges duplicate project+commit records", async () => {
  const path = storePath()
  _resetGitAiSuspectsForTest(path)
  const first = await recordGitAiSuspect({
    projectName: "yl-cwhsea-wms-system-api",
    commitSha: "c45dcf91771a43c764381ccd2bbc8441590cfbb8",
    repoPath: "/repo/system-api",
    eventSources: ["post-commit"],
    localNoteState: "missing",
  })
  const second = await recordGitAiSuspect({
    projectName: "yl-cwhsea-wms-system-api",
    commitSha: "c45dcf91771a43c764381ccd2bbc8441590cfbb8",
    gitlabProjectId: "13788",
    eventSources: ["pre-push"],
    localNoteState: "complete",
  })

  assert.equal(second.id, first.id)
  assert.deepEqual(second.eventSources.sort(), ["post-commit", "pre-push"])
  assert.equal(second.gitlabProjectId, "13788")
  assert.equal(second.localNoteState, "complete")
  assert.ok(existsSync(path))
  assert.match(readFileSync(path, "utf-8"), /yl-cwhsea-wms-system-api/)

  _resetGitAiSuspectsForTest(path)
  await initGitAiSuspects()
  assert.equal(listGitAiSuspects().length, 1)
})

test("refreshGitAiSuspects applies company checker result and stats", async () => {
  const path = storePath()
  _resetGitAiSuspectsForTest(path)
  await recordGitAiSuspect({
    projectName: "yl-cwhsea-wms-api",
    commitSha: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    eventSources: ["pre-push"],
  })

  const refreshed = await refreshGitAiSuspects({
    checker: async () => ({
      companyStatus: "missing_ai",
      companyError: null,
      commitTitle: "feat: test missing ai mark",
      aiRate: 0,
      aiLines: 0,
      humanLines: 12,
    }),
  })

  assert.equal(refreshed[0].companyStatus, "missing_ai")
  assert.equal(refreshed[0].commitTitle, "feat: test missing ai mark")
  const stats = buildGitAiSuspectStats(refreshed)
  assert.equal(stats.total, 1)
  assert.equal(stats.missingAi, 1)
  assert.equal(stats.pending, 0)
})
