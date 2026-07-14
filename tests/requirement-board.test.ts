/**
 * Regression tests for the flat requirement board filters.
 * Covers default completed hiding, multi-status selection, date ranges,
 * and cascading project/subproject matching without filesystem access.
 */

import { test } from "node:test"
import { strict as assert } from "node:assert"

import { buildRequirementBoardItems, parseRequirementDateBoundary } from "../src/requirementBoard.ts"
import type { Requirement } from "../src/requirements.ts"

function req(overrides: Partial<Requirement> & Pick<Requirement, "id" | "title">): Requirement {
  return {
    id: overrides.id,
    title: overrides.title,
    status: overrides.status ?? "开发中",
    category: overrides.category,
    projects: overrides.projects ?? [overrides.project ?? "WMS"],
    project: overrides.project ?? "WMS",
    groupPath: overrides.groupPath ?? [],
    description: overrides.description ?? "",
    sessionIds: overrides.sessionIds ?? [],
    createdAt: overrides.createdAt ?? new Date("2026-07-01T12:00:00").getTime(),
    updatedAt: overrides.updatedAt ?? new Date("2026-07-02T12:00:00").getTime(),
  }
}

const groups = [
  {
    project: "WMS",
    requirements: [
      req({ id: "dev", title: "Dev", groupPath: ["mq", "consumer"] }),
      req({ id: "test", title: "Test", status: "测试中", groupPath: ["inventory"] }),
      req({ id: "done", title: "Done", status: "已完成" }),
    ],
  },
  {
    project: "OMS",
    requirements: [
      req({ id: "oms", title: "OMS", project: "OMS", createdAt: new Date("2026-06-01T12:00:00").getTime() }),
    ],
  },
]

test("board hides completed requirements when no statuses are selected", () => {
  const items = buildRequirementBoardItems(groups, { statuses: [], project: "", subproject: "" })
  assert.deepEqual(items.map((item) => item.requirement.id).sort(), ["dev", "oms", "test"])
})

test("board supports multiple selected statuses including completed", () => {
  const items = buildRequirementBoardItems(groups, {
    statuses: ["测试中", "已完成"],
    project: "",
    subproject: "",
  })
  assert.deepEqual(items.map((item) => item.requirement.id).sort(), ["done", "test"])
})

test("board filters by first-level project and second-level group", () => {
  const items = buildRequirementBoardItems(groups, {
    statuses: [],
    project: "WMS",
    subproject: "mq",
  })
  assert.deepEqual(items.map((item) => item.requirement.id), ["dev"])
  assert.equal(items[0].hierarchy, "WMS / mq / consumer")
})

test("board filters requirements by any project tag", () => {
  const items = buildRequirementBoardItems(groups, {
    statuses: [],
    project: "WMS RabbitMQ 迁移 RocketMQ",
    subproject: "",
  })
  assert.deepEqual(items.map((item) => item.requirement.id), [])

  const tagged = buildRequirementBoardItems([
    { project: "WMS", requirements: [req({ id: "mq", title: "MQ", projects: ["WMS", "WMS RabbitMQ 迁移 RocketMQ"] })] },
    { project: "WMS RabbitMQ 迁移 RocketMQ", requirements: [req({ id: "mq", title: "MQ", projects: ["WMS", "WMS RabbitMQ 迁移 RocketMQ"] })] },
  ], {
    statuses: [],
    project: "WMS RabbitMQ 迁移 RocketMQ",
    subproject: "",
  })

  assert.deepEqual(tagged.map((item) => item.requirement.id), ["mq"])
  assert.equal(tagged[0].hierarchy, "WMS RabbitMQ 迁移 RocketMQ")
})

test("board filters by inclusive requirement creation dates", () => {
  const items = buildRequirementBoardItems(groups, {
    statuses: [],
    project: "",
    subproject: "",
    createdFrom: parseRequirementDateBoundary("2026-07-01"),
    createdTo: parseRequirementDateBoundary("2026-07-01", true),
  })
  assert.deepEqual(items.map((item) => item.requirement.id).sort(), ["dev", "test"])
})

test("invalid date values are ignored", () => {
  assert.equal(parseRequirementDateBoundary("2026-99-99"), undefined)
  assert.equal(parseRequirementDateBoundary("not-a-date"), undefined)
})

test("board filters by category", () => {
  const catGroups = [
    {
      project: "WMS",
      requirements: [
        req({ id: "normal", title: "Normal Req" }),
        req({ id: "incident", title: "Prod Issue", category: "线上问题" }),
      ],
    },
  ]
  // No category filter: both visible (incident has non-completed status by default).
  const all = buildRequirementBoardItems(catGroups, { statuses: [], project: "", subproject: "" })
  assert.deepEqual(all.map((i) => i.requirement.id).sort(), ["incident", "normal"])

  // Filter to only "线上问题".
  const incidents = buildRequirementBoardItems(catGroups, { statuses: [], project: "", subproject: "", category: "线上问题" })
  assert.deepEqual(incidents.map((i) => i.requirement.id), ["incident"])

  // Filter to only "需求" (default for untagged requirements).
  const normals = buildRequirementBoardItems(catGroups, { statuses: [], project: "", subproject: "", category: "需求" })
  assert.deepEqual(normals.map((i) => i.requirement.id), ["normal"])
})

test("board keyword search matches id, title, description, and project path", () => {
  const kwGroups = [
    {
      project: "WMS",
      requirements: [
        req({ id: "WMS-001", title: "RocketMQ 迁移", description: "把 RabbitMQ 换成 RocketMQ", groupPath: ["disaster-recovery"] }),
        req({ id: "WMS-002", title: "库存盘点", description: "优化盘点流程" }),
      ],
    },
  ]
  // Match by title fragment.
  assert.deepEqual(
    buildRequirementBoardItems(kwGroups, { statuses: [], project: "", subproject: "", keyword: "RocketMQ" }).map((i) => i.requirement.id),
    ["WMS-001"],
  )
  // Match by id fragment.
  assert.deepEqual(
    buildRequirementBoardItems(kwGroups, { statuses: [], project: "", subproject: "", keyword: "wms-002" }).map((i) => i.requirement.id),
    ["WMS-002"],
  )
  // Match by group path.
  assert.deepEqual(
    buildRequirementBoardItems(kwGroups, { statuses: [], project: "", subproject: "", keyword: "disaster" }).map((i) => i.requirement.id),
    ["WMS-001"],
  )
  // No match.
  assert.deepEqual(
    buildRequirementBoardItems(kwGroups, { statuses: [], project: "", subproject: "", keyword: "不存在的关键词" }).map((i) => i.requirement.id),
    [],
  )
})

test("board combines keyword and category filters", () => {
  const comboGroups = [
    {
      project: "WMS",
      requirements: [
        req({ id: "inc-1", title: "订单超卖", category: "线上问题" }),
        req({ id: "inc-2", title: "库存锁定", category: "线上问题" }),
        req({ id: "req-1", title: "订单超卖优化", category: "需求" }),
      ],
    },
  ]
  const items = buildRequirementBoardItems(comboGroups, { statuses: [], project: "", subproject: "", category: "线上问题", keyword: "超卖" })
  assert.deepEqual(items.map((i) => i.requirement.id), ["inc-1"])
})
