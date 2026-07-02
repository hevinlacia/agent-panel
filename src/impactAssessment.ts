/**
 * Requirement impact assessment parser and template.
 *
 * Role: turns `impact.md` into the dashboard's pre-coding safety gate
 * for WMS/core-business change risk.
 * Public surface: IMPACT_FILE, IMPACT_TEMPLATE, buildImpactAssessment.
 * Constraints / safety: pure functions only; callers own file I/O.
 * Read-this-with: src/requirements.ts and src/server.tsx.
 */

/** Shared filename for the pre-coding WMS impact assessment. */
export const IMPACT_FILE = "impact.md"

const REQUIRED_SECTIONS = [
  "风险等级",
  "核心链路",
  "影响入口",
  "数据影响",
  "阻塞风险",
  "自测清单",
  "回滚方案",
] as const

const PLACEHOLDER_RE = /(?:待补充|未评估|暂无|无\s*$|TODO|TBD|unknown)/i

const RISK_LEVEL_RE = /(?:P0|P1|P2|P3|高|中|低)风险?|risk\s*[:：]?\s*(?:high|medium|low)/i

const CORE_FLOW_RE = /(?:入库|收货|上架|库存|拣货|复核|打包|称重|发运|出库|波次|盘点|调拨|取消|回滚|回传|OMS|BMS|PDA|MQ|DTS|定时任务|回调)/i

const BLOCKER_RE = /(?:阻塞|中断|卡住|失败不可重试|不可补偿|影响主流程|数据污染|库存不准|状态错乱)/i

const TEST_RE = /(?:自测|回归|验证|测试|用例|链路|场景|主流程|重复|幂等|异常|边界|回滚)/i

/**
 * Dashboard-ready summary of impact.md completeness and high-risk signals.
 * Missing sections are intentionally explicit so the requirement page can
 * act as a safety gate before agents start coding.
 */
export interface ImpactAssessment {
  exists: boolean
  complete: boolean
  riskLevel: string
  missingSections: string[]
  coreFlows: string[]
  blockers: string[]
  testItems: string[]
  sections: Record<string, string>
}

/** Standard template written when a requirement has no impact.md yet. */
export const IMPACT_TEMPLATE = `# 需求影响面评估

用于：编码前判断需求是否可能阻塞 WMS 核心业务链路，并沉淀给自测/测试使用。

## 风险等级
- 等级：未评估
- 判断依据：待补充

## 核心链路
- [ ] 入库 / 收货 / 上架
- [ ] 库存 / 库存调整 / 库存同步
- [ ] 出库单创建 / 分配库存 / 波次
- [ ] 拣货 / 复核 / 打包 / 称重 / 发运
- [ ] OMS/BMS/奇门/外部回传
- [ ] MQ / DTS / 定时任务 / 外部回调
- 说明：待补充

## 影响入口
- API/Controller：待补充
- MQ 消费/生产：待补充
- 定时任务/DTS/回调：待补充
- 前端菜单/按钮/PDA：待补充

## 数据影响
- 核心表：待补充
- 状态字段/数量字段：待补充
- 事务/锁/幂等/唯一索引：待补充

## 阻塞风险
- 是否可能阻塞主流程：未评估
- 异常时行为：待补充
- 可重试/可补偿：待补充

## 自测清单
- [ ] 正常主流程
- [ ] 重复请求 / 重复消费 / 幂等
- [ ] 异常状态 / 边界状态
- [ ] 回滚或关闭开关后的行为

## 回滚方案
- 开关：待补充
- 回滚步骤：待补充
- 补偿/重扫方案：待补充
`

/**
 * Parse an impact.md body into a dashboard summary.
 * Empty or missing content returns an incomplete assessment with all
 * required sections missing so the UI can show a clear pre-coding gate.
 */
export function buildImpactAssessment(content?: string): ImpactAssessment {
  const text = (content ?? "").trim()
  if (!text) {
    return {
      exists: false,
      complete: false,
      riskLevel: "未评估",
      missingSections: [...REQUIRED_SECTIONS],
      coreFlows: [],
      blockers: [],
      testItems: [],
      sections: {},
    }
  }

  const sections = parseSections(text)
  const missingSections = REQUIRED_SECTIONS.filter((name) => !hasMeaningfulSection(sections[name]))
  const riskLevel = extractRiskLevel(text, sections["风险等级"])
  const coreFlows = extractLines(sections["核心链路"] ?? text, CORE_FLOW_RE)
  const blockers = extractLines(sections["阻塞风险"] ?? text, BLOCKER_RE)
  const testItems = extractLines(sections["自测清单"] ?? text, TEST_RE)
  const complete = missingSections.length === 0 && riskLevel !== "未评估" && testItems.length > 0

  return {
    exists: true,
    complete,
    riskLevel,
    missingSections,
    coreFlows,
    blockers,
    testItems,
    sections,
  }
}

function parseSections(text: string): Record<string, string> {
  const sections: Record<string, string> = {}
  const lines = text.replace(/\r\n/g, "\n").split("\n")
  let current = ""
  let body: string[] = []

  const flush = () => {
    if (current) sections[current] = body.join("\n").trim()
  }

  for (const line of lines) {
    const heading = line.match(/^##\s+(.+?)\s*$/)
    if (heading) {
      flush()
      current = heading[1].trim()
      body = []
      continue
    }
    if (current) body.push(line)
  }
  flush()
  return sections
}

function hasMeaningfulSection(section: string | undefined): boolean {
  if (!section) return false
  const meaningful = section
    .split("\n")
    .map((line) => line.trim().replace(/^[-*]\s*\[[ xX]\]\s*/, ""))
    .filter((line) => !PLACEHOLDER_RE.test(line))
  return meaningful.length > 0
}

function extractRiskLevel(text: string, riskSection?: string): string {
  const source = riskSection || text
  const explicit = source.match(/(?:等级|风险等级|risk)\s*[:：]\s*([^\n]+)/i)
  if (explicit) {
    const value = explicit[1].replace(/^[-*]\s*/, "").trim()
    if (value && !PLACEHOLDER_RE.test(value)) return value
  }
  const match = source.match(RISK_LEVEL_RE)
  return match ? match[0].trim() : "未评估"
}

function extractLines(source: string, pattern: RegExp): string[] {
  const out: string[] = []
  for (const raw of source.split("\n")) {
    const line = raw.trim()
    if (!line || line.startsWith("#") || /^\|\s*-/.test(line)) continue
    if (PLACEHOLDER_RE.test(line)) continue
    if (pattern.test(line)) out.push(line)
  }
  return [...new Set(out)].slice(0, 8)
}
