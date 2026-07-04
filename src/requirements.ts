/**
 * Requirement (需求) data layer — Hermes-backed.
 *
 * Requirement records live as Markdown directories under `~/.agents/req/`,
 * managed by the Hermes `req-tracker` skill. The dashboard owns only
 * session associations, persisted at
 * `~/.local/share/opencode-dashboard/associations.json`.
 * `alignment.md` is the product/business alignment brief for the first
 * requirement phase; `prd.md` is kept as the original-source trace.
 * `impact.md` is the pre-coding safety gate for business-flow risk.
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

import { ALIGNMENT_FILE, PRD_FILE } from "./requirementAlignment.ts"
import { readRequirementState } from "./requirementState.ts"

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

export type ReqStatus = "需求对齐" | "待开发" | "开发中" | "自测中" | "测试中" | "待上线" | "已完成"

export const REQ_STATUSES: ReqStatus[] = [
  "需求对齐",
  "待开发",
  "开发中",
  "自测中",
  "测试中",
  "待上线",
  "已完成",
]

/**
 * Status-specific execution contract injected into requirement-bound sessions.
 * The current requirement status acts as a lightweight role switch so each
 * phase gets different required context, prohibitions, and completion criteria.
 */
export interface RequirementPhaseProfile {
  role: string
  mustRead: string[]
  mustDo: string[]
  mustNotDo: string[]
  doneCriteria: string[]
}

/**
 * Dashboard-owned phase profiles keyed by the canonical requirement statuses.
 * Keep this exhaustive with `REQ_STATUSES`; tests assert every status has a
 * non-empty profile so new statuses cannot silently skip phase guidance.
 */
export const REQUIREMENT_PHASE_PROFILES: Record<ReqStatus, RequirementPhaseProfile> = {
  需求对齐: {
    role: "业务需求对齐者",
    mustRead: ["alignment.md", "memory.md", "background.md", "prd.md"],
    mustDo: ["只和产品或业务对齐真实业务诉求、目标、范围、验收口径和未决问题", "把 PRD 或飞书原文提炼成 alignment.md 标准格式", "在 memory.md/background.md 记录已确认结论和开放问题"],
    mustNotDo: ["讨论代码实现、技术方案、分支或改造方式", "把 PRD 原文当作后续阶段的主要上下文", "在业务口径未明确时推进到开发"],
    doneCriteria: ["alignment.md 已覆盖业务目标、范围、场景、规则、验收口径、非目标和未决问题", "prd.md 只保留来源链接/摘要/转化记录，后续阶段无需默认重读 PRD"],
  },
  待开发: {
    role: "技术方案 / 任务拆解者",
    mustRead: ["memory.md", "background.md", "impact.md", "branch.md", "config-changes.md"],
    mustDo: ["确认仓库、分支、基准分支和影响模块", "补齐编码前影响面与配置变更预估", "拆出可验证的开发任务"],
    mustNotDo: ["未确认目标分支就改动或提交", "遗漏 DB/Apollo/Nacos/RocketMQ 配置影响"],
    doneCriteria: ["branch.md 和 impact.md 足够指导开发", "config-changes.md 记录已知或预计配置变更"],
  },
  开发中: {
    role: "代码实现者",
    mustRead: ["memory.md", "impact.md", "branch.md", "config-changes.md", "~/.agents/knowledge/wms/conventions-wms-backend-logging.md"],
    mustDo: ["先按 impact.md 校验核心链路风险", "实现最小正确改动并同步维护 branch.md/config-changes.md/notes.md", "涉及入口、MQ、Job、外部调用、异常处理时补齐 tid 日志"],
    mustNotDo: ["只改代码不更新需求文件", "绕过现有项目规范或删除用户未授权改动", "引入无法追踪的硬编码配置"],
    doneCriteria: ["代码改动完成且关键路径可解释", "需求文件记录分支、配置、影响面和阶段性进展"],
  },
  自测中: {
    role: "自测验证者",
    mustRead: ["test.md", "impact.md", "config-changes.md", "~/.agents/knowledge/wms/conventions-wms-agent-self-test-evidence.md", "~/.agents/knowledge/wms/conventions-wms-backend-logging.md"],
    mustDo: ["记录触发方式和 tid", "用 tid 串起入口、关键分支、成功/失败日志", "验证 DB 或副作用并做反向检查", "在 test.md 写入 A/B/C/D 置信度"],
    mustNotDo: ["只用接口成功作为通过结论", "缺少 tid 时宣称链路验证通过", "忽略 ERROR/Exception/consumeFail/rollback 等反向证据"],
    doneCriteria: ["核心场景至少达到 B 级证据", "test.md 留下可复用验证链路和证据摘要"],
  },
  测试中: {
    role: "测试支持 / 缺陷排查者",
    mustRead: ["test.md", "impact.md", "notes.md", "config-changes.md", "~/.agents/knowledge/wms/conventions-wms-agent-self-test-evidence.md"],
    mustDo: ["围绕测试反馈复现并定位证据", "更新 test.md 的实际结果和缺陷证据", "把排查结论和待跟进项追加到 notes.md"],
    mustNotDo: ["把测试现象当根因", "未记录复现数据和日志关键字就结束排查"],
    doneCriteria: ["测试问题有复现、定位或明确阻塞项", "test.md/notes.md 可支撑后续回归"],
  },
  待上线: {
    role: "发布经理 / 风险审查者",
    mustRead: ["branch.md", "config-changes.md", "test.md", "impact.md", "review.md"],
    mustDo: ["检查分支合并、配置发布、测试证据、Review 结论和回滚方案", "把发布预检结论写入 release-check.md", "对阻塞项明确标注 OK/需关注/阻塞"],
    mustNotDo: ["缺少测试证据或配置确认时放行", "忽略 review.md 中未关闭的问题", "直接修改 state.json"],
    doneCriteria: ["release-check.md 覆盖分支、配置、测试、Review、回滚", "阻塞项清零或有用户确认的处理结论"],
  },
  已完成: {
    role: "复盘沉淀者",
    mustRead: ["memory.md", "notes.md", "test.md", "release-check.md"],
    mustDo: ["沉淀上线结果、复盘和可复用经验", "识别可进入 knowledge 的业务链路、接口、踩坑或规范", "保持 notes.md/memory.md 为后续 session 可读"],
    mustNotDo: ["继续当作开发任务推进", "把未验证猜测沉淀为事实"],
    doneCriteria: ["需求结论和经验已归档", "后续类似需求能从 memory.md/notes.md 复用上下文"],
  },
}

export interface Requirement {
  id: string
  title: string
  status: ReqStatus
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
   * Directory holding this requirement's files. Stored on the record so
   * the status-write API can locate `state.json` without re-deriving the
   * path from project/groupPath/id.
   */
  reqDir?: string
  /**
   * If this requirement is a child of another requirement (nested inside
   * its directory), the parent's req-id. Undefined for top-level or
   * parent requirements.
   */
  parentReqId?: string
  /**
   * If this requirement has child requirements (sub-directories with
   * their own meta.md), their req-ids. Undefined/empty for leaf
   * requirements.
   */
  childIds?: string[]
}

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

export const DEFAULT_REQ_ID = "__default__"
export const DEFAULT_PROJECT_NAME = "默认项目"

const DEFAULT_REQ_DIR = join(homedir(), ".agents", "req")
let _reqDir: string = DEFAULT_REQ_DIR

/**
 * Override the Hermes requirement scan root. Test-only — production code
 * relies on the default `~/.agents/req/` path. Mirrors `_setStorePath`.
 */
export function _setReqDir(path: string): void {
  _reqDir = path
}

export function _getReqDir(): string {
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
  parentProject: string,
  groupPath: string[] = [],
  parentReqId?: string,
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
  let project = parentProject
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
      const rawStatus = fields["status"]
      if (isReqStatus(rawStatus)) status = rawStatus
      if (fields["project"] && fields["project"].trim()) {
        project = fields["project"].trim()
      }
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

  // state.json wins over both frontmatter and the markdown-list status.
  // readRequirementState also migrates `- Status: <english>` from
  // meta.md the first time it runs.
  try {
    const state = await readRequirementState(dirPath)
    if (state) {
      status = state.status
      updatedAt = Math.max(updatedAt, state.updatedAt)
    }
  } catch {
    // ignore; fall back to whatever we already have.
  }

  return {
    id,
    title,
    status,
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
    reqDir: dirPath,
    parentReqId,
  }
}

/**
 * Recursively collect requirements (directories that contain meta.md)
 * under `rootPath`. Any directory without meta.md is treated as an
 * intermediate grouping directory and its segment name is appended to
 * `groupPath` for descendants.
 *
 * When a directory has meta.md, it is recorded as a requirement AND the
 * scan continues into its sub-directories to discover child requirements.
 * This supports the parent-child pattern where a top-level requirement
 * acts as a grouping container (e.g. WMS-003-rabbitmq-to-rocketmq/
 *   WMS-003-stock-diff-adjust/meta.md). Child requirements carry
 * `parentReqId` pointing back to the parent.
 *
 * Bounded recursion: max depth 6 to keep accidental symlink loops or
 * deeply nested test fixtures from spinning.
 */
async function collectRequirementsRecursive(
  rootPath: string,
  project: string,
  groupPath: string[],
  out: Requirement[],
  depth = 0,
  parentReqId?: string,
  skipSelfMeta = false,
  parentReqRef?: Requirement,
): Promise<void> {
  if (depth > 6) return
  let st
  try {
    st = await stat(rootPath)
  } catch {
    return
  }
  if (!st.isDirectory()) return

  let currentParent = parentReqId
  let currentGroupPath = groupPath
  let parentReq: Requirement | null = null

  // If skipSelfMeta is true, the caller already loaded this requirement
  // and passed it as parentReqRef. Use it directly so childIds can be
  // tracked on the already-pushed record.
  if (skipSelfMeta && parentReqRef) {
    parentReq = parentReqRef
    currentParent = parentReqRef.id
  }

  // If this directory itself has a meta.md, it IS a requirement.
  // Record it, then continue scanning sub-directories for children.
  // skipSelfMeta is used when we recurse into a child requirement's
  // directory to find grand-children — the child was already loaded by
  // the caller, so we must not load it again.
  if (!skipSelfMeta && existsSync(join(rootPath, "meta.md"))) {
    const dirName = rootPath.split("/").filter(Boolean).pop() || rootPath
    parentReq = await loadRequirementFromDir(rootPath, dirName, project, groupPath, parentReqId)
    if (parentReq) {
      out.push(parentReq)
      currentParent = parentReq.id
      currentGroupPath = groupPath
    }
  }

  let children: string[]
  try {
    children = await readdir(rootPath)
  } catch {
    return
  }
  for (const childName of children) {
    if (childName.startsWith(".") || childName === "README.md") continue
    // Skip non-directory files (meta.md, branch.md, notes.md, etc.)
    const childPath = join(rootPath, childName)
    let childSt
    try {
      childSt = await stat(childPath)
    } catch {
      continue
    }
    if (!childSt.isDirectory()) continue

    // If child has meta.md, load it as a child requirement. If not,
    // recurse as an intermediate grouping directory.
    if (existsSync(join(childPath, "meta.md"))) {
      const req = await loadRequirementFromDir(childPath, childName, project, currentGroupPath, currentParent)
      if (req) {
        out.push(req)
        if (parentReq) {
          if (!parentReq.childIds) parentReq.childIds = []
          parentReq.childIds.push(req.id)
        }
        // Continue scanning into the child for grand-children.
        // skipSelfMeta=true so the child is not loaded a second time;
        // pass req as parentReqRef so grand-children can be tracked.
        await collectRequirementsRecursive(childPath, project, currentGroupPath, out, depth + 1, req.id, true, req)
      }
    } else {
      await collectRequirementsRecursive(
        childPath,
        project,
        [...currentGroupPath, childName],
        out,
        depth + 1,
        currentParent,
      )
    }
  }
}

export async function scanHermesRequirements(): Promise<Requirement[]> {
  const reqDir = _reqDir
  if (!existsSync(reqDir)) return []
  let topEntries: string[]
  try {
    topEntries = await readdir(reqDir)
  } catch {
    return []
  }
  const out: Requirement[] = []
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

    // Resolve this directory's display project name. `_default` maps to
    // the synthetic default project name.
    const projectDisplay =
      name === "_default" ? DEFAULT_PROJECT_NAME : name

    const hasOwnMeta = existsSync(join(topPath, "meta.md"))

    if (hasOwnMeta) {
      // Legacy flat layout: ~/.agents/req/<req-id>/meta.md
      // project comes from frontmatter or defaults to DEFAULT_PROJECT_NAME.
      // Use collectRequirementsRecursive so children are discovered too.
      await collectRequirementsRecursive(topPath, DEFAULT_PROJECT_NAME, [], out)
      continue
    }

    // Project-level directory. Walk it recursively, accumulating the
    // intermediate grouping path under `groupPath` for each leaf.
    await collectRequirementsRecursive(topPath, projectDisplay, [], out)
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
    groupPath: [],
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
  *   8. the phase profile for the requirement's current status
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

    const profile = REQUIREMENT_PHASE_PROFILES[req.status]
    lines.push("")
    lines.push("阶段执行规范：")
    lines.push(`  - 当前身份：${profile.role}`)
    lines.push(`  - 必读：${profile.mustRead.join("、")}`)
    lines.push(`  - 必做：${profile.mustDo.join("；")}`)
    lines.push(`  - 禁止：${profile.mustNotDo.join("；")}`)
    lines.push(`  - 完成标准：${profile.doneCriteria.join("；")}`)

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
