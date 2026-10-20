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
  Copy,
  FileCode2,
  FlaskConical,
  Gauge,
  GitMerge,
  KeyRound,
  LayoutDashboard,
  Library,
  Lightbulb,
  List,
  ListChecks,
  CalendarRange,
  MonitorSmartphone,
  PanelLeftClose,
  PanelLeftOpen,
  RefreshCw,
  Rocket,
  Search,
  Server,
  Settings,
  Siren,
  Unlink2,
} from "lucide-react"
import { useEffect, useMemo, useState } from "react"
import { createPortal } from "react-dom"
import type {
  ApiSessions,
  AutoDrivePayload,
  BranchRepo,
  BranchScope,
  CainiaoMockStatus,
  CodeReviewPayload,
  CodeReviewSnapshot,
  ConfigPayload,
  DashboardStatsPayload,
  ExperienceSummaryDispatchPayload,
  ExperienceSummaryJob,
  ExperienceSummaryJobsPayload,
  KnowledgeDraft,
  KnowledgeItem,
  KnowledgeKind,
  KnowledgeListPayload,
  KnowledgeSavePayload,
  MasterDiffPayload,
  MergeBranchPayload,
  MergeKindOptions,
  MergeOptionsPayload,
  MergeRepoKind,
  MergeTarget,
  PiConfigFileSnapshot,
  PiConfigSummary,
  ProdMrPayload,
  ProdMrResult,
  ReqCategory,
  ReqStatus,
  Requirement,
  RequirementDocPayload,
  RequirementDuration,
  ReviewGatePayload,
  SessionInfo,
  SessionLogEntry,
  SessionLogPayload,
  StatusCount,
  SyncBasePayload,
  TestdataCapabilitiesPayload,
  TestdataCapabilityPayload,
  TestdataRunPayload,
} from "./types"
import { Markdown } from "./markdown"
import { fetchJson, fullUrl, postForm, postJson, setPanelToken, setUnauthorizedHandler, useFetch } from "./lib/api"
import { formatDate, formatDateTime, formatDuration, joinList, parseOnesRef, relAge, splitCsvText } from "./lib/format"
import { ISSUE_STATUSES, REQ_CATEGORIES, REQ_FLOW_STATUSES, REQ_STATUSES, statusMeta } from "./lib/requirements"
import { experienceSummaryPill, experienceSummaryStageLabel, onesBadge, projectsOf, statusPill } from "./features/requirements/badges"
import { cardVariants, EmptyCard, ErrorCard, KpiCard, LoadingCard, PageChrome, PanelHead } from "./components/ui"
import { ErrorBoundary } from "./components/error-boundary"
import { PROJECT_FILTER_KEY, readProjectFilter, readSidebarCollapsed, persistSidebarCollapsed } from "./lib/preferences"
import { getActiveConnection } from "./lib/connection"
import { useIsMobile } from "./lib/use-is-mobile"
import { HarnessSwitcher } from "./features/harness/harness-switcher"
import { RequirementsData } from "./pages/projects"
import { TestdataPage } from "./pages/testdata"
import { TestdataApiDetailPage } from "./pages/testdata-api-detail"
import { DashboardPage } from "./pages/dashboard"
import { KnowledgePage, ExperiencesPage } from "./pages/knowledge"
import { ProjectsPage } from "./pages/projects"
import { ReleasePlanPage } from "./pages/release-plan"
import { SchedulePage } from "./pages/schedule"
import { OnesMissingPage } from "./pages/ones-missing"
import { ManhourPage } from "./pages/manhour"
import { IssuesPage } from "./pages/issues"
import { compactPath, diffDomId, parseUnifiedDiffFiles, reviewStats, shortFileName } from "./lib/diff"
import { SessionPage, SessionsPage } from "./pages/sessions"
import { SettingsPage } from "./pages/settings"
import { SchedulersPage } from "./pages/schedulers"
import { AuthSitesPage } from "./pages/auth-sites"
import { ConnectPage } from "./pages/connect"
import { RequirementPage } from "./pages/requirement"
import { RequirementDocPage } from "./pages/requirement-doc"
import { RequirementDiffPage } from "./pages/requirement-diff"
import { RequirementMergePage } from "./pages/requirement-merge"
import { RequirementGatePage } from "./pages/requirement-gate"


const PROJECT_OPTIONS = [
  { value: "", label: "全部" },
  { value: "WMS", label: "WMS" },
]

function useProjectFilter() {
  const [project, setProjectState] = useState<string>(readProjectFilter)
  const setProject = (v: string) => {
    setProjectState(v)
    try { localStorage.setItem(PROJECT_FILTER_KEY, v) } catch { /* ignore */ }
  }
  return { project, setProject }
}

interface AppProps { apiPath: string }

const navItems = [
  { href: "/dashboard", label: "状态看板", short: "DB", icon: <LayoutDashboard size={16} /> },
  { href: "/projects", label: "需求看板", short: "PR", icon: <ListChecks size={16} /> },
  { href: "/release-plan", label: "发布计划", short: "RP", icon: <Rocket size={16} /> },
  { href: "/schedule", label: "需求排期", short: "SC", icon: <CalendarRange size={16} /> },
  { href: "/ones-missing", label: "ONES 缺失", short: "ON", icon: <Unlink2 size={16} /> },
  { href: "/manhour", label: "工时录入", short: "MH", icon: <Clock3 size={16} /> },
  { href: "/issues", label: "线上问题", short: "IS", icon: <Siren size={16} /> },
  { href: "/business-knowledge", label: "业务知识", short: "BK", icon: <Library size={16} /> },
  { href: "/experiences", label: "经验", short: "EX", icon: <Lightbulb size={16} /> },
  { href: "/sessions", label: "Sessions", short: "SE", icon: <Server size={16} /> },
  { href: "/connect", label: "连接", short: "CN", icon: <MonitorSmartphone size={16} /> },
  { href: "/schedulers", label: "Schedulers", short: "SC", icon: <Activity size={16} /> },
  { href: "/auth-sites", label: "登录态", short: "AU", icon: <KeyRound size={16} /> },
  { href: "/testdata", label: "接口测试", short: "IT", icon: <FlaskConical size={16} /> },
  { href: "/settings", label: "Settings", short: "ST", icon: <Settings size={16} /> },
]

function isActiveNav(path: string, href: string): boolean {
  if (href === "/dashboard") return path === "/" || path === "/dashboard"
  if (href === "/projects") return path === "/projects" || path === "/requirements" || path === "/requirement" || path === "/requirement-diff" || path === "/requirement-merge" || path === "/requirement-doc" || path === "/requirement-gate"
  if (href === "/release-plan") return path === "/release-plan"
  if (href === "/schedule") return path === "/schedule"
  if (href === "/ones-missing") return path === "/ones-missing"
  if (href === "/manhour") return path === "/manhour"
  if (href === "/issues") return path === "/issues"
  if (href === "/business-knowledge") return path === "/business-knowledge"
  if (href === "/experiences") return path === "/experiences"
  if (href === "/schedulers") return path === "/schedulers"
  if (href === "/auth-sites") return path === "/auth-sites"
  if (href === "/testdata") return path === "/testdata" || path.startsWith("/testdata/")
  return path === href
}

function titleForPath(path: string): { eyebrow: string; title: string } {
  if (path === "/" || path === "/dashboard") return { eyebrow: "Dashboard", title: "状态看板" }
  if (path === "/projects" || path === "/requirements") return { eyebrow: "Requirements", title: "需求进度看板" }
  if (path === "/release-plan") return { eyebrow: "Release Plan", title: "发布计划" }
  if (path === "/schedule") return { eyebrow: "Schedule", title: "需求排期" }
  if (path === "/ones-missing") return { eyebrow: "ONES Missing", title: "ONES 缺失统计" }
  if (path === "/manhour") return { eyebrow: "Manhour", title: "工时录入" }
  if (path === "/issues") return { eyebrow: "Online Issues", title: "线上问题" }
  if (path === "/business-knowledge") return { eyebrow: "Business Knowledge", title: "业务知识" }
  if (path === "/experiences") return { eyebrow: "Experiences", title: "经验" }
  if (path === "/requirement") return { eyebrow: "Requirement", title: "需求详情" }
  if (path === "/requirement-doc") return { eyebrow: "Requirement Doc", title: "需求文档" }
  if (path === "/requirement-gate") return { eyebrow: "Gate", title: "门禁详情" }
  if (path === "/requirement-diff") return { eyebrow: "Diff", title: "分支差异" }
  if (path === "/requirement-merge") return { eyebrow: "Merge", title: "分支合并" }
  if (path === "/sessions") return { eyebrow: "Pi Sessions", title: "Sessions" }
  if (path === "/session") return { eyebrow: "Session", title: "Session 详情" }
  if (path === "/pi-chat") return { eyebrow: "Pi Agent", title: "Pi 会话" }
  if (path === "/connect") return { eyebrow: "Connection", title: "服务器连接" }
  if (path === "/schedulers") return { eyebrow: "Schedulers", title: "定时任务" }
  if (path === "/auth-sites") return { eyebrow: "Browser Auth", title: "Chrome 登录态复用" }
  if (path === "/settings") return { eyebrow: "Settings", title: "Settings" }
  if (path === "/testdata") return { eyebrow: "Test Data", title: "接口测试" }
  if (path === "/testdata/api") return { eyebrow: "Test Data", title: "接口详情" }
  return { eyebrow: "Agent Panel", title: "React + Rust" }
}

function AppShell({ path, children, project, onProjectChange, docsMode }: { path: string; children: React.ReactNode; project: string; onProjectChange: (v: string) => void; docsMode: boolean }) {
  const meta = titleForPath(path)
  const [sidebarCollapsed, setSidebarCollapsed] = useState(readSidebarCollapsed)
  const toggleSidebar = () => setSidebarCollapsed((v) => { persistSidebarCollapsed(!v); return !v })
  const activeConn = getActiveConnection()
  const visibleNav = docsMode ? navItems.filter((item) => !WORKER_ONLY_HREFS.has(item.href)) : navItems
  // 融合说明：连接层卡片（mobile-pwa）+ docs 模式导航裁剪（hub-worker-split）。
  return <div className={`react-shell ${sidebarCollapsed ? "collapsed" : ""}`}><aside className="react-sidebar"><a className="react-brand" href="/dashboard" aria-label="Agent Panel home"><span className="react-brand-mark">AP</span><span className="react-brand-copy"><strong>Agent</strong><em>Panel</em></span></a><nav className="react-sidebar-nav" aria-label="Primary navigation">{visibleNav.map((item) => <a key={item.href} href={item.href} className={`react-sidebar-link ${isActiveNav(path, item.href) ? "active" : ""}`} title={sidebarCollapsed ? item.label : undefined}><span className="react-sidebar-icon">{item.icon}</span><span>{item.label}</span><small>{item.short}</small></a>)}</nav><div className="react-sidebar-card"><a href="/connect" title="管理服务器连接" style={{ display: "contents" }}><span>{docsMode ? "HUB (DOCS) MODE" : "CONNECTION"}</span><strong>{activeConn.url ? activeConn.name : `${activeConn.name}（同源）`}</strong><em>{activeConn.url || "localhost"}</em></a></div></aside><div className="react-content-shell"><header className="react-topbar"><div className="react-topbar-title"><button type="button" className="react-topbar-collapse" onClick={toggleSidebar} aria-label={sidebarCollapsed ? "展开侧边栏" : "收起侧边栏"} title={sidebarCollapsed ? "展开侧边栏" : "收起侧边栏"}>{sidebarCollapsed ? <PanelLeftOpen size={16} /> : <PanelLeftClose size={16} />}</button><div className="react-topbar-heading"><span>{meta.eyebrow}</span><strong>{meta.title}</strong></div></div><div className="react-topbar-actions">{!docsMode && <HarnessSwitcher />}<label className="react-project-select">项目<select value={project} onChange={(e) => onProjectChange(e.target.value)}>{PROJECT_OPTIONS.map((o) => <option key={o.value} value={o.value}>{o.label}</option>)}</select></label><a href="/dashboard">Home</a><button type="button" onClick={() => window.location.reload()}>Refresh</button></div></header><main className="react-main">{children}</main></div></div>
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

function RemovedPage({ title, detail }: { title: string; detail: string }) {
  return <PageChrome icon={<AlertTriangle size={15} />} eyebrow="Removed" title={title} description={detail}><EmptyCard>该页面属于旧 OpenCode / PTY 功能，已在 Rust + React 重写中移除。</EmptyCard></PageChrome>
}

function NotFoundPage() { return <PageChrome icon={<Search size={15} />} eyebrow="Not Found" title="页面不存在"><EmptyCard>当前路由没有匹配的 React 页面。</EmptyCard></PageChrome> }

/// docs 模式下 worker 类页面的占位提示（阶段二 worker 代理后恢复）。
function WorkerOnlyPage({ title }: { title: string }) {
  return <PageChrome icon={<AlertTriangle size={15} />} eyebrow="Worker" title={title} description="该能力属于 worker 执行类页面，当前连接的 hub 不提供。"><EmptyCard>请切换到具备 worker 能力的面板连接，或等待阶段二 worker 代理上线后恢复。</EmptyCard></PageChrome>
}

/// worker 类页面路由：docs 模式下导航隐藏且路由降级。
const WORKER_ONLY_HREFS = new Set(["/sessions", "/schedulers", "/auth-sites", "/testdata", "/testdata/api"])

export function App({ apiPath }: AppProps) {
  const key = useLocationKey()
  const path = window.location.pathname
  const { project, setProject } = useProjectFilter()
  const isMobile = useIsMobile()
  const [mode, setMode] = useState<"local" | "docs">("local")
  useEffect(() => {
    setUnauthorizedHandler(() => {
      const token = window.prompt("需要访问 token（即服务端 PANEL_AUTH_TOKEN）")
      if (token && token.trim()) {
        setPanelToken(token.trim())
        window.location.reload()
      }
    })
    // mode 探测跟随当前连接（连接层切换后指向对应服务器，而非 SPA 同源）。
    fetch(fullUrl("/api/mode"), { cache: "no-store" })
      .then((r) => r.json())
      .then((v: { mode?: string }) => { if (v?.mode === "docs") setMode("docs") })
      .catch(() => { /* 探测失败按 local 处理 */ })
    return () => setUnauthorizedHandler(null)
  }, [])
  const docsMode = mode === "docs"
  const workerPage = (title: string) => <WorkerOnlyPage title={title} />
  const page = path === "/" || path === "/dashboard" ? <DashboardPage apiPath={apiPath} project={project} />
    : path === "/projects" || path === "/requirements" ? <ProjectsPage globalProject={project} />
    : path === "/release-plan" ? <ReleasePlanPage globalProject={project} />
    : path === "/schedule" ? <SchedulePage globalProject={project} />
    : path === "/ones-missing" ? <OnesMissingPage globalProject={project} />
    : path === "/manhour" ? <ManhourPage globalProject={project} />
    : path === "/issues" ? <IssuesPage globalProject={project} />
    : path === "/business-knowledge" ? <KnowledgePage kind="businessKnowledge" />
    : path === "/experiences" ? <KnowledgePage kind="experience" />
    : docsMode && path === "/sessions" ? workerPage("Sessions")
    : docsMode && path === "/session" ? workerPage("Session 详情")
    : docsMode && path === "/schedulers" ? workerPage("Schedulers")
    : docsMode && path === "/auth-sites" ? workerPage("Chrome 登录态复用")
    : docsMode && (path === "/testdata" || path === "/testdata/api") ? workerPage("接口测试")
    : path === "/sessions" ? <SessionsPage />
    : path === "/session" ? <SessionPage />
    : path === "/requirement" ? <RequirementPage />
    : path === "/requirement-doc" ? <RequirementDocPage />
    : path === "/requirement-gate" ? <RequirementGatePage />
    : path === "/requirement-diff" ? <RequirementDiffPage />
    : path === "/requirement-merge" ? <RequirementMergePage />
    : path === "/schedulers" ? <SchedulersPage />
    : path === "/auth-sites" ? <AuthSitesPage />
    : path === "/connect" ? <ConnectPage />
    : path === "/testdata" ? <TestdataPage />
    : path === "/testdata/api" ? <TestdataApiDetailPage />
    : path === "/settings" ? <SettingsPage />
    : path === "/reports" || path === "/report" ? <RemovedPage title="Experience Reports 已移除" detail="OpenCode 经验报告、confirm/reject 和 auto-summary 链路不再保留。" />
    : path === "/env-vars" ? <RemovedPage title="Env Vars 已移除" detail="Rust 版暂未恢复浏览器环境变量编辑。" />
    : <NotFoundPage />

  // pi-chat 移动版脱离 shell：全屏 100dvh 聊天视口（shell 侧边栏会挤压会话流）。
  if (path === "/pi-chat" && isMobile) {
    return <ErrorBoundary key={key}>{page}</ErrorBoundary>
  }

  return <AppShell path={path} project={project} onProjectChange={setProject} docsMode={docsMode}><AnimatePresence mode="wait"><motion.div key={key} initial={{ opacity: 0, y: 10 }} animate={{ opacity: 1, y: 0 }} exit={{ opacity: 0, y: -6 }} transition={{ duration: 0.22 }}><ErrorBoundary key={key}>{page}</ErrorBoundary></motion.div></AnimatePresence></AppShell>
}
