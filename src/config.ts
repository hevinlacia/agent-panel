/**
 * Dashboard configuration store.
 *
 * Role: persist dashboard settings and manage OpenCode env variables from
 * the same env files used by skills and the SOPS-backed sync workflow.
 *
 * Public surface:
 *   - getConfig()/setConfig(): read and persist dashboard behavior toggles
 *   - safeEnvVars()/safeEnvVarsByFile(): redacted variable list for UI/API
 *   - getEnvFileMeta(): env file metadata for UI display
 *   - upsertEnvVar/deleteEnvVar: mutate OpenCode env files safely
 *   - ENV_VAR_CATALOG: always-visible known variable requirements
 *   - buildManagedEnv(extra): process env plus dashboard-managed variables
 *   - initConfig(): load from disk at startup
 *   - _resetForTest(path): test-only path override
 *
 * Constraints / safety:
 *   - Only `node:` built-ins.
 *   - Values are never returned to the browser; env files are edited
 *     locally and later encrypted by the workstation SOPS sync workflow.
 *
 * Read-this-with:
 *   - `src/server.tsx` (/settings route + /api/config)
 *   - `src/sessionExtract.ts` (EXTRACT_MODEL is the fallback default)
 */

import { chmod, mkdir, readFile, writeFile } from "node:fs/promises"
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
   * false (default) = nightly auto-extract is disabled.
   * true = the background scheduler fires at midnight (local time)
   * each night, sweeping all requirement-bound sessions. Sessions that
   * have never been smart-extracted are forked and their content is
   * analyzed to update requirement files. Already-extracted sessions
   * are skipped permanently.
   */
  autoExtractSchedule: boolean
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
  /**
   * false (default) = auto-valuation worker discovers candidates but
   * does NOT auto-mark them. The user must manually mark from the
   * candidate list.
   * true = the worker auto-marks sessions whose score ≥
   * `valuationThreshold`, feeding them directly into the experience-
   * summary pipeline.
   */
  autoValuation: boolean
  /**
   * Minimum score (0–100) for a session to be considered a candidate.
   * Sessions below this score are filtered out. Default: 25.
   */
  valuationThreshold: number
  /**
   * true (default) = dashboard runs the full OpenCode config sync once
   * per day at 20:30. This replaces frequent hook/systemd auto-syncs.
   */
  fullSyncSchedule: boolean
  /**
   * Legacy dashboard-managed variables from older versions. New writes go
   * to OpenCode env files so the SOPS-backed workstation sync can persist
   * them. These are still read for migration/fallback compatibility.
   */
  envVars: EnvVarEntry[]
}

export interface EnvVarEntry {
  name: string
  value: string
  note: string
  updatedAt: number
}

export interface SafeEnvVarEntry {
  name: string
  preview: string
  note: string
  updatedAt: number
  hasValue: boolean
  source: "managed" | "process" | "missing"
  requiredBy: string
  description: string
  /** Which env file the variable is stored in (or should be stored in). */
  file: EnvFileKind
  /** Filesystem path of the env file. */
  filePath: string
}

/** A group of variables belonging to one env file, with file metadata. */
export interface EnvFileGroup {
  file: EnvFileKind
  label: string
  path: string
  sensitive: boolean
  variables: SafeEnvVarEntry[]
}

export type ManagedEnv = Record<string, string>

export interface EnvVarCatalogEntry {
  name: string
  requiredBy: string
  description: string
  placeholder: string
  file: EnvFileKind
}

export type EnvFileKind = "config" | "internal" | "secrets"

export interface EnvFileValue {
  value: string
  file: EnvFileKind
  path: string
}

const OPENCODE_CONFIG_DIR = join(homedir(), ".config", "opencode")
let _envFiles = defaultEnvFiles(OPENCODE_CONFIG_DIR)

function defaultEnvFiles(configDir: string): Record<EnvFileKind, { path: string; sensitive: boolean; label: string }> {
  return {
    config: { path: join(configDir, "opencode-config.env"), sensitive: false, label: "opencode-config.env" },
    internal: { path: join(configDir, "opencode-internal.env"), sensitive: true, label: "opencode-internal.env" },
    secrets: { path: join(configDir, "opencode-secrets.env"), sensitive: true, label: "opencode-secrets.env" },
  }
}

export const ENV_VAR_CATALOG: readonly EnvVarCatalogEntry[] = [
  // ── config (non-sensitive) ──
  { name: "OPENCODE_MODEL_SUBAGENT", requiredBy: "Model routing", description: "Model ID used for subagent spawns.", placeholder: "ark-key-router/deepseek-v4-flash-auto", file: "config" },
  { name: "OPENCODE_MODEL_REVIEW", requiredBy: "Model routing", description: "Model ID used for code review.", placeholder: "MiniMax/MiniMax-M3", file: "config" },
  { name: "OPENCODE_KIBANA_TEST_USERNAME_SET", requiredBy: "Kibana log query", description: "Kibana test environment username.", placeholder: "", file: "config" },
  { name: "OPENCODE_KIBANA_TEST_PASSWORD_SET", requiredBy: "Kibana log query", description: "Kibana test environment password.", placeholder: "", file: "config" },
  { name: "OPENCODE_KIBANA_CN_INDEX_UAT", requiredBy: "Kibana log query", description: "CN UAT Kibana index pattern.", placeholder: "uat-cwh*applog*", file: "config" },
  { name: "OPENCODE_KIBANA_CN_INDEX_PRO", requiredBy: "Kibana log query", description: "CN PRO Kibana index pattern.", placeholder: "pro-cwh*-applog*", file: "config" },
  { name: "OPENCODE_KIBANA_SEA_INDEX_UAT", requiredBy: "Kibana log query", description: "SEA UAT Kibana index pattern.", placeholder: "uat-cwhsea-applog*", file: "config" },
  { name: "OPENCODE_KIBANA_SEA_INDEX_PRO", requiredBy: "Kibana log query", description: "SEA PRO Kibana index pattern.", placeholder: "pro-cwhsea*applog*", file: "config" },
  { name: "OPENCODE_NACOS_TEST_URL", requiredBy: "Nacos config query", description: "Nacos test environment base URL.", placeholder: "http://10.x.x.x:port/nacos", file: "config" },
  { name: "OPENCODE_NACOS_UAT_CN_URL", requiredBy: "Nacos config query", description: "Nacos CN UAT base URL.", placeholder: "http://10.x.x.x:port/nacos", file: "config" },
  { name: "OPENCODE_NACOS_UAT_SEA_URL", requiredBy: "Nacos config query", description: "Nacos SEA UAT base URL.", placeholder: "http://10.x.x.x:port/nacos", file: "config" },
  { name: "OPENCODE_ARCHERY_UAT_CN_INSTANCE", requiredBy: "MySQL query", description: "Archery CN UAT MySQL instance name.", placeholder: "", file: "config" },
  { name: "OPENCODE_ARCHERY_PRO_CN_INSTANCE", requiredBy: "MySQL query", description: "Archery CN PRO MySQL instance name.", placeholder: "", file: "config" },
  { name: "OPENCODE_ARCHERY_UAT_SEA_INSTANCE", requiredBy: "MySQL query", description: "Archery SEA UAT MySQL instance name.", placeholder: "", file: "config" },
  { name: "OPENCODE_ARCHERY_PRO_SEA_INSTANCE", requiredBy: "MySQL query", description: "Archery SEA PRO MySQL instance name.", placeholder: "", file: "config" },
  { name: "OPENCODE_ARCHERY_TEST_SEA_INSTANCE", requiredBy: "MySQL query", description: "Archery SEA test MySQL instance name.", placeholder: "", file: "config" },

  // ── internal (network credentials, SOPS encrypted) ──
  { name: "OPENCODE_DB_MYSQL_USERNAME", requiredBy: "MySQL direct query", description: "MySQL database username for direct JDBC.", placeholder: "", file: "internal" },
  { name: "OPENCODE_DB_MYSQL_PASSWORD", requiredBy: "MySQL direct query", description: "MySQL database password for direct JDBC.", placeholder: "", file: "internal" },
  { name: "OPENCODE_ES_PASSWORD", requiredBy: "Elasticsearch query", description: "Elasticsearch password.", placeholder: "", file: "internal" },
  { name: "OPENCODE_WMS_TEST_USERNAME", requiredBy: "WMS test API", description: "WMS test environment login username.", placeholder: "", file: "internal" },
  { name: "OPENCODE_WMS_TEST_PASSWORD", requiredBy: "WMS test API", description: "WMS test environment login password.", placeholder: "", file: "internal" },
  { name: "OPENCODE_WMS_UAT_CN_USERNAME", requiredBy: "WMS UAT API", description: "WMS CN UAT login username.", placeholder: "", file: "internal" },
  { name: "OPENCODE_WMS_UAT_CN_PASSWORD", requiredBy: "WMS UAT API", description: "WMS CN UAT login password.", placeholder: "", file: "internal" },
  { name: "OPENCODE_WMS_UAT_SEA_USERNAME", requiredBy: "WMS UAT API", description: "WMS SEA UAT login username.", placeholder: "", file: "internal" },
  { name: "OPENCODE_WMS_UAT_SEA_PASSWORD", requiredBy: "WMS UAT API", description: "WMS SEA UAT login password.", placeholder: "", file: "internal" },
  { name: "OPENCODE_KIBANA_CN_SID", requiredBy: "Kibana log query", description: "Kibana CN session ID cookie.", placeholder: "", file: "internal" },
  { name: "OPENCODE_KIBANA_CN_USERNAME", requiredBy: "Kibana log query", description: "Kibana CN login username.", placeholder: "", file: "internal" },
  { name: "OPENCODE_KIBANA_CN_PASSWORD", requiredBy: "Kibana log query", description: "Kibana CN login password.", placeholder: "", file: "internal" },
  { name: "OPENCODE_KIBANA_SEA_USERNAME", requiredBy: "Kibana log query", description: "Kibana SEA login username.", placeholder: "", file: "internal" },
  { name: "OPENCODE_KIBANA_SEA_PASSWORD", requiredBy: "Kibana log query", description: "Kibana SEA login password.", placeholder: "", file: "internal" },
  { name: "OPENCODE_KIBANA_SEA_SID", requiredBy: "Kibana log query", description: "Kibana SEA session ID cookie.", placeholder: "", file: "internal" },
  { name: "OPENCODE_KIBANA_TEST_SID", requiredBy: "Kibana log query", description: "Kibana test session ID cookie.", placeholder: "", file: "internal" },
  { name: "OPENCODE_NACOS_TEST_USERNAME", requiredBy: "Nacos config query", description: "Nacos test username.", placeholder: "", file: "internal" },
  { name: "OPENCODE_NACOS_TEST_PASSWORD", requiredBy: "Nacos config query", description: "Nacos test password.", placeholder: "", file: "internal" },
  { name: "OPENCODE_NACOS_UAT_CN_USERNAME", requiredBy: "Nacos config query", description: "Nacos CN UAT username.", placeholder: "", file: "internal" },
  { name: "OPENCODE_NACOS_UAT_CN_PASSWORD", requiredBy: "Nacos config query", description: "Nacos CN UAT password.", placeholder: "", file: "internal" },
  { name: "OPENCODE_NACOS_UAT_SEA_USERNAME", requiredBy: "Nacos config query", description: "Nacos SEA UAT username.", placeholder: "", file: "internal" },
  { name: "OPENCODE_NACOS_UAT_SEA_PASSWORD", requiredBy: "Nacos config query", description: "Nacos SEA UAT password.", placeholder: "", file: "internal" },
  { name: "OPENCODE_ARCHERY_USERNAME", requiredBy: "MySQL query", description: "Archery platform username.", placeholder: "", file: "internal" },
  { name: "OPENCODE_ARCHERY_PASSWORD", requiredBy: "MySQL query", description: "Archery platform password.", placeholder: "", file: "internal" },

  // ── secrets (high-risk API keys, SOPS encrypted) ──
  { name: "OPENCODE_AI_OPENAI_HEVIN_API_KEY", requiredBy: "AI model routing", description: "OpenAI API key (Hevin).", placeholder: "sk-…", file: "secrets" },
  { name: "OPENCODE_AI_OPENAI_GRAVIN_API_KEY", requiredBy: "AI model routing", description: "OpenAI API key (Gravin).", placeholder: "sk-…", file: "secrets" },
  { name: "OPENCODE_AI_MINIMAX_API_KEY", requiredBy: "AI model routing", description: "MiniMax API key.", placeholder: "", file: "secrets" },
  { name: "OPENCODE_AI_IFLYTEK_API_KEY", requiredBy: "AI model routing", description: "iFlytek API key.", placeholder: "", file: "secrets" },
  { name: "OPENCODE_AI_ARK_HEVIN_API_KEY", requiredBy: "AI model routing", description: "Ark API key (Hevin).", placeholder: "", file: "secrets" },
  { name: "OPENCODE_AI_ARK_MOSS_API_KEY", requiredBy: "AI model routing", description: "Ark API key (Moss).", placeholder: "", file: "secrets" },
  { name: "OPENCODE_AI_ARK_WILFORD_API_KEY", requiredBy: "AI model routing", description: "Ark API key (Wilford).", placeholder: "", file: "secrets" },
  { name: "OPENCODE_AI_ARK_CYRIL_API_KEY", requiredBy: "AI model routing", description: "Ark API key (Cyril).", placeholder: "", file: "secrets" },
  { name: "OPENCODE_AI_ARK_KHAINE_API_KEY", requiredBy: "AI model routing", description: "Ark API key (Khaine).", placeholder: "", file: "secrets" },
  { name: "OPENCODE_AI_ARK_GARVIN_API_KEY", requiredBy: "AI model routing", description: "Ark API key (Garvin).", placeholder: "", file: "secrets" },
  { name: "OPENCODE_AI_DEEPSEEK_API_KEY", requiredBy: "AI model routing", description: "DeepSeek API key.", placeholder: "", file: "secrets" },
  { name: "OPENCODE_AI_CONTEXT7_API_KEY", requiredBy: "Context7 MCP", description: "Context7 API key for doc lookup.", placeholder: "", file: "secrets" },
  { name: "OPENCODE_AI_BRAVE_API_KEY", requiredBy: "Brave Search MCP", description: "Brave Search API key.", placeholder: "", file: "secrets" },
  { name: "OPENCODE_AI_LITELLM_API_KEY", requiredBy: "LiteLLM router", description: "LiteLLM router API key.", placeholder: "", file: "secrets" },
  { name: "GITHUB_TOKEN", requiredBy: "GitHub CLI / MCP", description: "GitHub personal access token.", placeholder: "ghp_…", file: "secrets" },
  { name: "YLOPS_TOKEN", requiredBy: "Ylops CI/CD Deploy", description: "DevOps JWT token used for Ylops build/deploy requests; copied after browser login when captcha blocks automation.", placeholder: "Paste Ylops JWT token", file: "secrets" },
] as const

const DEFAULTS: AppConfig = {
  autoExtract: false,
  autoExtractSchedule: false,
  extractModel: "litellm-local/deepseek-v4-flash-auto",
  minChangeMessages: 5,
  autoValuation: false,
  valuationThreshold: 25,
  fullSyncSchedule: true,
  envVars: [],
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
    _cache = { ...DEFAULTS, envVars: [] }
    return _cache
  }
  try {
    const raw = await readFile(_path, "utf-8")
    const parsed = JSON.parse(raw)
    _cache = {
      autoExtract: parsed.autoExtract ?? DEFAULTS.autoExtract,
      autoExtractSchedule: parsed.autoExtractSchedule ?? DEFAULTS.autoExtractSchedule,
      extractModel: parsed.extractModel || DEFAULTS.extractModel,
      minChangeMessages: parsed.minChangeMessages ?? DEFAULTS.minChangeMessages,
      autoValuation: parsed.autoValuation ?? DEFAULTS.autoValuation,
      valuationThreshold: parsed.valuationThreshold ?? DEFAULTS.valuationThreshold,
      fullSyncSchedule: parsed.fullSyncSchedule ?? DEFAULTS.fullSyncSchedule,
      envVars: normalizeEnvVars(parsed.envVars),
    }
  } catch {
    _cache = { ...DEFAULTS, envVars: [] }
  }
  return _cache
}

export async function getConfig(): Promise<AppConfig> {
  return load()
}

export async function setConfig(
  partial: Partial<Pick<AppConfig, "autoExtract" | "autoExtractSchedule" | "extractModel" | "minChangeMessages" | "autoValuation" | "valuationThreshold" | "fullSyncSchedule" | "envVars">>,
): Promise<AppConfig> {
  const cur = await load()
  const next: AppConfig = {
    autoExtract: partial.autoExtract ?? cur.autoExtract,
    autoExtractSchedule: partial.autoExtractSchedule ?? cur.autoExtractSchedule,
    extractModel: partial.extractModel ?? cur.extractModel,
    minChangeMessages: partial.minChangeMessages ?? cur.minChangeMessages,
    autoValuation: partial.autoValuation ?? cur.autoValuation,
    valuationThreshold: partial.valuationThreshold ?? cur.valuationThreshold,
    fullSyncSchedule: partial.fullSyncSchedule ?? cur.fullSyncSchedule,
    envVars: partial.envVars ? normalizeEnvVars(partial.envVars) : cur.envVars,
  }
  _cache = next
  const dir = dirname(_path)
  if (!existsSync(dir)) await mkdir(dir, { recursive: true })
  await writeFile(_path, JSON.stringify(next, null, 2), "utf-8")
  return next
}

/** Return UI-safe variable metadata without exposing full secret values. */
export async function safeEnvVars(config?: AppConfig): Promise<SafeEnvVarEntry[]> {
  const cfg = config ?? await getConfig()
  const fileValues = await readEnvFileValues()
  const known = new Set(ENV_VAR_CATALOG.map((entry) => entry.name))
  const rows: SafeEnvVarEntry[] = ENV_VAR_CATALOG.map((catalog) => {
    const fileValue = fileValues.get(catalog.name)
    const managed = cfg.envVars.find((entry) => entry.name === catalog.name)
    const inherited = typeof process.env[catalog.name] === "string" && process.env[catalog.name] !== ""
    const value = fileValue?.value || managed?.value || (inherited ? process.env[catalog.name]! : "")
    const file = fileValue?.file ?? catalog.file
    return {
      name: catalog.name,
      preview: redactValue(value),
      note: managed?.note ?? "",
      updatedAt: managed?.updatedAt ?? 0,
      hasValue: value.length > 0,
      source: fileValue?.value || managed?.value ? "managed" : inherited ? "process" : "missing",
      requiredBy: catalog.requiredBy,
      description: `${catalog.description} Stored in ${_envFiles[catalog.file].label}; SOPS env sync can encrypt it for GitHub.`,
      file,
      filePath: _envFiles[file].path,
    }
  })
  for (const [name, fileValue] of fileValues) {
    if (known.has(name)) continue
    rows.push({
      name,
      preview: redactValue(fileValue.value),
      note: "",
      updatedAt: 0,
      hasValue: fileValue.value.length > 0,
      source: "managed",
      requiredBy: `OpenCode ${_envFiles[fileValue.file].label}`,
      description: `Stored in ${fileValue.path}; SOPS env sync can encrypt sensitive env files for GitHub.`,
      file: fileValue.file,
      filePath: fileValue.path,
    })
    known.add(name)
  }
  for (const entry of cfg.envVars) {
    if (known.has(entry.name)) continue
    const file: EnvFileKind = "secrets"
    rows.push({
      name: entry.name,
      preview: redactValue(entry.value),
      note: entry.note,
      updatedAt: entry.updatedAt,
      hasValue: entry.value.length > 0,
      source: entry.value ? "managed" : "missing",
      requiredBy: "Custom",
      description: "User-managed dashboard environment variable.",
      file,
      filePath: _envFiles[file].path,
    })
  }
  return rows
}

/**
 * Return variables grouped by env file, with file metadata.
 * Each group is ordered: config → internal → secrets.
 */
export async function safeEnvVarsByFile(config?: AppConfig): Promise<EnvFileGroup[]> {
  const vars = await safeEnvVars(config)
  const fileOrder: EnvFileKind[] = ["config", "internal", "secrets"]
  return fileOrder.map((file) => {
    const meta = _envFiles[file]
    return {
      file,
      label: meta.label,
      path: meta.path,
      sensitive: meta.sensitive,
      variables: vars.filter((v) => v.file === file).sort((a, b) => a.name.localeCompare(b.name)),
    }
  })
}

/** Return metadata for all env files (for UI display). */
export function getEnvFileMeta(): { file: EnvFileKind; label: string; path: string; sensitive: boolean }[] {
  return (Object.keys(_envFiles) as EnvFileKind[]).map((file) => ({
    file,
    ..._envFiles[file],
  }))
}

/** Save or replace a variable in the selected OpenCode env file. */
export async function upsertEnvVar(
  name: string,
  value: string,
  file: EnvFileKind = "secrets",
): Promise<void> {
  const key = normalizeEnvName(name)
  if (!key) throw new Error("Invalid variable name")
  if (!value) throw new Error("Missing value")
  await writeEnvFileValue(file, key, value)
}

/** Remove a variable from every OpenCode env file. */
export async function deleteEnvVar(name: string): Promise<void> {
  const key = normalizeEnvName(name)
  if (!key) throw new Error("Invalid variable name")
  for (const file of Object.keys(_envFiles) as EnvFileKind[]) {
    await writeEnvFileValue(file, key, null)
  }
}

/**
 * Build the environment used for dashboard-launched local processes.
 * Dashboard-managed variables intentionally override inherited env vars,
 * which makes copied cookies/tokens take effect without shell exports.
 */
export async function buildManagedEnv(extra: ManagedEnv = {}): Promise<ManagedEnv> {
  const cfg = await getConfig()
  const env: ManagedEnv = { ...(process.env as ManagedEnv) }
  for (const [name, entry] of await readEnvFileValues()) {
    if (entry.value) env[name] = entry.value
  }
  for (const entry of cfg.envVars) {
    if (entry.value) env[entry.name] = entry.value
  }
  return { ...env, ...extra }
}

async function readEnvFileValues(): Promise<Map<string, EnvFileValue>> {
  const values = new Map<string, EnvFileValue>()
  for (const file of Object.keys(_envFiles) as EnvFileKind[]) {
    const meta = _envFiles[file]
    const rows = await readEnvLines(meta.path)
    for (const row of rows) {
      if (!row.key) continue
      values.set(row.key, { value: row.value, file, path: meta.path })
    }
  }
  return values
}

async function writeEnvFileValue(file: EnvFileKind, key: string, value: string | null): Promise<void> {
  const meta = _envFiles[file]
  const rows = await readEnvLines(meta.path)
  let replaced = false
  const next: string[] = []
  for (const row of rows) {
    if (row.key === key) {
      replaced = true
      if (value !== null) next.push(formatEnvLine(key, value))
      continue
    }
    next.push(row.raw)
  }
  if (!replaced && value !== null) next.push(formatEnvLine(key, value))
  const dir = dirname(meta.path)
  if (!existsSync(dir)) await mkdir(dir, { recursive: true })
  await writeFile(meta.path, next.join("\n") + (next.length > 0 ? "\n" : ""), "utf-8")
  if (meta.sensitive) await chmod(meta.path, 0o600)
}

async function readEnvLines(path: string): Promise<{ raw: string; key: string; value: string }[]> {
  let raw = ""
  try {
    raw = await readFile(path, "utf-8")
  } catch {
    return []
  }
  return raw.split(/\r?\n/).filter((line, idx, arr) => idx < arr.length - 1 || line.length > 0).map((line) => {
    const match = line.match(/^\s*(?:export\s+)?([A-Z_][A-Z0-9_]*)=(.*)$/)
    if (!match || line.trimStart().startsWith("#")) return { raw: line, key: "", value: "" }
    return { raw: line, key: match[1], value: parseEnvValue(match[2]) }
  })
}

function parseEnvValue(raw: string): string {
  const trimmed = raw.trim()
  if ((trimmed.startsWith('"') && trimmed.endsWith('"')) || (trimmed.startsWith("'") && trimmed.endsWith("'"))) {
    return trimmed.slice(1, -1)
  }
  const hash = trimmed.indexOf(" #")
  return (hash >= 0 ? trimmed.slice(0, hash) : trimmed).trim()
}

function formatEnvLine(key: string, value: string): string {
  return `${key}=${JSON.stringify(value)}`
}

function normalizeEnvVars(value: unknown): EnvVarEntry[] {
  if (!Array.isArray(value)) return []
  const now = Date.now()
  const byName = new Map<string, EnvVarEntry>()
  for (const item of value) {
    if (!item || typeof item !== "object") continue
    const raw = item as Record<string, unknown>
    const name = typeof raw.name === "string" ? normalizeEnvName(raw.name) : ""
    if (!name) continue
    const entry: EnvVarEntry = {
      name,
      value: typeof raw.value === "string" ? raw.value : "",
      note: typeof raw.note === "string" ? raw.note.trim().slice(0, 300) : "",
      updatedAt: typeof raw.updatedAt === "number" && Number.isFinite(raw.updatedAt) ? raw.updatedAt : now,
    }
    byName.set(name, entry)
  }
  return [...byName.values()].sort((a, b) => a.name.localeCompare(b.name))
}

function normalizeEnvName(name: string): string {
  const normalized = name.trim().toUpperCase()
  return /^[A-Z_][A-Z0-9_]{0,79}$/.test(normalized) ? normalized : ""
}

function redactValue(value: string): string {
  if (!value) return "(empty)"
  if (value.length <= 8) return "****"
  return `${value.slice(0, 4)}****${value.slice(-4)}`
}

export function _resetForTest(path: string): void {
  _path = path
  _cache = null
  _envFiles = defaultEnvFiles(dirname(path))
}
