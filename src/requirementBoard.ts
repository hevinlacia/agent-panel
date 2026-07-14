/**
 * Role: pure filtering and hierarchy helpers for the flat requirement board.
 * Public surface: RequirementBoardFilters, RequirementBoardItem,
 * buildRequirementBoardItems, parseRequirementDateBoundary.
 * Constraints: no I/O; preserve requirement records and filter by creation time.
 * Read-this-with: src/requirements.ts and src/server.tsx.
 */

import type { Requirement, ReqStatus, ReqCategory } from "./requirements.ts"

/** Query-backed filters accepted by the requirement board. */
export interface RequirementBoardFilters {
  statuses: ReqStatus[]
  project: string
  subproject: string
  /** When set, only requirements with this category are shown. */
  category?: ReqCategory
  /** Case-insensitive keyword matched against id, title, description, and project path. */
  keyword?: string
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
  const keyword = (filters.keyword || "").trim().toLowerCase()
  const items: RequirementBoardItem[] = []
  const seen = new Set<string>()

  for (const group of groups) {
    for (const requirement of group.requirements) {
      if (explicitStatuses) {
        if (!allowedStatuses.has(requirement.status)) continue
      } else if (requirement.status === "已完成") {
        continue
      }
      // Category filter: undefined / "需求" are treated the same so legacy
      // requirements without an explicit category match the default.
      if (filters.category) {
        const reqCategory = requirement.category ?? "需求"
        if (reqCategory !== filters.category) continue
      }
      if (filters.createdFrom !== undefined && requirement.createdAt < filters.createdFrom) continue
      if (filters.createdTo !== undefined && requirement.createdAt > filters.createdTo) continue
      const requirementProjects = requirement.projects?.length ? requirement.projects : [requirement.project]
      if (filters.project && !requirementProjects.includes(filters.project)) continue
      if (seen.has(requirement.id)) continue

      const subproject = requirement.groupPath[0] ?? ""
      if (filters.subproject && subproject !== filters.subproject) continue

      const displayProject = filters.project || requirementProjects[0] || requirement.project
      const hierarchyParts = [displayProject, ...requirement.groupPath]
      const hierarchy = hierarchyParts.filter(Boolean).join(" / ")

      // Keyword search: match id, title, description, and the full hierarchy path.
      if (keyword) {
        const haystack = [
          requirement.id,
          requirement.title,
          requirement.description,
          hierarchy,
        ].join(" ").toLowerCase()
        if (!haystack.includes(keyword)) continue
      }

      items.push({
        requirement,
        project: displayProject,
        subproject,
        hierarchy,
      })
      seen.add(requirement.id)
    }
  }

  return items.sort((a, b) => b.requirement.updatedAt - a.requirement.updatedAt)
}
