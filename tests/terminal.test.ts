/**
 * Regression tests for `parseClientMessage`.
 *
 * The parser lives in `src/terminalProtocol.ts` (a side-effect-free module
 * that does not import `node-pty`), and is re-exported by `src/terminal.ts`
 * for runtime callers. Tests import the pure module directly so `node
 * --test` does not load the native pty binding just to exercise parsing.
 *
 * The helper translates a WebSocket text frame into one of:
 *   - { kind: "input",  data }   — write data to PTY stdin
 *   - { kind: "resize", cols, rows } — resize the PTY
 *   - { kind: "ignore" }         — drop the frame (e.g. ping / malformed resize)
 *
 * These tests intentionally do NOT spawn a PTY; only the pure parsing
 * function is exercised.
 */

import { test } from "node:test"
import { strict as assert } from "node:assert"

import { parseClientMessage } from "../src/terminalProtocol.ts"

test("parseClientMessage: raw input passes through unchanged", () => {
  const result = parseClientMessage("ls -la\n")
  assert.equal(result.kind, "input")
  if (result.kind === "input") {
    assert.equal(result.data, "ls -la\n")
  }
})

test("parseClientMessage: malformed JSON starting with `{` falls back to input", () => {
  // Looks like a JSON object but is not valid; the helper must not throw
  // and must treat the frame as raw PTY input.
  const raw = "{not valid json"
  const result = parseClientMessage(raw)
  assert.equal(result.kind, "input")
  if (result.kind === "input") {
    assert.equal(result.data, raw)
  }
})

test("parseClientMessage: valid resize yields resize kind with numeric cols/rows", () => {
  const raw = JSON.stringify({ type: "resize", cols: 120, rows: 40 })
  const result = parseClientMessage(raw)
  assert.deepEqual(result, { kind: "resize", cols: 120, rows: 40 })
})

test("parseClientMessage: resize accepts string numbers coerced to finite numbers", () => {
  // parseClientMessage uses Number(obj.cols) as a fallback when cols is a
  // non-number value, so digit-strings should still be accepted.
  const raw = JSON.stringify({ type: "resize", cols: "100", rows: "30" })
  const result = parseClientMessage(raw)
  assert.deepEqual(result, { kind: "resize", cols: 100, rows: 30 })
})

test("parseClientMessage: invalid resize (non-finite dims) is ignored", () => {
  // Non-numeric, non-numeric-string cols/rows => NaN => ignore.
  const raw = JSON.stringify({ type: "resize", cols: "abc", rows: "xyz" })
  const result = parseClientMessage(raw)
  assert.deepEqual(result, { kind: "ignore" })
})

test("parseClientMessage: ping is ignored (keepalive convenience)", () => {
  const result = parseClientMessage(JSON.stringify({ type: "ping" }))
  assert.deepEqual(result, { kind: "ignore" })
})

test("parseClientMessage: unknown JSON object remains as input (compatibility)", () => {
  // Documents current behavior: a JSON object with an unrecognized `type`
  // is NOT silently dropped. It is treated as raw PTY input so we don't
  // break clients that mix typed and untyped frames. If you change this,
  // update this test and the helper's doc comment together.
  const raw = JSON.stringify({ type: "unknown", payload: 42 })
  const result = parseClientMessage(raw)
  assert.equal(result.kind, "input")
  if (result.kind === "input") {
    assert.equal(result.data, raw)
  }
})

test("parseClientMessage: empty string is ignored", () => {
  assert.deepEqual(parseClientMessage(""), { kind: "ignore" })
})

test("parseClientMessage: non-JSON text starting with non-`{` is input", () => {
  const result = parseClientMessage("hello world")
  assert.equal(result.kind, "input")
  if (result.kind === "input") {
    assert.equal(result.data, "hello world")
  }
})

// --- No-type / missing-type boundary cases ---------------------------------
//
// Current behavior: parseClientMessage only short-circuits to a typed
// control message for `type === "resize"` (with finite cols/rows) or
// `type === "ping"`. Every other JSON object — including a bare `{}`,
// `null`, `undefined`, or an explicit `type: null` / `type: undefined` —
// falls through the `obj.type === ...` checks and is treated as raw PTY
// input. This is intentional: clients may interleave JSON control frames
// with terminal bytes, and the raw JSON string is forwarded verbatim so
// the downstream program can decide what to do with it. If this policy
// ever changes, update these tests and the helper's doc comment together.

test("parseClientMessage: bare `{}` object is treated as raw input", () => {
  const raw = "{}"
  const result = parseClientMessage(raw)
  assert.equal(result.kind, "input")
  if (result.kind === "input") {
    assert.equal(result.data, raw)
  }
})

test("parseClientMessage: { type: null } is treated as raw input", () => {
  const raw = '{"type":null}'
  const result = parseClientMessage(raw)
  assert.equal(result.kind, "input")
  if (result.kind === "input") {
    assert.equal(result.data, raw)
  }
})

test("parseClientMessage: object with no `type` key is treated as raw input", () => {
  // The parser only branches on `obj.type === "resize"` or
  // `obj.type === "ping"`, so a JSON object without a `type` field (or
  // whose `type` is null / undefined / an empty string) falls through
  // and is forwarded as raw PTY input. We feed the explicit raw JSON
  // string with no `type` key here; `JSON.stringify({ type: undefined,
  // cols: 80, rows: 24 })` produces the same wire bytes (JSON.stringify
  // drops `undefined`), so spelling the source object either way
  // exercises the same parser branch.
  const raw = '{"cols":80,"rows":24}'
  const result = parseClientMessage(raw)
  assert.equal(result.kind, "input")
  if (result.kind === "input") {
    assert.equal(result.data, raw)
  }
})

test('parseClientMessage: { type: "" } (empty string) is treated as raw input', () => {
  // Empty-string `type` is neither "resize" nor "ping", so it must not
  // be matched. Documents that only the exact two type strings are
  // recognized as control frames.
  const raw = '{"type":""}'
  const result = parseClientMessage(raw)
  assert.equal(result.kind, "input")
  if (result.kind === "input") {
    assert.equal(result.data, raw)
  }
})
