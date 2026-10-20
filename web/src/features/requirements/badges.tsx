import type { Requirement } from "../../types"
import { parseOnesRef } from "../../lib/format"
import { statusMeta } from "../../lib/requirements"

export function statusPill(status: string) {
  const meta = statusMeta[status] || statusMeta["需求澄清"]
  return <span className="react-status-pill" style={{ color: meta.color, background: meta.soft, borderColor: `${meta.color}55` }}>{status}</span>
}

/** 展示用状态：需求组用聚合状态（min 成员状态），普通需求用自身状态。 */
export function effectiveStatus(req: Pick<Requirement, "status" | "groupStatus">): string {
  return req.groupStatus || req.status
}

/** 引用式需求组徽标：成员数 + 瓶颈/失效引用提示。 */
export function groupBadge(req: Requirement) {
  const members = req.groupMembers ?? []
  if (!members.length) return null
  const missing = members.filter((m) => !m.found).length
  const bottleneck = members.find((m) => m.reqId === req.groupBottleneck)
  const title = [
    `引用式需求组：${members.length} 个成员（发布策略 ${req.groupPolicy === "together" ? "整体发布" : "独立发布"}）`,
    `组状态 = min(成员状态)，${bottleneck ? `当前瓶颈：${bottleneck.reqId}（${bottleneck.status ?? "?"}）` : "暂无可计算的成员状态"}`,
    missing ? `⚠ ${missing} 个失效引用（成员需求不存在）` : null,
    "session 绑定组时自动绑定所有成员；组状态不能手动设置，推进瓶颈成员即可推进组进度",
  ].filter(Boolean).join("\n")
  return <span className="react-status-pill react-group-badge" title={title} style={{ color: "#818cf8", background: "rgba(129, 140, 248, 0.14)", borderColor: "rgba(129, 140, 248, 0.45)" }}>组 {members.length}{missing ? "⚠" : ""}</span>
}

/** 返工徽标：需求曾被人工核查/测试阶段打回开发中重做（rework loop）。 */
export function reworkBadge(req: Requirement) {
  const rounds = req.reworkRounds ?? 0
  if (rounds <= 0) return null
  const high = rounds >= 3
  const title = [
    `打回返工 ${rounds} 轮（R${rounds}）：人工核查/测试中 → 开发中`,
    high ? "⚠ 打回≥3 次：建议回头补需求澄清/拆分，而不是继续磨" : null,
    "返工轮次 agent 加载 phase-dev-rework 变体提示词：增量修复 + test.md 返工回归小节 + 增量审查",
  ].filter(Boolean).join("\n")
  return <span className="react-status-pill react-rework-badge" title={title} style={{ color: high ? "#fca5a5" : "#fbbf24", background: high ? "rgba(248, 113, 113, 0.12)" : "rgba(251, 191, 36, 0.12)", borderColor: high ? "rgba(248, 113, 113, 0.5)" : "rgba(251, 191, 36, 0.45)" }}>↺ R{rounds}</span>
}

/** 发布就绪小循环徽标：需求曾从发布就绪回开发中做上线前快速修复（fast-fix loop）。 */
export function releaseReadyBadge(req: Requirement) {
  const rounds = req.releaseReadyRounds ?? 0
  if (rounds <= 0) return null
  const title = [
    `发布就绪小循环 ${rounds} 轮（F${rounds}）：发布就绪 → 开发中`,
    "上线前快速修复：小改动、默认免单测、只部署 UAT（CN+SEA 成对）并在 UAT 测试",
    "agent 加载 phase-dev-release-ready 变体提示词：增量审查后直通发布就绪",
  ].join("\n")
  return <span className="react-status-pill react-rework-badge" title={title} style={{ color: "#7dd3fc", background: "rgba(56, 189, 248, 0.12)", borderColor: "rgba(56, 189, 248, 0.45)" }}>⚡ F{rounds}</span>
}

export function experienceSummaryStage(req: Requirement): "available" | "running" | "completed" | "failed" | "skipped" | "none" {
  const status = req.experienceSummaryJob?.status || ""
  if (status === "completed") return "completed"
  if (status === "running" || status === "pending") return "running"
  if (status === "failed") return "failed"
  if (status === "skipped") return "skipped"
  if (req.status === "经验总结") return "available"
  return "none"
}

export function experienceSummaryPill(req: Requirement) {
  const stage = experienceSummaryStage(req)
  if (stage === "none") return null
  const meta: Record<string, { label: string; color: string; soft: string }> = {
    available: { label: "可经验总结", color: "#facc15", soft: "rgba(250, 204, 21, .14)" },
    running: { label: req.experienceSummaryJob?.status === "pending" ? "自动总结排队中" : "自动经验总结中", color: "#22d3ee", soft: "rgba(34, 211, 238, .14)" },
    completed: { label: "自动总结完毕", color: "#22c55e", soft: "rgba(34, 197, 94, .14)" },
    failed: { label: "自动总结失败", color: "#ef4444", soft: "rgba(239, 68, 68, .14)" },
    skipped: { label: "跳过总结", color: "#94a3b8", soft: "rgba(148, 163, 184, .14)" },
  }
  const item = meta[stage]
  return <span className="react-status-pill react-exp-summary-pill" style={{ color: item.color, background: item.soft, borderColor: `${item.color}66` }}>{item.label}</span>
}

export function experienceSummaryStageLabel(stage: string): string {
  switch (stage) {
    case "available": return "可经验总结"
    case "running": return "自动总结中"
    case "completed": return "总结完毕"
    case "failed": return "总结失败"
    case "skipped": return "已跳过"
    default: return "其他"
  }
}

export function projectsOf(req: Requirement): string {
  return (req.projects?.length ? req.projects : [req.project]).filter(Boolean).join(" / ") || "-"
}

export function onesBadge(ones?: string, opts?: { onMissingClick?: () => void }) {
  const ref = parseOnesRef(ones)
  if (!ref) return opts?.onMissingClick
    ? <button type="button" className="react-ones-badge react-ones-missing" onClick={opts.onMissingClick} title="未关联 ONES 任务，点击登记">⚠ 未关联 ONES</button>
    : <span className="react-ones-badge react-ones-missing" title="未关联 ONES 任务">⚠ 未关联 ONES</span>
  if (ref.url) return <a className="react-ones-badge react-ones-linked" href={ref.url} target="_blank" rel="noopener noreferrer" title={ref.raw}>🔗 ONES</a>
  return <span className="react-ones-badge react-ones-id" title={ref.label}>ONES</span>
}
