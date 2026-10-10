/**
 * Role: 需求排期算法（纯函数）— 并行均摊模型：需求的排期区间 = 投入日 → 提测日，剩余工时均摊到区间内
 * 每个工作日，得到每日工作饱和度；据此评估"能否接新需求 / 能接多大需求"。
 * Public surface: HOUR_UNIT, utilization, checkAcceptance, maxAcceptable, workdaysBetweenInclusive, addWorkdays,
 *                 ScheduleConfig, UtilResult, UtilItem, UtilDay, AcceptCheck, MaxAccept, ScheduleReqInput.
 * Constraints: 无副作用、无 fetch；工时粒度 = 4h 单位（remainingHours 已在页面层向上取整）；
 *              队列只收「开发工作未完成」的需求（状态 < 人工核查门槛）；跳过周末可配。
 *              口径：所有工时均为真实工时（ONES 登记口径已在页面层按折算系数换算）。
 * Read-this-with: web/src/lib/requirements.ts（REQ_FLOW_STATUSES 状态序）、web/src/pages/schedule.tsx（数据组装）。
 */

/** 工时最小单位：4h（真实工时口径）。页面层录入/推导的剩余工时都取整到该单位。 */
export const HOUR_UNIT = 4
/** 未评估需求的默认剩余工时：2 单位 = 8h。 */
export const DEFAULT_HOURS = 2 * HOUR_UNIT

export interface ScheduleConfig {
  /** 每日有效开发小时（真实口径，默认 6：8h 里扣掉会议/线上支持/碎片）。 */
  dailyHours: number
  /** 同时推进需求数告警上限（默认 3）。 */
  parallelMax: number
  /** 跳过周六周日（默认 true）。 */
  skipWeekends: boolean
  /** 未登记提测时间的需求，假设每天投入该小时数推进（默认 4h）→ 虚拟提测日 = 今天起第 ceil(hours/该值) 个工作日。 */
  defaultDailyPerReq: number
}

export const DEFAULT_CONFIG: ScheduleConfig = {
  dailyHours: 6,
  parallelMax: 3,
  skipWeekends: true,
  defaultDailyPerReq: 4,
}

export type HoursSource = "manual" | "ones" | "estimate" | "default"

export interface ScheduleReqInput {
  id: string
  title: string
  status: string
  /** meta.md start-date（未登记为 null）。 */
  startDate: string | null
  /** 提测时间 deadline（unknown/未登记为 null）。 */
  submitTestDate: string | null
  /** 绑定线上问题 = 抢修，展示红旗（均摊模型下不加权，由用户判断挤占）。 */
  isHotfix: boolean
  /** 剩余工时（页面层已向上取整到 4h 倍数，>= HOUR_UNIT；已滤除 0）。 */
  remainingHours: number
  hoursSource: HoursSource
  /** 工时来自默认值（无任何评估/录入），排期可信度低，页面标红提醒。 */
  unevaluated?: boolean
}

export interface UtilItem extends ScheduleReqInput {
  /** 有效提测日（实际或虚拟）。 */
  effDeadline: string
  /** 提测日为虚拟推算（未登记提测时间）。 */
  virtualDeadline: boolean
  /** 提测日早于今天（逾期未清）。 */
  overdue: boolean
  /** 投入日（今天与 startDate 的较晚者，ymd）。 */
  startPoint: string
  /** 投入日 → 提测日的工作日数（含两端）；逾期 = 0。 */
  spanDays: number
  /** 每日摊派（真实小时）= remainingHours / max(spanDays, 1)；逾期需求全额压今天。 */
  dailyLoad: number
}

/** 饱和度视图默认窗口（工作日数）；util.days 序列至少覆盖该窗口。 */
export const DEFAULT_VIEW_DAYS = 14

export interface UtilDay {
  date: string
  /** 当日总摊派（真实小时）——容量占用账，不锁定当天具体做哪个需求（实际安排弹性）。 */
  load: number
  capacity: number
  /** 饱和度 = load / capacity。 */
  saturation: number
  /** 当日负载构成（均摊口径的贡献来源），仅用于 tooltip 展示，不是日程。 */
  items: { id: string; hours: number }[]
}

export interface UtilResult {
  items: UtilItem[]
  /** 未来工作日序列（覆盖到最远提测日，上限 60 个），按日期升序。 */
  days: UtilDay[]
  totalHours: number
  totalUnits: number
  activeCount: number
  unevaluatedCount: number
  /** 今日饱和度（0~∞）。 */
  todaySaturation: number
  /** 序列内峰值饱和度与日期。 */
  peakSaturation: number
  peakDate: string | null
  /** 序列内超载（saturation > 1）的工作日数。 */
  overloadDays: number
}

export function ymd(d: Date): string {
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`
}

function parseYmd(s: string): Date {
  return new Date(`${s}T00:00:00`)
}

function isWorkday(d: Date, skipWeekends: boolean): boolean {
  if (!skipWeekends) return true
  const day = d.getDay()
  return day !== 0 && day !== 6
}

/** 从 from（含）起取 count 个工作日，返回 ymd 序列。 */
export function workdayList(from: Date, count: number, skipWeekends: boolean): string[] {
  const out: string[] = []
  const cursor = new Date(from.getFullYear(), from.getMonth(), from.getDate())
  while (out.length < count) {
    if (isWorkday(cursor, skipWeekends)) out.push(ymd(cursor))
    cursor.setDate(cursor.getDate() + 1)
  }
  return out
}

/** [a, b] 闭区间内的工作日数；b < a 时返回 0。 */
export function workdaysBetweenInclusive(a: string, b: string, skipWeekends: boolean): number {
  if (b < a) return 0
  let n = 0
  const cursor = parseYmd(a)
  const end = parseYmd(b)
  while (cursor <= end) {
    if (isWorkday(cursor, skipWeekends)) n += 1
    cursor.setDate(cursor.getDate() + 1)
  }
  return n
}

/** 从 from（含）起第 n 个工作日的 ymd（n >= 1）。 */
export function addWorkdays(from: Date, n: number, skipWeekends: boolean): string {
  return workdayList(from, Math.max(1, n), skipWeekends)[Math.max(1, n) - 1]
}

/**
 * 并行均摊：每个未完成需求的剩余工时均摊到 [投入日, 提测日] 的每个工作日；
 * 未登记提测时间的按 defaultDailyPerReq 推进推算虚拟提测日；
 * 逾期（提测日 < 今天）的需求剩余工时全额压今天。
 * 注意：均摊是容量占用账（回答“这天还剩多少容量”），不锁定每天具体做哪个需求。
 */
export function utilization(reqs: ScheduleReqInput[], config: ScheduleConfig, today = new Date()): UtilResult {
  const todayStr = ymd(today)
  const items: UtilItem[] = []
  for (const r of reqs) {
    if (r.remainingHours <= 0) continue
    const startPoint = r.startDate && r.startDate > todayStr ? r.startDate : todayStr
    let effDeadline = r.submitTestDate ?? ""
    let virtualDeadline = false
    if (!effDeadline) {
      const span = Math.max(1, Math.ceil(r.remainingHours / Math.max(0.5, config.defaultDailyPerReq)))
      effDeadline = addWorkdays(today, span, config.skipWeekends)
      virtualDeadline = true
    }
    const overdue = effDeadline < todayStr
    const spanDays = overdue ? 0 : workdaysBetweenInclusive(startPoint, effDeadline, config.skipWeekends)
    const dailyLoad = overdue ? r.remainingHours : r.remainingHours / Math.max(1, spanDays)
    items.push({
      ...r,
      effDeadline,
      virtualDeadline,
      overdue,
      startPoint,
      spanDays,
      dailyLoad,
    })
  }
  // 展示排序：逾期最前 → 提测日升序 → 工时降序 → id 稳定。
  items.sort((a, b) => {
    if (a.overdue !== b.overdue) return a.overdue ? -1 : 1
    if (a.effDeadline !== b.effDeadline) return a.effDeadline < b.effDeadline ? -1 : 1
    if (a.remainingHours !== b.remainingHours) return b.remainingHours - a.remainingHours
    return a.id < b.id ? -1 : a.id > b.id ? 1 : 0
  })

  // 工作日序列：覆盖到最远提测日与默认视图窗口（14 个工作日）的较大者（上限 60），首日 = 今天。
  const lastDeadline = items.reduce((m, it) => (it.effDeadline > m ? it.effDeadline : m), todayStr)
  const horizon = Math.min(
    60,
    Math.max(
      workdaysBetweenInclusive(todayStr, lastDeadline > todayStr ? lastDeadline : todayStr, config.skipWeekends),
      DEFAULT_VIEW_DAYS,
    ),
  )
  const dates = workdayList(today, horizon, config.skipWeekends)
  const capacity = Math.max(0.5, config.dailyHours)
  const days: UtilDay[] = dates.map((date) => ({ date, load: 0, capacity, saturation: 0, items: [] }))
  const dateIndex = new Map(dates.map((d, i) => [d, i]))

  for (const it of items) {
    if (it.overdue) {
      // 逾期需求全额压今天，持续暴露"有逾期未清"的信号。
      days[0].load += it.remainingHours
      days[0].items.push({ id: it.id, hours: it.remainingHours })
      continue
    }
    const fromIdx = Math.max(0, dateIndex.get(it.startPoint) ?? 0)
    const toIdxRaw = dateIndex.get(it.effDeadline)
    const toIdx = toIdxRaw ?? horizon - 1
    for (let i = fromIdx; i <= Math.min(toIdx, horizon - 1); i += 1) {
      days[i].load += it.dailyLoad
      days[i].items.push({ id: it.id, hours: it.dailyLoad })
    }
  }
  for (const d of days) d.saturation = d.load / capacity

  const totalHours = items.reduce((s, it) => s + it.remainingHours, 0)
  const peak = days.reduce((m, d) => (d.saturation > m.saturation ? d : m), days[0] ?? { saturation: 0, date: null as string | null })
  return {
    items,
    days,
    totalHours,
    totalUnits: Math.round(totalHours / HOUR_UNIT),
    activeCount: items.length,
    unevaluatedCount: items.filter((it) => it.unevaluated).length,
    todaySaturation: days[0]?.saturation ?? 0,
    peakSaturation: peak?.saturation ?? 0,
    peakDate: peak?.date ?? null,
    overloadDays: days.filter((d) => d.saturation > 1).length,
  }
}

export interface AcceptCheck {
  /** 全部覆盖日接入后饱和度 ≤ 1。 */
  acceptable: boolean
  /** 接入后峰值饱和度与日期。 */
  worstSaturation: number
  worstDate: string | null
  /** 接入后超载的日期列表。 */
  breachDates: { date: string; saturation: number }[]
  /** 新需求每日摊派。 */
  dailyLoad: number
  coveredDays: number
  /** 一句话结论（直接可回复产品）。 */
  summary: string
}

/** 接单评估：新需求（hoursUnits 单位，deadline 提测）均摊进 [今天, deadline]，逐日检查饱和度。 */
export function checkAcceptance(
  util: UtilResult,
  config: ScheduleConfig,
  opts: { hoursUnits: number; deadline: string; title?: string; today?: Date },
): AcceptCheck {
  const todayStr = ymd(opts.today ?? new Date())
  const hours = Math.max(1, Math.round(opts.hoursUnits)) * HOUR_UNIT
  const deadline = opts.deadline
  const name = opts.title?.trim() || "新需求"
  const empty: AcceptCheck = {
    acceptable: false,
    worstSaturation: 0,
    worstDate: null,
    breachDates: [],
    dailyLoad: 0,
    coveredDays: 0,
    summary: "提测时间无效（早于今天或格式错误），无法评估。",
  }
  if (!deadline || deadline < todayStr) return empty

  const span = workdaysBetweenInclusive(todayStr, deadline, config.skipWeekends)
  const dailyLoad = hours / Math.max(1, span)
  const capacity = Math.max(0.5, config.dailyHours)
  const loadByDate = new Map(util.days.map((d) => [d.date, d.load]))

  const breachDates: { date: string; saturation: number }[] = []
  let worstSaturation = 0
  let worstDate: string | null = null
  let coveredDays = 0
  // 覆盖日 = [今天, deadline] 的工作日；负载超出 util.days 序列（horizon 外）按 0 近似。
  for (const date of workdayList(opts.today ?? new Date(), span, config.skipWeekends)) {
    coveredDays += 1
    const newLoad = (loadByDate.get(date) ?? 0) + dailyLoad
    const sat = newLoad / capacity
    if (sat > worstSaturation) {
      worstSaturation = sat
      worstDate = date
    }
    if (sat > 1) breachDates.push({ date, saturation: sat })
  }
  const acceptable = breachDates.length === 0
  const u = Math.round(hours / HOUR_UNIT)
  const pct = (v: number) => `${Math.round(v * 100)}%`
  const summary = acceptable
    ? `可以接：${name}（${u}u，${deadline} 提测）每天摊 ${dailyLoad.toFixed(1)}h，接入后峰值饱和度 ${pct(worstSaturation)}（${worstDate}），全程不超载。`
    : `不建议接：${name}（${u}u，${deadline} 提测）接入后 ${breachDates.length} 天超载，最高 ${pct(worstSaturation)}（${breachDates[0]?.date ?? worstDate}）；建议提测推迟，或先清掉部分在制需求。`
  return { acceptable, worstSaturation, worstDate, breachDates, dailyLoad, coveredDays, summary }
}

export interface MaxAccept {
  deadline: string
  /** 到 deadline 均摊口径下还能接的最大真实工时 / 单位数。 */
  hours: number
  units: number
  /** 区间内每日剩余容量之和（= hours 上限）。 */
  freeHours: number
  coveredDays: number
  /** 一句话结论。 */
  summary: string
}

/** 能接多大：给定提测日，反算区间内每日剩余容量之和（新需求同样按均摊口径时最多可接的量）。 */
export function maxAcceptable(util: UtilResult, config: ScheduleConfig, deadline: string, today = new Date()): MaxAccept {
  const todayStr = ymd(today)
  const capacity = Math.max(0.5, config.dailyHours)
  const empty: MaxAccept = {
    deadline,
    hours: 0,
    units: 0,
    freeHours: 0,
    coveredDays: 0,
    summary: "提测时间无效（早于今天或格式错误），无法估算。",
  }
  if (!deadline || deadline < todayStr) return empty
  const coveredDays = workdaysBetweenInclusive(todayStr, deadline, config.skipWeekends)
  const loadByDate = new Map(util.days.map((d) => [d.date, d.load]))
  let freeHours = 0
  // 覆盖日 = [今天, deadline] 的工作日；负载超出 util.days 序列（horizon 外）按 0 近似。
  for (const date of workdayList(today ?? new Date(), coveredDays, config.skipWeekends)) {
    freeHours += Math.max(0, capacity - (loadByDate.get(date) ?? 0))
  }
  const units = Math.floor(freeHours / HOUR_UNIT)
  const hours = units * HOUR_UNIT
  const summary =
    units > 0
      ? `到 ${deadline} 提测：最多还能接 ${units}u（${hours}h 真实工时，按均摊口径）；当前区间剩余容量 ${freeHours.toFixed(1)}h。`
      : `到 ${deadline} 提测：没有剩余容量（在制需求已占满），接新需求必须先清掉存量或推迟提测。`
  return { deadline, hours, units, freeHours, coveredDays, summary }
}
