/**
 * Role: 接口测试页 — 整合 WMS testdata pack 的两层能力：
 * 1) API 接口目录（api/catalog.yaml + .bru 模板）：造数/测试/线上修数据标记、关键字筛选；
 *    卡片点击整页跳转 /testdata/api?id=<api-id> 详情页（入参/请求头/请求体编辑与触发、
 *    真实环境 curl 凭证均在详情页）。
 * 2) 造数脚本（capabilities.yaml）：目标状态 + CLI 参数（含 count 造数数量），
 *    dry-run 预览 / 执行真实创建。
 * Public surface: TestdataPage used by web/src/App.tsx route /testdata（侧边栏「接口测试」）；
 * 另导出 KIND_META / STATUS_META 供详情页 web/src/pages/testdata-api-detail.tsx 共享。
 * Constraints: 触发/执行均走后端代理（/api/apitest/trigger、/api/testdata/run）。
 * Read-this-with: web/src/pages/testdata-api-detail.tsx（接口详情页）、
 * src/api_catalog.rs（catalog/bru/curl 解析与触发契约）、src/capability.rs、
 * web/src/pages/projects.tsx（列表卡样式基准）。
 */
import { FlaskConical, Play, RefreshCw } from "lucide-react"
import { useEffect, useMemo, useState } from "react"
import type {
  ApitestApisPayload,
  TestdataCapabilitiesPayload,
  TestdataCapabilityPayload,
  TestdataRunPayload,
} from "../types"
import { fetchJson, postJson, useFetch } from "../lib/api"
import { EmptyCard, ErrorCard, LoadingCard, PageChrome, PanelHead } from "../components/ui"

/** 接口类型徽标（create 造数 / test 测试 / prod_fix 线上修数据），列表与详情页共享。 */
export const KIND_META: Record<string, { label: string; color: string; bg: string }> = {
  create: { label: "造数", color: "#67e8f9", bg: "rgba(34, 211, 238, 0.12)" },
  test: { label: "测试", color: "#86efac", bg: "rgba(34, 197, 94, 0.12)" },
  prod_fix: { label: "线上修数据", color: "#fda4af", bg: "rgba(244, 63, 94, 0.14)" },
}

/** 模板验证状态徽标，列表与详情页共享。 */
export const STATUS_META: Record<string, { label: string; color: string }> = {
  template_verified: { label: "已验证", color: "#4ade80" },
  template_candidate: { label: "候选模板", color: "#fbbf24" },
  deprecated_candidate: { label: "已废弃", color: "#94a3b8" },
}

/** 接口目录（纯列表）：筛选 + 卡片点击跳转详情页。 */
function ApiCatalogSection() {
  const apis = useFetch<ApitestApisPayload>("/api/apitest/apis")
  const [keyword, setKeyword] = useState("")
  const [domain, setDomain] = useState("")
  const [kind, setKind] = useState("all")

  const domains = useMemo(() => [...new Set((apis.data?.apis || []).map((a) => a.domain))], [apis.data])
  const items = useMemo(() => (apis.data?.apis || []).filter((a) => {
    if (domain && a.domain !== domain) return false
    if (kind !== "all" && a.kind !== kind) return false
    const kw = keyword.trim().toLowerCase()
    if (!kw) return true
    return `${a.id} ${a.purpose} ${a.url} ${a.tags.join(" ")}`.toLowerCase().includes(kw)
  }), [apis.data, domain, kind, keyword])

  const openDetail = (id: string) => {
    window.location.assign(`/testdata/api?id=${encodeURIComponent(id)}`)
  }

  return <section className="react-panel">
    <PanelHead kicker="API Catalog" title="接口目录" chip={`${items.length} 条`} />
    <div className="react-manhour-filters">
      <label>关键字 <input value={keyword} onChange={(e) => setKeyword(e.target.value)} placeholder="id / 用途 / URL / 标签" /></label>
      <label>域 <select value={domain} onChange={(e) => setDomain(e.target.value)}><option value="">全部</option>{domains.map((d) => <option key={d} value={d}>{d}</option>)}</select></label>
      <label>类型 <select value={kind} onChange={(e) => setKind(e.target.value)}><option value="all">全部</option><option value="create">造数</option><option value="test">测试</option><option value="prod_fix">线上修数据</option></select></label>
      <button type="button" className="react-ghost-btn" onClick={() => apis.refresh()} disabled={apis.loading}><RefreshCw size={13} className={apis.loading ? "react-spin" : ""} />刷新目录</button>
    </div>
    {apis.error ? <ErrorCard error={apis.error} /> : apis.loading ? <LoadingCard /> : !items.length ? <EmptyCard>没有匹配的接口：调整筛选条件。</EmptyCard> : <div className="react-card-list">{items.map((a) => {
      const kindMeta = KIND_META[a.kind] || KIND_META.create
      const statusMeta = STATUS_META[a.status]
      return <article key={a.id} className="react-list-card" onClick={() => openDetail(a.id)} style={{ cursor: "pointer" }} title="点击进入接口详情（入参/请求头编辑与触发）">
        <div>
          <span className="react-card-id">{a.domain} · {a.method}</span>
          <h3><code>{a.id}</code></h3>
          <p className="react-muted">{a.purpose}</p>
          <div className="react-card-meta">
            <span className="react-status-pill" style={{ color: kindMeta.color, background: kindMeta.bg, borderColor: kindMeta.color }}>{kindMeta.label}</span>
            {statusMeta ? <span className="react-status-pill" style={{ color: statusMeta.color, borderColor: statusMeta.color }} title={`模板状态 ${a.status}`}>{statusMeta.label}</span> : null}
            {a.hasRequestSchema ? <span title="有请求 schema">schema ✓</span> : null}
            {a.placeholderCount ? <span title="需关注/可覆盖的占位符数量">{a.placeholderCount} 占位符</span> : null}
          </div>
        </div>
        <div className="react-card-side"><code className="react-muted" title={a.url}>{a.url.replace("{{baseUrl}}", "…")}</code></div>
      </article>
    })}</div>}
  </section>
}

/** 造数脚本区：capabilities.yaml 能力（目标状态 + CLI 配置 + dry-run/执行）。 */
function CapabilityScriptSection() {
  const caps = useFetch<TestdataCapabilitiesPayload>("/api/capabilities?project=WMS")
  const [selectedId, setSelectedId] = useState("")
  const [detail, setDetail] = useState<TestdataCapabilityPayload | null>(null)
  const [detailLoading, setDetailLoading] = useState(false)
  const [target, setTarget] = useState("")
  const [env, setEnv] = useState("test")
  const [params, setParams] = useState<Record<string, string>>({})
  const [runResult, setRunResult] = useState<TestdataRunPayload | null>(null)
  const [running, setRunning] = useState(false)
  const [runError, setRunError] = useState<string | null>(null)

  const loadDetail = async (id: string) => {
    setSelectedId(id)
    setRunResult(null)
    setRunError(null)
    if (!id) { setDetail(null); return }
    setDetailLoading(true)
    try {
      const d = await fetchJson<TestdataCapabilityPayload>(`/api/capability?id=${encodeURIComponent(id)}&project=WMS`)
      setDetail(d)
      const firstTarget = (d.capability?.targets || []).find((t) => t.verified) || (d.capability?.targets || [])[0]
      setTarget(firstTarget?.name || "")
      const init: Record<string, string> = {}
      for (const [key, spec] of Object.entries(d.capability?.cli || {})) {
        if (key === "target" || key === "env") continue
        if (spec.default !== undefined) init[key] = String(spec.default)
      }
      setParams(init)
    } catch {
      setDetail(null)
    } finally {
      setDetailLoading(false)
    }
  }

  useEffect(() => { if (selectedId) loadDetail(selectedId) }, [selectedId])

  const run = async (execute: boolean) => {
    if (!selectedId || !target) return
    setRunning(true)
    setRunError(null)
    try {
      const res = await postJson<TestdataRunPayload>("/api/testdata/run", {
        project: "WMS",
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

  const cliFields = detail?.capability?.cli || {}
  const currentCap = (caps.data?.capabilities || []).find((c) => c.id === selectedId)

  return <section className="react-panel">
    <PanelHead kicker="Data Creation Scripts" title="造数脚本" chip={`${(caps.data?.capabilities || []).length} 个能力`} />
    <p className="react-muted">状态机造数脚本（出库/入库/盘点等完整链路自动推进）：选择能力与目标状态，配置数量等参数后预览或执行；登录态与接口调用由脚本内部处理。</p>
    {caps.error ? <ErrorCard error={caps.error} /> : caps.loading ? <LoadingCard /> : <div className="react-card-list">{(caps.data?.capabilities || []).map((c) => <article key={c.id} className={`react-list-card ${selectedId === c.id ? "react-cap-active" : ""}`} onClick={() => setSelectedId(c.id)} style={{ cursor: "pointer" }}><div><span className="react-card-id">{c.domain}</span><h3>{c.purpose}</h3><p className="react-muted">{c.id} · {c.execution}</p></div><div className="react-card-side"><span className="react-effort-badge">{c.script ? "script" : "recipe"}</span></div></article>)}</div>}
    {selectedId ? <div className="react-apitest-detail"><PanelHead kicker="Configure" title="造数配置" chip={detailLoading ? "loading" : currentCap?.id} />
      {detailLoading ? <LoadingCard /> : detail?.capability ? <>
        <div className="react-meta-grid"><span>能力 {detail.capability.purpose}</span><span>目标对象 {detail.capability.object}</span><span>验证环境 {detail.capability.verified_env || "-"}{detail.capability.verified_date ? `（${detail.capability.verified_date}）` : ""}</span><span>脚本 <code>{detail.capability.script || "-"}</code></span></div>
      {detail.capability.invocation ? <code className="react-command">{detail.capability.invocation}</code> : null}
      {detail.capability.recipe || detail.capability.state_graph ? <p className="react-muted">关联资产：{detail.capability.recipe ? <code>{detail.capability.recipe}</code> : null}{detail.capability.state_graph ? <> · <code>{detail.capability.state_graph}</code></> : null}</p> : null}
      {detail.capability.pitfalls?.length ? <details className="react-review-commits"><summary>相关踩坑笔记（{detail.capability.pitfalls.length}）</summary><div className="react-card-meta">{detail.capability.pitfalls.map((p) => <span key={p} className="react-linked-issue-chip"><code>{p}</code></span>)}</div></details> : null}
      <div className="react-tab-row">
          <button className={env === "test" ? "active" : ""} onClick={() => setEnv("test")}>test 环境</button>
          <button className={env === "uat-cn" ? "active" : ""} onClick={() => setEnv("uat-cn")}>UAT-CN 环境</button>
        </div>
        <label className="react-editor-label">目标状态 (target)<select value={target} onChange={(e) => setTarget(e.target.value)}>{(detail.capability.targets || []).map((t) => <option key={t.name} value={t.name}>{t.name} ({t.label}{t.verified ? "" : "·待验证"})</option>)}</select></label>
        <div className="react-settings-grid">{Object.entries(cliFields).filter(([k]) => k !== "target" && k !== "env").map(([key, spec]) => <label key={key} title={spec.description || undefined}>{key}{spec.description ? <em className="react-muted" style={{ textTransform: "none", letterSpacing: 0 }}>{spec.description}</em> : null}{spec.choices ? <select value={params[key] || ""} onChange={(e) => setParams({ ...params, [key]: e.target.value })}>{spec.choices.map((c) => <option key={c} value={c}>{c}</option>)}</select> : <input value={params[key] || ""} onChange={(e) => setParams({ ...params, [key]: e.target.value })} placeholder={spec.default !== undefined ? String(spec.default) : spec.description || ""} />}</label>)}</div>
        <div className="react-actions">
          <button onClick={() => run(false)} disabled={running || !target}>预览命令 (dry-run)</button>
          <button onClick={() => run(true)} disabled={running || !target} className="react-fix-note-btn"><Play size={13} />{running ? "执行中…" : "执行造数"}</button>
        </div>
        <p className="react-muted">⚠️ {env === "uat-cn" ? "UAT 造数为真实数据，且 MySQL 只读（Archery）；" : "test 造数为真实数据；"}建议先 dry-run 预览再执行。</p>
      </> : <EmptyCard>未加载能力详情</EmptyCard>}
      {runError ? <ErrorCard error={runError} /> : runResult ? <div className="react-apitest-result"><PanelHead kicker="Result" title={runResult.executed ? "执行结果" : "命令预览"} chip={runResult.executed ? `exit ${runResult.exitCode ?? "-"}` : "dry-run"} />
        <code className="react-command">{runResult.command}</code>
        <p className="react-muted">cwd: {runResult.cwd}</p>
        {runResult.executed && runResult.stdout && parseRunSummary(runResult.stdout) ? (() => {
          const s = parseRunSummary(runResult.stdout)!
          return <details className="react-review-commits" open><summary>造数结果：{s.success}/{s.total} 成功 · 目标 {s.target}</summary>
            <table className="react-manhour-table"><thead><tr><th>#</th><th>单号</th><th>最终状态</th><th>结果</th><th>备注</th></tr></thead><tbody>
              {(s.results || []).map((r, idx) => <tr key={idx}><td>{r.index ?? idx + 1}</td><td><code>{r.code ?? "-"}</code></td><td>{r.final_status ?? "-"}</td><td>{r.success ? "✅" : "❌"}</td><td>{r.error || r.steps || ""}</td></tr>)}
            </tbody></table>
          </details>
        })() : null}
        {runResult.executed && runResult.stdout ? <details className="react-review-commits" {...(parseRunSummary(runResult.stdout) ? {} : { open: true })}><summary>stdout 原文</summary><pre>{runResult.stdout}</pre></details> : null}
        {runResult.executed && runResult.stderr ? <details className="react-review-commits"><summary>stderr</summary><pre>{runResult.stderr}</pre></details> : null}
      </div> : null}
    </div> : null}
  </section>
}

/** 解析造数脚本 stdout JSON 摘要（stdout_json: true 的能力输出 {target,total,success,results[]}）。 */
interface RunSummary {
  target?: string
  total?: number
  success?: number
  results?: { index?: number; code?: string; final_status?: number | string; success?: boolean; error?: string; steps?: string[] }[]
}

function parseRunSummary(stdout: string): RunSummary | null {
  try {
    const start = stdout.indexOf("{")
    const v = JSON.parse(stdout.slice(start)) as RunSummary
    if (v && Array.isArray(v.results)) return v
  } catch { /* 非 JSON 输出走原文展示 */ }
  return null
}

export function TestdataPage() {
  return <PageChrome icon={<FlaskConical size={15} />} eyebrow="API Test" title="接口测试" description="WMS 已登记测试接口：造数/测试/线上修数据标记、关键字筛选；点击卡片进入接口详情页（入参/请求头/请求体临时编辑与触发、真实环境 curl 凭证线上修数）。下方保留状态机造数脚本的配置与执行。">
    <ApiCatalogSection />
    <CapabilityScriptSection />
  </PageChrome>
}
