/**
 * Recursive scanner regression tests.
 *
 * The production `~/.agents/req/` tree can be 3+ levels deep:
 *   ~/.agents/req/<project>/<sub-project>/<sub-module>/<req-id>/meta.md
 * Intermediate directories without `meta.md` are *grouping* directories;
 * their segment names are recorded in `groupPath`.
 *
 * These tests build a temp fixture under /tmp/opencode/test-req-scan-*
 * and point both the requirements store and the scan root at it via
 * `_setStorePath` / `_setReqDir`.
 */

import { test } from "node:test"
import { strict as assert } from "node:assert"
import { join } from "node:path"
import { mkdirSync, writeFileSync } from "node:fs"
import { randomBytes } from "node:crypto"

import {
  _setReqDir,
  _setStorePath,
  scanHermesRequirements,
  listRequirementsByProject,
  replaceAssociatedSession,
  associateSession,
  loadAssociations,
  DEFAULT_REQ_ID,
  DEFAULT_PROJECT_NAME,
} from "../src/requirements.ts"

function freshFixture(): string {
  const root = join("/tmp", "opencode", "test-req-scan-" + randomBytes(6).toString("hex"))
  mkdirSync(root, { recursive: true })
  _setReqDir(root)
  // Point the associations store at the same temp area so the fixture
  // is fully self-contained and never touches the real user store.
  _setStorePath(join(root, "associations.json"))
  return root
}

function writeMeta(dir: string, fields: Record<string, string>, body = ""): void {
  mkdirSync(dir, { recursive: true })
  const fm = ["---", ...Object.entries(fields).map(([k, v]) => `${k}: ${v}`), "---", body].join("\n")
  writeFileSync(join(dir, "meta.md"), fm, "utf-8")
}

test("scanHermesRequirements: 2-level layout — req under <project>/<req-id>", async () => {
  const root = freshFixture()
  writeMeta(join(root, "WMS", "0622-foo"), { "req-id": "0622-foo", title: "Foo", status: "开发中" })
  const reqs = await scanHermesRequirements()
  assert.equal(reqs.length, 1)
  const r = reqs[0]
  assert.equal(r.id, "0622-foo")
  assert.equal(r.project, "WMS")
  assert.deepEqual(r.groupPath, [])
  assert.equal(r.title, "Foo")
})

test("scanHermesRequirements: 4-level layout — sub-project + sub-module groups", async () => {
  const root = freshFixture()
  // WMS/disaster-recovery/mq-migration/<req-id>/meta.md
  writeMeta(
    join(root, "WMS", "disaster-recovery", "mq-migration", "0622-mq-a"),
    { "req-id": "0622-mq-a", title: "MQ A", status: "开发中" },
  )
  writeMeta(
    join(root, "WMS", "disaster-recovery", "mq-migration", "0622-mq-b"),
    { "req-id": "0622-mq-b", title: "MQ B", status: "测试中" },
  )
  // A sibling sub-project also under WMS, to exercise multiple group keys.
  writeMeta(
    join(root, "WMS", "disaster-recovery", "db-failover", "0622-db-a"),
    { "req-id": "0622-db-a", title: "DB A", status: "方案设计" },
  )

  const reqs = await scanHermesRequirements()
  const byId = Object.fromEntries(reqs.map((r) => [r.id, r]))

  assert.ok(byId["0622-mq-a"], "expected 0622-mq-a in scan output")
  assert.deepEqual(byId["0622-mq-a"].groupPath, ["disaster-recovery", "mq-migration"])
  assert.equal(byId["0622-mq-a"].project, "WMS")

  assert.ok(byId["0622-mq-b"], "expected 0622-mq-b in scan output")
  assert.deepEqual(byId["0622-mq-b"].groupPath, ["disaster-recovery", "mq-migration"])

  assert.ok(byId["0622-db-a"], "expected 0622-db-a in scan output")
  assert.deepEqual(byId["0622-db-a"].groupPath, ["disaster-recovery", "db-failover"])
})

test("scanHermesRequirements: container requirement becomes a project tag", async () => {
  const root = freshFixture()
  // A directory with meta.md AND nested requirement dirs is a project tag
  // container, not a requirement record. Descendants inherit its title.
  writeMeta(join(root, "WMS", "outer-req"), { "req-id": "outer-req", title: "Outer" })
  writeMeta(join(root, "WMS", "outer-req", "inner-req"), { "req-id": "inner-req", title: "Inner" })
  const reqs = await scanHermesRequirements()
  const ids = reqs.map((r) => r.id).sort()
  assert.deepEqual(ids, ["inner-req"])
  const inner = reqs.find((r) => r.id === "inner-req")!
  assert.deepEqual(inner.projects, ["WMS", "Outer"])
})

test("scanHermesRequirements: flat legacy layout — <req-id>/meta.md", async () => {
  const root = freshFixture()
  writeMeta(join(root, "legacy-flat"), { "req-id": "legacy-flat", title: "Legacy", status: "开发中" })
  const reqs = await scanHermesRequirements()
  assert.equal(reqs.length, 1)
  assert.equal(reqs[0].id, "legacy-flat")
  assert.deepEqual(reqs[0].groupPath, [])
  // Flat layout: no project frontmatter means DEFAULT project name.
  assert.equal(typeof reqs[0].project, "string")
})

test("scanHermesRequirements: flat requirement with frontmatter project is not tagged 默认项目", async () => {
  const root = freshFixture()
  writeMeta(join(root, "flat-wms"), { "req-id": "flat-wms", title: "Flat WMS", status: "开发中", project: "WMS" })
  const reqs = await scanHermesRequirements()
  const r = reqs.find((x) => x.id === "flat-wms")!
  assert.ok(r, "expected flat-wms in scan output")
  // Explicit frontmatter project wins; the DEFAULT_PROJECT_NAME fallback
  // must NOT be injected as a second project tag (would duplicate the
  // requirement across WMS and 默认项目 board groups).
  assert.deepEqual(r.projects, ["WMS"])
  assert.equal(r.project, "WMS")
  const groups = await listRequirementsByProject()
  assert.equal(groups.some((g) => g.project === DEFAULT_PROJECT_NAME), false)
})

test("scanHermesRequirements: project.json assigns flat requirement to a project", async () => {
  const root = freshFixture()
  const dir = join(root, "standalone-req")
  writeMeta(dir, { "req-id": "standalone-req", title: "Standalone", status: "开发中" })
  writeFileSync(
    join(dir, "project.json"),
    JSON.stringify({ project: "WMS", groupPath: ["mq", "consumer"] }, null, 2),
    "utf-8",
  )

  const reqs = await scanHermesRequirements()
  assert.equal(reqs.length, 1)
  assert.equal(reqs[0].project, "WMS")
  assert.deepEqual(reqs[0].groupPath, ["mq", "consumer"])
})

test("listRequirementsByProject: groups flat project.json requirements under their project", async () => {
  const root = freshFixture()
  const dir = join(root, "flat-wms-req")
  writeMeta(dir, { "req-id": "flat-wms-req", title: "Flat WMS", status: "开发中" })
  writeFileSync(join(dir, "project.json"), JSON.stringify({ project: "WMS" }), "utf-8")

  const groups = await listRequirementsByProject()
  const wms = groups.find((g) => g.project === "WMS")
  assert.ok(wms, "expected WMS project group")
  assert.deepEqual(wms!.requirements.map((r) => r.id), ["flat-wms-req"])
})

test("listRequirementsByProject: groups nested requirements under their project", async () => {
  const root = freshFixture()
  writeMeta(
    join(root, "WMS", "disaster-recovery", "mq-migration", "0622-mq-x"),
    { "req-id": "0622-mq-x", title: "MQ X", status: "开发中" },
  )
  writeMeta(
    join(root, "WMS", "0622-flat-under-wms"),
    { "req-id": "0622-flat-under-wms", title: "Flat under WMS", status: "开发中" },
  )
  const groups = await listRequirementsByProject()
  const wms = groups.find((g) => g.project === "WMS")
  assert.ok(wms, "expected WMS project group")
  const ids = wms!.requirements.map((r) => r.id).sort()
  assert.deepEqual(ids, ["0622-flat-under-wms", "0622-mq-x"])
})

test("listRequirementsByProject: excludes the synthetic default requirement", async () => {
  const root = freshFixture()
  // A real requirement under WMS so the list is non-empty.
  writeMeta(join(root, "WMS", "real-req"), { "req-id": "real-req", title: "Real", status: "开发中" })
  // An orphan association (reqId has no Hermes dir) plus a session tied
  // directly to DEFAULT_REQ_ID - both would previously be folded into the
  // synthetic default requirement.
  await associateSession("orphan-req", "ses_orphan")
  await associateSession(DEFAULT_REQ_ID, "ses_default")

  const groups = await listRequirementsByProject()
  const allIds = groups.flatMap((g) => g.requirements.map((r) => r.id))
  // The synthetic default requirement must NOT appear in the board.
  assert.equal(allIds.includes(DEFAULT_REQ_ID), false)
  // The synthetic default project must NOT appear either (no real req
  // carries it), even though orphan/default associations exist in the store.
  assert.equal(groups.some((g) => g.project === DEFAULT_PROJECT_NAME), false)
  // The real requirement is still listed under WMS.
  const wms = groups.find((g) => g.project === "WMS")
  assert.ok(wms, "expected WMS project group")
  assert.deepEqual(wms!.requirements.map((r) => r.id), ["real-req"])
})

test("replaceAssociatedSession: swaps placeholder id for the real one", async () => {
  freshFixture()
  await associateSession("0622-mq-a", "ses_placeholder")
  await replaceAssociatedSession("0622-mq-a", "ses_placeholder", "ses_real")
  const store = await loadAssociations()
  const sids = store.associations["0622-mq-a"] ?? []
  assert.deepEqual(sids, ["ses_real"])
})

test("replaceAssociatedSession: detaches the real id from any other requirement first", async () => {
  freshFixture()
  await associateSession("REQ-OLD", "ses_real")
  await associateSession("REQ-NEW", "ses_placeholder")
  await replaceAssociatedSession("REQ-NEW", "ses_placeholder", "ses_real")
  const store = await loadAssociations()
  // REQ-OLD no longer owns ses_real.
  assert.equal((store.associations["REQ-OLD"] ?? []).includes("ses_real"), false)
  // REQ-NEW owns exactly the real id.
  assert.deepEqual(store.associations["REQ-NEW"], ["ses_real"])
})

test("scanHermesRequirements: reads category from meta.md frontmatter", async () => {
  const root = freshFixture()
  writeMeta(join(root, "WMS", "0622-incident"), {
    "req-id": "0622-incident",
    title: "Prod Order Issue",
    status: "开发中",
    category: "线上问题",
  })
  writeMeta(join(root, "WMS", "0622-normal"), {
    "req-id": "0622-normal",
    title: "Normal Feature",
    status: "开发中",
  })
  const reqs = await scanHermesRequirements()
  const incident = reqs.find((r) => r.id === "0622-incident")
  const normal = reqs.find((r) => r.id === "0622-normal")
  assert.equal(incident!.category, "线上问题")
  assert.equal(normal!.category, "需求")
})

test("scanHermesRequirements: state.json category overrides frontmatter", async () => {
  const root = freshFixture()
  const reqDir = join(root, "WMS", "0622-cat-override")
  writeMeta(reqDir, { "req-id": "0622-cat-override", title: "Cat Override", status: "开发中", category: "需求" })
  // state.json with category 线上问题 should win over frontmatter.
  writeFileSync(
    join(reqDir, "state.json"),
    JSON.stringify({ version: 1, status: "开发中", category: "线上问题", updatedAt: Date.now(), history: [] }) + "\n",
    "utf-8",
  )
  const reqs = await scanHermesRequirements()
  const r = reqs.find((x) => x.id === "0622-cat-override")
  assert.equal(r!.category, "线上问题")
})
