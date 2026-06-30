import { readdir, readFile, stat } from "node:fs/promises"
import { join, basename } from "node:path"
import { parseReport, type ParsedReport } from "./parser.ts"

const HANDOFF_ROOT = "/tmp/opencode/handoff"

/** Directories that may contain report.md files */
const REPORT_DIRS = [
  join(HANDOFF_ROOT, "experience-summary"),
  join(HANDOFF_ROOT, "experience-batch"),
  join(HANDOFF_ROOT, "auto-summary"),
]

export type ReportSummary = {
  id: string
  dir: string
  reportPath: string
  session: string
  scope: string
  generated: string
  candidateCount: number
  highCount: number
  mediumCount: number
  title: string
}

async function findReportFiles(dir: string): Promise<string[]> {
  try {
    const entries = await readdir(dir, { withFileTypes: true })
    const files: string[] = []
    for (const entry of entries) {
      if (entry.isDirectory()) {
        const subDir = join(dir, entry.name)
        // Check for report.md directly in this subdirectory
        const reportPath = join(subDir, "report.md")
        try {
          await stat(reportPath)
          files.push(reportPath)
        } catch {
          // No report.md here, recurse one level
          const subFiles = await findReportFiles(subDir)
          files.push(...subFiles.filter((f) => f.endsWith("report.md")))
        }
      } else if (entry.name === "report.md") {
        files.push(join(dir, entry.name))
      }
    }
    return files
  } catch {
    return []
  }
}

export async function scanReports(): Promise<ReportSummary[]> {
  const allFiles: string[] = []
  for (const dir of REPORT_DIRS) {
    const files = await findReportFiles(dir)
    allFiles.push(...files)
  }

  // Also check for .report.md pattern (auto-summary uses <sid>.report.md)
  for (const dir of REPORT_DIRS) {
    try {
      const entries = await readdir(dir, { withFileTypes: true })
      for (const entry of entries) {
        if (entry.isFile() && entry.name.endsWith(".report.md")) {
          const path = join(dir, entry.name)
          if (!allFiles.includes(path)) allFiles.push(path)
        }
      }
    } catch {
      // dir doesn't exist
    }
  }

  const summaries: ReportSummary[] = []
  for (const reportPath of allFiles) {
    try {
      const content = await readFile(reportPath, "utf-8")
      const parsed = parseReport(content, reportPath)
      const highCount = parsed.candidates.filter((c) => c.valueRating === "高").length
      const mediumCount = parsed.candidates.filter((c) => c.valueRating === "中").length

      summaries.push({
        id: reportPath,
        dir: reportPath.replace(/\/report\.md$/, "").replace(/\.report\.md$/, ""),
        reportPath,
        session: parsed.meta.session || "unknown",
        scope: parsed.meta.scope || "full session",
        generated: parsed.meta.generated || "",
        candidateCount: parsed.candidates.length,
        highCount,
        mediumCount,
        title: `${parsed.meta.session || "session"} — ${parsed.candidates.length} candidates`,
      })
    } catch {
      // skip unreadable
    }
  }

  // Sort by generated date desc, fall back to file mtime
  summaries.sort((a, b) => (b.generated || "").localeCompare(a.generated || ""))
  return summaries
}

export async function getReport(reportPath: string): Promise<ParsedReport | null> {
  try {
    const content = await readFile(reportPath, "utf-8")
    return parseReport(content, reportPath)
  } catch {
    return null
  }
}

export type Confirmation = {
  reportPath: string
  confirmedIds: string[]
  rejectedIds: string[]
  mode: "confirm" | "reject"
  timestamp: string
}

const CONFIRMATION_DIR = "/tmp/opencode/handoff/confirmations"

export async function saveConfirmation(conf: Confirmation): Promise<string> {
  const { writeFile, mkdir } = await import("node:fs/promises")
  await mkdir(CONFIRMATION_DIR, { recursive: true })
  const slug = basename(conf.reportPath).replace(/\.report\.md$/, "").replace(/\.md$/, "")
  const fileName = `${slug}-${conf.mode}-${Date.now()}.json`
  const filePath = join(CONFIRMATION_DIR, fileName)
  await writeFile(filePath, JSON.stringify(conf, null, 2) + "\n", "utf-8")
  return filePath
}

export type ConfirmationStatus = {
  confirmedIds: string[]
  rejectedIds: string[]
}

/**
 * Read all confirmation JSON files and merge those whose `reportPath`
 * matches into a single { confirmedIds, rejectedIds } set. Matching by
 * the full reportPath inside the JSON (not by filename prefix) avoids
 * collisions: all regular reports are named `report.md`, so a filename
 * slug would be shared across different reports.
 */
export async function getConfirmationStatus(reportPath: string): Promise<ConfirmationStatus> {
  const confirmed = new Set<string>()
  const rejected = new Set<string>()
  try {
    const entries = await readdir(CONFIRMATION_DIR, { withFileTypes: true })
    for (const entry of entries) {
      if (!entry.isFile() || !entry.name.endsWith(".json")) continue
      try {
        const raw = await readFile(join(CONFIRMATION_DIR, entry.name), "utf-8")
        const conf = JSON.parse(raw) as Confirmation
        if (conf.reportPath !== reportPath) continue
        if (conf.mode === "confirm" && Array.isArray(conf.confirmedIds)) {
          conf.confirmedIds.forEach((id) => confirmed.add(id))
        }
        if (conf.mode === "reject" && Array.isArray(conf.rejectedIds)) {
          conf.rejectedIds.forEach((id) => rejected.add(id))
        }
      } catch {
        // skip unreadable / malformed JSON
      }
    }
  } catch {
    // confirmations dir doesn't exist yet
  }
  return { confirmedIds: [...confirmed], rejectedIds: [...rejected] }
}
