/**
 * Tests for `src/config.ts`.
 */

import { test } from "node:test"
import { strict as assert } from "node:assert"
import { mkdtempSync } from "node:fs"
import { tmpdir } from "node:os"
import { join } from "node:path"

import {
  buildManagedEnv,
  deleteEnvVar,
  getConfig,
  safeEnvVars,
  setConfig,
  initConfig,
  upsertEnvVar,
  _resetForTest,
} from "../src/config.ts"

function newTmpPath(): string {
  return join(mkdtempSync(join(tmpdir(), "opencode-config-")), "config.json")
}

test("getConfig returns defaults when no file exists", async () => {
  _resetForTest(newTmpPath())
  const cfg = await getConfig()
  assert.equal(cfg.autoExtract, false)
  assert.equal(cfg.extractModel, "litellm-local/deepseek-v4-flash-auto")
  assert.equal(cfg.minChangeMessages, 5)
  assert.equal(cfg.fullSyncSchedule, true)
  assert.deepEqual(cfg.fullSyncTimes, ["12:00", "18:00", "20:30", "23:30"])
  assert.deepEqual(cfg.fullSyncGithubRepos, [])
  assert.deepEqual(cfg.envVars, [])
})

test("setConfig persists and reloads", async () => {
  const p = newTmpPath()
  _resetForTest(p)
  await setConfig({
    autoExtract: true,
    extractModel: "gpt-4o",
    minChangeMessages: 10,
    fullSyncSchedule: false,
    fullSyncTimes: ["7:05", "18:00", "bad"],
    fullSyncGithubRepos: ["github/browser-harness", "/etc/passwd"],
  })
  _resetForTest(p)
  await initConfig()
  const cfg = await getConfig()
  assert.equal(cfg.autoExtract, true)
  assert.equal(cfg.extractModel, "gpt-4o")
  assert.equal(cfg.minChangeMessages, 10)
  assert.equal(cfg.fullSyncSchedule, false)
  assert.deepEqual(cfg.fullSyncTimes, ["07:05", "18:00"])
  assert.deepEqual(cfg.fullSyncGithubRepos, ["/home/hevin/Developer/github/browser-harness"])
})

test("setConfig merges partial updates", async () => {
  const p = newTmpPath()
  _resetForTest(p)
  await setConfig({ autoExtract: true })
  await setConfig({ minChangeMessages: 20 })
  const cfg = await getConfig()
  assert.equal(cfg.autoExtract, true)
  assert.equal(cfg.minChangeMessages, 20)
  assert.equal(cfg.extractModel, "litellm-local/deepseek-v4-flash-auto")
  assert.equal(cfg.fullSyncSchedule, true)
})

test("env vars persist normalized names and redacted safe previews", async () => {
  const p = newTmpPath()
  _resetForTest(p)
  await setConfig({
    envVars: [
      { name: " wms_uat_cookie ", value: "abcdef123456", note: "  UAT cookie  ", updatedAt: 123 },
      { name: "bad-name", value: "ignored", note: "", updatedAt: 456 },
    ],
  })
  _resetForTest(p)
  await initConfig()
  const cfg = await getConfig()
  assert.equal(cfg.envVars.length, 1)
  assert.equal(cfg.envVars[0].name, "WMS_UAT_COOKIE")
  assert.equal(cfg.envVars[0].value, "abcdef123456")
  assert.equal(cfg.envVars[0].note, "UAT cookie")
  const vars = await safeEnvVars(cfg)
  const custom = vars.find((v) => v.name === "WMS_UAT_COOKIE")
  assert.ok(custom, "WMS_UAT_COOKIE should appear in safeEnvVars")
  assert.equal(custom.preview, "abcd****3456")
  assert.equal(custom.note, "UAT cookie")
  assert.equal(custom.updatedAt, 123)
  assert.equal(custom.hasValue, true)
  assert.equal(custom.source, "managed")
  assert.equal(custom.requiredBy, "Custom")
  assert.equal(custom.file, "secrets")
  assert.ok(custom.filePath.includes("opencode-secrets.env"), "filePath should point to secrets env file")
})

test("safeEnvVars always shows Ylops token requirement", async () => {
  const p = newTmpPath()
  _resetForTest(p)
  const original = process.env.YLOPS_TOKEN
  delete process.env.YLOPS_TOKEN
  try {
    const cfg = await getConfig()
    const vars = await safeEnvVars(cfg)
    const ylops = vars.find((entry) => entry.name === "YLOPS_TOKEN")
    assert.equal(ylops?.requiredBy, "Ylops CI/CD Deploy")
    assert.equal(ylops?.source, "missing")
    assert.equal(ylops?.hasValue, false)
    assert.equal(ylops?.file, "secrets")
  } finally {
    if (original === undefined) delete process.env.YLOPS_TOKEN
    else process.env.YLOPS_TOKEN = original
  }
})

test("buildManagedEnv overlays dashboard-managed values", async () => {
  const p = newTmpPath()
  _resetForTest(p)
  await setConfig({ envVars: [{ name: "DASHBOARD_TEST_TOKEN", value: "secret-value", note: "", updatedAt: 1 }] })
  const env = await buildManagedEnv({ TERM: "xterm-256color" })
  assert.equal(env.DASHBOARD_TEST_TOKEN, "secret-value")
  assert.equal(env.TERM, "xterm-256color")
})

test("upsertEnvVar writes OpenCode secrets env and buildManagedEnv loads it", async () => {
  const p = newTmpPath()
  _resetForTest(p)
  await getConfig()
  await upsertEnvVar("YLOPS_TOKEN", "token-from-dashboard", "secrets")
  const cfg = await getConfig()
  const vars = await safeEnvVars(cfg)
  const ylops = vars.find((entry) => entry.name === "YLOPS_TOKEN")
  assert.equal(ylops?.source, "managed")
  assert.equal(ylops?.preview, "toke****oard")
  const env = await buildManagedEnv()
  assert.equal(env.YLOPS_TOKEN, "token-from-dashboard")
  await deleteEnvVar("YLOPS_TOKEN")
  const after = await buildManagedEnv()
  assert.notEqual(after.YLOPS_TOKEN, "token-from-dashboard")
})
