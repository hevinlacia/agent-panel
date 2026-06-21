/**
 * Requirement (需求) data layer — Hermes-backed.
 *
 * Requirement records live as Markdown directories under `~/.agents/req/`,
 * managed by the Hermes `req-tracker` skill. The dashboard owns only
 * session associations, persisted at
 * `~/.local/share/opencode-dashboard/associations.json`.
 *
 * Tests can override the associations store path via `_setStorePath`.
 *
 * Only `node:` built-ins are used. Never reads or writes any
 * `.env` / secret file.
 */

import { mkdir, readFile, readdir, stat, writeFile } from "node:fs/promises"
import { existsSync } from "node:fs"
import { homedir } from "node:os"
import { dirname, join } from "node:path"
import { randomBytes } from "node:crypto"

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

export type ReqStatus = "待设计" | "待开发" | "开发中" | "自测中" | "测试中" | "待上线" | "已完成"

export const REQ_STATUSES: ReqStatus[] = [
  "待设计",
  "待开发",
  "开发中",
  "自测中",
  "测试中",
  "待上线",
  "已完成",
]

export interface Requirement {
  id: string
  title: string
  status: ReqStatus
  project: string
  description: string
  sessionIds: string[]
  createdAt: number
  updatedAt: number
  metaPath?: string
  branchPath?: string
  testPath?: string
  notesPath?: string
  configPath?: string
}

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

export const DEFAULT_REQ_ID = "__default__"
export const DEFAULT_PROJECT_NAME = "默认项目"

const REQ_DIR = join(homedir(), ".agents", "req")

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
  "opencode-dashboard",
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

function isReqStatus(v: unknown): v is ReqStatus {
  return typeof v === "string" && (REQ_STATUSES as string[]).includes(v)
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

async function loadRequirementFromDir(
  dirPath: string,
  dirName: string,
  parentProject: string
): Promise<Requirement | null> {
  let st
  try {
    st = await stat(dirPath)
  } catch {
    return null
  }
  if (!st.isDirectory()) return null

  const metaPath = join(dirPath, "meta.md")
  const branchPath = join(dirPath, "branch.md")
  const testPath = join(dirPath, "test.md")
  const notesPath = join(dirPath, "notes.md")
  const configPath = join(dirPath, "config-changes.md")

  let title = dirName
  let status: ReqStatus = "开发中"
  let project = parentProject
  let description = ""
  let id = dirName
  let createdAt = st.mtimeMs

  let metaPresent = false
  if (existsSync(metaPath)) {
    metaPresent = true
    try {
      const raw = await readFile(metaPath, "utf-8")
      const fm = parseFrontmatter(raw)
      const fields = fm.fields
      if (fields["req-id"]) id = fields["req-id"]
      if (fields["title"]) title = fields["title"]
      const rawStatus = fields["status"]
      if (isReqStatus(rawStatus)) status = rawStatus
      if (fields["project"] && fields["project"].trim()) {
        project = fields["project"].trim()
      }
      const sd = parseStartDate(fields["start-date"])
      if (sd !== null) createdAt = sd
      const desc = firstParagraph(fm.body)
      if (desc) description = desc
    } catch {
      // Keep defaults.
    }
  }

  return {
    id,
    title,
    status,
    project,
    description,
    sessionIds: [],
    createdAt,
    updatedAt: st.mtimeMs,
    metaPath: metaPresent ? metaPath : undefined,
    branchPath: existsSync(branchPath) ? branchPath : undefined,
    testPath: existsSync(testPath) ? testPath : undefined,
    notesPath: existsSync(notesPath) ? notesPath : undefined,
    configPath: existsSync(configPath) ? configPath : undefined,
  }
}

export async function scanHermesRequirements(): Promise<Requirement[]> {
  if (!existsSync(REQ_DIR)) return []
  let topEntries: string[]
  try {
    topEntries = await readdir(REQ_DIR)
  } catch {
    return []
  }
  const out: Requirement[] = []
  for (const name of topEntries) {
    if (name === "README.md" || name.startsWith(".")) continue
    const topPath = join(REQ_DIR, name)
    let topSt
    try {
      topSt = await stat(topPath)
    } catch {
      continue
    }
    if (!topSt.isDirectory()) continue

    // Resolve this directory's display project name. `_default` maps to
    // the synthetic default project name.
    const projectDisplay =
      name === "_default" ? DEFAULT_PROJECT_NAME : name

    const hasOwnMeta = existsSync(join(topPath, "meta.md"))

    if (hasOwnMeta) {
      // Legacy flat layout: ~/.agents/req/<req-id>/meta.md
      // project comes from frontmatter or defaults to DEFAULT_PROJECT_NAME.
      const req = await loadRequirementFromDir(
        topPath,
        name,
        DEFAULT_PROJECT_NAME
      )
      if (req) out.push(req)
      continue
    }

    // Two-level layout: ~/.agents/req/<project>/<req-id>/
    let children: string[]
    try {
      children = await readdir(topPath)
    } catch {
      continue
    }
    for (const reqName of children) {
      if (reqName.startsWith(".")) continue
      const reqPath = join(topPath, reqName)
      const req = await loadRequirementFromDir(reqPath, reqName, projectDisplay)
      if (req) out.push(req)
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
    project: DEFAULT_PROJECT_NAME,
    description:
      "未关联到具体需求的 session 归属到此默认需求。如需独立管理，可在 ~/.agents/req/ 下创建对应需求目录后重新关联。",
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

export async function listRequirementsByProject(): Promise<
  { project: string; requirements: Requirement[] }[]
> {
  const [hermes, store] = await Promise.all([
    scanHermesRequirements(),
    loadAssociations(),
  ])

  // Attach sessionIds from associations.
  const hermesIds = new Set(hermes.map((r) => r.id))
  for (const r of hermes) {
    r.sessionIds = store.associations[r.id] ?? []
  }

  // Build the synthetic default requirement: it owns sessions under
  // DEFAULT_REQ_ID *and* any sessions associated with reqIds that no
  // longer exist in Hermes (orphaned associations).
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
  const defaultReq = buildDefaultRequirement(defaultSessions)

  // Group by project.
  const groups = new Map<string, Requirement[]>()
  // Track the latest updatedAt per non-default project to drive sort order.
  const projectLatest = new Map<string, number>()
  for (const r of hermes) {
    const proj = r.project || DEFAULT_PROJECT_NAME
    const bucket = groups.get(proj) ?? []
    bucket.push(r)
    groups.set(proj, bucket)
    const cur = projectLatest.get(proj) ?? 0
    if (r.updatedAt > cur) projectLatest.set(proj, r.updatedAt)
  }

  // Always include the default project (even if empty, it carries the
  // synthetic default requirement and any orphan sessions).
  const defaultBucket = groups.get(DEFAULT_PROJECT_NAME) ?? []
  defaultBucket.push(defaultReq)
  groups.set(DEFAULT_PROJECT_NAME, defaultBucket)

  // Sort: non-default projects by updatedAt desc, default project last.
  const nonDefault = [...groups.keys()]
    .filter((p) => p !== DEFAULT_PROJECT_NAME)
    .sort((a, b) => (projectLatest.get(b) ?? 0) - (projectLatest.get(a) ?? 0))
  const ordered = [...nonDefault, DEFAULT_PROJECT_NAME]

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

export async function buildInjectionContext(reqId: string): Promise<string> {
  if (reqId === DEFAULT_REQ_ID) {
    return [
      "【需求上下文】",
      "需求：默认需求",
      "状态：开发中",
      "请基于以上需求上下文继续。",
    ].join("\n")
  }
  const hermes = await scanHermesRequirements()
  const req = hermes.find((r) => r.id === reqId)
  if (!req) {
    return [
      "【需求上下文】",
      "需求：默认需求",
      "状态：开发中",
      "请基于以上需求上下文继续。",
    ].join("\n")
  }
  const lines: string[] = []
  lines.push("【需求上下文】")
  lines.push(`需求：${req.title}`)
  lines.push(`状态：${req.status}`)
  const branch = await readFileSnippet(req.branchPath)
  if (branch) lines.push(`分支信息：${branch}`)
  const notes = await readFileSnippet(req.notesPath)
  if (notes) lines.push(`开发笔记：${notes}`)
  const test = await readFileSnippet(req.testPath)
  if (test) lines.push(`测试范围：${test}`)
  lines.push("请基于以上需求上下文继续。")
  return lines.join("\n")
}
