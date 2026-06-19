/**
 * Side-effect-free helpers for resolving and validating filesystem paths
 * referenced from user input (report artifacts, etc.).
 *
 * Kept in its own module so unit tests can import it without booting
 * the Hono server in `src/server.tsx`.
 */

import { resolve } from "node:path"

/**
 * /tmp/opencode/handoff/ is the only writable root for report artifacts.
 * `resolveHandoffPath` resolves ".." segments and enforces a strict prefix
 * boundary so sibling directories (e.g. /tmp/opencode/handoff-evil) and
 * escapes (e.g. /tmp/opencode/handoff/../etc/passwd) are rejected.
 */
export const HANDOFF_ROOT = "/tmp/opencode/handoff"
const HANDOFF_ROOT_PREFIX = HANDOFF_ROOT + "/"

export function resolveHandoffPath(reportPath: unknown): string | null {
  if (typeof reportPath !== "string" || reportPath.length === 0) return null
  // Reject null bytes early; they are never legal in fs paths and are a
  // common way to smuggle past naive string comparisons.
  if (reportPath.indexOf("\0") !== -1) return null
  if (!reportPath.startsWith("/")) return null
  const resolved = resolve(reportPath)
  if (resolved !== HANDOFF_ROOT && !resolved.startsWith(HANDOFF_ROOT_PREFIX)) {
    return null
  }
  return resolved
}
