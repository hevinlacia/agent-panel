/**
 * Role: browser-side DTOs for the React dashboard island.
 * Public surface: DashboardStatsPayload and supporting requirement/stat types.
 * Constraints: mirrors /api/dashboard/stats JSON without importing Rust backend internals into Vite.
 * Read-this-with: src/main.rs and web/src/App.tsx.
 */

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
