/**
 * Release checklist parser — extracts structured deployment info from
 * the four Hermes-managed requirement files.
 *
 * Role: when a requirement reaches "待上线" status, the dashboard needs
 * to surface a concise "上线检查" card. This module parses the
 * free-form Markdown in meta.md, branch.md, config-changes.md, and
 * test.md to populate four checklist sections:
 *   1. 涉及应用 (applications)
 *   2. 涉及分支 (branches)
 *   3. 数据库变更 (DB table changes)
 *   4. Apollo/Nacos 配置变更 (config changes)
 *
 * Public surface:
 *   - buildReleaseChecklist(files): parse → ReleaseChecklist
 *
 * Constraints / safety:
 *   - Pure function; no I/O, no imports beyond node built-ins.
 *   - Tolerant of missing/malformed files — returns empty arrays.
 *
 * Read-this-with:
 *   - `src/requirements.ts` (where reqDir / *Path fields come from)
 *   - `src/server.tsx` (renders the checklist card)
 */

export interface ReleaseChecklist {
  applications: string[]
  branches: { label: string; value: string }[]
  dbChanges: string[]
  configChanges: string[]
  /** Any raw lines that look like release notes / 上线注意事项. */
  releaseNotes: string[]
}

export interface ChecklistFiles {
  meta?: string
  branch?: string
  config?: string
  test?: string
  notes?: string
}

/**
 * Parse the four Hermes files into a structured release checklist.
 *
 * The parsing is intentionally fuzzy — Hermes files are free-form
 * Markdown authored by humans, not machine-generated. We look for
 * common patterns (table rows, labeled fields, code spans) and
 * extract whatever we can. Missing or unparseable sections return
 * empty arrays rather than throwing.
 */
export function buildReleaseChecklist(files: ChecklistFiles): ReleaseChecklist {
  return {
    applications: extractApplications(files.meta ?? ""),
    branches: extractBranches(files.branch ?? ""),
    dbChanges: extractDbChanges(files.config ?? ""),
    configChanges: extractConfigChanges(files.config ?? ""),
    releaseNotes: extractReleaseNotes(files.notes ?? "", files.test ?? ""),
  }
}

// ---------------------------------------------------------------------------
// Parsers
// ---------------------------------------------------------------------------

/** Extract application names from meta.md. Looks for Projects, scope, etc. */
function extractApplications(meta: string): string[] {
  const apps = new Set<string>()
  // "Projects: wms-yl-cwhsea-wms" or "- Projects: wms-yl-cwhsea-wms"
  const projectMatch = meta.match(/[-*]?\s*Projects?\s*:\s*(.+)/i)
  if (projectMatch) {
    const val = projectMatch[1].trim()
    if (val && val.toLowerCase() !== "unknown") {
      val.split(/[,，、\s]+/).forEach((a) => {
        const s = a.trim()
        if (s && s.toLowerCase() !== "unknown") apps.add(s)
      })
    }
  }
  // "Stakeholders: ..." often lists app names too
  const stakeMatch = meta.match(/[-*]?\s*Stakeholders?\s*:\s*(.+)/i)
  if (stakeMatch) {
    const val = stakeMatch[1].trim()
    if (val && val.toLowerCase() !== "unknown") {
      val.split(/[,，、\s]+/).forEach((a) => {
        const s = a.trim()
        if (s && s.toLowerCase() !== "unknown") apps.add(s)
      })
    }
  }
  // Include section: application names often appear there
  const includeMatch = meta.match(/[-*]?\s*Include\s*:([\s\S]*?)(?:\n[-*]?\s*Exclude|$)/i)
  if (includeMatch) {
    const lines = includeMatch[1].split("\n")
    for (const line of lines) {
      const m = line.match(/[-*]?\s*(wms|oms|pay|tms|infra|hermes|opencode)\S*/i)
      if (m) apps.add(m[0].replace(/^[-*]\s*/, "").trim())
    }
  }
  return [...apps]
}

/** Extract branch info from branch.md. Parses table rows and labeled fields. */
function extractBranches(branch: string): { label: string; value: string }[] {
  const result: { label: string; value: string }[] = []
  // Table rows: | Source branch | `feature/xxx` |
  const tableRows = branch.matchAll(/\|\s*([^|]+?)\s*\|\s*(`[^`]+`|[^|]+?)\s*\|/g)
  for (const m of tableRows) {
    const label = m[1].trim()
    const value = m[2].trim().replace(/`/g, "")
    if (label.toLowerCase() === "item" || !label || !value) continue
    if (value.toLowerCase() === "unknown") continue
    result.push({ label, value })
  }
  // Labeled fields: "- Source branch: `feature/xxx`"
  const labeled = branch.matchAll(/[-*]?\s*(Source branch|Target branch|PR\/CR|Merge status|Project path)\s*:\s*(.+)/gi)
  for (const m of labeled) {
    const label = m[1].trim()
    const value = m[2].trim().replace(/`/g, "")
    if (value.toLowerCase() === "unknown") continue
    // Avoid duplicates with table rows
    if (!result.some((r) => r.label === label)) {
      result.push({ label, value })
    }
  }
  return result
}

/** Extract database table changes from config-changes.md. */
function extractDbChanges(config: string): string[] {
  const changes = new Set<string>()
  // Look for SQL keywords / table names
  // Patterns: ALTER TABLE, CREATE TABLE, table names in backticks
  const sqlPatterns = config.matchAll(/(?:ALTER|CREATE|DROP|MODIFY|ADD|INSERT|UPDATE)\s+TABLE\s+`?(\w+)`?/gi)
  for (const m of sqlPatterns) {
    changes.add(m[0].trim())
  }
  // "## DB" / "## 数据库" sections
  const dbSection = config.match(/##\s*(?:DB|数据库|Database|SQL)[\s\S]*?(?=\n##\s|$)/i)
  if (dbSection) {
    const lines = dbSection[0].split("\n").slice(1)
    for (const line of lines) {
      const trimmed = line.trim()
      if (trimmed && !trimmed.startsWith("#") && !trimmed.startsWith("|--")) {
        changes.add(trimmed)
      }
    }
  }
  // Table rows mentioning DDL
  const tableRows = config.matchAll(/\|\s*([^|]*?(?:DDL|表|table|ALTER|CREATE)[^|]*?)\s*\|/gi)
  for (const m of tableRows) {
    const val = m[1].trim()
    if (val && val.toLowerCase() !== "item") changes.add(val)
  }
  return [...changes]
}

/** Extract Apollo/Nacos config changes from config-changes.md. */
function extractConfigChanges(config: string): string[] {
  const changes = new Set<string>()
  // Lines with config key patterns: key = value, key: value
  // Apollo: `mq.switch.xxx = true` or `mq.switch.xxx: true`
  const configLines = config.matchAll(/`?([\w.\-]+(?:switch|config|rocket|mq|rabbit|apollo|nacos)[\w.\-]*)`?\s*[=:]\s*\S+/gi)
  for (const m of configLines) {
    changes.add(m[0].trim())
  }
  // Table rows in config sections
  const configSection = config.match(/##\s*(?:Config|配置|Apollo|Nacos|MQ)[\s\S]*?(?=\n##\s|$)/i)
  if (configSection) {
    const rows = configSection[0].matchAll(/\|\s*`?([^|`]+)`?\s*\|\s*`?([^|`]+)`?\s*\|/g)
    for (const m of rows) {
      const key = m[1].trim()
      const val = m[2].trim()
      if (key.toLowerCase() === "item" || key.toLowerCase() === "unknown") continue
      if (val.toLowerCase() === "unknown") continue
      changes.add(`${key} = ${val}`)
    }
  }
  return [...changes]
}

/** Extract release notes / 上线注意事项 from notes.md and test.md. */
function extractReleaseNotes(notes: string, test: string): string[] {
  const result: string[] = []
  // Look for "上线" / "部署" / "release" sections in notes
  const releaseSection = notes.match(/##\s*(?:上线|部署|release|发版|发布)[\s\S]*?(?=\n##\s|$)/i)
  if (releaseSection) {
    const lines = releaseSection[0].split("\n").slice(1)
    for (const line of lines) {
      const trimmed = line.trim()
      if (trimmed && !trimmed.startsWith("#")) result.push(trimmed)
    }
  }
  // "注意事项" in test.md
  const cautionSection = test.match(/##\s*(?:注意|caution|rollback|回滚|风险)[\s\S]*?(?=\n##\s|$)/i)
  if (cautionSection) {
    const lines = cautionSection[0].split("\n").slice(1)
    for (const line of lines) {
      const trimmed = line.trim()
      if (trimmed && !trimmed.startsWith("#")) result.push(trimmed)
    }
  }
  return result
}
