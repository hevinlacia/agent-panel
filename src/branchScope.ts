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
 *      20 pre-existing requirements still get an overview. Marked
 *      `fallback: true` so the UI can warn it may be imprecise.
 *
 * Public surface:
 *   - readBranchScope(reqDir): Promise<BranchScope | null>
 *   - fallbackFromBranchMd(branchMd): BranchRepo[]
 *   - types: BranchScope, BranchRepo, MergeState
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

/** Per-target merge state for a repo. `none` = not mentioned. */
export type MergeState = "merged" | "pending" | "none"

/**
 * One repo (application) the agent modified for a requirement.
 * `branches` are the feature/fix branches created in this repo, not the
 * base branches (master/test/uat) they merge into.
 */
export interface BranchRepo {
  repoName: string
  /** Human role label parsed from section titles: 后端/前端/BFF/PDA/中台. */
  role?: string
  /** Absolute or `~`-relative path to the repo working copy. */
  projectPath?: string
  /** Feature/fix branches created in this repo. */
  branches: string[]
  /** Merge progress into base branches. `uatBranch` records e.g. UAT-2607. */
  merge: { test?: MergeState; uat?: MergeState; master?: MergeState; uatBranch?: string }
  commitCount?: number
  changedFiles?: string[]
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
    repos.push({
      repoName,
      role: typeof rr.role === "string" && rr.role.trim() ? rr.role.trim() : undefined,
      projectPath: typeof rr.projectPath === "string" && rr.projectPath.trim() ? rr.projectPath.trim() : undefined,
      branches: Array.isArray(rr.branches)
        ? rr.branches.filter((b): b is string => typeof b === "string" && !!b.trim()).map((b) => b.trim())
        : [],
      merge: normalizeMerge(rr.merge),
      commitCount: typeof rr.commitCount === "number" ? rr.commitCount : undefined,
      changedFiles: Array.isArray(rr.changedFiles)
        ? rr.changedFiles.filter((f): f is string => typeof f === "string" && !!f.trim()).map((f) => f.trim())
        : undefined,
    })
  }
  if (repos.length === 0) return null
  return {
    version: typeof o.version === "number" ? o.version : 1,
    updatedAt: typeof o.updatedAt === "number" ? o.updatedAt : Date.now(),
    repos,
  }
}

function normalizeMerge(m: unknown): BranchRepo["merge"] {
  if (!m || typeof m !== "object") return {}
  const mm = m as Record<string, unknown>
  const out: BranchRepo["merge"] = {}
  for (const k of ["test", "uat", "master"] as const) {
    const v = mm[k]
    if (v === "merged" || v === "pending" || v === "none") out[k] = v
  }
  if (typeof mm.uatBranch === "string" && mm.uatBranch.trim()) out.uatBranch = mm.uatBranch.trim()
  return out
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
  // This filters stray `/` caught between two unrelated backtick pairs
  // (e.g. `e3e374e2`/`e0df2950`, `API/SYSTEM` inside a commit message).
  if (/\s/.test(b)) return false
  if (b.startsWith("/") || b.endsWith("/")) return false
  return true
}

/** Extract a `Project path` value, returning the repo dir basename too. */
function extractProjectPath(text: string): string | undefined {
  const m = text.match(/project\s*path\s*[:：]?\s*([^\n|]+)/i)
  if (!m) return undefined
  const p = m[1].replace(/`/g, "").trim()
  return p || undefined
}

function repoNameFromPath(p: string): string {
  return basename(p.replace(/\/+$/, "")) || p
}

/** Detect test/uat/master merge state from a text block. */
/**
 * Detect test/uat/master merge state from a text block. `merged` when a
 * merge/push/✅ cue appears within ~30 chars of the target name on the
 * same line (covers "已合并到 test", "已合 master", "test ✅", "已 merge
 * 到 UAT-2607"). `pending` when the target is mentioned without a cue,
 * `none` when not mentioned at all.
 */
function detectMergeStates(text: string): BranchRepo["merge"] {
  const out: BranchRepo["merge"] = {}
  const uatMatch = text.match(/UAT-?\d+/i)
  if (uatMatch) out.uatBranch = uatMatch[0]
  const cue = String.raw`已合并|已合|已\s*merge|已\s*push|✅`
  for (const target of ["test", "uat", "master"] as const) {
    const label = target === "uat" ? "uat" : target
    if (!new RegExp(label, "i").test(text)) {
      out[target] = "none"
      continue
    }
    const merged = new RegExp(
      `(?:${cue})[^\n]{0,30}${label}|${label}[^\n]{0,30}(?:${cue})`,
      "i",
    )
    out[target] = merged.test(text) ? "merged" : "pending"
  }
  return out
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
    const key = r.repoName + "|" + r.role
    if (!r.repoName && r.branches.length === 0) return
    if (r.repoName && seenRepos.has(key)) {
      // Merge branches into the existing repo entry.
      const ex = repos.find((x) => x.repoName === r.repoName && x.role === r.role)
      if (ex) {
        for (const b of r.branches) if (!ex.branches.includes(b)) ex.branches.push(b)
        ex.merge = { ...ex.merge, ...r.merge }
        return
      }
    }
    if (r.repoName) seenRepos.add(key)
    repos.push(r)
  }

  for (let i = 0; i < sections.length; i++) {
    const sec = sections[i]
    // Skip sections explicitly marked out-of-scope or archived so they
    // don't pollute the overview with no-change / historical repos.
    if (/无需改动|无改动/.test(sec.title)) continue
    if (/历史/.test(sec.title) && /归档/.test(sec.title)) continue

    // A "repo-column" table has the repo name in the first column
    // (header cell = 仓库/repo), e.g. `| 仓库 | 分支 | 说明 |`. An
    // `| Item | Value |` table is a field table, NOT this shape, and is
    // handled by the section-as-one-repo branch below.
    const headerMatch = sec.body.match(/^\s*\|\s*([^|\n]+?)\s*\|\s*([^|\n]+?)\s*\|/m)
    // A repo-column table has the repo in col 1 AND a branch in col 2.
    // A `| 仓库 | 合并方式 | commit | 状态 |` status table is NOT this
    // shape, so it won't spawn "no-branch" repo entries.
    const isRepoColTable = headerMatch
      ? /仓库|repo/i.test(headerMatch[1]) && /分支|branch/i.test(headerMatch[2])
      : false
    // Process repo-column tables row by row so multi-column rows
    // (| 仓库 | 分支 | 基于 | commit | … |) don't bleed later cells into
    // spurious repo entries.
    if (isRepoColTable) {
      let usedTable = false
      for (const line of sec.body.split("\n")) {
        const parts = line.split("|").map((s) => s.trim())
        if (parts.length < 3) continue
        const name = parts[1]
        const branch = parts[2].replace(/`/g, "").trim()
        if (!name || /^-+$/.test(name) || /^(仓库|repo|item|项目|分支|目标分支|环境)$/i.test(name)) continue
        // Skip base-branch status rows (test / UAT-2607 / master).
        if (!branch || BASE_BRANCH_RE.test(branch) || UAT_VERSIONED_RE.test(branch)) continue
        usedTable = true
        pushRepo({
          repoName: resolveRepoName(name),
          role: sec.role,
          branches: isFeatureBranch(branch) ? [branch] : [],
          merge: detectMergeStates(sec.body),
        })
      }
      if (usedTable) continue
    }

    // Section-as-one-repo: derive the repo from a token / Project path /
    // section title, then collect every feature branch in the section.
    const repoTokens = extractRepoTokens(sec.body)
    const projectPath = extractProjectPath(sec.body)
    const branches = extractBranchTokens(sec.body)
    const merge = detectMergeStates(sec.body)
    const titleRepo = extractRepoFromTitle(sec.title)
    const sectionRepoName =
      repoTokens[0] || (projectPath ? repoNameFromPath(projectPath) : "") || titleRepo

    // List-block repo: `- 仓库：x` then `- 分支：y` (WMS-001 main branch).
    const listRepo = sec.body.match(/仓库\s*[:：]\s*([^\n|`]+)/)
    if (listRepo && !sectionRepoName) {
      pushRepo({
        repoName: resolveRepoName(listRepo[1]),
        role: sec.role,
        projectPath,
        branches,
        merge,
      })
      continue
    }

    // Only emit a repo entry when we actually identified a repo for this
    // section. Branches without a repo are collected as orphans below so
    // auxiliary sections (e.g. "分支干净度检查") don't create noise.
    if (sectionRepoName) {
      pushRepo({
        repoName: sectionRepoName,
        role: sec.role,
        projectPath,
        branches,
        merge,
      })
    }
  }

  // Orphan branches: feature branches in the un-sectioned preamble with
  // no repo token. This rescues single-repo files whose only repo hint is
  // a labeled field. Branches from titled sections without a repo are
  // intentionally dropped (usually auxiliary notes / archived fix branches).
  const preamble = sections[0]
  if (preamble && !preamble.title) {
    const orphanBranches = extractBranchTokens(preamble.body)
    const known = new Set(repos.flatMap((r) => r.branches))
    const orphans = orphanBranches.filter((b) => !known.has(b))
    if (orphans.length && !repos.some((r) => !r.repoName)) {
      repos.push({ repoName: "", branches: orphans, merge: {} })
    }
  }

  return repos
}

function extractRepoFromTitle(title: string): string {
  const m = title.match(/(yl-cwhsea-wms-[\w-]+)/i)
  return m ? m[1] : ""
}
