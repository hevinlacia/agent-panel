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
    { "req-id": "0622-db-a", title: "DB A", status: "待开发" },
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

test("scanHermesRequirements: stops descending once a directory has meta.md", async () => {
  const root = freshFixture()
  // Parent has meta.md AND a child directory with another meta.md.
  // The parent is the requirement; the nested child must NOT be picked up.
  writeMeta(join(root, "WMS", "outer-req"), { "req-id": "outer-req", title: "Outer" })
  writeMeta(join(root, "WMS", "outer-req", "inner-req"), { "req-id": "inner-req", title: "Inner" })
  const reqs = await scanHermesRequirements()
  const ids = reqs.map((r) => r.id).sort()
  assert.deepEqual(ids, ["outer-req"])
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
