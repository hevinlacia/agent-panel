/**
 * Regression tests for the side-effect-free helpers exported by
 * `src/sessions.ts`. These cover the new SQLite metadata parsing and
 * the worktree derivation rules used by the redesigned Operator
 * sessions page.
 *
 * The helpers are pure and do NOT spawn the opencode CLI, sqlite3, or
 * touch the filesystem, so they can be exercised on any platform.
 */

import { test } from "node:test"
import { strict as assert } from "node:assert"
import { homedir } from "node:os"

import { parseModelString, deriveWorktree } from "../src/sessions.ts"

test("parseModelString: parses the canonical OpenCode model JSON", () => {
  const raw = JSON.stringify({
    id: "minimax-latest-auto",
    providerID: "litellm-local",
    variant: "default",
  })
  const out = parseModelString(raw)
  assert.equal(out.modelId, "minimax-latest-auto")
  assert.equal(out.modelProvider, "litellm-local")
  assert.equal(out.modelVariant, "default")
})

test("parseModelString: keeps the raw text when JSON parse fails", () => {
  // Some rows may carry a non-JSON placeholder; we must not throw and
  // must surface the raw text as modelId so the UI can still show it.
  const raw = "gpt-5.5"
  const out = parseModelString(raw)
  assert.equal(out.modelId, "gpt-5.5")
  assert.equal(out.modelProvider, undefined)
  assert.equal(out.modelVariant, undefined)
})

test("parseModelString: returns empty for non-string input", () => {
  assert.deepEqual(parseModelString(undefined), {})
  assert.deepEqual(parseModelString(null), {})
  assert.deepEqual(parseModelString(0), {})
  assert.deepEqual(parseModelString(""), {})
  assert.deepEqual(parseModelString({ id: "x", providerID: "y" }), {})
})

test("parseModelString: tolerates non-object JSON payloads", () => {
  // `JSON.parse("42")` returns the number 42; the helper should not
  // crash and should fall back to the raw text.
  const out = parseModelString("42")
  assert.equal(out.modelId, "42")
})

test("parseModelString: tolerates JSON with missing keys", () => {
  const out = parseModelString(JSON.stringify({ id: "claude-sonnet" }))
  assert.equal(out.modelId, "claude-sonnet")
  assert.equal(out.modelProvider, undefined)
  assert.equal(out.modelVariant, undefined)
})

test("parseModelString: tolerates JSON with non-string keys", () => {
  const out = parseModelString(JSON.stringify({ id: 7, providerID: null, variant: ["x"] }))
  // Non-string id -> fall back to the raw JSON text.
  assert.equal(out.modelId, JSON.stringify({ id: 7, providerID: null, variant: ["x"] }))
  assert.equal(out.modelProvider, undefined)
  assert.equal(out.modelVariant, undefined)
})

test("deriveWorktree: rewrites an under-$HOME directory as ~/...", () => {
  const home = homedir()
  const out = deriveWorktree({ directory: `${home}/GitHub/opencode-dashboard`, path: "GitHub/opencode-dashboard" })
  assert.equal(out, "~/GitHub/opencode-dashboard")
})

test("deriveWorktree: returns ~ when directory === $HOME", () => {
  const out = deriveWorktree({ directory: homedir() })
  assert.equal(out, "~")
})

test("deriveWorktree: keeps absolute path when directory is outside $HOME", () => {
  const out = deriveWorktree({ directory: "/srv/opencode", path: "opencode" })
  assert.equal(out, "/srv/opencode")
})

test("deriveWorktree: falls back to ~/path when only `path` is set", () => {
  const out = deriveWorktree({ directory: "", path: "GitHub/opencode-dashboard" })
  assert.equal(out, "~/GitHub/opencode-dashboard")
})

test("deriveWorktree: returns 'none' when both directory and path are empty", () => {
  assert.equal(deriveWorktree({}), "none")
  assert.equal(deriveWorktree({ directory: "", path: "" }), "none")
  assert.equal(deriveWorktree({ directory: undefined, path: undefined }), "none")
})

test("deriveWorktree: tolerates null inputs", () => {
  assert.equal(deriveWorktree({ directory: null, path: null }), "none")
})

test("deriveWorktree: strips leading slashes from `path`", () => {
  // The SQLite `path` column is a relative path; leading slashes are
  // an artifact of broken rows and should not leak into the UI.
  const out = deriveWorktree({ directory: "", path: "/GitHub/opencode-dashboard" })
  assert.equal(out, "~/GitHub/opencode-dashboard")
})
