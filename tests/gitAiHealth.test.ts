/**
 * Tests for read-only git-ai health aggregation.
 *
 * These tests assert the public payload shape and path-based Pi extension
 * detection without starting a Pi session or calling the company ai-stats API.
 */

import { test } from "node:test"
import { strict as assert } from "node:assert"

import { readGitAiHealth } from "../src/gitAiHealth.ts"

test("readGitAiHealth returns CLI hook and Pi extension sections", async () => {
  const health = await readGitAiHealth()
  assert.ok(health.generatedAt > 0)
  assert.ok(health.storePath.endsWith("git-ai-suspects.json"))
  assert.equal(typeof health.cli.installed, "boolean")
  assert.equal(typeof health.cli.postCommitHook.exists, "boolean")
  assert.equal(typeof health.cli.prePushHook.exists, "boolean")
  assert.ok(health.piExtension.globalPath.endsWith("/.pi/agent/extensions/git-ai.ts"))
  assert.ok(Array.isArray(health.piExtension.tracksTools))
})
