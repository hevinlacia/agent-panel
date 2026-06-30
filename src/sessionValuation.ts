/**
 * Two-tier session value scorer for experience summarization.
 *
 * Role: identify OpenCode sessions that are likely to contain reusable
 * experience — real debugging with log/DB verification, skill discoveries,
 * knowledge corrections, or knowledge recording. Uses a cheap metadata
 * pass first (token volume, duration, agent type, title keywords) and a
 * more expensive SQLite content pass second (tool-call counts, keyword
 * presence in message parts) to produce a score and human-readable
 * reasons. Sessions above a configurable threshold are candidates for
 * auto-marking by `src/autoValuation.ts`.
 *
 * Public surface:
 *   - scoreSessionMetadata(session): tier-1 score from SessionInfo only
 *   - scoreSessionContent(sessionId): tier-2 score from opencode.db parts
 *   - scoreSession(session, opts?): combined two-tier score
 *   - VALUATION_THRESHOLD: default minimum score for auto-marking
 *   - type ValuationResult, type ValueSignal
 *
 * Constraints / safety:
 *   - Read-only. Never writes to opencode.db.
 *   - SQLite queries use `.param set` named-bind protocol (same pattern
 *     as `src/forkSalvage.ts`); session id is the only runtime input and
 *     flows through a parameter placeholder, never string concatenation.
 *   - The LIKE patterns in the content query are static string literals
 *     (no user input), matching the established pattern in forkSalvage.ts.
 *   - Only `node:` built-ins.
 *
 * Read-this-with:
 *   - `src/autoValuation.ts` (the background worker that calls scoreSession).
 *   - `src/forkSalvage.ts` (the SQLite-via-CLI pattern this module mirrors).
 *   - `src/sessions.ts` (SessionInfo type and scanSessions).
 */

import { spawn } from "node:child_process"
import { existsSync } from "node:fs"
import { homedir } from "node:os"
import { join } from "node:path"
import type { SessionInfo } from "./sessions.ts"

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/** A named category of value signal detected in a session. */
export type ValueSignal =
  | "verification"   // session queried real systems (ES, Kibana, MySQL, Grafana, Archery)
  | "correction"     // existing knowledge/skill was wrong or incomplete
  | "skill"          // skill creation, improvement, or discovery
  | "knowledge"      // knowledge document recording or updating
  | "debugging"      // root-cause analysis with evidence

/** Result of scoring a single session. */
export interface ValuationResult {
  sessionId: string
  /** 0–100. Higher = more likely to contain reusable experience. */
  score: number
  /** Human-readable reasons explaining the score (for UI display). */
  reasons: string[]
  /** Which signal categories were detected. */
  signals: ValueSignal[]
  /** Tier-1 metadata sub-score. */
  metadataScore: number
  /** Tier-2 content sub-score (0 when content query was skipped). */
  contentScore: number
  /** Whether the session passed tier-1 and was content-scored. */
  contentScored: boolean
}

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

export const VALUATION_THRESHOLD = 25

const DEFAULT_DB_PATH = join(
  homedir(),
  ".local",
  "share",
  "opencode",
  "opencode.db",
)

const SQLITE_TIMEOUT_MS = 5_000
const STDOUT_CAP_BYTES = 256 * 1024

/**
 * Minimum tier-1 metadata score required to proceed to the tier-2
 * content query. Sessions below this are almost certainly not worth
 * the SQLite round-trip.
 */
const METADATA_GATE = 5

// ---------------------------------------------------------------------------
// Tier 1: Metadata scoring (SessionInfo only — cheap, no DB access)
// ---------------------------------------------------------------------------

/** Title keywords that hint at value, mapped to their signal category. */
const TITLE_KEYWORDS: { pattern: RegExp; signal: ValueSignal; reason: string }[] = [
  { pattern: /fix|修复|bug/i, signal: "debugging", reason: "标题包含修复/bug 关键词" },
  { pattern: /skill|技能/i, signal: "skill", reason: "标题包含 skill 关键词" },
  { pattern: /knowledge|知识|沉淀/i, signal: "knowledge", reason: "标题包含知识/沉淀关键词" },
  { pattern: /迁移|重构|refactor/i, signal: "knowledge", reason: "标题包含迁移/重构关键词" },
  { pattern: /修正|纠正|误导|踩坑/i, signal: "correction", reason: "标题包含修正/踩坑关键词" },
  { pattern: /debug|调试|根因|root.?cause/i, signal: "debugging", reason: "标题包含调试/根因关键词" },
]

/**
 * Score a session using only its metadata (SessionInfo). This is the
 * cheap first pass — no SQLite access needed.
 *
 * The score is a sum of:
 *   - Not a fork: +5 (forks are subagent work, already captured by parent)
 *   - Token volume: up to +10
 *   - Duration: up to +5
 *   - Agent type: up to +3
 *   - Title keyword hits: +3 each (capped at 2 hits)
 */
export function scoreSessionMetadata(session: SessionInfo): {
  score: number
  reasons: string[]
  signals: ValueSignal[]
} {
  let score = 0
  const reasons: string[] = []
  const signals = new Set<ValueSignal>()

  // Forks are subagent sessions — their value is captured by the parent.
  if (session.parentId) {
    return { score: 0, reasons: ["fork 子 session，跳过"], signals: [] }
  }
  score += 5
  reasons.push("非 fork session +5")

  // Token volume — proxy for work complexity.
  const totalTokens = (session.tokensInput ?? 0) + (session.tokensOutput ?? 0)
  if (totalTokens > 100_000) {
    score += 10
    reasons.push(`token 量 ${Math.round(totalTokens / 1000)}k +10`)
  } else if (totalTokens > 30_000) {
    score += 5
    reasons.push(`token 量 ${Math.round(totalTokens / 1000)}k +5`)
  }

  // Duration — proxy for depth of work.
  const durationMs = (session.updated || 0) - (session.created || 0)
  if (durationMs > 30 * 60_000) {
    score += 5
    reasons.push(`时长 ${Math.round(durationMs / 60_000)}min +5`)
  } else if (durationMs > 10 * 60_000) {
    score += 3
    reasons.push(`时长 ${Math.round(durationMs / 60_000)}min +3`)
  }

  // Agent type — orchestrator/general sessions are more likely to
  // contain cross-cutting decisions.
  const agent = session.agent ?? ""
  if (agent === "orchestrator" || agent === "general" || agent === "") {
    score += 3
    reasons.push(`agent=${agent || "(default)"} +3`)
  }

  // Title keyword scan.
  let titleHits = 0
  for (const kw of TITLE_KEYWORDS) {
    if (titleHits >= 2) break
    if (kw.pattern.test(session.title)) {
      score += 3
      reasons.push(`${kw.reason} +3`)
      signals.add(kw.signal)
      titleHits++
    }
  }

  return { score, reasons, signals: [...signals] }
}

// ---------------------------------------------------------------------------
// Tier 2: Content scoring (SQLite query on opencode.db part table)
// ---------------------------------------------------------------------------

/**
 * Keyword groups for content-level signal detection. Each group maps to
 * a ValueSignal category. The keywords are matched case-insensitively
 * against the concatenation of all text parts in the session.
 *
 * The keywords are chosen to be specific enough to avoid false positives
 * but broad enough to catch real usage:
 *   - verification: real system query tools and endpoints
 *   - correction: phrases indicating something was wrong
 *   - skill: skill file references and creation
 *   - knowledge: knowledge directory and recording phrases
 *   - debugging: root-cause and fix phrases
 */
const CONTENT_KEYWORD_GROUPS: { signal: ValueSignal; keywords: string[]; reason: string }[] = [
  {
    signal: "verification",
    keywords: ["kibana", "archery", "grafana", "elasticsearch", "bsearch", "rocketmq", "curl", "SELECT", "mysql", "es-log", "nacos", "apollo"],
    reason: "包含日志/DB/配置验证关键词",
  },
  {
    signal: "correction",
    keywords: ["误导", "踩坑", "不适用", "过期", "缺失", "误判", "不对", "错误", "修正", "需要更新", "不准确"],
    reason: "包含经验修正/纠错关键词",
  },
  {
    signal: "skill",
    keywords: ["SKILL.md", "skill-create", "skill-improvement", "触发词", "skill registry", "新建 skill", "更新 skill"],
    reason: "包含 skill 发现/改进关键词",
  },
  {
    signal: "knowledge",
    keywords: ["knowledge", "知识沉淀", "记录到", "保存到", "profile-", "conventions-", "biz-", "pitfall-", "link-"],
    reason: "包含知识记录关键词",
  },
  {
    signal: "debugging",
    keywords: ["根因", "root cause", "原因是", "找到原因", "定位到", "确认是", "验证通过", "验证结果"],
    reason: "包含根因分析/验证确认关键词",
  },
]

/** Shape of the single-row aggregate result from the content query. */
interface ContentQueryRow {
  part_count: number
  tool_count: number
  code_tool_count: number
  text_sample: string
}

/**
 * Query opencode.db for aggregate signals about a session's content.
 *
 * Uses a single SQL query that counts parts, tool parts, code-tool
 * parts (bash/edit/write), and concatenates a sample of text part data
 * for keyword scanning. The query is parameterized via `.param set` —
 * the session id is the only runtime input.
 *
 * Returns null on any error (DB missing, timeout, parse failure).
 * Never throws.
 */
function querySessionContent(
  sessionId: string,
  opts?: { dbPath?: string; sqliteFn?: typeof spawn },
): Promise<ContentQueryRow | null> {
  const dbPath = opts?.dbPath ?? DEFAULT_DB_PATH
  const sp = opts?.sqliteFn ?? spawn

  // Single aggregate query: counts + a substring of text parts.
  // The GROUP_CONCAT with LIMIT trick gives us up to 50 text parts'
  // data concatenated — enough for keyword scanning without
  // transferring the full transcript.
  const query = `
    SELECT
      COUNT(*) AS part_count,
      SUM(CASE WHEN p.data LIKE '%"type":"tool"%' THEN 1 ELSE 0 END) AS tool_count,
      SUM(CASE WHEN p.data LIKE '%"tool":"bash"%' OR p.data LIKE '%"tool":"edit"%' OR p.data LIKE '%"tool":"write"%' THEN 1 ELSE 0 END) AS code_tool_count,
      COALESCE(SUBSTR(GROUP_CONCAT(CASE WHEN p.data LIKE '%"type":"text"%' THEN p.data END, ''), 1, 100000), '') AS text_sample
    FROM part p
    WHERE p.session_id = :sid
  `.trim()

  return new Promise<ContentQueryRow | null>((resolve) => {
    if (!existsSync(dbPath)) {
      resolve(null)
      return
    }
    let proc: ReturnType<typeof spawn>
    try {
      proc = sp("sqlite3", ["-json", dbPath], { stdio: ["pipe", "pipe", "pipe"] })
    } catch {
      resolve(null)
      return
    }

    let stdout = ""
    const timer = setTimeout(() => {
      try { proc.kill("SIGKILL") } catch { /* noop */ }
    }, SQLITE_TIMEOUT_MS)

    proc.stdout?.on("data", (d: Buffer) => {
      if (stdout.length >= STDOUT_CAP_BYTES) return
      stdout += d.toString("utf-8")
    })
    proc.on("error", () => {
      clearTimeout(timer)
      resolve(null)
    })
    proc.on("close", (code) => {
      clearTimeout(timer)
      if (code !== 0) {
        resolve(null)
        return
      }
      let rows: unknown
      try {
        rows = JSON.parse(stdout || "[]")
      } catch {
        resolve(null)
        return
      }
      if (!Array.isArray(rows) || rows.length === 0) {
        resolve(null)
        return
      }
      const row = rows[0] as Record<string, unknown>
      resolve({
        part_count: typeof row.part_count === "number" ? row.part_count : 0,
        tool_count: typeof row.tool_count === "number" ? row.tool_count : 0,
        code_tool_count: typeof row.code_tool_count === "number" ? row.code_tool_count : 0,
        text_sample: typeof row.text_sample === "string" ? row.text_sample : "",
      })
    })

    const script =
      `.param init\n` +
      `.param set :sid ${JSON.stringify(sessionId)}\n` +
      query + ";\n" +
      `.quit\n`
    try {
      proc.stdin?.write(script)
      proc.stdin?.end()
    } catch {
      // close handler will resolve(null).
    }
  })
}

/**
 * Score a session's content by querying opencode.db for tool-call
 * counts and keyword presence in text parts.
 *
 * Returns the content sub-score, reasons, and detected signals.
 * Returns score=0 when the DB is unavailable or the session has
 * no content.
 */
export async function scoreSessionContent(
  sessionId: string,
  opts?: { dbPath?: string; sqliteFn?: typeof spawn },
): Promise<{ score: number; reasons: string[]; signals: ValueSignal[] }> {
  const row = await querySessionContent(sessionId, opts)
  if (!row) {
    return { score: 0, reasons: [], signals: [] }
  }

  let score = 0
  const reasons: string[] = []
  const signals = new Set<ValueSignal>()

  // Tool-call density — proxy for real work vs. just chatting.
  if (row.code_tool_count >= 15) {
    score += 10
    reasons.push(`代码工具调用 ${row.code_tool_count} 次 +10`)
  } else if (row.code_tool_count >= 5) {
    score += 5
    reasons.push(`代码工具调用 ${row.code_tool_count} 次 +5`)
  }

  // Keyword scan on the text sample.
  const textLower = row.text_sample.toLowerCase()
  for (const group of CONTENT_KEYWORD_GROUPS) {
    const hit = group.keywords.some((kw) => {
      // Case-sensitive for terms that contain uppercase; lowercase
      // for everything else to maximize coverage.
      if (kw === kw.toUpperCase() && kw.length > 1) {
        return row.text_sample.includes(kw)
      }
      return textLower.includes(kw.toLowerCase())
    })
    if (hit) {
      score += 15
      reasons.push(`${group.reason} +15`)
      signals.add(group.signal)
    }
  }

  // Bonus for sessions with both verification AND correction signals —
  // these are the most valuable (found real evidence that contradicts
  // existing knowledge).
  if (signals.has("verification") && signals.has("correction")) {
    score += 10
    reasons.push("验证 + 纠正双重信号 +10")
  }

  return { score, reasons, signals: [...signals] }
}

// ---------------------------------------------------------------------------
// Combined two-tier scoring
// ---------------------------------------------------------------------------

/**
 * Score a session using both tiers. Tier-1 (metadata) runs first; if
 * the score is below `METADATA_GATE`, tier-2 is skipped to avoid
 * unnecessary SQLite queries.
 *
 * The `skipContent` option forces tier-1-only mode (useful for batch
 * pre-filtering).
 */
export async function scoreSession(
  session: SessionInfo,
  opts?: {
    skipContent?: boolean
    dbPath?: string
    sqliteFn?: typeof spawn
  },
): Promise<ValuationResult> {
  const meta = scoreSessionMetadata(session)

  if (opts?.skipContent || meta.score < METADATA_GATE) {
    return {
      sessionId: session.id,
      score: meta.score,
      reasons: meta.reasons,
      signals: meta.signals,
      metadataScore: meta.score,
      contentScore: 0,
      contentScored: false,
    }
  }

  const content = await scoreSessionContent(session.id, {
    dbPath: opts?.dbPath,
    sqliteFn: opts?.sqliteFn,
  })

  const combinedSignals = new Set<ValueSignal>([...meta.signals, ...content.signals])

  return {
    sessionId: session.id,
    score: meta.score + content.score,
    reasons: [...meta.reasons, ...content.reasons],
    signals: [...combinedSignals],
    metadataScore: meta.score,
    contentScore: content.score,
    contentScored: true,
  }
}
