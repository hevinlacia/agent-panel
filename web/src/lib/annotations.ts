/**
 * Role: match CodeAnnotations (code-annotations.json) onto parsed diff views.
 * Matching granularity: file-level annotations key by `repo:path`; hunk-level
 * notes anchor by the exact `@@ -a,b +c,d @@` header prefix copied from the
 * diff text, with optional context lines as a disambiguator. Notes whose
 * header no longer matches the current diff degrade to an "unmatched" group
 * instead of being silently dropped.
 */
import type { AnnotationNote, CodeAnnotations, CodeFileAnnotation } from "../types"
import type { DiffFileView, DiffLine } from "./diff"

export type FileKey = string

/**
 * Defensive normalizer for code-annotations.json content. The backend rejects
 * malformed writes with a schema hint, but older/drifted documents may still
 * contain e.g. `variables` as a pipe-formatted string; rendering such data
 * crashes the whole React tree ("variables.map is not a function"). Normalize
 * shapes here so the diff page degrades gracefully instead of white-screening.
 */
export function normalizeCodeAnnotations(raw: unknown): CodeAnnotations | null {
  if (!raw || typeof raw !== "object" || Array.isArray(raw)) return null
  const doc = raw as CodeAnnotations & Record<string, unknown>
  const files = Array.isArray(doc.files)
    ? doc.files.map(normalizeFileAnnotation).filter((a): a is CodeFileAnnotation => a != null)
    : []
  return { ...doc, files }
}

function normalizeFileAnnotation(raw: unknown): CodeFileAnnotation | null {
  if (!raw || typeof raw !== "object" || Array.isArray(raw)) return null
  const a = raw as CodeFileAnnotation & Record<string, unknown>
  const out: CodeFileAnnotation = {
    repo: typeof a.repo === "string" ? a.repo : "",
    path: typeof a.path === "string" ? a.path : "",
  }
  for (const key of ["summary", "flow", "flowTitle", "flowType"] as const) {
    if (typeof a[key] === "string") out[key] = a[key]
  }
  if (Array.isArray(a.variables)) {
    out.variables = (a.variables as unknown[])
      .filter((v): v is Record<string, unknown> => Boolean(v) && typeof v === "object" && !Array.isArray(v))
      .map((v) => ({
        name: typeof v.name === "string" ? v.name : String(v.name ?? ""),
        ...(typeof v.kind === "string" ? { kind: v.kind } : {}),
        ...(typeof v.meaning === "string" ? { meaning: v.meaning } : {}),
        ...(typeof v.why === "string" ? { why: v.why } : {}),
      }))
  }
  if (Array.isArray(a.notes)) {
    out.notes = (a.notes as unknown[])
      .filter((n): n is Record<string, unknown> => Boolean(n) && typeof n === "object" && !Array.isArray(n))
      .map((n) => ({
        ...(n.anchor && typeof n.anchor === "object" && !Array.isArray(n.anchor)
          ? { anchor: n.anchor as AnnotationNote["anchor"] }
          : {}),
        ...(typeof n.title === "string" ? { title: n.title } : {}),
        note: typeof n.note === "string" ? n.note : String(n.note ?? ""),
      }))
      .filter((n) => n.note.trim().length > 0)
  }
  return out
}

export function fileKeyOf(view: DiffFileView): FileKey {
  return `${view.repo.repoName}:${view.file.path}`
}

/** Build fileKey -> annotation index for the current diff. */
export function buildAnnotationIndex(
  files: DiffFileView[],
  annotations: CodeAnnotations | null | undefined,
): Map<FileKey, CodeFileAnnotation> {
  const index = new Map<FileKey, CodeFileAnnotation>()
  if (!annotations?.files?.length) return index
  const byRepoPath = new Map<string, CodeFileAnnotation>()
  for (const ann of annotations.files) {
    if (!ann?.repo || !ann?.path) continue
    byRepoPath.set(`${ann.repo}:${ann.path}`, ann)
  }
  if (!byRepoPath.size) return index
  for (const view of files) {
    const key = fileKeyOf(view)
    const hit = byRepoPath.get(key)
    if (hit) index.set(key, hit)
  }
  return index
}

/** Index of the hunk header line each diff line belongs to (-1 = before first hunk). */
export function hunkOwners(lines: DiffLine[]): number[] {
  const owners: number[] = []
  let current = -1
  lines.forEach((line, i) => {
    if (line.type === "hunk") current = i
    owners.push(current)
  })
  return owners
}

export interface MatchedNote {
  note: AnnotationNote
  /** Index of the diff line the note is anchored to (the hunk header line). */
  lineIndex: number
}

export interface HunkNoteMatch {
  /** Anchored notes, ordered by position in the diff. */
  matched: MatchedNote[]
  /** Notes whose anchor header did not match any current hunk. */
  unmatched: AnnotationNote[]
}

interface HunkInfo {
  lineIndex: number
  header: string
  context: string[]
}

function collectHunks(lines: DiffLine[]): HunkInfo[] {
  const hunks: HunkInfo[] = []
  let current: HunkInfo | null = null
  for (let i = 0; i < lines.length; i += 1) {
    const line = lines[i]
    if (line.type === "hunk") {
      if (current) hunks.push(current)
      current = { lineIndex: i, header: line.text, context: [] }
      continue
    }
    if (current && line.type === "ctx" && current.context.length < 3) {
      current.context.push(line.text)
    }
  }
  if (current) hunks.push(current)
  return hunks
}

function contextMatches(hunk: HunkInfo, wanted: string[] | undefined): boolean {
  if (!wanted?.length) return true
  const first = wanted[0]
  return hunk.context.some((line) => line.trim() === first.trim())
}

/**
 * Match a file's hunk notes onto parsed diff lines. A note matches when the
 * stored hunkHeader is a prefix of a hunk header line in the diff; when
 * several hunks share the same header prefix, the optional context lines pick
 * the first hunk whose leading context matches.
 */
export function matchHunkNotes(lines: DiffLine[], notes: AnnotationNote[] | undefined): HunkNoteMatch {
  const matched: MatchedNote[] = []
  const unmatched: AnnotationNote[] = []
  if (!notes?.length) return { matched, unmatched }
  const hunks = collectHunks(lines)
  for (const note of notes) {
    const header = note?.anchor?.hunkHeader?.trim()
    if (!header || !hunks.length) {
      unmatched.push(note)
      continue
    }
    const candidates = hunks.filter((h) => h.header.startsWith(header) || header.startsWith(h.header))
    const hit = candidates.find((h) => contextMatches(h, note.anchor?.context)) || candidates[0]
    if (hit) matched.push({ note, lineIndex: hit.lineIndex })
    else unmatched.push(note)
  }
  matched.sort((a, b) => a.lineIndex - b.lineIndex)
  return { matched, unmatched }
}

export function hasAnnotationContent(annotation: CodeFileAnnotation | null | undefined): boolean {
  if (!annotation) return false
  return Boolean(
    annotation.summary?.trim()
    || annotation.variables?.length
    || annotation.flow?.trim()
    || annotation.notes?.length,
  )
}

/**
 * Stale marker: an annotation file records the reviewed target commit per
 * repo; when the diff snapshot advanced past it, anchored notes may be off.
 */
export function annotationStaleRepos(
  annotations: CodeAnnotations | null | undefined,
  files: DiffFileView[],
): string[] {
  const reviewed = annotations?.reviewedCommit || {}
  const stale = new Set<string>()
  for (const view of files) {
    const commit = reviewed[view.repo.repoName]
    if (commit && view.repo.targetCommit && commit !== view.repo.targetCommit) {
      stale.add(view.repo.repoName)
    }
  }
  return [...stale]
}
