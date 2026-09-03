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
  Gauge,
  GitBranch,
  GitMerge,
  KeyRound,
  LayoutDashboard,
  Library,
  Lightbulb,
  List,
  ListChecks,
  PanelLeftClose,
  PanelLeftOpen,
  RefreshCw,
  Rocket,
  Search,
  Server,
  Settings,
  Siren,
  Sparkles,
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
  GitAiCompanyStatus,
  GitAiFixResponse,
  GitAiHealthPayload,
  GitAiSuspectRecord,
  GitAiSuspectsPayload,
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
import { fetchJson, postForm, postJson, useFetch } from "./lib/api"
import { formatDate, formatDateTime, formatDuration, joinList, parseOnesRef, relAge, splitCsvText } from "./lib/format"
import { ISSUE_STATUSES, REQ_CATEGORIES, REQ_FLOW_STATUSES, REQ_STATUSES, statusMeta } from "./lib/requirements"
import { experienceSummaryPill, experienceSummaryStageLabel, onesBadge, projectsOf, statusPill } from "./features/requirements/badges"
import { cardVariants, EmptyCard, ErrorCard, KpiCard, LoadingCard, PageChrome, PanelHead } from "./components/ui"
import { PROJECT_FILTER_KEY, readProjectFilter, readSidebarCollapsed, persistSidebarCollapsed } from "./lib/preferences"
import { HarnessSwitcher } from "./features/harness/harness-switcher"
import { RequirementsData } from "./pages/projects"
import { GitAiPage } from "./pages/git-ai"
import { TestdataPage } from "./pages/testdata"
import { DashboardPage } from "./pages/dashboard"
import { KnowledgePage, ExperiencesPage } from "./pages/knowledge"
import { ProjectsPage } from "./pages/projects"
import { ReleasePlanPage } from "./pages/release-plan"
import { OnesMissingPage } from "./pages/ones-missing"
import { IssuesPage } from "./pages/issues"
import { compactPath, diffDomId, parseUnifiedDiffFiles, reviewStats, shortFileName } from "./lib/diff"
import { SessionPage, SessionsPage } from "./pages/sessions"
import { SessionChipList, SessionListModal } from "./pages/sessions"
import { SettingsPage } from "./pages/settings"
import { SchedulersPage } from "./pages/schedulers"
import { AuthSitesPage } from "./pages/auth-sites"
import { RequirementDocPage, RequirementDiffPage, RequirementMergePage, RequirementPage } from "./pages/requirement"


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
  { href: "/ones-missing", label: "ONES 缺失", short: "ON", icon: <Unlink2 size={16} /> },
  { href: "/issues", label: "线上问题", short: "IS", icon: <Siren size={16} /> },
  { href: "/business-knowledge", label: "业务知识", short: "BK", icon: <Library size={16} /> },
  { href: "/experiences", label: "经验", short: "EX", icon: <Lightbulb size={16} /> },
  { href: "/sessions", label: "Sessions", short: "SE", icon: <Server size={16} /> },
  { href: "/schedulers", label: "Schedulers", short: "SC", icon: <Activity size={16} /> },
  { href: "/auth-sites", label: "登录态", short: "AU", icon: <KeyRound size={16} /> },
  { href: "/git-ai", label: "Git AI", short: "AI", icon: <GitBranch size={16} /> },
  { href: "/testdata", label: "造数", short: "TD", icon: <Sparkles size={16} /> },
  { href: "/settings", label: "Settings", short: "ST", icon: <Settings size={16} /> },
]

function isActiveNav(path: string, href: string): boolean {
  if (href === "/dashboard") return path === "/" || path === "/dashboard"
  if (href === "/projects") return path === "/projects" || path === "/requirements" || path === "/requirement" || path === "/requirement-diff" || path === "/requirement-merge" || path === "/requirement-doc"
  if (href === "/release-plan") return path === "/release-plan"
  if (href === "/ones-missing") return path === "/ones-missing"
  if (href === "/issues") return path === "/issues"
  if (href === "/business-knowledge") return path === "/business-knowledge"
  if (href === "/experiences") return path === "/experiences"
  if (href === "/schedulers") return path === "/schedulers"
  if (href === "/auth-sites") return path === "/auth-sites"
  if (href === "/git-ai") return path === "/git-ai"
  if (href === "/testdata") return path === "/testdata"
  return path === href
}

function titleForPath(path: string): { eyebrow: string; title: string } {
  if (path === "/" || path === "/dashboard") return { eyebrow: "Dashboard", title: "状态看板" }
  if (path === "/projects" || path === "/requirements") return { eyebrow: "Requirements", title: "需求进度看板" }
  if (path === "/release-plan") return { eyebrow: "Release Plan", title: "发布计划" }
  if (path === "/ones-missing") return { eyebrow: "ONES Missing", title: "ONES 缺失统计" }
  if (path === "/issues") return { eyebrow: "Online Issues", title: "线上问题" }
  if (path === "/business-knowledge") return { eyebrow: "Business Knowledge", title: "业务知识" }
  if (path === "/experiences") return { eyebrow: "Experiences", title: "经验" }
  if (path === "/requirement") return { eyebrow: "Requirement", title: "需求详情" }
  if (path === "/requirement-doc") return { eyebrow: "Requirement Doc", title: "需求文档" }
  if (path === "/requirement-diff") return { eyebrow: "Diff", title: "分支差异" }
  if (path === "/requirement-merge") return { eyebrow: "Merge", title: "分支合并" }
  if (path === "/sessions") return { eyebrow: "Pi Sessions", title: "Sessions" }
  if (path === "/session") return { eyebrow: "Session", title: "Session 详情" }
  if (path === "/schedulers") return { eyebrow: "Schedulers", title: "定时任务" }
  if (path === "/auth-sites") return { eyebrow: "Browser Auth", title: "Chrome 登录态复用" }
  if (path === "/git-ai") return { eyebrow: "Git AI", title: "漏标检查" }
  if (path === "/settings") return { eyebrow: "Settings", title: "Settings" }
  if (path === "/testdata") return { eyebrow: "Test Data", title: "测试造数" }
  return { eyebrow: "Agent Panel", title: "React + Rust" }
}

function AppShell({ path, children, project, onProjectChange }: { path: string; children: React.ReactNode; project: string; onProjectChange: (v: string) => void }) {
  const meta = titleForPath(path)
  const [sidebarCollapsed, setSidebarCollapsed] = useState(readSidebarCollapsed)
  const toggleSidebar = () => setSidebarCollapsed((v) => { persistSidebarCollapsed(!v); return !v })
  return <div className={`react-shell ${sidebarCollapsed ? "collapsed" : ""}`}><aside className="react-sidebar"><a className="react-brand" href="/dashboard" aria-label="Agent Panel home"><span className="react-brand-mark">AP</span><span className="react-brand-copy"><strong>Agent</strong><em>Panel</em></span></a><nav className="react-sidebar-nav" aria-label="Primary navigation">{navItems.map((item) => <a key={item.href} href={item.href} className={`react-sidebar-link ${isActiveNav(path, item.href) ? "active" : ""}`} title={sidebarCollapsed ? item.label : undefined}><span className="react-sidebar-icon">{item.icon}</span><span>{item.label}</span><small>{item.short}</small></a>)}</nav><div className="react-sidebar-card"><span>RUST BACKEND</span><strong>localhost:7331</strong><em>PTY removed</em></div></aside><div className="react-content-shell"><header className="react-topbar"><div className="react-topbar-title"><button type="button" className="react-topbar-collapse" onClick={toggleSidebar} aria-label={sidebarCollapsed ? "展开侧边栏" : "收起侧边栏"} title={sidebarCollapsed ? "展开侧边栏" : "收起侧边栏"}>{sidebarCollapsed ? <PanelLeftOpen size={16} /> : <PanelLeftClose size={16} />}</button><div className="react-topbar-heading"><span>{meta.eyebrow}</span><strong>{meta.title}</strong></div></div><div className="react-topbar-actions"><HarnessSwitcher /><label className="react-project-select">项目<select value={project} onChange={(e) => onProjectChange(e.target.value)}>{PROJECT_OPTIONS.map((o) => <option key={o.value} value={o.value}>{o.label}</option>)}</select></label><a href="/dashboard">Home</a><button type="button" onClick={() => window.location.reload()}>Refresh</button></div></header><main className="react-main">{children}</main></div></div>
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

export function App({ apiPath }: AppProps) {
  const key = useLocationKey()
  const path = window.location.pathname
  const { project, setProject } = useProjectFilter()
  const page = path === "/" || path === "/dashboard" ? <DashboardPage apiPath={apiPath} project={project} />
    : path === "/projects" || path === "/requirements" ? <ProjectsPage globalProject={project} />
    : path === "/release-plan" ? <ReleasePlanPage globalProject={project} />
    : path === "/ones-missing" ? <OnesMissingPage globalProject={project} />
    : path === "/issues" ? <IssuesPage globalProject={project} />
    : path === "/business-knowledge" ? <KnowledgePage kind="businessKnowledge" />
    : path === "/experiences" ? <KnowledgePage kind="experience" />
    : path === "/sessions" ? <SessionsPage />
    : path === "/session" ? <SessionPage />
    : path === "/requirement" ? <RequirementPage />
    : path === "/requirement-doc" ? <RequirementDocPage />
    : path === "/requirement-diff" ? <RequirementDiffPage />
    : path === "/requirement-merge" ? <RequirementMergePage />
    : path === "/schedulers" ? <SchedulersPage />
    : path === "/auth-sites" ? <AuthSitesPage />
    : path === "/git-ai" ? <GitAiPage />
    : path === "/testdata" ? <TestdataPage />
    : path === "/settings" ? <SettingsPage />
    : path === "/reports" || path === "/report" ? <RemovedPage title="Experience Reports 已移除" detail="OpenCode 经验报告、confirm/reject 和 auto-summary 链路不再保留。" />
    : path === "/env-vars" ? <RemovedPage title="Env Vars 已移除" detail="Rust 版暂未恢复浏览器环境变量编辑。" />
    : <NotFoundPage />

  return <AppShell path={path} project={project} onProjectChange={setProject}><AnimatePresence mode="wait"><motion.div key={key} initial={{ opacity: 0, y: 10 }} animate={{ opacity: 1, y: 0 }} exit={{ opacity: 0, y: -6 }} transition={{ duration: 0.22 }}>{page}</motion.div></AnimatePresence></AppShell>
}
