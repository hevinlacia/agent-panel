/**
 * Role: safe read/write gateway for Pi agent configuration files.
 * Public surface: readPiConfigSummary/updatePiSettings/getPiConfigFile/savePiConfigFile.
 * Constraints: only whitelisted files under ~/.pi/agent are editable; model secrets are redacted and restored server-side.
 * Read-this-with: src/server.tsx API routes and web/src/App.tsx SettingsPage.
 */
import { chmod, mkdir, readFile, stat, writeFile } from "node:fs/promises"
import { existsSync } from "node:fs"
import { homedir } from "node:os"
import { dirname, join } from "node:path"

export type PiConfigFileKey = "settings" | "models" | "agents"

export interface PiModelOption {
  providerId: string
  modelId: string
  label: string
  name?: string
  contextWindow?: number
  maxTokens?: number
  reasoning?: boolean
  thinkingLevels: string[]
}

export interface PiProviderSummary {
  id: string
  api?: string
  baseUrl?: string
  modelCount: number
  hasApiKey: boolean
  models: PiModelOption[]
}

export interface PiSettingsSummary {
  path: string
  exists: boolean
  defaultProvider: string
  defaultModel: string
  defaultThinkingLevel: string
  enabledModels: string[]
  theme: string
}

export interface PiConfigFileMeta {
  file: PiConfigFileKey
  label: string
  path: string
  sensitive: boolean
  description: string
}

export interface PiConfigFileSnapshot extends PiConfigFileMeta {
  content: string
  updatedAt: number | null
}

export interface PiConfigSummary {
  settings: PiSettingsSummary
  providers: PiProviderSummary[]
  files: PiConfigFileMeta[]
  thinkingLevels: string[]
}

export interface PiSettingsUpdate {
  defaultProvider?: string
  defaultModel?: string
  defaultThinkingLevel?: string
  enabledModels?: string[]
  theme?: string
}

const THINKING_LEVELS = ["off", "minimal", "low", "medium", "high", "xhigh", "max"] as const
const SECRET_PLACEHOLDER = "__AGENT_PANEL_SECRET__"
let _piConfigDir = join(homedir(), ".pi", "agent")

function fileMeta(): Record<PiConfigFileKey, PiConfigFileMeta> {
  return {
    settings: {
      file: "settings",
      label: "settings.json",
      path: join(_piConfigDir, "settings.json"),
      sensitive: false,
      description: "Pi UI defaults: provider, model, thinking level, theme, and enabled model patterns.",
    },
    models: {
      file: "models",
      label: "models.json",
      path: join(_piConfigDir, "models.json"),
      sensitive: true,
      description: "Custom Pi providers and models. Secret fields are shown as placeholders and restored on save.",
    },
    agents: {
      file: "agents",
      label: "agents.json",
      path: join(_piConfigDir, "agents.json"),
      sensitive: false,
      description: "Agent profile bindings: default provider/model/tools/thinking per named agent.",
    },
  }
}

/** Validate file selectors before any filesystem access. */
export function isPiConfigFileKey(value: unknown): value is PiConfigFileKey {
  return value === "settings" || value === "models" || value === "agents"
}

/** Return UI-ready Pi model/settings metadata without exposing secret values. */
export async function readPiConfigSummary(): Promise<PiConfigSummary> {
  const metas = fileMeta()
  const [settingsObj, modelsObj] = await Promise.all([
    readJsonObject(metas.settings.path),
    readJsonObject(metas.models.path),
  ])
  return {
    settings: {
      path: metas.settings.path,
      exists: existsSync(metas.settings.path),
      defaultProvider: stringValue(settingsObj.defaultProvider),
      defaultModel: stringValue(settingsObj.defaultModel),
      defaultThinkingLevel: stringValue(settingsObj.defaultThinkingLevel),
      enabledModels: stringArray(settingsObj.enabledModels),
      theme: stringValue(settingsObj.theme),
    },
    providers: summarizeProviders(modelsObj),
    files: Object.values(metas),
    thinkingLevels: [...THINKING_LEVELS],
  }
}

/** Merge editable Pi defaults into settings.json while preserving unknown keys. */
export async function updatePiSettings(update: PiSettingsUpdate): Promise<PiConfigSummary> {
  const settingsPath = fileMeta().settings.path
  const current = await readJsonObject(settingsPath)
  const next = { ...current }
  setOptionalString(next, "defaultProvider", update.defaultProvider, 120)
  setOptionalString(next, "defaultModel", update.defaultModel, 200)
  setOptionalString(next, "theme", update.theme, 120)
  if (typeof update.defaultThinkingLevel === "string") {
    const level = update.defaultThinkingLevel.trim()
    if ((THINKING_LEVELS as readonly string[]).includes(level)) next.defaultThinkingLevel = level
  }
  if (Array.isArray(update.enabledModels)) {
    next.enabledModels = [...new Set(update.enabledModels
      .map((v) => typeof v === "string" ? v.trim() : "")
      .filter((v) => v && !v.includes("\0") && v.length <= 200))]
      .slice(0, 200)
  }
  await writeJsonObject(settingsPath, next)
  return readPiConfigSummary()
}

/** Read a whitelisted Pi config file for the browser editor. */
export async function getPiConfigFile(file: PiConfigFileKey): Promise<PiConfigFileSnapshot> {
  const meta = fileMeta()[file]
  const raw = await readFile(meta.path, "utf-8").catch(() => defaultContent(file))
  const content = file === "models"
    ? JSON.stringify(redactSecrets(JSON.parse(raw || "{}")), null, 2) + "\n"
    : normalizeJsonContent(raw, file)
  const info = await stat(meta.path).catch(() => null)
  return { ...meta, content, updatedAt: info?.mtimeMs ?? null }
}

/** Persist a whitelisted Pi config file, restoring redacted model secrets. */
export async function savePiConfigFile(file: PiConfigFileKey, content: string): Promise<PiConfigFileSnapshot> {
  if (typeof content !== "string" || content.length > 1024 * 1024) throw new Error("Config content is too large")
  if (content.includes("\0")) throw new Error("Config content contains NUL bytes")
  const meta = fileMeta()[file]
  const parsed = parseJsonObject(content, meta.label)
  const original = file === "models" ? await readJsonObject(meta.path) : {}
  const next = file === "models" ? restoreSecrets(parsed, original) as Record<string, unknown> : parsed
  await writeJsonObject(meta.path, next)
  return getPiConfigFile(file)
}

function summarizeProviders(modelsObj: Record<string, unknown>): PiProviderSummary[] {
  const providers = asRecord(modelsObj.providers)
  return Object.entries(providers).map(([id, raw]) => {
    const provider = asRecord(raw)
    const models = Array.isArray(provider.models) ? provider.models : []
    const overrides = asRecord(provider.modelOverrides)
    const modelOptions: PiModelOption[] = []
    for (const modelRaw of models) {
      const model = asRecord(modelRaw)
      const modelId = stringValue(model.id)
      if (!modelId) continue
      modelOptions.push({
        providerId: id,
        modelId,
        label: `${id}/${modelId}`,
        name: stringValue(model.name),
        contextWindow: numberValue(model.contextWindow),
        maxTokens: numberValue(model.maxTokens),
        reasoning: typeof model.reasoning === "boolean" ? model.reasoning : undefined,
        thinkingLevels: thinkingLevelsFromMap(model.thinkingLevelMap),
      })
    }
    for (const modelId of Object.keys(overrides)) {
      if (modelOptions.some((m) => m.modelId === modelId)) continue
      modelOptions.push({ providerId: id, modelId, label: `${id}/${modelId}`, thinkingLevels: [] })
    }
    return {
      id,
      api: stringValue(provider.api),
      baseUrl: stringValue(provider.baseUrl),
      modelCount: modelOptions.length,
      hasApiKey: Boolean(provider.apiKey),
      models: modelOptions.sort((a, b) => a.modelId.localeCompare(b.modelId)),
    }
  }).sort((a, b) => a.id.localeCompare(b.id))
}

function thinkingLevelsFromMap(value: unknown): string[] {
  const map = asRecord(value)
  return THINKING_LEVELS.filter((level) => Object.hasOwn(map, level) && map[level] !== null)
}

async function readJsonObject(filePath: string): Promise<Record<string, unknown>> {
  const raw = await readFile(filePath, "utf-8").catch(() => "{}")
  return parseJsonObject(raw || "{}", filePath)
}

async function writeJsonObject(filePath: string, value: Record<string, unknown>): Promise<void> {
  await mkdir(dirname(filePath), { recursive: true })
  await writeFile(filePath, JSON.stringify(value, null, 2) + "\n", "utf-8")
  if (filePath.endsWith("models.json")) await chmod(filePath, 0o600).catch(() => undefined)
}

function parseJsonObject(raw: string, label: string): Record<string, unknown> {
  let parsed: unknown
  try {
    parsed = JSON.parse(raw || "{}")
  } catch (error) {
    throw new Error(`${label} is not valid JSON: ${(error as Error).message}`)
  }
  if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) throw new Error(`${label} must be a JSON object`)
  return parsed as Record<string, unknown>
}

function normalizeJsonContent(raw: string, file: PiConfigFileKey): string {
  const parsed = parseJsonObject(raw || defaultContent(file), fileMeta()[file].label)
  return JSON.stringify(parsed, null, 2) + "\n"
}

function defaultContent(file: PiConfigFileKey): string {
  return file === "settings" ? "{}\n" : file === "models" ? "{\n  \"providers\": {}\n}\n" : "{\n  \"agents\": {}\n}\n"
}

function redactSecrets(value: unknown): unknown {
  if (Array.isArray(value)) return value.map(redactSecrets)
  if (!value || typeof value !== "object") return value
  const out: Record<string, unknown> = {}
  for (const [key, child] of Object.entries(value)) {
    out[key] = isSecretKey(key) && typeof child === "string" && child
      ? SECRET_PLACEHOLDER
      : redactSecrets(child)
  }
  return out
}

function restoreSecrets(next: unknown, original: unknown, key = ""): unknown {
  if (isSecretKey(key) && next === SECRET_PLACEHOLDER) return original ?? ""
  if (Array.isArray(next)) {
    const originalArray = Array.isArray(original) ? original : []
    return next.map((item, index) => restoreSecrets(item, originalArray[index]))
  }
  if (!next || typeof next !== "object") return next
  const originalObj = asRecord(original)
  const out: Record<string, unknown> = {}
  for (const [childKey, child] of Object.entries(next)) {
    out[childKey] = restoreSecrets(child, originalObj[childKey], childKey)
  }
  return out
}

function isSecretKey(key: string): boolean {
  return /^(apiKey|api_key|token|secret|password|authorization|cookie)$/i.test(key)
}

function asRecord(value: unknown): Record<string, unknown> {
  return value && typeof value === "object" && !Array.isArray(value) ? value as Record<string, unknown> : {}
}

function stringValue(value: unknown): string {
  return typeof value === "string" ? value : ""
}

function stringArray(value: unknown): string[] {
  return Array.isArray(value) ? value.filter((v): v is string => typeof v === "string") : []
}

function numberValue(value: unknown): number | undefined {
  return typeof value === "number" && Number.isFinite(value) ? value : undefined
}

function setOptionalString(target: Record<string, unknown>, key: string, value: unknown, maxLength: number): void {
  if (typeof value !== "string") return
  const trimmed = value.trim()
  if (!trimmed || trimmed.includes("\0") || trimmed.length > maxLength) return
  target[key] = trimmed
}

/** Test-only override so unit tests never touch real ~/.pi files. */
export function _resetForTest(dir: string): void {
  _piConfigDir = dir
}
