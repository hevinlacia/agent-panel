import { ArrowLeft, RefreshCw, Sparkles } from "lucide-react"
import { useEffect, useState } from "react"
import type { TestdataCapabilitiesPayload, TestdataCapabilityPayload, TestdataRunPayload } from "../types"
import { fetchJson, postJson, useFetch } from "../lib/api"
import { EmptyCard, ErrorCard, LoadingCard, PageChrome, PanelHead } from "../components/ui"

export function TestdataPage() {
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
