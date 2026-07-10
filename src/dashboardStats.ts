/**
 * Role: pure requirement statistics for the control-center dashboard.
 * Public surface: RequirementStats, buildRequirementStats, formatDuration.
 * Constraints: no I/O; all calculations derive from Requirement records + a clock value.
 * Read-this-with: src/requirements.ts and src/server.tsx.
 */

import type { Requirement, ReqStatus } from "./requirements.ts"
import { REQ_STATUSES } from "./requirements.ts"

export interface StatusCount {
  status: ReqStatus
  count: number
  /** Percentage of total (0–100), rounded to one decimal. */
  percent: number
}

export interface RequirementDuration {
  req: Requirement
  /** Milliseconds from creation to last update (completed) or to now (in-progress). */
  durationMs: number
}

export interface RequirementStats {
  total: number
  statusCounts: StatusCount[]
  durations: RequirementDuration[]
  /** Average delivery duration in ms (completed requirements only). */
  avgDeliveryMs: number
  /** Median delivery duration in ms (completed requirements only). */
  medianDeliveryMs: number
  /** Longest delivery duration in ms (completed requirements only). */
  maxDeliveryMs: number
  completedCount: number
  inProgressCount: number
}

/**
 * Format a duration in milliseconds into a compact Chinese string:
 * "3天5小时", "12小时30分钟", "45分钟", "30秒".
 */
export function formatDuration(ms: number): string {
  if (ms < 0) ms = 0
  const sec = Math.floor(ms / 1000)
  if (sec < 60) return `${sec}秒`
  const min = Math.floor(sec / 60)
  if (min < 60) return `${min}分钟`
  const hr = Math.floor(min / 60)
  const remainMin = min % 60
  if (hr < 24) return remainMin > 0 ? `${hr}小时${remainMin}分钟` : `${hr}小时`
  const day = Math.floor(hr / 24)
  const remainHr = hr % 24
  return remainHr > 0 ? `${day}天${remainHr}小时` : `${day}天`
}

function median(sorted: number[]): number {
  if (sorted.length === 0) return 0
  const mid = Math.floor(sorted.length / 2)
  return sorted.length % 2 === 0
    ? Math.round((sorted[mid - 1] + sorted[mid]) / 2)
    : sorted[mid]
}

/**
 * Compute dashboard statistics from a flat list of requirements.
 * The synthetic default requirement (__default__) is excluded.
 * `now` defaults to Date.now() but is injectable for tests.
 */
export function buildRequirementStats(
  requirements: Requirement[],
  now: number = Date.now(),
): RequirementStats {
  const real = requirements.filter((r) => r.id !== "__default__")
  const total = real.length

  const statusCounts = (REQ_STATUSES as readonly ReqStatus[]).map((status) => {
    const count = real.filter((r) => r.status === status).length
    return {
      status,
      count,
      percent: total > 0 ? Math.round((count / total) * 1000) / 10 : 0,
    }
  })

  const durations = real
    .map((req) => {
      const end = req.status === "已完成" ? req.updatedAt : now
      return { req, durationMs: Math.max(0, end - req.createdAt) }
    })
    .sort((a, b) => b.durationMs - a.durationMs)

  const completed = durations.filter((d) => d.req.status === "已完成")
  const completedDurations = completed.map((d) => d.durationMs).sort((a, b) => a - b)
  const avg = completedDurations.length > 0
    ? Math.round(completedDurations.reduce((s, v) => s + v, 0) / completedDurations.length)
    : 0

  return {
    total,
    statusCounts,
    durations,
    avgDeliveryMs: avg,
    medianDeliveryMs: median(completedDurations),
    maxDeliveryMs: completedDurations.length > 0 ? completedDurations[completedDurations.length - 1] : 0,
    completedCount: completed.length,
    inProgressCount: total - completed.length,
  }
}
