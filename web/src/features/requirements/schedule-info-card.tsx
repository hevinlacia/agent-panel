/**
 * Role: 需求详情页的排期信息卡 — ① 提测时间卡：实际/承诺两个提测时间登记（PATCH meta）+ 倒计时 +
 * ② 工时评估卡：agent 预估展示 + 人工矫正输入（POST effort-estimate manualHours），排期统计优先人工矫正。
 * Public surface: SubmitTestCard, EffortEstimateCard — used by web/src/pages/requirement.tsx.
 * Constraints: 写仅 PATCH /api/requirement（submitTestDate / promiseSubmitDate）与 POST /api/requirement/effort-estimate（manualHours）。
 * Read-this-with: web/src/lib/schedule.ts（4h 单位 + 推荐提测算法）、web/src/lib/schedule-input.ts（共享输入组装）、
 */
import { useMemo, useState } from "react"
import type { EffortEstimate, OnesTasksResponse, Requirement } from "../../types"
import { patchJson, postJson, useFetch } from "../../lib/api"
import { relAge } from "../../lib/format"
import { HOUR_UNIT, recommendSummary, utilization } from "../../lib/schedule"
import { QUEUEABLE_STATUSES, buildQueueReqs, loggedHoursMap, readPanelConfig } from "../../lib/schedule-input"
import { readScheduleDefaultExcludedStatuses } from "../../lib/preferences"
import { PanelHead } from "../../components/ui"

const DAY_MS = 24 * 3600 * 1000
/** 单位点选可选档位（与排期页一致）。 */
const UNIT_OPTIONS = [1, 2, 3, 4, 5, 6, 8]

function todayStr(): string {
  return ymdLocal(new Date())
}

function ymdLocal(d: Date): string {
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`
}

/** 提测倒计时：自然日差（提测日 − 今天）。 */
function countdown(deadline: string): { diff: number; text: string; cls: string } | null {
  if (!deadline || deadline === "unknown") return null
  const diff = Math.round((new Date(`${deadline}T00:00:00`).getTime() - new Date(`${todayStr()}T00:00:00`).getTime()) / DAY_MS)
  if (diff < 0) return { diff, text: `已逾期 ${-diff} 天`, cls: "react-sched-risk-overdue" }
  if (diff === 0) return { diff, text: "今天提测", cls: "react-sched-risk-tight" }
  const cls = diff <= 3 ? "react-sched-risk-tight" : diff <= 7 ? "react-sched-risk-tight" : "react-sched-risk-ok"
  return { diff, text: `还有 ${diff} 天`, cls }
}

export function SubmitTestCard({ req, allReqs, onSaved }: { req: Requirement; allReqs: Requirement[]; onSaved: () => void }) {
  const valid = (v?: string | null) => (v && v !== "unknown" ? v : "")
  const actual = valid(req.submitTestDate)
  const promise = valid(req.promiseSubmitDate)
  const [draftActual, setDraftActual] = useState(actual)
  const [draftPromise, setDraftPromise] = useState(promise)
  const [savingField, setSavingField] = useState<"actual" | "promise" | "recommend" | null>(null)
  const [err, setErr] = useState<string | null>(null)
  // 倒计时 = 紧急度口径：优先承诺提测时间，未设回退实际提测时间
  const cd = countdown(promise || actual)

  // Agent 推荐提测：与排期页同口径（全局需求 + ONES 登记工时 + 本机排期配置）跑容量填充推算；
  // 仅在本需求未登记实际提测时展示。ONES 接口失败不阻塞推荐（loggedMap 为空时工时回退评估/默认值）。
  const ones = useFetch<OnesTasksResponse>("/api/ones/tasks")
  const recommend = useMemo(() => {
    const config = readPanelConfig()
    const loggedMap = loggedHoursMap(ones.data)
    const queue = buildQueueReqs(allReqs, loggedMap, config.onesHoursFactor ?? 0.8, new Set(readScheduleDefaultExcludedStatuses()), QUEUEABLE_STATUSES)
    const item = utilization(queue, config).items.find((it) => it.id === req.id)
    if (!item || !item.virtualDeadline || !item.recommendedDeadline) return null
    return { item, summary: recommendSummary(item, config) ?? "" }
  }, [req.id, allReqs, ones.data])

  async function save(field: "actual" | "promise" | "recommend", date: string) {
    setSavingField(field)
    setErr(null)
    try {
      const body = field === "promise" ? { reqId: req.id, promiseSubmitDate: date || "unknown" } : { reqId: req.id, submitTestDate: date || "unknown" }
      await patchJson("/api/requirement", body)
      onSaved()
    } catch (e) {
      setErr(e instanceof Error ? e.message : String(e))
    } finally {
      setSavingField(null)
    }
  }

  const rows = [
    { key: "actual" as const, label: "实际提测", hint: "排期面板饱和度口径；进入「发布就绪」时自动刷新为当天（覆盖式）；未登记时排期默认按 Agent 推荐提测日计算", current: actual, draft: draftActual, setDraft: setDraftActual },
    { key: "promise" as const, label: "承诺提测", hint: "给产品/测试承诺的提测日，判断需求紧急度；倒计时按此计算", current: promise, draft: draftPromise, setDraft: setDraftPromise },
  ]

  return (
    <section className="react-panel react-sched-info-card">
      <PanelHead
        kicker="Submit Test"
        title="提测时间"
        chip={cd ? <span className={`react-sched-risk ${cd.cls}`}>{cd.text}</span> : <span className="react-muted">未登记</span>}
      />
      <p className="react-muted">实际 = 排期饱和度口径；承诺 = 给产品/测试的紧急度口径（倒计时按承诺，未设回退实际）。两者独立登记、互不覆盖。未登记实际提测时，排期默认按下方 Agent 推荐值计算。</p>
      {rows.map((row) => {
        const dirty = row.draft !== row.current
        return (
          <div key={row.key} className="react-sched-info-row">
            <span className="react-sched-info-label" title={row.hint}>{row.label}</span>
            <input
              type="date"
              value={row.draft}
              disabled={savingField !== null}
              onChange={(e) => row.setDraft(e.target.value)}
            />
            {dirty ? (
              <button type="button" disabled={savingField !== null} onClick={() => save(row.key, row.draft)}>
                {savingField === row.key ? "保存中…" : "保存"}
              </button>
            ) : null}
            {!dirty && row.current ? (
              <button type="button" className="react-ghost-btn" disabled={savingField !== null} title={`清除${row.label}`} onClick={() => save(row.key, "")}>
                清除
              </button>
            ) : null}
          </div>
        )
      })}
      {recommend ? (
        <div className="react-sched-info-row react-sched-recommend-row">
          <span className="react-sched-info-label" title="按全局排期容量填充推算：开发完成日 + 预留工作日（代码审查/测试反馈修复/上线准备）；未登记时排期默认按此值计算">Agent 推荐</span>
          <strong>{recommend.item.recommendedDeadline}</strong>
          <button
            type="button"
            className="react-ghost-btn"
            disabled={savingField !== null}
            title={recommend.summary}
            onClick={() => save("recommend", recommend.item.recommendedDeadline!)}
          >
            {savingField === "recommend" ? "保存中…" : "采用推荐"}
          </button>
          <em className="react-sched-recommend-detail" title={recommend.summary}>
            开发完成 {recommend.item.devFinishDate}
          </em>
        </div>
      ) : null}
      {err ? <p className="react-effort-error">{err}</p> : null}
    </section>
  )
}

const MODEL_LABEL: Record<string, string> = {
  "agent-auto": "agent 自动预估",
  "agent-manual": "agent 手动评估",
  "schedule-panel": "排期页设定",
  "manual-placeholder": "占位（未评估）",
}

export function EffortEstimateCard({ req, onSaved }: { req: Requirement; onSaved: () => void }) {
  const est: EffortEstimate | undefined = req.effortEstimate
  const agentH = est?.estimatedHours ?? 0
  const manualH = est?.manualHours ?? null
  const activeH = manualH != null && manualH > 0 ? manualH : agentH
  const activeUnits = Math.round(activeH / HOUR_UNIT)
  const [saving, setSaving] = useState(false)
  const [err, setErr] = useState<string | null>(null)
  const agentValid = agentH > 0 && est?.model !== "manual-placeholder"

  async function correct(units: number) {
    setSaving(true)
    setErr(null)
    try {
      await postJson("/api/requirement/effort-estimate", {
        reqId: req.id,
        manualHours: units * HOUR_UNIT,
        summary: `人工矫正：${units} 单位（${units * HOUR_UNIT}h 真实工时）`,
      })
      onSaved()
    } catch (e) {
      setErr(e instanceof Error ? e.message : String(e))
    } finally {
      setSaving(false)
    }
  }

  return (
    <section className="react-panel react-sched-info-card">
      <PanelHead
        kicker="Effort"
        title="工时评估"
        chip={<span className="react-muted">排期采用：{activeH > 0 ? `${activeUnits}u · ${activeH}h` : "未评估"}</span>}
      />
      <div className="react-sched-effort-grid">
        <div className="react-sched-effort-cell">
          <span className="react-sched-effort-label">agent 预估</span>
          {agentValid ? (
            <>
              <strong>{Math.round(agentH / HOUR_UNIT)}u · {agentH}h</strong>
              <em className="react-muted">{MODEL_LABEL[est?.model ?? ""] ?? est?.model} {est?.updatedAt ? `· ${relAge(est.updatedAt)}` : ""}</em>
              {est?.summary ? <em className="react-sched-effort-summary" title={est.summary}>{est.summary}</em> : null}
            </>
          ) : (
            <em className="react-muted">未评估（需求澄清完成时由 agent 自动预估，工时预估门禁拦截缺失）</em>
          )}
        </div>
        <div className="react-sched-effort-cell">
          <span className="react-sched-effort-label">人工矫正</span>
          {manualH != null && manualH > 0 ? (
            <strong>{Math.round(manualH / HOUR_UNIT)}u · {manualH}h</strong>
          ) : (
            <em className="react-muted">未矫正（默认采用 agent 预估）</em>
          )}
          <div className="react-sched-units react-sched-units-sm">
            {UNIT_OPTIONS.map((u) => (
              <button
                key={u}
                type="button"
                className={`react-sched-unit ${activeUnits === u ? "is-active" : ""}`}
                disabled={saving}
                title={`矫正为 ${u} 单位（${u * HOUR_UNIT}h 真实工时），排期统计优先于 agent 预估`}
                onClick={() => correct(u)}
              >
                {u}u
              </button>
            ))}
          </div>
        </div>
      </div>
      {err ? <p className="react-effort-error">{err}</p> : null}
    </section>
  )
}
