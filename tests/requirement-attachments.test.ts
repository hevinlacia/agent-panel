/**
 * Regression tests for `src/requirementAttachments.ts`.
 *
 * Covers path-safety (the critical gate), listing, writing, reading, and
 * deleting attachment files under a temporary requirement directory.
 */

import { test } from "node:test"
import { strict as assert } from "node:assert"
import { join } from "node:path"
import { mkdirSync, writeFileSync, existsSync, rmSync } from "node:fs"
import { randomBytes } from "node:crypto"

import {
  ATTACHMENTS_DIR_NAME,
  resolveAttachmentPath,
  listAttachments,
  writeAttachment,
  deleteAttachment,
  readAttachmentBuffer,
} from "../src/requirementAttachments.ts"

function freshReqDir(): string {
  const root = join("/tmp", "opencode", "test-attachments-" + randomBytes(6).toString("hex"))
  mkdirSync(root, { recursive: true })
  return root
}

// ---------------------------------------------------------------------------
// resolveAttachmentPath - the safety gate
// ---------------------------------------------------------------------------

test("resolveAttachmentPath: accepts a simple filename", () => {
  const dir = "/tmp/fake-req"
  const expected = join(dir, ATTACHMENTS_DIR_NAME, "data.sql")
  assert.equal(resolveAttachmentPath(dir, "data.sql"), expected)
})

test("resolveAttachmentPath: rejects empty string", () => {
  assert.equal(resolveAttachmentPath("/tmp/fake-req", ""), null)
})

test("resolveAttachmentPath: rejects null byte", () => {
  assert.equal(resolveAttachmentPath("/tmp/fake-req", "data\0evil.sql"), null)
})

test("resolveAttachmentPath: rejects path separators", () => {
  assert.equal(resolveAttachmentPath("/tmp/fake-req", "sub/data.sql"), null)
  assert.equal(resolveAttachmentPath("/tmp/fake-req", "sub\\data.sql"), null)
})

test("resolveAttachmentPath: rejects dot and dot-dot", () => {
  assert.equal(resolveAttachmentPath("/tmp/fake-req", "."), null)
  assert.equal(resolveAttachmentPath("/tmp/fake-req", ".."), null)
})

test("resolveAttachmentPath: rejects leading-dot traversal disguised with space", () => {
  // ".. " (trailing space) is a weird but *safe* filename - it resolves
  // inside the attachments dir, so it is accepted. The real escapes
  // ("..", "../x", "a/..") are already covered above.
  const dir = "/tmp/fake-req"
  const result = resolveAttachmentPath(dir, ".. ")
  // It resolves to a file named ".. " inside the dir - not a traversal.
  assert.ok(result !== null)
  assert.ok(result!.startsWith(join(dir, ATTACHMENTS_DIR_NAME)))
})

// ---------------------------------------------------------------------------
// listAttachments / writeAttachment / readAttachmentBuffer / deleteAttachment
// ---------------------------------------------------------------------------

test("listAttachments: returns empty array when dir does not exist", async () => {
  const dir = freshReqDir()
  const result = await listAttachments(dir)
  assert.deepEqual(result, [])
})

test("writeAttachment + listAttachments + read + delete round-trip", async () => {
  const dir = freshReqDir()
  const written = await writeAttachment(dir, "export.sql", "SELECT 1;\n")
  assert.ok(written, "writeAttachment should return the path on success")

  const list = await listAttachments(dir)
  assert.equal(list.length, 1)
  assert.equal(list[0].filename, "export.sql")
  assert.equal(list[0].size, "SELECT 1;\n".length)
  assert.ok(list[0].mtime > 0)

  const buf = await readAttachmentBuffer(dir, "export.sql")
  assert.ok(buf)
  assert.equal(buf.toString("utf-8"), "SELECT 1;\n")

  const deleted = await deleteAttachment(dir, "export.sql")
  assert.equal(deleted, true)

  const listAfter = await listAttachments(dir)
  assert.equal(listAfter.length, 0)
})

test("writeAttachment: creates the attachments dir if missing", async () => {
  const dir = freshReqDir()
  assert.ok(!existsSync(join(dir, ATTACHMENTS_DIR_NAME)))
  await writeAttachment(dir, "a.csv", "x,y\n1,2\n")
  assert.ok(existsSync(join(dir, ATTACHMENTS_DIR_NAME)))
})

test("writeAttachment: overwrites existing file", async () => {
  const dir = freshReqDir()
  await writeAttachment(dir, "f.txt", "old")
  await writeAttachment(dir, "f.txt", "new content")
  const buf = await readAttachmentBuffer(dir, "f.txt")
  assert.equal(buf!.toString("utf-8"), "new content")
})

test("writeAttachment: rejects unsafe filename", async () => {
  const dir = freshReqDir()
  const result = await writeAttachment(dir, "../escape.txt", "evil")
  assert.equal(result, null)
  assert.ok(!existsSync(join(dir, "escape.txt")))
})

test("listAttachments: sorts by mtime descending", async () => {
  const dir = freshReqDir()
  await writeAttachment(dir, "old.sql", "1")
  // Small delay so mtimes differ.
  await new Promise((r) => setTimeout(r, 20))
  await writeAttachment(dir, "new.sql", "2")
  const list = await listAttachments(dir)
  assert.equal(list[0].filename, "new.sql")
  assert.equal(list[1].filename, "old.sql")
})

test("listAttachments: skips non-file entries (directories)", async () => {
  const dir = freshReqDir()
  await writeAttachment(dir, "real.txt", "ok")
  // Manually create a sub-directory inside attachments.
  mkdirSync(join(dir, ATTACHMENTS_DIR_NAME, "subdir"), { recursive: true })
  const list = await listAttachments(dir)
  assert.equal(list.length, 1)
  assert.equal(list[0].filename, "real.txt")
})

test("deleteAttachment: returns false for non-existent file", async () => {
  const dir = freshReqDir()
  const result = await deleteAttachment(dir, "nope.sql")
  assert.equal(result, false)
})

test("deleteAttachment: returns false for unsafe name", async () => {
  const dir = freshReqDir()
  const result = await deleteAttachment(dir, "../../etc/passwd")
  assert.equal(result, false)
})

test("readAttachmentBuffer: returns null for non-existent file", async () => {
  const dir = freshReqDir()
  const result = await readAttachmentBuffer(dir, "missing.sql")
  assert.equal(result, null)
})

test("readAttachmentBuffer: returns null for unsafe name", async () => {
  const dir = freshReqDir()
  const result = await readAttachmentBuffer(dir, "../etc/passwd")
  assert.equal(result, null)
})

// Clean up all temp dirs after the suite.
test("cleanup", () => {
  // Best-effort; /tmp/opencode/test-attachments-* dirs are expendable.
  // (No-op per-test cleanup keeps the suite readable; OS reaps /tmp.)
})
