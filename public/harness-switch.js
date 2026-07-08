/**
 * public/harness-switch.js
 *
 * Role: page-scoped script loaded on every page via Layout. Renders the
 * top-right OpenCode/Pi harness toggle, keeps its active state in sync
 * with /api/config, and reloads the page on switch so every route
 * re-reads the active harness.
 *
 * Constraints / safety: no external deps; vanilla DOM only. Never logs
 * secrets - it only reads/writes the `harness` field of AppConfig.
 *
 * Read-this-with:
 *   - src/server.tsx (Layout topbar markup, /api/config GET+POST)
 *   - src/config.ts (AppConfig.harness)
 */

(function () {
  "use strict"

  var box = document.getElementById("op-harness-switch")
  if (!box) return

  var buttons = box.querySelectorAll(".op-harness-btn")
  var ready = false

  function setActive(harness) {
    ready = true
    buttons.forEach(function (btn) {
      var match = btn.getAttribute("data-harness") === harness
      btn.setAttribute("aria-pressed", match ? "true" : "false")
      btn.classList.toggle("op-harness-btn-active", match)
    })
  }

  // Sync current state from the server.
  fetch("/api/config", { cache: "no-store" })
    .then(function (res) { return res.ok ? res.json() : null })
    .then(function (cfg) {
      if (cfg && (cfg.harness === "pi" || cfg.harness === "opencode")) {
        setActive(cfg.harness)
      } else {
        setActive("opencode")
      }
    })
    .catch(function () { setActive("opencode") })

  buttons.forEach(function (btn) {
    btn.addEventListener("click", function () {
      if (!ready) return
      var next = btn.getAttribute("data-harness")
      if (next !== "pi" && next !== "opencode") return
      // Optimistic active state; the reload re-syncs from the server.
      setActive(next)
      fetch("/api/config", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ harness: next }),
      })
        .then(function () {
          // Reload so list/session/terminal routes re-read the harness.
          window.location.reload()
        })
        .catch(function () {
          // Revert on failure; the server is the source of truth.
          fetch("/api/config", { cache: "no-store" })
            .then(function (r) { return r.ok ? r.json() : null })
            .then(function (cfg) { setActive((cfg && cfg.harness) || "opencode") })
        })
    })
  })
})()
