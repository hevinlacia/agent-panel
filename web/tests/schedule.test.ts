/**
 * 排期算法（web/src/lib/schedule.ts）推荐提测日测试 — bun test 运行：`bun test web/tests`。
 * 固定 today 确定性断言：2026-01-05（周一）；容量默认 6h/天、跳过周末、预留 2 个工作日（除非用例覆盖）。
 */
import { describe, expect, test } from "bun:test"
import {
  DEFAULT_CONFIG,
  type ScheduleReqInput,
  addWorkdaysAfter,
  recommendSummary,
  utilization,
} from "../src/lib/schedule"

const TODAY = new Date("2026-01-05T00:00:00") // 周一
// 2026-01-05 起的工作日：01-05(一) 01-06(二) 01-07(三) 01-08(四) 01-09(五) | 01-12(一) 01-13(二) 01-14(三) 01-15(四) 01-16(五)

function req(patch: Partial<ScheduleReqInput>): ScheduleReqInput {
  return {
    id: "REQ-1",
    title: "t",
    status: "开发中",
    startDate: null,
    submitTestDate: null,
    promiseDate: null,
    isHotfix: false,
    remainingHours: 8,
    hoursSource: "estimate",
    ...patch,
  }
}

function utilOf(reqs: ScheduleReqInput[], patch: Partial<typeof DEFAULT_CONFIG> = {}) {
  const config = { ...DEFAULT_CONFIG, ...patch }
  return { config, util: utilization(reqs, config, TODAY) }
}

describe("addWorkdaysAfter", () => {
  test("完成日之后第 n 个工作日，跳过周末", () => {
    // 周四完成 → +1 = 周五；+2 = 下周一
    expect(addWorkdaysAfter("2026-01-08", 1, true)).toBe("2026-01-09")
    expect(addWorkdaysAfter("2026-01-08", 2, true)).toBe("2026-01-12")
    // 周五完成 → +1 跳过周六日 = 下周一
    expect(addWorkdaysAfter("2026-01-09", 1, true)).toBe("2026-01-12")
    // 不跳过周末时按自然日推进
    expect(addWorkdaysAfter("2026-01-09", 2, false)).toBe("2026-01-11")
  })
})

describe("推荐提测日（容量填充 + 预留）", () => {
  test("单需求 8h、每日容量 6h → 第 2 个工作日完成，预留 2 天 → 第 4 个工作日提测", () => {
    const { config, util } = utilOf([req({})])
    expect(util.items).toHaveLength(1)
    const it = util.items[0]
    expect(it.devFinishDate).toBe("2026-01-06") // 6h + 2h
    expect(it.recommendedDeadline).toBe("2026-01-08") // 完成后 +2 个工作日
    expect(it.effDeadline).toBe("2026-01-08") // 默认排期即按推荐值
    expect(it.virtualDeadline).toBe(true)
    expect(it.overdue).toBe(false)
    expect(it.spanDays).toBe(4)
    expect(it.dailyLoad).toBeCloseTo(8 / 4)
    expect(recommendSummary(it, config)).toContain("2026-01-08")
  })

  test("大工时跨周末：30h → 周五完成，预留 2 天跨周末 → 下周二提测", () => {
    const { util } = utilOf([req({ remainingHours: 30 })])
    const it = util.items[0]
    expect(it.devFinishDate).toBe("2026-01-09") // 周五
    expect(it.recommendedDeadline).toBe("2026-01-13") // 下周二（跳过周末）
  })

  test("已登记提测时间的需求不受推荐影响", () => {
    const { util } = utilOf([req({ submitTestDate: "2026-01-30" })])
    const it = util.items[0]
    expect(it.effDeadline).toBe("2026-01-30")
    expect(it.virtualDeadline).toBe(false)
    expect(it.recommendedDeadline).toBeNull()
    expect(it.devFinishDate).toBeNull()
    expect(recommendSummary(it, DEFAULT_CONFIG)).toBeNull()
  })

  test("bufferDays = 0 时推荐提测日 = 开发完成日", () => {
    const { util } = utilOf([req({})], { submitBufferDays: 0 })
    const it = util.items[0]
    expect(it.devFinishDate).toBe("2026-01-06")
    expect(it.recommendedDeadline).toBe("2026-01-06")
  })

  test("startDate 未来：投入日 = startDate，不早于今天开工", () => {
    const { util } = utilOf([req({ startDate: "2026-01-12", remainingHours: 4 })])
    const it = util.items[0]
    expect(it.startPoint).toBe("2026-01-12")
    expect(it.devFinishDate).toBe("2026-01-12") // 当天容量 6h ≥ 4h
    expect(it.recommendedDeadline).toBe("2026-01-14")
    expect(it.spanDays).toBe(3)
  })

  test("多个未登记需求贪心依次分配：大工时先占容量，后续需求在其之上扣除", () => {
    // A 16h（大，先处理）；B 8h；容量 6h/天；buffer 0 简化断言
    const { util } = utilOf([
      req({ id: "REQ-B", remainingHours: 8 }),
      req({ id: "REQ-A", remainingHours: 16 }),
    ], { submitBufferDays: 0 })
    const a = util.items.find((it) => it.id === "REQ-A")!
    const b = util.items.find((it) => it.id === "REQ-B")!
    // A：6+6+4 → 01-07 完成
    expect(a.devFinishDate).toBe("2026-01-07")
    expect(a.recommendedDeadline).toBe("2026-01-07")
    // A 负载按推荐区间均摊生效（16h / 3 工作日 ≈ 5.33h/天），B 只能吃剩余容量：
    // 01-05..01-07 每天 0.67h，01-08 起 A 区间外 → B 在 01-08 完成
    expect(b.devFinishDate).toBe("2026-01-08")
    expect(b.recommendedDeadline).toBe("2026-01-08")
  })

  test("其他在制负载（已登记）挤压未登记需求的推荐日", () => {
    // 已登记需求 R 占满容量（每天摊 6h 压满 6h 容量）；未登记需求 8h 只能从 R 提测日之后开工
    const { util } = utilOf([
      req({ id: "REQ-R", submitTestDate: "2026-01-16", remainingHours: 30 }), // 30h / 10 工作日 = 3h/天
      req({ id: "REQ-X", remainingHours: 8 }),
    ])
    const x = util.items.find((it) => it.id === "REQ-X")!
    // X 每天 avail = 6-3 = 3h → 3 个工作日（01-05/06/07）完成 → +2 预留 = 01-09
    expect(x.devFinishDate).toBe("2026-01-07")
    expect(x.recommendedDeadline).toBe("2026-01-09")
  })

  test("容量占满时推荐被防御上限截断（不死循环）", () => {
    // 已登记需求占满全部容量 520+ 工作日：未登记需求拿不到任何容量，finish = 游标截断处
    const far = addWorkdaysAfter("2026-01-05", 400, true)
    const { util } = utilOf([
      req({ id: "REQ-FULL", submitTestDate: far, remainingHours: 400 * 6 }),
      req({ id: "REQ-X", remainingHours: 8 }),
    ])
    const x = util.items.find((it) => it.id === "REQ-X")!
    expect(x.devFinishDate).toBeTruthy()
    expect(x.recommendedDeadline).toBeTruthy()
  })

  test("逾期已登记需求全额压今天，未登记需求从明天起获容量", () => {
    // R 提测日在过去（逾期）：负载全压今天；X 今天 avail = 0，01-06 起 6h+2h 两天完成
    const { util } = utilOf([
      req({ id: "REQ-R", submitTestDate: "2026-01-02", remainingHours: 12 }),
      req({ id: "REQ-X", remainingHours: 8 }),
    ])
    const x = util.items.find((it) => it.id === "REQ-X")!
    expect(x.devFinishDate).toBe("2026-01-07")
    expect(x.recommendedDeadline).toBe("2026-01-09")
  })
})
