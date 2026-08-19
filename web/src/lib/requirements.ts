import type { ReqCategory, ReqStatus } from "../types"

export const REQ_FLOW_STATUSES: ReqStatus[] = ["需求澄清", "开发中", "自测中", "测试中", "经验总结", "已完成"]
export const ISSUE_STATUSES: ReqStatus[] = ["排查中", "已确认"]
export const REQ_STATUSES: ReqStatus[] = [...REQ_FLOW_STATUSES, ...ISSUE_STATUSES]
export const REQ_CATEGORIES: ReqCategory[] = ["需求", "线上问题"]

export const statusMeta: Record<string, { color: string; soft: string }> = {
  需求澄清: { color: "#94a3b8", soft: "rgba(148, 163, 184, 0.14)" },
  开发中: { color: "#22d3ee", soft: "rgba(34, 211, 238, 0.14)" },
  自测中: { color: "#3b82f6", soft: "rgba(59, 130, 246, 0.14)" },
  测试中: { color: "#a855f7", soft: "rgba(168, 85, 247, 0.14)" },
  经验总结: { color: "#eab308", soft: "rgba(234, 179, 8, 0.14)" },
  已完成: { color: "#22c55e", soft: "rgba(34, 197, 94, 0.14)" },
  需求对齐: { color: "#94a3b8", soft: "rgba(148, 163, 184, 0.14)" },
  方案设计: { color: "#94a3b8", soft: "rgba(148, 163, 184, 0.14)" },
  待上线: { color: "#eab308", soft: "rgba(234, 179, 8, 0.14)" },
  排查中: { color: "#fb7185", soft: "rgba(244, 63, 94, 0.14)" },
  已确认: { color: "#f97316", soft: "rgba(249, 115, 22, 0.14)" },
}
