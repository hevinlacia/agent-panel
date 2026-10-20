/**
 * Role: 需求排期页 — 并行均摊模型：每个在制需求的剩余工时均摊到「投入 → 提测」区间，得到每日工作饱和度；
 * 据此判断"能否接新需求 / 能接多大需求"。饱和度是容量占用账，不锁定每天具体做哪个需求（实际安排弹性）。
 * 未登记提测时间的需求由容量填充推算推荐提测日（开发完成日 + 预留工作日），默认排期即按推荐值计算。
 * Public surface: SchedulePage used by web/src/App.tsx route /schedule.
 * Constraints: 读 /api/requirements + /api/ones/tasks；写仅 POST /api/requirement/effort-estimate（工时）
 *              与 PATCH /api/requirement（提测日期）；容量配置/默认排除状态存 localStorage。
 * Read-this-with: web/src/lib/schedule.ts（均摊 + 推荐提测算法）、web/src/lib/schedule-input.ts（输入组装）、
 *                 web/src/pages/manhour.tsx（ONES logged 匹配）、web/src/pages/release-plan.tsx（页面骨架模式）。
 */
import { CalendarClock, CalendarRange, ChevronDown, Gauge, ListOrdered, TriangleAlert, Wand2 } from "lucide-react"
import { useMemo, useState, type ReactNode } from "react"
import type { OnesTasksResponse, Requirement } from "../types"
import { patchJson, postJson, useFetch } from "../lib/api"
import { SCHEDULE_FALLBACK_EXCLUDED_STATUSES, persistScheduleDefaultExcludedStatuses, readScheduleDefaultExcludedStatuses } from "../lib/preferences"
import {
  HOUR_UNIT,
  type HoursSource,
  type UtilResult,
  type AcceptCheck,
  type MaxAccept,
  SCHEDULE_STATUS_OPTIONS,
  checkAcceptance,
  maxAcceptable,
  recommendSummary,
  utilization,
  ymd,
} from "../lib/schedule"
import {
  type PanelConfig,
  DEFAULT_PANEL_CONFIG,
  QUEUEABLE_STATUSES,
  buildQueueReqs,
  loggedHoursMap,
  readPanelConfig,
  savePanelConfig,
} from "../lib/schedule-input"
import { projectsOf, statusPill } from "../features/requirements/badges"
import { EmptyCard, ErrorCard, LoadingCard, PageChrome, PanelHead } from "../components/ui"
import { RequirementsData } from "./projects"

/** 饱和度视图展示前 14 个工作日。 */
const VIEW_DAYS = 14
/** 单位点选可选档位（1u=4h 虚拟工时，8u=两周量级封顶）。 */
const UNIT_OPTIONS = [1, 2, 3, 4, 5, 6, 8]

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

/** 排期页平铺统计格：icon + label/sub 左侧，value 右侧（区别于通用 KpiCard 的纵向高卡片）。 */
function SchedStat({ icon, label, value, sub, tone }: { icon: ReactNode; label: string; value: string | number; sub: string; tone: string }) {
  return (
    <div className={`react-sched-stat react-sched-stat-${tone}`}>
      <div className="react-sched-stat-icon">{icon}</div>
      <div className="react-sched-stat-text">
        <span className="react-sched-stat-label">{label}</span>
        <span className="react-sched-stat-sub">{sub}</span>
      </div>
      <strong className="react-sched-stat-value">{value}</strong>
    </div>
  )
}

export function SchedulePage({ globalProject }: { globalProject?: string }) {
  const { data, error, loading, refresh } = RequirementsData()
  const ones = useFetch<OnesTasksResponse>("/api/ones/tasks")
  const [project] = useState(globalProject || "")
  const [config, setConfig] = useState<PanelConfig>(() => readPanelConfig())
  const [whatIfMode, setWhatIfMode] = useState<"check" | "max">("check")
  const [whatIfUnits, setWhatIfUnits] = useState(2)
  const [whatIfTitle, setWhatIfTitle] = useState("")
  const [whatIfDeadline, setWhatIfDeadline] = useState("")
  const [savingReq, setSavingReq] = useState<string | null>(null)
  // 默认排除状态（模仿需求列表页）：勾选的状态不进饱和度计算；草稿式编辑，点「保存默认排除」才落盘并生效。
  const [defaultExcludedStatuses, setDefaultExcludedStatusesState] = useState<string[]>(readScheduleDefaultExcludedStatuses)
  const [excludedDraft, setExcludedDraft] = useState<string[]>(() => readScheduleDefaultExcludedStatuses())
  const [excludedOpen, setExcludedOpen] = useState(false)
  const [excludedJustSaved, setExcludedJustSaved] = useState(false)

  const loggedMap = useMemo(() => loggedHoursMap(ones.data), [ones.data])

  const allReqs = useMemo(() => {
    const list = data?.requirements || []
    return project ? list.filter((r) => (r.projects || [r.project]).includes(project)) : list
  }, [data, project])

  // 各状态需求计数（跟随项目筛选），供「默认排除状态」复选框展示。
  const statusCounts = useMemo(() => {
    const counts: Record<string, number> = {}
    for (const r of allReqs) counts[r.status] = (counts[r.status] || 0) + 1
    return counts
  }, [allReqs])
  const excludedStatusSet = useMemo(() => new Set(defaultExcludedStatuses), [defaultExcludedStatuses])

  const queueReqs = useMemo(
    () => buildQueueReqs(allReqs, loggedMap, config.onesHoursFactor ?? 0.8, excludedStatusSet, QUEUEABLE_STATUSES),
    [allReqs, loggedMap, config.onesHoursFactor, excludedStatusSet],
  )
  const util: UtilResult = useMemo(() => utilization(queueReqs, config), [queueReqs, config])

  const viewDays = util.days.slice(0, VIEW_DAYS)
  const dayDates = viewDays.map((d) => d.date)
  const firstDay = dayDates[0] ?? ""
  const overParallel = util.activeCount > config.parallelMax
  // 紧急度看「承诺提测时间」（未设回退实际）；容量/饱和度始终用实际提测时间（it.effDeadline）
  const todayStr = ymd(new Date())
  const dueIn7 = util.items.filter((it) => {
    if (!it.promiseDate) return false
    const diff = Math.round((new Date(`${it.promiseDate}T00:00:00`).getTime() - Date.now()) / (24 * 3600 * 1000))
    return diff >= 0 && diff <= 7
  })
  const overdueCount = util.items.filter((it) => it.promiseDate != null && it.promiseDate < todayStr).length

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
    savePanelConfig(next)
  }

  const statusOrder = (list: string[]) => SCHEDULE_STATUS_OPTIONS.filter((s) => list.includes(s))
  const excludedDirty = JSON.stringify(statusOrder(excludedDraft)) !== JSON.stringify(statusOrder(defaultExcludedStatuses))
  const toggleExcludedOpen = () => {
    if (!excludedOpen) setExcludedDraft(defaultExcludedStatuses) // 展开时以已保存值为草稿
    setExcludedOpen(!excludedOpen)
  }
  // 仅显式动作（保存按钮 / 恢复默认）才写 localStorage；展开区勾选只改草稿。
  const saveDefaultExcludedStatuses = (next: string[]) => {
    const cleaned = next.filter((s, i, arr) => SCHEDULE_STATUS_OPTIONS.includes(s) && arr.indexOf(s) === i)
    setDefaultExcludedStatusesState(cleaned)
    setExcludedDraft(cleaned)
    persistScheduleDefaultExcludedStatuses(cleaned)
  }
  const resetDefaultExcludedStatuses = () => saveDefaultExcludedStatuses(SCHEDULE_FALLBACK_EXCLUDED_STATUSES)
  const saveExcludedDraft = () => {
    saveDefaultExcludedStatuses(excludedDraft)
    setExcludedJustSaved(true)
    window.setTimeout(() => setExcludedJustSaved(false), 1600)
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
      description="并行均摊：每个在制需求的剩余工时（虚拟工时口径）均摊到「投入 → 提测」区间得到每日饱和度。未登记提测时间的需求按剩余容量推算推荐提测日（开发完成 + 预留天数），未手动修改前默认按推荐值参与排期。"
    >
      {error ? <ErrorCard error={error} /> : null}
      {loading ? <LoadingCard /> : null}
      {!loading && !error ? (
        <>
          <div className="react-sched-strip">
            <SchedStat
              icon={<Gauge size={18} />}
              label="今日饱和度"
              value={pctText(util.todaySaturation)}
              sub={overParallel ? `${util.activeCount} 个在制，超并行上限 ${config.parallelMax}` : `${util.activeCount} 个在制需求 ÷ 每日 ${config.dailyHours}h`}
              tone={util.todaySaturation > 1 ? "rose" : util.todaySaturation >= 0.7 ? "warn" : "green"}
            />
            <SchedStat
              icon={<TriangleAlert size={18} />}
              label="未来峰值"
              value={pctText(util.peakSaturation)}
              sub={util.peakDate ? `峰值日 ${util.peakDate}；超载 ${util.overloadDays} 天` : "无排期数据"}
              tone={util.overloadDays > 0 ? "rose" : "active"}
            />
            <SchedStat
              icon={<CalendarClock size={18} />}
              label="7 天内承诺提测"
              value={`${dueIn7.length}`}
              sub={overdueCount > 0 ? `${overdueCount} 个承诺已逾期` : "按承诺提测时间（紧急度）"}
              tone={overdueCount > 0 ? "warn" : "violet"}
            />
            <SchedStat
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
              <label title="多个未登记提测时间的需求之间分配容量时，未轮到推荐推算的需求按该小时数临时均摊占位">
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
              <label title="推荐提测 = 开发完成日之后预留的工作日数，覆盖代码审查 / 测试反馈问题修复 / 上线前准备；未登记提测时间的需求默认按推荐值排期">
                提测预留
                <input
                  type="number"
                  min={0}
                  max={10}
                  value={config.submitBufferDays ?? 2}
                  onChange={(e) => updateConfig({ submitBufferDays: Math.max(0, Math.min(10, Math.floor(Number(e.target.value) || 0))) })}
                />
                个工作日
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
            <button
              type="button"
              className={`react-filter-section-head react-collapse-head ${excludedOpen ? "open" : ""}`}
              onClick={toggleExcludedOpen}
              aria-expanded={excludedOpen}
            >
              <span>默认排除状态</span>
              <em className="react-collapse-summary">
                {excludedOpen
                  ? "勾选的状态不进饱和度计算；勾选只改草稿，点「保存默认排除」才生效"
                  : defaultExcludedStatuses.length
                    ? `默认排除：${defaultExcludedStatuses.join(" / ")}`
                    : "默认不排除任何状态"}
                <ChevronDown size={14} className="react-collapse-chevron" />
              </em>
            </button>
            {excludedOpen ? (
              <div className="react-excluded-editor">
                <div className="react-status-options react-excluded-status-options">
                  {SCHEDULE_STATUS_OPTIONS.map((s) => (
                    <label
                      key={s}
                      className={`react-status-option react-excluded-status-option ${excludedDraft.includes(s) ? "active" : ""}`}
                      title={
                        s === "挂起"
                          ? "流水外标记状态：任何状态可挂起，恢复后回到原状态；默认不进饱和度计算"
                          : s === "需求创建"
                            ? "子需求初始状态（从父需求/整合需求拆出的 XXX-S1）；拆出即计入饱和度，默认不排除"
                            : undefined
                      }
                    >
                      <input
                        type="checkbox"
                        checked={excludedDraft.includes(s)}
                        onChange={(e) => setExcludedDraft((cur) => (e.target.checked ? [...cur, s] : cur.filter((x) => x !== s)))}
                      />
                      <span>{s}</span>
                      <strong>{statusCounts[s] || 0}</strong>
                    </label>
                  ))}
                </div>
                <div className="react-excluded-save-row">
                  <button type="button" onClick={saveExcludedDraft} disabled={!excludedDirty}>{excludedJustSaved ? "已保存 ✓" : "保存默认排除"}</button>
                  <button type="button" onClick={resetDefaultExcludedStatuses}>默认排除恢复默认</button>
                  <em>{excludedDirty ? "有未保存的修改，保存后作用于排期计算并记住" : "与已保存一致"}</em>
                </div>
                <p className="react-muted">全部需求状态可勾选/取消；完成态（人工核查及以上）与挂起默认不进排期，取消勾选即纳入饱和度计算；子需求含「需求创建」初始状态一并计入；线上问题类状态不进排期页。</p>
              </div>
            ) : null}
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
                      <th>实际提测</th>
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
                              title={
                                it.virtualDeadline && it.recommendedDeadline
                                  ? recommendSummary(it, config) ?? ""
                                  : it.submitSource === "releaseReady"
                                    ? "实际提测：未登记，暂按进入「发布就绪」的日期推定；点选日期即登记覆盖"
                                    : "实际提测（排期饱和度口径）：登记/调整"
                              }
                              onChange={(e) => setSubmitTestDate(it.id, e.target.value)}
                            />
                            {it.virtualDeadline && it.recommendedDeadline ? (
                              <span className="react-sched-recommend">
                                <em title={recommendSummary(it, config) ?? ""}>推荐 {it.recommendedDeadline.slice(5)}</em>
                                <button
                                  type="button"
                                  className="react-ghost-btn"
                                  disabled={savingReq === it.id}
                                  title={`把实际提测登记为推荐值 ${it.recommendedDeadline}（当前未登记，排期已默认按推荐值计算）`}
                                  onClick={() => setSubmitTestDate(it.id, it.recommendedDeadline!)}
                                >
                                  采用
                                </button>
                              </span>
                            ) : null}
                            {it.promiseDate ? <span className="react-sched-promise" title="承诺提测时间（紧急度），详情页可改">承诺 {it.promiseDate}</span> : null}
                          </td>
                          <td>
                            {it.overdue ? (
                              <span className="react-sched-risk react-sched-risk-overdue">已逾期</span>
                            ) : (
                              <>
                                {it.spanDays} 天
                                {it.virtualDeadline ? <span className="react-muted">（推荐）</span> : null}
                                {!it.virtualDeadline && it.submitSource === "releaseReady" ? <span className="react-muted">（发布就绪推定）</span> : null}
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
                                  title={it.unevaluated ? "当前为默认值（未评估），点击设为该单位" : `设为 ${u} 单位（${u * HOUR_UNIT}h 虚拟工时）`}
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
