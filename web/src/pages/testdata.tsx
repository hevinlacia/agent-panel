/**
 * Role: 接口测试页 — 整合 WMS testdata pack 的两层能力：
 * 1) API 接口目录（api/catalog.yaml + .bru 模板）：造数/测试标记、关键字筛选、
 *    详情查看（入参占位符/出参 schema/tests/notes）、切换 test/uat-cn/uat-sea 直接触发；
 *    触发时临时修改入参（内存编辑不写回模板），登录态由后端经 login.mjs 自动获取。
 * 2) 造数脚本（capabilities.yaml）：目标状态 + CLI 参数（含 count 造数数量），
 *    dry-run 预览 / 执行真实创建。
 * Public surface: TestdataPage used by web/src/App.tsx route /testdata（侧边栏「接口测试」）.
 * Constraints: 触发/执行均走后端代理（/api/apitest/trigger、/api/testdata/run），
 * token 不经过浏览器；入参值仅 localStorage 记忆，不回写 .bru。
 * Read-this-with: src/api_catalog.rs（catalog/bru 解析与触发契约）、src/capability.rs、
 * web/src/pages/projects.tsx（列表卡样式基准）。
 */
import { FlaskConical, Play, RefreshCw, Send } from "lucide-react"
import { useEffect, useMemo, useState } from "react"
import type {
  ApitestApisPayload,
  ApitestDetailPayload,
  ApitestTriggerPayload,
  TestdataCapabilitiesPayload,
  TestdataCapabilityPayload,
  TestdataRunPayload,
} from "../types"
import { fetchJson, postJson, useFetch } from "../lib/api"
import { EmptyCard, ErrorCard, LoadingCard, PageChrome, PanelHead } from "../components/ui"

const ENV_TABS = [
  { value: "test", label: "test 环境" },
  { value: "uat-cn", label: "UAT-CN 环境" },
  { value: "uat-sea", label: "UAT-SEA 环境" },
]

const KIND_META: Record<string, { label: string; color: string; bg: string }> = {
  create: { label: "造数", color: "#67e8f9", bg: "rgba(34, 211, 238, 0.12)" },
  test: { label: "测试", color: "#86efac", bg: "rgba(34, 197, 94, 0.12)" },
}

const STATUS_META: Record<string, { label: string; color: string }> = {
  template_verified: { label: "已验证", color: "#4ade80" },
  template_candidate: { label: "候选模板", color: "#fbbf24" },
  deprecated_candidate: { label: "已废弃", color: "#94a3b8" },
}

const VAR_MEMORY_PREFIX = "react-apitest-var-"

function readVarMemory(name: string): string {
  try { return localStorage.getItem(VAR_MEMORY_PREFIX + name) || "" } catch { return "" }
}

function writeVarMemory(name: string, value: string) {
  try {
    if (value) localStorage.setItem(VAR_MEMORY_PREFIX + name, value)
    else localStorage.removeItem(VAR_MEMORY_PREFIX + name)
  } catch { /* ignore */ }
}

/** 接口目录 + 详情 + 触发。 */
function ApiCatalogSection() {
  const apis = useFetch<ApitestApisPayload>("/api/apitest/apis")
  const [keyword, setKeyword] = useState("")
  const [domain, setDomain] = useState("")
  const [kind, setKind] = useState("all")
  const [selectedId, setSelectedId] = useState("")
  const [detail, setDetail] = useState<ApitestDetailPayload | null>(null)
  const [detailLoading, setDetailLoading] = useState(false)
  const [env, setEnv] = useState("test")
  const [vars, setVars] = useState<Record<string, string>>({})
  const [bodyText, setBodyText] = useState("")
  const [triggering, setTriggering] = useState(false)
  const [result, setResult] = useState<ApitestTriggerPayload | null>(null)
  const [triggerError, setTriggerError] = useState<string | null>(null)

  const domains = useMemo(() => [...new Set((apis.data?.apis || []).map((a) => a.domain))], [apis.data])
  const items = useMemo(() => (apis.data?.apis || []).filter((a) => {
    if (domain && a.domain !== domain) return false
    if (kind !== "all" && a.kind !== kind) return false
    const kw = keyword.trim().toLowerCase()
    if (!kw) return true
    return `${a.id} ${a.purpose} ${a.url} ${a.tags.join(" ")}`.toLowerCase().includes(kw)
  }), [apis.data, domain, kind, keyword])

  const loadDetail = async (id: string) => {
    setSelectedId(id)
    setResult(null)
    setTriggerError(null)
    if (!id) { setDetail(null); return }
    setDetailLoading(true)
    try {
      const d = await fetchJson<ApitestDetailPayload>(`/api/apitest/api?id=${encodeURIComponent(id)}`)
      setDetail(d)
      const init: Record<string, string> = {}
      for (const p of d.placeholders || []) {
        if (p.derived) continue
        const remembered = readVarMemory(p.name)
        if (remembered) init[p.name] = remembered
      }
      setVars(init)
      setBodyText(d.template?.body || "")
    } catch {
      setDetail(null)
    } finally {
      setDetailLoading(false)
    }
  }

  const setVar = (name: string, value: string) => {
    setVars((prev) => ({ ...prev, [name]: value }))
    if (!detail?.placeholders.find((p) => p.name === name)?.derived) writeVarMemory(name, value)
  }

  const trigger = async () => {
    if (triggering || !selectedId) return
    setTriggering(true)
    setTriggerError(null)
    setResult(null)
    try {
      const filled = Object.fromEntries(Object.entries(vars).filter(([, v]) => v.trim() !== ""))
      const res = await postJson<ApitestTriggerPayload>("/api/apitest/trigger", {
        id: selectedId,
        env,
        vars: filled,
        bodyOverride: bodyText.trim() === (detail?.template?.body || "").trim() ? undefined : bodyText,
      })
      setResult(res)
      if (!res.ok && res.missing?.length) setTriggerError(`缺少入参：${res.missing.join("、")}`)
    } catch (e) {
      setTriggerError(e instanceof Error ? e.message : String(e))
    } finally {
      setTriggering(false)
    }
  }

  const selected = items.find((a) => a.id === selectedId)

  return <section className="react-panel">
    <PanelHead kicker="API Catalog" title="接口目录" chip={`${items.length} 条`} />
    <div className="react-manhour-filters">
      <label>关键字 <input value={keyword} onChange={(e) => setKeyword(e.target.value)} placeholder="id / 用途 / URL / 标签" /></label>
      <label>域 <select value={domain} onChange={(e) => setDomain(e.target.value)}><option value="">全部</option>{domains.map((d) => <option key={d} value={d}>{d}</option>)}</select></label>
      <label>类型 <select value={kind} onChange={(e) => setKind(e.target.value)}><option value="all">全部</option><option value="create">造数</option><option value="test">测试</option></select></label>
      <button type="button" className="react-ghost-btn" onClick={() => apis.refresh()} disabled={apis.loading}><RefreshCw size={13} className={apis.loading ? "react-spin" : ""} />刷新目录</button>
    </div>
    {apis.error ? <ErrorCard error={apis.error} /> : apis.loading ? <LoadingCard /> : !items.length ? <EmptyCard>没有匹配的接口：调整筛选条件。</EmptyCard> : <div className="react-card-list">{items.map((a) => {
      const kindMeta = KIND_META[a.kind] || KIND_META.create
      const statusMeta = STATUS_META[a.status]
      return <article key={a.id} className={`react-list-card ${selectedId === a.id ? "react-cap-active" : ""}`} onClick={() => loadDetail(a.id)} style={{ cursor: "pointer" }}>
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
    {selected && detailLoading ? <LoadingCard /> : null}
    {selected && detail?.api ? <div className="react-apitest-detail">
      <PanelHead kicker="Trigger" title={`触发接口 · ${detail.api.id}`} chip={selected.method} />
      <div className="react-meta-grid">
        <span>用途 {detail.api.purpose}</span>
        <span>URL <code className="react-apitest-url">{detail.api.url}</code></span>
        <span>环境支持 {Object.entries(detail.api.envSupport || {}).map(([k, v]) => `${k}:${v}`).join(" · ") || "-"}</span>
        <span>模板状态 {STATUS_META[detail.api.status]?.label || detail.api.status}</span>
      </div>
      {detail.api.notes ? <p className="react-muted">📝 {detail.api.notes}</p> : null}
      <div className="react-tab-row">{ENV_TABS.map((t) => <button key={t.value} className={env === t.value ? "active" : ""} onClick={() => setEnv(t.value)}>{t.label}</button>)}</div>
      {detail.placeholders.length ? <div className="react-settings-grid">
        {detail.placeholders.map((p) => <label key={p.name}>
          {p.name}{p.derived ? <em className="react-muted" title="登录态/环境自动注入，留空即可">（自动注入）</em> : null}
          <input value={vars[p.name] || ""} onChange={(e) => setVar(p.name, e.target.value)} placeholder={p.derived ? "留空自动注入" : p.name} />
        </label>)}
      </div> : <p className="react-muted">该接口无占位符入参。</p>}
      {detail.template.body ? <label className="react-editor-label">请求体（可临时修改，不影响模板）<textarea className="react-apitest-body" rows={10} value={bodyText} onChange={(e) => setBodyText(e.target.value)} spellCheck={false} /></label> : null}
      {detail.requestSchema ? <details className="react-review-commits"><summary>请求 schema（{detail.requestSchemaPath}）</summary><pre>{detail.requestSchema}</pre></details> : null}
      {detail.tests.length ? <details className="react-review-commits"><summary>出参断言（.bru tests，{detail.tests.length}）</summary><pre>{detail.tests.join("\n")}</pre></details> : null}
      <div className="react-actions">
        <button type="button" onClick={trigger} disabled={triggering}><Send size={13} />{triggering ? "触发中…" : `触发接口（${ENV_TABS.find((t) => t.value === env)?.label}）`}</button>
      </div>
      <p className="react-muted">登录态自动获取（Chrome 登录态优先 → 缓存 → 账密刷新）；入参值仅本机记忆，不写回接口模板。</p>
      {triggerError ? <p className="react-effort-error">{triggerError}</p> : null}
      {result ? <div className="react-apitest-result">
        <PanelHead kicker="Response" title="触发结果" chip={result.status != null ? `HTTP ${result.status} · ${result.durationMs}ms` : "未发送"} />
        {result.missing?.length ? <p className="react-effort-error">缺少入参：{result.missing.join("、")}</p> : null}
        {result.url ? <code className="react-command">{result.env} → {result.url}</code> : null}
        {result.bodyJson && result.bodyJson !== null ? <pre className="react-apitest-json">{JSON.stringify(result.bodyJson, null, 2)}</pre> : result.bodyText ? <pre className="react-apitest-json">{result.bodyText}</pre> : null}
        {result.note ? <p className="react-muted">{result.note}</p> : null}
      </div> : null}
    </div> : null}
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
        <div className="react-meta-grid"><span>能力 {detail.capability.purpose}</span><span>目标对象 {detail.capability.object}</span><span>验证环境 test</span></div>
        <div className="react-tab-row">
          <button className={env === "test" ? "active" : ""} onClick={() => setEnv("test")}>test 环境</button>
          <button className={env === "uat-cn" ? "active" : ""} onClick={() => setEnv("uat-cn")}>UAT-CN 环境</button>
        </div>
        <label className="react-editor-label">目标状态 (target)<select value={target} onChange={(e) => setTarget(e.target.value)}>{(detail.capability.targets || []).map((t) => <option key={t.name} value={t.name}>{t.name} ({t.label}{t.verified ? "" : "·待验证"})</option>)}</select></label>
        <div className="react-settings-grid">{Object.entries(cliFields).filter(([k]) => k !== "target" && k !== "env").map(([key, spec]) => <label key={key}>{key}{spec.choices ? <select value={params[key] || ""} onChange={(e) => setParams({ ...params, [key]: e.target.value })}>{spec.choices.map((c) => <option key={c} value={c}>{c}</option>)}</select> : <input value={params[key] || ""} onChange={(e) => setParams({ ...params, [key]: e.target.value })} placeholder={spec.default !== undefined ? String(spec.default) : ""} />}</label>)}</div>
        <div className="react-actions">
          <button onClick={() => run(false)} disabled={running || !target}>预览命令 (dry-run)</button>
          <button onClick={() => run(true)} disabled={running || !target} className="react-fix-note-btn"><Play size={13} />{running ? "执行中…" : "执行造数"}</button>
        </div>
        <p className="react-muted">⚠️ {env === "uat-cn" ? "UAT 造数为真实数据，且 MySQL 只读（Archery）；" : "test 造数为真实数据；"}建议先 dry-run 预览再执行。</p>
      </> : <EmptyCard>未加载能力详情</EmptyCard>}
      {runError ? <ErrorCard error={runError} /> : runResult ? <div className="react-apitest-result"><PanelHead kicker="Result" title={runResult.executed ? "执行结果" : "命令预览"} chip={runResult.executed ? `exit ${runResult.exitCode ?? "-"}` : "dry-run"} />
        <code className="react-command">{runResult.command}</code>
        <p className="react-muted">cwd: {runResult.cwd}</p>
        {runResult.executed && runResult.stdout ? <details className="react-review-commits" open><summary>stdout</summary><pre>{runResult.stdout}</pre></details> : null}
        {runResult.executed && runResult.stderr ? <details className="react-review-commits"><summary>stderr</summary><pre>{runResult.stderr}</pre></details> : null}
      </div> : null}
    </div> : null}
  </section>
}

export function TestdataPage() {
  return <PageChrome icon={<FlaskConical size={15} />} eyebrow="API Test" title="接口测试" description="WMS 已登记测试接口：造数/测试标记、关键字筛选、查看入参出参并直接触发（可切 test/UAT，临时修改入参不写回模板）；下方保留状态机造数脚本的配置与执行。">
    <ApiCatalogSection />
    <CapabilityScriptSection />
  </PageChrome>
}
