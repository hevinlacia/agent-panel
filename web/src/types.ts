/**
 * Role: browser-side DTOs for the React dashboard island.
 * Public surface: API payload and shared UI DTO types used across React pages/features.
 * Constraints: mirrors /api/* JSON without importing Rust backend internals into Vite.
 * Read-this-with: src/main.rs and web/src/App.tsx.
 */

export type ReqStatus = "需求澄清" | "开发中" | "自测中" | "测试中" | "人工核查" | "发布就绪" | "经验总结" | "已完成" | "排查中" | "已定位" | "已修复" | "已复盘" | "已关闭" | "需求创建" | "已合入" | "已发布" | "已取消" | "挂起"
export type ReqCategory = "需求" | "线上问题" | "测试问题"

export interface EffortEstimate {
  coefficient: number
  baseHours: number
  estimatedHours: number
  summary?: string
  updatedAt?: number
}

/** 需求工时档案（ones-manhour.json，随需求索引返回）。 */
export interface OnesManhourFileValue {
  manualHours?: number
  agentHours?: number
  agentHoursNote?: string
  logHistory?: OnesManhourLogEntry[]
  updatedAt?: number
}

/** 一条通过面板发起的工时登记记录。 */
export interface OnesManhourLogEntry {
  date: string
  hours: number
  remark?: string
  status: "prepared" | "logged" | "failed" | string
  createdAt: number
  taskUrl?: string
  message?: string
}

/** GET /api/apitest/apis 单条接口目录项。 */
export interface ApitestApiItem {
  id: string
  domain: string
  purpose: string
  method: string
  url: string
  status: string
  kind: "create" | "test" | string
  tags: string[]
  envSupport: Record<string, string>
  notes?: string
  hasRequestSchema: boolean
  placeholderCount: number
  templateOk: boolean
}

export interface ApitestApisPayload {
  ok: boolean
  count: number
  apis: ApitestApiItem[]
  packRoot: string
}

/** GET /api/apitest/api 响应：模板 + 占位符 + schema + tests。 */
export interface ApitestDetailPayload {
  ok: boolean
  api: ApitestApiItem
  template: {
    headers: [string, string][]
    body: string | null
    varsMap: Record<string, string>
  }
  tests: string[]
  placeholders: { name: string; envVar: string; derived: boolean }[]
  requestSchema?: string | null
  requestSchemaPath?: string | null
  responseSchemaPath?: string | null
}

/** POST /api/apitest/trigger 响应。 */
export interface ApitestTriggerPayload {
  ok: boolean
  dryRun?: boolean
  status?: number
  durationMs?: number
  contentType?: string | null
  url?: string
  method?: string
  headers?: [string, string][]
  body?: string | null
  bodyText?: string
  bodyJson?: unknown
  truncated?: boolean
  env?: string
  apiId?: string
  kind?: string
  tests?: string[]
  missing?: string[]
  message?: string
  note?: string
}

/** GET /api/ones/manhour 响应：需求工时四联汇总（人工/自动/ONES 已登记/agent 实际）。 */
export interface OnesManhourInfo {
  reqId: string
  title: string
  status: string
  ones?: { raw: string; url: string | null; label: string } | null
  displayId?: string | null
  taskUuid?: string | null
  taskName?: string | null
  taskUrl?: string | null
  manualHours?: number | null
  autoHours?: number | null
  autoHoursUpdatedAt?: number | null
  loggedHours?: number | null
  remainingHours?: number | null
  agentHours?: number | null
  agentHoursNote?: string | null
  logHistory?: OnesManhourLogEntry[]
  manhourWindowDays?: number
  onesFetchedAt?: number
  cacheHit?: boolean
  warnings?: string[]
}

/** POST /api/ones/manhour/log 单条结果。 */
export interface OnesManhourLogResult {
  date: string
  hours: number
  remark?: string
  status: "prepared" | "logged" | "failed" | string
  taskUrl?: string
  message?: string
}

export interface OnesManhourLogResponse {
  ok: boolean
  mode: "prepared" | "execute" | string
  reqId: string
  displayId?: string
  results: OnesManhourLogResult[]
  warnings?: string[]
  note?: string
}

export interface ExperienceSummaryJob {
  version?: number
  reqId?: string
  status?: "pending" | "running" | "completed" | "failed" | "skipped" | string
  sessionId?: string | null
  model?: string | null
  startedAt?: number | null
  finishedAt?: number | null
  attempts?: number
  error?: string | null
  reportPath?: string | null
  updatedAt?: number
}

export interface ExperienceSummaryJobsPayload {
  ok: boolean
  generatedAt: number
  config: { enabled: boolean; model?: string; maxAgents: number }
  stats: { total: number; available: number; running: number; completed: number; failed: number; skipped: number }
  items: { req: Requirement; stage: string }[]
}

export interface ExperienceSummaryDispatchPayload { ok: boolean; report: { enabled: boolean; maxAgents: number; active: number; queued: number; completed: number; failed: number; dispatched: unknown[]; skipped: unknown[] } }
export interface ExperienceSummaryReportPayload { ok: boolean; reqId: string; path: string; exists: boolean; content: string; job?: ExperienceSummaryJob | null }

export interface GroupMemberRef {
  reqId: string
  note?: string
  /** 扫描回填：成员需求标题。 */
  title?: string
  /** 扫描回填：成员当前状态。 */
  status?: string
  /** 成员需求是否在索引中找到（false = 失效引用）。 */
  found: boolean
  /** 成员自身也是需求组（不支持嵌套）。 */
  nested?: boolean
}

/** 需求文档分册（docs/<doc-base>/<NNN>-<slug>.md）。 */
export interface DocPartInfo {
  docBase: string
  filename: string
  /** 相对需求目录路径：docs/notes/001-slug.md */
  relPath: string
  title?: string
  bytes: number
  updatedAt: number
  /** 主文档索引段是否包含该分册链接（false = 未索引/失效）。 */
  indexLinked?: boolean
}

/** GET /api/requirement/doc-parts 响应。 */
export interface DocPartsPayload {
  ok: boolean
  reqId: string
  docType?: string
  partsDir: string
  splitWarnBytes: number
  count: number
  parts: DocPartInfo[]
  mainDoc?: { file: string; bytes: number; overSplitThreshold: boolean }
}

/** 子需求引用（父需求视角）；title/status/found 由扫描回填。 */
export interface SubReqRef {
  reqId: string
  title?: string
  status?: string
  found: boolean
  merged?: boolean
  cancelled?: boolean
}

/** 子需求分支操作（init-branches / sync-parent / merge-to-parent）的单仓结果。 */
export interface SubRepoOpResult {
  repoName: string
  role?: string
  sourceBranch?: string
  targetBranch?: string
  parentBranch?: string
  subBranch?: string
  branchStatus?: string
  worktreeStatus?: string
  worktreePath?: string
  status: string
  message?: string
}

/** 子需求分支操作的聚合响应。 */
export interface SubBranchOpResult {
  ok: boolean
  reqId: string
  parentReqId?: string
  repos?: SubRepoOpResult[]
  statusState?: unknown
}

/** 整合需求发布分支登记（release-branches.json 的单条分支 + 运行时状态）。 */
export interface ReleaseBranchRepo {
  repoName: string
  role?: string
  baseBranch?: string
  baseCommit?: string
  diffFiles?: number
  diffAdditions?: number
  diffDeletions?: number
}

export interface ReleaseMergedSub {
  reqId: string
  title?: string
  status?: string
  found?: boolean
  mergedAt?: number
  repos?: string[]
  note?: string
}

export interface ReleaseBranchEntry {
  id: string
  name: string
  createdAt?: number
  status: "active" | "released" | string
  releasedAt?: number | null
  note?: string | null
  repos?: ReleaseBranchRepo[]
  mergedSubs?: ReleaseMergedSub[]
}

export interface ReleaseBranchListResult {
  ok: boolean
  reqId: string
  branches?: ReleaseBranchEntry[]
}

/** 发布分支操作（create / merge-sub / sync-prod / prod-mr / mark-released）聚合响应。 */
export interface ReleaseBranchOpResult {
  ok: boolean
  reqId: string
  branch?: string | null
  branchId?: string | null
  repos?: SubRepoOpResult[]
  skipped?: SubRepoOpResult[]
  results?: unknown[]
  advancedSubs?: string[]
  message?: string
  statusState?: unknown
}

export interface Requirement {
  id: string
  title: string
  description?: string
  status: ReqStatus
  category?: ReqCategory
  /** 需求推动方：产品推动（默认）/ 开发推动。 */
  source?: string
  project: string
  projects?: string[]
  groupPath?: string[]
  createdAt: number
  updatedAt: number
  completedAt?: number
  /** 打回返工轮次：人工核查/测试中 → 开发中 的回退次数（rework loop）。 */
  reworkRounds?: number
  /** 发布就绪小循环轮次：发布就绪 → 开发中 的上线前快速修复次数（fast-fix loop）。 */
  releaseReadyRounds?: number
  sessionIds: string[]
  reqDir?: string
  metaPath?: string
  alignmentPath?: string
  backgroundPath?: string
  memoryPath?: string
  branchPath?: string
  testPath?: string
  notesPath?: string
  configPath?: string
  impactPath?: string
  reviewPath?: string
  technicalPlanPath?: string
  releaseManifestPath?: string
  releaseCheckPath?: string
  experienceSummaryPath?: string
  troubleshootingPath?: string
  incidentPath?: string
  rootCausePath?: string
  experienceSummaryJob?: ExperienceSummaryJob
  prdPath?: string
  ones?: string
  /** 需求工时档案（ones-manhour.json）：人工预估 / agent 实际 / 录入历史。 */
  onesManhour?: OnesManhourFileValue
  /** 绑定的线上问题 req id 列表（仅普通需求，meta.md issues 字段）。 */
  issues?: string[]
  /** 测试场景文档路径（开发推动的需求必须维护）。 */
  testScenarioPath?: string
  planRelease?: string
  effortEstimate?: EffortEstimate
  /** 引用式需求组：本需求 group.json 的成员列表（非空 = 本需求是组）。 */
  groupMembers?: GroupMemberRef[]
  /** 组发布策略：independent（默认）/ together（整体发布）。 */
  groupPolicy?: "together" | "independent"
  /** 本需求作为成员所属的需求组 req id 列表。 */
  memberOf?: string[]
  /** 组聚合状态 = min(成员需求流状态)；非组时缺省。 */
  groupStatus?: string
  /** 组瓶颈成员（聚合状态来源，最慢成员 req id）。 */
  groupBottleneck?: string
  /** 子需求：meta.md frontmatter parent-req-id；存在即子需求。 */
  parentReqId?: string
  /** 派生字段：是否子需求。 */
  isSubReq?: boolean
  /** 实体类型标记：meta.md req-kind（如 rollup = 整合需求）。 */
  reqKind?: string
  /** 本需求作为父需求时拆出的子需求列表（扫描回填）。 */
  subReqs?: SubReqRef[]
}

export interface RequirementAttachment {
  filename: string
  path: string
  relativePath: string
  extension: string
  size: number
  mtime: number
  summary: string[]
  sample: string
}

export interface RequirementAttachmentsPayload {
  attachments: RequirementAttachment[]
}

export interface RequirementSummary {
  id: string
  title: string
  status: string
  project: string
  projects?: string[]
  groupPath?: string[]
  createdAt: number
  updatedAt: number
}

export interface StatusCount {
  status: string
  count: number
  percent: number
}

export interface RequirementDuration {
  req: RequirementSummary
  durationMs: number
}

export interface ReleaseDayCount {
  date: string
  count: number
}

export interface DashboardStats {
  total: number
  statusCounts: StatusCount[]
  durations: RequirementDuration[]
  avgDeliveryMs: number
  medianDeliveryMs: number
  maxDeliveryMs: number
  completedCount: number
  inProgressCount: number
  releaseSchedule: ReleaseDayCount[]
  nextRelease: ReleaseDayCount | null
}

export interface DashboardStatsPayload {
  generatedAt: number
  stats: DashboardStats
}

export interface SessionInfo {
  id: string
  title: string
  status: "running" | "idle" | "stale" | string
  agent?: string
  model?: string
  provider?: string
  modelId?: string
  modelProvider?: string
  directory?: string
  worktree?: string
  path?: string
  updated?: number
  created?: number
  messageCount?: number
  userMessageCount?: number
  assistantMessageCount?: number
  toolCallCount?: number
  tokensInput?: number
  tokensOutput?: number
  cost?: number
}

export interface RequirementDocPayload { ok: boolean; reqId: string; docType: string; file: string; path: string; exists: boolean; content: string; template?: string }
export interface SessionLogTool { kind?: string; name?: string; id?: string }
export interface SessionLogEntry { line: number; type: string; timestamp?: number | null; title?: string; text?: string; tools?: SessionLogTool[]; usage?: Record<string, unknown> | null; rawType?: string }
export interface SessionLogPayload { ok: boolean; sessionId: string; path: string; cursor: number; total: number; hasMore: boolean; updatedAt: number; entries: SessionLogEntry[] }
export interface ApiSessions { summary: Record<string, number>; sessions: SessionInfo[]; harness?: string; days?: number }

/** 发版冻结：部署脚本 ylops_deploy.py 直读 config.json 的 deployFreeze 字段拦 UAT 构建/部署。 */
export interface DeployFreeze {
  enabled?: boolean
  reason?: string
  /** 最近一次开启时间（RFC3339）；关闭时保留供展示。 */
  since?: string
}

export interface StatusGateRule {
  from: ReqStatus | string
  to: ReqStatus | string
  gates: string[]
}

export interface StatusGateDef {
  id: string
  label: string
  description: string
}

/** 门禁三态：passed 校验且通过 / failed 未通过（会拦 agent）/ unverified 未校验（人工或系统跳过） */
export type StatusFlowGateState = "passed" | "failed" | "unverified"

export interface StatusFlowGate {
  id: string
  label: string
  state: StatusFlowGateState
  reason: string
  /** 放行但需用户注意的警示（如 P1 严重问题、无法测试项），前端渲染 ⚠。 */
  warnings?: string[]
}

export interface StatusFlowTransition {
  from: string
  to: string
  gates: StatusFlowGate[]
}

/** 返工回路（rework loop）：从人工核查/测试中打回开发中重新迭代。 */
export interface StatusFlowRework {
  to: string
  fromStatuses: string[]
  /** 打回返工轮次（向后兼容字段，等于 loops.rework.rounds）。 */
  rounds: number
  /** 发布就绪小循环轮次：发布就绪 → 开发中（上线前快速修复）。 */
  releaseReadyRounds?: number
  /** 分循环轮次明细。 */
  loops?: {
    rework?: { fromStatuses: string[]; rounds: number }
    releaseReady?: { fromStatuses: string[]; rounds: number }
  }
}

export interface StatusFlowPayload {
  ok: boolean
  reqId: string
  category?: string | null
  currentStatus: string
  currentKnown: boolean
  /** 挂起是流水外标记状态：suspended=true 时 currentIndex 为 null，resumeStatus 为挂起前状态。 */
  suspended?: boolean
  resumeStatus?: string | null
  statuses: string[]
  currentIndex: number | null
  transitions: StatusFlowTransition[]
  rework?: StatusFlowRework | null
}

/** 门禁验证详情页：实时校验状态 + 按门禁类型的详细内容（字段按 gate 类型可选） */
export interface StatusGateDetail {
  status?: string
  label?: string
  allowsTesting?: boolean
  reason?: string
  source?: string | null
  reviewPath?: string
  aiReviewPath?: string
  riskTags?: string[]
  inventoryRisk?: boolean
  staleRepos?: ReviewGateStaleRepo[]
  incrementalReview?: CodeReviewSnapshot | null
  checklist?: {
    present?: boolean
    total?: number
    concluded?: number
    failed?: number
    error?: string
    failedItems?: { id?: string; title?: string }[]
    items?: { id?: string; title?: string; conclusion?: string; note?: string; evidence?: string }[]
  } | null
  annotations?: { present?: boolean; stale?: boolean; reason?: string } | null
  /** 按循环语境整理的历史审查结论记录（主流程 / 返工轮次 / 发布就绪小循环）。 */
  roundRecords?: ReviewRoundRecord[]
  /** diff 快照栈里出现过的轮次号（供按轮次回看差异材料）。 */
  snapshotRounds?: number[]
  /** 放行但需用户注意的警示（如 P1 严重问题、无法测试项）。 */
  warnings?: string[] | null
  actions?: string[]
  problems?: string[]
  hotfix?: boolean
  applicable?: boolean
  filled?: boolean
  rootCauseFilled?: boolean
  legacyPlanFilled?: boolean
  category?: string | null
  /** 自测清单门禁：test.md 结构化自测清单（每项含结果与原因）。 */
  selftestChecklist?: SelftestChecklistPayload | null
}

/** 循环语境审查结论记录：主流程结论 + 各轮次追加结论（review.md / code-review-ai.md）。 */
export interface ReviewRoundRecord {
  kind: "main" | "rework" | "release-ready" | "other"
  round?: number | null
  heading?: string | null
  source: string
  conclusion?: "PASS" | "BLOCKED" | "WAIVED" | null
  docUpdatedAt?: number
}

export interface SelftestChecklistItem {
  no: number
  item: string
  result: "pass" | "fail" | "cannot" | "missing"
  resultText: string
  reason?: string | null
}

export interface SelftestChecklistSection {
  category: string
  notApplicableReason?: string | null
  riskAnalysis: string[]
  items: SelftestChecklistItem[]
}

export interface SelftestChecklistPayload {
  found: boolean
  sections: SelftestChecklistSection[]
}

export interface StatusGateDetailPayload {
  ok: boolean
  reqId: string
  gate: string
  label: string
  description: string
  state: "passed" | "failed" | "unverified"
  reason: string
  detail: StatusGateDetail | null
  checkedAt: number
}

export interface ConfigPayload {
  harness?: string
  dshProfile?: string
  requirementScanRoots?: string[]
  fullSyncSchedule?: boolean
  fullSyncTimes?: string[]
  fullSyncGithubRepos?: string[]
  codeReviewPiModel?: string
  branchScopePiModel?: string
  effortEstimatePiModel?: string
  effortEstimateBaseHours?: number
  autoExperienceSummary?: boolean
  experienceSummaryPiModel?: string
  experienceSummaryMaxAgents?: number
  cainiaoMockEnabled?: boolean
  cainiaoMockPort?: number
  /** 发版冻结：开启后 ylops_deploy.py 拒绝在 UAT（uat-sg/uat-cn）触发构建/部署。 */
  deployFreeze?: DeployFreeze
  mergeExcludedRepos?: string[]
  /** 未配置（undefined）时后端使用内置默认门禁；显式空数组 = 关闭所有门禁。 */
  statusGates?: StatusGateRule[] | null
  availableStatusGates?: StatusGateDef[]
  effectiveStatusGates?: StatusGateRule[]
  /** skill 位置映射：skill 名 → skill 目录或 SKILL.md 路径（~/ 展开）。未配置走默认解析。 */
  skillPathOverrides?: Record<string, string>
}

/** 单个 panel 依赖 skill 的解析与生效状态（GET /api/config/skills）。 */
export interface SkillDependStatus {
  name: string
  label: string
  usedBy: string
  source: "override" | "default"
  overridePath?: string | null
  path: string
  exists: boolean
  bytes: number
}

export interface SkillDependsPayload {
  ok: boolean
  allOk: boolean
  skills: SkillDependStatus[]
}

export interface HarnessCurrent {
  harness: "pi" | "dsh-web" | "dsh-tui"
  label: string
}

export interface SessionCandidatesPayload {
  harness: "pi" | "dsh-web" | "dsh-tui"
  projectRoot?: string | null
  candidates: SessionInfo[]
}

export interface NewSessionPayload {
  ok: boolean
  harness?: "pi" | "dsh-web" | "dsh-tui"
  command?: string
  sessionId?: string
  profile?: string
  cwd?: string | null
  contextPath?: string | null
  /** pi/dsh-tui：true 表示复用了未使用过的 pending session id，未新建。 */
  reused?: boolean
  /** dsh: base URL of the running web GUI where the created session appears. */
  url?: string
  /** dsh: raw `sessions.prompt` command response (the /requirement-bind dispatch). */
  bind?: unknown
}

/** 需求当前的待使用终端命令（未被用过的 session id 对应的启动命令）。 */
export interface PendingSessionCommand {
  sessionId: string
  command: string
  contextPath: string
  harness: "pi" | "dsh-tui" | string
  createdAt: number
  /** session 文件已存在（命令已被使用过）；下次复制命令会自动换新 id。 */
  used: boolean
}

export interface PendingSessionPayload {
  ok: boolean
  harness?: "pi" | "dsh-web" | "dsh-tui"
  pending: PendingSessionCommand | null
}

export interface CainiaoMockStatus { enabled: boolean; running: boolean; port: number }

export type KnowledgeKind = "businessKnowledge" | "experience"
export interface KnowledgeItem {
  id: string
  title: string
  kind: KnowledgeKind | string
  type?: string
  domain?: string
  project?: string
  scope?: string
  status?: string
  confidence?: string
  tags?: string[]
  triggerTerms?: string[]
  relatedSkills?: string[]
  relatedRepos?: string[]
  relatedTables?: string[]
  relatedApis?: string[]
  source?: string
  createdAt?: string
  updatedAt?: string
  lastVerifiedAt?: string
  validUntil?: string
  summary?: string
  details?: string | null
  detailsTruncated?: boolean
  path?: string
  score?: number
  whyMatched?: string[]
}
export interface KnowledgeListPayload { items: KnowledgeItem[]; generatedAt?: number }
export interface KnowledgeSavePayload { ok: boolean; item: KnowledgeItem }
export type KnowledgeDraft = Partial<KnowledgeItem> & { details?: string; root?: string }

export interface PiConfigFileSnapshot { file: string; label: string; path: string; sensitive: boolean; description: string; content: string; updatedAt: number | null }
export interface PiModelOption { providerId: string; modelId: string; label: string; name?: string; contextWindow?: number | null; reasoning?: boolean; thinkingLevels: string[] }
export interface PiProviderSummary { id: string; api?: string; baseUrl?: string; modelCount: number; hasApiKey: boolean; models: PiModelOption[] }
export interface PiConfigSummary { settings: { path: string; exists: boolean; defaultProvider: string; defaultModel: string; defaultThinkingLevel: string; enabledModels: string[]; theme: string }; providers: PiProviderSummary[]; thinkingLevels: string[] }

export interface AutoDrivePayload { jobs: unknown[]; active: number; blocked: number; queue: { active: number; queued: number }; message?: string }
export interface BranchRepo { repoName: string; branches: string[]; role?: string; path?: string; baseRef?: string; testTargetBranch?: string; uatTargetBranch?: string }
export interface BranchScope { version: number; updatedAt: number; repos: BranchRepo[]; fallback?: boolean }

export interface CodeReviewFile { path: string; status: string; additions: number; deletions: number; riskTags?: string[] }
export interface CodeReviewRepoSnapshot {
  repoName: string
  projectPath?: string
  branch: string
  resolvedTargetRef?: string
  targetCommit?: string | null
  baseRef: string
  baseCommit?: string | null
  coverageFromCommit?: string | null
  coverageToCommit?: string | null
  linearHistory?: boolean
  currentBranch?: string
  dirty?: boolean
  commits?: string[]
  files: CodeReviewFile[]
  additions: number
  deletions: number
  diff?: string
  diffTruncated?: boolean
  warnings?: string[]
  error?: string | null
}
export interface CodeReviewSnapshot { version: number; reqId: string; updatedAt: number; baseRef: string; frontendBaseRef?: string; backendBaseRef?: string; sourceFallback?: boolean; mode?: string; sourceSnapshot?: string; baseDescription?: string; targetDescription?: string; repos: CodeReviewRepoSnapshot[] }
export interface CodeReviewPayload { ok: boolean; branchScope?: BranchScope | null; review?: CodeReviewSnapshot | null; incrementalReview?: CodeReviewSnapshot | null }
export interface ReviewGateStaleRepo { repoName: string; branch: string; projectPath?: string | null; reviewedTargetRef?: string; reviewedTargetCommit?: string; currentTargetRef?: string; currentTargetCommit?: string }
export interface ReviewGatePayload { ok: boolean; reqId: string; gate: { status: string; label: string; allowsTesting: boolean; reason: string; source?: string | null; reviewPath: string; aiReviewPath: string; riskTags?: string[]; inventoryRisk?: boolean; staleRepos?: ReviewGateStaleRepo[]; incrementalReview?: CodeReviewSnapshot | null; checkedAt: number; actions: string[]; warnings?: string[]; annotations?: { present?: boolean; stale?: boolean; reason?: string } | null } }
export interface CodeDiffSnapshot extends CodeReviewSnapshot { savedAt?: number; round?: number }
export interface DiffSnapshotsPayload { ok: boolean; round?: number; snapshots: CodeDiffSnapshot[] }
export interface MasterDiffPayload { ok: boolean; round?: number; branchScope?: BranchScope | null; prunedRepos?: string[]; prunedWritten?: boolean; snapshots: CodeDiffSnapshot[] }
/** 分支登记轮次：1 = 原始 branches.json；>=2 = 合入生产后的修复轮次文件 branches-round-<n>.json */
export interface BranchRoundInfo { round: number; file: string; updatedAt: number; repoCount: number; branchCount: number; sealed: boolean }
export interface BranchRoundsPayload { ok: boolean; reqId: string; rounds: BranchRoundInfo[]; latest: number }
export interface BranchRegistrationPayload { ok: boolean; reqId: string; round: number; file: string; scope?: BranchScope | null }

export interface ReviewMaterialsRepo { repoName: string; branch: string; fromCommit: string; toCommit: string; additions?: number; deletions?: number; riskTags?: string[]; diffTruncated?: boolean; linearHistory?: boolean | null }
export interface ReviewMaterials {
  mode: "full-initial" | "full-ready" | "full-regenerate" | "incremental" | "incremental-pending" | string
  reason: string
  materialKind: "full" | "incremental" | string
  materialFile: string
  materialPath: string
  riskTags?: string[]
  inventoryRisk?: boolean
  repos: ReviewMaterialsRepo[]
  warnings: string[]
  handoffHints: string[]
  checkedAt: number
}
export interface ReviewMaterialsPayload { ok: boolean; reqId: string; materials: ReviewMaterials }

export interface AnnotationVariable { name: string; kind?: string; meaning?: string; why?: string }
export interface AnnotationAnchor { type?: "hunk" | "file"; hunkHeader?: string; context?: string[]; newStart?: number }
export interface AnnotationNote { anchor?: AnnotationAnchor; title?: string; note: string }
export interface CodeFileAnnotation {
  repo: string
  path: string
  summary?: string
  variables?: AnnotationVariable[]
  flow?: string
  flowType?: string
  flowTitle?: string
  notes?: AnnotationNote[]
}
export interface CodeAnnotations {
  version?: number
  reqId?: string
  generatedAt?: number
  generatedBy?: string
  baseRef?: string
  reviewedCommit?: Record<string, string>
  files?: CodeFileAnnotation[]
}
export interface AnnotationsPayload { ok: boolean; annotations: CodeAnnotations | null }
export interface SyncBaseResult { repoName: string; ok: boolean; status: string; baseRef?: string; remoteRef?: string; localBranch?: string; currentBranch?: string; beforeCommit?: string; afterCommit?: string; message: string; warnings?: string[] }
export interface SyncBasePayload { ok: boolean; generatedAt: number; results: SyncBaseResult[] }
export interface ProdMrResult { repoName: string; role?: string | null; projectPath?: string | null; sourceBranch: string; targetBranch: string; status: "created" | "reused" | "failed" | "skipped" | "no_diff" | string; iid?: number | null; webUrl?: string | null; title?: string | null; error?: string | null; diffFiles?: number | null; diffAdditions?: number | null; diffDeletions?: number | null }
export interface ProdMrPayload { ok: boolean; reqId: string; generatedAt: number; branchScope?: BranchScope | null; results: ProdMrResult[] }
export type MergeTarget = "test" | "uat"
export type MergeRepoKind = "frontend" | "backend"
export interface MergeOption { value: string; label: string; target: MergeTarget | string }
export interface MergeKindOptions { repoKind: MergeRepoKind | string; options: MergeOption[]; defaultValue?: string | null }
export interface MergeOptionsPayload { ok: boolean; reqId: string; status: ReqStatus | string; generatedAt: number; branchScope?: BranchScope | null; options: { frontend: MergeKindOptions; backend: MergeKindOptions } }
export interface MergeBranchResult { repoName: string; role?: string | null; projectPath?: string | null; sourceBranch: string; target: MergeTarget | string; targetBranch?: string | null; status: "merged" | "upToDate" | "conflict" | "failed" | "skipped" | "idle" | "pending" | string; message?: string | null; conflictFiles?: string[]; worktreePath?: string | null; warnings?: string[]; commands?: string[] }
export interface MergeBranchPayload { ok: boolean; reqId: string; target?: MergeTarget | string; targetBranch?: string | null; repoKind?: MergeRepoKind | string | null; status: string; generatedAt: number; branchScope?: BranchScope | null; results: MergeBranchResult[] }

export interface TestdataTarget {
  name: string
  status_code: number | null
  label: string
  verified: boolean
}

export interface TestdataCliField {
  required?: boolean
  choices?: string[]
  default?: string | number
  /** 参数说明（capabilities.yaml description，脚本 argparse help 同步）。 */
  description?: string
}

export interface TestdataCapabilityItem {
  id: string
  domain: string
  object: string
  execution: string
  purpose: string
  script: string
  /** 验证环境与日期（capabilities.yaml 透传，snake_case）。 */
  verified_env?: string
  verified_date?: string
  /** 脚本 stdout 是否为结构化 JSON。 */
  stdout_json?: boolean
  /** 完整调用示例。 */
  invocation?: string
  cli?: Record<string, TestdataCliField>
  targets?: TestdataTarget[]
  /** 关联知识资产（相对 pack 根路径）。 */
  recipe?: string
  state_graph?: string
  pitfalls?: string[]
  notes?: unknown[]
}

export interface TestdataCapabilitiesPayload {
  ok: boolean
  project: string
  capabilities: TestdataCapabilityItem[]
}

export interface TestdataCapabilityPayload {
  ok: boolean
  capability: TestdataCapabilityItem
}

export interface TestdataRunPayload {
  ok: boolean
  dryRun: boolean
  executed: boolean
  project?: string
  capabilityId?: string
  target?: string
  command: string
  cwd: string
  stdout?: string
  stderr?: string
  exitCode?: number | null
  safety?: { env?: string; note?: string }
}

// --- Browser Auth (Chrome 登录态复用) ---
export interface BrowserAuthLoginCheck {
  method?: string
  path?: string
  expect?: number
}

export interface BrowserAuthSite {
  id: string
  label?: string
  enabled?: boolean
  baseUrl?: string
  cookieDomains?: string[]
  allowedHosts?: string[]
  allowedPathPrefixes?: string[]
  defaultHeaders?: Record<string, string>
  loginCheck?: BrowserAuthLoginCheck | null
  status?: {
    ok?: boolean
    cookieCount?: number
    matchedDomains?: string[]
    hasCookieValue?: boolean
  }
}

export interface BrowserAuthSitesPayload {
  generatedAt: number
  config: { cdpUrl?: string; sites: BrowserAuthSite[] }
  cdp: { connected: boolean; source?: string; message: string }
  sites: (BrowserAuthSite & { status: BrowserAuthSite["status"] } & {
    label: string
    enabled: boolean
    baseUrl: string
    allowedHosts: string[]
    allowedPathPrefixes: string[]
    cookieDomains: string[]
    loginCheck: BrowserAuthLoginCheck | null
  })[]
  security: {
    returnsSecrets: boolean
    tokenPersistence: string
    auditFile: string
    allowlistEnforced?: boolean
    cookieAllowlist?: string[]
    effectiveAllowlist?: string[]
    heldCookieCount?: number
  }
}

export interface BrowserAuthCheckPayload {
  ok: boolean
  generatedAt: number
  site: string
  status: { ok?: boolean; cookieCount?: number; matchedDomains?: string[]; hasCookieValue?: boolean }
  login: {
    ok?: boolean
    status?: number
    expected?: number
    contentType?: string | null
    bodyPreview?: string
    error?: string
    skipped?: boolean
    reason?: string
  }
}

export interface BrowserAuthRequestPayload {
  method?: string
  path: string
  headers?: Record<string, string>
  json?: unknown
  body?: string
}

export interface BrowserAuthRequestResult {
  ok: boolean
  status: number
  url?: string
  contentType?: string | null
  headers?: Record<string, string>
  bodyText?: string
  bodyJson?: unknown
  truncated?: boolean
  secretsReturned?: boolean
}

/** ONES 候选任务：notices + 工时报表聚合去重后的一条工作项（GET /api/ones/tasks）。 */
export interface OnesTaskCandidate {
  displayId: string
  name: string
  project: string
  taskUuid: string
  sources: string[]
  lastActivityAt: number
  /** 工时报表窗口内已登记工时（小时，原始值÷100000）；无登记为 0。 */
  actualHours?: number
  actualHoursRaw?: number
  url: string
  refText: string
}

/** ONES 推荐项：按需求标题匹配度排序，refText 可直接写入 ones 字段。 */
export interface OnesRecommendation {
  displayId: string
  name: string
  project: string
  url: string
  refText: string
  sources: string[]
  score: number
  displayIdBoost: boolean
}

/** GET /api/ones/tasks 响应：reqId 缺省时 recommendations 为空数组；默认走服务端缓存，refresh=true 回源。 */
export interface OnesTasksResponse {
  generatedAt: number
  team: string
  count: number
  candidates: OnesTaskCandidate[]
  recommendations: OnesRecommendation[]
  requirementTitle: string
  warnings: string[]
  cacheHit: boolean
  cachedAt: number
}
