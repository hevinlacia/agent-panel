/**
 * Role: 接口测试页 — 整合 WMS testdata pack 的两层能力：
 * 1) API 接口目录（api/catalog.yaml + .bru 模板）：造数/测试/线上修数据标记、关键字筛选、
 *    卡片点击展开详情（占位符入参/模板请求头/请求体均可临时修改不写回模板）、
 *    切换 test/uat-cn/uat-sea 直接触发；登录态由后端经 login.mjs 自动获取；
 *    支持粘贴真实环境 curl（线上修数场景）：解析出域名与登录态凭证后以该环境发起请求。
 * 2) 造数脚本（capabilities.yaml）：目标状态 + CLI 参数（含 count 造数数量），
 *    dry-run 预览 / 执行真实创建。
 * Public surface: TestdataPage used by web/src/App.tsx route /testdata（侧边栏「接口测试」）.
 * Constraints: 触发/执行均走后端代理（/api/apitest/trigger、/api/testdata/run），
 * token 不落盘；curl 原文仅存本机内存（组件 state），不写回模板/模板目录。
 * Read-this-with: src/api_catalog.rs（catalog/bru/curl 解析与触发契约）、src/capability.rs、
 * web/src/pages/projects.tsx（列表卡样式基准）。
 */
import { FlaskConical, Link2, Play, RefreshCw, Send } from "lucide-react"
import { useEffect, useMemo, useRef, useState } from "react"
import type {
  ApitestApisPayload,
  ApitestCurlImportPayload,
  ApitestDetailPayload,
  ApitestHeaderOverrideInput,
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
  prod_fix: { label: "线上修数据", color: "#fda4af", bg: "rgba(244, 63, 94, 0.14)" },
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
  /** 模板请求头临时编辑：头名 → 当前值 + 是否发送（不写回模板）。 */
  const [headerEdits, setHeaderEdits] = useState<Record<string, { value: string; enabled: boolean }>>({})
  /** 粘贴的真实环境 curl（section 级共享，切换接口不丢失；仅本机内存）。 */
  const [curlText, setCurlText] = useState("")
  const [curlInfo, setCurlInfo] = useState<ApitestCurlImportPayload | null>(null)
  const [curlParsing, setCurlParsing] = useState(false)
  const [curlError, setCurlError] = useState<string | null>(null)
  const [triggering, setTriggering] = useState(false)
  const [result, setResult] = useState<ApitestTriggerPayload | null>(null)
  const [triggerError, setTriggerError] = useState<string | null>(null)
  const detailRef = useRef<HTMLDivElement>(null)

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
      const heads: Record<string, { value: string; enabled: boolean }> = {}
      for (const [n, v] of d.template?.headers || []) heads[n] = { value: v, enabled: true }
      setHeaderEdits(heads)
    } catch {
      setDetail(null)
    } finally {
      setDetailLoading(false)
    }
  }

  // 卡片在长列表上方，详情在列表下方：选中后自动滚动定位，否则看起来“点了没反应”。
  useEffect(() => {
    if (detail?.api && selectedId && !detailLoading) {
      detailRef.current?.scrollIntoView({ behavior: "smooth", block: "start" })
    }
  }, [detail, selectedId, detailLoading])

  const setHeaderEdit = (name: string, patch: Partial<{ value: string; enabled: boolean }>) => {
    setHeaderEdits((prev) => ({
      ...prev,
      [name]: { ...(prev[name] || { value: "", enabled: true }), ...patch },
    }))
  }

  const importCurl = async () => {
    if (!curlText.trim() || curlParsing) return
    setCurlParsing(true)
    setCurlError(null)
    try {
      setCurlInfo(await postJson<ApitestCurlImportPayload>("/api/apitest/curl-import", { curl: curlText }))
    } catch (e) {
      setCurlInfo(null)
      setCurlError(e instanceof Error ? e.message : String(e))
    } finally {
      setCurlParsing(false)
    }
  }

  const setVar = (name: string, value: string) => {
    setVars((prev) => ({ ...prev, [name]: value }))
    if (!detail?.placeholders.find((p) => p.name === name)?.derived) writeVarMemory(name, value)
  }

  const run = async (dryRun: boolean) => {
    if (triggering || !selectedId) return
    setTriggering(true)
    setTriggerError(null)
    setResult(null)
    try {
      const filled = Object.fromEntries(Object.entries(vars).filter(([, v]) => v.trim() !== ""))
      // 请求头临时覆盖：只发送有改动的行（改值 → 覆盖；停用 → 移除）。
      const overrides: ApitestHeaderOverrideInput[] = []
      for (const [n, v] of detail?.template?.headers || []) {
        const edit = headerEdits[n]
        if (!edit) continue
        if (!edit.enabled) overrides.push({ name: n, value: null })
        else if (edit.value.trim() !== v.trim()) overrides.push({ name: n, value: edit.value })
      }
      const res = await postJson<ApitestTriggerPayload>("/api/apitest/trigger", {
        id: selectedId,
        env,
        vars: filled,
        bodyOverride: bodyText.trim() === (detail?.template?.body || "").trim() ? undefined : bodyText,
        headerOverrides: overrides.length ? overrides : undefined,
        curl: curlInfo && curlText.trim() ? curlText : undefined,
        dryRun,
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
      <label>类型 <select value={kind} onChange={(e) => setKind(e.target.value)}><option value="all">全部</option><option value="create">造数</option><option value="test">测试</option><option value="prod_fix">线上修数据</option></select></label>
      <button type="button" className="react-ghost-btn" onClick={() => apis.refresh()} disabled={apis.loading}><RefreshCw size={13} className={apis.loading ? "react-spin" : ""} />刷新目录</button>
    </div>
    {apis.error ? <ErrorCard error={apis.error} /> : apis.loading ? <LoadingCard /> : !items.length ? <EmptyCard>没有匹配的接口：调整筛选条件。</EmptyCard> : <div className="react-card-list">{items.map((a) => {
      const kindMeta = KIND_META[a.kind] || KIND_META.create
      const statusMeta = STATUS_META[a.status]
      return <article key={a.id} className={`react-list-card ${selectedId === a.id ? "react-cap-active" : ""}`} onClick={() => loadDetail(a.id)} style={{ cursor: "pointer" }} title="点击展开入参/请求头编辑与触发">
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
    {selected && detail?.api ? <div className="react-apitest-detail" ref={detailRef}>
      <PanelHead kicker="Trigger" title={`触发接口 · ${detail.api.id}`} chip={selected.method} />
      <div className="react-meta-grid">
        <span>用途 {detail.api.purpose}</span>
        <span>URL <code className="react-apitest-url">{detail.api.url}</code></span>
        <span>环境支持 {Object.entries(detail.api.envSupport || {}).map(([k, v]) => `${k}:${v}`).join(" · ") || "-"}</span>
        <span>模板状态 {STATUS_META[detail.api.status]?.label || detail.api.status}</span>
      </div>
      {detail.api.notes ? <p className="react-muted">📝 {detail.api.notes}</p> : null}
      {curlInfo ? <p className="react-muted">🔗 已启用真实环境 curl 凭证（<code>{curlInfo.origin}</code>）：触发时使用该域名与登录态，下方环境切换不生效。</p> : null}
      <div className="react-tab-row" style={curlInfo ? { opacity: 0.45 } : undefined}>{ENV_TABS.map((t) => <button key={t.value} className={env === t.value ? "active" : ""} onClick={() => setEnv(t.value)}>{t.label}</button>)}</div>
      {detail.placeholders.length ? <div className="react-settings-grid">
        {detail.placeholders.map((p) => <label key={p.name}>
          {p.name}{p.derived ? <em className="react-muted" title="登录态/环境自动注入，留空即可">（自动注入）</em> : null}
          <input value={vars[p.name] || ""} onChange={(e) => setVar(p.name, e.target.value)} placeholder={p.derived ? "留空自动注入" : p.name} />
        </label>)}
      </div> : <p className="react-muted">该接口无占位符入参。</p>}
      {detail.template.headers.length ? <div className="react-editor-label">请求头（模板头，可临时改值/停用，不影响模板；含占位符的仍会自动注入）
        <div className="react-apitest-header-list">
          {detail.template.headers.map(([n, v]) => <div key={n} className="react-apitest-header-row">
            <input type="checkbox" checked={headerEdits[n]?.enabled ?? true} onChange={(e) => setHeaderEdit(n, { enabled: e.target.checked })} title="取消勾选后本次触发不发送该头" />
            <code title={n}>{n}</code>
            <input value={headerEdits[n]?.value ?? v} onChange={(e) => setHeaderEdit(n, { value: e.target.value })} spellCheck={false} />
          </div>)}
        </div>
      </div> : null}
      {detail.template.body ? <label className="react-editor-label">请求体（可临时修改，不影响模板）<textarea className="react-apitest-body" rows={10} value={bodyText} onChange={(e) => setBodyText(e.target.value)} spellCheck={false} /></label> : null}
      <details className="react-review-commits">
        <summary><Link2 size={12} style={{ display: "inline", verticalAlign: "-2px", marginRight: 4 }} />真实环境凭证（粘贴 curl · 线上修数据）{curlInfo ? <span className="react-status-pill" style={{ color: curlInfo.hasToken ? "#4ade80" : "#fbbf24", borderColor: curlInfo.hasToken ? "#4ade80" : "#fbbf24", marginLeft: 6 }}>{curlInfo.hasToken ? "✓ 已加载" : "✗ 无 X-Token"} · {curlInfo.origin}</span> : null}</summary>
        <p className="react-muted">在已登录的真实环境页面打开 DevTools → Network → 任意请求右键 Copy as cURL，枯贴到这里解析：触发时改用该 curl 的域名与登录态发起请求（跳过 login.mjs，不携带 x-env/Cookie）。curl 原文只在本机内存中使用，不落盘、不回显完整凭证。</p>
        <textarea className="react-apitest-body react-curl-textarea" rows={5} value={curlText} onChange={(e) => setCurlText(e.target.value)} placeholder="curl 'https://…/wms-web/…' -H 'X-Token: …' -H 'x-params: …' --data-raw '…'" spellCheck={false} />
        <div className="react-actions">
          <button type="button" onClick={importCurl} disabled={curlParsing || !curlText.trim()}><Link2 size={13} className={curlParsing ? "react-spin" : ""} />{curlParsing ? "解析中…" : "解析 curl 凭证"}</button>
          {curlInfo ? <button type="button" className="react-ghost-btn" onClick={() => { setCurlInfo(null); setCurlText(""); setCurlError(null) }}>清除凭证</button> : null}
        </div>
        {curlError ? <p className="react-effort-error">{curlError}</p> : null}
        {curlInfo ? <div>
          <p className={curlInfo.hasToken ? "react-muted" : "react-effort-error"}>{curlInfo.note}</p>
          <code className="react-command">{curlInfo.method} {curlInfo.url}</code>
          {curlInfo.headers.length ? <details className="react-review-commits"><summary>识别到的请求头（凭证已脱敏，{curlInfo.headers.length}）</summary><pre>{curlInfo.headers.map(([n, v]) => `${n}: ${v}`).join("\n")}</pre></details> : null}
        </div> : null}
      </details>
      {detail.requestSchema ? <details className="react-review-commits"><summary>请求 schema（{detail.requestSchemaPath}）</summary><pre>{detail.requestSchema}</pre></details> : null}
      {detail.tests.length ? <details className="react-review-commits"><summary>出参断言（.bru tests，{detail.tests.length}）</summary><pre>{detail.tests.join("\n")}</pre></details> : null}
      <div className="react-actions">
        <button type="button" onClick={() => run(true)} disabled={triggering}><RefreshCw size={13} className={triggering ? "react-spin" : ""} />{triggering ? "构造中…" : "预览请求 (dry-run)"}</button>
        <button type="button" onClick={() => run(false)} disabled={triggering}><Send size={13} />{triggering ? "触发中…" : curlInfo ? `触发接口（真实环境 ${curlInfo.origin}）` : `触发接口（${ENV_TABS.find((t) => t.value === env)?.label}）`}</button>
      </div>
      <p className="react-muted">{curlInfo ? "⚠️ 真实环境凭证已启用：请求将写入该环境，线上修数请先 dry-run 确认域名与入参。" : "登录态自动获取（Chrome 登录态优先 → 缓存 → 账密刷新）；入参值仅本机记忆，不写回接口模板。"}</p>
      {triggerError ? <p className="react-effort-error">{triggerError}</p> : null}
      {result ? <div className="react-apitest-result">
        <PanelHead kicker="Response" title={result.dryRun ? "请求构造预览（dry-run）" : "触发结果"} chip={result.dryRun ? "未发送" : result.status != null ? `HTTP ${result.status} · ${result.durationMs}ms` : "未发送"} />
        {result.missing?.length ? <p className="react-effort-error">缺少入参：{result.missing.join("、")}</p> : null}
        {result.credentialSource === "curl" ? <p className="react-muted">🔗 curl 凭证模式：本次请求发往粘贴 curl 的真实环境域名。</p> : null}
        {result.url ? <code className="react-command">{result.credentialSource === "curl" ? "真实环境" : result.env} → {result.method ?? "POST"} {result.url}</code> : null}
        {result.dryRun && result.headers?.length ? <details className="react-review-commits" open><summary>请求头（凭证已脱敏）</summary><pre>{result.headers.map(([n, v]) => `${n}: ${v}`).join("\n")}</pre></details> : null}
        {result.dryRun && result.body ? <details className="react-review-commits" open><summary>请求体</summary><pre>{result.body}</pre></details> : null}
        {!result.dryRun && (result.bodyJson != null || result.bodyText) ? <pre className="react-apitest-json">{result.bodyJson != null ? JSON.stringify(result.bodyJson, null, 2) : result.bodyText}</pre> : null}
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
  return <PageChrome icon={<FlaskConical size={15} />} eyebrow="API Test" title="接口测试" description="WMS 已登记测试接口：造数/测试/线上修数据标记、关键字筛选，点击卡片展开入参/请求头/请求体编辑并直接触发（可切 test/UAT，临时修改不写回模板）；支持粘贴真实环境 curl 提取凭证发起线上修数请求。下方保留状态机造数脚本的配置与执行。">
    <ApiCatalogSection />
    <CapabilityScriptSection />
  </PageChrome>
}
