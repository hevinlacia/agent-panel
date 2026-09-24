/**
 * Role: browser-side DTOs for the React dashboard island.
 * Public surface: API payload and shared UI DTO types used across React pages/features.
 * Constraints: mirrors /api/* JSON without importing Rust backend internals into Vite.
 * Read-this-with: src/main.rs and web/src/App.tsx.
 */

export type ReqStatus = "需求澄清" | "开发中" | "自测中" | "测试中" | "发布就绪" | "经验总结" | "已完成" | "排查中" | "已定位" | "已修复" | "已复盘" | "已关闭" | "需求创建" | "已合入" | "已取消"
export type ReqCategory = "需求" | "线上问题" | "测试问题"

export interface EffortEstimate {
  coefficient: number
  baseHours: number
  estimatedHours: number
  summary?: string
  updatedAt?: number
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

export interface StatusFlowPayload {
  ok: boolean
  reqId: string
  category?: string | null
  currentStatus: string
  currentKnown: boolean
  statuses: string[]
  currentIndex: number | null
  transitions: StatusFlowTransition[]
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
  mergeExcludedRepos?: string[]
  /** 未配置（undefined）时后端使用内置默认门禁；显式空数组 = 关闭所有门禁。 */
  statusGates?: StatusGateRule[] | null
  availableStatusGates?: StatusGateDef[]
  effectiveStatusGates?: StatusGateRule[]
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

export interface GitAiSuspectStats { total: number; pending: number; confirmedAi: number; missingAi: number; notFound: number; checkFailed: number }
export type GitAiCompanyStatus = "pending" | "confirmed_ai" | "missing_ai" | "not_found" | "check_failed"
export interface GitAiSuspectRecord { id: string; projectName: string; commitSha: string; shortSha: string; repoPath?: string | null; remoteUrl?: string | null; subject?: string | null; branch?: string | null; eventSources?: string[]; localNoteState?: string; companyStatus: GitAiCompanyStatus; companyCheckedAt?: number | null; companyError?: string | null; commitWebUrl?: string | null; commitTitle?: string | null; aiRate?: number | null; aiLines?: number | null; humanLines?: number | null; authorName?: string | null; lastSeenAt: number }
export interface GitAiSuspectsPayload { records: GitAiSuspectRecord[]; stats: GitAiSuspectStats; generatedAt: number }
export interface GitAiFixStep { label: string; command: string; ok: boolean; stdout?: string; stderr?: string }
export interface GitAiFixResponse {
  ok: boolean
  stillMissing: boolean
  recheck?: Record<string, unknown> & { companyStatus?: GitAiCompanyStatus; companyError?: string | null }
  pushSteps?: GitAiFixStep[]
  piAgent?: { dispatched: boolean; sessionId?: string; skillPath?: string; message: string }
}
export interface GitAiHookHealth { path: string | null; exists: boolean; mode: string; recordsToAgentPanel: boolean; executable: boolean }
export interface GitAiHealthPayload {
  generatedAt: number
  storePath: string
  cli: {
    binaryPath: string | null
    installed: boolean
    version: string | null
    daemonOk: boolean
    daemonMessage: string | null
    trace2Target: string | null
    trace2Socket: string | null
    trace2SocketExists: boolean
    hooksPath: string | null
    postCommitHook: GitAiHookHealth
    prePushHook: GitAiHookHealth
  }
  piExtension: {
    globalPath: string
    sourcePath: string
    globalExists: boolean
    sourceExists: boolean
    sourceMatchesGlobal: boolean
    autoDiscoveryPath: boolean
    gitAiBinaryExistsForExtension: boolean
    registersStatus: boolean
    tracksTools: string[]
    status: "ok" | "warn" | "error" | "unknown"
    message: string
  }
}

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
export interface MasterDiffPayload { ok: boolean; round?: number; branchScope?: BranchScope | null; snapshots: CodeDiffSnapshot[] }
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
}

export interface TestdataCapabilityItem {
  id: string
  domain: string
  object: string
  execution: string
  purpose: string
  script: string
  cli?: Record<string, TestdataCliField>
  targets?: TestdataTarget[]
}

export interface TestdataCapabilitiesPayload {
  ok: boolean
  project: string
  capabilities: TestdataCapabilityItem[]
}

export interface TestdataCapabilityPayload {
  ok: boolean
  capability: TestdataCapabilityItem & { pitfalls?: string[]; notes?: unknown[] }
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
