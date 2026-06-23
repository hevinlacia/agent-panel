/**
 * Requirement/session recommendation helpers.
 *
 * Role: score unbound OpenCode sessions against a Hermes requirement so
 * the detail page can surface likely matches before the user manually
 * searches a long datalist.
 *
 * Public surface:
 *   - recommendSessionsForRequirement(req, candidates, limit)
 *   - scoreSessionForRequirement(req, session)
 *
 * Constraints / safety:
 *   - Pure functions only; no I/O and no session transcript reads.
 *   - Scores only title, cwd/worktree metadata, timestamps, and req text.
 *
 * Read-this-with:
 *   - `src/requirements.ts` for requirement metadata.
 *   - `src/server.tsx` for the requirement detail rendering.
 */

import type { Requirement } from "./requirements.ts"
import type { SessionInfo } from "./sessions.ts"

export interface SessionRecommendation {
  session: SessionInfo
  score: number
  reasons: string[]
}

const ASCII_STOP_WORDS = new Set([
  "title",
  "status",
  "owner",
  "start",
  "date",
  "planned",
  "release",
  "project",
  "stakeholders",
  "unknown",
  "java",
  "groovy",
])

const HAN_STOP_WORDS = new Set([
  "需求",
  "状态",
  "项目",
  "未知",
  "当前",
  "具体",
  "影响",
  "范围",
  "开发中",
  "测试中",
  "待补",
  "同事",
])

function normalize(text: string): string {
  return text.toLowerCase().replace(/\s+/g, " ").trim()
}

function compact(text: string): string {
  return normalize(text).replace(/\s+/g, "")
}

function addReason(reasons: string[], reason: string): void {
  if (reasons.length >= 5) return
  if (!reasons.includes(reason)) reasons.push(reason)
}

function addKeyword(out: Map<string, number>, keyword: string, weight: number): void {
  const k = keyword.trim().toLowerCase()
  if (!k) return
  if (/^[a-z0-9_-]+$/.test(k) && ASCII_STOP_WORDS.has(k)) return
  if (HAN_STOP_WORDS.has(k)) return
  if (k.length < 2) return
  const prev = out.get(k) ?? 0
  if (weight > prev) out.set(k, weight)
}

function collectKeywords(req: Requirement): Map<string, number> {
  const out = new Map<string, number>()
  const texts = [
    req.id,
    req.title,
    req.project,
    ...(req.groupPath ?? []),
    req.description,
  ].filter(Boolean)

  for (const text of texts) {
    const lower = normalize(text)
    for (const token of lower.match(/[a-z0-9][a-z0-9_-]{2,}/g) ?? []) {
      const weight = token.includes("_") || token.includes("-") ? 18 : 12
      addKeyword(out, token, weight)
    }
    for (const phrase of lower.match(/[\u4e00-\u9fff]{2,}/g) ?? []) {
      addKeyword(out, phrase, Math.min(24, 8 + phrase.length))
      for (let size = 2; size <= Math.min(4, phrase.length); size++) {
        for (let i = 0; i <= phrase.length - size; i++) {
          addKeyword(out, phrase.slice(i, i + size), 6 + size)
        }
      }
    }
  }

  return out
}

/**
 * Score one session against a requirement using only cheap metadata.
 * A zero score means the session should not be shown as a recommendation.
 */
export function scoreSessionForRequirement(
  req: Requirement,
  session: SessionInfo,
): SessionRecommendation | null {
  const title = normalize(session.title || "")
  const titleCompact = compact(session.title || "")
  const reqTitleCompact = compact(req.title || "")
  const meta = normalize([
    session.directory,
    session.worktree,
    session.path,
    session.projectId,
    session.modelId,
  ].filter(Boolean).join(" "))
  const reasons: string[] = []
  let score = 0

  if (reqTitleCompact.length >= 4 && titleCompact.includes(reqTitleCompact)) {
    score += 80
    addReason(reasons, "标题直接命中需求名")
  }

  const keywords = collectKeywords(req)
  let keywordScore = 0
  for (const [keyword, weight] of [...keywords.entries()].sort((a, b) => b[1] - a[1])) {
    if (title.includes(keyword)) {
      keywordScore += weight
      addReason(reasons, `标题包含 ${keyword}`)
    } else if (meta.includes(keyword)) {
      keywordScore += Math.max(4, Math.floor(weight / 2))
      addReason(reasons, `路径包含 ${keyword}`)
    }
    if (keywordScore >= 70) break
  }
  score += Math.min(70, keywordScore)

  const project = normalize(req.project || "")
  if (project && project !== "默认项目" && !ASCII_STOP_WORDS.has(project)) {
    if (title.includes(project) || meta.includes(project)) {
      score += 8
      addReason(reasons, `命中项目 ${req.project}`)
    }
  }

  const sessionTime = session.updated || session.created || 0
  if (sessionTime > 0) {
    if (req.createdAt && sessionTime >= req.createdAt - 60 * 60_000) {
      score += 5
      addReason(reasons, "时间晚于需求创建")
    } else if (req.updatedAt && sessionTime >= req.updatedAt - 14 * 24 * 60 * 60_000) {
      score += 3
      addReason(reasons, "近期活跃")
    }
  }

  if (score < 10 || reasons.length === 0) return null
  return { session, score, reasons }
}

/**
 * Return the highest scoring unbound sessions for the requirement.
 * Ties prefer the most recently updated session so "continue task" stays useful.
 */
export function recommendSessionsForRequirement(
  req: Requirement,
  candidates: SessionInfo[],
  limit = 6,
): SessionRecommendation[] {
  return candidates
    .map((s) => scoreSessionForRequirement(req, s))
    .filter((r): r is SessionRecommendation => r !== null)
    .sort((a, b) => {
      if (b.score !== a.score) return b.score - a.score
      return (b.session.updated || b.session.created || 0) - (a.session.updated || a.session.created || 0)
    })
    .slice(0, Math.max(0, limit))
}
