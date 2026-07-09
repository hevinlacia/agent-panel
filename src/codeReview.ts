/**
 * Requirement code-review snapshot builder.
 *
 * Role: turn a requirement's `branches.json` scope into a human-reviewable
 * PRO/base-branch diff package and persist the reviewer verdict.
 * Public surface: runCodeReviewScan, readCodeReviewSnapshot,
 * saveCodeReviewVerdict, upsertCodeReviewBlock, and exported types/constants.
 * Constraints / safety: fixed `git` argv only (no shell), reads only repo Git
 * metadata/diffs, and writes only `<req-dir>/code-review.json` + review.md.
 * Read-this-with: src/branchScope.ts and src/server.tsx CodeReviewCard.
 */

import { spawn } from "node:child_process"
import { existsSync } from "node:fs"
import { mkdir, readFile, rename, writeFile } from "node:fs/promises"
import { homedir } from "node:os"
import { dirname, isAbsolute, join, resolve } from "node:path"

import type { BranchRepo, BranchScope } from "./branchScope.ts"

export const CODE_REVIEW_FILE = "code-review.json"
export const DEFAULT_CODE_REVIEW_BASE_REF = "origin/master"
export const CODE_REVIEW_BLOCK_START = "<!-- agent-panel:code-review:start -->"
export const CODE_REVIEW_BLOCK_END = "<!-- agent-panel:code-review:end -->"

export type CodeReviewStatus = "not_started" | "approved" | "changes_requested" | "blocked"

export const CODE_REVIEW_STATUSES: CodeReviewStatus[] = [
  "not_started",
  "approved",
  "changes_requested",
  "blocked",
]

export interface CodeReviewGitStep {
  label: string
  command: string
  ok: boolean
  stdout?: string
  stderr?: string
}

export interface CodeReviewBaseUpdate {
  ok: boolean
  remote: string
  remoteBranch: string
  localBranch: string
  steps: CodeReviewGitStep[]
}

export interface CodeReviewFile {
  path: string
  status: string
  additions: number
  deletions: number
  riskTags: string[]
}

export interface CodeReviewRepoSnapshot {
  repoName: string
  projectPath?: string
  branch: string
  resolvedTargetRef: string
  baseRef: string
  currentBranch?: string
  dirty: boolean
  baseUpdate: CodeReviewBaseUpdate
  commits: string[]
  files: CodeReviewFile[]
  additions: number
  deletions: number
  diff: string
  diffTruncated: boolean
  warnings: string[]
  error?: string
}

export interface CodeReviewVerdict {
  status: CodeReviewStatus
  reviewer: string
  summary: string
  items: string[]
  updatedAt: number
}

export interface CodeReviewSnapshot {
  version: 1
  reqId: string
  updatedAt: number
  baseRef: string
  sourceFallback?: boolean
  repos: CodeReviewRepoSnapshot[]
  verdict?: CodeReviewVerdict
}

interface GitCommandResult {
  ok: boolean
  code: number | null
  command: string
  stdout: string
  stderr: string
  outputTruncated: boolean
  timedOut?: boolean
}

interface BaseRefInfo {
  baseRef: string
  remote: string
  remoteBranch: string
  localBranch: string
}

const COMMAND_OUTPUT_LIMIT = 80_000
const DIFF_OUTPUT_LIMIT = 180_000

/** Read the persisted review snapshot from `<req-dir>/code-review.json`. */
export async function readCodeReviewSnapshot(reqDir: string): Promise<CodeReviewSnapshot | null> {
  const p = join(reqDir, CODE_REVIEW_FILE)
  if (!existsSync(p)) return null
  try {
    const parsed = JSON.parse(await readFile(p, "utf-8")) as CodeReviewSnapshot
    if (!parsed || parsed.version !== 1 || !Array.isArray(parsed.repos)) return null
    return parsed
  } catch {
    return null
  }
}

/**
 * Build and persist a review snapshot for all repos/branches in scope.
 * Before diffing, each repo refreshes the configured production base via
 * `git fetch`, then best-effort fast-forwards the matching local branch.
 */
export async function runCodeReviewScan(
  reqDir: string,
  reqId: string,
  scope: BranchScope,
  opts: { baseRef?: string } = {},
): Promise<CodeReviewSnapshot> {
  const baseInfo = parseBaseRef(opts.baseRef || DEFAULT_CODE_REVIEW_BASE_REF)
  const repos: CodeReviewRepoSnapshot[] = []
  for (const repo of scope.repos) {
    const branches = repo.branches.length > 0 ? repo.branches : [""]
    for (const branch of branches) {
      repos.push(await scanRepoBranch(repo, branch, baseInfo))
    }
  }
  const snapshot: CodeReviewSnapshot = {
    version: 1,
    reqId,
    updatedAt: Date.now(),
    baseRef: baseInfo.baseRef,
    sourceFallback: scope.fallback || undefined,
    repos,
  }
  await writeCodeReviewSnapshot(reqDir, snapshot)
  return snapshot
}

/** Persist a reviewer verdict and mirror it into review.md as a managed block. */
export async function saveCodeReviewVerdict(
  reqDir: string,
  snapshot: CodeReviewSnapshot,
  verdict: CodeReviewVerdict,
): Promise<CodeReviewSnapshot> {
  const next: CodeReviewSnapshot = { ...snapshot, verdict, updatedAt: Date.now() }
  await writeCodeReviewSnapshot(reqDir, next)
  const reviewPath = join(reqDir, "review.md")
  const existing = existsSync(reviewPath) ? await readFile(reviewPath, "utf-8").catch(() => "") : ""
  await writeFile(reviewPath, upsertCodeReviewBlock(existing, next), "utf-8")
  return next
}

/** Replace or append the managed code-review block inside review.md. */
export function upsertCodeReviewBlock(existing: string, snapshot: CodeReviewSnapshot): string {
  const block = buildCodeReviewMarkdown(snapshot)
  const start = existing.indexOf(CODE_REVIEW_BLOCK_START)
  const end = existing.indexOf(CODE_REVIEW_BLOCK_END)
  if (start >= 0 && end > start) {
    const afterEnd = end + CODE_REVIEW_BLOCK_END.length
    return existing.slice(0, start).trimEnd() + "\n\n" + block + existing.slice(afterEnd)
  }
  return existing.trimEnd() + (existing.trim() ? "\n\n" : "") + block + "\n"
}

/** Classify changed files into lightweight review-risk tags for the UI. */
export function classifyCodeReviewRiskTags(path: string, status: string, additions = 0, deletions = 0): string[] {
  const p = path.toLowerCase()
  const tags: string[] = []
  if (/controller|endpoint|resource/.test(p)) tags.push("入口/API")
  if (/listener|consumer|producer|rocketmq|rabbitmq|kafka|mq/.test(p)) tags.push("MQ")
  if (/mapper|dao|repository|\.xml$|\.sql$/.test(p)) tags.push("DB")
  if (/apollo|nacos|bootstrap|application|config|\.ya?ml$|\.properties$/.test(p)) tags.push("配置")
  if (/transaction|lock|inventory|stock|shipment|receipt|outbound|inbound/.test(p)) tags.push("核心链路")
  if (/test|spec/.test(p)) tags.push("测试")
  if (/^d/i.test(status)) tags.push("删除")
  if (additions + deletions >= 300) tags.push("大改动")
  return [...new Set(tags)]
}

async function writeCodeReviewSnapshot(reqDir: string, snapshot: CodeReviewSnapshot): Promise<void> {
  const p = join(reqDir, CODE_REVIEW_FILE)
  await mkdir(dirname(p), { recursive: true })
  const tmp = `${p}.tmp-${process.pid}-${Date.now()}`
  await writeFile(tmp, JSON.stringify(snapshot, null, 2) + "\n", "utf-8")
  await rename(tmp, p)
}

async function scanRepoBranch(repo: BranchRepo, branch: string, baseInfo: BaseRefInfo): Promise<CodeReviewRepoSnapshot> {
  const warnings: string[] = []
  const projectPath = resolveProjectPath(repo.projectPath)
  const emptyBaseUpdate: CodeReviewBaseUpdate = {
    ok: false,
    remote: baseInfo.remote,
    remoteBranch: baseInfo.remoteBranch,
    localBranch: baseInfo.localBranch,
    steps: [],
  }
  if (!projectPath) {
    return emptyRepoSnapshot(repo, branch || "(未指定分支)", baseInfo.baseRef, emptyBaseUpdate, warnings, "branches.json 缺少 projectPath")
  }
  if (!existsSync(projectPath)) {
    return emptyRepoSnapshot(repo, branch || "(未指定分支)", baseInfo.baseRef, emptyBaseUpdate, warnings, `仓库路径不存在：${projectPath}`)
  }
  if (!branch) {
    return emptyRepoSnapshot(repo, "(未指定分支)", baseInfo.baseRef, emptyBaseUpdate, warnings, "branches.json 缺少需求分支")
  }

  const gitRoot = await git(projectPath, ["rev-parse", "--show-toplevel"])
  if (!gitRoot.ok) {
    return emptyRepoSnapshot(repo, branch, baseInfo.baseRef, emptyBaseUpdate, warnings, "projectPath 不是 Git 仓库")
  }

  const [currentBranch, dirtyState, baseUpdate] = await Promise.all([
    git(projectPath, ["rev-parse", "--abbrev-ref", "HEAD"]),
    git(projectPath, ["status", "--porcelain"], { maxOutput: COMMAND_OUTPUT_LIMIT }),
    refreshProductionBase(projectPath, baseInfo),
  ])
  if (!baseUpdate.ok) warnings.push("生产基线刷新存在失败步骤，请检查命令输出")

  const target = await resolveTargetRef(projectPath, branch)
  if (target.warning) warnings.push(target.warning)

  const commitResult = await git(projectPath, ["log", "--oneline", "--decorate=short", "--max-count=80", `${baseInfo.baseRef}..${target.ref}`])
  if (!commitResult.ok) warnings.push("提交列表读取失败：" + shortErr(commitResult))

  const nameStatus = await git(projectPath, ["diff", "--name-status", "--find-renames", `${baseInfo.baseRef}...${target.ref}`, "--"])
  const numstat = await git(projectPath, ["diff", "--numstat", "--find-renames", `${baseInfo.baseRef}...${target.ref}`, "--"])
  if (!nameStatus.ok) warnings.push("文件列表读取失败：" + shortErr(nameStatus))
  if (!numstat.ok) warnings.push("增删行统计读取失败：" + shortErr(numstat))

  const diff = await git(projectPath, ["diff", "--no-ext-diff", "--no-color", "--find-renames", "--unified=80", `${baseInfo.baseRef}...${target.ref}`, "--"], {
    maxOutput: DIFF_OUTPUT_LIMIT,
    timeoutMs: 60_000,
  })
  if (!diff.ok) warnings.push("Diff 读取失败：" + shortErr(diff))

  const files = mergeFileStats(nameStatus.stdout, numstat.stdout)
  return {
    repoName: repo.repoName,
    projectPath,
    branch,
    resolvedTargetRef: target.ref,
    baseRef: baseInfo.baseRef,
    currentBranch: currentBranch.ok ? currentBranch.stdout.trim() : undefined,
    dirty: dirtyState.ok ? !!dirtyState.stdout.trim() : false,
    baseUpdate,
    commits: commitResult.ok ? commitResult.stdout.split("\n").filter(Boolean) : [],
    files,
    additions: files.reduce((n, f) => n + f.additions, 0),
    deletions: files.reduce((n, f) => n + f.deletions, 0),
    diff: diff.ok ? diff.stdout : "",
    diffTruncated: diff.outputTruncated,
    warnings,
    error: diff.ok || files.length > 0 ? undefined : shortErr(diff),
  }
}

function emptyRepoSnapshot(
  repo: BranchRepo,
  branch: string,
  baseRef: string,
  baseUpdate: CodeReviewBaseUpdate,
  warnings: string[],
  error: string,
): CodeReviewRepoSnapshot {
  return {
    repoName: repo.repoName,
    projectPath: resolveProjectPath(repo.projectPath) || repo.projectPath,
    branch,
    resolvedTargetRef: branch,
    baseRef,
    dirty: false,
    baseUpdate,
    commits: [],
    files: [],
    additions: 0,
    deletions: 0,
    diff: "",
    diffTruncated: false,
    warnings,
    error,
  }
}

function resolveProjectPath(projectPath?: string): string | undefined {
  if (!projectPath || !projectPath.trim()) return undefined
  const raw = projectPath.trim()
  const expanded = raw === "~" ? homedir() : raw.startsWith("~/") ? join(homedir(), raw.slice(2)) : raw
  return isAbsolute(expanded) ? expanded : resolve(expanded)
}

function parseBaseRef(input: string): BaseRefInfo {
  const baseRef = input.trim() || DEFAULT_CODE_REVIEW_BASE_REF
  if (baseRef.includes("/") && !baseRef.startsWith("refs/")) {
    const [remote, ...rest] = baseRef.split("/")
    const remoteBranch = rest.join("/") || "master"
    return { baseRef, remote, remoteBranch, localBranch: remoteBranch }
  }
  return { baseRef, remote: "origin", remoteBranch: baseRef, localBranch: baseRef }
}

async function refreshProductionBase(repoPath: string, info: BaseRefInfo): Promise<CodeReviewBaseUpdate> {
  const steps: CodeReviewGitStep[] = []
  const add = (label: string, result: GitCommandResult) => {
    steps.push({ label, command: result.command, ok: result.ok, stdout: compact(result.stdout), stderr: compact(result.stderr) })
  }

  add("fetch production base", await git(repoPath, ["fetch", "--prune", info.remote, info.remoteBranch], { timeoutMs: 60_000 }))

  const current = await git(repoPath, ["rev-parse", "--abbrev-ref", "HEAD"])
  const currentName = current.ok ? current.stdout.trim() : ""
  const localRef = `refs/heads/${info.localBranch}`
  const hasLocal = await git(repoPath, ["show-ref", "--verify", "--quiet", localRef])

  if (currentName === info.localBranch) {
    add("fast-forward checked-out local base", await git(repoPath, ["pull", "--ff-only", info.remote, info.remoteBranch], { timeoutMs: 60_000 }))
  } else if (hasLocal.ok) {
    add("fast-forward local base ref", await git(repoPath, ["fetch", info.remote, `${info.remoteBranch}:${localRef}`], { timeoutMs: 60_000 }))
  } else {
    add("create local base tracking ref", await git(repoPath, ["branch", "--track", info.localBranch, `${info.remote}/${info.remoteBranch}`]))
  }

  return {
    ok: steps.every((s) => s.ok),
    remote: info.remote,
    remoteBranch: info.remoteBranch,
    localBranch: info.localBranch,
    steps,
  }
}

async function resolveTargetRef(repoPath: string, branch: string): Promise<{ ref: string; warning?: string }> {
  const local = await git(repoPath, ["rev-parse", "--verify", `${branch}^{commit}`])
  if (local.ok) return { ref: branch }
  const remoteBranch = `origin/${branch}`
  const remote = await git(repoPath, ["rev-parse", "--verify", `${remoteBranch}^{commit}`])
  if (remote.ok) return { ref: remoteBranch, warning: `本地分支 ${branch} 不存在，已使用 ${remoteBranch}` }
  return { ref: branch, warning: `无法验证需求分支 ${branch}，diff 可能失败` }
}

function mergeFileStats(nameStatusOut: string, numstatOut: string): CodeReviewFile[] {
  const byPath = new Map<string, { path: string; status: string; additions: number; deletions: number }>()
  for (const line of nameStatusOut.split("\n")) {
    if (!line.trim()) continue
    const cols = line.split("\t")
    const status = cols[0] || "M"
    const path = cols.length >= 3 && /^[RC]/.test(status) ? cols[2] : cols[1]
    if (!path) continue
    byPath.set(path, { path, status, additions: 0, deletions: 0 })
  }
  for (const line of numstatOut.split("\n")) {
    if (!line.trim()) continue
    const cols = line.split("\t")
    if (cols.length < 3) continue
    const additions = parseNumstat(cols[0])
    const deletions = parseNumstat(cols[1])
    const path = normalizeNumstatPath(cols.slice(2).join("\t"))
    const cur = byPath.get(path) || { path, status: "M", additions: 0, deletions: 0 }
    cur.additions = additions
    cur.deletions = deletions
    byPath.set(path, cur)
  }
  return [...byPath.values()]
    .map((f) => ({ ...f, riskTags: classifyCodeReviewRiskTags(f.path, f.status, f.additions, f.deletions) }))
    .sort((a, b) => a.path.localeCompare(b.path))
}

function parseNumstat(s: string): number {
  const n = Number(s)
  return Number.isFinite(n) ? n : 0
}

function normalizeNumstatPath(raw: string): string {
  // Rename lines can be `old => new`; the new path is what reviewers open.
  const arrow = raw.match(/=>\s*(.*)$/)
  return (arrow ? arrow[1] : raw).replace(/[{}]/g, "").trim()
}

function buildCodeReviewMarkdown(snapshot: CodeReviewSnapshot): string {
  const verdict = snapshot.verdict
  const status = verdict?.status || "not_started"
  const files = snapshot.repos.reduce((n, r) => n + r.files.length, 0)
  const additions = snapshot.repos.reduce((n, r) => n + r.additions, 0)
  const deletions = snapshot.repos.reduce((n, r) => n + r.deletions, 0)
  const lines = [
    CODE_REVIEW_BLOCK_START,
    "## Code Review（人工）",
    "",
    `- 结论：${statusLabel(status)}`,
    `- Reviewer：${verdict?.reviewer || "待填写"}`,
    `- 更新时间：${new Date(verdict?.updatedAt || snapshot.updatedAt).toISOString()}`,
    `- 对比基线：${snapshot.baseRef}`,
    `- 变更规模：${snapshot.repos.length} 个仓库/分支，${files} 个文件，+${additions}/-${deletions}`,
    "",
    "### Review 摘要",
    verdict?.summary?.trim() || "- 待填写",
    "",
    "### 待修复 / 关注项",
    ...(verdict?.items?.length ? verdict.items.map((x) => `- ${x}`) : ["- 待填写"]),
    "",
    "### Diff 范围",
  ]
  for (const repo of snapshot.repos) {
    const flags = [repo.baseUpdate.ok ? "基线已刷新" : "基线刷新异常", repo.dirty ? "工作区有未提交改动" : "工作区干净"]
    lines.push(`- ${repo.repoName} / ${repo.branch}：${repo.files.length} 文件，+${repo.additions}/-${repo.deletions}（${flags.join("；")}）`)
  }
  lines.push(CODE_REVIEW_BLOCK_END)
  return lines.join("\n")
}

function statusLabel(status: CodeReviewStatus): string {
  if (status === "approved") return "通过"
  if (status === "changes_requested") return "需修改"
  if (status === "blocked") return "阻塞"
  return "未开始"
}

async function git(
  cwd: string,
  args: string[],
  opts: { timeoutMs?: number; maxOutput?: number } = {},
): Promise<GitCommandResult> {
  const timeoutMs = opts.timeoutMs ?? 30_000
  const maxOutput = opts.maxOutput ?? COMMAND_OUTPUT_LIMIT
  const command = ["git", ...args].map(shellQuote).join(" ")
  return new Promise((resolveResult) => {
    const child = spawn("git", args, { cwd, stdio: ["ignore", "pipe", "pipe"] })
    let stdout = ""
    let stderr = ""
    let outputTruncated = false
    let settled = false
    let timedOut = false
    const append = (cur: string, chunk: Buffer): string => {
      if (cur.length >= maxOutput) {
        outputTruncated = true
        return cur
      }
      const s = chunk.toString("utf-8")
      const room = maxOutput - cur.length
      if (s.length > room) outputTruncated = true
      return cur + s.slice(0, room)
    }
    const timer = setTimeout(() => {
      timedOut = true
      child.kill("SIGTERM")
    }, timeoutMs)
    child.stdout.on("data", (d: Buffer) => { stdout = append(stdout, d) })
    child.stderr.on("data", (d: Buffer) => { stderr = append(stderr, d) })
    child.on("error", (err) => {
      if (settled) return
      settled = true
      clearTimeout(timer)
      resolveResult({ ok: false, code: null, command, stdout, stderr: err.message, outputTruncated, timedOut })
    })
    child.on("close", (code) => {
      if (settled) return
      settled = true
      clearTimeout(timer)
      if (timedOut) stderr = (stderr + "\nTimed out").trim()
      resolveResult({ ok: code === 0 && !timedOut, code, command, stdout, stderr, outputTruncated, timedOut })
    })
  })
}

function shellQuote(s: string): string {
  if (/^[A-Za-z0-9_./:=@%+-]+$/.test(s)) return s
  return JSON.stringify(s)
}

function compact(s: string, max = 600): string | undefined {
  const t = s.trim()
  if (!t) return undefined
  return t.length > max ? t.slice(0, max) + "…" : t
}

function shortErr(result: GitCommandResult): string {
  return compact(result.stderr || result.stdout || `exit ${result.code}`) || "unknown git error"
}
