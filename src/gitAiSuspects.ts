/**
 * Role: persistent Agent Panel store for commits that may be missing git-ai
 *   authorship marks, plus the company ai-stats checker used for final status.
 * Public surface: init/list/record/refresh helpers consumed by src/server.tsx.
 * Constraints: company ai-stats is the source of truth; local git notes are
 *   recorded only as timing hints because git-ai may write them asynchronously.
 * Read-this-with: src/server.tsx API routes and web/src/App.tsx GitAiPage.
 */

import { mkdir, readFile, writeFile } from "node:fs/promises"
import { existsSync } from "node:fs"
import { randomBytes } from "node:crypto"
import { homedir } from "node:os"
import { dirname, join } from "node:path"

export type GitAiEventSource = "post-commit" | "pre-push" | "manual" | "api"
export type GitAiLocalNoteState = "complete" | "missing" | "unknown"
export type GitAiCompanyStatus = "pending" | "confirmed_ai" | "missing_ai" | "not_found" | "check_failed"

/** UI/API row for one app + commit that should be checked against ai-stats. */
export interface GitAiSuspectRecord {
  id: string
  projectName: string
  commitSha: string
  shortSha: string
  gitlabProjectId: string | null
  repoPath: string | null
  remoteUrl: string | null
  branch: string | null
  subject: string | null
  authorName: string | null
  eventSources: GitAiEventSource[]
  localNoteState: GitAiLocalNoteState
  companyStatus: GitAiCompanyStatus
  companyCheckedAt: number | null
  companyError: string | null
  commitWebUrl: string | null
  commitTitle: string | null
  committedAt: string | null
  originBranch: string | null
  additions: number | null
  deletions: number | null
  aiRate: number | null
  aiLines: number | null
  humanLines: number | null
  firstSeenAt: number
  lastSeenAt: number
}

export interface GitAiSuspectStats {
  total: number
  pending: number
  confirmedAi: number
  missingAi: number
  notFound: number
  checkFailed: number
}

export interface GitAiCompanyCheckResult {
  companyStatus: Exclude<GitAiCompanyStatus, "pending">
  companyError?: string | null
  commitWebUrl?: string | null
  commitTitle?: string | null
  committedAt?: string | null
  originBranch?: string | null
  additions?: number | null
  deletions?: number | null
  aiRate?: number | null
  aiLines?: number | null
  humanLines?: number | null
}

interface PersistedStore {
  version: 1
  records: GitAiSuspectRecord[]
}

const DEFAULT_STORE_PATH = join(
  homedir(),
  ".local",
  "share",
  "agent-panel",
  "git-ai-suspects.json",
)

const COMPANY_CHECK_ENDPOINT = process.env.AGENT_PANEL_AI_STATS_CHECK_URL || "http://10.24.12.40/api/ai-stats/check-commit"
const STATUS_ORDER: GitAiCompanyStatus[] = ["missing_ai", "pending", "not_found", "check_failed", "confirmed_ai"]

let _storePath = process.env.AGENT_PANEL_GIT_AI_STORE || DEFAULT_STORE_PATH
let _records = new Map<string, GitAiSuspectRecord>()

type CompanyChecker = (record: GitAiSuspectRecord) => Promise<GitAiCompanyCheckResult>

function newId(): string {
  return randomBytes(6).toString("hex")
}

function keyFor(projectName: string, commitSha: string): string {
  return `${projectName}::${commitSha}`
}

function normalizeSha(raw: unknown): string {
  return String(raw || "").trim().toLowerCase()
}

function normalizeProject(raw: unknown): string {
  return String(raw || "").trim()
}

function isValidSha(sha: string): boolean {
  return /^[0-9a-f]{7,40}$/i.test(sha)
}

function isSource(value: string): value is GitAiEventSource {
  return value === "post-commit" || value === "pre-push" || value === "manual" || value === "api"
}

function isLocalNoteState(value: string): value is GitAiLocalNoteState {
  return value === "complete" || value === "missing" || value === "unknown"
}

function normalizeRecord(value: Partial<GitAiSuspectRecord>): GitAiSuspectRecord | null {
  const projectName = normalizeProject(value.projectName)
  const commitSha = normalizeSha(value.commitSha)
  if (!projectName || !isValidSha(commitSha)) return null
  const now = Date.now()
  const sources = Array.isArray(value.eventSources)
    ? value.eventSources.filter((s): s is GitAiEventSource => isSource(String(s)))
    : []
  const localNoteState = isLocalNoteState(String(value.localNoteState)) ? value.localNoteState! : "unknown"
  const companyStatus = STATUS_ORDER.includes(value.companyStatus as GitAiCompanyStatus) ? value.companyStatus! : "pending"
  return {
    id: value.id || newId(),
    projectName,
    commitSha,
    shortSha: commitSha.slice(0, 12),
    gitlabProjectId: value.gitlabProjectId ? String(value.gitlabProjectId) : null,
    repoPath: value.repoPath ? String(value.repoPath) : null,
    remoteUrl: value.remoteUrl ? String(value.remoteUrl) : null,
    branch: value.branch ? String(value.branch) : null,
    subject: value.subject ? String(value.subject) : null,
    authorName: value.authorName ? String(value.authorName) : null,
    eventSources: [...new Set(sources)],
    localNoteState,
    companyStatus,
    companyCheckedAt: typeof value.companyCheckedAt === "number" ? value.companyCheckedAt : null,
    companyError: value.companyError ? String(value.companyError) : null,
    commitWebUrl: value.commitWebUrl ? String(value.commitWebUrl) : null,
    commitTitle: value.commitTitle ? String(value.commitTitle) : null,
    committedAt: value.committedAt ? String(value.committedAt) : null,
    originBranch: value.originBranch ? String(value.originBranch) : null,
    additions: typeof value.additions === "number" ? value.additions : null,
    deletions: typeof value.deletions === "number" ? value.deletions : null,
    aiRate: typeof value.aiRate === "number" ? value.aiRate : null,
    aiLines: typeof value.aiLines === "number" ? value.aiLines : null,
    humanLines: typeof value.humanLines === "number" ? value.humanLines : null,
    firstSeenAt: typeof value.firstSeenAt === "number" ? value.firstSeenAt : now,
    lastSeenAt: typeof value.lastSeenAt === "number" ? value.lastSeenAt : now,
  }
}

async function ensureDir(): Promise<void> {
  const dir = dirname(_storePath)
  if (!existsSync(dir)) await mkdir(dir, { recursive: true })
}

async function loadFromDisk(): Promise<void> {
  _records.clear()
  if (!existsSync(_storePath)) return
  try {
    const raw = await readFile(_storePath, "utf-8")
    const store = JSON.parse(raw) as PersistedStore
    for (const item of store.records || []) {
      const record = normalizeRecord(item)
      if (record) _records.set(keyFor(record.projectName, record.commitSha), record)
    }
  } catch {
    _records.clear()
  }
}

async function saveToDisk(): Promise<void> {
  await ensureDir()
  const records = [..._records.values()].sort((a, b) => b.lastSeenAt - a.lastSeenAt)
  await writeFile(_storePath, JSON.stringify({ version: 1, records }, null, 2) + "\n", "utf-8")
}

/** Load persisted git-ai suspect records at server startup. */
export async function initGitAiSuspects(): Promise<void> {
  await loadFromDisk()
}

/** Record or update one app+commit candidate. Used by hooks and the API. */
export async function recordGitAiSuspect(input: Partial<GitAiSuspectRecord>): Promise<GitAiSuspectRecord> {
  const normalized = normalizeRecord(input)
  if (!normalized) throw new Error("Invalid projectName or commitSha")
  const key = keyFor(normalized.projectName, normalized.commitSha)
  const existing = _records.get(key)
  if (!existing) {
    _records.set(key, normalized)
    await saveToDisk()
    return normalized
  }

  existing.gitlabProjectId = normalized.gitlabProjectId || existing.gitlabProjectId
  existing.repoPath = normalized.repoPath || existing.repoPath
  existing.remoteUrl = normalized.remoteUrl || existing.remoteUrl
  existing.branch = normalized.branch || existing.branch
  existing.subject = normalized.subject || existing.subject
  existing.authorName = normalized.authorName || existing.authorName
  existing.eventSources = [...new Set([...existing.eventSources, ...normalized.eventSources])]
  if (normalized.localNoteState !== "unknown" || existing.localNoteState === "unknown") {
    existing.localNoteState = normalized.localNoteState
  }
  existing.firstSeenAt = Math.min(existing.firstSeenAt, normalized.firstSeenAt)
  existing.lastSeenAt = Math.max(existing.lastSeenAt, normalized.lastSeenAt)
  await saveToDisk()
  return existing
}

/** Return records newest-first, optionally filtered by company status. */
export function listGitAiSuspects(status?: GitAiCompanyStatus): GitAiSuspectRecord[] {
  const records = [..._records.values()]
  records.sort((a, b) => {
    const statusDelta = STATUS_ORDER.indexOf(a.companyStatus) - STATUS_ORDER.indexOf(b.companyStatus)
    return statusDelta || b.lastSeenAt - a.lastSeenAt
  })
  return status ? records.filter((r) => r.companyStatus === status) : records
}

/** Count records by final/company-check status for KPI cards. */
export function buildGitAiSuspectStats(records = listGitAiSuspects()): GitAiSuspectStats {
  return {
    total: records.length,
    pending: records.filter((r) => r.companyStatus === "pending").length,
    confirmedAi: records.filter((r) => r.companyStatus === "confirmed_ai").length,
    missingAi: records.filter((r) => r.companyStatus === "missing_ai").length,
    notFound: records.filter((r) => r.companyStatus === "not_found").length,
    checkFailed: records.filter((r) => r.companyStatus === "check_failed").length,
  }
}

function numberOrNull(value: unknown): number | null {
  const n = typeof value === "number" ? value : Number(value)
  return Number.isFinite(n) ? n : null
}

function hasCompanyAiMark(payload: any): boolean {
  const aiNote = payload?.ai_note
  const stats = payload?.stats
  return Boolean(
    numberOrNull(aiNote?.ai_lines_total) ||
    numberOrNull(aiNote?.frontmatter_ai_lines) ||
    numberOrNull(aiNote?.prompts_count) ||
    numberOrNull(stats?.ai_additions) ||
    (numberOrNull(stats?.ai_rate) ?? 0) > 0,
  )
}

async function defaultCompanyChecker(record: GitAiSuspectRecord): Promise<GitAiCompanyCheckResult> {
  const url = new URL(COMPANY_CHECK_ENDPOINT)
  url.searchParams.set("project_name", record.projectName)
  url.searchParams.set("commit_sha", record.commitSha)
  if (record.gitlabProjectId) url.searchParams.set("gitlab_project_id", record.gitlabProjectId)
  try {
    const res = await fetch(url, {
      headers: { Accept: "application/json" },
      signal: AbortSignal.timeout(6_000),
    })
    const text = await res.text()
    let payload: any = null
    try { payload = text ? JSON.parse(text) : null } catch { /* non-json response */ }
    if (!res.ok) {
      return { companyStatus: "check_failed", companyError: payload?.detail || `HTTP ${res.status}` }
    }
    if (payload?.detail && !payload?.commit) {
      return { companyStatus: "not_found", companyError: String(payload.detail) }
    }
    if (!payload?.commit) {
      return { companyStatus: "check_failed", companyError: "公司接口未返回 commit 对象" }
    }
    const stats = payload.stats || {}
    const noteMarked = hasCompanyAiMark(payload)
    return {
      companyStatus: noteMarked ? "confirmed_ai" : "missing_ai",
      companyError: null,
      commitWebUrl: payload.commit.web_url || null,
      commitTitle: payload.commit.title || null,
      committedAt: payload.commit.committed_at || null,
      originBranch: payload.commit.origin_branch || payload.commit.branch || null,
      additions: numberOrNull(payload.commit.additions),
      deletions: numberOrNull(payload.commit.deletions),
      aiRate: numberOrNull(stats.ai_rate),
      aiLines: numberOrNull(stats.ai_additions ?? payload.ai_note?.ai_lines_total),
      humanLines: numberOrNull(stats.human_additions),
    }
  } catch (err) {
    return { companyStatus: "check_failed", companyError: err instanceof Error ? err.message : String(err) }
  }
}

function applyCompanyResult(record: GitAiSuspectRecord, result: GitAiCompanyCheckResult, checkedAt: number): void {
  record.companyStatus = result.companyStatus
  record.companyCheckedAt = checkedAt
  record.companyError = result.companyError ?? null
  record.commitWebUrl = result.commitWebUrl ?? record.commitWebUrl
  record.commitTitle = result.commitTitle ?? record.commitTitle
  record.committedAt = result.committedAt ?? record.committedAt
  record.originBranch = result.originBranch ?? record.originBranch
  record.additions = result.additions ?? record.additions
  record.deletions = result.deletions ?? record.deletions
  record.aiRate = result.aiRate ?? record.aiRate
  record.aiLines = result.aiLines ?? record.aiLines
  record.humanLines = result.humanLines ?? record.humanLines
}

/** Refresh company ai-stats status; final missing/confirmed state comes only from that API. */
export async function refreshGitAiSuspects(opts: { limit?: number; checker?: CompanyChecker } = {}): Promise<GitAiSuspectRecord[]> {
  const checker = opts.checker || defaultCompanyChecker
  const records = listGitAiSuspects().slice(0, opts.limit ?? 200)
  for (const record of records) {
    const result = await checker(record)
    applyCompanyResult(record, result, Date.now())
  }
  await saveToDisk()
  return listGitAiSuspects()
}

/** Test-only: isolate the persistent store path and clear memory. */
export function _resetGitAiSuspectsForTest(path: string): void {
  _storePath = path
  _records.clear()
}

/** Test-only: load records from JSON without touching disk. */
export function _loadGitAiSuspectsForTest(json: string): void {
  _records.clear()
  const parsed = JSON.parse(json) as PersistedStore
  for (const item of parsed.records || []) {
    const record = normalizeRecord(item)
    if (record) _records.set(keyFor(record.projectName, record.commitSha), record)
  }
}
