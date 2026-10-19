/**
 * Role: 接口详情页（路由 /testdata/api?id=<api-id>，「接口测试」列表的子页面）：
 * 展示已登记接口的占位符入参 / 模板请求头（可临时改值/停用）/ 请求体（可临时修改），
 * 切换 test/UAT 或粘贴真实环境 curl 凭证（线上修数场景）后直接触发；
 * 全部临时修改不写回模板。
 * Public surface: TestdataApiDetailPage used by web/src/App.tsx route /testdata/api.
 * Constraints: 触发走后端代理 /api/apitest/trigger（登录态由后端 login.mjs 自动获取，
 * token 不回显）；curl 原文仅存本机 sessionStorage（关标签即清，后端不落盘）；
 * 入参值 localStorage 记忆。
 * Read-this-with: web/src/pages/testdata.tsx（列表页，KIND_META/STATUS_META 共享）、
 * src/api_catalog.rs（curl 解析与触发契约）。
 */
import { ArrowLeft, Link2, RefreshCw, Send } from "lucide-react"
import { useEffect, useState } from "react"
import type {
  ApitestCurlImportPayload,
  ApitestDetailPayload,
  ApitestHeaderOverrideInput,
  ApitestTriggerPayload,
} from "../types"
import { fetchJson, postJson } from "../lib/api"
import { EmptyCard, ErrorCard, LoadingCard, PageChrome, PanelHead } from "../components/ui"
import { KIND_META, STATUS_META } from "./testdata"

const ENV_TABS = [
  { value: "test", label: "test 环境" },
  { value: "uat-cn", label: "UAT-CN 环境" },
  { value: "uat-sea", label: "UAT-SEA 环境" },
]

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

/** 粘贴 curl 原文的 sessionStorage key：跨「列表 → 详情」页面跳转保留（同标签页），关标签即清。 */
const CURL_MEMORY_KEY = "react-apitest-curl"

function readCurlMemory(): string {
  try { return sessionStorage.getItem(CURL_MEMORY_KEY) || "" } catch { return "" }
}

function writeCurlMemory(value: string) {
  try {
    if (value) sessionStorage.setItem(CURL_MEMORY_KEY, value)
    else sessionStorage.removeItem(CURL_MEMORY_KEY)
  } catch { /* ignore */ }
}

/** 接口详情 + 触发配置。 */
export function TestdataApiDetailPage() {
  const id = new URLSearchParams(window.location.search).get("id") || ""
  const [detail, setDetail] = useState<ApitestDetailPayload | null>(null)
  const [loading, setLoading] = useState(true)
  const [loadError, setLoadError] = useState<string | null>(null)
  const [env, setEnv] = useState("test")
  const [vars, setVars] = useState<Record<string, string>>({})
  const [bodyText, setBodyText] = useState("")
  /** 模板请求头临时编辑：头名 → 当前值 + 是否发送（不写回模板）。 */
  const [headerEdits, setHeaderEdits] = useState<Record<string, { value: string; enabled: boolean }>>({})
  /** 粘贴的真实环境 curl（sessionStorage 持久，跨接口复用；仅本机浏览器内存）。 */
  const [curlText, setCurlText] = useState(readCurlMemory)
  const [curlInfo, setCurlInfo] = useState<ApitestCurlImportPayload | null>(null)
  const [curlParsing, setCurlParsing] = useState(false)
  const [curlError, setCurlError] = useState<string | null>(null)
  const [triggering, setTriggering] = useState(false)
  const [result, setResult] = useState<ApitestTriggerPayload | null>(null)
  const [triggerError, setTriggerError] = useState<string | null>(null)

  useEffect(() => {
    if (!id) { setLoading(false); return }
    let cancelled = false
    setLoading(true)
    setLoadError(null)
    fetchJson<ApitestDetailPayload>(`/api/apitest/api?id=${encodeURIComponent(id)}`)
      .then((d) => {
        if (cancelled) return
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
      })
      .catch((e) => { if (!cancelled) setLoadError(e instanceof Error ? e.message : String(e)) })
      .finally(() => { if (!cancelled) setLoading(false) })
    return () => { cancelled = true }
  }, [id])

  const importCurl = async (text?: string) => {
    const source = text ?? curlText
    if (!source.trim() || curlParsing) return
    setCurlParsing(true)
    setCurlError(null)
    try {
      const info = await postJson<ApitestCurlImportPayload>("/api/apitest/curl-import", { curl: source })
      setCurlInfo(info)
      writeCurlMemory(source)
    } catch (e) {
      setCurlInfo(null)
      setCurlError(e instanceof Error ? e.message : String(e))
    } finally {
      setCurlParsing(false)
    }
  }

  // 从列表页带过来的 curl（sessionStorage 命中）自动重新解析，恢复凭证状态。
  useEffect(() => {
    if (curlText.trim()) void importCurl(curlText)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  const setVar = (name: string, value: string) => {
    setVars((prev) => ({ ...prev, [name]: value }))
    if (!detail?.placeholders.find((p) => p.name === name)?.derived) writeVarMemory(name, value)
  }

  const setHeaderEdit = (name: string, patch: Partial<{ value: string; enabled: boolean }>) => {
    setHeaderEdits((prev) => ({
      ...prev,
      [name]: { ...(prev[name] || { value: "", enabled: true }), ...patch },
    }))
  }

  const changeCurlText = (value: string) => {
    setCurlText(value)
    writeCurlMemory(value)
  }

  const clearCurl = () => {
    setCurlInfo(null)
    setCurlError(null)
    changeCurlText("")
  }

  const run = async (dryRun: boolean) => {
    if (triggering || !id) return
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
        id,
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

  const kindMeta = detail?.api ? KIND_META[detail.api.kind] || KIND_META.create : null

  return <PageChrome icon={<Link2 size={15} />} eyebrow="Test Data" title={detail?.api?.id || id || "接口详情"} description="查看入参占位符、模板请求头/请求体并触发；支持粘贴真实环境 curl 凭证发起线上修数请求（临时修改不写回模板）。" actions={<a href="/testdata"><ArrowLeft size={15} />返回接口目录</a>}>
    {loading ? <LoadingCard /> : loadError ? <ErrorCard error={loadError} /> : !detail?.api ? <EmptyCard>接口不存在或未登记：{id || "（缺少 id 参数）"}</EmptyCard> : <section className="react-panel">
      <PanelHead kicker="Trigger" title={`触发接口 · ${detail.api.id}`} chip={detail.api.method} />
      <div className="react-meta-grid">
        <span>用途 {detail.api.purpose}</span>
        <span>URL <code className="react-apitest-url">{detail.api.url}</code></span>
        <span>环境支持 {Object.entries(detail.api.envSupport || {}).map(([k, v]) => `${k}:${v}`).join(" · ") || "-"}</span>
        <span>模板状态 {STATUS_META[detail.api.status]?.label || detail.api.status}{kindMeta ? <> · <span className="react-status-pill" style={{ color: kindMeta.color, background: kindMeta.bg, borderColor: kindMeta.color }}>{kindMeta.label}</span></> : null}</span>
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
        <p className="react-muted">在已登录的真实环境页面打开 DevTools → Network → 任意请求右键 Copy as cURL，粘贴到这里解析：触发时改用该 curl 的域名与登录态发起请求（跳过 login.mjs，不携带 x-env/Cookie）。凭证在本接口间复用（仅本机浏览器 sessionStorage，关标签即清），后端不落盘、不回显完整凭证。</p>
        <textarea className="react-apitest-body react-curl-textarea" rows={5} value={curlText} onChange={(e) => changeCurlText(e.target.value)} placeholder="curl 'https://…/wms-web/…' -H 'X-Token: …' -H 'x-params: …' --data-raw '…'" spellCheck={false} />
        <div className="react-actions">
          <button type="button" onClick={() => importCurl()} disabled={curlParsing || !curlText.trim()}><Link2 size={13} className={curlParsing ? "react-spin" : ""} />{curlParsing ? "解析中…" : "解析 curl 凭证"}</button>
          {curlInfo || curlText ? <button type="button" className="react-ghost-btn" onClick={clearCurl}>清除凭证</button> : null}
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
    </section>}
  </PageChrome>
}
