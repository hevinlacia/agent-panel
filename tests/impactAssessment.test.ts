/**
 * Tests for `src/impactAssessment.ts`.
 */

import { test } from "node:test"
import { strict as assert } from "node:assert"

import {
  IMPACT_TEMPLATE,
  buildImpactAssessment,
} from "../src/impactAssessment.ts"

test("buildImpactAssessment marks missing content incomplete", () => {
  const assessment = buildImpactAssessment()
  assert.equal(assessment.exists, false)
  assert.equal(assessment.complete, false)
  assert.equal(assessment.riskLevel, "未评估")
  assert.ok(assessment.missingSections.includes("风险等级"))
})

test("buildImpactAssessment parses completed WMS impact assessment", () => {
  const content = `# 需求影响面评估

## 风险等级
- 等级：P1 高风险
- 判断依据：改出库复核状态推进

## 核心链路
- 出库复核完成后触发回传
- MQ 消费失败会影响库存同步

## 影响入口
- API/Controller：finishCheck
- MQ 消费/生产：wms-shipment-upload-topic

## 数据影响
- 核心表：shipment_header
- 状态字段/数量字段：shipment_status

## 阻塞风险
- 可能阻塞主流程：复核完成后状态无法推进
- 异常时行为：失败可重试并记录业务主键

## 自测清单
- [ ] 正常主流程：创建出库单到复核完成
- [ ] 重复消费幂等验证

## 回滚方案
- 开关：review.new-flow.enabled=false
- 回滚步骤：关闭开关后走原链路
`
  const assessment = buildImpactAssessment(content)
  assert.equal(assessment.exists, true)
  assert.equal(assessment.complete, true)
  assert.match(assessment.riskLevel, /P1 高风险/)
  assert.ok(assessment.coreFlows.some((x) => x.includes("出库复核")))
  assert.ok(assessment.blockers.some((x) => x.includes("阻塞主流程")))
  assert.ok(assessment.testItems.some((x) => x.includes("正常主流程")))
})

test("IMPACT_TEMPLATE starts incomplete so users fill it deliberately", () => {
  const assessment = buildImpactAssessment(IMPACT_TEMPLATE)
  assert.equal(assessment.exists, true)
  assert.equal(assessment.complete, false)
  assert.ok(assessment.missingSections.length > 0)
})
