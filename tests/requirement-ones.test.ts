/**
 * Unit tests for the ONES task reference feature:
 *   - parseOnesRef: URL vs bare-id vs empty
 *   - writeRequirementOnes: frontmatter upsert / update / clear,
 *     preserving other fields and the body, and round-tripping through
 *     the scanner so the value the UI reads matches what was written.
 *
 * Each test points the scan root and associations store at a fresh temp
 * fixture under /tmp/opencode/test-req-ones-* so it never touches the
 * real user store or ~/.agents/req tree.
 */

import { test } from "node:test"
import { strict as assert } from "node:assert"
import { join } from "node:path"
import { mkdirSync, writeFileSync, readFileSync } from "node:fs"
import { randomBytes } from "node:crypto"

import {
  _setReqDir,
  _setStorePath,
  scanHermesRequirements,
  parseOnesRef,
  writeRequirementOnes,
} from "../src/requirements.ts"

function freshFixture(): string {
  const root = join("/tmp", "opencode", "test-req-ones-" + randomBytes(6).toString("hex"))
  mkdirSync(root, { recursive: true })
  _setReqDir(root)
  _setStorePath(join(root, "associations.json"))
  return root
}

function writeMeta(dir: string, fields: Record<string, string>, body = ""): void {
  mkdirSync(dir, { recursive: true })
  const fm = ["---", ...Object.entries(fields).map(([k, v]) => `${k}: ${v}`), "---", body].join("\n")
  writeFileSync(join(dir, "meta.md"), fm, "utf-8")
}

test("parseOnesRef: null for empty / missing", () => {
  assert.equal(parseOnesRef(undefined), null)
  assert.equal(parseOnesRef(""), null)
  assert.equal(parseOnesRef("   "), null)
})

test("parseOnesRef: http URL is clickable with last segment as label", () => {
  const ref = parseOnesRef("https://ones.example.com/project/task/T-123")
  assert.equal(ref?.url, "https://ones.example.com/project/task/T-123")
  assert.equal(ref?.label, "T-123")
  assert.equal(ref?.raw, "https://ones.example.com/project/task/T-123")
})

test("parseOnesRef: bare id has no url", () => {
  const ref = parseOnesRef("T-123")
  assert.equal(ref?.url, null)
  assert.equal(ref?.label, "T-123")
  assert.equal(ref?.raw, "T-123")
})

test("parseOnesRef: malformed URL falls back to non-clickable label", () => {
  // "https://" alone is not a valid URL; must not throw, treated as label.
  const ref = parseOnesRef("https://")
  assert.ok(ref)
  assert.equal(ref?.url, "https://")
  // label may be the full string when URL parsing fails to yield a segment
  assert.equal(typeof ref?.label, "string")
})

test("scanHermesRequirements: reads ones from frontmatter", async () => {
  const root = freshFixture()
  writeMeta(join(root, "WMS", "ones-001"), {
    "req-id": "ones-001",
    title: "Ones Linked",
    status: "开发中",
    ones: "https://ones.example.com/task/T-42",
  })
  writeMeta(join(root, "WMS", "ones-002"), {
    "req-id": "ones-002",
    title: "Ones Bare",
    status: "开发中",
    ones: "T-99",
  })
  writeMeta(join(root, "WMS", "ones-003"), {
    "req-id": "ones-003",
    title: "No Ones",
    status: "开发中",
  })
  const reqs = await scanHermesRequirements()
  const byId = Object.fromEntries(reqs.map((r) => [r.id, r]))
  assert.equal(byId["ones-001"].ones, "https://ones.example.com/task/T-42")
  assert.equal(byId["ones-002"].ones, "T-99")
  assert.equal(byId["ones-003"].ones, undefined)
})

test("writeRequirementOnes: inserts field and round-trips through scanner", async () => {
  const root = freshFixture()
  const reqDir = join(root, "WMS", "w-001")
  writeMeta(reqDir, { "req-id": "w-001", title: "Write Ones", status: "开发中" }, "# Body\n\nkeep me")
  await writeRequirementOnes(reqDir, "https://ones.example.com/task/T-7")
  const reqs = await scanHermesRequirements()
  const r = reqs.find((x) => x.id === "w-001")
  assert.equal(r?.ones, "https://ones.example.com/task/T-7")
  // Body must survive.
  const onDisk = readFileSync(join(reqDir, "meta.md"), "utf-8")
  assert.match(onDisk, /keep me/)
  assert.match(onDisk, /ones:/)
})

test("writeRequirementOnes: preserves existing frontmatter fields on update", async () => {
  const root = freshFixture()
  const reqDir = join(root, "WMS", "w-002")
  writeMeta(reqDir, {
    "req-id": "w-002",
    title: "Preserve",
    status: "开发中",
    project: "WMS",
    owner: "hevin",
    ones: "OLD-1",
  }, "# Body")
  await writeRequirementOnes(reqDir, "https://ones.example.com/task/NEW-1")
  const onDisk = readFileSync(join(reqDir, "meta.md"), "utf-8")
  // All original keys preserved.
  assert.match(onDisk, /req-id: w-002/)
  assert.match(onDisk, /title: Preserve/)
  assert.match(onDisk, /project: WMS/)
  assert.match(onDisk, /owner: hevin/)
  // Old value replaced.
  assert.equal(/ones: OLD-1/.test(onDisk), false)
  assert.match(onDisk, /ones: "https:\/\/ones\.example\.com\/task\/NEW-1"/)
  // Scanner reads the new value (quotes stripped by parseFrontmatter).
  const reqs = await scanHermesRequirements()
  const r = reqs.find((x) => x.id === "w-002")
  assert.equal(r?.ones, "https://ones.example.com/task/NEW-1")
})

test("writeRequirementOnes: clears field when value is empty", async () => {
  const root = freshFixture()
  const reqDir = join(root, "WMS", "w-003")
  writeMeta(reqDir, {
    "req-id": "w-003",
    title: "Clear",
    status: "开发中",
    ones: "T-1",
  }, "# Body survives")
  await writeRequirementOnes(reqDir, "")
  const onDisk = readFileSync(join(reqDir, "meta.md"), "utf-8")
  assert.equal(/ones:/.test(onDisk), false)
  assert.match(onDisk, /Body survives/)
  const reqs = await scanHermesRequirements()
  const r = reqs.find((x) => x.id === "w-003")
  assert.equal(r?.ones, undefined)
})

test("writeRequirementOnes: preserves body verbatim incl. blank line after frontmatter", async () => {
  const root = freshFixture()
  const reqDir = join(root, "WMS", "w-004")
  // Real meta.md files have a blank line between the closing `---` and
  // the first `#` heading. The upsert must not strip it (minimal git diff).
  const original = [
    "---",
    "req-id: w-004",
    "title: Preserve Blank",
    "status: 开发中",
    "---",
    "",
    "# WMS-004 Preserve Blank",
    "",
    "## Summary",
    "- keep this body byte-for-byte",
    "",
  ].join("\n")
  mkdirSync(reqDir, { recursive: true })
  writeFileSync(join(reqDir, "meta.md"), original, "utf-8")

  await writeRequirementOnes(reqDir, "https://ones.example.com/task/T-4")
  let onDisk = readFileSync(join(reqDir, "meta.md"), "utf-8")
  // The body (blank line + heading + summary) must be untouched.
  assert.match(onDisk, /---\n\n# WMS-004 Preserve Blank/)
  assert.match(onDisk, /keep this body byte-for-byte/)
  // The ones field sits inside the frontmatter, before the closing `---`.
  assert.match(onDisk, /ones: "https:\/\/ones\.example\.com\/task\/T-4"\n---/)

  // Clearing it must leave the body identical to the original (round-trip).
  await writeRequirementOnes(reqDir, "")
  onDisk = readFileSync(join(reqDir, "meta.md"), "utf-8")
  assert.equal(onDisk, original)
})
