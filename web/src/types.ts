/**
 * Role: browser-side DTOs for the React dashboard island.
 * Public surface: DashboardStatsPayload and supporting requirement/stat types.
 * Constraints: mirrors /api/dashboard/stats JSON without importing server modules into Vite.
 * Read-this-with: src/dashboardStats.ts and web/src/App.tsx.
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
