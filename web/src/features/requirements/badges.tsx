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
