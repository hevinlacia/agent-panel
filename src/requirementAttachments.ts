/**
 * Requirement attachment files - list / read / write / delete helpers.
 *
 * Attachments live in `<reqDir>/attachments/` and are meant for
 * incidental artifacts tied to a requirement: SQL data files, CSV
 * exports, screenshots, etc. They are NOT context files parsed by
 * the Hermes skills (those live directly under `<reqDir>/`).
 *
 * Public surface:
 *   - ATTACHMENTS_DIR_NAME  - sub-directory name ("attachments")
 *   - AttachmentInfo         - metadata returned by listAttachments
 *   - resolveAttachmentPath - safe path gate, rejects traversal / absolute
 *   - listAttachments       - stat every file under the attachments dir
 *   - writeAttachment       - write (overwrite) a file by safe filename
 *   - deleteAttachment      - unlink a file by safe filename
 *   - readAttachmentBuffer  - raw bytes for download streaming
 *
 * Constraints / safety (AGENTS.md §3):
 *   - All filenames pass through `resolveAttachmentPath`, which rejects
 *     `..`, leading `/`, null bytes, and any resolved path that escapes
 *     the `<reqDir>/attachments/` prefix. This is the same boundary
 *     pattern as `resolveHandoffPath` in `src/paths.ts`.
 *   - Only `node:` built-ins are used. Never reads or writes any
 *     `.env` / secret file.
 *
 * Read-this-with: `src/paths.ts` for the sibling path-safety pattern,
 * `src/server.tsx` for the HTTP routes that consume this module.
 */

import { readFile, writeFile, unlink, readdir, mkdir, stat } from "node:fs/promises"
import { existsSync } from "node:fs"
import { join, resolve, basename } from "node:path"

export const ATTACHMENTS_DIR_NAME = "attachments"

/**
 * Metadata for a single attachment, as returned by `listAttachments`.
 * Sizes are in bytes; `mtime` is epoch milliseconds.
 */
export interface AttachmentInfo {
  filename: string
  size: number
  mtime: number
}

/**
 * Resolve a user-supplied filename to an absolute path inside
 * `<reqDir>/attachments/`, or return `null` if the name is unsafe.
 *
 * Rejects:
 *   - empty / non-string input
 *   - null bytes (smuggling vector)
 *   - paths containing `/` or `\` (no sub-directory traversal)
 *   - `.` or `..` (dot / dot-dot)
 *   - any resolved path that does not stay within the attachments dir
 *
 * Returns the resolved absolute path on success.
 */
export function resolveAttachmentPath(reqDir: string, filename: string): string | null {
  if (typeof filename !== "string" || filename.length === 0) return null
  if (filename.indexOf("\0") !== -1) return null
  // Reject any path separator - attachments are flat files, not trees.
  if (filename.includes("/") || filename.includes("\\")) return null
  const base = basename(filename)
  if (base !== filename) return null
  if (base === "." || base === "..") return null
  const dir = join(reqDir, ATTACHMENTS_DIR_NAME)
  const resolved = resolve(dir, base)
  const prefix = resolve(dir) + "/"
  // Exact dir match is not a file; reject. Otherwise enforce prefix boundary
  // so sibling dirs or escapes cannot impersonate the attachments root.
  if (resolved + "/" === prefix || (!resolved.startsWith(prefix) && resolved !== resolve(dir))) {
    return null
  }
  if (!resolved.startsWith(prefix)) return null
  return resolved
}

/**
 * Ensure the attachments directory exists for a requirement. Called
 * before write so a fresh requirement can receive attachments without
 * the caller pre-creating the directory.
 */
async function ensureDir(reqDir: string): Promise<string> {
  const dir = join(reqDir, ATTACHMENTS_DIR_NAME)
  await mkdir(dir, { recursive: true })
  return dir
}

/**
 * List all attachment files for a requirement, sorted by modification
 * time descending (newest first). Returns an empty array when the
 * attachments directory does not exist yet.
 */
export async function listAttachments(reqDir: string): Promise<AttachmentInfo[]> {
  const dir = join(reqDir, ATTACHMENTS_DIR_NAME)
  if (!existsSync(dir)) return []
  let names: string[]
  try {
    names = await readdir(dir)
  } catch {
    return []
  }
  const results: AttachmentInfo[] = []
  for (const name of names) {
    const safePath = resolveAttachmentPath(reqDir, name)
    if (!safePath) continue
    try {
      const s = await stat(safePath)
      if (!s.isFile()) continue
      results.push({ filename: name, size: s.size, mtime: s.mtimeMs })
    } catch {
      // File vanished between readdir and stat - skip it.
    }
  }
  results.sort((a, b) => b.mtime - a.mtime)
  return results
}

/**
 * Write (overwrite) an attachment file. The filename is validated by
 * `resolveAttachmentPath`; returns `null` if the name is rejected.
 */
export async function writeAttachment(
  reqDir: string,
  filename: string,
  content: Buffer | string,
): Promise<string | null> {
  const safePath = resolveAttachmentPath(reqDir, filename)
  if (!safePath) return null
  await ensureDir(reqDir)
  await writeFile(safePath, content)
  return safePath
}

/**
 * Delete an attachment file. Returns `true` if a file was removed,
 * `false` if the name was rejected or the file did not exist.
 */
export async function deleteAttachment(reqDir: string, filename: string): Promise<boolean> {
  const safePath = resolveAttachmentPath(reqDir, filename)
  if (!safePath) return false
  if (!existsSync(safePath)) return false
  await unlink(safePath)
  return true
}

/**
 * Read raw bytes of an attachment for download streaming. Returns
 * `null` if the name is rejected; throws if the file vanished between
 * the existence check and the read (caller should catch and 404).
 */
export async function readAttachmentBuffer(
  reqDir: string,
  filename: string,
): Promise<Buffer | null> {
  const safePath = resolveAttachmentPath(reqDir, filename)
  if (!safePath) return null
  if (!existsSync(safePath)) return null
  return readFile(safePath)
}
