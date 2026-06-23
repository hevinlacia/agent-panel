/**
 * Unit tests for the new Hermes-backed src/requirements.ts.
 *
 * The Hermes scanner reads from `~/.agents/req/`, which is hard to
 * isolate per-test, so these tests focus on the *association store*
 * (overridable via `_setStorePath`) and on functions whose behavior
 * is well-defined when no Hermes requirement directory is present:
 *
 *   - the synthetic default requirement
 *   - associateSession / getRequirementForSession / getRequirementTitleForSession
 *   - getAllAssociatedSessionIds
 *   - generateSessionId
 *   - buildInjectionContext for DEFAULT_REQ_ID
 *   - load/save round-trip + legacy migration
 *
 * Each test points the store at a fresh temp file under
 * /tmp/opencode/test-req-X/associations.json so the tests cannot
 * interfere with the real user store at
 * ~/.local/share/opencode-dashboard/associations.json.
 */

import { test } from "node:test"
import { strict as assert } from "node:assert"
import { join, dirname } from "node:path"
import { mkdirSync, writeFileSync, existsSync, readFileSync } from "node:fs"
import { randomBytes } from "node:crypto"

import {
  _setStorePath,
  _getStorePath,
  _setReqDir,
  _getReqDir,
  loadAssociations,
  saveAssociations,
  associateSession,
  getRequirementForSession,
  getRequirementTitleForSession,
  getAllAssociatedSessionIds,
  generateSessionId,
  buildInjectionContext,
  DEFAULT_REQ_ID,
} from "../src/requirements.ts"

function freshStore(): string {
  const dir = join("/tmp", "opencode", "test-req-" + randomBytes(6).toString("hex"))
  mkdirSync(dir, { recursive: true })
  const path = join(dir, "associations.json")
  _setStorePath(path)
  return path
}

test("loadAssociations: returns empty store when file doesn't exist", async () => {
  const path = freshStore()
  assert.equal(existsSync(path), false)
  const store = await loadAssociations()
  assert.equal(store.version, 2)
  assert.deepEqual(store.associations, {})
  assert.equal(_getStorePath(), path)
})

test("associateSession: adds sessionId to requirement", async () => {
  freshStore()
  await associateSession("REQ-TEST-001", "ses_abc")
  const req = await getRequirementForSession("ses_abc")
  // Since no Hermes dir contains "REQ-TEST-001", the function falls
  // back to the synthetic default requirement.
  assert.equal(req.id, DEFAULT_REQ_ID)

  // But the association itself is recorded in the store.
  const all = await getAllAssociatedSessionIds()
  assert.ok(all.has("ses_abc"))
})

test("associateSession: moves session from one requirement to another", async () => {
  freshStore()
  await associateSession("REQ-A", "ses_move")
  await associateSession("REQ-B", "ses_move")

  const store = await loadAssociations()
  // REQ-A should no longer contain the session (and may have been deleted).
  const inA = store.associations["REQ-A"] ?? []
  assert.equal(inA.includes("ses_move"), false)
  // REQ-B should contain it.
  const inB = store.associations["REQ-B"] ?? []
  assert.deepEqual(inB, ["ses_move"])
})

test("getRequirementForSession: returns default for unassociated session", async () => {
  freshStore()
  const req = await getRequirementForSession("ses_orphan")
  assert.equal(req.id, DEFAULT_REQ_ID)
  assert.equal(req.title, "默认需求")
})

test("getRequirementTitleForSession: returns title", async () => {
  freshStore()
  await associateSession("REQ-TEST-001", "ses_titled")
  // No Hermes dir for REQ-TEST-001, so falls back to the synthetic
  // default requirement whose title is "默认需求".
  const title = await getRequirementTitleForSession("ses_titled")
  assert.equal(title, "默认需求")
})

test("getAllAssociatedSessionIds: returns correct set", async () => {
  freshStore()
  await associateSession("REQ-A", "ses_1")
  await associateSession("REQ-B", "ses_2")

  const all = await getAllAssociatedSessionIds()
  assert.equal(all.size, 2)
  assert.ok(all.has("ses_1"))
  assert.ok(all.has("ses_2"))
  assert.equal(all.has("ses_3"), false)
})

test("generateSessionId: returns string matching ^ses_[A-Za-z0-9]+$", () => {
  for (let i = 0; i < 20; i++) {
    const id = generateSessionId()
    assert.match(id, /^ses_[A-Za-z0-9]+$/)
    // 24 hex chars after the prefix (12 random bytes hex-encoded).
    assert.equal(id.length, 4 + 24)
  }
})

test("buildInjectionContext: returns minimal context for DEFAULT_REQ_ID", async () => {
  freshStore()
  const ctx = await buildInjectionContext(DEFAULT_REQ_ID)
  assert.match(ctx, /需求：默认需求/)
  assert.match(ctx, /状态：开发中/)
  assert.match(ctx, /请基于以上需求上下文继续。/)
  // DEFAULT_REQ_ID fallback must NOT include the new path-listing /
  // file-modification hints — those are only for real Hermes requirements.
  assert.equal(ctx.includes("需求文件"), false)
  assert.equal(ctx.includes("你可以直接修改上述文件"), false)
})

test("buildInjectionContext: lists file paths and content for a real requirement", async () => {
  freshStore()
  const reqId = "REQ-PATHS-" + randomBytes(4).toString("hex")
  // Legacy flat layout: <reqDir>/<req-id>/meta.md — matches
  // scanHermesRequirements' legacy branch.
  const reqDir = join(
    "/tmp",
    "opencode",
    "test-req-paths-" + randomBytes(6).toString("hex"),
  )
  const reqSubDir = join(reqDir, reqId)
  mkdirSync(reqSubDir, { recursive: true })

  const metaContent =
    "---\n" +
    "title: Path Test Requirement\n" +
    "status: 开发中\n" +
    "---\n" +
    "Path test description."
  const branchContent = "Branch info snippet line one."
  const notesContent = "Notes snippet line one."
  writeFileSync(join(reqSubDir, "meta.md"), metaContent, "utf-8")
  writeFileSync(join(reqSubDir, "branch.md"), branchContent, "utf-8")
  writeFileSync(join(reqSubDir, "notes.md"), notesContent, "utf-8")

  const prevReqDir = _getReqDir()
  _setReqDir(reqDir)
  try {
    const ctx = await buildInjectionContext(reqId)

    // Path-listing section must be present and include all four known
    // files (branch, notes, test, config-changes) by absolute path.
    assert.match(ctx, /需求文件：/)
    assert.ok(ctx.includes(join(reqSubDir, "branch.md")))
    assert.ok(ctx.includes(join(reqSubDir, "notes.md")))
    assert.ok(ctx.includes(join(reqSubDir, "test.md")))
    assert.ok(ctx.includes(join(reqSubDir, "config-changes.md")))

    // Content sections: label includes the path in parentheses, and the
    // file body follows on the next line. Build RegExp from escaped path
    // strings so any future '/' in paths does not break matching.
    const esc = (s: string) => s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")
    assert.match(
      ctx,
      new RegExp(`分支信息（${esc(join(reqSubDir, "branch.md"))}）：\\n${esc(branchContent)}`),
    )
    assert.match(
      ctx,
      new RegExp(`开发笔记（${esc(join(reqSubDir, "notes.md"))}）：\\n${esc(notesContent)}`),
    )
    assert.ok(ctx.includes(branchContent))
    assert.ok(ctx.includes(notesContent))

    // Files we did NOT create (test.md, config-changes.md) still appear
    // in the path listing but have no content section.
    assert.equal(ctx.includes("Test missing snippet"), false)
    assert.equal(ctx.includes("测试范围（"), false)
    assert.equal(ctx.includes("配置变更（"), false)

    // Closing line must invite the injected agent to modify the files.
    assert.match(ctx, /请基于以上需求上下文继续。/)
    assert.match(ctx, /你可以直接修改上述文件来更新需求信息。/)
  } finally {
    _setReqDir(prevReqDir)
  }
})

test("saveAssociations + loadAssociations: round-trip", async () => {
  freshStore()
  const written = {
    version: 2 as const,
    associations: {
      "REQ-X": ["ses_x1", "ses_x2"],
      "REQ-Y": ["ses_y1"],
    },
  }
  await saveAssociations(written)
  const loaded = await loadAssociations()
  assert.equal(loaded.version, 2)
  assert.deepEqual(loaded.associations, written.associations)
})

test("migration: old requirements.json format migrates to associations", async () => {
  // freshStore() picks a brand-new directory; the new store path does
  // NOT exist yet. Drop a legacy `requirements.json` next to it so
  // loadAssociations() finds and migrates it.
  const newPath = freshStore()
  const legacyPath = join(dirname(newPath), "requirements.json")
  const legacy = {
    requirements: [
      { id: "req_old1", sessionIds: ["ses_a", "ses_b"] },
      { id: "req_old2", sessionIds: ["ses_c"] },
      // legacy entry with no sessionIds — should be skipped (sids.length > 0).
      { id: "req_old3" },
    ],
  }
  writeFileSync(legacyPath, JSON.stringify(legacy), "utf-8")

  const store = await loadAssociations()
  assert.equal(store.version, 2)
  assert.deepEqual(store.associations["req_old1"], ["ses_a", "ses_b"])
  assert.deepEqual(store.associations["req_old2"], ["ses_c"])
  assert.equal(store.associations["req_old3"], undefined)

  // After migration, the new associations file should now exist.
  assert.equal(existsSync(newPath), true)
  const onDisk = JSON.parse(readFileSync(newPath, "utf-8"))
  assert.equal(onDisk.version, 2)
  assert.deepEqual(onDisk.associations["req_old1"], ["ses_a", "ses_b"])
})
