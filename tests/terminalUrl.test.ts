/**
 * Regression tests for the requirement-context auto-injection gating.
 *
 * Lives in `src/terminalUrl.ts` as a pure module so these tests can run
 * without spawning Hono, node-pty, or a browser. Tests both the gating
 * predicate (`shouldAutoInjectRequirementContext`) and the WS URL builder
 * (`buildTerminalWebSocketUrl`) — the latter mirrors the inline string
 * construction in `public/terminal.js`.
 */

import { test } from "node:test"
import { strict as assert } from "node:assert"

import {
  buildTerminalWebSocketUrl,
  shouldAutoInjectRequirementContext,
} from "../src/terminalUrl.ts"

// ---------------------------------------------------------------------------
// shouldAutoInjectRequirementContext
// ---------------------------------------------------------------------------

test("shouldAutoInjectRequirementContext: ?req=REQ-1 alone => false", () => {
  assert.equal(
    shouldAutoInjectRequirementContext("https://x/session?req=REQ-1"),
    false,
  )
  assert.equal(shouldAutoInjectRequirementContext("?req=REQ-1"), false)
  assert.equal(shouldAutoInjectRequirementContext("req=REQ-1"), false)
})

test("shouldAutoInjectRequirementContext: ?req=REQ-1&inject=1 => true", () => {
  assert.equal(
    shouldAutoInjectRequirementContext("https://x/session?req=REQ-1&inject=1"),
    true,
  )
  assert.equal(
    shouldAutoInjectRequirementContext("?req=REQ-1&inject=1"),
    true,
  )
  assert.equal(shouldAutoInjectRequirementContext("inject=1&req=REQ-1"), true)
})

test("shouldAutoInjectRequirementContext: inject=true / 0 / missing => false", () => {
  assert.equal(shouldAutoInjectRequirementContext("?inject=true"), false)
  assert.equal(shouldAutoInjectRequirementContext("?inject=0"), false)
  assert.equal(shouldAutoInjectRequirementContext("?inject="), false)
  assert.equal(shouldAutoInjectRequirementContext("?other=1"), false)
  assert.equal(shouldAutoInjectRequirementContext(""), false)
  assert.equal(shouldAutoInjectRequirementContext(null), false)
  assert.equal(shouldAutoInjectRequirementContext(undefined), false)
})

test("shouldAutoInjectRequirementContext: accepts URL and URLSearchParams instances", () => {
  const u = new URL("https://x/session?req=REQ-1&inject=1")
  assert.equal(shouldAutoInjectRequirementContext(u), true)

  const p = new URLSearchParams("req=REQ-1&inject=1")
  assert.equal(shouldAutoInjectRequirementContext(p), true)

  const u2 = new URL("https://x/session?req=REQ-1")
  assert.equal(shouldAutoInjectRequirementContext(u2), false)

  const p2 = new URLSearchParams("inject=true")
  assert.equal(shouldAutoInjectRequirementContext(p2), false)
})

// ---------------------------------------------------------------------------
// buildTerminalWebSocketUrl
// ---------------------------------------------------------------------------

test("buildTerminalWebSocketUrl: no reqId, no autoInject => only id", () => {
  const url = buildTerminalWebSocketUrl({
    protocol: "http:",
    host: "localhost:7331",
    sessionId: "ses_abc123",
  })
  assert.equal(
    url,
    "ws://localhost:7331/ws/session-terminal?id=ses_abc123",
  )
})

test("buildTerminalWebSocketUrl: includes req without inject by default", () => {
  const url = buildTerminalWebSocketUrl({
    protocol: "http:",
    host: "localhost:7331",
    sessionId: "ses_abc123",
    reqId: "REQ-1",
  })
  assert.equal(
    url,
    "ws://localhost:7331/ws/session-terminal?id=ses_abc123&req=REQ-1",
  )
  assert.ok(!url.includes("inject="))
})

test("buildTerminalWebSocketUrl: includes inject=1 only when autoInject true", () => {
  const url = buildTerminalWebSocketUrl({
    protocol: "http:",
    host: "localhost:7331",
    sessionId: "ses_abc123",
    reqId: "REQ-1",
    autoInject: true,
  })
  assert.equal(
    url,
    "ws://localhost:7331/ws/session-terminal?id=ses_abc123&req=REQ-1&inject=1",
  )
})

test("buildTerminalWebSocketUrl: autoInject without reqId still appends inject", () => {
  // The server-side gate also requires reqId, but the builder is intentionally
  // permissive — the gating belongs to `shouldAutoInjectRequirementContext`
  // and the server handler, not the URL formatter.
  const url = buildTerminalWebSocketUrl({
    protocol: "http:",
    host: "localhost:7331",
    sessionId: "ses_abc123",
    autoInject: true,
  })
  assert.equal(
    url,
    "ws://localhost:7331/ws/session-terminal?id=ses_abc123&inject=1",
  )
})

test("buildTerminalWebSocketUrl: https protocol upgrades to wss", () => {
  const url = buildTerminalWebSocketUrl({
    protocol: "https:",
    host: "example.com",
    sessionId: "ses_abc123",
    reqId: "REQ-1",
    autoInject: true,
  })
  assert.equal(
    url,
    "wss://example.com/ws/session-terminal?id=ses_abc123&req=REQ-1&inject=1",
  )
})

test("buildTerminalWebSocketUrl: encodes sessionId and reqId", () => {
  const url = buildTerminalWebSocketUrl({
    protocol: "http:",
    host: "localhost:7331",
    sessionId: "ses with space",
    reqId: "REQ/1?x",
  })
  assert.equal(
    url,
    "ws://localhost:7331/ws/session-terminal?id=ses%20with%20space&req=REQ%2F1%3Fx",
  )
})
