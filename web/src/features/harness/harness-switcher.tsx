import { useEffect, useState, useRef } from "react"
import { Bot, ChevronDown, Sparkles } from "lucide-react"
import { postJson, useFetch } from "../../lib/api"

type HarnessId = "pi" | "dsh"

interface HarnessModelRow {
  id: string
  providerId: string
  modelId: string
  label: string
  contextWindow?: number | null
  reasoning?: boolean
}

interface HarnessPayload {
  harness: HarnessId
  label: string
  kind: string
  defaultProvider: string
  defaultModel: string
  defaultThinkingLevel?: string
  providers: { id: string; label: string; models: HarnessModelRow[] }[]
  models: HarnessModelRow[]
  modelCount: number
  settingsPath?: string
}

interface HarnessList {
  harnesses: HarnessPayload[]
  defaultHarness: HarnessId
}
interface HarnessCurrent {
  harness: HarnessId
  label: string
  payload: HarnessPayload
}

export function HarnessSwitcher() {
  const list = useFetch<HarnessList>("/api/harness/list")
  const current = useFetch<HarnessCurrent>("/api/harness/current")
  const [open, setOpen] = useState(false)
  const [busy, setBusy] = useState<string | null>(null)
  const ref = useRef<HTMLDivElement>(null)

  useEffect(() => {
    const onDocClick = (e: MouseEvent) => {
      if (!ref.current?.contains(e.target as Node)) setOpen(false)
    }
    if (open) document.addEventListener("mousedown", onDocClick)
    return () => document.removeEventListener("mousedown", onDocClick)
  }, [open])

  const harnesses = list.data?.harnesses ?? []
  const cur = current.data?.payload ?? null
  const curHarness = (current.data?.harness as HarnessId) ?? "pi"

  const switchHarness = async (h: HarnessId) => {
    setBusy(h)
    try {
      await postJson("/api/harness/switch", { harness: h })
      list.refresh()
      current.refresh()
      setOpen(false)
    } finally {
      setBusy(null)
    }
  }

  if (list.loading && !list.data) {
    return <span className="react-harness-pill">加载中…</span>
  }

  return (
    <div ref={ref} className="react-harness-switcher">
      <button
        type="button"
        className={`react-harness-trigger ${open ? "open" : ""}`}
        onClick={() => setOpen((v) => !v)}
        aria-haspopup="menu"
        aria-expanded={open}
        title={cur?.settingsPath || ""}
      >
        <span className={`react-harness-badge ${curHarness}`}>
          {curHarness === "dsh" ? <Sparkles size={13} /> : <Bot size={13} />}
          {curHarness === "dsh" ? "DSH" : "Pi"}
        </span>
        <ChevronDown size={14} className={open ? "rot" : ""} />
      </button>

      {open && (
        <div className="react-harness-menu" role="menu">
          <div className="react-harness-menu-head">
            <span>对接 Agent</span>
            <em>{curHarness === "dsh" ? "DSH" : "Pi"}</em>
          </div>

          {harnesses.map((h) => {
            const isActive = h.harness === curHarness
            return (
              <div key={h.harness} className={`react-harness-group ${isActive ? "active" : ""}`}>
                <button
                  type="button"
                  className="react-harness-group-head"
                  onClick={() => switchHarness(h.harness as HarnessId)}
                  disabled={!!busy}
                >
                  <span className="react-harness-group-label">
                    {h.harness === "dsh" ? <Sparkles size={14} /> : <Bot size={14} />}
                    {h.label}
                  </span>
                  {isActive ? (
                    <span className="react-harness-current">当前</span>
                  ) : busy === h.harness ? (
                    <span>切换中…</span>
                  ) : (
                    <span className="react-harness-switch-hint">切换</span>
                  )}
                </button>
              </div>
            )
          })}

          <div className="react-harness-foot">
            <a href="/settings">去 Settings 完整配置</a>
            <button type="button" onClick={() => { list.refresh(); current.refresh() }}>刷新</button>
          </div>
        </div>
      )}
    </div>
  )
}
