/**
 * Role: Hermes-backed requirement scanning, metadata updates, session commands, and session associations.
 * Public surface: requirement records, lifecycle helpers, ONES references, command helpers, and association APIs.
 * Constraints: only `node:` built-ins; never reads or writes `.env` or secret files.
 * Tests may isolate scan roots and stores through the exported `_set*` helpers.
 * Read-this-with: src/requirementState.ts, src/requirementAlignment.ts, and src/server.tsx.
 */

import { mkdir, readFile, readdir, rename, stat, writeFile } from "node:fs/promises"
import { existsSync } from "node:fs"
import { homedir } from "node:os"
import { fileURLToPath } from "node:url"
import { dirname, join, resolve } from "node:path"
import { randomBytes } from "node:crypto"

import { ALIGNMENT_FILE, PRD_FILE } from "./requirementAlignment.ts"
import { readRequirementState } from "./requirementState.ts"
import type { EffortEstimate } from "./effortEstimation.ts"
import { getConfig } from "./config.ts"

const REQUIREMENT_PROJECT_FILE = "project.json"
const EFFORT_ESTIMATE_FILE = "effort-estimate.json"

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

export type ReqStatus = "需求对齐" | "方案设计" | "开发中" | "自测中" | "测试中" | "待上线" | "已完成"

export const REQ_STATUSES: ReqStatus[] = [
  "需求对齐",
  "方案设计",
  "开发中",
  "自测中",
  "测试中",
  "待上线",
  "已完成",
]

/**
 * Requirement category - a classification orthogonal to status. "线上问题"
 * (production/online issue) skips the early alignment/design phases and
 * defaults to "开发中"; "需求" is the normal product-requirement default.
 */
export type ReqCategory = "需求" | "线上问题"

export const REQ_CATEGORIES: ReqCategory[] = ["需求", "线上问题"]

/**
 * Statuses considered "pre-development". A requirement switched to the
 * "线上问题" category is auto-advanced past these into "开发中" because
 * production issues don't go through alignment/design gating.
 */
export const PRE_DEV_STATUSES: ReqStatus[] = ["需求对齐", "方案设计"]
/**
 * ASCII slug per requirement status, used for the editable per-phase prompt
 * files (`prompts/phase-<slug>.md`) and the board status badge CSS class.
 * Shared here so the prompt loader and the UI stay in sync on naming.
 */
export const REQ_STATUS_SLUG: Record<ReqStatus, string> = {
  "需求对齐": "align",
  "方案设计": "design",
  "开发中": "dev",
  "自测中": "selftest",
  "测试中": "testing",
  "待上线": "deploy",
  "已完成": "done",
}

/**
 * Directory holding the per-phase prompt Markdown files, resolved relative
 * to this module so it works regardless of the server's `cwd`. Each file
 * `phase-<slug>.md` describes what to do in that requirement phase and is
 * injected into a new session's context as the "阶段执行规范" section.
 */
const PHASE_PROMPT_DIR = join(dirname(fileURLToPath(import.meta.url)), "..", "prompts")

/**
 * Load the phase prompt for a requirement status from
 * `prompts/phase-<slug>.md`. The file is the source of truth for per-phase
 * guidance (role, must-read/do/not-do, completion criteria) so it can be
 * tuned without a code change. Returns a clear missing-file note instead of
 * throwing so a missing template degrades the guidance rather than breaking
 * session creation.
 */
export async function readPhasePrompt(status: ReqStatus): Promise<string> {
  const slug = REQ_STATUS_SLUG[status]
  const path = join(PHASE_PROMPT_DIR, `phase-${slug}.md`)
  if (!existsSync(path)) {
    return `（未找到该阶段（${status}）的提示词文件 prompts/phase-${slug}.md，请补充该阶段的执行规范。）`
  }
  return readFile(path, "utf-8").catch(() => `（读取阶段提示词失败：prompts/phase-${slug}.md）`)
}


export interface Requirement {
  id: string
  title: string
  status: ReqStatus
  /** All projects this requirement belongs to. `project` mirrors the first item for legacy callers. */
  projects: string[]
  project: string
  /**
   * Sub-path of intermediate grouping directories between the project
   * root and this requirement. For example, a requirement at
   *   ~/.agents/req/WMS/disaster-recovery/mq-migration/<req>/meta.md
   * has project = "WMS" and groupPath = ["disaster-recovery", "mq-migration"].
   * Legacy flat layouts (~/.agents/req/<req>/meta.md) carry an empty
   * groupPath.
   */
  groupPath: string[]
  description: string
  sessionIds: string[]
  /** Requirement category ("需求" | "线上问题"). Defaults to "需求" when unset. */
  category?: ReqCategory
  /**
   * ONES task reference associated with this requirement, stored in
   * meta.md frontmatter as `ones`. May be a full ONES task URL
   * (clickable in the UI) or a bare task id (display-only). Absent when
   * no ONES task has been linked yet - the board surfaces this as
   * "未关联 ONES" so the user knows to ask the PM to create one.
   */
  ones?: string
  createdAt: number
  updatedAt: number
  metaPath?: string
  backgroundPath?: string
  branchPath?: string
  testPath?: string
  notesPath?: string
  configPath?: string
  impactPath?: string
  memoryPath?: string
  reviewPath?: string
  /** Standard business-alignment brief used by the first requirement phase. */
  alignmentPath?: string
  /** Raw or semi-raw PRD source trace; not a primary context file after alignment. */
  prdPath?: string
  /**
   * AI-powered relative effort estimate, persisted in `effort-estimate.json`.
   * Absent when no estimate has been run yet.
   */
  effortEstimate?: EffortEstimate
  /**
   * Directory holding this requirement's files. Stored on the record so
   * the status-write API can locate `state.json` without re-deriving the
   * path from project/groupPath/id.
   */
  reqDir?: string
}

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

export const DEFAULT_REQ_ID = "__default__"
export const DEFAULT_PROJECT_NAME = "默认项目"

/**
 * When set (by tests via `_setReqDir`), scanning is confined to this single
 * requirement directory and ignores the configured scan roots. `null` in
 * production, where `scanHermesRequirements` derives requirement directories
 * from `requirementScanRoots` in the dashboard config. Mirrors `_setStorePath`.
 */
let _reqDir: string | null = null

/** Override the requirement scan directory for tests; pass `null` to restore
 * config-driven scanning. */
export function _setReqDir(path: string | null): void {
  _reqDir = path
}

export function _getReqDir(): string | null {
  return _reqDir
}

// ---------------------------------------------------------------------------
// Associations store (test-overridable)
// ---------------------------------------------------------------------------

interface AssociationStore {
  version: 2
  associations: Record<string, string[]>
}

const DEFAULT_STORE_PATH = join(
  homedir(),
  ".local",
  "share",
  "agent-panel",
  "associations.json"
)

let _storePath: string = DEFAULT_STORE_PATH

export function _setStorePath(path: string): void {
  _storePath = path
}

export function _getStorePath(): string {
  return _storePath
}

async function ensureStoreDir(): Promise<void> {
  const dir = dirname(_storePath)
  if (!existsSync(dir)) {
    await mkdir(dir, { recursive: true })
  }
}

function emptyAssociations(): AssociationStore {
  return { version: 2, associations: {} }
}

function normalizeReqStatus(v: unknown): ReqStatus | null {
  if (v === "待开发") return "方案设计"
  if (typeof v === "string" && (REQ_STATUSES as string[]).includes(v)) return v as ReqStatus
  return null
}

/** Validate and normalize a raw category value; returns null for unknown values. */
function normalizeReqCategory(v: unknown): ReqCategory | null {
  if (typeof v === "string" && (REQ_CATEGORIES as string[]).includes(v)) return v as ReqCategory
  return null
}

/**
 * Load associations. Migrates the legacy `requirements.json` format
 * (which embedded sessionIds in each requirement record) into the new
 * shape on first read.
 */
export async function loadAssociations(): Promise<AssociationStore> {
  if (!existsSync(_storePath)) {
    // Check for a legacy requirements.json sitting next to the new file
    // and migrate any sessionIds out of it.
    const legacyPath = join(dirname(_storePath), "requirements.json")
    if (existsSync(legacyPath) && legacyPath !== _storePath) {
      try {
        const raw = await readFile(legacyPath, "utf-8")
        const parsed = JSON.parse(raw) as unknown
        const store = emptyAssociations()
        if (parsed && typeof parsed === "object") {
          const reqArr = (parsed as { requirements?: unknown }).requirements
          if (Array.isArray(reqArr)) {
            for (const item of reqArr) {
              if (!item || typeof item !== "object") continue
              const o = item as Record<string, unknown>
              if (typeof o.id !== "string" || !o.id) continue
              const sids = Array.isArray(o.sessionIds)
                ? (o.sessionIds.filter((s) => typeof s === "string") as string[])
                : []
              if (sids.length > 0) {
                store.associations[o.id] = sids
              }
            }
          }
        }
        await saveAssociations(store)
        return store
      } catch {
        // Fall through to empty store.
      }
    }
    const empty = emptyAssociations()
    await saveAssociations(empty)
    return empty
  }
  let raw: string
  try {
    raw = await readFile(_storePath, "utf-8")
  } catch {
    return emptyAssociations()
  }
  let parsed: unknown
  try {
    parsed = JSON.parse(raw)
  } catch {
    return emptyAssociations()
  }
  if (!parsed || typeof parsed !== "object") {
    return emptyAssociations()
  }
  const obj = parsed as Record<string, unknown>

  // Legacy format detection: presence of a `requirements` array.
  if (Array.isArray(obj.requirements)) {
    const store = emptyAssociations()
    for (const item of obj.requirements as unknown[]) {
      if (!item || typeof item !== "object") continue
      const o = item as Record<string, unknown>
      if (typeof o.id !== "string" || !o.id) continue
      const sids = Array.isArray(o.sessionIds)
        ? (o.sessionIds.filter((s) => typeof s === "string") as string[])
        : []
      if (sids.length > 0) {
        store.associations[o.id] = sids
      }
    }
    await saveAssociations(store)
    return store
  }

  // New format.
  const associations: Record<string, string[]> = {}
  const rawAssoc = obj.associations
  if (rawAssoc && typeof rawAssoc === "object") {
    for (const [k, v] of Object.entries(rawAssoc as Record<string, unknown>)) {
      if (Array.isArray(v)) {
        const sids = v.filter((s): s is string => typeof s === "string")
        if (sids.length > 0) associations[k] = sids
      }
    }
  }
  return { version: 2, associations }
}

export async function saveAssociations(store: AssociationStore): Promise<void> {
  await ensureStoreDir()
  await writeFile(_storePath, JSON.stringify(store, null, 2), "utf-8")
}

// ---------------------------------------------------------------------------
// Hermes scanner
// ---------------------------------------------------------------------------

interface Frontmatter {
  fields: Record<string, string>
  body: string
}

/**
 * Parse simple YAML-ish frontmatter:
 *   ---
 *   key: value
 *   key2: value2
 *   ---
 *   <body>
 * Quoted values have surrounding single/double quotes stripped.
 * If the file does not start with a `---` line, the entire content is
 * treated as the body.
 */
function parseFrontmatter(text: string): Frontmatter {
  const fields: Record<string, string> = {}
  // Normalize line endings.
  const normalized = text.replace(/\r\n/g, "\n")
  if (!normalized.startsWith("---\n") && normalized !== "---") {
    return { fields, body: normalized }
  }
  const lines = normalized.split("\n")
  // First line is `---`. Find the next `---`.
  let endIdx = -1
  for (let i = 1; i < lines.length; i++) {
    if (lines[i] === "---") {
      endIdx = i
      break
    }
  }
  if (endIdx === -1) {
    // Unterminated; treat as no frontmatter.
    return { fields, body: normalized }
  }
  for (let i = 1; i < endIdx; i++) {
    const line = lines[i]
    if (!line || !line.trim() || line.trim().startsWith("#")) continue
    const colonIdx = line.indexOf(":")
    if (colonIdx === -1) continue
    const key = line.slice(0, colonIdx).trim()
    let value = line.slice(colonIdx + 1).trim()
    if (
      (value.startsWith('"') && value.endsWith('"') && value.length >= 2) ||
      (value.startsWith("'") && value.endsWith("'") && value.length >= 2)
    ) {
      value = value.slice(1, -1)
    }
    if (key) fields[key] = value
  }
  const body = lines.slice(endIdx + 1).join("\n")
  return { fields, body }
}

function firstParagraph(body: string): string {
  const trimmed = body.replace(/^\s+/, "")
  if (!trimmed) return ""
  // Split on blank lines.
  const paragraphs = trimmed.split(/\n\s*\n/)
  for (const p of paragraphs) {
    const cleaned = p
      .split("\n")
      // Drop pure heading lines so the description isn't just `# 标题`.
      .filter((l) => !/^\s*#{1,6}\s+/.test(l))
      .join("\n")
      .trim()
    if (cleaned) return cleaned
  }
  return ""
}

function parseStartDate(value: string | undefined): number | null {
  if (!value) return null
  const s = value.trim()
  if (!s) return null
  // Accept YYYY-MM-DD, YYYY/MM/DD, or full ISO.
  const ts = Date.parse(s.replace(/\//g, "-"))
  if (Number.isNaN(ts)) return null
  return ts
}

function normalizeProjectPath(value: unknown): string[] {
  if (Array.isArray(value)) {
    return value
      .map((item) => (typeof item === "string" ? item.trim() : ""))
      .filter(Boolean)
  }
  if (typeof value !== "string") return []
  return value
    .split(/[\/]/)
    .map((item) => item.trim())
    .filter(Boolean)
}

function normalizeProjectList(value: unknown): string[] {
  if (Array.isArray(value)) {
    return value
      .map((item) => (typeof item === "string" ? item.trim() : ""))
      .filter(Boolean)
  }
  if (typeof value !== "string") return []
  return value
    .split(/[,，]/)
    .map((item) => item.trim())
    .filter(Boolean)
}

function uniqueStrings(values: string[]): string[] {
  const seen = new Set<string>()
  const out: string[] = []
  for (const value of values) {
    const clean = value.trim()
    if (!clean || seen.has(clean)) continue
    seen.add(clean)
    out.push(clean)
  }
  return out
}

async function readRequirementProjectFile(
  dirPath: string,
): Promise<{ projects?: string[]; groupPath?: string[] }> {
  const path = join(dirPath, REQUIREMENT_PROJECT_FILE)
  if (!existsSync(path)) return {}
  try {
    const parsed = JSON.parse(await readFile(path, "utf-8")) as unknown
    if (!parsed || typeof parsed !== "object") return {}
    const obj = parsed as Record<string, unknown>
    const projects = uniqueStrings([
      ...normalizeProjectList(obj.projects),
      ...normalizeProjectList(obj.project),
    ])
    const groupPath = normalizeProjectPath(obj.groupPath ?? obj.subproject ?? obj.path)
    return { projects: projects.length > 0 ? projects : undefined, groupPath }
  } catch {
    return {}
  }
}

async function readRequirementProjectTags(dirPath: string, fallbackName: string): Promise<string[]> {
  const metaPath = join(dirPath, "meta.md")
  if (!existsSync(metaPath)) return [fallbackName]
  try {
    const raw = await readFile(metaPath, "utf-8")
    const fm = parseFrontmatter(raw)
    const projects = uniqueStrings([
      ...normalizeProjectList(fm.fields["projects"]),
      ...normalizeProjectList(fm.fields["project"]),
    ])
    const title = fm.fields["title"]?.trim()
    if (title) projects.push(title)
    const titleMatch = raw.match(/^\s*-\s*Title\s*:\s*(.+?)\s*$/im)
    if (titleMatch?.[1]?.trim()) projects.push(titleMatch[1].trim())
    return uniqueStrings(projects.length > 0 ? projects : [fallbackName])
  } catch {
    // fall back to the directory name
  }
  return [fallbackName]
}

async function loadRequirementFromDir(
  dirPath: string,
  dirName: string,
  parentProjects: string[],
  groupPath: string[] = [],
): Promise<Requirement | null> {
  let st
  try {
    st = await stat(dirPath)
  } catch {
    return null
  }
  if (!st.isDirectory()) return null

  const metaPath = join(dirPath, "meta.md")
  const backgroundPath = join(dirPath, "background.md")
  const branchPath = join(dirPath, "branch.md")
  const testPath = join(dirPath, "test.md")
  const notesPath = join(dirPath, "notes.md")
  const configPath = join(dirPath, "config-changes.md")
  const impactPath = join(dirPath, "impact.md")
  const memoryPath = join(dirPath, "memory.md")
  const reviewPath = join(dirPath, "review.md")
  const alignmentPath = join(dirPath, ALIGNMENT_FILE)
  const prdPath = join(dirPath, PRD_FILE)

  let title = dirName
  let status: ReqStatus = "开发中"
  let category: ReqCategory = "需求"
  let ones: string | undefined
  // Explicit project sources (frontmatter + project.json) are collected
  // first; they take precedence over inherited parentProjects / the
  // DEFAULT_PROJECT_NAME fallback, so a flat requirement that declares its
  // own `project:` is not also tagged 默认项目.
  let frontmatterProjects: string[] = []
  let description = ""
  let id = dirName
  let createdAt = st.mtimeMs
  let updatedAt = st.mtimeMs

  let metaPresent = false
  if (existsSync(metaPath)) {
    metaPresent = true
    try {
      const raw = await readFile(metaPath, "utf-8")
      const fm = parseFrontmatter(raw)
      const fields = fm.fields
      if (fields["req-id"]) id = fields["req-id"]
      if (fields["title"]) title = fields["title"]
      const rawStatus = normalizeReqStatus(fields["status"])
      if (rawStatus) status = rawStatus
      const rawCategory = normalizeReqCategory(fields["category"])
      if (rawCategory) category = rawCategory
      const rawOnes = (fields["ones"] || "").trim()
      if (rawOnes) ones = rawOnes
      if (fields["project"] && fields["project"].trim()) {
        frontmatterProjects.push(fields["project"].trim())
      }
      frontmatterProjects.push(...normalizeProjectList(fields["projects"]))
      const sd = parseStartDate(fields["start-date"])
      if (sd !== null) createdAt = sd
      const desc = firstParagraph(fm.body)
      if (desc) description = desc

      // Markdown-list fallback for hermes meta.md (e.g. "- Title: Foo").
      // Only used when YAML frontmatter didn't already provide a value.
      const titleMatch = raw.match(/^\s*-\s*Title\s*:\s*(.+?)\s*$/im)
      if (titleMatch && (title === dirName || !title)) {
        title = titleMatch[1].trim()
      }
    } catch {
      // Keep defaults.
    }
  }

  const projectFile = await readRequirementProjectFile(dirPath)
  if (projectFile.groupPath) groupPath = projectFile.groupPath
  // Explicit sources win; otherwise inherit ancestor grouping projects;
  // otherwise the synthetic default project. Precedence (project.json >
  // frontmatter > inherited > default) matches the legacy accumulation.
  const explicitProjects = uniqueStrings([...(projectFile.projects ?? []), ...frontmatterProjects])
  const projects = explicitProjects.length > 0
    ? explicitProjects
    : uniqueStrings(parentProjects.length > 0 ? parentProjects : [DEFAULT_PROJECT_NAME])
  const project = projects[0] ?? DEFAULT_PROJECT_NAME

  // state.json wins over both frontmatter and the markdown-list status.
  // readRequirementState also migrates `- Status: <english>` from
  // meta.md the first time it runs.
  try {
    const state = await readRequirementState(dirPath)
    if (state) {
      status = state.status
      updatedAt = Math.max(updatedAt, state.updatedAt)
      // state.json category wins over frontmatter when present.
      if (state.category) category = state.category
    }
  } catch {
    // ignore; fall back to whatever we already have.
  }

  // Read AI effort estimate if it exists (best-effort, never blocks scanning).
  let effortEstimate: EffortEstimate | undefined
  try {
    const ep = join(dirPath, EFFORT_ESTIMATE_FILE)
    if (existsSync(ep)) {
      const parsed = JSON.parse(await readFile(ep, "utf-8")) as unknown
      if (parsed && typeof parsed === "object" && typeof (parsed as EffortEstimate).coefficient === "number") {
        effortEstimate = parsed as EffortEstimate
      }
    }
  } catch {
    // ignore
  }

  return {
    id,
    title,
    status,
    category,
    ones,
    projects,
    project,
    groupPath,
    description,
    sessionIds: [],
    createdAt,
    updatedAt,
    metaPath: metaPresent ? metaPath : undefined,
    backgroundPath: existsSync(backgroundPath) ? backgroundPath : undefined,
    branchPath: existsSync(branchPath) ? branchPath : undefined,
    testPath: existsSync(testPath) ? testPath : undefined,
    notesPath: existsSync(notesPath) ? notesPath : undefined,
    configPath: existsSync(configPath) ? configPath : undefined,
    impactPath: existsSync(impactPath) ? impactPath : undefined,
    memoryPath: existsSync(memoryPath) ? memoryPath : undefined,
    reviewPath: existsSync(reviewPath) ? reviewPath : undefined,
    alignmentPath: existsSync(alignmentPath) ? alignmentPath : undefined,
    prdPath: existsSync(prdPath) ? prdPath : undefined,
    effortEstimate,
    reqDir: dirPath,
  }
}

/**
 * Parsed view of a requirement's ONES task reference for the UI. `url` is
 * set only when the stored value is an http(s) link (clickable); a bare
 * task id yields `url: null` so the board renders it as plain text.
 * `label` is the issue code from an ONES hash route, otherwise the last
 * URL path segment for links, or the raw value for a bare task id.
 */
export interface OnesRef {
  raw: string
  url: string | null
  label: string
}

/**
 * Turn a raw `ones` frontmatter value into a display-ready reference.
 * ONES uses hash routing, so `#/.../issue/<code>` wins over pathname.
 * Returns null for empty/missing values and never throws on malformed URLs.
 */
export function parseOnesRef(raw: string | undefined | null): OnesRef | null {
  const value = (raw || "").trim()
  if (!value) return null
  if (/^https?:\/\//i.test(value)) {
    let label = value
    try {
      const url = new URL(value)
      const issueCode = url.hash.match(/(?:^|\/)issue\/([^/?#]+)/i)?.[1]
      const pathSegment = url.pathname.split("/").filter(Boolean).pop()
      const segment = issueCode || pathSegment
      if (segment && segment.length <= 60) label = decodeURIComponent(segment)
    } catch {
      // keep full value as label
    }
    return { raw: value, url: value, label }
  }
  return { raw: value, url: null, label: value }
}

/**
 * Upsert (or remove, when value is empty) a single frontmatter key in
 * meta.md, preserving the body and every other frontmatter field. This is
 * the write path for requirement metadata that lives in meta.md (like the
 * ONES reference) rather than in the Agent Panel-managed state.json.
 * Quoting matches what `parseFrontmatter` can strip: values containing a
 * colon, `#`, quotes, or surrounding whitespace are quoted so the lenient
 * split-on-first-colon parser round-trips them losslessly.
 */
export async function upsertMetaFrontmatterField(
  reqDir: string,
  key: string,
  value: string,
): Promise<void> {
  const metaPath = join(reqDir, "meta.md")
  const raw = existsSync(metaPath) ? await readFile(metaPath, "utf-8").catch(() => "") : ""
  const normalized = raw.replace(/\r\n/g, "\n")
  const hasFrontmatter = normalized.startsWith("---\n") || normalized === "---"
  const lines = normalized.split("\n")

  let fmEnd = -1
  if (hasFrontmatter) {
    for (let i = 1; i < lines.length; i++) {
      if (lines[i] === "---") { fmEnd = i; break }
    }
  }

  const fmLines = hasFrontmatter && fmEnd > 0 ? lines.slice(1, fmEnd) : []
  const bodyStart = hasFrontmatter && fmEnd > 0 ? fmEnd + 1 : 0
  const body = lines.slice(bodyStart).join("\n")

  const kept: string[] = []
  for (const line of fmLines) {
    const colonIdx = line.indexOf(":")
    if (colonIdx > 0 && line.slice(0, colonIdx).trim() === key) {
      // Drop the existing value line; re-add below only if value is non-empty.
      continue
    }
    kept.push(line)
  }

  const trimmedValue = value.trim()
  if (trimmedValue) {
    kept.push(`${key}: ${formatFrontmatterValue(trimmedValue)}`)
  }

  const fmBlock = kept.length > 0 ? ["---", ...kept, "---"].join("\n") + "\n" : ""
  // Preserve the body verbatim, including any blank line that separated the
  // closing `---` from the first heading, so re-writing a frontmatter field
  // is a no-op on the body and produces a minimal git diff.
  const next = fmBlock + body
  await mkdir(dirname(metaPath), { recursive: true })
  const tmp = `${metaPath}.tmp.${process.pid}.${Date.now()}`
  await writeFile(tmp, next, "utf-8")
  await rename(tmp, metaPath)
}

function formatFrontmatterValue(value: string): string {
  if (/[:#"']/.test(value) || value !== value.trim()) {
    // Double-quote and escape embedded double quotes.
    return `"${value.replace(/"/g, '\\"')}"`
  }
  return value
}

/**
 * Set (or clear, when `ones` is empty) the requirement's ONES task
 * reference in meta.md frontmatter. Returns the normalized stored value
 * so the API can echo it back without re-reading the file.
 */
export async function writeRequirementOnes(reqDir: string, ones: string): Promise<string> {
  const normalized = ones.trim()
  await upsertMetaFrontmatterField(reqDir, "ones", normalized)
  return normalized
}

/**
 * Infer the owning project workspace from a requirement directory. The
 * scanner stores requirement files under `<project>/.agents/req/...` or
 * `<project>/req/...`; new pi sessions should start in `<project>` so
 * AGENTS.md discovery, git commands, and relative paths match the project.
 * Returns null for legacy/external layouts where no project marker exists.
 */
export function resolveRequirementProjectCwd(req: Pick<Requirement, "reqDir">): string | null {
  const reqDir = req.reqDir ? resolve(req.reqDir) : ""
  if (!reqDir) return null
  for (const marker of [`/.agents/req/`, `/req/`]) {
    const idx = reqDir.indexOf(marker)
    if (idx <= 0) continue
    const cwd = reqDir.slice(0, idx)
    return cwd && existsSync(cwd) ? cwd : null
  }
  return null
}

/**
 * Build the copyable command for a requirement-bound Pi session. Prefixes
 * the command with `cd <project>` when the requirement directory reveals a
 * project root; otherwise falls back to the current terminal directory.
 */
export function buildPiRequirementSessionCommand(req: Pick<Requirement, "reqDir">, args: readonly string[]): string {
  const piCommand = ["pi", ...args].map(shellQuote).join(" ")
  const cwd = resolveRequirementProjectCwd(req)
  return cwd ? `cd ${shellQuote(cwd)} && ${piCommand}` : piCommand
}

function shellQuote(value: string): string {
  if (/^[A-Za-z0-9_./:@%+=,-]+$/.test(value)) return value
  return `'${value.replace(/'/g, `'"'"'`)}'`
}

/**
 * Recursively collect requirements (directories that contain meta.md)
 * under `rootPath`. Any directory without meta.md is treated as an
 * intermediate grouping directory and its segment name is appended to
 * `groupPath` for descendants.
 *
 * A directory with meta.md and nested requirement directories is treated as
 * a project tag container rather than a requirement record. Descendant
 * requirements inherit its title as an additional project tag.
 *
 * Bounded recursion: max depth 6 to keep accidental symlink loops or
 * deeply nested test fixtures from spinning.
 */
async function collectRequirementsRecursive(
  rootPath: string,
  projects: string[],
  groupPath: string[],
  out: Requirement[],
  depth = 0,
): Promise<void> {
  if (depth > 6) return
  let st
  try {
    st = await stat(rootPath)
  } catch {
    return
  }
  if (!st.isDirectory()) return

  let currentGroupPath = groupPath
  let currentProjects = projects
  const hasOwnMeta = existsSync(join(rootPath, "meta.md"))
  const childDirs: { name: string; path: string; hasMeta: boolean }[] = []

  let children: string[]
  try {
    children = await readdir(rootPath)
  } catch {
    return
  }
  for (const childName of children) {
    if (childName.startsWith(".") || childName === "README.md") continue
    const childPath = join(rootPath, childName)
    let childSt
    try {
      childSt = await stat(childPath)
    } catch {
      continue
    }
    if (!childSt.isDirectory()) continue
    childDirs.push({ name: childName, path: childPath, hasMeta: existsSync(join(childPath, "meta.md")) })
  }

  const hasNestedRequirements = childDirs.some((child) => child.hasMeta)

  if (hasOwnMeta && hasNestedRequirements) {
    const dirName = rootPath.split("/").filter(Boolean).pop() || rootPath
    currentProjects = uniqueStrings([...projects, ...(await readRequirementProjectTags(rootPath, dirName))])
  } else if (hasOwnMeta) {
    const dirName = rootPath.split("/").filter(Boolean).pop() || rootPath
    const req = await loadRequirementFromDir(rootPath, dirName, currentProjects, groupPath)
    if (req) out.push(req)
  }

  for (const child of childDirs) {
    if (child.hasMeta) {
      await collectRequirementsRecursive(child.path, currentProjects, currentGroupPath, out, depth + 1)
    } else {
      await collectRequirementsRecursive(
        child.path,
        currentProjects,
        [...currentGroupPath, child.name],
        out,
        depth + 1,
      )
    }
  }
}

/**
 * Resolve the requirement directories to scan. When `_reqDir` is set (test
 * override), scanning is confined to that single directory. Otherwise the
 * directories are derived from `requirementScanRoots` in the dashboard
 * config: each root contributes its `.agents/req/` and/or `req/`
 * subdirectory if present. Production no longer reads `~/.agents/req/`.
 */
async function resolveReqScanDirs(): Promise<string[]> {
  if (_reqDir) return [_reqDir]
  const cfg = await getConfig()
  const roots = cfg.requirementScanRoots ?? []
  const dirs: string[] = []
  const seen = new Set<string>()
  for (const root of roots) {
    for (const sub of [".agents/req", "req"]) {
      const candidate = join(root, sub)
      if (existsSync(candidate) && !seen.has(candidate)) {
        seen.add(candidate)
        dirs.push(candidate)
      }
    }
  }
  return dirs
}

/**
 * Scan one requirement directory: iterate its top-level entries. A directory
 * with `meta.md` is a flat requirement whose project comes from frontmatter;
 * a directory without `meta.md` is a project-level grouping whose name
 * becomes the inherited project tag. Children are discovered recursively
 * via `collectRequirementsRecursive`.
 */
async function scanReqDir(reqDir: string, out: Requirement[]): Promise<void> {
  let topEntries: string[]
  try {
    topEntries = await readdir(reqDir)
  } catch {
    return
  }
  for (const name of topEntries) {
    if (name === "README.md" || name.startsWith(".")) continue
    const topPath = join(reqDir, name)
    let topSt
    try {
      topSt = await stat(topPath)
    } catch {
      continue
    }
    if (!topSt.isDirectory()) continue

    // `_default` maps to the synthetic default project name; any other
    // meta-less top dir is a project grouping whose name is inherited.
    const projectDisplay = name === "_default" ? DEFAULT_PROJECT_NAME : name
    const hasOwnMeta = existsSync(join(topPath, "meta.md"))

    if (hasOwnMeta) {
      // Flat layout: <req-id>/meta.md. Project comes from frontmatter or
      // defaults to DEFAULT_PROJECT_NAME; children are still discovered.
      await collectRequirementsRecursive(topPath, [DEFAULT_PROJECT_NAME], [], out)
      continue
    }
    await collectRequirementsRecursive(topPath, [projectDisplay], [], out)
  }
}

export async function scanHermesRequirements(): Promise<Requirement[]> {
  const reqDirs = await resolveReqScanDirs()
  const out: Requirement[] = []
  const seenIds = new Set<string>()
  for (const reqDir of reqDirs) {
    if (!existsSync(reqDir)) continue
    const batch: Requirement[] = []
    await scanReqDir(reqDir, batch)
    for (const r of batch) {
      // Dedupe by id across overlapping scan roots (keeps first occurrence).
      if (seenIds.has(r.id)) continue
      seenIds.add(r.id)
      out.push(r)
    }
  }
  return out
}

// ---------------------------------------------------------------------------
// Synthetic default requirement
// ---------------------------------------------------------------------------

function buildDefaultRequirement(sessionIds: string[]): Requirement {
  const now = Date.now()
  return {
    id: DEFAULT_REQ_ID,
    title: "默认需求",
    status: "开发中",
    projects: [DEFAULT_PROJECT_NAME],
    project: DEFAULT_PROJECT_NAME,
    groupPath: [],
    description:
      "未关联到具体需求的 session 归属到此默认需求。如需独立管理，可在需求扫描目录下创建对应需求目录后重新关联。",
    sessionIds,
    createdAt: now,
    updatedAt: now,
  }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

export async function getRequirement(id: string): Promise<Requirement | null> {
  const [hermes, store] = await Promise.all([
    scanHermesRequirements(),
    loadAssociations(),
  ])
  if (id === DEFAULT_REQ_ID) {
    // Mirror listRequirementsByProject: the default requirement also owns
    // sessions associated with reqIds that no longer exist in Hermes
    // (orphaned associations), so /projects and /requirement?id=__default__
    // agree on session count.
    const hermesIds = new Set(hermes.map((r) => r.id))
    const orphanSessions: string[] = []
    for (const [reqId, sids] of Object.entries(store.associations)) {
      if (reqId === DEFAULT_REQ_ID) continue
      if (!hermesIds.has(reqId)) {
        for (const s of sids) orphanSessions.push(s)
      }
    }
    const defaultSessions = [
      ...(store.associations[DEFAULT_REQ_ID] ?? []),
      ...orphanSessions,
    ]
    return buildDefaultRequirement(defaultSessions)
  }
  const found = hermes.find((r) => r.id === id)
  if (!found) return null
  found.sessionIds = store.associations[found.id] ?? []
  return found
}

/**
 * List all real (Hermes-backed) requirements grouped by project, sorted by
 * updatedAt desc. The synthetic default requirement (DEFAULT_REQ_ID) is
 * deliberately excluded - it is not a real requirement and only exists as a
 * fallback bucket for unassociated/orphaned sessions. Associations remain
 * preserved in the store; use getRequirement(DEFAULT_REQ_ID) or
 * getRequirementForSession() for direct lookups.
 */
export async function listRequirementsByProject(): Promise<
  { project: string; requirements: Requirement[] }[]
> {
  const [hermes, store] = await Promise.all([
    scanHermesRequirements(),
    loadAssociations(),
  ])

  // Attach sessionIds from associations.
  for (const r of hermes) {
    r.sessionIds = store.associations[r.id] ?? []
  }

  // Group by project tag; a requirement may appear under multiple projects.
  const groups = new Map<string, Requirement[]>()
  // Track the latest updatedAt per non-default project to drive sort order.
  const projectLatest = new Map<string, number>()
  for (const r of hermes) {
    const projects = r.projects?.length ? r.projects : [r.project || DEFAULT_PROJECT_NAME]
    for (const proj of projects) {
      const bucket = groups.get(proj) ?? []
      bucket.push(r)
      groups.set(proj, bucket)
      const cur = projectLatest.get(proj) ?? 0
      if (r.updatedAt > cur) projectLatest.set(proj, r.updatedAt)
    }
  }

  // The synthetic default requirement (__default__) is intentionally excluded
  // from the board: it is not a real requirement, only a bucket for
  // unassociated/orphaned sessions. Associations stay preserved in the store;
  // getRequirement(DEFAULT_REQ_ID) / getRequirementForSession() keep working
  // for direct lookups (e.g. resolving a session's requirement title).
  // Sort: non-default projects by updatedAt desc, default project last
  // (only when a real requirement carries the default project tag).
  const nonDefault = [...groups.keys()]
    .filter((p) => p !== DEFAULT_PROJECT_NAME)
    .sort((a, b) => (projectLatest.get(b) ?? 0) - (projectLatest.get(a) ?? 0))
  const ordered = groups.has(DEFAULT_PROJECT_NAME)
    ? [...nonDefault, DEFAULT_PROJECT_NAME]
    : nonDefault

  return ordered.map((p) => {
    const reqs = (groups.get(p) ?? [])
      .slice()
      .sort((a, b) => b.updatedAt - a.updatedAt)
    return { project: p, requirements: reqs }
  })
}

export async function associateSession(
  reqId: string,
  sessionId: string
): Promise<void> {
  if (!sessionId) return
  const store = await loadAssociations()
  // Remove the session from any other association first.
  for (const [k, sids] of Object.entries(store.associations)) {
    if (k === reqId) continue
    const idx = sids.indexOf(sessionId)
    if (idx !== -1) {
      sids.splice(idx, 1)
      if (sids.length === 0) {
        delete store.associations[k]
      } else {
        store.associations[k] = sids
      }
    }
  }
  const cur = store.associations[reqId] ?? []
  if (!cur.includes(sessionId)) {
    cur.push(sessionId)
  }
  store.associations[reqId] = cur
  await saveAssociations(store)
}

export async function replaceAssociatedSession(
  reqId: string,
  oldSessionId: string,
  newSessionId: string
): Promise<void> {
  if (!newSessionId) return
  const store = await loadAssociations()
  for (const [k, sids] of Object.entries(store.associations)) {
    if (k === reqId) continue
    const next = sids.filter((s) => s !== newSessionId)
    if (next.length === 0) delete store.associations[k]
    else store.associations[k] = next
  }

  const cur = store.associations[reqId] ?? []
  const next = cur.filter((s) => s !== oldSessionId && s !== newSessionId)
  next.push(newSessionId)
  store.associations[reqId] = next
  await saveAssociations(store)
}

/**
 * Remove a session association from a requirement. If the session is not
 * currently associated, this is a no-op. The session becomes an orphan
 * (visible in the default requirement's list) unless re-associated.
 */
export async function dissociateSession(
  reqId: string,
  sessionId: string
): Promise<void> {
  if (!sessionId || !reqId) return
  const store = await loadAssociations()
  const cur = store.associations[reqId]
  if (!cur) return
  const next = cur.filter((s) => s !== sessionId)
  if (next.length === 0) {
    delete store.associations[reqId]
  } else {
    store.associations[reqId] = next
  }
  await saveAssociations(store)
}

export async function getRequirementForSession(
  sessionId: string
): Promise<Requirement> {
  const store = await loadAssociations()
  let foundReqId: string | null = null
  for (const [reqId, sids] of Object.entries(store.associations)) {
    if (sids.includes(sessionId)) {
      foundReqId = reqId
      break
    }
  }
  if (foundReqId && foundReqId !== DEFAULT_REQ_ID) {
    const hermes = await scanHermesRequirements()
    const hit = hermes.find((r) => r.id === foundReqId)
    if (hit) {
      hit.sessionIds = store.associations[hit.id] ?? []
      return hit
    }
  }
  // Default / orphaned / unassociated → synthetic default.
  const defaultSessions = store.associations[DEFAULT_REQ_ID] ?? []
  return buildDefaultRequirement(defaultSessions)
}

export async function getRequirementTitleForSession(
  sessionId: string
): Promise<string> {
  const req = await getRequirementForSession(sessionId)
  return req.title || "默认需求"
}

export async function getAllAssociatedSessionIds(): Promise<Set<string>> {
  const store = await loadAssociations()
  const out = new Set<string>()
  for (const sids of Object.values(store.associations)) {
    for (const s of sids) out.add(s)
  }
  return out
}

// ---------------------------------------------------------------------------
// Session-id and PTY injection helpers
// ---------------------------------------------------------------------------

export function generateSessionId(): string {
  return "ses_" + randomBytes(12).toString("hex")
}

async function readFileSnippet(path: string | undefined, limit = 500): Promise<string> {
  if (!path || !existsSync(path)) return ""
  try {
    const raw = await readFile(path, "utf-8")
    const trimmed = raw.replace(/^\uFEFF/, "").trim()
    if (!trimmed) return ""
    if (trimmed.length <= limit) return trimmed
    return trimmed.slice(0, limit)
  } catch {
    return ""
  }
}

/**
 * Build the agent-context preamble injected into a session that is bound
 * to a Hermes requirement. The output is concise, memory-first:
 *   1. requirement title + status (always)
 *   2. memory.md content (up to 1,200 chars) — the lifecycle memory ledger
  *   3. alignment.md content (up to 900 chars) — business alignment brief
  *   4. background.md content (up to 500 chars) — the why/what of the work
  *   5. notes.md (current progress, up to 300 chars)
  *   6. impact.md (pre-coding safety gate, up to 500 chars)
  *   7. branch.md (branch / commit context, up to 300 chars)
  *   8. the phase prompt for the requirement's current status (loaded from
 *      prompts/phase-<slug>.md)
  *   9. absolute paths to all known files so the agent knows where
  *      to read further or write updates
  *   10. a routing guide that tells the agent which file is authoritative
  *      for release/test/review work
  *   11. a closing line that tells the agent NOT to start work and to wait
  *      for the user to issue the next instruction
  *
  * test.md, config-changes.md, review.md, and prd.md are listed by path
  * but their bodies are NOT inlined — the agent can read them on demand
  * once the user gives it a concrete task. impact.md is inlined because
  * it is the coding safety gate. alignment.md is inlined because it is
  * the normalized business source of truth. Files that do not exist on
  * disk are still listed by path (the agent may create them).
 *
 * The DEFAULT_REQ_ID / "req not found" fallbacks return a minimal
 * 4-line block that only carries the new closing instruction.
 */
/** Directory where per-session injection context files are written for `pi --append-system-prompt`. */
const INJECTION_CTX_DIR = join(homedir(), ".local", "share", "agent-panel", "ctx")

/**
 * Write the requirement injection context to a per-session file and return
 * its path, so the `pi --session-id <id> --name <title> --append-system-prompt
 * <file>` command can feed a large multi-line context without overflowing
 * argv limits or shell-escaping pain.
 */
export async function writeInjectionContext(sessionId: string, ctx: string): Promise<string> {
  await mkdir(INJECTION_CTX_DIR, { recursive: true })
  const path = join(INJECTION_CTX_DIR, `${sessionId}.md`)
  await writeFile(path, ctx, "utf-8")
  return path
}

export async function buildInjectionContext(reqId: string): Promise<string> {
  const closing =
    "请阅读以上需求背景、需求对齐结论和进展信息。不要自行开始执行任何任务，等待用户下达具体任务安排。"
  if (reqId === DEFAULT_REQ_ID) {
    return [
      "【需求上下文】",
      "需求：默认需求",
      "状态：开发中",
      closing,
    ].join("\n")
  }
  const hermes = await scanHermesRequirements()
  const req = hermes.find((r) => r.id === reqId)
  if (!req) {
    return [
      "【需求上下文】",
      "需求：默认需求",
      "状态：开发中",
      closing,
    ].join("\n")
  }
  const lines: string[] = []
  lines.push("【需求上下文】")
  lines.push(`需求：${req.title}`)
  lines.push(`状态：${req.status}`)
  if (req.reqDir) {
    // Prefer the per-record *Path populated by loadRequirementFromDir;
    // fall back to <reqDir>/<basename> so paths are always emitted, even
    // for files that don't exist yet (the agent may create them).
    const backgroundFile = req.backgroundPath ?? join(req.reqDir, "background.md")
    const branchFile = req.branchPath ?? join(req.reqDir, "branch.md")
    const notesFile = req.notesPath ?? join(req.reqDir, "notes.md")
    const testFile = req.testPath ?? join(req.reqDir, "test.md")
    const configFile = req.configPath ?? join(req.reqDir, "config-changes.md")
    const impactFile = req.impactPath ?? join(req.reqDir, "impact.md")
    const memoryFile = req.memoryPath ?? join(req.reqDir, "memory.md")
    const reviewFile = req.reviewPath ?? join(req.reqDir, "review.md")
    const alignmentFile = req.alignmentPath ?? join(req.reqDir, ALIGNMENT_FILE)
    const prdFile = req.prdPath ?? join(req.reqDir, PRD_FILE)

    lines.push("")
    lines.push("需求记忆：")
    const memory = await readFileSnippet(memoryFile, 1200)
    if (memory) {
      lines.push(memory)
    } else {
      lines.push(`（未提供 memory.md，路径：${memoryFile}。这是跨 session 的需求生命周期记忆入口。）`)
    }

    lines.push("")
    lines.push("需求对齐：")
    const alignment = await readFileSnippet(alignmentFile, 900)
    if (alignment) {
      lines.push(alignment)
    } else {
      lines.push(`（未提供 alignment.md，路径：${alignmentFile}。需求对齐阶段必须把产品/业务 PRD 或口述需求提炼成此标准格式。）`)
    }

    lines.push("")
    lines.push("需求背景：")
    const background = await readFileSnippet(backgroundFile, 500)
    if (background) {
      lines.push(background)
    } else {
      lines.push(`（未提供 background.md，路径：${backgroundFile}）`)
    }

    lines.push("")
    lines.push("当前进展：")
    const notes = await readFileSnippet(notesFile, 300)
    if (notes) {
      lines.push(notes)
    } else {
      lines.push(`（未提供 notes.md，路径：${notesFile}）`)
    }

    lines.push("")
    lines.push("影响面评估：")
    const impact = await readFileSnippet(impactFile, 500)
    if (impact) {
      lines.push(impact)
    } else {
      lines.push(`（未提供 impact.md，路径：${impactFile}。编码前必须补齐核心链路、阻塞风险、自测清单和回滚方案。）`)
    }

    lines.push("")
    lines.push("分支与改动：")
    const branch = await readFileSnippet(branchFile, 300)
    if (branch) {
      lines.push(branch)
    } else {
      lines.push(`（未提供 branch.md，路径：${branchFile}）`)
    }

    const phasePrompt = await readPhasePrompt(req.status)
    lines.push("")
    lines.push("阶段执行规范：")
    lines.push(phasePrompt.trim())

    lines.push("")
    lines.push("需求文件：")
    lines.push(`  - 需求记忆：${memoryFile}`)
    lines.push(`  - 需求对齐：${alignmentFile}`)
    lines.push(`  - PRD 来源：${prdFile}`)
    lines.push(`  - 需求背景：${backgroundFile}`)
    lines.push(`  - 分支信息：${branchFile}`)
    lines.push(`  - 开发笔记：${notesFile}`)
    lines.push(`  - 影响面评估：${impactFile}`)
    lines.push(`  - 测试范围：${testFile}`)
    lines.push(`  - 配置变更：${configFile}`)
    lines.push(`  - 上线 Review：${reviewFile}`)

    lines.push("")
    lines.push("AI 路由说明：")
    lines.push("  - 新 session 先读 memory.md 和 alignment.md；prd.md 只用于必要时回溯原始 PRD 来源。")
    lines.push("  - 需求对齐阶段只处理业务目标、范围、规则、验收和未决问题，不进入代码方案。")
    lines.push("  - 编码前必须先读/补 impact.md，确认不会阻塞 WMS 核心链路。")
    lines.push("  - 上线清单以 branch.md、config-changes.md、test.md、review.md 为准。")
    lines.push("  - 测试用例和可复用验证链路维护在 test.md。")
    lines.push("  - 待上线 code review 记录维护在 review.md。")
    lines.push("  - 状态只通过 dashboard/API 更新，不直接改 state.json。")
  } else {
    // No reqDir on the record (should not happen for real Hermes
    // requirements, but stays defensive): fall back to the old behavior.
    const background = await readFileSnippet(req.backgroundPath, 500)
    if (background) lines.push(`需求背景：${background}`)
    const branch = await readFileSnippet(req.branchPath, 300)
    if (branch) lines.push(`分支与改动：${branch}`)
    const notes = await readFileSnippet(req.notesPath, 300)
    if (notes) lines.push(`当前进展：${notes}`)
    const test = await readFileSnippet(req.testPath)
    if (test) lines.push(`测试范围：${test}`)
  }
  lines.push("")
  lines.push(closing)

  // Maintenance instructions — only injected for real requirements with a
  // reqDir, not for DEFAULT_REQ_ID or not-found fallbacks. This shifts
  // requirement-document upkeep from a delayed fork-based extraction into
  // the live session, so the agent that does the work also records it.
  if (req.reqDir) {
    lines.push("")
    lines.push("【需求文档维护 — 必须执行】")
    lines.push(
      "本 session 关联了上述需求文件。以下事件发生后，必须立即更新对应文件，不得跳过：",
    )
    lines.push("- 用户提供 PRD/飞书需求文档，或完成产品/业务口径澄清 → prd.md（来源记录）+ alignment.md（标准化需求对齐结论）")
    lines.push("- 代码 push 或 merge 成功 → branch.md（记录分支名、关键 commit、合并状态）")
    lines.push("- 新增/修改 DB / Apollo / Nacos 配置 → config-changes.md")
    lines.push("- 明确测试场景或回归范围 → test.md")
    lines.push("- 编码前、影响面变化或发现核心链路风险 → impact.md")
    lines.push("- 完成阶段性进展、关键决策、踩坑 → 追加到 notes.md")
    lines.push(
      "重要：更新需求文件是任务的一部分。代码 push 完成但需求文件未更新 = 任务未完成。",
    )
    lines.push(
      "直接编辑上述路径的文件，保持简洁。不要修改 meta.md 的 status 字段（由 dashboard 管理）。",
    )
  }

  return lines.join("\n")
}
