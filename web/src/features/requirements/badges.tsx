import type { Requirement } from "../../types"
import { parseOnesRef } from "../../lib/format"
import { statusMeta } from "../../lib/requirements"

export function statusPill(status: string) {
  const meta = statusMeta[status] || statusMeta["需求澄清"]
  return <span className="react-status-pill" style={{ color: meta.color, background: meta.soft, borderColor: `${meta.color}55` }}>{status}</span>
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

export function onesBadge(ones?: string) {
  const ref = parseOnesRef(ones)
  if (!ref) return <span className="react-ones-badge react-ones-missing" title="未关联 ONES 任务">⚠ 未关联 ONES</span>
  if (ref.url) return <a className="react-ones-badge react-ones-linked" href={ref.url} target="_blank" rel="noopener noreferrer" title={ref.raw}>🔗 ONES</a>
  return <span className="react-ones-badge react-ones-id" title={ref.label}>ONES</span>
}
