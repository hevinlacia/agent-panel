/**
 * Role: render an agent-written mermaid diagram (data/state flow) inside the
 * diff annotation inspector. Mermaid is heavy, so it is dynamically imported
 * on first use and cached; on syntax errors the raw source degrades to a
 * <pre> block instead of breaking the panel.
 */
import { useEffect, useId, useState } from "react"

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
      })
      return mermaid
    })
  }
  return mermaidReady
}

export function MermaidFlow({ code }: { code: string }) {
  const [svg, setSvg] = useState<string | null>(null)
  const [failed, setFailed] = useState(false)
  const reactId = useId()
  const renderId = `mermaid-${reactId.replace(/[^a-zA-Z0-9]/g, "")}`
  useEffect(() => {
    let cancelled = false
    setSvg(null)
    setFailed(false)
    getMermaid()
      .then((mermaid) => mermaid.render(renderId, code))
      .then(({ svg: out }) => { if (!cancelled) setSvg(out) })
      .catch(() => { if (!cancelled) setFailed(true) })
    return () => { cancelled = true }
  }, [code, renderId])
  if (failed) return <pre className="react-annotation-flow-fallback">{code}</pre>
  if (!svg) return <p className="react-muted react-annotation-flow-loading">渲染流转图…</p>
  // mermaid.render output under securityLevel:"strict" is sanitized; only the
  // generated SVG is injected, never raw agent text as HTML.
  return <div className="react-annotation-flow" dangerouslySetInnerHTML={{ __html: svg }} />
}
