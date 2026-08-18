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
      const diff = chunks.get(file.path) || ""
      rows.push({ repo, file, diff, lines: parseDiffLines(diff) })
    }
  }
  return rows
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
      const match = line.match(/^diff --git a\/(.*?) b\/(.*)$/)
      currentPath = match?.[2] || ""
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

export function compactPath(path: string, max = 52): string {
  if (path.length <= max) return path
  const parts = path.split("/")
  if (parts.length <= 3) return `…${path.slice(-(max - 1))}`
  return `${parts[0]}/…/${parts.slice(-3).join("/")}`
}

export function diffDomId(key: string): string {
  return `diff-file-${encodeURIComponent(key).replace(/%/g, "_")}`
}
