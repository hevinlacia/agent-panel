/**
 * Tests for `src/piConfig.ts` safe Pi config editing helpers.
 */

import { test } from "node:test"
import { strict as assert } from "node:assert"
import { mkdtempSync } from "node:fs"
import { readFileSync, writeFileSync } from "node:fs"
import { tmpdir } from "node:os"
import { join } from "node:path"

import {
  _resetForTest,
  getPiConfigFile,
  readPiConfigSummary,
  resolvePiModelCredentials,
  savePiConfigFile,
  updatePiSettings,
} from "../src/piConfig.ts"

function newTmpDir(): string {
  return mkdtempSync(join(tmpdir(), "pi-config-"))
}

test("readPiConfigSummary lists models without exposing api keys", async () => {
  const dir = newTmpDir()
  _resetForTest(dir)
  writeFileSync(join(dir, "settings.json"), JSON.stringify({ defaultProvider: "router", defaultModel: "fast", defaultThinkingLevel: "high", enabledModels: ["router/*"] }))
  writeFileSync(join(dir, "models.json"), JSON.stringify({ providers: { router: { api: "openai-responses", apiKey: "secret", models: [{ id: "fast", name: "Fast", reasoning: true, thinkingLevelMap: { high: "high", max: "max", off: null } }] } } }))

  const summary = await readPiConfigSummary()
  assert.equal(summary.settings.defaultProvider, "router")
  assert.equal(summary.providers[0].hasApiKey, true)
  assert.equal(summary.providers[0].models[0].modelId, "fast")
  assert.deepEqual(summary.providers[0].models[0].thinkingLevels, ["high", "max"])
  assert.equal(JSON.stringify(summary).includes("secret"), false)
})

test("updatePiSettings preserves unknown keys and normalizes enabled models", async () => {
  const dir = newTmpDir()
  _resetForTest(dir)
  writeFileSync(join(dir, "settings.json"), JSON.stringify({ lastChangelogVersion: "0.80.6" }))

  await updatePiSettings({ defaultProvider: "router", defaultModel: "fast", defaultThinkingLevel: "max", enabledModels: [" router/* ", "", "router/*"] })
  const parsed = JSON.parse(readFileSync(join(dir, "settings.json"), "utf-8"))
  assert.equal(parsed.lastChangelogVersion, "0.80.6")
  assert.equal(parsed.defaultProvider, "router")
  assert.equal(parsed.defaultModel, "fast")
  assert.equal(parsed.defaultThinkingLevel, "max")
  assert.deepEqual(parsed.enabledModels, ["router/*"])
})

test("models config editor redacts and restores secret placeholders", async () => {
  const dir = newTmpDir()
  _resetForTest(dir)
  writeFileSync(join(dir, "models.json"), JSON.stringify({ providers: { router: { apiKey: "keep-me", models: [{ id: "old" }] } } }))

  const snapshot = await getPiConfigFile("models")
  assert.equal(snapshot.content.includes("keep-me"), false)
  assert.equal(snapshot.content.includes("__AGENT_PANEL_SECRET__"), true)

  const edited = snapshot.content.replace('"old"', '"new"')
  await savePiConfigFile("models", edited)
  const parsed = JSON.parse(readFileSync(join(dir, "models.json"), "utf-8"))
  assert.equal(parsed.providers.router.apiKey, "keep-me")
  assert.equal(parsed.providers.router.models[0].id, "new")
})

test("resolvePiModelCredentials returns baseUrl + real key for a provider/model", async () => {
  const dir = newTmpDir()
  _resetForTest(dir)
  writeFileSync(join(dir, "models.json"), JSON.stringify({
    providers: {
      router: { api: "openai-completions", baseUrl: "http://127.0.0.1:8789/v1", apiKey: "router-secret", models: [{ id: "fast" }] },
      nokey: { baseUrl: "https://api.example.com", models: [{ id: "m" }] },
    },
  }))

  const creds = await resolvePiModelCredentials("router/fast")
  assert.equal(creds?.providerId, "router")
  assert.equal(creds?.modelId, "fast")
  assert.equal(creds?.baseUrl, "http://127.0.0.1:8789/v1")
  assert.equal(creds?.apiKey, "router-secret")
})

test("resolvePiModelCredentials returns null for missing provider, key, or baseUrl", async () => {
  const dir = newTmpDir()
  _resetForTest(dir)
  writeFileSync(join(dir, "models.json"), JSON.stringify({
    providers: {
      router: { baseUrl: "http://127.0.0.1:8789/v1", apiKey: "router-secret", models: [{ id: "fast" }] },
      nokey: { baseUrl: "https://api.example.com", models: [{ id: "m" }] },
      nobase: { apiKey: "k", models: [{ id: "m" }] },
    },
  }))

  // Unknown provider.
  assert.equal(await resolvePiModelCredentials("ghost/fast"), null)
  // Provider without an API key.
  assert.equal(await resolvePiModelCredentials("nokey/m"), null)
  // Provider without a baseUrl.
  assert.equal(await resolvePiModelCredentials("nobase/m"), null)
  // Empty / malformed input.
  assert.equal(await resolvePiModelCredentials(""), null)
  assert.equal(await resolvePiModelCredentials("router"), null)
  assert.equal(await resolvePiModelCredentials("/fast"), null)
})
