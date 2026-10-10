/**
 * Role: 需求排期页 — 并行均摊模型：每个在制需求的剩余工时均摊到「投入 → 提测」区间，得到每日工作饱和度；
 * 据此判断"能否接新需求 / 能接多大需求"。饱和度是容量占用账，不锁定每天具体做哪个需求（实际安排弹性）。
 * Public surface: SchedulePage used by web/src/App.tsx route /schedule.
 * Constraints: 读 /api/requirements + /api/ones/tasks；写仅 POST /api/requirement/effort-estimate（工时）
 *              与 PATCH /api/requirement（提测日期）；容量配置/手动工时覆盖存 localStorage。
 * Read-this-with: web/src/lib/schedule.ts（均摊算法）、web/src/pages/manhour.tsx（ONES logged 匹配）、
 *                 web/src/pages/release-plan.tsx（页面骨架模式）。
 */
import { CalendarClock, CalendarRange, Gauge, ListOrdered, TriangleAlert, Wand2 } from "lucide-react"
import { useMemo, useState } from "react"
import type { OnesTasksResponse, Requirement } from "../types"
import { patchJson, postJson, useFetch } from "../lib/api"
import {
  DEFAULT_CONFIG,
  DEFAULT_HOURS,
  HOUR_UNIT,
  type HoursSource,
  type ScheduleConfig,
  type ScheduleReqInput,
  type UtilResult,
  type AcceptCheck,
  type MaxAccept,
  checkAcceptance,
  maxAcceptable,
  utilization,
  ymd,
} from "../lib/schedule"
import { projectsOf, statusPill } from "../features/requirements/badges"
import { EmptyCard, ErrorCard, KpiCard, LoadingCard, PageChrome, PanelHead } from "../components/ui"
import { RequirementsData } from "./projects"

const LS_CONFIG = "react-schedule-config"
/** 饱和度视图展示前 14 个工作日。 */
const VIEW_DAYS = 14
/** 单位点选可选档位（1u=4h 真实工时，8u=两周量级封顶）。 */
const UNIT_OPTIONS = [1, 2, 3, 4, 5, 6, 8]
/** 队列门槛：开发工作未完成（状态 < 人工核查）的状态集合。 */
const QUEUEABLE_STATUSES = new Set(["需求澄清", "需求创建", "开发中", "自测中", "测试中"])
/** 永不进队列的状态（完成/发布/挂起/取消家族）。 */
const EXCLUDED_STATUSES = new Set(["人工核查", "发布就绪", "经验总结", "已完成", "已发布", "已合入", "已关闭", "已取消", "挂起"])

type PanelConfig = ScheduleConfig & { onesHoursFactor: number }
/** ONES 登记 10h/天 ≈ 实际 8h/天：登记工时按 0.8 折算（历史验证 657.5h×0.8/63 天 ≈ 8.35h/天）。 */
const DEFAULT_PANEL_CONFIG: PanelConfig = { ...DEFAULT_CONFIG, onesHoursFactor: 0.8 }

function loadJson<T extends object>(key: string, fallback: T): T {
  try {
    const raw = localStorage.getItem(key)
    return raw ? ({ ...fallback, ...JSON.parse(raw) } as T) : fallback
  } catch {
    return fallback
  }
}

function saveJson(key: string, value: unknown) {
  try {
    localStorage.setItem(key, JSON.stringify(value))
  } catch {
    /* 存储不可用时静默降级为会话内状态 */
  }
}

/** 与 manhour 页一致：从 ones 字段提取任务编号（后续用大写匹配 ONES 候选）。 */
function reqDisplayId(req: Requirement): string | null {
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
  return ymd(new Date(ms))
}

/** 剩余工时推导：人工矫正 > agent 预估 > ONES 剩余(登记口径×factor 折算真实) > 默认 8h。 */
function deriveHours(
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
  return { hours: DEFAULT_HOURS, source: "default", unevaluated: true }
}

function buildQueueReqs(
  reqs: Requirement[],
  loggedMap: Map<string, number>,
  onesFactor: number,
): ScheduleReqInput[] {
  return reqs
    .filter((r) => !EXCLUDED_STATUSES.has(r.status) && QUEUEABLE_STATUSES.has(r.status))
    .map((r) => {
      const did = reqDisplayId(r)
      const logged = did ? loggedMap.get(did) ?? null : null
      const { hours, source, unevaluated } = deriveHours(r, logged, onesFactor)
      const submit = r.submitTestDate && r.submitTestDate !== "unknown" ? r.submitTestDate : null
      return {
        id: r.id,
        title: r.title,
        status: r.status,
        startDate: msToYmd(r.createdAt),
        submitTestDate: submit,
        isHotfix: (r.issues?.length ?? 0) > 0,
        remainingHours: hours,
        hoursSource: source,
        unevaluated,
      }
    })
    .filter((r) => r.remainingHours > 0)
}

const SOURCE_LABEL: Record<HoursSource, string> = {
  manual: "人工矫正",
  ones: "ONES 剩余",
  estimate: "agent 评估",
  default: "未评估",
}

/** 饱和度色阶：<70% 从容、70-100% 满、>100% 超载。 */
function saturationTone(sat: number): { cls: string; label: string } {
  if (sat > 1) return { cls: "react-sched-sat-over", label: "超载" }
  if (sat >= 0.7) return { cls: "react-sched-sat-full", label: "饱和" }
  return { cls: "react-sched-sat-ok", label: "从容" }
}

const pctText = (v: number) => `${Math.round(v * 100)}%`

export function SchedulePage({ globalProject }: { globalProject?: string }) {
  const { data, error, loading, refresh } = RequirementsData()
  const ones = useFetch<OnesTasksResponse>("/api/ones/tasks")
  const [project] = useState(globalProject || "")
  const [config, setConfig] = useState<PanelConfig>(() => loadJson(LS_CONFIG, DEFAULT_PANEL_CONFIG))
  const [whatIfMode, setWhatIfMode] = useState<"check" | "max">("check")
  const [whatIfUnits, setWhatIfUnits] = useState(2)
  const [whatIfTitle, setWhatIfTitle] = useState("")
  const [whatIfDeadline, setWhatIfDeadline] = useState("")
  const [savingReq, setSavingReq] = useState<string | null>(null)

  const loggedMap = useMemo(() => {
    const map = new Map<string, number>()
    for (const c of ones.data?.candidates || []) {
      if (c.actualHours && c.actualHours > 0) map.set(c.displayId.toUpperCase(), c.actualHours)
    }
    return map
  }, [ones.data])

  const allReqs = useMemo(() => {
    const list = data?.requirements || []
    return project ? list.filter((r) => (r.projects || [r.project]).includes(project)) : list
  }, [data, project])

  const queueReqs = useMemo(
    () => buildQueueReqs(allReqs, loggedMap, config.onesHoursFactor ?? 0.8),
    [allReqs, loggedMap, config.onesHoursFactor],
  )
  const util: UtilResult = useMemo(() => utilization(queueReqs, config), [queueReqs, config])

  const viewDays = util.days.slice(0, VIEW_DAYS)
  const dayDates = viewDays.map((d) => d.date)
  const firstDay = dayDates[0] ?? ""
  const overParallel = util.activeCount > config.parallelMax
  const dueIn7 = util.items.filter((it) => {
    if (!it.effDeadline || it.virtualDeadline) return false
    const diff = Math.round((new Date(`${it.effDeadline}T00:00:00`).getTime() - Date.now()) / (24 * 3600 * 1000))
    return diff >= 0 && diff <= 7
  })
  const overdueCount = util.items.filter((it) => it.overdue).length

  // what-if：模式 A 评估接单（工时 + 提测日）；模式 B 能接多大（仅提测日）。
  const accept: AcceptCheck | null = useMemo(() => {
    if (whatIfMode !== "check" || !whatIfDeadline) return null
    return checkAcceptance(util, config, { hoursUnits: whatIfUnits, deadline: whatIfDeadline, title: whatIfTitle })
  }, [util, config, whatIfMode, whatIfUnits, whatIfDeadline, whatIfTitle])
  const maxAcc: MaxAccept | null = useMemo(() => {
    if (whatIfMode !== "max" || !whatIfDeadline) return null
    return maxAcceptable(util, config, whatIfDeadline)
  }, [util, config, whatIfMode, whatIfDeadline])

  function updateConfig(patch: Partial<PanelConfig>) {
    const next = { ...config, ...patch }
    setConfig(next)
    saveJson(LS_CONFIG, next)
  }

  function setManualUnit(reqId: string, units: number) {
    setSavingReq(reqId)
    // 人工矫正写入 effort-estimate.json 的 manualHours（保留 agent 评估）；失败不阻塞
    postJson("/api/requirement/effort-estimate", {
      reqId,
      manualHours: units * HOUR_UNIT,
      summary: `人工矫正：${units} 单位（${units * HOUR_UNIT}h 真实工时）`,
    })
      .then(() => refresh())
      .catch(() => undefined)
      .finally(() => setSavingReq(null))
  }

  async function setSubmitTestDate(reqId: string, date: string) {
    setSavingReq(reqId)
    try {
      await patchJson("/api/requirement", { reqId, submitTestDate: date || "unknown" })
      refresh()
    } catch {
      /* 错误静默：下次 refresh 反映真实状态 */
    } finally {
      setSavingReq(null)
    }
  }

  return (
    <PageChrome
      icon={<CalendarRange size={18} />}
      eyebrow="Schedule"
      title="需求排期"
      description="并行均摊：每个在制需求的剩余工时均摊到「投入 → 提测」区间得到每日饱和度；饱和度是容量占用账，不锁定每天具体做哪个需求，实际安排弹性调整。"
    >
      {error ? <ErrorCard error={error} /> : null}
      {loading ? <LoadingCard /> : null}
      {!loading && !error ? (
        <>
          <div className="react-kpis">
            <KpiCard
              icon={<Gauge size={18} />}
              label="今日饱和度"
              value={pctText(util.todaySaturation)}
              sub={overParallel ? `${util.activeCount} 个在制，超并行上限 ${config.parallelMax}` : `${util.activeCount} 个在制需求 ÷ 每日 ${config.dailyHours}h`}
              tone={util.todaySaturation > 1 ? "rose" : util.todaySaturation >= 0.7 ? "warn" : "green"}
            />
            <KpiCard
              icon={<TriangleAlert size={18} />}
              label="未来峰值"
              value={pctText(util.peakSaturation)}
              sub={util.peakDate ? `峰值日 ${util.peakDate}；超载 ${util.overloadDays} 天` : "无排期数据"}
              tone={util.overloadDays > 0 ? "rose" : "active"}
            />
            <KpiCard
              icon={<CalendarClock size={18} />}
              label="7 天内提测"
              value={`${dueIn7.length}`}
              sub={overdueCount > 0 ? `${overdueCount} 个已逾期未清` : "按已登记的提测时间"}
              tone={overdueCount > 0 ? "warn" : "violet"}
            />
            <KpiCard
              icon={<ListOrdered size={18} />}
              label="剩余工作量"
              value={`${util.totalUnits}u`}
              sub={`${util.totalHours}h 真实工时 · 未评估 ${util.unevaluatedCount} 个`}
              tone="avg"
            />
          </div>

          <div className="react-panel">
            <PanelHead kicker="Capacity" title="容量设置" chip={<span className="react-muted">存本机，随调随算</span>} />
            <div className="react-sched-config">
              <label>
                每日有效开发
                <input
                  type="number"
                  min={1}
                  max={12}
                  step={0.5}
                  value={config.dailyHours}
                  onChange={(e) => updateConfig({ dailyHours: Math.max(0.5, Number(e.target.value) || 6) })}
                />
                小时
              </label>
              <label>
                并行上限
                <input
                  type="number"
                  min={1}
                  max={10}
                  value={config.parallelMax}
                  onChange={(e) => updateConfig({ parallelMax: Math.max(1, Number(e.target.value) || 3) })}
                />
                个需求
              </label>
              <label title="未登记提测时间的需求，假设每天投入该小时数推进，推算虚拟提测日">
                未登记提测：按每日
                <input
                  type="number"
                  min={1}
                  max={12}
                  step={0.5}
                  value={config.defaultDailyPerReq}
                  onChange={(e) => updateConfig({ defaultDailyPerReq: Math.max(0.5, Number(e.target.value) || 4) })}
                />
                h 推进
              </label>
              <label title="ONES 登记工时为膨胀口径（登记 10h/天 ≈ 实际 8h/天），折算为真实工时后再冲抵剩余工时">
                ONES 折算
                <input
                  type="number"
                  min={0.5}
                  max={1}
                  step={0.05}
                  value={config.onesHoursFactor ?? 0.8}
                  onChange={(e) => updateConfig({ onesHoursFactor: Math.min(1, Math.max(0.5, Number(e.target.value) || 0.8)) })}
                />
                ×（登记→真实）
              </label>
              <label className="react-sched-check">
                <input type="checkbox" checked={config.skipWeekends} onChange={(e) => updateConfig({ skipWeekends: e.target.checked })} />
                跳过周末
              </label>
            </div>
          </div>

          <div className="react-panel">
            <PanelHead kicker="What-if" title="接单评估" chip={<span className="react-muted">单位 = 4h 真实工时（非 ONES 登记口径）</span>} />
            <div className="react-sched-whatif">
              <div className="react-sched-pos">
                <button type="button" className={whatIfMode === "check" ? "is-active" : ""} onClick={() => setWhatIfMode("check")}>
                  能不能接
                </button>
                <button type="button" className={whatIfMode === "max" ? "is-active" : ""} onClick={() => setWhatIfMode("max")}>
                  能接多大
                </button>
              </div>
              {whatIfMode === "check" ? (
                <>
                  <input
                    className="react-sched-whatif-title"
                    placeholder="新需求名称（可选）"
                    value={whatIfTitle}
                    onChange={(e) => setWhatIfTitle(e.target.value)}
                  />
                  <div className="react-sched-units">
                    {UNIT_OPTIONS.map((u) => (
                      <button key={u} type="button" className={`react-sched-unit ${whatIfUnits === u ? "is-active" : ""}`} onClick={() => setWhatIfUnits(u)}>
                        {u}u
                      </button>
                    ))}
                  </div>
                </>
              ) : null}
              <label className="react-sched-deadline">
                提测时间
                <input type="date" value={whatIfDeadline} onChange={(e) => setWhatIfDeadline(e.target.value)} />
              </label>
            </div>
            {accept ? (
              <div className={`react-sched-whatif-result ${accept.acceptable ? "" : "is-breach"}`}>
                <p className="react-sched-whatif-summary">{accept.summary}</p>
                {accept.breachDates.length > 0 ? (
                  <p className="react-sched-whatif-detail">
                    超载日：{accept.breachDates.map((b) => `${b.date}（${pctText(b.saturation)}）`).join("、")}
                  </p>
                ) : null}
              </div>
            ) : null}
            {maxAcc ? (
              <div className={`react-sched-whatif-result ${maxAcc.units > 0 ? "" : "is-breach"}`}>
                <p className="react-sched-whatif-summary">{maxAcc.summary}</p>
              </div>
            ) : null}
          </div>

          <div className="react-panel">
            <PanelHead
              kicker="Daily Saturation"
              title="每日工作饱和度"
              chip={<span className="react-muted">未来 {viewDays.length} 个工作日 · 均摊口径，非日程</span>}
            />
            <div className="react-sched-days">
              {viewDays.map((d) => {
                const tone = saturationTone(d.saturation)
                return (
                  <div key={d.date} className={`react-sched-day ${tone.cls}`} title={`负载构成（均摊口径，实际弹性安排）：\n${d.items.map((i) => `${i.id} ${i.hours.toFixed(1)}h`).join("\n") || "无在制负载"}`}>
                    <span className="react-sched-day-date">{d.date.slice(5)}</span>
                    <div className="react-sched-day-bar">
                      <div
                        className={`react-sched-day-fill ${d.saturation > 1 ? "is-over" : d.saturation >= 0.7 ? "is-full" : ""}`}
                        style={{ width: `${Math.min(100, d.saturation * 100)}%` }}
                      />
                    </div>
                    <span className="react-sched-day-num">{pctText(d.saturation)}</span>
                  </div>
                )
              })}
            </div>
          </div>

          <div className="react-panel">
            <PanelHead
              kicker="WIP"
              title="在制需求均摊"
              chip={<span className="react-muted">{util.items.length} 个在制 · 按提测时间排序</span>}
            />
            {util.items.length === 0 ? (
              <EmptyCard>当前没有在制需求（状态 &lt; 人工核查），随时可以接新需求。</EmptyCard>
            ) : (
              <div className="react-sched-table-wrap">
                <table className="react-sched-table">
                  <thead>
                    <tr>
                      <th>需求</th>
                      <th>状态</th>
                      <th>剩余</th>
                      <th>每日摊派</th>
                      <th>提测时间</th>
                      <th>剩余工作日</th>
                      <th>区间</th>
                      <th>改工时</th>
                    </tr>
                  </thead>
                  <tbody>
                    {util.items.map((it) => {
                      const units = Math.round(it.remainingHours / HOUR_UNIT)
                      const loadPct = it.dailyLoad / Math.max(0.5, config.dailyHours)
                      // 区间着色：投入日 → 有效提测日 与视图窗口的交集
                      const startRaw = it.startPoint > firstDay ? it.startPoint : firstDay
                      const startIdx = dayDates.indexOf(startRaw)
                      const endIdx = dayDates.indexOf(it.effDeadline)
                      const workFrom = startIdx >= 0 ? startIdx : 0
                      const workTo = endIdx >= 0 ? endIdx : VIEW_DAYS - 1
                      const deadlineIdx = dayDates.indexOf(it.effDeadline)
                      const req = allReqs.find((r) => r.id === it.id)
                      return (
                        <tr key={it.id} className={it.overdue ? "is-overdue" : ""}>
                          <td>
                            {it.isHotfix ? <span className="react-sched-hotfix" title="绑定线上问题的抢修需求">🚑</span> : null}
                            <a href={`/requirement?id=${encodeURIComponent(it.id)}`} className="react-sched-req-link">
                              {it.id}
                            </a>
                            <span className="react-sched-req-title">{it.title}</span>
                            {req ? <span className="react-muted react-sched-req-projects">{projectsOf(req)}</span> : null}
                          </td>
                          <td>{statusPill(it.status)}</td>
                          <td>
                            <strong>{units}u</strong>
                            <span className="react-muted"> {it.remainingHours}h</span>
                            <em className={`react-sched-src ${it.hoursSource === "default" ? "is-warn" : ""}`}>{SOURCE_LABEL[it.hoursSource]}</em>
                          </td>
                          <td>
                            <strong className={loadPct > 1 ? "react-sched-overload-text" : ""}>{it.dailyLoad.toFixed(1)}h</strong>
                            <span className="react-muted">/天（{pctText(loadPct)} 容量）</span>
                          </td>
                          <td>
                            <input
                              type="date"
                              value={it.virtualDeadline ? "" : it.effDeadline}
                              disabled={savingReq === it.id}
                              title={it.virtualDeadline ? "未登记提测时间，当前为虚拟推算；点选日期即登记" : "登记/调整提测时间"}
                              onChange={(e) => setSubmitTestDate(it.id, e.target.value)}
                            />
                          </td>
                          <td>
                            {it.overdue ? (
                              <span className="react-sched-risk react-sched-risk-overdue">已逾期</span>
                            ) : (
                              <>
                                {it.spanDays} 天
                                {it.virtualDeadline ? <span className="react-muted">（虚拟）</span> : null}
                              </>
                            )}
                          </td>
                          <td>
                            <div className="react-sched-tl">
                              {dayDates.map((d, i) => (
                                <span key={d} className={i >= workFrom && i <= workTo ? "is-work" : i === deadlineIdx ? "is-deadline" : ""} />
                              ))}
                            </div>
                          </td>
                          <td>
                            <div className="react-sched-units react-sched-units-sm">
                              {UNIT_OPTIONS.map((u) => (
                                <button
                                  key={u}
                                  type="button"
                                  className={`react-sched-unit ${units === u ? "is-active" : ""}`}
                                  disabled={savingReq === it.id}
                                  title={it.unevaluated ? "当前为默认值（未评估），点击设为该单位" : `设为 ${u} 单位（${u * HOUR_UNIT}h 真实工时）`}
                                  onClick={() => setManualUnit(it.id, u)}
                                >
                                  {u}u
                                </button>
                              ))}
                            </div>
                          </td>
                        </tr>
                      )
                    })}
                  </tbody>
                </table>
              </div>
            )}
          </div>
        </>
      ) : null}
    </PageChrome>
  )
}
