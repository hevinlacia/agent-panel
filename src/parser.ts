/**
 * Parse an experience-summary report.md into structured candidate data.
 *
 * Report format (produced by experience-summarizer agent):
 *
 * # Experience Summary Report
 *
 * ## 元信息
 * - Session: <sessionID or current>
 * - Artifact: <path>
 * - Summary scope: <scope>
 * - Generated: <timestamp>
 *
 * ## 中高价值候选
 *
 * ### 候选清单
 * 1. `[C1]` [标题]
 *    价值评级: 高/中（[依据]）
 *    验证依据: [runtime-derivable | code-derivable | code-derivable exception] — [证据]
 *    来源: [WMS 知识/测试状态图/Skill 改进/...]
 *    目标文件或目录: [path]
 *    变更摘要: [要写什么]
 *    后续处理 skill: [writer/maintainer skill]
 *    关键证据: [artifact path + reference]
 *    执行注意事项: [边界、不要写什么、依赖前提]
 *
 * ### 主/子 agent 互动优化候选
 * - ...
 *
 * ## Low-value omitted
 * - <count and one-line reason, or `0`>
 *
 * ## Risks/gaps
 * - ...
 */

export type ValueRating = "高" | "中" | "低"

export type Candidate = {
  id: string
  title: string
  valueRating: ValueRating
  valueReason: string
  evidenceType: string
  evidenceDetail: string
  source: string
  targetFile: string
  changeSummary: string
  followUpSkill: string
  keyEvidence: string
  executionNotes: string
  category: "candidate" | "interaction"
}

export type ReportMeta = {
  session: string
  artifact: string
  scope: string
  generated: string
}

export type ParsedReport = {
  meta: ReportMeta
  candidates: Candidate[]
  lowValueOmitted: string
  risksGaps: string
  rawPath: string
}

const FIELD_PATTERNS: Array<[keyof Candidate, RegExp]> = [
  ["valueRating", /^\s*价值评级:\s*(高|中|低)/],
  ["valueReason", /^\s*价值评级:\s*(?:高|中|低)[（(]([^）)]*)[）)]/],
  ["evidenceType", /^\s*验证依据:\s*(\[?[^\]—]*\]?)\s*[—–-]/],
  ["evidenceDetail", /^\s*验证依据:\s*.+?[—–-]\s*(.+)/],
  ["source", /^\s*来源:\s*(.+)/],
  ["targetFile", /^\s*目标文件或目录:\s*(.+)/],
  ["changeSummary", /^\s*变更摘要:\s*(.+)/],
  ["followUpSkill", /^\s*后续处理 skill:\s*(.+)/],
  ["keyEvidence", /^\s*关键证据:\s*(.+)/],
  ["executionNotes", /^\s*执行注意事项:\s*(.+)/],
]

function parseCandidateBlock(
  lines: string[],
  startIndex: number,
  category: "candidate" | "interaction"
): { candidate: Candidate | null; nextIndex: number } {
  const headerLine = lines[startIndex]
  // Match: `1. `[C1]` [标题]`  or  `- `[C1]` [标题]`
  const headerMatch = headerLine.match(/^(?:\d+\.|-)\s*`?\[?(C\d+)\]?`?\s*(.+)/)
  if (!headerMatch) return { candidate: null, nextIndex: startIndex + 1 }

  const id = headerMatch[1]
  const title = headerMatch[2].trim()
  const candidate: Candidate = {
    id,
    title,
    valueRating: "中",
    valueReason: "",
    evidenceType: "",
    evidenceDetail: "",
    source: "",
    targetFile: "",
    changeSummary: "",
    followUpSkill: "",
    keyEvidence: "",
    executionNotes: "",
    category,
  }

  let i = startIndex + 1
  while (i < lines.length) {
    const line = lines[i]
    // Next candidate header or section header
    if (/^(\d+\.|-)\s*`?\[?C\d+\]?`?\s+/.test(line)) break
    if (/^#{1,4}\s/.test(line)) break
    if (/^(##|###)\s/.test(line)) break

    for (const [field, pattern] of FIELD_PATTERNS) {
      const m = line.match(pattern)
      if (m) {
        const value = m[1]?.trim() || ""
        if (field === "valueRating") {
          candidate.valueRating = value as ValueRating
        } else {
          ;(candidate[field] as string) = value
        }
        break
      }
    }
    i++
  }

  return { candidate, nextIndex: i }
}

function extractMeta(lines: string[]): ReportMeta {
  const meta: ReportMeta = {
    session: "",
    artifact: "",
    scope: "",
    generated: "",
  }
  let inMeta = false
  for (const line of lines) {
    if (line.startsWith("## 元信息")) {
      inMeta = true
      continue
    }
    if (inMeta && line.startsWith("## ")) break
    if (!inMeta) continue

    const sessionMatch = line.match(/^-\s*Session:\s*(.+)/)
    if (sessionMatch) meta.session = sessionMatch[1].trim()

    const artifactMatch = line.match(/^-\s*Artifact:\s*(.+)/)
    if (artifactMatch) meta.artifact = artifactMatch[1].trim()

    const scopeMatch = line.match(/^-\s*Summary scope:\s*(.+)/)
    if (scopeMatch) meta.scope = scopeMatch[1].trim()

    const generatedMatch = line.match(/^-\s*Generated:\s*(.+)/)
    if (generatedMatch) meta.generated = generatedMatch[1].trim()
  }
  return meta
}

function extractSection(lines: string[], headerPattern: RegExp): string {
  let inSection = false
  const parts: string[] = []
  for (const line of lines) {
    if (headerPattern.test(line)) {
      inSection = true
      continue
    }
    if (inSection && /^## /.test(line)) break
    if (inSection) parts.push(line)
  }
  return parts.join("\n").trim()
}

export function parseReport(content: string, rawPath: string): ParsedReport {
  const lines = content.split("\n")
  const meta = extractMeta(lines)
  const candidates: Candidate[] = []

  let i = 0
  let currentCategory: "candidate" | "interaction" = "candidate"

  while (i < lines.length) {
    const line = lines[i]

    if (line.includes("### 候选清单")) {
      currentCategory = "candidate"
      i++
      continue
    }
    if (line.includes("### 主/子 agent 互动优化候选")) {
      currentCategory = "interaction"
      i++
      continue
    }

    // Candidate header: `1. `[C1]` title` or `- `[C1]` title`
    if (/^(?:\d+\.|-)\s*`?\[?C\d+\]?`?\s+/.test(line)) {
      const result = parseCandidateBlock(lines, i, currentCategory)
      if (result.candidate) candidates.push(result.candidate)
      i = result.nextIndex
      continue
    }

    i++
  }

  const lowValueOmitted = extractSection(lines, /^## Low-value omitted/)
  const risksGaps = extractSection(lines, /^## Risks\/gaps/)

  return {
    meta,
    candidates,
    lowValueOmitted,
    risksGaps,
    rawPath,
  }
}
