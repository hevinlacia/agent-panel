/**
 * Role: browser-side DTOs for the React dashboard island.
 * Public surface: API payload and shared UI DTO types used across React pages/features.
 * Constraints: mirrors /api/* JSON without importing Rust backend internals into Vite.
 * Read-this-with: src/main.rs and web/src/App.tsx.
 */

export type ReqStatus = "需求澄清" | "开发中" | "自测中" | "测试中" | "经验总结" | "已完成" | "排查中" | "已确认"
export type ReqCategory = "需求" | "线上问题"

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

export interface Requirement {
  id: string
  title: string
  description?: string
  status: ReqStatus
  category?: ReqCategory
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
  experienceSummaryJob?: ExperienceSummaryJob
  prdPath?: string
  ones?: string
  effortEstimate?: EffortEstimate
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

export interface DashboardStats {
  total: number
  statusCounts: StatusCount[]
  durations: RequirementDuration[]
  avgDeliveryMs: number
  medianDeliveryMs: number
  maxDeliveryMs: number
  completedCount: number
  inProgressCount: number
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

export interface ConfigPayload {
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
export interface ReviewGatePayload { ok: boolean; reqId: string; gate: { status: string; label: string; allowsTesting: boolean; reason: string; source?: string | null; reviewPath: string; aiReviewPath: string; riskTags?: string[]; inventoryRisk?: boolean; staleRepos?: ReviewGateStaleRepo[]; incrementalReview?: CodeReviewSnapshot | null; checkedAt: number; actions: string[] } }
export interface MasterDiffPayload { ok: boolean; branchScope?: BranchScope | null; review?: CodeReviewSnapshot | null }
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
