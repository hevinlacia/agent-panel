/**
 * Dashboard configuration store.
 *
 * Role: persist user-editable settings (extract mode, model, threshold)
 * to `~/.local/share/opencode-dashboard/config.json` so they survive
 * dashboard restarts.
 *
 * Public surface:
 *   - getConfig(): read current config (in-memory cache + lazy load)
 *   - setConfig(partial): merge + persist
 *   - initConfig(): load from disk at startup
 *   - _resetForTest(path): test-only path override
 *
 * Constraints / safety:
 *   - Only `node:` built-ins.
 *   - Never reads or writes `.env` / secret files.
 *
 * Read-this-with:
 *   - `src/server.tsx` (/settings route + /api/config)
 *   - `src/sessionExtract.ts` (EXTRACT_MODEL is the fallback default)
 */

import { mkdir, readFile, writeFile } from "node:fs/promises"
import { existsSync } from "node:fs"
import { homedir } from "node:os"
import { dirname, join } from "node:path"

export interface AppConfig {
  /**
   * false (default) = manual trigger only — user clicks "提取上下文".
   * true = auto-trigger when an associated session transitions to idle
   * and the message delta exceeds `minChangeMessages`.
   */
  autoExtract: boolean
  /**
   * Model used for extract spawns. Falls back to
   * `litellm-local/deepseek-v4-flash-auto` when empty.
   */
  extractModel: string
  /**
   * Minimum number of new messages since the last extract for the
   * auto-trigger to fire. Prevents wasting tokens on trivial changes.
   */
  minChangeMessages: number
}

const DEFAULTS: AppConfig = {
  autoExtract: false,
  extractModel: "litellm-local/deepseek-v4-flash-auto",
  minChangeMessages: 5,
}

const DEFAULT_PATH = join(
  homedir(),
  ".local",
  "share",
  "opencode-dashboard",
  "config.json",
)

let _path = DEFAULT_PATH
let _cache: AppConfig | null = null

/** Load config from disk. Call once at startup. */
export async function initConfig(): Promise<void> {
  _cache = null
  await load()
}

async function load(): Promise<AppConfig> {
  if (_cache) return _cache
  if (!existsSync(_path)) {
    _cache = { ...DEFAULTS }
    return _cache
  }
  try {
    const raw = await readFile(_path, "utf-8")
    const parsed = JSON.parse(raw)
    _cache = {
      autoExtract: parsed.autoExtract ?? DEFAULTS.autoExtract,
      extractModel: parsed.extractModel || DEFAULTS.extractModel,
      minChangeMessages: parsed.minChangeMessages ?? DEFAULTS.minChangeMessages,
    }
  } catch {
    _cache = { ...DEFAULTS }
  }
  return _cache
}

export async function getConfig(): Promise<AppConfig> {
  return load()
}

export async function setConfig(
  partial: Partial<Pick<AppConfig, "autoExtract" | "extractModel" | "minChangeMessages">>,
): Promise<AppConfig> {
  const cur = await load()
  const next: AppConfig = {
    autoExtract: partial.autoExtract ?? cur.autoExtract,
    extractModel: partial.extractModel ?? cur.extractModel,
    minChangeMessages: partial.minChangeMessages ?? cur.minChangeMessages,
  }
  _cache = next
  const dir = dirname(_path)
  if (!existsSync(dir)) await mkdir(dir, { recursive: true })
  await writeFile(_path, JSON.stringify(next, null, 2), "utf-8")
  return next
}

export function _resetForTest(path: string): void {
  _path = path
  _cache = null
}
