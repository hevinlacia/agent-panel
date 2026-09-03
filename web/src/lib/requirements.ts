import type { ReqCategory, ReqStatus } from "../types"

export const REQ_FLOW_STATUSES: ReqStatus[] = ["需求澄清", "开发中", "自测中", "测试中", "经验总结", "发布就绪", "已完成"]
export const ISSUE_STATUSES: ReqStatus[] = ["排查中", "已定位", "已修复", "已复盘", "已关闭"]
export const REQ_STATUSES: ReqStatus[] = [...REQ_FLOW_STATUSES, ...ISSUE_STATUSES]
export const REQ_CATEGORIES: ReqCategory[] = ["需求", "线上问题"]
export const REQ_SOURCES = ["产品推动", "开发推动"] as const
export type ReqSource = (typeof REQ_SOURCES)[number]

export const statusMeta: Record<string, { color: string; soft: string }> = {
  需求澄清: { color: "#94a3b8", soft: "rgba(148, 163, 184, 0.14)" },
  开发中: { color: "#22d3ee", soft: "rgba(34, 211, 238, 0.14)" },
  自测中: { color: "#3b82f6", soft: "rgba(59, 130, 246, 0.14)" },
  测试中: { color: "#a855f7", soft: "rgba(168, 85, 247, 0.14)" },
  经验总结: { color: "#eab308", soft: "rgba(234, 179, 8, 0.14)" },
  发布就绪: { color: "#34d399", soft: "rgba(52, 211, 153, 0.14)" },
  已完成: { color: "#22c55e", soft: "rgba(34, 197, 94, 0.14)" },
  需求对齐: { color: "#94a3b8", soft: "rgba(148, 163, 184, 0.14)" },
  方案设计: { color: "#94a3b8", soft: "rgba(148, 163, 184, 0.14)" },
  待上线: { color: "#eab308", soft: "rgba(234, 179, 8, 0.14)" },
  排查中: { color: "#fb7185", soft: "rgba(244, 63, 94, 0.14)" },
  已定位: { color: "#f97316", soft: "rgba(249, 115, 22, 0.14)" },
  已修复: { color: "#22d3ee", soft: "rgba(34, 211, 238, 0.14)" },
  已复盘: { color: "#818cf8", soft: "rgba(129, 140, 248, 0.14)" },
  已关闭: { color: "#94a3b8", soft: "rgba(148, 163, 184, 0.14)" },
}
