/**
 * Regression tests for `resolveHandoffPath` in src/paths.ts.
 *
 * The resolver is the single gate for any filesystem access driven by a
 * user-supplied `path` query parameter (`/report`, `/api/report`,
 * `/api/confirm`). It MUST reject:
 *   - anything outside `/tmp/opencode/handoff/`
 *   - traversal escapes via `..`
 *   - sibling directories with a longer shared prefix
 *   - null bytes
 *   - non-string / empty / relative input
 */

import { test } from "node:test"
import { strict as assert } from "node:assert"
import { resolve } from "node:path"

import { resolveHandoffPath, HANDOFF_ROOT } from "../src/paths.ts"

test("resolveHandoffPath: accepts a valid handoff report path", () => {
  const input = `${HANDOFF_ROOT}/auto-summary/foo.report.md`
  const expected = resolve(input)
  assert.equal(resolveHandoffPath(input), expected)
})

test("resolveHandoffPath: accepts the handoff root itself", () => {
  // Exact root (no trailing slash, no extra segment) is allowed by design.
  assert.equal(resolveHandoffPath(HANDOFF_ROOT), resolve(HANDOFF_ROOT))
})

test("resolveHandoffPath: rejects /etc/passwd", () => {
  assert.equal(resolveHandoffPath("/etc/passwd"), null)
})

test("resolveHandoffPath: rejects traversal escape via `..`", () => {
  // `/tmp/opencode/handoff/../etc/passwd` resolves to
  // `/tmp/opencode/etc/passwd`, which is outside the handoff root.
  const input = `${HANDOFF_ROOT}/../etc/passwd`
  assert.equal(resolveHandoffPath(input), null)
})

test("resolveHandoffPath: rejects deeply nested traversal", () => {
  // Even after `resolve()`, anything that escapes the prefix must be rejected.
  const input = `${HANDOFF_ROOT}/foo/../../../../etc/passwd`
  assert.equal(resolveHandoffPath(input), null)
})

test("resolveHandoffPath: rejects sibling directory /tmp/opencode/handoff-evil", () => {
  // The strict prefix check uses `/tmp/opencode/handoff/`, so the
  // sibling `/tmp/opencode/handoff-evil/` must NOT be accepted even
  // though it shares a long prefix with the root.
  const result = resolveHandoffPath("/tmp/opencode/handoff-evil/report.md")
  assert.equal(result, null)
})

test("resolveHandoffPath: rejects null byte", () => {
  // Null bytes are never legal in fs paths and are a common way to
  // smuggle past naive string comparisons.
  assert.equal(resolveHandoffPath(`${HANDOFF_ROOT}/foo\0bar.md`), null)
})

test("resolveHandoffPath: rejects empty string", () => {
  assert.equal(resolveHandoffPath(""), null)
})

test("resolveHandoffPath: rejects null", () => {
  assert.equal(resolveHandoffPath(null), null)
})

test("resolveHandoffPath: rejects undefined", () => {
  assert.equal(resolveHandoffPath(undefined), null)
})

test("resolveHandoffPath: rejects non-string types (number, object, array)", () => {
  assert.equal(resolveHandoffPath(123), null)
  assert.equal(resolveHandoffPath({}), null)
  assert.equal(resolveHandoffPath([]), null)
  assert.equal(resolveHandoffPath(true), null)
})

test("resolveHandoffPath: rejects relative (non-absolute) path", () => {
  // Must start with `/` to be considered.
  assert.equal(resolveHandoffPath("tmp/opencode/handoff/foo.md"), null)
  assert.equal(resolveHandoffPath("./handoff/foo.md"), null)
})
