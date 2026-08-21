import { CheckCircle2, KeyRound, RefreshCw, ShieldCheck, XCircle } from "lucide-react"
import { useEffect, useMemo, useState } from "react"
import type { BrowserAuthCheckPayload, BrowserAuthRequestResult, BrowserAuthSitesPayload } from "../types"
import { fetchJson, postJson, useFetch } from "../lib/api"
import { ErrorCard, EmptyCard, LoadingCard, PageChrome, PanelHead } from "../components/ui"

function statusPill(ok?: boolean, label?: string) {
  if (ok === true) return <span className="react-status-pill" style={{ color: "#86efac", background: "rgba(34,197,94,.12)", borderColor: "rgba(34,197,94,.38)" }}>{label || "已登录"}</span>
  if (ok === false) return <span className="react-status-pill" style={{ color: "#fca5a5", background: "rgba(239,68,68,.12)", borderColor: "rgba(239,68,68,.38)" }}>{label || "未登录"}</span>
  return <span className="react-status-pill" style={{ color: "#fcd34d", background: "rgba(245,158,11,.12)", borderColor: "rgba(245,158,11,.38)" }}>{label || "未知"}</span>
}

function SiteCheckResult({ result }: { result: BrowserAuthCheckPayload | null }) {
  if (!result) return null
  const login = result.login
  const ok = login.ok
  return (
    <div className="react-auth-check">
      <div className="react-review-summary">
        {statusPill(ok, ok ? "登录检查通过" : "登录检查失败")}
        {login.skipped ? <span className="react-review-tag">未配置 loginCheck</span> : null}
        {!login.skipped && typeof login.status === "number" ? <span className="react-review-tag">HTTP {login.status} / 期望 {login.expected}</span> : null}
        <span className="react-review-tag">cookies {result.status?.cookieCount ?? 0}</span>
        <span className="react-review-tag">domains {(result.status?.matchedDomains || []).join(" / ") || "-"}</span>
      </div>
      {login.error ? <p className="react-error">{login.error}</p> : null}
      {login.bodyPreview ? <pre className="react-diff-preview react-auth-preview">{login.bodyPreview}</pre> : null}
    </div>
  )
}

export function AuthSitesPage() {
  const sites = useFetch<BrowserAuthSitesPayload>("/api/auth-sites")
  const [draft, setDraft] = useState("")
  const [savedHint, setSavedHint] = useState<string | null>(null)
  const [checks, setChecks] = useState<Record<string, BrowserAuthCheckPayload | null>>({})
  const [checking, setChecking] = useState<Record<string, boolean>>({})
  const [reqSite, setReqSite] = useState("")
  const [reqMethod, setReqMethod] = useState("GET")
  const [reqPath, setReqPath] = useState("")
  const [reqBody, setReqBody] = useState("")
  const [reqResult, setReqResult] = useState<BrowserAuthRequestResult | null>(null)
  const [reqError, setReqError] = useState<string | null>(null)
  const [reqBusy, setReqBusy] = useState(false)
  const [newDomain, setNewDomain] = useState("")

  useEffect(() => {
    const data = sites.data
    if (data) {
      setDraft(JSON.stringify({ cdpUrl: data.config.cdpUrl || "", sites: data.config.sites || [] }, null, 2))
      setReqSite((cur) => cur || (data.sites[0]?.id ?? ""))
    }
  }, [sites.data])

  const byId = useMemo(() => {
    const map: Record<string, NonNullable<BrowserAuthSitesPayload["sites"]>[number]> = {}
    for (const s of sites.data?.sites || []) map[s.id] = s
    return map
  }, [sites.data])

  const saveConfig = async () => {
    try {
      const parsed = JSON.parse(draft)
      await postJson("/api/config", { browserAuth: parsed })
      sites.refresh()
      setSavedHint("Auth 配置已保存")
    } catch (err) {
      setSavedHint(`保存失败：${(err as Error).message}`)
    }
  }

  const saveAllowlist = async (domains: string[]) => {
    try {
      const parsed = JSON.parse(draft)
      parsed.cookieAllowlist = [...new Set(domains.map((d) => d.trim().toLowerCase().replace(/^\./, "")).filter(Boolean))]
      setDraft(JSON.stringify(parsed, null, 2))
      await postJson("/api/config", { browserAuth: parsed })
      sites.refresh()
      setSavedHint("Cookie 白名单已保存")
    } catch (err) {
      setSavedHint(`保存失败：${(err as Error).message}`)
    }
  }

  const addDomain = () => {
    const d = newDomain.trim().toLowerCase().replace(/^\./, "")
    if (!d) return
    const current = sites.data?.security?.cookieAllowlist || []
    saveAllowlist([...current, d])
    setNewDomain("")
  }

  const removeDomain = (d: string) => {
    const current = sites.data?.security?.cookieAllowlist || []
    saveAllowlist(current.filter((x) => x !== d))
  }

  const runCheck = async (siteId: string) => {
    setChecking((c) => ({ ...c, [siteId]: true }))
    try {
      const result = await postJson<BrowserAuthCheckPayload>(`/api/auth-sites/${encodeURIComponent(siteId)}/check`, {})
      setChecks((c) => ({ ...c, [siteId]: result }))
    } catch (err) {
      setChecks((c) => ({ ...c, [siteId]: { ok: false, generatedAt: 0, site: siteId, status: {}, login: { ok: false, error: (err as Error).message } } }))
    } finally {
      setChecking((c) => ({ ...c, [siteId]: false }))
    }
  }

  const runRequest = async () => {
    if (!reqSite || !reqPath) return
    setReqBusy(true)
    setReqError(null)
    setReqResult(null)
    let jsonBody: unknown
    let body: string | undefined
    if (reqBody.trim()) {
      try {
        jsonBody = JSON.parse(reqBody)
      } catch {
        body = reqBody
      }
    }
    try {
      const result = await postJson<BrowserAuthRequestResult>(`/api/auth-sites/${encodeURIComponent(reqSite)}/request`, {
        method: reqMethod,
        path: reqPath,
        ...(jsonBody !== undefined ? { json: jsonBody } : body !== undefined ? { body } : {}),
      })
      setReqResult(result)
    } catch (err) {
      setReqError((err as Error).message)
    } finally {
      setReqBusy(false)
    }
  }

  const previewBody = reqResult?.bodyText
    ? reqResult.bodyJson !== undefined && reqResult.bodyJson !== null
      ? JSON.stringify(reqResult.bodyJson, null, 2)
      : reqResult.bodyText
    : ""

  return (
    <PageChrome
      icon={<KeyRound size={15} />}
      eyebrow="Browser Auth"
      title="Chrome 登录态复用"
      description="只读复用本机 Chrome 的登录 cookie，按站点白名单代发接口请求；不返回 cookie/token，不写回浏览器。"
      actions={<button onClick={sites.refresh}><RefreshCw size={15} />刷新</button>}
    >
      <section className="react-panel">
        <PanelHead kicker="CDP" title="Chrome 连接" chip={sites.data?.cdp?.connected ? "connected" : "disconnected"} />
        {sites.data ? (
          <div className="react-meta-grid">
            <span>DevTools</span><span>{sites.data.cdp.connected ? "已连接" : "未连接"}</span>
            <span>读取方式</span><span>{sites.data.cdp.source === "db" ? "Cookie 数据库直读（无弹窗）" : sites.data.cdp.source === "cdp" ? "CDP（可能弹确认）" : "-"}</span>
            <span>说明</span><span>{sites.data.cdp.message}</span>
            <span>Secrets 返回</span><span>{sites.data.security.returnsSecrets ? "是" : "否"}</span>
            <span>Token 持久化</span><span>{sites.data.security.tokenPersistence}</span>
            <span>审计日志</span><span><code>{sites.data.security.auditFile}</code></span>
          </div>
        ) : sites.error ? <ErrorCard error={sites.error} /> : <LoadingCard />}
      </section>

      <section className="react-panel react-auth-allowlist">
        <PanelHead kicker="Permission" title="权限控制 · Cookie 白名单" chip={sites.data?.security?.allowlistEnforced ? "白名单已启用" : "-"} />
        <p className="react-muted">白名单机制：<strong>只有列出的域名</strong>才会被从 Chrome 自动获取并解密登录态（cookie），其余域名一律不读取、不解密、不进内存。未列出的站点即使配置了也拿不到登录态。空白名单 = 不获取任何站点登录态（严格安全默认）。</p>
        <div className="react-meta-grid">
          <span>白名单启用</span><span>{sites.data?.security?.allowlistEnforced ? "是（硬过滤）" : "-"}</span>
          <span>当前持有 cookie</span><span>{sites.data?.security?.heldCookieCount ?? 0} 条（仅白名单域名内）</span>
          <span>生效白名单</span><span>{sites.data?.security?.effectiveAllowlist?.length || 0} 个域名</span>
        </div>
        <div className="react-auth-allowlist-box">
          {(sites.data?.security?.effectiveAllowlist || []).length === 0 ? (
            <p className="react-muted">尚未配置白名单，当前不会自动获取任何登录态。</p>
          ) : (
            <div className="react-chip-list">
              {(sites.data?.security?.effectiveAllowlist || []).map((d) => (
                <span key={d} className="react-auth-domain">{d}<button type="button" onClick={() => removeDomain(d)} title={`从白名单移除 ${d}`}>×</button></span>
              ))}
            </div>
          )}
        </div>
        <div className="react-inline-form">
          <input value={newDomain} onChange={(e) => setNewDomain(e.target.value)} placeholder="例如 kibana.example.com（含子域）" onKeyDown={(e) => { if (e.key === "Enter") addDomain() }} />
          <button onClick={addDomain} disabled={!newDomain.trim()}>添加域名</button>
        </div>
        <p className="react-muted">提示：显式配置白名单后，站点请求只会使用白名单内 cookie；未配置时自动取所有启用站点 <code>cookieDomains</code> 的并集。</p>
      </section>

      <section className="react-panel">
        <PanelHead kicker="Sites" title="站点登录态" chip={`${sites.data?.sites?.length ?? 0} sites`} />
        {sites.error ? <ErrorCard error={sites.error} /> : sites.loading ? <LoadingCard /> : (sites.data?.sites?.length || 0) === 0 ? (
          <EmptyCard>尚未配置 auth site。在下方 JSON 配置里添加站点后保存。</EmptyCard>
        ) : (
          <div className="react-card-list">
            {sites.data!.sites.map((site) => (
              <div key={site.id} className="react-list-card react-auth-card">
                <div>
                  <span className="react-card-id">{site.id}</span>
                  <h3>{site.label} {site.enabled ? null : <em className="react-muted">(disabled)</em>}</h3>
                  <p><code>{site.baseUrl}</code></p>
                  <div className="react-card-meta">
                    {statusPill(site.status?.ok)}
                    <span>cookies {site.status?.cookieCount ?? 0}</span>
                    <span>hosts {(site.allowedHosts || []).join(" / ") || "-"}</span>
                    <span>paths {(site.allowedPathPrefixes || []).join(" / ") || "-"}</span>
                    <span>loginCheck {site.loginCheck ? `${site.loginCheck.method || "GET"} ${site.loginCheck.path || ""} (${site.loginCheck.expect ?? 200})` : "未配置"}</span>
                  </div>
                  <SiteCheckResult result={checks[site.id] || null} />
                </div>
                <div className="react-card-side">
                  <button onClick={() => runCheck(site.id)} disabled={checking[site.id]}>{checking[site.id] ? "检查中…" : "检查登录"}</button>
                </div>
              </div>
            ))}
          </div>
        )}
      </section>

      <section className="react-panel">
        <PanelHead kicker="Request" title="白名单接口请求" chip="不返回 secrets" />
        <p className="react-muted">仅允许站点 <code>allowedPathPrefixes</code> 内的路径；由后端从 Chrome 注入 cookie 后代发，响应只返回接口结果。</p>
        <div className="react-filter-grid">
          <label>站点
            <select value={reqSite} onChange={(e) => setReqSite(e.target.value)}>
              {(sites.data?.sites || []).filter((s) => s.enabled).map((s) => <option key={s.id} value={s.id}>{s.id}</option>)}
              {!byId[reqSite]?.enabled && reqSite ? <option value={reqSite}>{reqSite} (disabled)</option> : null}
            </select>
          </label>
          <label>方法
            <select value={reqMethod} onChange={(e) => setReqMethod(e.target.value)}>
              {["GET", "POST", "PUT", "PATCH", "DELETE"].map((m) => <option key={m} value={m}>{m}</option>)}
            </select>
          </label>
          <label className="react-filter-grow">路径
            <input value={reqPath} onChange={(e) => setReqPath(e.target.value)} placeholder="/api/status" />
          </label>
        </div>
        <label className="react-editor-label">JSON / Body（可选）
          <textarea className="react-auth-body" rows={5} value={reqBody} onChange={(e) => setReqBody(e.target.value)} placeholder='{"query": {"term": "..."}}' />
        </label>
        <div className="react-actions">
          <button onClick={runRequest} disabled={reqBusy || !reqSite || !reqPath}>{reqBusy ? "请求中…" : "发送请求"}</button>
          {reqResult ? <span className="react-save-hint">HTTP {reqResult.status} · {reqResult.truncated ? "已截断" : "完整"}</span> : null}
        </div>
        {reqError ? <ErrorCard error={reqError} /> : null}
        {reqResult ? <pre className="react-diff-preview react-auth-preview">{previewBody || "(empty body)"}</pre> : null}
      </section>

      <section className="react-panel react-config-editor">
        <PanelHead kicker="Config" title="Auth 站点配置" chip={savedHint || "config.json"} />
        <p className="react-muted">编辑 <code>cdpUrl</code>（可选，默认自动发现 Chrome DevTools）和 <code>sites</code> 列表。保存写入 <code>~/.local/share/agent-panel/config.json</code>。</p>
        <textarea className="react-code-textarea" value={draft} onChange={(e) => { setDraft(e.target.value); setSavedHint(null) }} spellCheck={false} />
        <div className="react-actions">
          <button onClick={saveConfig}>保存 Auth 配置</button>
          <span className="react-save-hint"><ShieldCheck size={14} /> 配置里只写站点元信息，不写 cookie/token</span>
        </div>
      </section>
    </PageChrome>
  )
}
