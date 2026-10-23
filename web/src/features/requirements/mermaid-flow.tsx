/**
 * Role: render an agent-written mermaid diagram (data/state flow) inside the
 * diff annotation inspector. Mermaid is heavy, so it is dynamically imported
 * on first use and cached; on syntax errors the raw source degrades to a
 * <pre> block instead of breaking the panel.
 *
 * The inline diagram is narrow (annotation sidebar), so it is clickable and
 * opens a fullscreen lightbox that reuses the already-rendered SVG with
 * cursor-centered wheel zoom, drag pan, fit-to-window reset, and Esc/close.
 */
import { useCallback, useEffect, useId, useRef, useState } from "react"
import { createPortal } from "react-dom"
import { Maximize2, Minus, Plus, Search, X } from "lucide-react"

type MermaidModule = typeof import("mermaid")["default"]

let mermaidReady: Promise<MermaidModule> | null = null

function getMermaid(): Promise<MermaidModule> {
  if (!mermaidReady) {
    mermaidReady = import("mermaid").then((mod) => {
      const mermaid = mod.default
      mermaid.initialize({
        startOnLoad: false,
        securityLevel: "strict",
        theme: "dark",
        fontFamily: "var(--react-mono, monospace)",
        // parse 失败时 mermaid 会往 document.body 注入全局错误卡片（「Syntax error in text」
        // 炸弹图标），降级 <pre> 挡不住它——关掉，错误只在组件内降级展示。
        suppressErrorRendering: true,
      })
      return mermaid
    })
  }
  return mermaidReady
}

/** mermaid 自有语法的开头关键词；不命中视为纯箭头链文本走 normalize。 */
const MERMAID_SOURCE_RE =
  /^\s*(graph|flowchart|sequenceDiagram|classDiagram|stateDiagram(-v2)?|erDiagram|mindmap|gantt|pie|gitGraph|journey|timeline|sankey|quadrantChart)\b/i

/**
 * flow 字段实际以纯箭头链文本编写（「A → B → C」，全部存量 annotations 如此），
 * 不是 mermaid 语法——直接喂给 render 必然 syntax error。这里自动转成 flowchart LR：
 * 每行一条链，同标签节点合并，标签加引号并转义特殊字符（括号/等号/逗号是 mermaid 破坏者）。
 */
function normalizeFlowSource(code: string): string {
  const trimmed = code.trim()
  if (!trimmed || MERMAID_SOURCE_RE.test(trimmed)) return trimmed
  const lines = trimmed
    .split(/\r?\n/)
    .map((l) => l.trim())
    .filter(Boolean)
  if (!lines.length) return trimmed
  const esc = (s: string) => s.replace(/#/g, "#35;").replace(/"/g, "#quot;")
  const ids = new Map<string, string>()
  const nodeId = (label: string) => {
    const key = label.trim()
    if (!ids.has(key)) ids.set(key, `n${ids.size}`)
    return ids.get(key)!
  }
  const stmts: string[] = []
  for (const line of lines) {
    const steps = line
      .split(/\s*(?:→|->|⇒|=>)\s*/)
      .map((s) => s.trim())
      .filter(Boolean)
    if (!steps.length) continue
    if (steps.length === 1) {
      stmts.push(`${nodeId(steps[0])}["${esc(steps[0])}"]`)
      continue
    }
    for (let i = 0; i < steps.length - 1; i++) {
      stmts.push(
        `${nodeId(steps[i])}["${esc(steps[i])}"] --> ${nodeId(steps[i + 1])}["${esc(steps[i + 1])}"]`,
      )
    }
  }
  if (!stmts.length) return trimmed
  return "flowchart LR\n" + stmts.map((s) => "  " + s).join("\n")
}

const MIN_SCALE = 0.1
const MAX_SCALE = 8
const WHEEL_STEP = 1.2
const BUTTON_STEP = 1.25

function clampScale(k: number) {
  return Math.min(MAX_SCALE, Math.max(MIN_SCALE, k))
}

interface View {
  x: number
  y: number
  k: number
}

/** Zoom around an anchor point (surface coords) keeping that point fixed on screen. */
function zoomView(cur: View, cx: number, cy: number, k: number): View {
  const next = clampScale(k)
  return {
    k: next,
    x: cx - ((cx - cur.x) * next) / cur.k,
    y: cy - ((cy - cur.y) * next) / cur.k,
  }
}

function FlowLightbox({ svg, title, onClose }: { svg: string; title: string; onClose: () => void }) {
  const surfaceRef = useRef<HTMLDivElement | null>(null)
  const contentRef = useRef<HTMLDivElement | null>(null)
  const naturalRef = useRef({ w: 0, h: 0 })
  const [view, setView] = useState<View>({ x: 0, y: 0, k: 1 })
  const viewRef = useRef(view)
  viewRef.current = view
  const dragRef = useRef<{ px: number; py: number; x: number; y: number } | null>(null)
  const [dragging, setDragging] = useState(false)

  /** Fit the whole diagram into the surface, centered; allows upscaling (SVG is lossless). */
  const applyFit = useCallback(() => {
    const surface = surfaceRef.current
    const { w, h } = naturalRef.current
    if (!surface || !w || !h) return
    const rect = surface.getBoundingClientRect()
    const pad = 40
    const k = clampScale(Math.min((rect.width - pad) / w, (rect.height - pad) / h))
    setView({ x: (rect.width - w * k) / 2, y: (rect.height - h * k) / 2, k })
  }, [])

  // Measure the natural diagram size once (unlock mermaid's max-width), then fit
  // and keep the fit on window resize.
  useEffect(() => {
    const el = contentRef.current?.querySelector("svg")
    if (el) {
      const inlineMax = parseFloat(el.style.maxWidth)
      const vb = el.viewBox.baseVal
      let w = Number.isFinite(inlineMax) && inlineMax > 0 ? inlineMax : 0
      if (!w && vb && vb.width > 0) w = vb.width
      if (w > 0) el.style.width = `${w}px`
      el.style.maxWidth = "none"
      const rect = el.getBoundingClientRect()
      naturalRef.current = { w: rect.width || w, h: rect.height }
    }
    applyFit()
    window.addEventListener("resize", applyFit)
    return () => window.removeEventListener("resize", applyFit)
  }, [applyFit])

  // Esc closes; wheel zooms around the cursor (non-passive listener so
  // preventDefault actually stops page scroll behind the overlay).
  useEffect(() => {
    const surface = surfaceRef.current
    if (!surface) return
    const onKey = (e: KeyboardEvent) => { if (e.key === "Escape") onClose() }
    const onWheel = (e: WheelEvent) => {
      e.preventDefault()
      const rect = surface.getBoundingClientRect()
      const cur = viewRef.current
      const factor = e.deltaY < 0 ? WHEEL_STEP : 1 / WHEEL_STEP
      setView(zoomView(cur, e.clientX - rect.left, e.clientY - rect.top, cur.k * factor))
    }
    window.addEventListener("keydown", onKey)
    surface.addEventListener("wheel", onWheel, { passive: false })
    return () => {
      window.removeEventListener("keydown", onKey)
      surface.removeEventListener("wheel", onWheel)
    }
  }, [onClose])

  // Lock body scroll behind the fullscreen overlay.
  useEffect(() => {
    const prev = document.body.style.overflow
    document.body.style.overflow = "hidden"
    return () => { document.body.style.overflow = prev }
  }, [])

  const onPointerDown = (e: React.PointerEvent<HTMLDivElement>) => {
    if (e.button !== 0) return
    dragRef.current = { px: e.clientX, py: e.clientY, x: viewRef.current.x, y: viewRef.current.y }
    setDragging(true)
    e.currentTarget.setPointerCapture(e.pointerId)
  }
  const onPointerMove = (e: React.PointerEvent<HTMLDivElement>) => {
    const d = dragRef.current
    if (!d) return
    setView((v) => ({ ...v, x: d.x + (e.clientX - d.px), y: d.y + (e.clientY - d.py) }))
  }
  const endDrag = (e: React.PointerEvent<HTMLDivElement>) => {
    dragRef.current = null
    setDragging(false)
    if (e.currentTarget.hasPointerCapture?.(e.pointerId)) e.currentTarget.releasePointerCapture(e.pointerId)
  }

  /** Toolbar zoom around the surface center. */
  const zoomBy = (factor: number) => {
    const rect = surfaceRef.current?.getBoundingClientRect()
    const cur = viewRef.current
    setView(zoomView(cur, rect ? rect.width / 2 : 0, rect ? rect.height / 2 : 0, cur.k * factor))
  }
  /** Double-click toggles between fit and 100% (around surface center). */
  const onDoubleClick = () => {
    if (Math.abs(viewRef.current.k - 1) < 0.05) { applyFit(); return }
    const rect = surfaceRef.current?.getBoundingClientRect()
    setView(zoomView(viewRef.current, rect ? rect.width / 2 : 0, rect ? rect.height / 2 : 0, 1))
  }

  return createPortal(
    <div className="react-flow-lightbox" onClick={(e) => { if (e.target === e.currentTarget) onClose() }}>
      <div className="react-flow-lightbox-bar">
        <span className="react-flow-lightbox-title">{title}</span>
        <span className="react-flow-lightbox-scale">{Math.round(view.k * 100)}%</span>
        <button type="button" onClick={() => zoomBy(1 / BUTTON_STEP)} title="缩小"><Minus size={14} /></button>
        <button type="button" onClick={() => zoomBy(BUTTON_STEP)} title="放大"><Plus size={14} /></button>
        <button type="button" onClick={applyFit} title="适应窗口"><Maximize2 size={13} /></button>
        <button type="button" onClick={onClose} title="关闭 (Esc)"><X size={14} /></button>
      </div>
      <div
        ref={surfaceRef}
        className={`react-flow-lightbox-surface${dragging ? " dragging" : ""}`}
        onPointerDown={onPointerDown}
        onPointerMove={onPointerMove}
        onPointerUp={endDrag}
        onPointerCancel={endDrag}
        onDoubleClick={onDoubleClick}
      >
        {/* mermaid.render output under securityLevel:"strict" is sanitized; the
            same sanitized string as the inline copy, never raw agent text. */}
        <div
          ref={contentRef}
          className="react-flow-lightbox-content"
          style={{ transform: `translate(${view.x}px, ${view.y}px) scale(${view.k})` }}
          dangerouslySetInnerHTML={{ __html: svg }}
        />
      </div>
    </div>,
    document.body,
  )
}

export function MermaidFlow({ code }: { code: string }) {
  const [svg, setSvg] = useState<string | null>(null)
  const [failed, setFailed] = useState(false)
  const [zoomed, setZoomed] = useState(false)
  const reactId = useId()
  const renderId = `mermaid-${reactId.replace(/[^a-zA-Z0-9]/g, "")}`
  useEffect(() => {
    let cancelled = false
    const source = normalizeFlowSource(code)
    setSvg(null)
    setFailed(false)
    setZoomed(false)
    getMermaid()
      .then((mermaid) => mermaid.render(renderId, source))
      .then(({ svg: out }) => { if (!cancelled) setSvg(out) })
      .catch(() => { if (!cancelled) setFailed(true) })
    return () => { cancelled = true }
  }, [code, renderId])
  if (failed) return <pre className="react-annotation-flow-fallback">{code}</pre>
  if (!svg) return <p className="react-muted react-annotation-flow-loading">渲染流转图…</p>
  // mermaid.render output under securityLevel:"strict" is sanitized; only the
  // generated SVG is injected, never raw agent text as HTML.
  return <>
    <div
      className="react-annotation-flow react-annotation-flow-click"
      role="button"
      aria-label="点击放大流转图"
      title="点击查看大图（滚轮缩放 · 拖拽平移）"
      onClick={() => setZoomed(true)}
    >
      <div dangerouslySetInnerHTML={{ __html: svg }} />
      <span className="react-annotation-flow-hint"><Search size={10} />点击放大</span>
    </div>
    {zoomed ? <FlowLightbox svg={svg} title="数据 / 状态流转（滚轮缩放 · 拖拽平移 · 双击适应窗口 · Esc 关闭）" onClose={() => setZoomed(false)} /> : null}
  </>
}
