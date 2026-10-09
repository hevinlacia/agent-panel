import type { ReqCategory, ReqStatus } from "../types"

export const REQ_FLOW_STATUSES: ReqStatus[] = ["需求澄清", "开发中", "自测中", "测试中", "人工核查", "发布就绪", "经验总结", "已完成"]
/** 流水外标记状态：任何状态可挂起（相当于需求标记），恢复时回到挂起前状态；不参与组聚合/门禁。 */
export const SUSPEND_STATUS: ReqStatus = "挂起"
/** 需求列表页状态筛选选项：流水状态 + 流水外标记状态。 */
export const REQ_LIST_STATUSES: ReqStatus[] = [...REQ_FLOW_STATUSES, SUSPEND_STATUS]
export const ISSUE_STATUSES: ReqStatus[] = ["排查中", "已定位", "已修复", "已复盘", "已关闭"]
/** 线上问题页状态筛选选项：专用状态机 + 「常规流程中」伪状态（问题正走常规需求流程或尚未登记状态）。 */
export const ISSUE_STATUS_OPTIONS: string[] = [...ISSUE_STATUSES, "常规流程中"]
/** 子需求状态机：父集成模型（需求创建→开发中→已合入）或整合发布模型（…→发布就绪→已合入→已发布，独立发布进度）。 */
export const SUB_REQ_STATUSES: ReqStatus[] = ["需求创建", "开发中", "自测中", "测试中", "发布就绪", "已合入", "已发布", "已取消"]
export const REQ_STATUSES: ReqStatus[] = [...REQ_FLOW_STATUSES, ...ISSUE_STATUSES]
export const REQ_CATEGORIES: ReqCategory[] = ["需求", "线上问题", "测试问题"]
export const REQ_SOURCES = ["产品推动", "开发推动"] as const
export type ReqSource = (typeof REQ_SOURCES)[number]

export const statusMeta: Record<string, { color: string; soft: string }> = {
  需求澄清: { color: "#94a3b8", soft: "rgba(148, 163, 184, 0.14)" },
  开发中: { color: "#22d3ee", soft: "rgba(34, 211, 238, 0.14)" },
  自测中: { color: "#3b82f6", soft: "rgba(59, 130, 246, 0.14)" },
  测试中: { color: "#a855f7", soft: "rgba(168, 85, 247, 0.14)" },
  人工核查: { color: "#fb923c", soft: "rgba(251, 146, 60, 0.14)" },
  人工复测: { color: "#fb923c", soft: "rgba(251, 146, 60, 0.14)" },
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
  // 子需求状态
  需求创建: { color: "#94a3b8", soft: "rgba(148, 163, 184, 0.14)" },
  已合入: { color: "#34d399", soft: "rgba(52, 211, 153, 0.14)" },
  已发布: { color: "#22c55e", soft: "rgba(34, 197, 94, 0.14)" },
  // 流水外标记状态
  挂起: { color: "#f59e0b", soft: "rgba(245, 158, 11, 0.14)" },
}
