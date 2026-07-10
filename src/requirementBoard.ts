/**
 * Role: pure filtering and hierarchy helpers for the flat requirement board.
 * Public surface: RequirementBoardFilters, RequirementBoardItem,
 * buildRequirementBoardItems, parseRequirementDateBoundary.
 * Constraints: no I/O; preserve requirement records and filter by creation time.
 * Read-this-with: src/requirements.ts and src/server.tsx.
 */

import type { Requirement, ReqStatus } from "./requirements.ts"

/** Query-backed filters accepted by the requirement board. */
export interface RequirementBoardFilters {
  statuses: ReqStatus[]
  project: string
  subproject: string
  createdFrom?: number
  createdTo?: number
}

/** Flat board row with display-ready project hierarchy metadata. */
export interface RequirementBoardItem {
  requirement: Requirement
  project: string
  subproject: string
  hierarchy: string
}

/**
 * Convert an HTML date value into a local-day boundary. Invalid values are
 * ignored so a malformed query cannot hide every requirement unexpectedly.
 */
export function parseRequirementDateBoundary(value: string, endOfDay = false): number | undefined {
  if (!/^\d{4}-\d{2}-\d{2}$/.test(value)) return undefined
  const suffix = endOfDay ? "T23:59:59.999" : "T00:00:00.000"
  const timestamp = new Date(`${value}${suffix}`).getTime()
  return Number.isFinite(timestamp) ? timestamp : undefined
}

/**
 * Flatten project groups and apply status, creation-date, and two-level
 * project filters. With no explicit status selection, completed work stays
 * hidden to keep the default board focused on current requirements.
 */
export function buildRequirementBoardItems(
  groups: { project: string; requirements: Requirement[] }[],
  filters: RequirementBoardFilters,
): RequirementBoardItem[] {
  const explicitStatuses = filters.statuses.length > 0
  const allowedStatuses = new Set(filters.statuses)
  const items: RequirementBoardItem[] = []

  for (const group of groups) {
    for (const requirement of group.requirements) {
      if (explicitStatuses) {
        if (!allowedStatuses.has(requirement.status)) continue
      } else if (requirement.status === "已完成") {
        continue
      }
      if (filters.createdFrom !== undefined && requirement.createdAt < filters.createdFrom) continue
      if (filters.createdTo !== undefined && requirement.createdAt > filters.createdTo) continue
      if (filters.project && requirement.project !== filters.project) continue

      const subproject = requirement.groupPath[0] ?? ""
      if (filters.subproject && subproject !== filters.subproject) continue

      const hierarchyParts = [requirement.project, ...requirement.groupPath]
      items.push({
        requirement,
        project: requirement.project,
        subproject,
        hierarchy: hierarchyParts.filter(Boolean).join(" / "),
      })
    }
  }

  return items.sort((a, b) => b.requirement.updatedAt - a.requirement.updatedAt)
}
