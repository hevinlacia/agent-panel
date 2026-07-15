/**
 * Structured branch-scope reader for a requirement.
 *
 * Role: surface a concise "代码改动范围" overview - which repos an
 * agent touched and which feature branches it created in each - without
 * forcing the dashboard to parse the free-form `branch.md`. Mirrors the
 * `state.json` pattern: a machine-friendly JSON file (`branches.json`)
 * that agents and the dashboard read/write without parsing Markdown.
 *
 * Two data sources, in priority order:
 *   1. `<req-dir>/branches.json` - authoritative, written by agents.
 *   2. `branch.md` heuristic fallback - best-effort extraction so the
 *      pre-existing requirements still get an overview. Marked
 *      `fallback: true` so the UI can warn it may be imprecise.
 *
 * v2 format (simplified): only repoName + branches + optional role/path/baseRef.
 * Removed merge/commitCount/changedFiles - those belong in release-check
 * or review workflows, not in the branch record. `readBranchScope` still
 * reads the v1 `projectPath` field for backwards compatibility.
 *
 * Public surface:
 *   - readBranchScope(reqDir): Promise<BranchScope | null>
 *   - fallbackFromBranchMd(branchMd): BranchRepo[]
 *   - types: BranchScope, BranchRepo
 *
 * Constraints / safety:
 *   - Pure module; only `node:fs`/`node:path` built-ins.
 *   - Never throws - malformed JSON / unreadable files return null,
 *     fallback parsers return whatever they can (possibly empty).
 *   - Does not touch secret files; only reads `branches.json` in reqDir.
 *
 * Read-this-with:
 *   - `src/requirementState.ts` (the `state.json` precedent this follows)
 *   - `src/server.tsx` (renders the BranchScopeCard from this data)
 */

import { readFile } from "node:fs/promises"
import { existsSync } from "node:fs"
import { join, basename } from "node:path"

export const BRANCH_SCOPE_FILE = "branches.json"

/**
 * One repo (application) the agent modified for a requirement.
 * `branches` are the feature/fix branches created in this repo, not the
 * base branches (master/test/uat) they merge into.
 */
export interface BranchRepo {
  repoName: string
  branches: string[]
  /** Human role label: 后端/前端/BFF/PDA. */
  role?: string
  /** Absolute or `~`-relative path to the repo working copy. */
  path?: string
  /**
   * Optional explicit PRO diff base ref for this repo (e.g.
   * `origin/production` for WMS frontend repos). When absent the code-review
   * scan auto-detects via `detectDefaultBaseRef`: frontend repos use
   * `origin/production`, everything else `origin/master`.
   */
  baseRef?: string
}

/**
 * Full branch scope for a requirement. `fallback` is true when this was
 * derived from `branch.md` heuristics rather than read from
 * `branches.json`; the UI uses it to show an accuracy warning.
 */
export interface BranchScope {
  version: number
  updatedAt: number
  repos: BranchRepo[]
  fallback?: boolean
}

/** Base branch labels that must NOT be treated as feature branches. */
const BASE_BRANCH_RE = /^(origin\/)?(master|test|uat|dev|develop)$/i
const UAT_VERSIONED_RE = /^(origin\/)?uat-\d+$/i

/**
 * Read `branches.json` from a requirement directory. Returns null when
 * the file is absent or unparseable (caller should then fall back to
 * `fallbackFromBranchMd`). Tolerant of missing optional fields.
 *
 * Reads `path` (v2) and falls back to `projectPath` (v1) for backwards
 * compatibility. Ignores v1-only fields (merge, commitCount, changedFiles).
 */
export async function readBranchScope(reqDir: string): Promise<BranchScope | null> {
  const p = join(reqDir, BRANCH_SCOPE_FILE)
  if (!existsSync(p)) return null
  let parsed: unknown
  try {
    parsed = JSON.parse(await readFile(p, "utf-8"))
  } catch {
    return null
  }
  if (!parsed || typeof parsed !== "object") return null
  const o = parsed as Record<string, unknown>
  const rawRepos = Array.isArray(o.repos) ? o.repos : []
  const repos: BranchRepo[] = []
  for (const r of rawRepos) {
    if (!r || typeof r !== "object") continue
    const rr = r as Record<string, unknown>
    const repoName = typeof rr.repoName === "string" ? rr.repoName.trim() : ""
    if (!repoName) continue
    // v2 uses `path`; v1 used `projectPath` - accept either.
    const path =
      typeof rr.path === "string" && rr.path.trim()
        ? rr.path.trim()
        : typeof rr.projectPath === "string" && rr.projectPath.trim()
          ? rr.projectPath.trim()
          : undefined
    repos.push({
      repoName,
      branches: Array.isArray(rr.branches)
        ? rr.branches
            .filter((b): b is string => typeof b === "string" && !!b.trim())
            .map((b) => b.trim())
        : [],
      role:
        typeof rr.role === "string" && rr.role.trim() ? rr.role.trim() : undefined,
      path,
      baseRef:
        typeof rr.baseRef === "string" && rr.baseRef.trim() ? rr.baseRef.trim() : undefined,
    })
  }
  if (repos.length === 0) return null
  return {
    version: typeof o.version === "number" ? o.version : 1,
    updatedAt: typeof o.updatedAt === "number" ? o.updatedAt : Date.now(),
    repos,
  }
}

// ---------------------------------------------------------------------------
// branch.md heuristic fallback
// ---------------------------------------------------------------------------

interface Section {
  title: string
  role?: string
  body: string
}

/** Split markdown into `## `-level sections; a leading preamble becomes one section. */
function splitSections(md: string): Section[] {
  const lines = md.replace(/\r\n/g, "\n").split("\n")
  const sections: Section[] = []
  let cur: Section = { title: "", body: "" }
  for (const line of lines) {
    const m = line.match(/^##\s+(.*)$/)
    if (m) {
      if (cur.body.trim() || sections.length === 0) sections.push(cur)
      cur = { title: m[1].trim(), role: detectRole(m[1]), body: "" }
    } else {
      cur.body += line + "\n"
    }
  }
  sections.push(cur)
  return sections.filter((s) => s.body.trim() || s.title)
}

function detectRole(title: string): string | undefined {
  if (/前端/.test(title)) return "前端"
  if (/后端/.test(title)) return "后端"
  if (/bff/i.test(title)) return "BFF"
  if (/pda/i.test(title)) return "PDA"
  if (/中台/.test(title)) return "中台"
  return undefined
}

/** Full WMS repo names: `yl-cwhsea-wms-outbound-api` etc. */
function extractRepoTokens(text: string): string[] {
  const out = new Set<string>()
  for (const m of text.matchAll(/yl-cwhsea-wms-[\w-]+/gi)) out.add(m[0])
  return [...out]
}

/** Resolve a repo short name from a table cell, prefixing `yl-cwhsea-wms-` when needed. */
function resolveRepoName(raw: string): string {
  const s = raw.replace(/`/g, "").trim()
  if (!s) return ""
  if (/yl-cwhsea-wms/i.test(s)) {
    const m = s.match(/yl-cwhsea-wms-[\w-]+/i)
    return m ? m[0] : s
  }
  // Short form like "plus-api" / "outbound-api" used in WMS-001 tables.
  if (/^[\w-]+-api$/i.test(s) || /^[\w-]+-front$/i.test(s) || /^[\w-]+-web$/i.test(s)) {
    return `yl-cwhsea-wms-${s}`
  }
  return s
}

/**
 * Extract feature/fix branch names from a text block. Three sources:
 *   1. backticked tokens containing `/` (single-line, so a closing
 *      backtick can't pair with the next opening one across lines/tokens)
 *   2. inline/list labels: `分支：x` / `branch: x`
 *   3. table cells `| <label with branch/分支> | <value> |` (rescues
 *      un-backticked values like WMS-016's `Requirement branch | ...`)
 */
function extractBranchTokens(text: string): string[] {
  const out = new Set<string>()
  const add = (b: string) => {
    const s = b.trim().replace(/`/g, "")
    if (isFeatureBranch(s)) out.add(s)
  }
  for (const m of text.matchAll(/`([^`\n]*\/[^`\n]*)`/g)) add(m[1])
  for (const m of text.matchAll(/(?:分支|branch)\s*[:：]\s*([^\s,，|`\n]+)/gi)) add(m[1])
  for (const line of text.split("\n")) {
    const parts = line.split("|").map((s) => s.trim())
    if (parts.length >= 3 && /branch|分支/i.test(parts[1])) add(parts[2])
  }
  return [...out]
}

function isFeatureBranch(b: string): boolean {
  if (!b || !b.includes("/")) return false
  if (BASE_BRANCH_RE.test(b)) return false
  if (UAT_VERSIONED_RE.test(b)) return false
  if (/^origin\//i.test(b)) return false
  // Branch names have no whitespace, and `/` is never first/last char.
  if (/\s/.test(b)) return false
  if (b.startsWith("/") || b.endsWith("/")) return false
  return true
}

/** Extract a `Project path` value from a text block. */
function extractPath(text: string): string | undefined {
  const m = text.match(/project\s*path\s*[:：]?\s*([^\n|]+)/i)
  if (!m) return undefined
  const p = m[1].replace(/`/g, "").trim()
  return p || undefined
}

function repoNameFromPath(p: string): string {
  return basename(p.replace(/\/+$/, "")) || p
}

/**
 * Best-effort parse of `branch.md` into per-repo branch records. Handles
 * the common formats seen in the wild:
 *   - `## 后端仓库` / `## 仓库: <name>` sections with `| Item | Value |` tables
 *   - single-repo flat tables (no section)
 *   - `| 仓库 | 分支 | … |` tables where each row is a different repo
 *   - `- 仓库：x` / `- 分支：y` list blocks
 *
 * Returns `[]` when nothing recognisable is found. Imprecision is
 * expected; callers should surface the `fallback` flag to the user.
 */
export function fallbackFromBranchMd(branchMd: string): BranchRepo[] {
  if (!branchMd || !branchMd.trim()) return []
  const sections = splitSections(branchMd)
  const repos: BranchRepo[] = []
  const seenRepos = new Set<string>()

  const pushRepo = (r: BranchRepo) => {
    const key = r.repoName + "|" + (r.role ?? "")
    if (!r.repoName && r.branches.length === 0) return
    if (r.repoName && seenRepos.has(key)) {
      const ex = repos.find(
        (x) => x.repoName === r.repoName && (x.role ?? "") === (r.role ?? ""),
      )
      if (ex) {
        for (const b of r.branches) if (!ex.branches.includes(b)) ex.branches.push(b)
        if (!ex.path && r.path) ex.path = r.path
        return
      }
    }
    if (r.repoName) seenRepos.add(key)
    repos.push(r)
  }

  for (let i = 0; i < sections.length; i++) {
    const sec = sections[i]
    // Skip sections explicitly marked out-of-scope or archived.
    if (/无需改动|无改动/.test(sec.title)) continue
    if (/历史/.test(sec.title) && /归档/.test(sec.title)) continue

    // A "repo-column" table has the repo name in the first column
    // (header cell = 仓库/repo), e.g. `| 仓库 | 分支 | 说明 |`.
    const headerMatch = sec.body.match(/^\s*\|\s*([^|\n]+?)\s*\|\s*([^|\n]+?)\s*\|/m)
    const isRepoColTable = headerMatch
      ? /仓库|repo/i.test(headerMatch[1]) && /分支|branch/i.test(headerMatch[2])
      : false
    if (isRepoColTable) {
      let usedTable = false
      for (const line of sec.body.split("\n")) {
        const parts = line.split("|").map((s) => s.trim())
        if (parts.length < 3) continue
        const name = parts[1]
        const branch = parts[2].replace(/`/g, "").trim()
        if (!name || /^-+$/.test(name) || /^(仓库|repo|item|项目|分支|目标分支|环境)$/i.test(name))
          continue
        if (!branch || BASE_BRANCH_RE.test(branch) || UAT_VERSIONED_RE.test(branch)) continue
        usedTable = true
        pushRepo({
          repoName: resolveRepoName(name),
          role: sec.role,
          branches: isFeatureBranch(branch) ? [branch] : [],
        })
      }
      if (usedTable) continue
    }

    // Section-as-one-repo: derive the repo from a token / path / title.
    const repoTokens = extractRepoTokens(sec.body)
    const path = extractPath(sec.body)
    const branches = extractBranchTokens(sec.body)
    const titleRepo = extractRepoFromTitle(sec.title)
    const sectionRepoName =
      repoTokens[0] || (path ? repoNameFromPath(path) : "") || titleRepo

    // List-block repo: `- 仓库：x` then `- 分支：y`.
    const listRepo = sec.body.match(/仓库\s*[:：]\s*([^\n|`]+)/)
    if (listRepo && !sectionRepoName) {
      pushRepo({
        repoName: resolveRepoName(listRepo[1]),
        role: sec.role,
        path,
        branches,
      })
      continue
    }

    if (sectionRepoName) {
      pushRepo({
        repoName: sectionRepoName,
        role: sec.role,
        path,
        branches,
      })
    }
  }

  // Orphan branches: feature branches in the un-sectioned preamble.
  const preamble = sections[0]
  if (preamble && !preamble.title) {
    const orphanBranches = extractBranchTokens(preamble.body)
    const known = new Set(repos.flatMap((r) => r.branches))
    const orphans = orphanBranches.filter((b) => !known.has(b))
    if (orphans.length && !repos.some((r) => !r.repoName)) {
      repos.push({ repoName: "", branches: orphans })
    }
  }

  return repos
}

function extractRepoFromTitle(title: string): string {
  const m = title.match(/(yl-cwhsea-wms-[\w-]+)/i)
  return m ? m[1] : ""
}
