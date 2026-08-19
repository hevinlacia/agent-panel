import { Activity, Clock3, Gauge, Sparkles } from "lucide-react"
import type { AutoDrivePayload, ConfigPayload } from "../types"
import { useFetch } from "../lib/api"
import { ErrorCard, KpiCard, LoadingCard, PageChrome, PanelHead } from "../components/ui"

export function SchedulersPage() {
  const config = useFetch<ConfigPayload>("/api/config")
  const autoDrive = useFetch<AutoDrivePayload>("/api/requirement/auto-drive")
  return <PageChrome icon={<Activity size={15} />} eyebrow="Schedulers" title="定时任务查看" description="查看当前 Rust 版保留的调度配置和队列状态。"><section className="react-kpi-grid"><KpiCard icon={<Activity size={20} />} label="Full Sync" value={config.data?.fullSyncSchedule ? "ON" : "OFF"} sub={(config.data?.fullSyncTimes || []).join(" / ") || "no schedule"} tone="active" /><KpiCard icon={<Clock3 size={20} />} label="Auto Drive" value={autoDrive.data?.active ?? 0} sub={`blocked ${autoDrive.data?.blocked ?? 0}`} tone="avg" /><KpiCard icon={<Sparkles size={20} />} label="Repos" value={config.data?.fullSyncGithubRepos?.length ?? 0} sub="full sync repos" tone="done" /><KpiCard icon={<Gauge size={20} />} label="Queue" value={autoDrive.data?.queue?.queued ?? 0} sub="queued jobs" tone="total" /></section><section className="react-panel"><PanelHead kicker="Config" title="定时任务配置" chip={config.loading ? "loading" : "config"} />{config.error ? <ErrorCard error={config.error} /> : <div className="react-meta-grid"><span>Full sync schedule</span><span>{config.data?.fullSyncSchedule ? "开启" : "关闭"}</span><span>Full sync times</span><span>{(config.data?.fullSyncTimes || []).join(" / ") || "-"}</span><span>Full sync repos</span><span>{config.data?.fullSyncGithubRepos?.length || 0}</span><span>Auto drive message</span><span>{autoDrive.data?.message || "Rust rewrite currently exposes queue state only"}</span></div>}</section></PageChrome>
}
