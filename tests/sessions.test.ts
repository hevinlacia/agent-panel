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

import { parseModelString, deriveWorktree, groupSessionsByParent, applyAgeFilter, type SessionInfo } from "../src/sessions.ts"

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
  const out = deriveWorktree({ directory: `${home}/GitHub/agent-panel`, path: "GitHub/agent-panel" })
  assert.equal(out, "~/GitHub/agent-panel")
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
  const out = deriveWorktree({ directory: "", path: "GitHub/agent-panel" })
  assert.equal(out, "~/GitHub/agent-panel")
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
  const out = deriveWorktree({ directory: "", path: "/GitHub/agent-panel" })
  assert.equal(out, "~/GitHub/agent-panel")
})


// ---------------------------------------------------------------------------
// groupSessionsByParent
// ---------------------------------------------------------------------------

function mkSession(over: Partial<SessionInfo> & { id: string }): SessionInfo {
  return {
    id: over.id,
    title: over.title ?? `title-${over.id}`,
    created: over.created ?? 1_000_000,
    updated: over.updated ?? 1_000_000,
    projectId: over.projectId ?? "global",
    directory: over.directory ?? "",
    status: over.status ?? "idle",
    source: over.source ?? "db",
    parentId: over.parentId,
  }
}

test("groupSessionsByParent: top-level sessions (no parentId) appear in top", () => {
  const sessions = [
    mkSession({ id: "ses_aaa" }),
    mkSession({ id: "ses_bbb" }),
  ]
  const { top, childrenByParent } = groupSessionsByParent(sessions)
  assert.equal(top.length, 2)
  assert.deepEqual(top.map((s) => s.id).sort(), ["ses_aaa", "ses_bbb"])
  assert.equal(childrenByParent.size, 0)
})

test("groupSessionsByParent: children are bucketed under the correct parent", () => {
  const sessions = [
    mkSession({ id: "ses_parent" }),
    mkSession({ id: "ses_child1", parentId: "ses_parent" }),
    mkSession({ id: "ses_child2", parentId: "ses_parent" }),
    mkSession({ id: "ses_other" }),
  ]
  const { top, childrenByParent } = groupSessionsByParent(sessions)
  assert.equal(top.length, 2)
  assert.ok(top.some((s) => s.id === "ses_parent"))
  assert.ok(top.some((s) => s.id === "ses_other"))
  const kids = childrenByParent.get("ses_parent")
  assert.ok(kids, "expected childrenByParent entry for ses_parent")
  assert.equal(kids!.length, 2)
  assert.deepEqual(
    kids!.map((s) => s.id).sort(),
    ["ses_child1", "ses_child2"],
  )
  assert.equal(childrenByParent.has("ses_other"), false)
})

test("groupSessionsByParent: orphan children (parentId not in id set) fall back to top", () => {
  const sessions = [
    mkSession({ id: "ses_orphan1", parentId: "ses_missing1" }),
    mkSession({ id: "ses_orphan2", parentId: "ses_missing2" }),
    mkSession({ id: "ses_real" }),
  ]
  const { top, childrenByParent } = groupSessionsByParent(sessions)
  assert.equal(top.length, 3)
  assert.equal(childrenByParent.size, 0)
})

test("groupSessionsByParent: children are sorted by updated desc within each parent", () => {
  const sessions = [
    mkSession({ id: "ses_parent" }),
    mkSession({ id: "ses_oldest", parentId: "ses_parent", updated: 1000 }),
    mkSession({ id: "ses_newest", parentId: "ses_parent", updated: 5000 }),
    mkSession({ id: "ses_middle", parentId: "ses_parent", updated: 3000 }),
  ]
  const { childrenByParent } = groupSessionsByParent(sessions)
  const kids = childrenByParent.get("ses_parent")!
  assert.deepEqual(
    kids.map((s) => s.id),
    ["ses_newest", "ses_middle", "ses_oldest"],
  )
})

test("groupSessionsByParent: empty input returns empty top and empty map", () => {
  const { top, childrenByParent } = groupSessionsByParent([])
  assert.deepEqual(top, [])
  assert.equal(childrenByParent.size, 0)
})


// ---------------------------------------------------------------------------
// applyAgeFilter
// ---------------------------------------------------------------------------

test("applyAgeFilter: undefined maxAgeMs returns the input untouched", () => {
  const now = Date.now()
  const sessions = [
    mkSession({ id: "ses_a", updated: now }),
    mkSession({ id: "ses_b", updated: now - 60_000 }),
  ]
  const out = applyAgeFilter(sessions, undefined)
  assert.equal(out, sessions)
  assert.equal(out.length, 2)
})

test("applyAgeFilter: filters out sessions older than maxAgeMs", () => {
  const now = Date.now()
  const sessions = [
    mkSession({ id: "ses_fresh", updated: now - 10_000 }),       // 10s ago
    mkSession({ id: "ses_recent", updated: now - 30_000 }),      // 30s ago
    mkSession({ id: "ses_old", updated: now - 5 * 60_000 }),     // 5m ago
    mkSession({ id: "ses_ancient", updated: now - 24 * 3600_000 }), // 1d ago
  ]
  // 1 minute window: only the two newer sessions survive.
  const out = applyAgeFilter(sessions, 60_000)
  assert.deepEqual(out.map((s) => s.id), ["ses_fresh", "ses_recent"])
})

test("applyAgeFilter: zero or negative maxAgeMs disables the filter", () => {
  const now = Date.now()
  const sessions = [
    mkSession({ id: "ses_a", updated: now - 10 * 365 * 24 * 3600_000 }),
    mkSession({ id: "ses_b", updated: now }),
  ]
  assert.equal(applyAgeFilter(sessions, 0).length, 2)
  assert.equal(applyAgeFilter(sessions, -1).length, 2)
})

test("applyAgeFilter: non-finite maxAgeMs disables the filter", () => {
  const now = Date.now()
  const sessions = [mkSession({ id: "ses_a", updated: now - 1_000_000 })]
  assert.equal(applyAgeFilter(sessions, Number.NaN).length, 1)
  assert.equal(applyAgeFilter(sessions, Number.POSITIVE_INFINITY).length, 1)
})

test("applyAgeFilter: empty input returns empty array", () => {
  const out = applyAgeFilter([], 60_000)
  assert.deepEqual(out, [])
})

test("applyAgeFilter: falls back to `created` when `updated` is 0", () => {
  const now = Date.now()
  const sessions = [
    mkSession({ id: "ses_a", updated: 0, created: now - 10_000 }),  // recent via created
    mkSession({ id: "ses_b", updated: 0, created: now - 5 * 60_000 }), // 5m old via created
  ]
  const out = applyAgeFilter(sessions, 60_000)
  assert.deepEqual(out.map((s) => s.id), ["ses_a"])
})
