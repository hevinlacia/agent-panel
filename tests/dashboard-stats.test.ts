/**
 * Regression tests for dashboard statistics calculations.
 * Covers status counts, duration sorting, and aggregate metrics.
 */

import { test } from "node:test"
import { strict as assert } from "node:assert"

import { buildRequirementStats, formatDuration } from "../src/dashboardStats.ts"
import type { Requirement } from "../src/requirements.ts"

function req(overrides: Partial<Requirement> & Pick<Requirement, "id" | "title">): Requirement {
  return {
    id: overrides.id,
    title: overrides.title,
    status: overrides.status ?? "开发中",
    projects: overrides.projects ?? [overrides.project ?? "WMS"],
    project: overrides.project ?? "WMS",
    groupPath: overrides.groupPath ?? [],
    description: overrides.description ?? "",
    sessionIds: overrides.sessionIds ?? [],
    createdAt: overrides.createdAt ?? new Date("2026-07-01T00:00:00").getTime(),
    updatedAt: overrides.updatedAt ?? new Date("2026-07-05T00:00:00").getTime(),
  }
}

const NOW = new Date("2026-07-10T00:00:00").getTime()
const DAY = 24 * 60 * 60 * 1000

test("formatDuration: formats days, hours, minutes, seconds", () => {
  assert.equal(formatDuration(30_000), "30秒")
  assert.equal(formatDuration(5 * 60_000), "5分钟")
  assert.equal(formatDuration(90 * 60_000), "1小时30分钟")
  assert.equal(formatDuration(2 * 60 * 60_000), "2小时")
  assert.equal(formatDuration(3 * DAY + 5 * 60 * 60_000), "3天5小时")
  assert.equal(formatDuration(7 * DAY), "7天")
  assert.equal(formatDuration(0), "0秒")
  assert.equal(formatDuration(-100), "0秒")
})

test("buildRequirementStats: counts statuses and percentages", () => {
  const stats = buildRequirementStats([
    req({ id: "a", title: "A", status: "开发中" }),
    req({ id: "b", title: "B", status: "开发中" }),
    req({ id: "c", title: "C", status: "已完成" }),
  ], NOW)
  assert.equal(stats.total, 3)
  const dev = stats.statusCounts.find((s) => s.status === "开发中")!
  assert.equal(dev.count, 2)
  assert.equal(dev.percent, 66.7)
  const done = stats.statusCounts.find((s) => s.status === "已完成")!
  assert.equal(done.count, 1)
  assert.equal(done.percent, 33.3)
})

test("buildRequirementStats: durations sorted descending", () => {
  const stats = buildRequirementStats([
    req({ id: "a", title: "A", status: "已完成", createdAt: NOW - 2 * DAY, updatedAt: NOW }),
    req({ id: "b", title: "B", status: "开发中", createdAt: NOW - 1 * DAY, updatedAt: NOW }),
  ], NOW)
  assert.equal(stats.durations[0].req.id, "a")
  assert.equal(stats.durations[1].req.id, "b")
  assert.equal(stats.durations[0].durationMs, 2 * DAY)
  assert.equal(stats.durations[1].durationMs, 1 * DAY)
})

test("buildRequirementStats: aggregate metrics only from completed", () => {
  const stats = buildRequirementStats([
    req({ id: "a", title: "A", status: "已完成", createdAt: NOW - 4 * DAY, updatedAt: NOW - 2 * DAY }),
    req({ id: "b", title: "B", status: "已完成", createdAt: NOW - 3 * DAY, updatedAt: NOW - 1 * DAY }),
    req({ id: "c", title: "C", status: "开发中", createdAt: NOW - 1 * DAY, updatedAt: NOW }),
  ], NOW)
  assert.equal(stats.completedCount, 2)
  assert.equal(stats.inProgressCount, 1)
  assert.equal(stats.avgDeliveryMs, 2 * DAY)
  assert.equal(stats.medianDeliveryMs, 2 * DAY)
  assert.equal(stats.maxDeliveryMs, 2 * DAY)
})

test("buildRequirementStats: excludes synthetic default requirement", () => {
  const stats = buildRequirementStats([
    req({ id: "__default__", title: "Default" }),
    req({ id: "real", title: "Real" }),
  ], NOW)
  assert.equal(stats.total, 1)
})

test("buildRequirementStats: counts each multi-project requirement once", () => {
  const stats = buildRequirementStats([
    req({ id: "child-a", title: "Child A", status: "测试中", projects: ["WMS", "WMS RabbitMQ 迁移 RocketMQ"] }),
  ], NOW)

  assert.equal(stats.total, 1)
  assert.equal(stats.statusCounts.find((s) => s.status === "测试中")!.count, 1)
  assert.deepEqual(stats.durations.map((d) => d.req.id), ["child-a"])
})

test("buildRequirementStats: empty input returns zeros", () => {
  const stats = buildRequirementStats([], NOW)
  assert.equal(stats.total, 0)
  assert.equal(stats.avgDeliveryMs, 0)
  assert.equal(stats.medianDeliveryMs, 0)
  assert.equal(stats.maxDeliveryMs, 0)
  assert.equal(stats.statusCounts.length, 7)
  assert.equal(stats.statusCounts.every((s) => s.count === 0), true)
})
