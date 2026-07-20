/**
 * Role: read-only health checks for git-ai CLI/hooks and the Pi git-ai extension.
 * Public surface: readGitAiHealth consumed by src/server.tsx /api/git-ai/health.
 * Constraints: no secrets, no repo writes, no Pi session startup; checks are file
 *   existence, fixed git/git-ai commands, and extension auto-discovery paths.
 * Read-this-with: src/gitAiSuspects.ts and web/src/App.tsx GitAiPage.
 */

import { access, readFile, stat } from "node:fs/promises"
import { existsSync } from "node:fs"
import { homedir } from "node:os"
import { join } from "node:path"
import { spawn } from "node:child_process"

export type HealthTone = "ok" | "warn" | "error" | "unknown"

export interface GitAiCliHealth {
  binaryPath: string | null
  installed: boolean
  version: string | null
  daemonOk: boolean
  daemonMessage: string | null
  trace2Target: string | null
  trace2Socket: string | null
  trace2SocketExists: boolean
  hooksPath: string | null
  postCommitHook: HookHealth
  prePushHook: HookHealth
}

export interface HookHealth {
  path: string | null
  exists: boolean
  mode: string
  recordsToAgentPanel: boolean
  executable: boolean
}

export interface PiGitAiExtensionHealth {
  globalPath: string
  sourcePath: string
  globalExists: boolean
  sourceExists: boolean
  sourceMatchesGlobal: boolean
  autoDiscoveryPath: boolean
  gitAiBinaryExistsForExtension: boolean
  registersStatus: boolean
  tracksTools: string[]
  status: HealthTone
  message: string
}

export interface GitAiHealthPayload {
  generatedAt: number
  storePath: string
  cli: GitAiCliHealth
  piExtension: PiGitAiExtensionHealth
}

const DEFAULT_GIT_AI_BIN = join(homedir(), ".git-ai", "bin", "git-ai")
const DEFAULT_STORE_PATH = join(homedir(), ".local", "share", "agent-panel", "git-ai-suspects.json")
const PI_GLOBAL_EXTENSION_PATH = join(homedir(), ".pi", "agent", "extensions", "git-ai.ts")
const PI_SOURCE_EXTENSION_PATH = join(homedir(), "Developer", "infra", "ai-code-config", "core", "pi", "agent", "extensions", "git-ai.ts")

function run(cmd: string, args: string[], timeoutMs = 4_000): Promise<{ code: number | null; stdout: string; stderr: string }> {
  return new Promise((resolve) => {
    const child = spawn(cmd, args, { stdio: ["ignore", "pipe", "pipe"] })
    let stdout = ""
    let stderr = ""
    const timer = setTimeout(() => {
      child.kill("SIGTERM")
      resolve({ code: null, stdout, stderr: stderr || "timeout" })
    }, timeoutMs)
    child.stdout.on("data", (chunk) => { stdout += chunk.toString() })
    child.stderr.on("data", (chunk) => { stderr += chunk.toString() })
    child.on("error", (err) => {
      clearTimeout(timer)
      resolve({ code: null, stdout, stderr: err.message })
    })
    child.on("close", (code) => {
      clearTimeout(timer)
      resolve({ code, stdout, stderr })
    })
  })
}

async function executable(path: string | null): Promise<boolean> {
  if (!path) return false
  try {
    await access(path, 0o1)
    return true
  } catch {
    return false
  }
}

async function readText(path: string | null): Promise<string> {
  if (!path) return ""
  return readFile(path, "utf-8").catch(() => "")
}

async function findGitAiBinary(): Promise<string | null> {
  const envPath = process.env.GIT_AI_BIN
  if (envPath && existsSync(envPath)) return envPath
  const which = await run("bash", ["-lc", "command -v git-ai"], 2_000)
  const found = which.stdout.trim()
  if (which.code === 0 && found && existsSync(found)) return found
  if (existsSync(DEFAULT_GIT_AI_BIN)) return DEFAULT_GIT_AI_BIN
  return null
}

function parseTrace2Socket(target: string | null): string | null {
  if (!target) return null
  const marker = "af_unix:stream:"
  if (!target.includes(marker)) return target
  return target.slice(target.indexOf(marker) + marker.length).trim() || null
}

async function hookHealth(path: string | null, kind: "post-commit" | "pre-push"): Promise<HookHealth> {
  const text = await readText(path)
  const exists = Boolean(path && existsSync(path))
  const recordsToAgentPanel = text.includes("record_git_ai_suspect") && text.includes("AGENT_PANEL_STORE")
  let mode = "missing"
  if (exists && recordsToAgentPanel) mode = "record"
  if (kind === "pre-push" && /GIT_AI_PUSH_MODE[^\n]+block/.test(text) && !/GIT_AI_PUSH_MODE[^\n]+record/.test(text)) mode = "block"
  if (kind === "post-commit" && text.includes("NO_BLOCK") && !text.includes("GIT_AI_BLOCK")) mode = "block"
  return { path, exists, mode, recordsToAgentPanel, executable: await executable(path) }
}

async function fileEquals(a: string, b: string): Promise<boolean> {
  const [left, right] = await Promise.all([readText(a), readText(b)])
  return Boolean(left && right && left === right)
}

async function readCliHealth(): Promise<GitAiCliHealth> {
  const binaryPath = await findGitAiBinary()
  const version = binaryPath ? (await run(binaryPath, ["--version"], 3_000)).stdout.trim() || null : null
  const bg = binaryPath ? await run(binaryPath, ["bg", "status"], 4_000) : { code: null, stdout: "", stderr: "git-ai binary missing" }
  let daemonOk = false
  let daemonMessage: string | null = null
  try {
    const parsed = JSON.parse(bg.stdout || "{}")
    daemonOk = Boolean(parsed.ok) && !parsed.data?.last_error
    daemonMessage = parsed.data?.last_error || (daemonOk ? "running" : "not running")
  } catch {
    daemonMessage = bg.stderr || bg.stdout || "unknown"
  }
  const trace = await run("git", ["config", "--global", "trace2.eventtarget"], 2_000)
  const trace2Target = trace.code === 0 ? trace.stdout.trim() || null : null
  const trace2Socket = parseTrace2Socket(trace2Target)
  const hooks = await run("git", ["config", "--global", "core.hooksPath"], 2_000)
  const hooksPath = hooks.code === 0 ? hooks.stdout.trim() || null : null
  return {
    binaryPath,
    installed: Boolean(binaryPath && version),
    version,
    daemonOk,
    daemonMessage,
    trace2Target,
    trace2Socket,
    trace2SocketExists: Boolean(trace2Socket && existsSync(trace2Socket)),
    hooksPath,
    postCommitHook: await hookHealth(hooksPath ? join(hooksPath, "post-commit") : null, "post-commit"),
    prePushHook: await hookHealth(hooksPath ? join(hooksPath, "pre-push") : null, "pre-push"),
  }
}

function trackedTools(text: string): string[] {
  const out: string[] = []
  if (/EDIT_TOOLS[^\n]+edit/.test(text) || text.includes('new Set(["edit", "write"])')) out.push("edit")
  if (/EDIT_TOOLS[^\n]+write/.test(text) || text.includes('new Set(["edit", "write"])')) out.push("write")
  if (text.includes('tool === "bash"')) out.push("bash")
  return [...new Set(out)]
}

async function readPiExtensionHealth(): Promise<PiGitAiExtensionHealth> {
  const text = await readText(PI_GLOBAL_EXTENSION_PATH)
  const globalExists = existsSync(PI_GLOBAL_EXTENSION_PATH)
  const sourceExists = existsSync(PI_SOURCE_EXTENSION_PATH)
  const gitAiBinary = text.match(/const GIT_AI_BIN = process\.env\.GIT_AI_BIN \|\| "([^"]+)"/)?.[1] || DEFAULT_GIT_AI_BIN
  const gitAiBinaryExistsForExtension = existsSync(process.env.GIT_AI_BIN || gitAiBinary)
  const sourceMatchesGlobal = sourceExists && globalExists ? await fileEquals(PI_GLOBAL_EXTENSION_PATH, PI_SOURCE_EXTENSION_PATH) : false
  const autoDiscoveryPath = PI_GLOBAL_EXTENSION_PATH.endsWith("/.pi/agent/extensions/git-ai.ts")
  const registersStatus = text.includes('ctx.ui.setStatus("git-ai"')
  const tools = trackedTools(text)
  let status: HealthTone = "ok"
  const problems: string[] = []
  if (!globalExists) problems.push("global extension missing")
  if (!gitAiBinaryExistsForExtension) problems.push("git-ai binary missing for extension")
  if (!registersStatus) problems.push("no git-ai UI status registration")
  if (tools.length === 0) problems.push("no tracked tools detected")
  if (!sourceMatchesGlobal) problems.push("runtime extension differs from config source")
  if (problems.length > 0) status = problems.some((p) => p.includes("missing")) ? "error" : "warn"
  return {
    globalPath: PI_GLOBAL_EXTENSION_PATH,
    sourcePath: PI_SOURCE_EXTENSION_PATH,
    globalExists,
    sourceExists,
    sourceMatchesGlobal,
    autoDiscoveryPath,
    gitAiBinaryExistsForExtension,
    registersStatus,
    tracksTools: tools,
    status,
    message: problems.length ? problems.join("; ") : "Pi auto-discovery path is configured and git-ai extension looks ready",
  }
}

/** Build the /git-ai page health payload without reading secrets or starting Pi. */
export async function readGitAiHealth(): Promise<GitAiHealthPayload> {
  const [cli, piExtension] = await Promise.all([readCliHealth(), readPiExtensionHealth()])
  return {
    generatedAt: Date.now(),
    storePath: process.env.AGENT_PANEL_GIT_AI_STORE || DEFAULT_STORE_PATH,
    cli,
    piExtension,
  }
}
