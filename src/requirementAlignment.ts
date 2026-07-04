/**
 * Requirement alignment templates for the business-only intake phase.
 *
 * Public surface: ALIGNMENT_FILE, PRD_FILE, ALIGNMENT_TEMPLATE,
 * PRD_TEMPLATE.
 * Constraints / safety: pure constants only; no filesystem or secret access.
 * Read-this-with: src/requirements.ts and src/server.tsx.
 */

/** Requirement alignment brief filename used across dashboard flows. */
export const ALIGNMENT_FILE = "alignment.md"

/** PRD source-trace filename; later phases should not treat it as primary context. */
export const PRD_FILE = "prd.md"

/**
 * Standard business-only document for the `需求对齐` phase.
 * It intentionally excludes implementation/design sections so this phase
 * stays focused on what product/business actually needs.
 */
export const ALIGNMENT_TEMPLATE = `# 需求对齐

用于：把产品/业务真实诉求转成后续 AI 和人都能直接使用的业务需求说明。
原则：只记录业务目标、边界、规则、场景和验收口径；不写代码方案、技术设计或分支计划。

## 1. 业务目标
- 要解决的问题：待补充
- 期望业务结果：待补充
- 成功衡量口径：待补充

## 2. 用户与场景
| 用户/角色 | 当前痛点 | 触发场景 | 期望变化 |
| --- | --- | --- | --- |
| 待补充 | 待补充 | 待补充 | 待补充 |

## 3. 需求范围
### 本次包含
- 待补充

### 本次不包含
- 待补充

## 4. 业务规则
| 规则 | 说明 | 来源/确认人 | 状态 |
| --- | --- | --- | --- |
| 待补充 | 待补充 | 待补充 | 待确认 |

## 5. 业务流程
### 正常流程
1. 待补充

### 异常/边界场景
- 待补充

## 6. 验收口径
| 验收点 | 输入/前置条件 | 期望结果 | 备注 |
| --- | --- | --- | --- |
| 待补充 | 待补充 | 待补充 | 待补充 |

## 7. 依赖与约束
- 业务依赖：待补充
- 时间/上线约束：待补充
- 数据/权限/组织约束：待补充

## 8. 未决问题
| 问题 | 负责人 | 截止时间 | 当前状态 |
| --- | --- | --- | --- |
| 待补充 | 待补充 | 待补充 | 待确认 |

## 9. PRD 转化记录
- PRD 来源：见 prd.md
- 已转化结论：待补充
- 未采纳/待确认内容：待补充
`

/**
 * Source-trace template for product PRDs, especially Feishu docs.
 * This keeps the original source discoverable while preventing raw PRD
 * text from becoming the default context in later phases.
 */
export const PRD_TEMPLATE = `# PRD 来源追溯

用于：记录产品 PRD/飞书文档来源和转化摘要。后续阶段默认以 alignment.md 为准，只有需要核对原始描述时才回看本文。

## 来源信息
- PRD 标题：待补充
- 来源类型：飞书文档 / 附件 / 用户口述 / 其他
- 链接或位置：待补充
- 产品/业务联系人：待补充
- 获取时间：待补充
- PRD 更新时间：待补充

## 原文摘要
- 背景：待补充
- 目标：待补充
- 范围：待补充
- 关键规则：待补充
- 验收口径：待补充

## 已转化到 alignment.md
- 待补充

## 仍需回看 PRD 的内容
- 待补充；如果没有，写“无”。
`
