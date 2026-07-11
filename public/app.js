/**
 * Role: shared browser polish for the Agent Panel shell and report detail page.
 * Public surface: none; binds DOM events after SSR renders static markup.
 * Constraints / safety: page-scoped only, no user input is shell-executed.
 * Read-this-with: src/server.tsx for markup contracts and public/style.css for classes.
 */

(function () {
  "use strict"

  document.documentElement.classList.add("op-js")

  const animatedLinks = document.querySelectorAll('a[href]:not([target]):not([download])')
  animatedLinks.forEach((link) => {
    const href = link.getAttribute("href") || ""
    if (!href || href.startsWith("#") || href.startsWith("javascript:") || href.startsWith("mailto:")) return
    link.addEventListener("click", function () {
      document.body.classList.add("op-page-leaving")
    })
  })

  document.querySelectorAll(".dash-kpi-value").forEach((node) => {
    const raw = (node.textContent || "").trim()
    const value = Number(raw)
    if (!Number.isFinite(value) || raw === "" || value > 9999) return
    const start = performance.now()
    const duration = 520
    const tick = (now) => {
      const progress = Math.min(1, (now - start) / duration)
      const eased = 1 - Math.pow(1 - progress, 3)
      node.textContent = String(Math.round(value * eased))
      if (progress < 1) requestAnimationFrame(tick)
      else node.textContent = raw
    }
    requestAnimationFrame(tick)
  })

  document
    .querySelectorAll(".op-lane, .req-board-card, .report-card, .candidate-card, .proj-card, .req-card, .settings-section, .terminal-wrap, .op-hints")
    .forEach((node, index) => {
      node.style.setProperty("--op-i", String(Math.min(index, 16)))
    })

  const forceRefresh = document.getElementById("op-force-refresh")
  if (forceRefresh) {
    forceRefresh.addEventListener("click", function () {
      const url = new URL(window.location.href)
      url.searchParams.set("_force", String(Date.now()))
      window.location.replace(url.toString())
    })
  }

  // ----- Sessions list (page-scoped) --------------------------------------
  // Refresh button is a plain link to /sessions/refresh; no JS required.

  // ----- Report detail (page-scoped) --------------------------------------
  const reportPath = window.__REPORT_PATH__
  if (!reportPath) return

  const confirmedIds = window.__CONFIRMED_IDS__ || []
  const rejectedIds = window.__REJECTED_IDS__ || []

  const checkboxes = document.querySelectorAll('input[type="checkbox"][data-cid]')
  if (checkboxes.length === 0) return
  const selectionInfo = document.getElementById("selection-info")
  const btnConfirm = document.getElementById("btn-confirm")
  const btnReject = document.getElementById("btn-reject")
  const btnSelectAll = document.getElementById("btn-select-all")
  const btnDeselectAll = document.getElementById("btn-deselect-all")

  function getSelectedIds() {
    return Array.from(checkboxes)
      .filter((cb) => cb.checked)
      .map((cb) => cb.dataset.cid)
  }

  function updateUI() {
    const ids = getSelectedIds()
    if (selectionInfo) selectionInfo.textContent = `${ids.length} selected`
    if (btnConfirm) btnConfirm.disabled = ids.length === 0
    if (btnReject) btnReject.disabled = ids.length === 0

    // Update card visual state
    document.querySelectorAll(".candidate-card").forEach((card) => {
      const cid = card.dataset.cid
      const cb = card.querySelector(`input[data-cid="${cid}"]`)
      card.classList.toggle("checked", cb && cb.checked)
    })
  }

  async function submitSelection(mode) {
    const ids = getSelectedIds()
    if (ids.length === 0) return

    btnConfirm.disabled = true
    btnReject.disabled = true

    try {
      const res = await fetch("/api/confirm", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          reportPath: reportPath,
          confirmedIds: mode === "confirm" ? ids : [],
          rejectedIds: mode === "reject" ? ids : [],
          mode: mode,
        }),
      })
      const data = await res.json()
      if (data.ok) {
        if (data.executionTriggered) {
          showToast(
            `✓ Confirmed ${ids.length} candidate(s): ${ids.join(", ")} — execution fork started`,
            "success"
          )
        } else {
          showToast(
            mode === "confirm"
              ? `✓ Confirmed ${ids.length} candidate(s): ${ids.join(", ")}`
              : `✗ Rejected ${ids.length} candidate(s): ${ids.join(", ")}`,
            "success"
          )
        }
      } else {
        showToast("Error: " + (data.error || "unknown"), "error")
      }
    } catch (err) {
      showToast("Network error: " + err.message, "error")
    } finally {
      updateUI()
    }
  }

  function showToast(msg, type) {
    const toast = document.createElement("div")
    toast.className = "toast" + (type === "error" ? " error" : "")
    toast.textContent = msg
    document.body.appendChild(toast)
    setTimeout(() => toast.remove(), 5000)
  }

  // Event listeners
  checkboxes.forEach((cb) => cb.addEventListener("change", updateUI))
  if (btnConfirm) btnConfirm.addEventListener("click", () => submitSelection("confirm"))
  if (btnReject) btnReject.addEventListener("click", () => submitSelection("reject"))
  if (btnSelectAll) btnSelectAll.addEventListener("click", () => {
    checkboxes.forEach((cb) => (cb.checked = true))
    updateUI()
  })
  if (btnDeselectAll) btnDeselectAll.addEventListener("click", () => {
    checkboxes.forEach((cb) => (cb.checked = false))
    updateUI()
  })

  updateUI()
})()
