// public/terminal.js
// Page-scoped: only runs on /session. Loads xterm + fit addon from /vendor/*,
// connects to /ws/session-terminal?id=<id>, and bridges stdin/stdout/resize.

(function () {
  "use strict"

  const host = document.getElementById("terminal")
  if (!host) return
  const statusEl = document.getElementById("terminal-status")
  const setStatus = (msg) => { if (statusEl) statusEl.textContent = msg }

  const params = new URLSearchParams(window.location.search)
  // Fall back to data-session-id in case a future route strips the query.
  const id = params.get("id") || host.dataset.sessionId || ""
  if (!id) {
    setStatus("error: missing session id")
    return
  }

  // Style helpers -----------------------------------------------------------
  const THEME = {
    background: "#0a0d12",
    foreground: "#d4dae3",
    cursor: "#22d3ee",
    cursorAccent: "#0a0d12",
    selectionBackground: "#264f78",
    black: "#0a0d12",
    red: "#ef4444",
    green: "#22c55e",
    yellow: "#f59e0b",
    blue: "#3b82f6",
    magenta: "#a855f7",
    cyan: "#22d3ee",
    white: "#d4dae3",
    brightBlack: "#475569",
    brightRed: "#fca5a5",
    brightGreen: "#86efac",
    brightYellow: "#fcd34d",
    brightBlue: "#93c5fd",
    brightMagenta: "#d8b4fe",
    brightCyan: "#67e8f9",
    brightWhite: "#f1f5f9",
  }

  // xterm.js: load the UMD bundle which exposes a global `Terminal`.
  // We intentionally use the prebuilt xterm.js (UMD) to keep things dependency-free.
  function loadScript(src) {
    return new Promise((resolve, reject) => {
      const s = document.createElement("script")
      s.src = src
      s.async = false
      s.onload = () => resolve(true)
      s.onerror = () => reject(new Error("failed to load " + src))
      document.head.appendChild(s)
    })
  }

  async function bootstrap() {
    setStatus("loading xterm…")
    await loadScript("/vendor/xterm/xterm.js")
    await loadScript("/vendor/xterm-addon-fit/addon-fit.js")
    await loadCss("/vendor/xterm/xterm.css")

    if (typeof window.Terminal !== "function") {
      setStatus("error: xterm failed to load")
      return
    }
    if (typeof window.FitAddon === "undefined" || !window.FitAddon.FitAddon) {
      setStatus("error: xterm fit addon failed to load")
      return
    }
    const FitAddon = window.FitAddon.FitAddon

    const term = new window.Terminal({
      cursorBlink: true,
      fontFamily: '"Noto Sans Mono CJK SC", "JetBrains Mono", "Fira Code", "SFMono-Regular", Consolas, monospace',
      fontSize: 13,
      lineHeight: 1,
      letterSpacing: 0,
      scrollback: 5000,
      convertEol: false,
      allowProposedApi: true,
      theme: THEME,
    })
    const fit = new FitAddon()
    term.loadAddon(fit)
    term.open(host)
    queueMicrotask(() => {
      try { fit.fit() } catch { /* noop */ }
    })

    setStatus("connecting…")
    const proto = window.location.protocol === "https:" ? "wss" : "ws"
    const ws = new WebSocket(`${proto}://${window.location.host}/ws/session-terminal?id=${encodeURIComponent(id)}`)
    ws.binaryType = "arraybuffer"

    let ready = false
    let cols = 0
    let rows = 0
    let pendingResize = null

    function sendResize() {
      if (!ready || !ws.readyState) return
      if (cols > 0 && rows > 0) {
        ws.send(JSON.stringify({ type: "resize", cols, rows }))
      }
    }

    function pushResize() {
      try {
        fit.fit()
      } catch { /* noop */ }
      const c = term.cols
      const r = term.rows
      if (c === cols && r === rows) return
      cols = c
      rows = r
      if (ready) sendResize()
      else pendingResize = { cols: c, rows: r }
    }

    ws.addEventListener("open", () => {
      setStatus("connected, awaiting shell…")
    })

    ws.addEventListener("message", (evt) => {
      const raw = typeof evt.data === "string" ? evt.data : ""
      if (!raw) return
      // Control frames are JSON objects that begin with `{` and have a `type` field.
      if (raw.charCodeAt(0) === 0x7b /* "{" */) {
        let parsed
        try { parsed = JSON.parse(raw) } catch { term.write(raw); return }
        if (parsed && typeof parsed === "object") {
          if (parsed.type === "ready") {
            ready = true
            if (typeof parsed.cols === "number") cols = parsed.cols
            if (typeof parsed.rows === "number") rows = parsed.rows
            setStatus("connected: opencode --session " + (parsed.id || id))
            if (pendingResize) {
              ws.send(JSON.stringify({ type: "resize", cols: pendingResize.cols, rows: pendingResize.rows }))
              pendingResize = null
            } else {
              sendResize()
            }
            term.focus()
            return
          }
          if (parsed.type === "exit") {
            setStatus("process exited (code " + parsed.code + ")")
            term.write("\r\n\x1b[2m[process exited code=" + parsed.code + "]\x1b[0m\r\n")
            return
          }
          if (parsed.type === "error") {
            setStatus("error: " + parsed.message)
            term.write("\r\n\x1b[31m[error] " + String(parsed.message).replace(/\x1b/g, "") + "\x1b[0m\r\n")
            return
          }
        }
      }
      term.write(raw)
    })

    ws.addEventListener("close", (evt) => {
      ready = false
      const reason = (evt && evt.reason) ? evt.reason : "closed"
      setStatus("disconnected (" + reason + ")")
    })

    ws.addEventListener("error", () => {
      setStatus("websocket error — see server logs")
    })

    term.onData((data) => {
      if (ws.readyState === 1) ws.send(data)
    })

    window.addEventListener("resize", () => {
      pushResize()
    })

    // If the fit addon is ready, recompute on next frame too (in case fonts
    // weren't loaded yet at initial fit()).
    requestAnimationFrame(() => {
      try { fit.fit() } catch { /* noop */ }
      pushResize()
    })
  }

  function loadCss(href) {
    return new Promise((resolve, reject) => {
      const l = document.createElement("link")
      l.rel = "stylesheet"
      l.href = href
      l.onload = () => resolve(true)
      l.onerror = () => reject(new Error("failed to load " + href))
      document.head.appendChild(l)
    })
  }

  bootstrap().catch((err) => {
    setStatus("error: " + (err && err.message ? err.message : String(err)))
    // eslint-disable-next-line no-console
    console.error("[terminal]", err)
  })
})()
