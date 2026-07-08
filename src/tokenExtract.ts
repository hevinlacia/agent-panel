/**
 * src/tokenExtract.ts
 *
 * Role: parse curl command text and extract JWT tokens for one-click env
 * var population. Currently supports Ylops DevOps tokens (access + refresh).
 *
 * Public surface:
 *   - extractTokensFromCurl(curlText): ExtractedToken[]
 *
 * Constraints / safety:
 *   - Pure module, no I/O, no external deps.
 *   - Only extracts known token patterns via regex; does not execute or
 *     eval any part of the input.
 *
 * Read-this-with:
 *   - src/config.ts (ENV_VAR_CATALOG defines target env var names + files)
 *   - src/server.tsx (/api/env-vars/extract-tokens route)
 *   - public/env-vars.js (extract modal UI handler)
 */

/** A single token extracted from a curl command. */
export interface ExtractedToken {
  /** Target env var name, e.g. "YLOPS_TOKEN". */
  name: string
  /** The raw token value. */
  value: string
  /** Which env file to write to. */
  file: "secrets" | "config" | "internal"
  /** Human-readable source description for the confirmation dialog. */
  source: string
}

/** JWT character class for regex matching. */
const JWT_CHARS = "[A-Za-z0-9._-]+"

/**
 * Extract Ylops tokens from a curl command string.
 *
 * Recognised patterns:
 *   - `Authorization: Bearer <jwt>` → YLOPS_TOKEN
 *   - `visionToken=<jwt>` (Cookie header) → YLOPS_TOKEN (fallback)
 *   - `visionRefresh=<jwt>` (Cookie header) → YLOPS_REFRESH_TOKEN
 *
 * Returns an empty array if no recognised tokens are found.
 */
export function extractTokensFromCurl(curlText: string): ExtractedToken[] {
  const tokens: ExtractedToken[] = []

  // 1. Authorization: Bearer <token>
  const authMatch = curlText.match(
    new RegExp(`Authorization:\\s*Bearer\\s*(${JWT_CHARS})`),
  )
  if (authMatch?.[1]) {
    tokens.push({
      name: "YLOPS_TOKEN",
      value: authMatch[1],
      file: "secrets",
      source: "Authorization header",
    })
  }

  // 2. Cookie header — look for visionToken and visionRefresh
  const cookieMatch = curlText.match(/Cookie:\s*([^\r\n'"]+)/)
  if (cookieMatch?.[1]) {
    const cookieStr = cookieMatch[1]

    // visionToken → YLOPS_TOKEN (only if Authorization didn't already provide it)
    if (!tokens.some((t) => t.name === "YLOPS_TOKEN")) {
      const vtMatch = cookieStr.match(
        new RegExp(`visionToken=(${JWT_CHARS})`),
      )
      if (vtMatch?.[1]) {
        tokens.push({
          name: "YLOPS_TOKEN",
          value: vtMatch[1],
          file: "secrets",
          source: "visionToken cookie",
        })
      }
    }

    // visionRefresh → YLOPS_REFRESH_TOKEN
    const vrMatch = cookieStr.match(
      new RegExp(`visionRefresh=(${JWT_CHARS})`),
    )
    if (vrMatch?.[1]) {
      tokens.push({
        name: "YLOPS_REFRESH_TOKEN",
        value: vrMatch[1],
        file: "secrets",
        source: "visionRefresh cookie",
      })
    }
  }

  return tokens
}
