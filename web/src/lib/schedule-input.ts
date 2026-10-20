/**
 * Role: 排期输入组装 — 把需求列表 + ONES 已登记工时 + 本机排期配置，组装成排期算法（lib/schedule.ts）需要的
 * ScheduleReqInput 队列与 PanelConfig；排期页与需求详情页（提测推荐展示）共用，保证两处口径一致。
 * Public surface: SCHEDULE_CONFIG_LS_KEY, PanelConfig, DEFAULT_PANEL_CONFIG, readPanelConfig, savePanelConfig,
 *                 reqDisplayId, deriveHours, buildQueueReqs.
 * Constraints: 纯组装、无 fetch；工时优先级 = 人工矫正 > agent 预估 > ONES 剩余（登记口径 × factor 折算真实） > 默认 8h，
 *              全部向上取整到 4h 单位（虚拟工时排期口径）；ONES 折算系数与容量配置存 localStorage。
 * Read-this-with: web/src/lib/schedule.ts（ScheduleReqInput / ScheduleConfig）、web/src/pages/schedule.tsx（排期页消费方）、
 *                 web/src/features/requirements/schedule-info-card.tsx（详情页消费方）。
 */
import type { OnesTasksResponse, Requirement } from "../types"
import { DEFAULT_CONFIG, HOUR_UNIT, SCHEDULE_STATUS_OPTIONS, type HoursSource, type ScheduleConfig, type ScheduleReqInput } from "./schedule"

/** 排期队列门槛：排期页状态选项全集（需求流水 + 子需求「需求创建」+ 挂起）；哪些状态计入由「默认排除状态」决定，完成态默认排除。 */
export const QUEUEABLE_STATUSES = new Set<string>(SCHEDULE_STATUS_OPTIONS)

export type PanelConfig = ScheduleConfig & { onesHoursFactor: number }

export const SCHEDULE_CONFIG_LS_KEY = "react-schedule-config"

/** ONES 登记 10h/天 ≈ 实际 8h/天：登记工时按 0.8 折算（历史验证 657.5h×0.8/63 天 ≈ 8.35h/天）。 */
export const DEFAULT_PANEL_CONFIG: PanelConfig = { ...DEFAULT_CONFIG, onesHoursFactor: 0.8 }

export function readPanelConfig(): PanelConfig {
  try {
    const raw = localStorage.getItem(SCHEDULE_CONFIG_LS_KEY)
    return raw ? ({ ...DEFAULT_PANEL_CONFIG, ...JSON.parse(raw) } as PanelConfig) : DEFAULT_PANEL_CONFIG
  } catch {
    return DEFAULT_PANEL_CONFIG
  }
}

export function savePanelConfig(value: PanelConfig) {
  try {
    localStorage.setItem(SCHEDULE_CONFIG_LS_KEY, JSON.stringify(value))
  } catch {
    /* 存储不可用时静默降级为会话内状态 */
  }
}

/** 与 manhour 页一致：从 ones 字段提取任务编号（后续用大写匹配 ONES 候选）。 */
export function reqDisplayId(req: Requirement): string | null {
  const m = (req.ones || "").match(/[A-Za-z][A-Za-z0-9_]*-\d+/)
  return m ? m[0].toUpperCase() : null
}

/** 向上取整到 4h 单位（保守估计）。 */
function ceilUnit(hours: number): number {
  return Math.max(1, Math.ceil(hours / HOUR_UNIT)) * HOUR_UNIT
}

/** 后端未暴露 start-date 原文，但 created_at 即 start-date 的 ms（UTC 零点，本地 +08 转换同日）；异常时 null。 */
function msToYmd(ms: number): string | null {
  if (!Number.isFinite(ms) || ms <= 0) return null
  const d = new Date(ms)
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`
}

/** 剩余工时推导（虚拟工时排期口径）：人工矫正 > agent 预估 > ONES 剩余(登记口径×factor 折算真实) > 默认 8h。 */
export function deriveHours(
  req: Requirement,
  logged: number | null,
  onesFactor: number,
): { hours: number; source: HoursSource; unevaluated: boolean } {
  const est = req.effortEstimate
  const manualH = est?.manualHours ?? null
  if (manualH != null && manualH > 0) return { hours: ceilUnit(manualH), source: "manual", unevaluated: false }
  const estH = est?.estimatedHours ?? null
  if (estH != null && estH > 0) return { hours: ceilUnit(estH), source: "estimate", unevaluated: false }
  const manualTotal = req.onesManhour?.manualHours ?? null
  if (manualTotal != null && manualTotal > 0) {
    // 登记口径统一折算成真实口径后再相减
    const remain = Math.max(0, manualTotal * onesFactor - (logged ?? 0) * onesFactor)
    if (remain > 0) return { hours: ceilUnit(remain), source: "ones", unevaluated: false }
    return { hours: 0, source: "ones", unevaluated: false }
  }
  return { hours: 2 * HOUR_UNIT, source: "default", unevaluated: true }
}

/** ONES 候选 → 任务编号 → 已登记工时映射（大写编号）。 */
export function loggedHoursMap(ones: OnesTasksResponse | null): Map<string, number> {
  const map = new Map<string, number>()
  for (const c of ones?.candidates || []) {
    if (c.actualHours && c.actualHours > 0) map.set(c.displayId.toUpperCase(), c.actualHours)
  }
  return map
}

/** 组装排期队列：按状态过滤（默认排除状态外全部计入）→ 推导工时 → 归一化提测/投入字段。 */
export function buildQueueReqs(
  reqs: Requirement[],
  loggedMap: Map<string, number>,
  onesFactor: number,
  excludedStatuses: Set<string>,
  queueableStatuses: Set<string>,
): ScheduleReqInput[] {
  return reqs
    .filter((r) => queueableStatuses.has(r.status) && !excludedStatuses.has(r.status))
    .map((r) => {
      const did = reqDisplayId(r)
      const logged = did ? loggedMap.get(did) ?? null : null
      const { hours, source, unevaluated } = deriveHours(r, logged, onesFactor)
      const registered = r.submitTestDate && r.submitTestDate !== "unknown" ? r.submitTestDate : null
      // 未登记提测时间时，按最近一次进入「发布就绪」的日期推定（后端状态历史）；仍无则交给排期模型推荐推算。
      const releaseReady = !registered && r.releaseReadyAt ? msToYmd(r.releaseReadyAt) : null
      const submit = registered ?? releaseReady
      // 紧急度口径：承诺提测时间优先，未设回退实际提测时间（容量/饱和度始终用 submit）
      const promiseRaw = r.promiseSubmitDate && r.promiseSubmitDate !== "unknown" ? r.promiseSubmitDate : null
      const promiseDate = promiseRaw ?? registered
      const submitSource: "registered" | "releaseReady" | undefined = registered ? "registered" : releaseReady ? "releaseReady" : undefined
      return {
        id: r.id,
        title: r.title,
        status: r.status,
        startDate: msToYmd(r.createdAt),
        submitTestDate: submit,
        submitSource,
        promiseDate,
        isHotfix: (r.issues?.length ?? 0) > 0,
        remainingHours: hours,
        hoursSource: source,
        unevaluated,
      }
    })
    .filter((r) => r.remainingHours > 0)
}
