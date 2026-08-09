/**
 * Role: React SPA for the Rust Agent Panel rewrite.
 * Public surface: App component mounted by web/src/main.tsx.
 * Constraints: browser-only UI; no PTY/xterm and no OpenCode report flows.
 * Read-this-with: src/main.rs for the JSON API contract and web/src/styles.css.
 */
import { AnimatePresence, motion } from "framer-motion"
import {
  Activity,
  AlertTriangle,
  ArrowLeft,
  CheckCircle2,
  Clock3,
  FileCode2,
  Gauge,
  GitBranch,
  GitMerge,
  LayoutDashboard,
  Library,
  Lightbulb,
  ListChecks,
  RefreshCw,
  Search,
  Server,
  Settings,
  Sparkles,
} from "lucide-react"
import { useEffect, useMemo, useState } from "react"
import type { DashboardStatsPayload, RequirementDuration, StatusCount } from "./types"

interface AppProps { apiPath: string }

type ReqStatus = "需求澄清" | "开发中" | "自测中" | "测试中" | "经验总结" | "已完成"
type ReqCategory = "需求" | "线上问题"

const REQ_STATUSES: ReqStatus[] = ["需求澄清", "开发中", "自测中", "测试中", "经验总结", "已完成"]
const REQ_CATEGORIES: ReqCategory[] = ["需求", "线上问题"]

const statusMeta: Record<string, { color: string; soft: string }> = {
  需求澄清: { color: "#94a3b8", soft: "rgba(148, 163, 184, 0.14)" },
  开发中: { color: "#22d3ee", soft: "rgba(34, 211, 238, 0.14)" },
  自测中: { color: "#3b82f6", soft: "rgba(59, 130, 246, 0.14)" },
  测试中: { color: "#a855f7", soft: "rgba(168, 85, 247, 0.14)" },
  经验总结: { color: "#eab308", soft: "rgba(234, 179, 8, 0.14)" },
  已完成: { color: "#22c55e", soft: "rgba(34, 197, 94, 0.14)" },
  需求对齐: { color: "#94a3b8", soft: "rgba(148, 163, 184, 0.14)" },
  方案设计: { color: "#94a3b8", soft: "rgba(148, 163, 184, 0.14)" },
  待上线: { color: "#eab308", soft: "rgba(234, 179, 8, 0.14)" },
}

interface EffortEstimate {
  coefficient: number
  baseHours: number
  estimatedHours: number
  summary?: string
  updatedAt?: number
}

interface Requirement {
  id: string
  title: string
  description?: string
  status: ReqStatus
  category?: ReqCategory
  project: string
  projects?: string[]
  groupPath?: string[]
  createdAt: number
  updatedAt: number
  completedAt?: number
  sessionIds: string[]
  reqDir?: string
  metaPath?: string
  alignmentPath?: string
  backgroundPath?: string
  memoryPath?: string
  branchPath?: string
  testPath?: string
  notesPath?: string
  configPath?: string
  impactPath?: string
  reviewPath?: string
  releaseManifestPath?: string
  releaseCheckPath?: string
  experienceSummaryPath?: string
  prdPath?: string
  ones?: string
  effortEstimate?: EffortEstimate
}

interface SessionInfo {
  id: string
  title: string
  status: "running" | "idle" | "stale" | string
  agent?: string
  model?: string
  provider?: string
  modelId?: string
  modelProvider?: string
  directory?: string
  worktree?: string
  path?: string
  updated?: number
  created?: number
  messageCount?: number
  userMessageCount?: number
  assistantMessageCount?: number
  toolCallCount?: number
  tokensInput?: number
  tokensOutput?: number
  cost?: number
}

interface RequirementDocPayload { ok: boolean; reqId: string; docType: string; file: string; path: string; exists: boolean; content: string }
interface ApiSessions { summary: Record<string, number>; sessions: SessionInfo[]; harness?: string; days?: number }
interface ConfigPayload {
  requirementScanRoots?: string[]
  fullSyncSchedule?: boolean
  fullSyncTimes?: string[]
  fullSyncGithubRepos?: string[]
  codeReviewPiModel?: string
  branchScopePiModel?: string
  effortEstimatePiModel?: string
  effortEstimateBaseHours?: number
  cainiaoMockEnabled?: boolean
  cainiaoMockPort?: number
}

interface CainiaoMockStatus { enabled: boolean; running: boolean; port: number }

type KnowledgeKind = "businessKnowledge" | "experience"
interface KnowledgeItem {
  id: string
  title: string
  kind: KnowledgeKind | string
  type?: string
  domain?: string
  project?: string
  scope?: string
  status?: string
  confidence?: string
  tags?: string[]
  triggerTerms?: string[]
  relatedSkills?: string[]
  relatedRepos?: string[]
  relatedTables?: string[]
  relatedApis?: string[]
  source?: string
  createdAt?: string
  updatedAt?: string
  lastVerifiedAt?: string
  validUntil?: string
  summary?: string
  details?: string | null
  detailsTruncated?: boolean
  path?: string
  score?: number
  whyMatched?: string[]
}
interface KnowledgeListPayload { items: KnowledgeItem[]; generatedAt?: number }
interface KnowledgeSavePayload { ok: boolean; item: KnowledgeItem }
type KnowledgeDraft = Partial<KnowledgeItem> & { details?: string; root?: string }
interface PiConfigFileSnapshot { file: string; label: string; path: string; sensitive: boolean; description: string; content: string; updatedAt: number | null }
interface PiModelOption { providerId: string; modelId: string; label: string; name?: string; contextWindow?: number | null; reasoning?: boolean; thinkingLevels: string[] }
interface PiProviderSummary { id: string; api?: string; baseUrl?: string; modelCount: number; hasApiKey: boolean; models: PiModelOption[] }
interface PiConfigSummary { settings: { path: string; exists: boolean; defaultProvider: string; defaultModel: string; defaultThinkingLevel: string; enabledModels: string[]; theme: string }; providers: PiProviderSummary[]; thinkingLevels: string[] }
interface GitAiSuspectStats { total: number; pending: number; confirmedAi: number; missingAi: number; notFound: number; checkFailed: number }
type GitAiCompanyStatus = "pending" | "confirmed_ai" | "missing_ai" | "not_found" | "check_failed"
interface GitAiSuspectRecord { id: string; projectName: string; commitSha: string; shortSha: string; repoPath?: string | null; remoteUrl?: string | null; subject?: string | null; branch?: string | null; eventSources?: string[]; localNoteState?: string; companyStatus: GitAiCompanyStatus; companyCheckedAt?: number | null; companyError?: string | null; commitWebUrl?: string | null; commitTitle?: string | null; aiRate?: number | null; aiLines?: number | null; humanLines?: number | null; authorName?: string | null; lastSeenAt: number }
interface GitAiSuspectsPayload { records: GitAiSuspectRecord[]; stats: GitAiSuspectStats; generatedAt: number }
interface GitAiFixStep { label: string; command: string; ok: boolean; stdout?: string; stderr?: string }
interface GitAiFixResponse {
  ok: boolean
  stillMissing: boolean
  recheck?: Record<string, unknown> & { companyStatus?: GitAiCompanyStatus; companyError?: string | null }
  pushSteps?: GitAiFixStep[]
  piAgent?: { dispatched: boolean; sessionId?: string; skillPath?: string; message: string }
}
interface GitAiHookHealth { path: string | null; exists: boolean; mode: string; recordsToAgentPanel: boolean; executable: boolean }
interface GitAiHealthPayload {
  generatedAt: number
  storePath: string
  cli: {
    binaryPath: string | null
    installed: boolean
    version: string | null
    daemonOk: boolean
    daemonMessage: string | null
    trace2Target: string | null
    trace2Socket: string | null
    trace2SocketExists: boolean
    hooksPath: string | null
    postCommitHook: GitAiHookHealth
    prePushHook: GitAiHookHealth
  }
  piExtension: {
    globalPath: string
    sourcePath: string
    globalExists: boolean
    sourceExists: boolean
    sourceMatchesGlobal: boolean
    autoDiscoveryPath: boolean
    gitAiBinaryExistsForExtension: boolean
    registersStatus: boolean
    tracksTools: string[]
    status: "ok" | "warn" | "error" | "unknown"
    message: string
  }
}
interface AutoDrivePayload { jobs: unknown[]; active: number; blocked: number; queue: { active: number; queued: number }; message?: string }
interface BranchRepo { repoName: string; branches: string[]; role?: string; path?: string; baseRef?: string; testTargetBranch?: string; uatTargetBranch?: string }
interface BranchScope { version: number; updatedAt: number; repos: BranchRepo[]; fallback?: boolean }
interface CodeReviewFile { path: string; status: string; additions: number; deletions: number; riskTags?: string[] }
interface CodeReviewRepoSnapshot {
  repoName: string
  projectPath?: string
  branch: string
  resolvedTargetRef?: string
  baseRef: string
  currentBranch?: string
  dirty?: boolean
  commits?: string[]
  files: CodeReviewFile[]
  additions: number
  deletions: number
  diff?: string
  diffTruncated?: boolean
  warnings?: string[]
  error?: string | null
}
interface CodeReviewSnapshot { version: number; reqId: string; updatedAt: number; baseRef: string; frontendBaseRef?: string; backendBaseRef?: string; sourceFallback?: boolean; repos: CodeReviewRepoSnapshot[] }
interface CodeReviewPayload { ok: boolean; branchScope?: BranchScope | null; review?: CodeReviewSnapshot | null }
interface ReviewGatePayload { ok: boolean; reqId: string; gate: { status: string; label: string; allowsTesting: boolean; reason: string; source?: string | null; reviewPath: string; aiReviewPath: string; checkedAt: number; actions: string[] } }
interface MasterDiffPayload { ok: boolean; branchScope?: BranchScope | null; review?: CodeReviewSnapshot | null }
interface SyncBaseResult { repoName: string; ok: boolean; status: string; baseRef?: string; remoteRef?: string; localBranch?: string; currentBranch?: string; beforeCommit?: string; afterCommit?: string; message: string; warnings?: string[] }
interface SyncBasePayload { ok: boolean; generatedAt: number; results: SyncBaseResult[] }
interface ProdMrResult { repoName: string; role?: string | null; projectPath?: string | null; sourceBranch: string; targetBranch: string; status: "created" | "reused" | "failed" | "skipped" | "no_diff" | string; iid?: number | null; webUrl?: string | null; title?: string | null; error?: string | null; diffFiles?: number | null; diffAdditions?: number | null; diffDeletions?: number | null }
interface ProdMrPayload { ok: boolean; reqId: string; generatedAt: number; branchScope?: BranchScope | null; results: ProdMrResult[] }
type MergeTarget = "test" | "uat"
type MergeRepoKind = "frontend" | "backend"
interface MergeOption { value: string; label: string; target: MergeTarget | string }
interface MergeKindOptions { repoKind: MergeRepoKind | string; options: MergeOption[]; defaultValue?: string | null }
interface MergeOptionsPayload { ok: boolean; reqId: string; status: ReqStatus | string; generatedAt: number; branchScope?: BranchScope | null; options: { frontend: MergeKindOptions; backend: MergeKindOptions } }
interface MergeBranchResult { repoName: string; role?: string | null; projectPath?: string | null; sourceBranch: string; target: MergeTarget | string; targetBranch?: string | null; status: "merged" | "upToDate" | "conflict" | "failed" | "skipped" | "idle" | "pending" | string; message?: string | null; conflictFiles?: string[]; worktreePath?: string | null; warnings?: string[]; commands?: string[] }
interface MergeBranchPayload { ok: boolean; reqId: string; target?: MergeTarget | string; targetBranch?: string | null; repoKind?: MergeRepoKind | string | null; status: string; generatedAt: number; branchScope?: BranchScope | null; results: MergeBranchResult[] }
interface DiffLine { type: "add" | "del" | "ctx" | "hunk"; oldNo: string; newNo: string; text: string }
interface DiffFileView { repo: CodeReviewRepoSnapshot; file: CodeReviewFile; diff: string; lines: DiffLine[] }

const cardVariants = {
  hidden: { opacity: 0, y: 18, scale: 0.98 },
  show: { opacity: 1, y: 0, scale: 1 },
}

const navItems = [
  { href: "/dashboard", label: "状态看板", short: "DB", icon: <LayoutDashboard size={16} /> },
  { href: "/projects", label: "需求看板", short: "PR", icon: <ListChecks size={16} /> },
  { href: "/business-knowledge", label: "业务知识", short: "BK", icon: <Library size={16} /> },
  { href: "/experiences", label: "经验", short: "EX", icon: <Lightbulb size={16} /> },
  { href: "/sessions", label: "Sessions", short: "SE", icon: <Server size={16} /> },
  { href: "/schedulers", label: "Schedulers", short: "SC", icon: <Activity size={16} /> },
  { href: "/git-ai", label: "Git AI", short: "AI", icon: <GitBranch size={16} /> },
  { href: "/testdata", label: "造数", short: "TD", icon: <Sparkles size={16} /> },
  { href: "/settings", label: "Settings", short: "ST", icon: <Settings size={16} /> },
]

function isActiveNav(path: string, href: string): boolean {
  if (href === "/dashboard") return path === "/" || path === "/dashboard"
  if (href === "/projects") return path === "/projects" || path === "/requirements" || path === "/requirement" || path === "/requirement-diff" || path === "/requirement-merge"
  if (href === "/business-knowledge") return path === "/business-knowledge"
  if (href === "/experiences") return path === "/experiences"
  if (href === "/schedulers") return path === "/schedulers"
  if (href === "/git-ai") return path === "/git-ai"
  if (href === "/testdata") return path === "/testdata"
  return path === href
}

function titleForPath(path: string): { eyebrow: string; title: string } {
  if (path === "/" || path === "/dashboard") return { eyebrow: "Dashboard", title: "状态看板" }
  if (path === "/projects" || path === "/requirements") return { eyebrow: "Requirements", title: "需求进度看板" }
  if (path === "/business-knowledge") return { eyebrow: "Business Knowledge", title: "业务知识" }
  if (path === "/experiences") return { eyebrow: "Experiences", title: "经验" }
  if (path === "/requirement") return { eyebrow: "Requirement", title: "需求详情" }
  if (path === "/requirement-diff") return { eyebrow: "Diff", title: "分支差异" }
  if (path === "/requirement-merge") return { eyebrow: "Merge", title: "分支合并" }
  if (path === "/sessions") return { eyebrow: "Pi Sessions", title: "Sessions" }
  if (path === "/session") return { eyebrow: "Session", title: "Session 详情" }
  if (path === "/schedulers") return { eyebrow: "Schedulers", title: "定时任务" }
  if (path === "/git-ai") return { eyebrow: "Git AI", title: "漏标检查" }
  if (path === "/settings") return { eyebrow: "Settings", title: "Settings" }
  if (path === "/testdata") return { eyebrow: "Test Data", title: "测试造数" }
  return { eyebrow: "Agent Panel", title: "React + Rust" }
}

function AppShell({ path, children, project, onProjectChange }: { path: string; children: React.ReactNode; project: string; onProjectChange: (v: string) => void }) {
  const meta = titleForPath(path)
  return <div className="react-shell"><aside className="react-sidebar"><a className="react-brand" href="/dashboard" aria-label="Agent Panel home"><span className="react-brand-mark">AP</span><span className="react-brand-copy"><strong>Agent</strong><em>Panel</em></span></a><nav className="react-sidebar-nav" aria-label="Primary navigation">{navItems.map((item) => <a key={item.href} href={item.href} className={`react-sidebar-link ${isActiveNav(path, item.href) ? "active" : ""}`}><span className="react-sidebar-icon">{item.icon}</span><span>{item.label}</span><small>{item.short}</small></a>)}</nav><div className="react-sidebar-card"><span>RUST BACKEND</span><strong>localhost:7331</strong><em>PTY removed</em></div></aside><div className="react-content-shell"><header className="react-topbar"><div><span>{meta.eyebrow}</span><strong>{meta.title}</strong></div><div className="react-topbar-actions"><label className="react-project-select">项目<select value={project} onChange={(e) => onProjectChange(e.target.value)}>{PROJECT_OPTIONS.map((o) => <option key={o.value} value={o.value}>{o.label}</option>)}</select></label><a href="/dashboard">Home</a><button type="button" onClick={() => window.location.reload()}>Refresh</button></div></header><main className="react-main">{children}</main></div></div>
}

function useLocationKey() {
  const [key, setKey] = useState(() => window.location.pathname + window.location.search)
  useEffect(() => {
    const onPop = () => setKey(window.location.pathname + window.location.search)
    window.addEventListener("popstate", onPop)
    return () => window.removeEventListener("popstate", onPop)
  }, [])
  return key
}

async function fetchJson<T>(url: string, init?: RequestInit): Promise<T> {
  const res = await fetch(url, { cache: "no-store", ...init })
  if (!res.ok) {
    const text = await res.text().catch(() => "")
    throw new Error(text || `HTTP ${res.status}`)
  }
  return res.json() as Promise<T>
}

async function postJson<T>(url: string, data: unknown): Promise<T> {
  return fetchJson<T>(url, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(data),
  })
}

async function postForm<T>(url: string, data: Record<string, string>): Promise<T> {
  return fetchJson<T>(url, {
    method: "POST",
    headers: { "Content-Type": "application/x-www-form-urlencoded" },
    body: new URLSearchParams(data),
  })
}

const PROJECT_FILTER_KEY = "agent-panel.project"
const PROJECT_OPTIONS = [
  { value: "", label: "全部" },
  { value: "WMS", label: "WMS" },
]

function readProjectFilter(): string {
  try { return localStorage.getItem(PROJECT_FILTER_KEY) || "" } catch { return "" }
}

function useProjectFilter() {
  const [project, setProjectState] = useState<string>(readProjectFilter)
  const setProject = (v: string) => {
    setProjectState(v)
    try { localStorage.setItem(PROJECT_FILTER_KEY, v) } catch { /* ignore */ }
  }
  return { project, setProject }
}

function useFetch<T>(url: string | null, deps: unknown[] = []) {
  const [data, setData] = useState<T | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [loading, setLoading] = useState(Boolean(url))
  const [nonce, setNonce] = useState(0)
  useEffect(() => {
    if (!url) return
    let cancelled = false
    setLoading(true)
    fetchJson<T>(`${url}${url.includes("?") ? "&" : "?"}t=${Date.now()}`)
      .then((value) => { if (!cancelled) { setData(value); setError(null) } })
      .catch((err: Error) => { if (!cancelled) setError(err.message) })
      .finally(() => { if (!cancelled) setLoading(false) })
    return () => { cancelled = true }
  }, [url, nonce, ...deps])
  return { data, error, loading, refresh: () => setNonce((v) => v + 1) }
}

function formatDuration(ms: number): string {
  const safe = Math.max(0, ms)
  const sec = Math.floor(safe / 1000)
  if (sec < 60) return `${sec}秒`
  const min = Math.floor(sec / 60)
  if (min < 60) return `${min}分钟`
  const hr = Math.floor(min / 60)
  const remainMin = min % 60
  if (hr < 24) return remainMin > 0 ? `${hr}小时${remainMin}分钟` : `${hr}小时`
  const day = Math.floor(hr / 24)
  const remainHr = hr % 24
  return remainHr > 0 ? `${day}天${remainHr}小时` : `${day}天`
}

function formatDate(ms?: number): string {
  if (!ms) return "-"
  return new Date(ms).toLocaleDateString("zh-CN")
}

function formatDateTime(ms?: number): string {
  if (!ms) return "-"
  return new Date(ms).toLocaleString("zh-CN")
}

function relAge(ms?: number): string {
  if (!ms) return "-"
  const diff = Math.max(0, Date.now() - ms)
  if (diff < 60_000) return "刚刚"
  if (diff < 3_600_000) return `${Math.floor(diff / 60_000)}分钟前`
  if (diff < 86_400_000) return `${Math.floor(diff / 3_600_000)}小时前`
  return `${Math.floor(diff / 86_400_000)}天前`
}

function statusPill(status: string) {
  const meta = statusMeta[status] || statusMeta["需求澄清"]
  return <span className="react-status-pill" style={{ color: meta.color, background: meta.soft, borderColor: `${meta.color}55` }}>{status}</span>
}

function projectsOf(req: Requirement): string {
  return (req.projects?.length ? req.projects : [req.project]).filter(Boolean).join(" / ") || "-"
}

function parseOnesRef(raw?: string): { raw: string; url: string | null; label: string } | null {
  const value = (raw || "").trim()
  if (!value) return null
  if (/^https?:\/\//i.test(value)) {
    let label = value
    try {
      const url = new URL(value)
      const issueCode = url.hash.match(/(?:^|\/)issue\/([^/?#]+)/i)?.[1]
      const pathSegment = url.pathname.split("/").filter(Boolean).pop()
      const segment = issueCode || pathSegment
      if (segment && segment.length <= 60) label = decodeURIComponent(segment)
    } catch { /* keep full value */ }
    return { raw: value, url: value, label }
  }
  return { raw: value, url: null, label: value }
}

function onesBadge(ones?: string) {
  const ref = parseOnesRef(ones)
  if (!ref) return <span className="react-ones-badge react-ones-missing" title="未关联 ONES 任务">⚠ 未关联 ONES</span>
  if (ref.url) return <a className="react-ones-badge react-ones-linked" href={ref.url} target="_blank" rel="noopener noreferrer" title={ref.raw}>🔗 ONES</a>
  return <span className="react-ones-badge react-ones-id" title={ref.label}>ONES</span>
}

function splitCsvText(value?: string | string[]): string[] {
  if (Array.isArray(value)) return value.map((v) => v.trim()).filter(Boolean)
  return (value || "").split(/[，,\n]/).map((v) => v.trim()).filter(Boolean)
}

function joinList(value?: string[]): string {
  return (value || []).join(", ")
}

function emptyKnowledgeDraft(kind: KnowledgeKind): KnowledgeDraft {
  return {
    kind,
    title: "",
    domain: "general",
    project: "",
    scope: "project",
    status: "active",
    confidence: "medium",
    tags: [],
    triggerTerms: [],
    relatedSkills: [],
    relatedRepos: [],
    relatedTables: [],
    relatedApis: [],
    source: "manual",
    summary: "",
    details: kind === "businessKnowledge" ? "## 概述\n\n## 规则\n\n## 相关接口\n\n## 相关表\n\n## 注意事项" : "## 现象\n\n## 根因\n\n## 处理方式\n\n## 验证方式\n\n## 过期风险",
  }
}

function knowledgeStatusPill(status?: string) {
  const s = status || "active"
  const color = s === "active" ? "#22c55e" : s === "stale" ? "#f59e0b" : s === "deprecated" ? "#ef4444" : "#94a3b8"
  const soft = s === "active" ? "rgba(34,197,94,.14)" : s === "stale" ? "rgba(245,158,11,.14)" : s === "deprecated" ? "rgba(239,68,68,.14)" : "rgba(148,163,184,.14)"
  return <span className="react-status-pill" style={{ color, background: soft, borderColor: `${color}66` }}>{s}</span>
}

function confidencePill(confidence?: string) {
  const value = confidence || "medium"
  return <span className="react-effort-badge">confidence {value}</span>
}

function KnowledgePage({ kind }: { kind: KnowledgeKind }) {
  const isBiz = kind === "businessKnowledge"
  const label = isBiz ? "业务知识" : "经验"
  const [query, setQuery] = useState("")
  const [domain, setDomain] = useState("")
  const [status, setStatus] = useState("")
  const [selectedId, setSelectedId] = useState<string | null>(null)
  const [draft, setDraft] = useState<KnowledgeDraft>(() => emptyKnowledgeDraft(kind))
  const [saving, setSaving] = useState(false)
  const [message, setMessage] = useState<string | null>(null)
  const params = new URLSearchParams({ kind, limit: "100" })
  if (query) params.set("q", query)
  if (domain) params.set("domain", domain)
  if (status) params.set("status", status)
  const list = useFetch<KnowledgeListPayload>(`/api/knowledge?${params.toString()}`, [kind, query, domain, status])
  const items = list.data?.items || []
  const domains = useMemo(() => Array.from(new Set(items.map((i) => i.domain).filter(Boolean) as string[])).sort(), [items])
  const selected = selectedId ? items.find((item) => item.id === selectedId) || null : null
  useEffect(() => { setDraft(emptyKnowledgeDraft(kind)); setSelectedId(null); setMessage(null) }, [kind])
  const editItem = async (item: KnowledgeItem) => {
    setSelectedId(item.id)
    try {
      const full = await fetchJson<{ item: KnowledgeItem }>(`/api/knowledge/item?id=${encodeURIComponent(item.id)}`)
      setDraft({ ...full.item, details: full.item.details || "" })
    } catch {
      setDraft({ ...item, details: item.summary || "" })
    }
  }
  const resetDraft = () => { setSelectedId(null); setDraft(emptyKnowledgeDraft(kind)); setMessage(null) }
  const save = async () => {
    if (!draft.title?.trim() || saving) return
    setSaving(true)
    setMessage(null)
    try {
      const payload = {
        id: selectedId || draft.id || undefined,
        kind,
        title: draft.title,
        domain: draft.domain || "general",
        project: draft.project || "",
        scope: draft.scope || "project",
        status: draft.status || "active",
        confidence: draft.confidence || "medium",
        tags: splitCsvText(draft.tags),
        triggerTerms: splitCsvText(draft.triggerTerms),
        relatedSkills: splitCsvText(draft.relatedSkills),
        relatedRepos: splitCsvText(draft.relatedRepos),
        relatedTables: splitCsvText(draft.relatedTables),
        relatedApis: splitCsvText(draft.relatedApis),
        source: draft.source || "manual",
        summary: draft.summary || "",
        details: draft.details || "",
      }
      const saved = await postJson<KnowledgeSavePayload>("/api/knowledge", payload)
      setSelectedId(saved.item.id)
      setDraft({ ...saved.item, details: saved.item.details || draft.details || "" })
      setMessage("已保存")
      list.refresh()
    } catch (err) {
      setMessage(`保存失败：${err instanceof Error ? err.message : String(err)}`)
    } finally {
      setSaving(false)
    }
  }
  const apiHint = `POST /api/agent/knowledge/query\n{\n  "kind": "${kind}",\n  "intent": "${query || (isBiz ? "状态流转 / 接口字段" : "排障现象 / 错误信息")}",\n  "domain": "${domain || "wms"}",\n  "limit": 5,\n  "tokensBudget": 2200\n}`
  return <PageChrome icon={isBiz ? <Library size={15} /> : <Lightbulb size={15} />} eyebrow={isBiz ? "Business Knowledge" : "Experiences"} title={label} description={isBiz ? "维护短保质期的业务事实、规则、接口和表关系；Agent 默认只查询摘要。" : "维护特化排障经验、踩坑记录和历史处理方式；成熟稳定后再提升为 skill。"} actions={<button onClick={list.refresh}><RefreshCw size={15} />刷新</button>}><section className="react-panel react-filter-panel"><div className="react-filter-grid"><label>关键词<input value={query} onChange={(e) => setQuery(e.target.value)} placeholder={isBiz ? "状态 / 接口 / 表 / 字段" : "现象 / 报错 / 模块 / 处理方式"} /></label><label>Domain<input list="knowledge-domains" value={domain} onChange={(e) => setDomain(e.target.value)} placeholder="wms / general" /><datalist id="knowledge-domains">{domains.map((d) => <option key={d} value={d} />)}</datalist></label><label>Status<select value={status} onChange={(e) => setStatus(e.target.value)}><option value="">全部</option><option value="active">active</option><option value="stale">stale</option><option value="deprecated">deprecated</option><option value="draft">draft</option></select></label><label>接口提示<textarea readOnly rows={4} value={apiHint} /></label></div><div className="react-actions"><button onClick={resetDraft}>新建{label}</button><a href={isBiz ? "/experiences" : "/business-knowledge"}>{isBiz ? "切到经验" : "切到业务知识"}</a></div></section>{list.error ? <ErrorCard error={list.error} /> : <div className="react-knowledge-layout"><section className="react-card-list">{list.loading ? <LoadingCard /> : items.length === 0 ? <EmptyCard>暂无{label}记录。</EmptyCard> : items.map((item, index) => <KnowledgeCard key={item.id} item={item} selected={item.id === selectedId} index={index} onEdit={() => editItem(item)} />)}</section><KnowledgeEditor kind={kind} draft={draft} setDraft={setDraft} saving={saving} selected={selected} message={message} onSave={save} onReset={resetDraft} /></div>}</PageChrome>
}

function KnowledgeCard({ item, selected, index, onEdit }: { item: KnowledgeItem; selected: boolean; index: number; onEdit: () => void }) {
  return <motion.article className={`react-list-card react-knowledge-card ${selected ? "active" : ""}`} initial={{ opacity: 0, y: 12 }} animate={{ opacity: 1, y: 0 }} transition={{ delay: Math.min(index, 16) * 0.025 }} whileHover={{ y: -3 }}><div><span className="react-card-id">{item.id}</span><h3>{item.title}</h3><p>{item.summary || "暂无摘要"}</p><div className="react-card-meta"><span>{item.domain || "general"}</span>{item.project ? <span>{item.project}</span> : null}<span>{item.scope || "project"}</span><span>更新 {item.updatedAt || "-"}</span>{item.whyMatched?.length ? <span>match: {item.whyMatched.join(" / ")}</span> : null}</div><div className="react-chip-list">{(item.tags || []).slice(0, 6).map((tag) => <span key={tag}>{tag}</span>)}</div></div><div className="react-card-side">{knowledgeStatusPill(item.status)}{confidencePill(item.confidence)}<button type="button" className="react-copy-link-btn" onClick={onEdit}>编辑</button></div></motion.article>
}

function KnowledgeEditor({ kind, draft, setDraft, saving, selected, message, onSave, onReset }: { kind: KnowledgeKind; draft: KnowledgeDraft; setDraft: (draft: KnowledgeDraft) => void; saving: boolean; selected: KnowledgeItem | null; message: string | null; onSave: () => void; onReset: () => void }) {
  const isBiz = kind === "businessKnowledge"
  const set = (patch: KnowledgeDraft) => setDraft({ ...draft, ...patch })
  return <section className="react-panel react-knowledge-editor"><PanelHead kicker={selected ? "Edit" : "Create"} title={selected ? selected.title : `新建${isBiz ? "业务知识" : "经验"}`} chip={draft.id || "new"} /><div className="react-knowledge-form"><label>标题<input value={draft.title || ""} onChange={(e) => set({ title: e.target.value })} placeholder={isBiz ? "例如：WMS 出库单状态流转" : "例如：波次取消后库存占用未释放排查"} /></label><label>Domain<input value={draft.domain || ""} onChange={(e) => set({ domain: e.target.value })} placeholder="wms" /></label><label>Project<input value={draft.project || ""} onChange={(e) => set({ project: e.target.value })} placeholder="可选" /></label><label>Scope<select value={draft.scope || "project"} onChange={(e) => set({ scope: e.target.value })}><option value="project">project</option><option value="global">global</option></select></label><label>Status<select value={draft.status || "active"} onChange={(e) => set({ status: e.target.value })}><option value="active">active</option><option value="stale">stale</option><option value="deprecated">deprecated</option><option value="draft">draft</option></select></label><label>Confidence<select value={draft.confidence || "medium"} onChange={(e) => set({ confidence: e.target.value })}><option value="low">low</option><option value="medium">medium</option><option value="high">high</option></select></label><label>Tags<input value={joinList(draft.tags)} onChange={(e) => set({ tags: splitCsvText(e.target.value) })} placeholder="逗号分隔" /></label><label>Trigger Terms<input value={joinList(draft.triggerTerms)} onChange={(e) => set({ triggerTerms: splitCsvText(e.target.value) })} placeholder="Agent 查询关键词" /></label><label>Related Skills<input value={joinList(draft.relatedSkills)} onChange={(e) => set({ relatedSkills: splitCsvText(e.target.value) })} placeholder="skill-name" /></label><label>Related Repos<input value={joinList(draft.relatedRepos)} onChange={(e) => set({ relatedRepos: splitCsvText(e.target.value) })} placeholder="yl-cwhsea-wms-web" /></label><label>Related Tables<input value={joinList(draft.relatedTables)} onChange={(e) => set({ relatedTables: splitCsvText(e.target.value) })} placeholder="shipment_header" /></label><label>Related APIs<input value={joinList(draft.relatedApis)} onChange={(e) => set({ relatedApis: splitCsvText(e.target.value) })} placeholder="POST /wms-web/... 或 api-* id" /></label><label>Source<input value={draft.source || ""} onChange={(e) => set({ source: e.target.value })} placeholder="manual / code / test / session" /></label><label className="react-knowledge-wide">摘要<textarea rows={3} value={draft.summary || ""} onChange={(e) => set({ summary: e.target.value })} placeholder="给 Agent 默认返回的一句话或短段摘要" /></label><label className="react-knowledge-wide">详情<textarea rows={14} value={draft.details || ""} onChange={(e) => set({ details: e.target.value })} placeholder="Markdown 正文" /></label></div><div className="react-actions"><button onClick={onSave} disabled={saving || !draft.title?.trim()}>{saving ? "保存中…" : "保存"}</button><button type="button" onClick={onReset}>重置</button></div>{message ? <p className={message.startsWith("保存失败") ? "react-effort-error" : "react-save-hint"}>{message}</p> : null}<p className="react-muted">保存后写入 <code>{isBiz ? ".agents/business-knowledge/" : ".agents/experiences/"}</code> 或全局 <code>~/.agents/</code> 对应目录，自动维护 created_at / updated_at。</p></section>
}

function PageChrome({ icon, eyebrow, title, description, actions, children }: { icon: React.ReactNode; eyebrow: string; title: string; description?: string; actions?: React.ReactNode; children: React.ReactNode }) {
  return (
    <div className="react-page">
      <motion.section className="react-hero react-page-hero" initial={{ opacity: 0, y: 16 }} animate={{ opacity: 1, y: 0 }} transition={{ duration: 0.42 }}>
        <div className="react-hero-grid" aria-hidden="true" />
        <div className="react-hero-copy">
          <span className="react-eyebrow">{icon} {eyebrow}</span>
          <h1>{title}</h1>
          {description ? <p>{description}</p> : null}
          {actions ? <div className="react-hero-actions">{actions}</div> : null}
        </div>
      </motion.section>
      {children}
    </div>
  )
}

function LoadingCard({ label = "正在加载…" }: { label?: string }) { return <div className="react-loading">{label}</div> }
function ErrorCard({ error }: { error: string }) { return <div className="react-error">加载失败：{error}</div> }
function EmptyCard({ children }: { children: React.ReactNode }) { return <div className="react-empty">{children}</div> }

function PanelHead({ kicker, title, chip }: { kicker: string; title: string; chip?: React.ReactNode }) {
  return <div className="react-panel-head"><div><span>{kicker}</span><h2>{title}</h2></div>{chip ? <em>{chip}</em> : null}</div>
}

function KpiCard({ icon, label, value, sub, tone }: { icon: React.ReactNode; label: string; value: string | number; sub: string; tone: string }) {
  return <motion.article className={`react-kpi react-kpi-${tone}`} variants={cardVariants} whileHover={{ y: -5, scale: 1.01 }} transition={{ type: "spring", stiffness: 260, damping: 24 }}><div className="react-kpi-icon">{icon}</div><span className="react-kpi-label">{label}</span><motion.strong className="react-kpi-value" layout>{value}</motion.strong><span className="react-kpi-sub">{sub}</span></motion.article>
}

function PipelineBar({ item, index }: { item: StatusCount; index: number }) {
  const meta = statusMeta[item.status] || { color: "#94a3b8", soft: "rgba(148, 163, 184, 0.14)" }
  return <motion.div className="react-pipeline-row" initial={{ opacity: 0, x: -14 }} animate={{ opacity: 1, x: 0 }} transition={{ delay: 0.08 + index * 0.045 }}><div className="react-pipeline-label"><span className="react-pipeline-dot" style={{ background: meta.color, boxShadow: `0 0 14px ${meta.color}66` }} /><span>{item.status}</span></div><div className="react-pipeline-track"><motion.div className="react-pipeline-fill" style={{ background: `linear-gradient(90deg, ${meta.color}, ${meta.color}99)` }} initial={{ width: 0 }} animate={{ width: `${Math.max(1.5, item.percent)}%` }} transition={{ duration: 0.85, delay: 0.16 + index * 0.04 }} /></div><strong>{item.count}</strong><span>{item.percent}%</span></motion.div>
}

function DurationRow({ item, max, index }: { item: RequirementDuration; max: number; index: number }) {
  const meta = statusMeta[item.req.status] || statusMeta["需求澄清"]
  const pct = Math.min(100, (item.durationMs / Math.max(max, 1)) * 100)
  return <motion.tr initial={{ opacity: 0, y: 8 }} animate={{ opacity: 1, y: 0 }} transition={{ delay: 0.16 + index * 0.035 }}><td><a href={`/requirement?id=${encodeURIComponent(item.req.id)}`}>{item.req.title}</a><div className="react-duration-id">{item.req.id}</div></td><td><span className="react-status-pill" style={{ color: meta.color, background: meta.soft, borderColor: `${meta.color}55` }}>{item.req.status}</span></td><td className="react-muted">{(item.req.projects?.length ? item.req.projects : [item.req.project]).filter(Boolean).join(" / ") || "-"}</td><td className="react-muted">{formatDate(item.req.createdAt)}</td><td className="react-duration-cell"><motion.div className="react-duration-fill" initial={{ width: 0 }} animate={{ width: `${pct}%` }} transition={{ duration: 0.8, delay: 0.2 + index * 0.025 }} /><span>{formatDuration(item.durationMs)}</span></td></motion.tr>
}

function DashboardPage({ apiPath, project }: { apiPath: string; project: string }) {
  const url = project ? `${apiPath}${apiPath.includes("?") ? "&" : "?"}project=${encodeURIComponent(project)}` : apiPath
  const { data: payload, error, loading, refresh } = useFetch<DashboardStatsPayload>(url, [project])
  const stats = payload?.stats
  const completionRate = useMemo(() => (!stats || stats.total === 0) ? 0 : Math.round((stats.completedCount / stats.total) * 100), [stats])
  const activeRate = useMemo(() => (!stats || stats.total === 0) ? 0 : Math.round((stats.inProgressCount / stats.total) * 100), [stats])
  return <div className="react-dashboard"><motion.section className="react-hero" initial={{ opacity: 0, y: 16 }} animate={{ opacity: 1, y: 0 }} transition={{ duration: 0.48 }}><div className="react-hero-grid" aria-hidden="true" /><div className="react-hero-copy"><span className="react-eyebrow"><LayoutDashboard size={14} /> Agent Panel</span><h1>React + Rust 控制台</h1><p>前端由 React SPA 接管，后端已切到 Rust/Axum；OpenCode 旧报告链路和 PTY terminal 已移除。</p><div className="react-hero-actions"><button type="button" onClick={refresh} disabled={loading}><RefreshCw size={15} className={loading ? "react-spin" : ""} /> 刷新数据</button><a href="/projects"><Sparkles size={15} /> 进入需求看板</a></div></div><motion.div className="react-orb" animate={{ y: [0, -8, 0] }} transition={{ duration: 5, repeat: Infinity }}><strong>{stats?.total ?? "—"}</strong><span>REQS</span></motion.div></motion.section>{error ? <ErrorCard error={error} /> : !stats ? <LoadingCard label="正在加载 dashboard stats…" /> : <motion.div className="react-dashboard-body" initial="hidden" animate="show" transition={{ staggerChildren: 0.06 }}><motion.section className="react-kpi-grid" variants={{ hidden: {}, show: { transition: { staggerChildren: 0.07 } } }}><KpiCard icon={<Gauge size={20} />} label="需求总数" value={stats.total} sub="Tracked requirements" tone="total" /><KpiCard icon={<CheckCircle2 size={20} />} label="已完成" value={stats.completedCount} sub={`${completionRate}% complete`} tone="done" /><KpiCard icon={<Activity size={20} />} label="进行中" value={stats.inProgressCount} sub={`${activeRate}% active`} tone="active" /><KpiCard icon={<Clock3 size={20} />} label="平均交付时长" value={formatDuration(stats.avgDeliveryMs)} sub={`中位数 ${formatDuration(stats.medianDeliveryMs)} · 最长 ${formatDuration(stats.maxDeliveryMs)}`} tone="avg" /></motion.section><section className="react-content-grid"><motion.article className="react-panel" variants={cardVariants}><PanelHead kicker="Pipeline" title="需求状态分布" chip={`${stats.statusCounts.length} stages`} /><div className="react-pipeline-list">{stats.statusCounts.map((item, index) => <PipelineBar key={item.status} item={item} index={index} />)}</div></motion.article><motion.article className="react-panel react-delivery-panel" variants={cardVariants}><PanelHead kicker="Delivery" title="需求交付时长" chip={<><Clock3 size={13} /> Top durations</>} />{stats.durations.length === 0 ? <EmptyCard>暂无需求数据。</EmptyCard> : <DurationTable durations={stats.durations.slice(0, 18)} max={stats.maxDeliveryMs} />}</motion.article></section></motion.div>}</div>
}

function DurationTable({ durations, max }: { durations: RequirementDuration[]; max: number }) {
  return <div className="react-table-wrap"><table className="react-duration-table"><thead><tr><th>需求</th><th>状态</th><th>项目</th><th>创建时间</th><th>交付时长</th></tr></thead><tbody>{durations.map((item, index) => <DurationRow key={item.req.id} item={item} max={max} index={index} />)}</tbody></table></div>
}

function billableWindow(): { from: number; to: number; label: string } {
  const now = new Date()
  const y = now.getFullYear()
  const m = now.getMonth()
  const to = new Date(y, m, 20, 23, 59, 59, 999)
  const from = new Date(y, m - 1, 20, 0, 0, 0, 0)
  const fmt = (d: Date) => `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`
  return { from: from.getTime(), to: to.getTime(), label: `${fmt(from)} ~ ${fmt(to)}` }
}

function ProjectsPage({ globalProject }: { globalProject: string }) {
  const { data, error, loading } = useFetch<{ requirements: Requirement[] }>("/api/requirements")
  const params = new URLSearchParams(window.location.search)
  const [project, setProject] = useState(params.get("project") || readProjectFilter())
  useEffect(() => {
    if (!globalProject) return
    setProject(globalProject)
    const q = new URLSearchParams(window.location.search)
    q.set("project", globalProject)
    window.history.replaceState(null, "", `${window.location.pathname}?${q.toString()}`)
  }, [globalProject])
  const [createdFrom, setCreatedFrom] = useState(params.get("createdFrom") || "")
  const [createdTo, setCreatedTo] = useState(params.get("createdTo") || "")
  const [statuses, setStatuses] = useState<string[]>(params.getAll("status"))
  const [category, setCategory] = useState<string>(params.get("category") || "")
  const [keyword, setKeyword] = useState(params.get("q") || "")
  const [billableOnly, setBillableOnly] = useState(false)
  const reqs = data?.requirements || []
  const win = useMemo(() => billableWindow(), [])
  const projects = useMemo(() => [...new Set(reqs.flatMap((r) => r.projects?.length ? r.projects : [r.project]).filter(Boolean))].sort(), [reqs])
  const counts = useMemo(() => Object.fromEntries(REQ_STATUSES.map((s) => [s, reqs.filter((r) => r.status === s).length])), [reqs]) as Record<string, number>
  const filtered = useMemo(() => reqs.filter((r) => {
    if (!billableOnly && statuses.length === 0 && r.status === "已完成") return false
    if (statuses.length && !statuses.includes(r.status)) return false
    if (category && (r.category ?? "需求") !== category) return false
    if (project && !(r.projects?.length ? r.projects : [r.project]).includes(project)) return false
    if (createdFrom && r.createdAt < new Date(`${createdFrom}T00:00:00`).getTime()) return false
    if (createdTo && r.createdAt > new Date(`${createdTo}T23:59:59`).getTime()) return false
    if (keyword.trim()) {
      const kw = keyword.trim().toLowerCase()
      const haystack = [r.id, r.title, r.description || "", projectsOf(r)].join(" ").toLowerCase()
      if (!haystack.includes(kw)) return false
    }
    if (billableOnly) {
      if (parseOnesRef(r.ones)) return false
      if ((r.category ?? "需求") === "线上问题") return false
      if (r.status === "已完成") {
        const doneAt = r.completedAt ?? r.updatedAt
        if (doneAt < win.from || doneAt > win.to) return false
      }
    }
    return true
  }).sort((a, b) => b.updatedAt - a.updatedAt), [reqs, statuses, category, project, createdFrom, createdTo, keyword, billableOnly, win])
  const billableHours = useMemo(() => filtered.reduce((sum, r) => sum + (r.effortEstimate?.estimatedHours || 0), 0), [filtered])
  const apply = () => {
    const q = new URLSearchParams()
    if (createdFrom) q.set("createdFrom", createdFrom)
    if (createdTo) q.set("createdTo", createdTo)
    if (project) q.set("project", project)
    if (category) q.set("category", category)
    if (keyword) q.set("q", keyword)
    for (const s of statuses) q.append("status", s)
    window.location.href = `/projects${q.toString() ? `?${q}` : ""}`
  }
  return <PageChrome icon={<ListChecks size={15} />} eyebrow="Requirements" title="需求进度看板" description="按项目、状态和创建时间筛选需求，查看关联 pi session 和最近更新。"><section className="react-panel react-filter-panel"><div className="react-filter-grid"><label>项目<select value={project} onChange={(e) => setProject(e.target.value)}><option value="">全部项目</option>{projects.map((p) => <option key={p} value={p}>{p}</option>)}</select></label><label>类别<select value={category} onChange={(e) => setCategory(e.target.value)}><option value="">全部类别</option>{REQ_CATEGORIES.map((c) => <option key={c} value={c}>{c}</option>)}</select></label><label>创建开始<input type="date" value={createdFrom} onChange={(e) => setCreatedFrom(e.target.value)} /></label><label>创建结束<input type="date" value={createdTo} onChange={(e) => setCreatedTo(e.target.value)} /></label><label className="react-filter-grow">关键词<input value={keyword} onChange={(e) => setKeyword(e.target.value)} placeholder="标题 / req id / 描述" /></label></div><div className="react-status-options">{REQ_STATUSES.map((s) => <label key={s} className={`react-status-option ${statuses.includes(s) ? "active" : ""}`}><input type="checkbox" checked={statuses.includes(s)} onChange={(e) => setStatuses((cur) => e.target.checked ? [...cur, s] : cur.filter((x) => x !== s))} /><span>{s}</span><strong>{counts[s] || 0}</strong></label>)}</div><div className="react-actions"><button onClick={apply}>应用筛选</button><a href="/projects">重置</a><button type="button" className={`react-toggle-btn ${billableOnly ? "active" : ""}`} onClick={() => setBillableOnly((v) => !v)} title="筛选：未关联 ONES、非线上问题、完成时间在上月20到本月20之间或未完成">本月可结算业务工时</button></div></section>{error ? <ErrorCard error={error} /> : loading ? <LoadingCard /> : <>{billableOnly ? <div className="react-billable-summary"><span>本月可结算业务工时</span><strong>{billableHours.toFixed(1)}h</strong><span>{filtered.length} 条需求</span><span className="react-muted">{win.label}</span></div> : null}<div className="react-card-list">{filtered.length === 0 ? <EmptyCard>暂无符合条件的需求。</EmptyCard> : filtered.map((req, index) => <RequirementCard key={req.id} req={req} index={index} />)}</div></>}</PageChrome>
}

function RequirementCard({ req, index }: { req: Requirement; index: number }) {
  return <motion.article className="react-list-card react-req-card" initial={{ opacity: 0, y: 12 }} animate={{ opacity: 1, y: 0 }} transition={{ delay: Math.min(index, 16) * 0.025 }} whileHover={{ y: -3 }}><div><span className="react-card-id">{req.id}</span><h3><a href={`/requirement?id=${encodeURIComponent(req.id)}`}>{req.title}</a></h3><p>{req.description || "暂无描述"}</p><div className="react-card-meta"><span>{projectsOf(req)}</span><span>{req.sessionIds?.length || 0} session(s)</span><span>更新 {relAge(req.updatedAt)}</span></div></div><div className="react-card-side">{req.effortEstimate ? <span className="react-effort-badge">{req.effortEstimate.estimatedHours}h</span> : null}{req.category === "线上问题" ? <span className="react-status-pill" style={{ color: "#f87171", background: "rgba(239, 68, 68, 0.14)", borderColor: "rgba(239, 68, 68, 0.4)" }}>线上问题</span> : null}{statusPill(req.status)}{onesBadge(req.ones)}</div></motion.article>
}

function SessionsPage() {
  const days = new URLSearchParams(window.location.search).get("days") || "7"
  const { data, error, loading, refresh } = useFetch<ApiSessions>(`/api/sessions?days=${encodeURIComponent(days)}`, [days])
  const sessions = data?.sessions || []
  return <PageChrome icon={<Server size={15} />} eyebrow="Pi Sessions" title="Sessions" description="只读浏览本机 pi session；PTY / terminal 已从新版移除。" actions={<button onClick={refresh}><RefreshCw size={15} />刷新</button>}><div className="react-tab-row">{[1, 3, 7, 14, 30, 0].map((d) => <a key={d} className={String(d) === days ? "active" : ""} href={`/sessions?days=${d}`}>{d === 0 ? "全部时间" : `近 ${d} 天`}</a>)}</div>{error ? <ErrorCard error={error} /> : loading ? <LoadingCard /> : <div className="react-card-list">{sessions.length === 0 ? <EmptyCard>暂无 pi session。</EmptyCard> : sessions.map((s, index) => <SessionCard key={s.id} session={s} index={index} />)}</div>}</PageChrome>
}

function SessionCard({ session, index }: { session: SessionInfo; index: number }) {
  return <motion.article className="react-list-card react-session-card" initial={{ opacity: 0, y: 12 }} animate={{ opacity: 1, y: 0 }} transition={{ delay: Math.min(index, 16) * 0.025 }} whileHover={{ y: -3 }}><div><span className="react-card-id">{session.id}</span><h3><a href={`/session?id=${encodeURIComponent(session.id)}`}>{session.title || session.id}</a></h3><p>{session.directory || "-"}</p><div className="react-card-meta"><span>{session.agent || "pi"}</span><span>{session.model || session.modelId || session.provider || "model n/a"}</span><span>更新 {relAge(session.updated || session.created)}</span><span>{session.messageCount || 0} messages</span></div></div><div className="react-card-side">{statusPill(session.status)}<span className="react-muted">{session.worktree || "-"}</span></div></motion.article>
}

function SessionPage() {
  const id = new URLSearchParams(window.location.search).get("id") || ""
  const { data, error, loading } = useFetch<{ session: SessionInfo | null; terminalRemoved?: boolean }>(id ? `/api/session?id=${encodeURIComponent(id)}` : null, [id])
  const session = data?.session
  return <PageChrome icon={<Server size={15} />} eyebrow="Session" title={session?.title || id || "Session"} description="新版仅保留 pi session 元数据浏览；terminal/PTY 已删除。" actions={<a href="/sessions"><ArrowLeft size={15} />返回 Sessions</a>}>{error ? <ErrorCard error={error} /> : loading ? <LoadingCard /> : !session ? <EmptyCard>Session not found.</EmptyCard> : <div className="react-detail-grid"><section className="react-panel"><PanelHead kicker="Overview" title="Session 信息" chip={statusPill(session.status)} /><div className="react-meta-grid"><span>ID <code>{session.id}</code></span><span>Agent {session.agent || "pi"}</span><span>Model {session.model || session.modelId || "-"}</span><span>Updated {formatDateTime(session.updated || session.created)}</span><span>Worktree {session.worktree || "-"}</span><span>Messages {session.messageCount || 0}</span></div><p className="react-detail-desc">{session.directory || "-"}</p></section><section className="react-panel"><PanelHead kicker="Removed" title="Terminal 已移除" /><p className="react-muted">PTY / xterm bridge 不再随 Agent Panel 提供。需要继续会话时，请在本机终端运行 pi 自身命令。</p><code className="react-command">pi --session {session.id}</code></section></div>}</PageChrome>
}

function RequirementsData() { return useFetch<{ requirements: Requirement[] }>("/api/requirements") }

function SessionChipList({ sessionIds }: { sessionIds: string[] }) {
  return <div className="react-chip-list">{sessionIds.map((sid) => <a key={sid} href={`/session?id=${encodeURIComponent(sid)}`}>{sid.slice(0, 8)}…</a>)}</div>
}

function OnesPanel({ req, onSaved }: { req: Requirement; onSaved: () => void }) {
  const [ones, setOnes] = useState(req.ones || "")
  const [saving, setSaving] = useState(false)
  const [feedback, setFeedback] = useState<string | null>(null)
  const changed = ones.trim() !== (req.ones ?? "").trim()
  const submit = async () => {
    if (saving || !changed) return
    setSaving(true)
    try {
      await postForm("/api/requirement/ones", { reqId: req.id, ones })
      setFeedback("保存成功")
      onSaved()
    } catch (err) {
      setFeedback(`保存失败：${err instanceof Error ? err.message : String(err)}`)
    } finally {
      setSaving(false)
    }
  }
  return <section className="react-panel"><PanelHead kicker="ONES" title="ONES 任务关联" chip={onesBadge(req.ones)} /><p className="react-muted">粘贴 ONES 网址会自动展示为可点击引用；留空保存可清除关联。</p><div className="react-inline-form"><input value={ones} onChange={(e) => { setOnes(e.target.value); setFeedback(null) }} placeholder="ONES 任务网址或编号" /><button onClick={submit} disabled={saving || !changed}>{saving ? "保存中…" : "保存"}</button></div>{feedback ? <p className="react-save-hint">{feedback}</p> : null}</section>
}

function reviewStats(review?: CodeReviewSnapshot | null) {
  const repos = review?.repos || []
  return {
    repoCount: repos.length,
    fileCount: repos.reduce((n, r) => n + (r.files?.length || 0), 0),
    additions: repos.reduce((n, r) => n + (r.additions || 0), 0),
    deletions: repos.reduce((n, r) => n + (r.deletions || 0), 0),
  }
}

function parseUnifiedDiffFiles(review?: CodeReviewSnapshot | null): DiffFileView[] {
  if (!review) return []
  const rows: DiffFileView[] = []
  for (const repo of review.repos || []) {
    const chunks = splitUnifiedDiff(repo.diff || "")
    for (const file of repo.files || []) {
      const diff = chunks.get(file.path) || ""
      rows.push({ repo, file, diff, lines: parseDiffLines(diff) })
    }
  }
  return rows
}

function splitUnifiedDiff(diff: string): Map<string, string> {
  const files = new Map<string, string>()
  let currentPath = ""
  let buffer: string[] = []
  const flush = () => {
    if (currentPath) files.set(currentPath, buffer.join("\n"))
  }
  for (const line of diff.split("\n")) {
    if (line.startsWith("diff --git ")) {
      flush()
      const match = line.match(/^diff --git a\/(.*?) b\/(.*)$/)
      currentPath = match?.[2] || ""
      buffer = [line]
      continue
    }
    if (currentPath) buffer.push(line)
  }
  flush()
  return files
}

function parseDiffLines(diff: string): DiffLine[] {
  const lines: DiffLine[] = []
  let oldNo = 0
  let newNo = 0
  for (const raw of diff.split("\n")) {
    const hunk = raw.match(/^@@ -(\d+)(?:,\d+)? \+(\d+)(?:,\d+)? @@(.*)$/)
    if (hunk) {
      oldNo = Number(hunk[1])
      newNo = Number(hunk[2])
      lines.push({ type: "hunk", oldNo: "", newNo: "", text: raw })
      continue
    }
    if (!raw || raw.startsWith("diff --git") || raw.startsWith("index ") || raw.startsWith("--- ") || raw.startsWith("+++ ")) continue
    if (raw.startsWith("+")) {
      lines.push({ type: "add", oldNo: "", newNo: String(newNo++), text: raw.slice(1) })
    } else if (raw.startsWith("-")) {
      lines.push({ type: "del", oldNo: String(oldNo++), newNo: "", text: raw.slice(1) })
    } else {
      lines.push({ type: "ctx", oldNo: String(oldNo++), newNo: String(newNo++), text: raw.startsWith(" ") ? raw.slice(1) : raw })
    }
  }
  return lines
}

function shortFileName(path: string): string {
  const parts = path.split("/").filter(Boolean)
  return parts.slice(-1)[0] || path
}

function compactPath(path: string, max = 52): string {
  if (path.length <= max) return path
  const parts = path.split("/")
  if (parts.length <= 3) return `…${path.slice(-(max - 1))}`
  return `${parts[0]}/…/${parts.slice(-3).join("/")}`
}

function diffDomId(key: string): string {
  return `diff-file-${encodeURIComponent(key).replace(/%/g, "_")}`
}

function CodeReviewPanel({ req }: { req: Requirement }) {
  const { data, error, loading, refresh } = useFetch<CodeReviewPayload>(`/api/requirement/code-review?id=${encodeURIComponent(req.id)}`, [req.id])
  const gate = useFetch<ReviewGatePayload>(`/api/requirement/review-gate?id=${encodeURIComponent(req.id)}`, [req.id])
  const [refreshing, setRefreshing] = useState(false)
  const [showDiff, setShowDiff] = useState(false)
  const [actionError, setActionError] = useState<string | null>(null)
  const [syncing, setSyncing] = useState(false)
  const [syncPayload, setSyncPayload] = useState<SyncBasePayload | null>(null)
  const scope = data?.branchScope || null
  const review = data?.review || null
  const stats = reviewStats(review)
  const canScan = Boolean(scope?.repos?.length)
  const refreshScan = async () => {
    if (!canScan || refreshing) return
    setRefreshing(true)
    setActionError(null)
    try {
      const payload = await postForm<CodeReviewPayload & { sync?: SyncBasePayload }>("/api/requirement/code-review", { reqId: req.id })
      if (payload.sync?.results?.length) setSyncPayload(payload.sync)
      refresh()
      gate.refresh()
    } catch (err) {
      setActionError(err instanceof Error ? err.message : String(err))
    } finally {
      setRefreshing(false)
    }
  }
  const syncBase = async () => {
    if (!canScan || syncing) return
    setSyncing(true)
    setActionError(null)
    try {
      const payload = await postForm<SyncBasePayload>("/api/requirement/sync-base", { reqId: req.id })
      setSyncPayload(payload)
      refresh()
      gate.refresh()
    } catch (err) {
      setActionError(err instanceof Error ? err.message : String(err))
    } finally {
      setSyncing(false)
    }
  }
  return <section id="code-review" className="react-panel react-code-review-panel"><PanelHead kicker="Code Review Gate" title="代码审查门禁" chip={gate.data?.gate?.label || (gate.loading ? "loading" : "gate")} />
    <div className={`react-review-gate react-review-gate-${gate.data?.gate?.status || "unknown"}`}><strong>{gate.data?.gate?.label || "读取中"}</strong><span>{gate.data?.gate?.reason || "自测中推进到测试中前必须完成代码审查门禁。"}</span>{gate.data?.gate?.source ? <em>source: {gate.data.gate.source}</em> : null}</div>
    {gate.data?.gate?.actions?.length ? <div className="react-drive-blockers"><strong>门禁动作</strong><ul>{gate.data.gate.actions.map((item) => <li key={item}>{item}</li>)}</ul></div> : null}
    {gate.error ? <p className="react-effort-error">门禁加载失败：{gate.error}</p> : null}
    <div className="react-actions"><button onClick={refreshScan} disabled={!canScan || refreshing}><RefreshCw size={15} className={refreshing ? "react-spin" : ""} />{review ? "刷新代码差异" : "生成代码差异"}</button><button onClick={syncBase} disabled={!canScan || syncing} title="fetch 远端生产分支并 reset 本地 master/production 到最新,工作区有改动时自动跳过"><RefreshCw size={15} className={syncing ? "react-spin" : ""} />{syncing ? "同步中…" : "同步生产基线"}</button>{review ? <button onClick={() => setShowDiff((v) => !v)}>{showDiff ? "隐藏 unified diff" : "展示 unified diff"}</button> : null}<a href={`/requirement-diff?id=${encodeURIComponent(req.id)}&base=origin%2Fmaster`}><GitBranch size={15} />打开分支差异页</a></div>
    {syncPayload?.results?.length ? <details className="react-review-repo" open><summary><span><strong>生产基线同步</strong><em>{formatDateTime(syncPayload.generatedAt)}</em></span><span className="react-review-size">{syncPayload.results.filter((r) => r.ok).length}/{syncPayload.results.length} ok</span></summary><div className="react-table-wrap react-code-file-wrap"><table className="react-code-file-table"><thead><tr><th>应用</th><th>本地分支</th><th>状态</th><th>before</th><th>after</th><th>说明</th></tr></thead><tbody>{syncPayload.results.map((r) => <tr key={r.repoName}><td><strong>{r.repoName}</strong></td><td><code>{r.localBranch || r.baseRef || "-"}</code></td><td><span className={`react-merge-status ${r.ok ? "merged" : "conflict"}`}>{r.status}</span></td><td><code>{r.beforeCommit || "-"}</code></td><td><code>{r.afterCommit || "-"}</code></td><td>{r.message}{r.warnings?.length ? <em>{r.warnings.join("; ")}</em> : null}</td></tr>)}</tbody></table></div></details> : null}
    {error ? <p className="react-effort-error">加载失败：{error}</p> : null}{actionError ? <p className="react-effort-error">刷新失败：{actionError}</p> : null}
    {loading ? <LoadingCard label="正在加载代码差异…" /> : <>
      <div className="react-branch-scope">
        {scope?.repos?.length ? scope.repos.map((repo) => <div key={`${repo.repoName}-${repo.branches?.join("/")}`} className="react-branch-card"><strong>{repo.repoName}</strong><span>{repo.role || "repo"}</span><code>{repo.branches?.join(" / ") || "未指定分支"}</code><em>{repo.baseRef || (repo.role === "前端" ? "origin/production" : "origin/master")}</em></div>) : <p className="react-muted">未找到 <code>branches.json</code>，无法生成代码差异；请先运行 <code>req-branches-update</code>。</p>}
      </div>
      {review ? <div className="react-review-summary"><span>{stats.repoCount} repo/branch</span><span>{stats.fileCount} files</span><span className="react-review-add">+{stats.additions}</span><span className="react-review-del">-{stats.deletions}</span><span>更新 {formatDateTime(review.updatedAt)}</span></div> : <p className="react-muted">暂无 <code>code-review.json</code> 快照；点击“生成代码差异”后会读取本地 git diff 并写回需求目录。</p>}
      {review?.repos?.map((repo, index) => <details key={`${repo.repoName}-${repo.branch}-${index}`} className="react-review-repo" open={index === 0}>
        <summary><span><strong>{repo.repoName}</strong><em>{repo.branch}</em></span><span className="react-review-size">+{repo.additions || 0} / -{repo.deletions || 0}</span></summary>
        <div className="react-card-meta"><span>base {repo.baseRef || review.baseRef}</span><span>current {repo.currentBranch || "-"}</span><span>{repo.dirty ? "工作区有未提交改动" : "工作区干净"}</span><span>{repo.projectPath || "path n/a"}</span></div>
        {repo.error ? <p className="react-effort-error">{repo.error}</p> : null}
        {repo.warnings?.length ? <div className="react-drive-blockers"><strong>Warnings</strong><ul>{repo.warnings.map((w) => <li key={w}>{w}</li>)}</ul></div> : null}
        {repo.commits?.length ? <details className="react-review-commits"><summary>提交列表（{repo.commits.length}）</summary><pre>{repo.commits.join("\n")}</pre></details> : null}
        {repo.files?.length ? <div className="react-table-wrap react-code-file-wrap"><table className="react-code-file-table"><thead><tr><th>文件</th><th>状态</th><th>增删</th><th>风险</th></tr></thead><tbody>{repo.files.map((file) => <tr key={file.path}><td><code>{file.path}</code></td><td>{file.status}</td><td><span className="react-review-add">+{file.additions}</span> / <span className="react-review-del">-{file.deletions}</span></td><td>{file.riskTags?.length ? file.riskTags.map((tag) => <span key={tag} className="react-review-tag">{tag}</span>) : <span className="react-muted">-</span>}</td></tr>)}</tbody></table></div> : <p className="react-muted">没有文件级差异。</p>}
        {showDiff && repo.diff ? <pre className="react-diff-preview">{repo.diff}{repo.diffTruncated ? "\n… diff 已截断" : ""}</pre> : null}
      </details>)}
    </>}
  </section>
}

function MergeBranchPanel({ req }: { req: Requirement }) {
  const optionData = useFetch<MergeOptionsPayload>(`/api/requirement/merge-options?id=${encodeURIComponent(req.id)}`, [req.id])
  const [payloads, setPayloads] = useState<MergeBranchPayload[]>([])
  const [frontendBranch, setFrontendBranch] = useState("")
  const [backendBranch, setBackendBranch] = useState("")
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  useEffect(() => {
    const options = optionData.data?.options
    if (!options) return
    setFrontendBranch(options.frontend?.defaultValue || "")
    setBackendBranch(options.backend?.defaultValue || "")
  }, [optionData.data?.generatedAt, req.id])
  const runMerge = async () => {
    if (loading) return
    const jobs: Array<{ kind: MergeRepoKind; branch: string; target: string }> = []
    const frontendOption = optionData.data?.options.frontend.options.find((item) => item.value === frontendBranch)
    const backendOption = optionData.data?.options.backend.options.find((item) => item.value === backendBranch)
    if (frontendBranch && frontendOption) jobs.push({ kind: "frontend", branch: frontendBranch, target: frontendOption.target })
    if (backendBranch && backendOption) jobs.push({ kind: "backend", branch: backendBranch, target: backendOption.target })
    if (!jobs.length) { setError("请先选择前端或后端目标分支"); return }
    setLoading(true)
    setError(null)
    try {
      const next: MergeBranchPayload[] = []
      for (const job of jobs) {
        next.push(await postForm<MergeBranchPayload>("/api/requirement/merge-branch", { reqId: req.id, repoKind: job.kind, targetBranch: job.branch, target: job.target }))
      }
      setPayloads(next)
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err))
    } finally {
      setLoading(false)
    }
  }
  const results = payloads.flatMap((item) => item.results || [])
  const conflictCount = results.filter((item) => item.status === "conflict").length
  const mergedCount = results.filter((item) => item.status === "merged" || item.status === "upToDate").length
  const latestAt = Math.max(0, ...payloads.map((item) => item.generatedAt || 0))
  const chip = payloads.length ? payloads.map((item) => item.status).join(" / ") : "选择环境分支"
  const selectBranch = (kind: MergeRepoKind, value: string, onChange: (v: string) => void, options?: MergeKindOptions) => <label className="react-merge-select"><span>{kind === "frontend" ? "前端分支" : "后端分支"}</span><select value={value} onChange={(e) => onChange(e.target.value)} disabled={loading || optionData.loading || !options?.options?.length}><option value="">不合并</option>{(options?.options || []).map((item) => <option key={`${kind}-${item.value}`} value={item.value}>{item.label}</option>)}</select></label>
  return <section className="react-panel react-merge-panel"><PanelHead kicker="Branch Merge" title="合并到测试 / UAT" chip={chip} />
    <p className="react-muted">选择要合并的环境分支后执行；未选择的前端/后端不会合并。自测中默认选 test，测试中默认选 UAT 分支，其他状态默认不选择。</p>
    {optionData.error ? <p className="react-effort-error">分支选项加载失败：{optionData.error}</p> : null}
    <div className="react-merge-select-grid">{selectBranch("frontend", frontendBranch, setFrontendBranch, optionData.data?.options.frontend)}{selectBranch("backend", backendBranch, setBackendBranch, optionData.data?.options.backend)}</div>
    <div className="react-actions"><button onClick={runMerge} disabled={loading || optionData.loading || (!frontendBranch && !backendBranch)}><GitMerge size={15} />{loading ? "合并中…" : "合并所选分支"}</button><a href={`/requirement-merge?id=${encodeURIComponent(req.id)}`}><AlertTriangle size={15} />查看冲突 / 合并状态</a>{latestAt ? <span className="react-muted">更新 {formatDateTime(latestAt)}</span> : null}</div>
    {error ? <p className="react-effort-error">合并失败：{error}</p> : null}
    {payloads.length ? <div className="react-review-summary"><span>{results.length} repo/branch</span><span className="react-review-add">{mergedCount} merged</span><span className={conflictCount ? "react-review-del" : undefined}>{conflictCount} conflict</span><span>{[frontendBranch, backendBranch].filter(Boolean).join(" / ") || "-"}</span></div> : null}
    {results.length ? <div className="react-table-wrap react-prod-mr-wrap"><table className="react-code-file-table react-prod-mr-table"><thead><tr><th>应用</th><th>源分支</th><th>目标</th><th>状态</th><th>冲突 / 位置</th></tr></thead><tbody>{results.map((item, index) => <tr key={`${item.repoName}-${item.sourceBranch}-${item.target}-${index}`}><td><strong>{item.repoName}</strong><span>{item.role || "repo"}</span></td><td><code>{item.sourceBranch}</code></td><td><code>{item.targetBranch || item.target}</code></td><td><span className={`react-merge-status ${item.status}`}>{mergeStatusLabel(item.status)}</span>{item.message ? <em>{item.message}</em> : null}</td><td>{item.status === "conflict" ? <a href={`/requirement-merge?id=${encodeURIComponent(req.id)}&target=${encodeURIComponent(String(item.target))}`}>{item.conflictFiles?.length || 0} 个冲突文件</a> : item.worktreePath ? <code>{item.worktreePath}</code> : <span className="react-muted">-</span>}</td></tr>)}</tbody></table></div> : payloads.length ? <p className="react-muted">未返回合并结果，请检查 <code>branches.json</code>。</p> : null}
  </section>
}

function mergeStatusLabel(status: string): string {
  if (status === "merged") return "已合并"
  if (status === "upToDate") return "已最新"
  if (status === "conflict") return "冲突"
  if (status === "skipped") return "跳过"
  if (status === "idle") return "空闲"
  if (status === "pending") return "待检查"
  if (status === "failed") return "失败"
  return status
}

function ProdMrPanel({ req }: { req: Requirement }) {
  const [payload, setPayload] = useState<ProdMrPayload | null>(null)
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [copiedKey, setCopiedKey] = useState<string | null>(null)
  const createMrs = async () => {
    if (loading) return
    setLoading(true)
    setError(null)
    try {
      const next = await postForm<ProdMrPayload>("/api/requirement/prod-mrs", { reqId: req.id })
      setPayload(next)
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err))
    } finally {
      setLoading(false)
    }
  }
  const copyMrLink = async (item: ProdMrResult, index: number) => {
    if (!item.webUrl) return
    const key = `${item.repoName}-${item.sourceBranch}-${index}`
    try {
      await navigator.clipboard.writeText(item.webUrl)
      setCopiedKey(key)
      window.setTimeout(() => setCopiedKey((current) => current === key ? null : current), 1600)
    } catch (err) {
      setError(err instanceof Error ? err.message : "复制失败，请手动复制链接")
    }
  }
  const results = payload?.results || []
  const okCount = results.filter((item) => item.webUrl).length
  const noDiffCount = results.filter((item) => item.status === "no_diff").length
  const chip = payload ? `${okCount}/${results.length} ready${noDiffCount ? ` · ${noDiffCount} 无差异` : ""}` : "test/uat → prod"
  return <section className="react-panel react-prod-mr-panel"><PanelHead kicker="Production MR" title="生产 MR" chip={chip} />
    <p className="react-muted">按 <code>branches.json</code> 中每个应用的每个需求分支创建生产 MR；后端目标 <code>master</code>，前端目标 <code>production</code>。生成前先比对需求分支与生产分支的差异，<strong>无差异的仓库自动跳过 MR</strong>。再次点击会重新扫描并复用已存在的 open MR。</p>
    <div className="react-actions"><button onClick={createMrs} disabled={loading}><GitBranch size={15} />{loading ? "生成中…" : payload ? "重新生成 / 复用 MR" : "生成生产 MR 链接"}</button>{payload?.generatedAt ? <span className="react-muted">更新 {formatDateTime(payload.generatedAt)}</span> : null}</div>
    {error ? <p className="react-effort-error">生成失败：{error}</p> : null}
    {results.length ? <div className="react-table-wrap react-prod-mr-wrap"><table className="react-code-file-table react-prod-mr-table"><thead><tr><th>应用</th><th>源分支</th><th>目标</th><th>状态</th><th>生产差异</th><th>MR</th><th>复制</th></tr></thead><tbody>{results.map((item, index) => { const key = `${item.repoName}-${item.sourceBranch}-${index}`; return <tr key={key}><td><strong>{item.repoName}</strong><span>{item.role || "repo"}</span></td><td><code>{item.sourceBranch}</code></td><td><code>{item.targetBranch}</code></td><td><span className={`react-prod-mr-status ${item.status}`}>{item.status === "created" ? "新建" : item.status === "reused" ? "复用" : item.status === "skipped" ? "跳过" : item.status === "no_diff" ? "无差异" : "失败"}</span>{item.error ? <em>{item.error}</em> : null}</td><td>{item.diffFiles != null ? (item.diffFiles === 0 ? <span className="react-muted">无</span> : <span><span className="react-review-add">+{item.diffAdditions ?? 0}</span> / <span className="react-review-del">-{item.diffDeletions ?? 0}</span> <span className="react-muted">({item.diffFiles})</span></span>) : <span className="react-muted">-</span>}</td><td>{item.webUrl ? <a href={item.webUrl} target="_blank" rel="noopener noreferrer">!{item.iid || "MR"}</a> : <span className="react-muted">-</span>}</td><td>{item.webUrl ? <button type="button" className="react-copy-link-btn" onClick={() => copyMrLink(item, index)}>{copiedKey === key ? "已复制" : "复制链接"}</button> : <span className="react-muted">-</span>}</td></tr> })}</tbody></table></div> : payload ? <p className="react-muted">未生成 MR，请检查 <code>branches.json</code> 是否包含应用和分支。</p> : null}
  </section>
}

function RequirementDiffPage() {
  const params = new URLSearchParams(window.location.search)
  const reqId = params.get("id") || params.get("reqId") || ""
  const initialBase = params.get("base") || "origin/master"
  const requirements = RequirementsData()
  const req = requirements.data?.requirements.find((r) => r.id === reqId)
  const [baseRef, setBaseRef] = useState(initialBase)
  const [loadingDiff, setLoadingDiff] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [review, setReview] = useState<CodeReviewSnapshot | null>(null)
  const files = useMemo(() => parseUnifiedDiffFiles(review), [review])
  const stats = reviewStats(review)
  const [activeKey, setActiveKey] = useState("")
  useEffect(() => {
    if (!reqId) return
    let cancelled = false
    setLoadingDiff(true)
    setError(null)
    postForm<MasterDiffPayload>("/api/requirement/master-diff", { reqId, baseRef })
      .then((payload) => { if (!cancelled) setReview(payload.review || null) })
      .catch((err) => { if (!cancelled) setError(err instanceof Error ? err.message : String(err)) })
      .finally(() => { if (!cancelled) setLoadingDiff(false) })
    return () => { cancelled = true }
  }, [reqId, baseRef])
  useEffect(() => {
    if (!files.length) { setActiveKey(""); return }
    const exists = files.some((item) => `${item.repo.repoName}:${item.file.path}` === activeKey)
    if (!exists) setActiveKey(`${files[0].repo.repoName}:${files[0].file.path}`)
  }, [files, activeKey])
  const activeIndex = Math.max(0, files.findIndex((item) => `${item.repo.repoName}:${item.file.path}` === activeKey))
  const scrollToFile = (key: string) => {
    setActiveKey(key)
    document.getElementById(diffDomId(key))?.scrollIntoView({ behavior: "smooth", block: "start" })
  }
  const changeBase = (next: string) => {
    setBaseRef(next)
    const q = new URLSearchParams(window.location.search)
    q.set("id", reqId)
    q.set("base", next)
    window.history.replaceState(null, "", `/requirement-diff?${q.toString()}`)
  }
  const title = req?.title || reqId || "分支差异"
  return <PageChrome icon={<GitBranch size={15} />} eyebrow="Diff" title={title} description="按需求分支和指定基准分支生成代码差异，左侧选择文件，中间查看改动内容。" actions={<><a href={`/requirement?id=${encodeURIComponent(reqId)}`}><ArrowLeft size={15} />返回需求</a><button onClick={() => changeBase(baseRef)} disabled={loadingDiff}><RefreshCw size={15} className={loadingDiff ? "react-spin" : ""} />刷新 diff</button></>}>
    <section className="react-diff-shell">
      <aside className="react-diff-sidebar"><div className="react-diff-compare"><span>Compare</span><select value={baseRef} onChange={(e) => changeBase(e.target.value)}><option value="origin/master">origin/master</option><option value="origin/production">origin/production</option><option value="master">master</option><option value="production">production</option></select><em>and latest version</em></div><label className="react-diff-search"><Search size={14} /><input placeholder="Search files (Ctrl+P)" onChange={(e) => { const hit = files.find((f) => f.file.path.toLowerCase().includes(e.target.value.toLowerCase())); if (hit && e.target.value) scrollToFile(`${hit.repo.repoName}:${hit.file.path}`) }} /></label><div className="react-diff-file-list">{[...new Set(review?.repos?.map((r) => r.repoName) || [])].map((repoName) => {
          const repo = review?.repos?.find((r) => r.repoName === repoName)
          const repoFiles = files.filter((f) => f.repo.repoName === repoName)
          return <details key={repoName} className="react-diff-repo-group" open>
            <summary><strong>{repoName}</strong><em>base: {repo?.baseRef || baseRef}</em></summary>
            {repoFiles.length ? repoFiles.map((item) => { const key = `${item.repo.repoName}:${item.file.path}`; return <button key={key} className={key === activeKey ? "active" : ""} onClick={() => scrollToFile(key)}><FileCode2 size={14} /><span><strong>{shortFileName(item.file.path)}</strong><small>{compactPath(item.file.path)}</small></span><em><b>+{item.file.additions}</b> <i>-{item.file.deletions}</i></em></button> }) : <p className="react-muted" style={{padding: '8px'}}>暂无文件差异</p>}
          </details>
        })}</div></aside>
      <main className="react-diff-main"><div className="react-diff-toolbar"><div><strong>{stats.fileCount} files</strong><span className="react-review-add">+{stats.additions}</span><span className="react-review-del">-{stats.deletions}</span>{review?.updatedAt ? <span>生成 {formatDateTime(review.updatedAt)}</span> : null}</div><span>{activeIndex + 1}/{Math.max(files.length, 1)}</span></div>{requirements.error ? <ErrorCard error={requirements.error} /> : error ? <ErrorCard error={error} /> : loadingDiff ? <LoadingCard label="正在生成分支差异…" /> : files.length === 0 ? <EmptyCard>没有可展示的文件级差异。</EmptyCard> : files.map((item) => { const key = `${item.repo.repoName}:${item.file.path}`; return <article key={key} id={diffDomId(key)} className="react-diff-file-card"><header><div><FileCode2 size={16} /><strong>{item.repo.repoName}/{item.file.path}</strong><em className="react-diff-base-label">vs {item.repo.baseRef || review?.baseRef || "?"}</em></div><span><b>+{item.file.additions}</b><i>-{item.file.deletions}</i></span></header>{item.lines.length ? <table className="react-diff-code"><tbody>{item.lines.map((line, i) => <tr key={i} className={`react-diff-line-${line.type}`}><td>{line.oldNo}</td><td>{line.newNo}</td><td><code>{line.type === "add" ? "+" : line.type === "del" ? "-" : line.type === "hunk" ? "" : " "}{line.text || " "}</code></td></tr>)}</tbody></table> : <pre className="react-diff-preview">{item.diff || "该文件 diff 已截断或为空。"}</pre>}</article> })}</main>
    </section>
  </PageChrome>
}

function RequirementMergePage() {
  const params = new URLSearchParams(window.location.search)
  const reqId = params.get("id") || params.get("reqId") || ""
  const targetParam = params.get("target") || ""
  const targetFilter = targetParam === "test" || targetParam === "uat" ? targetParam : ""
  const requirements = RequirementsData()
  const req = requirements.data?.requirements.find((r) => r.id === reqId)
  const [target, setTarget] = useState<MergeTarget | "">(targetFilter as MergeTarget | "")
  const [payload, setPayload] = useState<MergeBranchPayload | null>(null)
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const loadStatus = async (nextTarget = target) => {
    if (!reqId) return
    setLoading(true)
    setError(null)
    try {
      const query = new URLSearchParams({ id: reqId })
      if (nextTarget) query.set("target", nextTarget)
      const next = await fetchJson<MergeBranchPayload>(`/api/requirement/merge-status?${query.toString()}`)
      setPayload(next)
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err))
    } finally {
      setLoading(false)
    }
  }
  useEffect(() => { loadStatus(targetFilter as MergeTarget | "") }, [reqId])
  const changeTarget = (next: MergeTarget | "") => {
    setTarget(next)
    const q = new URLSearchParams(window.location.search)
    q.set("id", reqId)
    if (next) q.set("target", next); else q.delete("target")
    window.history.replaceState(null, "", `/requirement-merge?${q.toString()}`)
    loadStatus(next)
  }
  const results = payload?.results || []
  const conflicts = results.filter((item) => item.status === "conflict")
  const title = req?.title || reqId || "分支合并"
  return <PageChrome icon={<GitMerge size={15} />} eyebrow="Merge" title={title} description="查看 Agent Panel 自动合并结果；冲突 worktree 会保留，人工或 agent 可按返回路径继续处理。" actions={<><a href={`/requirement?id=${encodeURIComponent(reqId)}`}><ArrowLeft size={15} />返回需求</a><button onClick={() => loadStatus()} disabled={loading}><RefreshCw size={15} className={loading ? "react-spin" : ""} />刷新状态</button></>}>
    <section className="react-panel react-merge-panel"><PanelHead kicker="Merge Status" title="环境合并状态" chip={payload?.status || "status"} />
      <div className="react-tab-row"><button className={target === "" ? "active" : ""} onClick={() => changeTarget("")}>全部</button><button className={target === "test" ? "active" : ""} onClick={() => changeTarget("test")}>test</button><button className={target === "uat" ? "active" : ""} onClick={() => changeTarget("uat")}>UAT</button></div>
      {requirements.error ? <ErrorCard error={requirements.error} /> : error ? <ErrorCard error={error} /> : loading ? <LoadingCard label="正在读取合并状态…" /> : results.length === 0 ? <EmptyCard>暂无合并状态；请先在需求详情页触发合并。</EmptyCard> : <>
        <div className="react-review-summary"><span>{results.length} repo/branch</span><span className="react-review-del">{conflicts.length} conflict</span><span>{payload?.generatedAt ? formatDateTime(payload.generatedAt) : "-"}</span></div>
        {conflicts.length ? <div className="react-drive-blockers"><strong>冲突处理提示</strong><ul><li>人工处理：进入下方 <code>worktreePath</code> 后解决冲突、提交并推送到目标分支。</li><li>Agent 处理：把接口返回的 <code>repoName / targetBranch / worktreePath / conflictFiles</code> 交给 agent，agent 可继续在该 worktree 解决冲突。</li></ul></div> : null}
        <div className="react-card-list react-merge-result-list">{results.map((item, index) => <article key={`${item.repoName}-${item.target}-${item.sourceBranch}-${index}`} className={`react-list-card react-merge-card react-merge-card-${item.status}`}><div><span className="react-card-id">{item.repoName} · {item.target}</span><h3>{item.sourceBranch} → {item.targetBranch || item.target}</h3><p>{item.message || mergeStatusLabel(item.status)}</p><div className="react-card-meta"><span>{mergeStatusLabel(item.status)}</span><span>{item.role || "repo"}</span><span>{item.projectPath || "path n/a"}</span></div>{item.worktreePath ? <code className="react-command">{item.worktreePath}</code> : null}{item.conflictFiles?.length ? <details className="react-review-commits" open><summary>冲突文件（{item.conflictFiles.length}）</summary><pre>{item.conflictFiles.join("\n")}</pre></details> : null}{item.warnings?.length ? <div className="react-drive-blockers"><strong>Warnings</strong><ul>{item.warnings.map((w) => <li key={w}>{w}</li>)}</ul></div> : null}</div><div className="react-card-side"><span className={`react-merge-status ${item.status}`}>{mergeStatusLabel(item.status)}</span><span className="react-muted">{item.targetBranch || "-"}</span></div></article>)}</div>
      </>}
    </section>
  </PageChrome>
}

function markdownPreview(content: string, expanded: boolean, max = 2600): { text: string; truncated: boolean } {
  const clean = (content || "").trim()
  if (!clean) return { text: "", truncated: false }
  if (expanded || clean.length <= max) return { text: clean, truncated: false }
  return { text: `${clean.slice(0, max)}\n…`, truncated: true }
}

function RequirementDocPanel({ id, req, docType, title, kicker, description, path, actions }: { id: string; req: Requirement; docType: string; title: string; kicker: string; description: string; path?: string; actions?: React.ReactNode }) {
  const [expanded, setExpanded] = useState(false)
  const doc = useFetch<RequirementDocPayload>(req.reqDir ? `/api/requirement/doc?id=${encodeURIComponent(req.id)}&file=${encodeURIComponent(docType)}` : null, [req.id, docType])
  const content = doc.data?.content || ""
  const preview = markdownPreview(content, expanded)
  const chip = doc.loading ? "loading" : doc.data?.exists ? doc.data.file : path ? "path only" : "missing"
  return <section id={id} className="react-panel react-doc-panel"><PanelHead kicker={kicker} title={title} chip={chip} />
    <p className="react-muted">{description}</p>
    <div className="react-actions">{actions}{path || doc.data?.path ? <code className="react-doc-path">{doc.data?.path || path}</code> : null}</div>
    {doc.error ? <p className="react-effort-error">加载失败：{doc.error}</p> : doc.loading ? <LoadingCard label="正在加载文档…" /> : preview.text ? <><pre className="react-doc-preview">{preview.text}</pre>{preview.truncated || expanded ? <div className="react-actions"><button type="button" onClick={() => setExpanded((v) => !v)}>{expanded ? "收起" : "展开全文"}</button></div> : null}</> : <p className="react-muted">暂无内容。可在需求澄清阶段生成或更新该文档。</p>}
  </section>
}

function RequirementFilesPanel({ req }: { req: Requirement }) {
  const rows = [
    ["alignment.md", req.alignmentPath || (req.reqDir ? `${req.reqDir}/alignment.md` : "-")],
    ["background.md", req.backgroundPath || (req.reqDir ? `${req.reqDir}/background.md` : "-")],
    ["memory.md", req.memoryPath || (req.reqDir ? `${req.reqDir}/memory.md` : "-")],
    ["impact.md", req.impactPath || (req.reqDir ? `${req.reqDir}/impact.md` : "-")],
    ["branch.md", req.branchPath || (req.reqDir ? `${req.reqDir}/branch.md` : "-")],
    ["test.md", req.testPath || (req.reqDir ? `${req.reqDir}/test.md` : "-")],
    ["release-manifest.md", req.releaseManifestPath || (req.reqDir ? `${req.reqDir}/release-manifest.md` : "-")],
    ["release-check.md", req.releaseCheckPath || (req.reqDir ? `${req.reqDir}/release-check.md` : "-")],
    ["experience-summary.md", req.experienceSummaryPath || (req.reqDir ? `${req.reqDir}/experience-summary.md` : "-")],
    ["notes.md", req.notesPath || (req.reqDir ? `${req.reqDir}/notes.md` : "-")],
  ]
  return <section className="react-panel"><PanelHead kicker="Files" title="需求文件" />
    <div className="react-meta-grid">{rows.flatMap(([name, value]) => [<span key={`${name}-n`}>{name}</span>, <span key={`${name}-v`}>{value}</span>])}</div>
  </section>
}

function RequirementPage() {
  const id = new URLSearchParams(window.location.search).get("id") || new URLSearchParams(window.location.search).get("reqId") || ""
  const { data, error, loading, refresh } = RequirementsData()
  const req = data?.requirements.find((r) => r.id === id)
  const [note, setNote] = useState("")
  const [status, setStatus] = useState<ReqStatus | "">("")
  const [category, setCategory] = useState<ReqCategory | "">("")
  const [savingStatus, setSavingStatus] = useState(false)
  const [savingCategory, setSavingCategory] = useState(false)
  const [statusMessage, setStatusMessage] = useState<string | null>(null)
  const [command, setCommand] = useState("")
  const submitStatus = async () => {
    if (!req || !status || savingStatus) return
    setSavingStatus(true)
    setStatusMessage(null)
    try {
      await postForm("/api/requirement/status", { reqId: req.id, status, note })
      setStatusMessage("状态已保存")
      refresh()
    } catch (err) {
      setStatusMessage(`状态保存失败：${err instanceof Error ? err.message : String(err)}`)
    } finally {
      setSavingStatus(false)
    }
  }
  const submitCategory = async () => {
    if (!req || !category || savingCategory) return
    setSavingCategory(true)
    try { await postForm("/api/requirement/category", { reqId: req.id, category }); refresh() }
    finally { setSavingCategory(false) }
  }
  const newSession = async () => {
    if (!req) return
    const res = await postForm<{ command: string }>("/api/requirement/new-session", { reqId: req.id })
    setCommand(res.command)
  }
  return <PageChrome icon={<GitBranch size={15} />} eyebrow="Requirement" title={req?.title || id || "Requirement"} description={req?.description || "需求详情、状态流转、上线清单、业务背景、经验总结与关联 pi session。"} actions={<><a href="/projects"><ArrowLeft size={15} />返回需求列表</a>{req ? <a href="#release-manifest"><AlertTriangle size={15} />上线清单</a> : null}{req ? <a href="#business-background"><Library size={15} />业务背景</a> : null}{req ? <a href="#experience-summary"><Lightbulb size={15} />经验总结</a> : null}{req ? <a href="#code-review"><GitBranch size={15} />代码差异</a> : null}</>}>
    {error ? <ErrorCard error={error} /> : loading ? <LoadingCard /> : !req ? <EmptyCard>需求不存在：{id}</EmptyCard> : <div className="react-detail-grid">
      <section className="react-panel"><PanelHead kicker="Overview" title="需求信息" chip={statusPill(req.status)} /><div className="react-meta-grid"><span>Req ID <code>{req.id}</code></span><span>项目 {projectsOf(req)}</span><span>创建 {formatDate(req.createdAt)}</span><span>更新 {relAge(req.updatedAt)}</span><span>目录 {req.reqDir || "-"}</span><span>类别 {req.category || "需求"}</span></div><p className="react-detail-desc">{req.description || "暂无描述"}</p></section>
      <RequirementDocPanel id="release-manifest" req={req} docType="release-manifest" title="上线清单" kicker="Release Manifest" path={req.releaseManifestPath} description="贯穿需求全流程维护：集中展示 DB 表、配置、Topic/Group、Job、开关、接口和上线人工动作，避免发布时遗漏。" actions={<><a href={`/api/requirement/context?id=${encodeURIComponent(req.id)}&intent=release-check&tokens=req.releaseManifest,req.configChanges,req.branchScope&budget=3000`} target="_blank" rel="noreferrer">清单上下文</a></>} />
      <RequirementDocPanel id="business-background" req={req} docType="background" title="业务背景文档" kicker="Business Context" path={req.backgroundPath} description="给不熟悉业务的开发/测试快速理解背景，也作为后续经验总结的参考材料。" actions={<><a href="/business-knowledge"><Library size={15} />业务知识库</a><a href={`/api/requirement/context?id=${encodeURIComponent(req.id)}&intent=clarification&budget=3000`} target="_blank" rel="noreferrer">澄清上下文</a></>} />
      <RequirementDocPanel id="experience-summary" req={req} docType="experience-summary" title="经验总结闭环" kicker="Capability Evolution" path={req.experienceSummaryPath} description="记录本次需求暴露出的业务知识、经验、skill 和流程改进，让下一次需求执行更快更稳。" actions={<><a href="/experiences"><Lightbulb size={15} />经验库</a><a href={`/api/requirement/context?id=${encodeURIComponent(req.id)}&intent=experience-summary&budget=3000`} target="_blank" rel="noreferrer">总结上下文</a></>} />
      <CodeReviewPanel req={req} />
      <MergeBranchPanel req={req} />
      <ProdMrPanel req={req} />
      <OnesPanel req={req} onSaved={refresh} />
      <section className="react-panel"><PanelHead kicker="Status" title="状态切换" /><p className="react-muted">新版流程：需求澄清 → 开发中 → 自测中 → 测试中 → 经验总结 → 已完成。自测中推进到测试中前必须通过代码审查门禁，旧状态会自动兼容映射。</p><div className="react-inline-form"><select value={status} onChange={(e) => { setStatus(e.target.value as ReqStatus); setStatusMessage(null) }}><option value="">选择状态</option>{REQ_STATUSES.map((s) => <option key={s} value={s}>{s}</option>)}</select><input value={note} onChange={(e) => setNote(e.target.value)} placeholder="备注" /><button onClick={submitStatus} disabled={!status || savingStatus}>{savingStatus ? "保存中…" : "保存状态"}</button></div>{statusMessage ? <p className={statusMessage.startsWith("状态保存失败") ? "react-effort-error" : "react-save-hint"}>{statusMessage}</p> : null}<div className="react-inline-form react-category-form"><label>类别</label><select value={category} onChange={(e) => setCategory(e.target.value as ReqCategory)}><option value="">{req.category ?? "需求"}</option>{REQ_CATEGORIES.map((c) => <option key={c} value={c}>{c}</option>)}</select><button onClick={submitCategory} disabled={!category || savingCategory}>{savingCategory ? "保存中…" : "保存类别"}</button></div></section>
      <section className="react-panel"><PanelHead kicker="Sessions" title="关联 Session" chip={`${req.sessionIds?.length || 0}`} />{req.sessionIds?.length ? <SessionChipList sessionIds={req.sessionIds} /> : <p className="react-muted">暂无关联 session。</p>}<div className="react-actions"><button onClick={newSession}>生成新 pi session 命令</button></div>{command ? <code className="react-command">{command}</code> : null}</section>
      <RequirementFilesPanel req={req} />
    </div>}
  </PageChrome>
}

function modelOptions(pi: PiConfigSummary | null): string[] {
  return (pi?.providers || []).flatMap((p) => p.models.map((m) => `${p.id}/${m.modelId}`))
}

function SettingsPage() {
  const dashboard = useFetch<ConfigPayload>("/api/config")
  const piConfig = useFetch<PiConfigSummary>("/api/pi-config")
  const cainiaoStatus = useFetch<CainiaoMockStatus>("/api/cainiao-mock/status")
  const [scanRootsText, setScanRootsText] = useState("")
  const [draft, setDraft] = useState<ConfigPayload>({})
  const [fileContent, setFileContent] = useState("")
  const [fileSnapshot, setFileSnapshot] = useState<PiConfigFileSnapshot | null>(null)
  const [savedHint, setSavedHint] = useState<string | null>(null)
  useEffect(() => { if (dashboard.data) { setDraft(dashboard.data); setScanRootsText((dashboard.data.requirementScanRoots || []).join("\n")) } }, [dashboard.data])
  useEffect(() => { fetchJson<PiConfigFileSnapshot>("/api/pi-config/file?file=settings").then((s) => { setFileSnapshot(s); setFileContent(s.content) }).catch(() => undefined) }, [savedHint])
  const options = modelOptions(piConfig.data)
  const saveScanRoots = async () => { await postJson("/api/config", { requirementScanRoots: scanRootsText.split(/[\n,]/).map((v) => v.trim()).filter(Boolean) }); dashboard.refresh(); setSavedHint("扫描目录已保存") }
  const saveModels = async () => { await postJson("/api/config", draft); dashboard.refresh(); setSavedHint("配置已保存") }
  const saveCainiaoMock = async () => { await postJson("/api/config", { cainiaoMockEnabled: draft.cainiaoMockEnabled, cainiaoMockPort: draft.cainiaoMockPort }); dashboard.refresh(); cainiaoStatus.refresh(); setSavedHint("菜鸟打印 Mock 已保存") }
  const savePiFile = async () => { const next = await postJson<PiConfigFileSnapshot>("/api/pi-config/file", { file: "settings", content: fileContent }); setFileSnapshot(next); setFileContent(next.content); setSavedHint("Pi settings.json 已保存") }
  const selectModel = (key: keyof ConfigPayload) => <select value={String(draft[key] || "")} onChange={(e) => setDraft({ ...draft, [key]: e.target.value })}><option value="">选择 Pi 模型</option>{options.map((m) => <option key={m} value={m}>{m}</option>)}</select>
  return <PageChrome icon={<Settings size={15} />} eyebrow="Settings" title="Settings" description="配置需求扫描目录、Pi 任务模型和 Pi settings。">{dashboard.error ? <ErrorCard error={dashboard.error} /> : dashboard.loading ? <LoadingCard /> : <div className="react-settings-layout"><section className="react-panel"><PanelHead kicker="Scan Roots" title="需求扫描目录" chip={savedHint || undefined} /><p className="react-muted">每个目录会自动查找其下的 <code>.agents/req/</code> 或 <code>req/</code>。</p><label className="react-editor-label">扫描目录<textarea value={scanRootsText} onChange={(e) => setScanRootsText(e.target.value)} rows={5} placeholder="/home/hevin/Developer/company/WMS" /></label><div className="react-actions"><button onClick={saveScanRoots}>保存扫描目录</button></div></section><section className="react-panel"><PanelHead kicker="Models" title="Pi 任务模型" chip={piConfig.loading ? "loading" : `${options.length} models`} /><p className="react-muted">模型选项来自 <code>~/.pi/agent/models.json</code>，只读取 provider/model 名称，不回显 API Key。</p><div className="react-settings-grid"><label>Code Review{selectModel("codeReviewPiModel")}</label><label>Branch Scope{selectModel("branchScopePiModel")}</label><label>Effort Estimate{selectModel("effortEstimatePiModel")}</label><label>基础工时<input type="number" value={draft.effortEstimateBaseHours || 4} onChange={(e) => setDraft({ ...draft, effortEstimateBaseHours: Number(e.target.value) })} /></label></div><div className="react-actions"><button onClick={saveModels}>保存模型配置</button><button type="button" onClick={piConfig.refresh}>刷新模型列表</button></div>{piConfig.error ? <ErrorCard error={piConfig.error} /> : <div className="react-model-list">{(piConfig.data?.providers || []).map((p) => <div key={p.id} className="react-model-card"><strong>{p.id}</strong><span>{p.modelCount} models · key {p.hasApiKey ? "set" : "missing"}</span><p>{p.models.slice(0, 5).map((m) => m.modelId).join(" / ")}{p.models.length > 5 ? " …" : ""}</p></div>)}</div>}</section><section className="react-panel"><PanelHead kicker="Cainiao" title="菜鸟打印 Mock" chip={cainiaoStatus.data ? (cainiaoStatus.data.running ? "运行中" : "已停止") : "..."} /><p className="react-muted">内置 mock 菜鸟云打印客户端，监听 <code>ws://127.0.0.1:{draft.cainiaoMockPort || 13528}</code>，让 WMS 前端以为打印成功（getPrinters / print 返回 success）。</p><label className="react-switch"><input type="checkbox" checked={Boolean(draft.cainiaoMockEnabled)} onChange={(e) => setDraft({ ...draft, cainiaoMockEnabled: e.target.checked })} />启用菜鸟打印 Mock</label><div className="react-settings-grid"><label>监听端口<input type="number" value={draft.cainiaoMockPort || 13528} onChange={(e) => setDraft({ ...draft, cainiaoMockPort: Number(e.target.value) })} /></label></div><div className="react-actions"><button onClick={saveCainiaoMock}>保存并应用</button><button type="button" onClick={cainiaoStatus.refresh}>刷新状态</button></div></section><section className="react-panel react-config-editor"><PanelHead kicker="Pi File" title="settings.json" chip={fileSnapshot?.path || "~/.pi/agent/settings.json"} /><textarea className="react-code-textarea" value={fileContent} onChange={(e) => setFileContent(e.target.value)} spellCheck={false} /><div className="react-actions"><button onClick={savePiFile}>保存 settings.json</button></div></section></div>}</PageChrome>
}

function SchedulersPage() {
  const config = useFetch<ConfigPayload>("/api/config")
  const autoDrive = useFetch<AutoDrivePayload>("/api/requirement/auto-drive")
  return <PageChrome icon={<Activity size={15} />} eyebrow="Schedulers" title="定时任务查看" description="查看当前 Rust 版保留的调度配置和队列状态。"><section className="react-kpi-grid"><KpiCard icon={<Activity size={20} />} label="Full Sync" value={config.data?.fullSyncSchedule ? "ON" : "OFF"} sub={(config.data?.fullSyncTimes || []).join(" / ") || "no schedule"} tone="active" /><KpiCard icon={<Clock3 size={20} />} label="Auto Drive" value={autoDrive.data?.active ?? 0} sub={`blocked ${autoDrive.data?.blocked ?? 0}`} tone="avg" /><KpiCard icon={<Sparkles size={20} />} label="Repos" value={config.data?.fullSyncGithubRepos?.length ?? 0} sub="full sync repos" tone="done" /><KpiCard icon={<Gauge size={20} />} label="Queue" value={autoDrive.data?.queue?.queued ?? 0} sub="queued jobs" tone="total" /></section><section className="react-panel"><PanelHead kicker="Config" title="定时任务配置" chip={config.loading ? "loading" : "config"} />{config.error ? <ErrorCard error={config.error} /> : <div className="react-meta-grid"><span>Full sync schedule</span><span>{config.data?.fullSyncSchedule ? "开启" : "关闭"}</span><span>Full sync times</span><span>{(config.data?.fullSyncTimes || []).join(" / ") || "-"}</span><span>Full sync repos</span><span>{config.data?.fullSyncGithubRepos?.length || 0}</span><span>Auto drive message</span><span>{autoDrive.data?.message || "Rust rewrite currently exposes queue state only"}</span></div>}</section></PageChrome>
}

const gitAiStatusMeta: Record<GitAiCompanyStatus, { label: string; color: string; soft: string }> = {
  pending: { label: "待确认", color: "#f59e0b", soft: "rgba(245, 158, 11, .14)" },
  confirmed_ai: { label: "已标记", color: "#22c55e", soft: "rgba(34, 197, 94, .14)" },
  missing_ai: { label: "确认缺失", color: "#ef4444", soft: "rgba(239, 68, 68, .14)" },
  not_found: { label: "未找到", color: "#94a3b8", soft: "rgba(148, 163, 184, .14)" },
  check_failed: { label: "检查失败", color: "#f97316", soft: "rgba(249, 115, 22, .14)" },
}

function gitAiStatusPill(status: GitAiCompanyStatus) {
  const meta = gitAiStatusMeta[status] || gitAiStatusMeta.pending
  return <span className="react-status-pill" style={{ color: meta.color, background: meta.soft, borderColor: `${meta.color}66` }}>{meta.label}</span>
}

function authorShort(name?: string | null): string {
  return (name || "").split("<")[0].trim()
}

function GitAiPage() {
  const feed = useFetch<GitAiSuspectsPayload>("/api/git-ai/suspects")
  const health = useFetch<GitAiHealthPayload>("/api/git-ai/health")
  const [status, setStatus] = useState<GitAiCompanyStatus | "all">("all")
  const [refreshingCompany, setRefreshingCompany] = useState(false)
  const [fixingId, setFixingId] = useState<string | null>(null)
  const [fixResults, setFixResults] = useState<Record<string, GitAiFixResponse>>({})
  const [author, setAuthor] = useState<string>("")
  const records = feed.data?.records || []
  const authors = useMemo(() => Array.from(new Set(records.map((r) => authorShort(r.authorName)).filter(Boolean))).sort(), [records])
  const filtered = (status === "all" ? records.filter((r) => r.companyStatus !== "confirmed_ai" && r.companyStatus !== "missing_ai") : records.filter((r) => r.companyStatus === status)).filter((r) => !author || authorShort(r.authorName) === author)
  const stats = feed.data?.stats
  const refreshCompany = async () => {
    setRefreshingCompany(true)
    try {
      await postJson<GitAiSuspectsPayload>("/api/git-ai/suspects/refresh", { limit: 200 })
      feed.refresh()
      health.refresh()
    } finally {
      setRefreshingCompany(false)
    }
  }
  const fixNote = async (record: GitAiSuspectRecord) => {
    if (!record.id || fixingId) return
    setFixingId(record.id)
    try {
      const res = await postJson<GitAiFixResponse>("/api/git-ai/suspects/fix-note", { id: record.id })
      setFixResults((cur) => ({ ...cur, [record.id]: res }))
      feed.refresh()
      health.refresh()
    } catch (err) {
      setFixResults((cur) => ({ ...cur, [record.id]: { ok: false, stillMissing: true, piAgent: { dispatched: false, message: err instanceof Error ? err.message : String(err) } } }))
    } finally {
      setFixingId(null)
    }
  }
  const canFix = (r: GitAiSuspectRecord) => r.companyStatus === "missing_ai" || r.companyStatus === "pending" || r.companyStatus === "not_found"
  return <PageChrome icon={<GitBranch size={15} />} eyebrow="Git AI" title="AI 标记漏标检查" description="刷新会调用公司 ai-stats/check-commit 接口；git-ai 是否打标以公司接口结果为准。" actions={<button onClick={refreshCompany} disabled={refreshingCompany}><RefreshCw size={15} />{refreshingCompany ? "公司检查中…" : "刷新公司检查"}</button>}><section className="react-kpi-grid"><KpiCard icon={<AlertTriangle size={20} />} label="疑似记录" value={stats?.total ?? "-"} sub="待判定" tone="avg" /><KpiCard icon={<Clock3 size={20} />} label="待确认" value={stats?.pending ?? "-"} sub="pending company check" tone="active" /><KpiCard icon={<AlertTriangle size={20} />} label="确认缺失" value={stats?.missingAi ?? "-"} sub="company says missing" tone="total" /><KpiCard icon={<CheckCircle2 size={20} />} label="已标记" value={stats?.confirmedAi ?? "-"} sub="company says tagged" tone="done" /></section><section className="react-panel"><PanelHead kicker="Health" title="git-ai 状态" chip={health.data?.cli?.daemonOk ? "ok" : (health.data?.piExtension?.status || "unknown")} />{health.error ? <ErrorCard error={health.error} /> : <div className="react-meta-grid"><span>Store</span><span><code>{health.data?.storePath || "-"}</code></span><span>git-ai binary</span><span><code>{health.data?.cli?.binaryPath || "missing"}</code></span><span>CLI version</span><span>{health.data?.cli?.version || "-"}</span><span>Daemon</span><span>{health.data?.cli?.daemonOk ? "running" : (health.data?.cli?.daemonMessage || "not running")}</span><span>Trace2 socket</span><span>{health.data?.cli?.trace2SocketExists ? "ok" : "missing"} <code>{health.data?.cli?.trace2Socket || "-"}</code></span><span>Hooks path</span><span><code>{health.data?.cli?.hooksPath || "-"}</code></span><span>post-commit hook</span><span>{health.data?.cli?.postCommitHook?.mode || "-"} · record={String(Boolean(health.data?.cli?.postCommitHook?.recordsToAgentPanel))}</span><span>pre-push hook</span><span>{health.data?.cli?.prePushHook?.mode || "-"} · record={String(Boolean(health.data?.cli?.prePushHook?.recordsToAgentPanel))}</span><span>Pi extension</span><span>{health.data?.piExtension?.status || "unknown"} · {health.data?.piExtension?.message || "-"}</span><span>Tracked tools</span><span>{health.data?.piExtension?.tracksTools?.join(" / ") || "-"}</span></div>}<div className="react-tab-row"><button className={status === "all" ? "active" : ""} onClick={() => setStatus("all")}>疑似待处理</button>{(Object.keys(gitAiStatusMeta) as GitAiCompanyStatus[]).map((s) => <button key={s} className={status === s ? "active" : ""} onClick={() => setStatus(s)}>{gitAiStatusMeta[s].label}</button>)}<select className="react-tab-select" value={author} onChange={(e) => setAuthor(e.target.value)} aria-label="按提交人筛选"><option value="">全部提交人</option>{authors.map((a) => <option key={a} value={a}>{a}</option>)}</select></div></section>{feed.error ? <ErrorCard error={feed.error} /> : feed.loading ? <LoadingCard /> : <div className="react-card-list">{filtered.length === 0 ? <EmptyCard>暂无符合条件的疑似漏标记录。</EmptyCard> : filtered.map((r, i) => <motion.article key={r.id || `${r.projectName}-${r.commitSha}`} className="react-list-card react-session-card react-gitai-card" initial={{ opacity: 0, y: 12 }} animate={{ opacity: 1, y: 0 }} transition={{ delay: Math.min(i, 16) * 0.025 }} whileHover={{ y: -3 }}><div><span className="react-card-id">{r.projectName} · {r.shortSha || r.commitSha?.slice(0, 12)}</span><h3>{r.commitWebUrl ? <a href={r.commitWebUrl} target="_blank" rel="noopener noreferrer">{r.commitTitle || r.subject || r.commitSha}</a> : (r.commitTitle || r.subject || r.commitSha)}</h3><p>{r.repoPath || r.remoteUrl || "repo path n/a"}</p><div className="react-card-meta"><span>{gitAiStatusPill(r.companyStatus || "pending")}</span>{r.authorName ? <span>提交人 {authorShort(r.authorName)}</span> : null}<span>hook: {(r.eventSources || []).join(" / ") || "-"}</span><span>本地 note: {r.localNoteState || "unknown"}</span><span>记录 {relAge(r.lastSeenAt)}</span>{r.companyCheckedAt ? <span>公司检查 {relAge(r.companyCheckedAt)}</span> : null}{typeof r.aiRate === "number" ? <span>AI rate {r.aiRate}%</span> : null}</div>{r.companyError ? <p className="react-error">{r.companyError}</p> : null}{fixResults[r.id] ? <div className="react-gitai-fix-result">{fixResults[r.id].pushSteps?.length ? <details className="react-review-commits"><summary>重推 notes 步骤（{fixResults[r.id].pushSteps!.filter((s) => s.ok).length}/{fixResults[r.id].pushSteps!.length} 成功）</summary><pre>{fixResults[r.id].pushSteps!.map((s) => `${s.ok ? "✓" : "✗"} ${s.label} — ${s.command}\n${s.stderr || s.stdout || ""}`).join("\n")}</pre></details> : null}{!fixResults[r.id].stillMissing ? <p className="react-effort-error react-fix-ok">✅ 重推 notes 后公司接口已确认标记。</p> : fixResults[r.id].piAgent ? <div className="react-gitai-agent-info"><span>{fixResults[r.id].piAgent!.dispatched ? "🤖" : "⚠️"} {fixResults[r.id].piAgent!.message}</span>{fixResults[r.id].piAgent!.sessionId ? <code>pi --session {fixResults[r.id].piAgent!.sessionId}</code> : null}</div> : null}</div> : null}</div><div className="react-card-side"><span className="react-effort-badge">{r.aiLines ?? 0} AI / {r.humanLines ?? 0} human</span><span className="react-muted">{r.branch || "branch n/a"}</span><code>{(r.commitSha || "").slice(0, 12)}</code>{canFix(r) ? <button className="react-fix-note-btn" onClick={() => fixNote(r)} disabled={!!fixingId}>{fixingId === r.id ? "补标中…" : "一键补标"}</button> : null}</div></motion.article>)}</div>}</PageChrome>
}

interface TestdataTarget {
  name: string
  status_code: number | null
  label: string
  verified: boolean
}

interface TestdataCliField {
  required?: boolean
  choices?: string[]
  default?: string | number
}

interface TestdataCapabilityItem {
  id: string
  domain: string
  object: string
  execution: string
  purpose: string
  script: string
  cli?: Record<string, TestdataCliField>
  targets?: TestdataTarget[]
}

interface TestdataCapabilitiesPayload {
  ok: boolean
  project: string
  capabilities: TestdataCapabilityItem[]
}

interface TestdataCapabilityPayload {
  ok: boolean
  capability: TestdataCapabilityItem & { pitfalls?: string[]; notes?: unknown[] }
}

interface TestdataRunPayload {
  ok: boolean
  dryRun: boolean
  executed: boolean
  project?: string
  capabilityId?: string
  target?: string
  command: string
  cwd: string
  stdout?: string
  stderr?: string
  exitCode?: number | null
  safety?: { env?: string; note?: string }
}

function TestdataPage() {
  const project = "WMS"
  const caps = useFetch<TestdataCapabilitiesPayload>(`/api/capabilities?project=${project}`)
  const [selectedId, setSelectedId] = useState<string>("")
  const [detail, setDetail] = useState<TestdataCapabilityPayload | null>(null)
  const [detailLoading, setDetailLoading] = useState(false)
  const [target, setTarget] = useState("")
  const [env, setEnv] = useState("test")
  const [params, setParams] = useState<Record<string, string>>({})
  const [runResult, setRunResult] = useState<TestdataRunPayload | null>(null)
  const [running, setRunning] = useState(false)
  const [runError, setRunError] = useState<string | null>(null)

  const createCaps = (caps.data?.capabilities || []).filter((c) => c.domain === "outbound" || c.domain === "inbound")

  const loadDetail = async (id: string) => {
    if (!id) { setDetail(null); return }
    setDetailLoading(true)
    setRunResult(null)
    setRunError(null)
    try {
      const d = await fetchJson<TestdataCapabilityPayload>(`/api/capability?id=${encodeURIComponent(id)}&project=${project}`)
      setDetail(d)
      const firstTarget = (d.capability?.targets || []).find((t) => t.verified) || (d.capability?.targets || [])[0]
      setTarget(firstTarget?.name || "")
      const init: Record<string, string> = {}
      for (const [key, spec] of Object.entries(d.capability?.cli || {})) {
        if (key === "target" || key === "env") continue
        if (spec.default !== undefined) init[key] = String(spec.default)
      }
      setParams(init)
    } catch (e) {
      setDetail(null)
    } finally {
      setDetailLoading(false)
    }
  }

  useEffect(() => { if (selectedId) loadDetail(selectedId) }, [selectedId])

  const currentCap = createCaps.find((c) => c.id === selectedId)
  const cliFields = detail?.capability?.cli || {}

  const run = async (execute: boolean) => {
    if (!selectedId || !target) return
    setRunning(true)
    setRunError(null)
    try {
      const res = await postJson<TestdataRunPayload>("/api/testdata/run", {
        project,
        capabilityId: selectedId,
        target,
        env,
        params,
        dryRun: !execute,
        execute,
      })
      setRunResult(res)
    } catch (e) {
      setRunError(e instanceof Error ? e.message : String(e))
    } finally {
      setRunning(false)
    }
  }

  return <PageChrome icon={<Sparkles size={15} />} eyebrow="Test Data" title="测试造数" description="选择能力，在 WMS test 环境创建指定状态的测试单据。Agent Panel 调用项目内 capability pack 脚本，真实创建数据。" actions={<><a href="/dashboard"><ArrowLeft size={15} />返回</a><button onClick={() => caps.refresh()} disabled={caps.loading}><RefreshCw size={15} className={caps.loading ? "react-spin" : ""} />刷新能力</button></>}>
    <section className="react-panel"><PanelHead kicker="Project" title="项目选择" chip={project} />
      <div className="react-tab-row"><button className="active">WMS</button><button disabled>其他项目（敬请期待）</button></div>
    </section>
    {caps.error ? <ErrorCard error={caps.error} /> : caps.loading ? <LoadingCard /> : <section className="react-panel"><PanelHead kicker="Capability" title="选择造数能力" chip={`${createCaps.length} available`} />
      <div className="react-card-list">{createCaps.length === 0 ? <EmptyCard>暂无出库/入库造数能力。</EmptyCard> : createCaps.map((c) => <article key={c.id} className={`react-list-card ${selectedId === c.id ? "react-cap-active" : ""}`} onClick={() => setSelectedId(c.id)} style={{ cursor: "pointer" }}><div><span className="react-card-id">{c.domain}</span><h3>{c.purpose}</h3><p className="react-muted">{c.id} · {c.execution}</p></div><div className="react-card-side"><span className="react-effort-badge">{c.script ? "script" : "recipe"}</span></div></article>)}</div>
    </section>}
    {selectedId ? <section className="react-panel"><PanelHead kicker="Configure" title="配置造数参数" chip={detailLoading ? "loading" : currentCap?.id} />
      {detailLoading ? <LoadingCard /> : detail?.capability ? <><div className="react-meta-grid"><span>能力</span><span>{detail.capability.purpose}</span><span>目标对象</span><span>{detail.capability.object}</span><span>验证环境</span><span>test</span></div>
        <div className="react-tab-row"><button className={env === "test" ? "active" : ""} onClick={() => setEnv("test")}>test 环境</button><button className={env === "uat-cn" ? "active" : ""} onClick={() => setEnv("uat-cn")}>UAT-CN 环境</button></div>
        <label className="react-editor-label">目标状态 (target)<select value={target} onChange={(e) => setTarget(e.target.value)}>{(detail.capability.targets || []).map((t) => <option key={t.name} value={t.name}>{t.name} ({t.label}{t.verified ? "" : "·待验证"})</option>)}</select></label>
        <div className="react-settings-grid">{Object.entries(cliFields).filter(([k]) => k !== "target" && k !== "env").map(([key, spec]) => <label key={key}>{key}{spec.choices ? <select value={params[key] || ""} onChange={(e) => setParams({ ...params, [key]: e.target.value })}>{spec.choices.map((c) => <option key={c} value={c}>{c}</option>)}</select> : <input value={params[key] || ""} onChange={(e) => setParams({ ...params, [key]: e.target.value })} placeholder={spec.default !== undefined ? String(spec.default) : ""} />}</label>)}</div>
        <div className="react-actions"><button onClick={() => run(false)} disabled={running || !target}>预览命令 (dry-run)</button><button onClick={() => run(true)} disabled={running || !target} className="react-fix-note-btn">{running ? "执行中…" : "执行造数 (test 环境)"}</button></div>
        <p className="react-muted">⚠️ {env === "uat-cn" ? "UAT 环境造数为真实数据，且 MySQL 只读（Archery）；" : "test 环境造数为真实数据；"}建议先预览命令，确认无误再执行。</p>
      </> : <EmptyCard>未加载能力详情</EmptyCard>}
    </section> : null}
    {runError ? <ErrorCard error={runError} /> : runResult ? <section className="react-panel"><PanelHead kicker="Result" title={runResult.executed ? "执行结果" : "命令预览"} chip={runResult.executed ? `exit ${runResult.exitCode ?? "-"}` : "dry-run"} />
      <code className="react-command">{runResult.command}</code>
      <p className="react-muted">cwd: {runResult.cwd}</p>
      {runResult.executed && runResult.stdout ? <details className="react-review-commits" open><summary>stdout</summary><pre>{runResult.stdout}</pre></details> : null}
      {runResult.executed && runResult.stderr ? <details className="react-review-commits"><summary>stderr</summary><pre>{runResult.stderr}</pre></details> : null}
      {runResult.safety?.note ? <p className="react-muted">{runResult.safety.note}</p> : null}
    </section> : null}
  </PageChrome>
}

function RemovedPage({ title, detail }: { title: string; detail: string }) {
  return <PageChrome icon={<AlertTriangle size={15} />} eyebrow="Removed" title={title} description={detail}><EmptyCard>该页面属于旧 OpenCode / PTY 功能，已在 Rust + React 重写中移除。</EmptyCard></PageChrome>
}

function NotFoundPage() { return <PageChrome icon={<Search size={15} />} eyebrow="Not Found" title="页面不存在"><EmptyCard>当前路由没有匹配的 React 页面。</EmptyCard></PageChrome> }

export function App({ apiPath }: AppProps) {
  const key = useLocationKey()
  const path = window.location.pathname
  const { project, setProject } = useProjectFilter()
  const page = path === "/" || path === "/dashboard" ? <DashboardPage apiPath={apiPath} project={project} />
    : path === "/projects" || path === "/requirements" ? <ProjectsPage globalProject={project} />
    : path === "/business-knowledge" ? <KnowledgePage kind="businessKnowledge" />
    : path === "/experiences" ? <KnowledgePage kind="experience" />
    : path === "/sessions" ? <SessionsPage />
    : path === "/session" ? <SessionPage />
    : path === "/requirement" ? <RequirementPage />
    : path === "/requirement-diff" ? <RequirementDiffPage />
    : path === "/requirement-merge" ? <RequirementMergePage />
    : path === "/schedulers" ? <SchedulersPage />
    : path === "/git-ai" ? <GitAiPage />
    : path === "/testdata" ? <TestdataPage />
    : path === "/settings" ? <SettingsPage />
    : path === "/reports" || path === "/report" ? <RemovedPage title="Experience Reports 已移除" detail="OpenCode 经验报告、confirm/reject 和 auto-summary 链路不再保留。" />
    : path === "/env-vars" ? <RemovedPage title="Env Vars 已移除" detail="Rust 版暂未恢复浏览器环境变量编辑。" />
    : <NotFoundPage />

  return <AppShell path={path} project={project} onProjectChange={setProject}><AnimatePresence mode="wait"><motion.div key={key} initial={{ opacity: 0, y: 10 }} animate={{ opacity: 1, y: 0 }} exit={{ opacity: 0, y: -6 }} transition={{ duration: 0.22 }}>{page}</motion.div></AnimatePresence></AppShell>
}
