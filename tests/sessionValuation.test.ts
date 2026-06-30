import { describe, it, beforeEach } from "node:test"
import assert from "node:assert/strict"
import { EventEmitter } from "node:events"
import { mkdtemp, writeFile } from "node:fs/promises"
import { tmpdir } from "node:os"
import { join } from "node:path"

import {
  scoreSessionMetadata,
  scoreSessionContent,
  scoreSession,
  VALUATION_THRESHOLD,
} from "../src/sessionValuation.ts"
import type { SessionInfo } from "../src/sessions.ts"

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function makeSession(overrides: Partial<SessionInfo> = {}): SessionInfo {
  return {
    id: "ses_test123abc",
    title: "test session",
    created: Date.now() - 60_000,
    updated: Date.now() - 30_000,
    projectId: "test",
    directory: "/tmp/test",
    status: "idle",
    source: "db",
    ...overrides,
  }
}

/** Create a temp file to use as a fake DB path (must exist for existsSync). */
let _fakeDbPath: string
beforeEach(async () => {
  const dir = await mkdtemp(join(tmpdir(), "valtest-"))
  _fakeDbPath = join(dir, "fake.db")
  await writeFile(_fakeDbPath, "")
})

/** Build a fake spawn that returns a single JSON row of content query results. */
function makeFakeSpawn(row: Record<string, unknown>) {
  return (() => {
    const proc = new EventEmitter() as any
    proc.stdout = new EventEmitter()
    proc.stderr = new EventEmitter()
    proc.stdin = {
      write: () => {
        process.nextTick(() => {
          proc.stdout.emit("data", Buffer.from(JSON.stringify([row])))
          proc.emit("close", 0)
        })
        return true
      },
      end: () => {},
    }
    proc.kill = () => {}
    return proc
  }) as unknown as typeof import("node:child_process").spawn
}

function makeFailingSpawn() {
  return (() => {
    const proc = new EventEmitter() as any
    proc.stdout = new EventEmitter()
    proc.stderr = new EventEmitter()
    proc.stdin = { write: () => true, end: () => {} }
    proc.kill = () => {}
    process.nextTick(() => proc.emit("close", 1))
    return proc
  }) as unknown as typeof import("node:child_process").spawn
}

// ---------------------------------------------------------------------------
// Tier 1: Metadata scoring
// ---------------------------------------------------------------------------

describe("scoreSessionMetadata", () => {
  it("fork sessions always score 0", () => {
    const session = makeSession({
      parentId: "ses_parent001",
      tokensInput: 200_000,
      tokensOutput: 100_000,
    })
    const result = scoreSessionMetadata(session)
    assert.equal(result.score, 0)
    assert.ok(result.reasons.some((r) => r.includes("fork")))
    assert.equal(result.signals.length, 0)
  })

  it("non-fork session gets base +5", () => {
    const session = makeSession()
    const result = scoreSessionMetadata(session)
    assert.ok(result.score >= 5)
    assert.ok(result.reasons.some((r) => r.includes("非 fork")))
  })

  it("high token volume adds +10", () => {
    const session = makeSession({
      tokensInput: 80_000,
      tokensOutput: 30_000,
    })
    const result = scoreSessionMetadata(session)
    assert.ok(result.reasons.some((r) => r.includes("+10")))
    assert.ok(result.score >= 15)
  })

  it("medium token volume adds +5", () => {
    const session = makeSession({
      tokensInput: 20_000,
      tokensOutput: 15_000,
    })
    const result = scoreSessionMetadata(session)
    assert.ok(result.reasons.some((r) => r.includes("+5")))
    assert.ok(result.score >= 10)
  })

  it("long duration adds +5", () => {
    const now = Date.now()
    const session = makeSession({
      created: now - 45 * 60_000,
      updated: now - 5_000,
    })
    const result = scoreSessionMetadata(session)
    assert.ok(result.reasons.some((r) => r.includes("+5")))
  })

  it("title with fix/bug keywords adds signal", () => {
    const session = makeSession({ title: "fix memory leak bug" })
    const result = scoreSessionMetadata(session)
    assert.ok(result.signals.includes("debugging"))
    assert.ok(result.reasons.some((r) => r.includes("修复/bug")))
  })

  it("title with skill keyword adds signal", () => {
    const session = makeSession({ title: "create new skill for auth" })
    const result = scoreSessionMetadata(session)
    assert.ok(result.signals.includes("skill"))
  })

  it("title with correction keywords adds signal", () => {
    const session = makeSession({ title: "修正文档中的误导信息" })
    const result = scoreSessionMetadata(session)
    assert.ok(result.signals.includes("correction"))
  })

  it("title keyword hits capped at 2", () => {
    const session = makeSession({
      title: "fix skill knowledge 踩坑",
    })
    const result = scoreSessionMetadata(session)
    const titleHits = result.reasons.filter((r) => r.includes("+3") && r.includes("标题"))
    assert.ok(titleHits.length <= 2)
  })

  it("orchestrator agent adds +3", () => {
    const session = makeSession({ agent: "orchestrator" })
    const result = scoreSessionMetadata(session)
    assert.ok(result.reasons.some((r) => r.includes("orchestrator")))
  })
})

// ---------------------------------------------------------------------------
// Tier 2: Content scoring
// ---------------------------------------------------------------------------

describe("scoreSessionContent", () => {
  it("returns 0 when DB query fails", async () => {
    const result = await scoreSessionContent("ses_test123", {
      dbPath: _fakeDbPath,
      sqliteFn: makeFailingSpawn(),
    })
    assert.equal(result.score, 0)
    assert.equal(result.reasons.length, 0)
  })

  it("detects verification keywords in text sample", async () => {
    const row = {
      part_count: 50,
      tool_count: 20,
      code_tool_count: 10,
      text_sample: '{"type":"text","text":"查询 Kibana 日志确认问题"}',
    }
    const result = await scoreSessionContent("ses_test123", {
      dbPath: _fakeDbPath,
      sqliteFn: makeFakeSpawn(row),
    })
    assert.ok(result.signals.includes("verification"))
    assert.ok(result.reasons.some((r) => r.includes("验证")))
    assert.ok(result.score >= 15)
  })

  it("detects correction keywords", async () => {
    const row = {
      part_count: 30,
      tool_count: 10,
      code_tool_count: 5,
      text_sample: '{"type":"text","text":"发现现有文档有误导，需要修正"}',
    }
    const result = await scoreSessionContent("ses_test123", {
      dbPath: _fakeDbPath,
      sqliteFn: makeFakeSpawn(row),
    })
    assert.ok(result.signals.includes("correction"))
  })

  it("detects skill keywords", async () => {
    const row = {
      part_count: 20,
      tool_count: 8,
      code_tool_count: 3,
      text_sample: '{"type":"text","text":"创建新的 SKILL.md 文件"}',
    }
    const result = await scoreSessionContent("ses_test123", {
      dbPath: _fakeDbPath,
      sqliteFn: makeFakeSpawn(row),
    })
    assert.ok(result.signals.includes("skill"))
  })

  it("detects knowledge keywords", async () => {
    const row = {
      part_count: 15,
      tool_count: 5,
      code_tool_count: 2,
      text_sample: '{"type":"text","text":"记录到 knowledge 目录"}',
    }
    const result = await scoreSessionContent("ses_test123", {
      dbPath: _fakeDbPath,
      sqliteFn: makeFakeSpawn(row),
    })
    assert.ok(result.signals.includes("knowledge"))
  })

  it("detects debugging keywords", async () => {
    const row = {
      part_count: 40,
      tool_count: 15,
      code_tool_count: 8,
      text_sample: '{"type":"text","text":"定位到根因是连接池泄漏"}',
    }
    const result = await scoreSessionContent("ses_test123", {
      dbPath: _fakeDbPath,
      sqliteFn: makeFakeSpawn(row),
    })
    assert.ok(result.signals.includes("debugging"))
  })

  it("verification + correction bonus adds +10", async () => {
    const row = {
      part_count: 50,
      tool_count: 20,
      code_tool_count: 10,
      text_sample: '{"type":"text","text":"通过 Kibana 查询确认现有文档有误导"}',
    }
    const result = await scoreSessionContent("ses_test123", {
      dbPath: _fakeDbPath,
      sqliteFn: makeFakeSpawn(row),
    })
    assert.ok(result.signals.includes("verification"))
    assert.ok(result.signals.includes("correction"))
    assert.ok(result.reasons.some((r) => r.includes("双重信号")))
    assert.ok(result.score >= 40)
  })

  it("high code tool count adds +10", async () => {
    const row = {
      part_count: 100,
      tool_count: 50,
      code_tool_count: 20,
      text_sample: "",
    }
    const result = await scoreSessionContent("ses_test123", {
      dbPath: _fakeDbPath,
      sqliteFn: makeFakeSpawn(row),
    })
    assert.ok(result.reasons.some((r) => r.includes("+10")))
  })

  it("medium code tool count adds +5", async () => {
    const row = {
      part_count: 50,
      tool_count: 15,
      code_tool_count: 7,
      text_sample: "",
    }
    const result = await scoreSessionContent("ses_test123", {
      dbPath: _fakeDbPath,
      sqliteFn: makeFakeSpawn(row),
    })
    assert.ok(result.reasons.some((r) => r.includes("+5")))
  })

  it("returns 0 when DB file does not exist", async () => {
    const result = await scoreSessionContent("ses_test123", {
      dbPath: "/nonexistent/path/fake.db",
    })
    assert.equal(result.score, 0)
    assert.equal(result.signals.length, 0)
  })
})

// ---------------------------------------------------------------------------
// Combined two-tier scoring
// ---------------------------------------------------------------------------

describe("scoreSession", () => {
  it("skips content query for fork sessions (score 0, below gate)", async () => {
    const session = makeSession({
      parentId: "ses_parent001",
      tokensInput: 100,
      tokensOutput: 50,
    })
    const result = await scoreSession(session, {
      dbPath: _fakeDbPath,
      sqliteFn: makeFailingSpawn(),
    })
    assert.equal(result.contentScored, false)
    assert.equal(result.contentScore, 0)
    assert.equal(result.metadataScore, 0)
    assert.equal(result.score, 0)
  })

  it("runs content query when metadata score passes gate", async () => {
    const session = makeSession({
      tokensInput: 50_000,
      tokensOutput: 20_000,
      created: Date.now() - 40 * 60_000,
      updated: Date.now() - 5_000,
      title: "fix ES log query bug",
    })
    const row = {
      part_count: 80,
      tool_count: 30,
      code_tool_count: 15,
      text_sample: '{"type":"text","text":"通过 Kibana 查询确认根因是缓存问题"}',
    }
    const result = await scoreSession(session, {
      dbPath: _fakeDbPath,
      sqliteFn: makeFakeSpawn(row),
    })
    assert.equal(result.contentScored, true)
    assert.ok(result.contentScore > 0)
    assert.ok(result.score > result.metadataScore)
    assert.ok(result.signals.length >= 2)
  })

  it("skipContent option forces metadata-only", async () => {
    const session = makeSession({
      tokensInput: 100_000,
      tokensOutput: 50_000,
    })
    const result = await scoreSession(session, { skipContent: true })
    assert.equal(result.contentScored, false)
    assert.equal(result.contentScore, 0)
  })
})

describe("VALUATION_THRESHOLD", () => {
  it("is a positive number", () => {
    assert.ok(typeof VALUATION_THRESHOLD === "number")
    assert.ok(VALUATION_THRESHOLD > 0)
    assert.ok(VALUATION_THRESHOLD <= 100)
  })
})
