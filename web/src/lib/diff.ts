import type { CodeReviewFile, CodeReviewRepoSnapshot, CodeReviewSnapshot } from "../types"

export interface DiffLine {
  type: "add" | "del" | "ctx" | "hunk"
  oldNo: string
  newNo: string
  text: string
}

export interface DiffFileView {
  repo: CodeReviewRepoSnapshot
  file: CodeReviewFile
  diff: string
  lines: DiffLine[]
}

export function reviewStats(review?: CodeReviewSnapshot | null) {
  const repos = review?.repos || []
  return {
    repoCount: repos.length,
    fileCount: repos.reduce((n, r) => n + (r.files?.length || 0), 0),
    additions: repos.reduce((n, r) => n + (r.additions || 0), 0),
    deletions: repos.reduce((n, r) => n + (r.deletions || 0), 0),
  }
}

export function parseUnifiedDiffFiles(review?: CodeReviewSnapshot | null): DiffFileView[] {
  if (!review) return []
  const rows: DiffFileView[] = []
  for (const repo of review.repos || []) {
    const chunks = splitUnifiedDiff(repo.diff || "")
    for (const file of repo.files || []) {
      const path = unquoteGitPath(file.path)
      const diff = chunks.get(file.path) || chunks.get(path) || ""
      rows.push({ repo, file: { ...file, path }, diff, lines: parseDiffLines(diff) })
    }
  }
  return rows
}

/** 解析 git 输出中带引号+转义的路径（core.quotepath 开启时非 ASCII/特殊字符路径会被转义成 \NNN 八进制）。 */
export function unquoteGitPath(raw: string): string {
  const s = raw.trim()
  if (s.length < 2 || !s.startsWith('"') || !s.endsWith('"')) return s
  const inner = s.slice(1, -1)
  const bytes: number[] = []
  let pending = ""
  const flushPlain = () => {
    if (pending) {
      for (const b of new TextEncoder().encode(pending)) bytes.push(b)
      pending = ""
    }
  }
  const chars = Array.from(inner)
  for (let i = 0; i < chars.length; i += 1) {
    const ch = chars[i]
    if (ch === "\\" && i + 1 < chars.length) {
      const next = chars[i + 1]
      const oct = next + (chars[i + 2] ?? "") + (chars[i + 3] ?? "")
      if (/^[0-7]{3}$/.test(oct)) {
        flushPlain()
        bytes.push(parseInt(oct, 8))
        i += 3
        continue
      }
      const escaped: Record<string, number> = { a: 7, b: 8, f: 12, n: 10, r: 13, t: 9, v: 11, '"': 34, "\\": 92 }
      if (next in escaped) {
        flushPlain()
        bytes.push(escaped[next])
        i += 1
        continue
      }
    }
    pending += ch
  }
  flushPlain()
  try {
    return new TextDecoder().decode(Uint8Array.from(bytes))
  } catch {
    return inner
  }
}

export function splitUnifiedDiff(diff: string): Map<string, string> {
  const files = new Map<string, string>()
  let currentPath = ""
  let buffer: string[] = []
  const flush = () => {
    if (currentPath) files.set(currentPath, buffer.join("\n"))
  }
  for (const line of diff.split("\n")) {
    if (line.startsWith("diff --git ")) {
      flush()
      const plain = line.match(/^diff --git a\/(.*?) b\/(.*)$/)
      const quoted = line.match(/^diff --git "a\/(.*)" "b\/(.*)"$/)
      currentPath = plain ? unquoteGitPath(plain[2]) : quoted ? unquoteGitPath(quoted[2]) : ""
      buffer = [line]
      continue
    }
    if (currentPath) buffer.push(line)
  }
  flush()
  return files
}

export function parseDiffLines(diff: string): DiffLine[] {
  const lines: DiffLine[] = []
  let oldNo = 0
  let newNo = 0
  for (const raw of diff.split("\n")) {
    const hunk = raw.match(/^@@ -(\d+)(?:,\d+)? \+(\d+)(?:,\d+)? @@(.*)$/)
    if (hunk) {
      oldNo = Number(hunk[1])
      newNo = Number(hunk[2])
      lines.push({ type: "hunk", oldNo: "", newNo: "", text: raw })
      continue
    }
    if (!raw || raw.startsWith("diff --git") || raw.startsWith("index ") || raw.startsWith("--- ") || raw.startsWith("+++ ")) continue
    if (raw.startsWith("+")) {
      lines.push({ type: "add", oldNo: "", newNo: String(newNo++), text: raw.slice(1) })
    } else if (raw.startsWith("-")) {
      lines.push({ type: "del", oldNo: String(oldNo++), newNo: "", text: raw.slice(1) })
    } else {
      lines.push({ type: "ctx", oldNo: String(oldNo++), newNo: String(newNo++), text: raw.startsWith(" ") ? raw.slice(1) : raw })
    }
  }
  return lines
}

export function shortFileName(path: string): string {
  const parts = path.split("/").filter(Boolean)
  return parts.slice(-1)[0] || path
}

/**
 * import/package 等引入声明行识别：用于把 import 区域折叠成可展开区块。
 * 覆盖 Java/Groovy(package/import)、TS/JS(import/require)、Python(import/from)、
 * Go(import 块)、Rust(use)、C(#include)、C#(using) 等常见形态。
 */
const IMPORT_START_RE = /^\s*(?:import\s|package\s+[\w.]+|from\s+[\w.]+\s+import\b|using\s+[A-Za-z_]|#include\b|use\s+[A-Za-z_]|extern\s+crate\b|@import\b|(?:const|let|var)\s+\w+.*=\s*require\s*\()/
/** import 起始行之后的延续行（多行 import 列表成员、收尾的 } from "x" / from "x" / ) 等）。 */
const IMPORT_CONT_RE = /^\s*(?:\}\s*(?:from\s.+)?[;,]?\s*|\)\s*;?\s*|from\s+['"][^'"]*['"];?\s*|[\w$]+(?:\s*,\s*[\w$]+)*\s*,?\s*|["'][^"']*["'],?\s*)$/

export function isImportLikeLine(text: string, inRun: boolean): boolean {
  if (IMPORT_START_RE.test(text)) return true
  return inRun && text.trim().length > 0 && IMPORT_CONT_RE.test(text)
}

/** 连续 import 行达到该长度才折叠，避免把零星一两行 import 也折起来。 */
export const IMPORT_COLLAPSE_MIN_LINES = 3

export interface ImportBlock {
  id: string
  /** 解析后 diff 行下标范围（含两端） */
  from: number
  to: number
  adds: number
  dels: number
}

/** 找出文件 diff 中可折叠的连续 import 区块（跨 add/del/ctx 行类型）。 */
export function buildImportBlocks(lines: DiffLine[], minRun = IMPORT_COLLAPSE_MIN_LINES): ImportBlock[] {
  const blocks: ImportBlock[] = []
  let start = -1
  let adds = 0
  let dels = 0
  const flush = (endExclusive: number) => {
    if (start >= 0 && endExclusive - start >= minRun) {
      blocks.push({ id: `imp-${start}`, from: start, to: endExclusive - 1, adds, dels })
    }
    start = -1
    adds = 0
    dels = 0
  }
  lines.forEach((line, i) => {
    const inRun = start >= 0
    if (line.type !== "hunk" && isImportLikeLine(line.text, inRun)) {
      if (!inRun) start = i
      if (line.type === "add") adds += 1
      else if (line.type === "del") dels += 1
      return
    }
    flush(i)
  })
  flush(lines.length)
  return blocks
}

export interface FileGap {
  id: string
  /** 新侧（目标分支文件）1-based 行号范围（含两端），该范围内行未变化、diff 未包含 */
  from: number
  to: number
  /** 新旧行号偏移（newNo - oldNo），用于展开行同时显示两侧行号 */
  drift: number
}

export interface FileGapScan {
  /** 文件头部/两个 hunk 之间的未修改区间（不含文件尾，尾部需知道总行数后由调用方补） */
  gaps: FileGap[]
  /** diff 覆盖到的最后一个新侧行号 */
  lastNew: number
  /** diff 结束时的新旧行号偏移，用于尾部 gap 的旧行号换算 */
  lastDrift: number
  /** diff 中是否存在新侧内容（纯删除文件为 false，不做区间展开） */
  hasNewSide: boolean
}

/**
 * 扫描解析后的 diff 行，找出未展示的未修改行区间（文件头、hunk 之间）。
 * git 保证相邻 hunk 的上下文不重叠，因此这些区间在当前展示中完全缺失，
 * 是“文件展示不全/突然截断”的根源；前端用可展开标记补齐。
 */
export function computeFileGaps(lines: DiffLine[]): FileGapScan {
  const gaps: FileGap[] = []
  let lastOld = 0
  let lastNew = 0
  let seq = 0
  let hasNewSide = false
  for (const line of lines) {
    if (line.type === "hunk") {
      const m = line.text.match(/^@@ -(\d+)(?:,\d+)? \+(\d+)(?:,\d+)? @@/)
      if (!m) continue
      const oldStart = Number(m[1])
      const newStart = Number(m[2])
      if (newStart > lastNew + 1) {
        gaps.push({ id: `gap-${seq++}`, from: lastNew + 1, to: newStart - 1, drift: lastNew - lastOld })
      }
      // 同步到 hunk 体起点：后续行的 oldNo/newNo 在该基础上递增（首个 hunk 从 269 开始时，
      // 行号必须落到 269 而不是从 1 重新计数，否则尾部 gap 计算全错）。
      lastOld = Math.max(0, oldStart - 1)
      lastNew = Math.max(0, newStart - 1)
      continue
    }
    if (line.type === "add") {
      hasNewSide = true
      lastNew += 1
    } else if (line.type === "del") {
      lastOld += 1
    } else {
      hasNewSide = true
      lastOld += 1
      lastNew += 1
    }
  }
  return { gaps, lastNew, lastDrift: lastNew - lastOld, hasNewSide }
}

export function compactPath(path: string, max = 52): string {
  if (path.length <= max) return path
  const parts = path.split("/")
  if (parts.length <= 3) return `…${path.slice(-(max - 1))}`
  return `${parts[0]}/…/${parts.slice(-3).join("/")}`
}

export function diffDomId(key: string): string {
  return `diff-file-${encodeURIComponent(key).replace(/%/g, "_")}`
}

/** DOM id for a single diff line row, used by annotation panel locate links. */
export function diffLineDomId(key: string, lineIndex: number): string {
  return `diff-line-${encodeURIComponent(key).replace(/%/g, "_")}-${lineIndex}`
}
