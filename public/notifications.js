/**
 * public/notifications.js
 *
 * Role: page-scoped script for the notification center. Injects a bell
 * icon into the topbar with a badge showing unread count. On click, a
 * dropdown panel lists all non-dismissed notifications with per-item
 * actions (jump to preview, dismiss) and panel-level "全部标记已读" /
 * "全部清除" buttons. Polls unread count every 5s.
 *
 * Constraints / safety:
 *   - No external deps; vanilla DOM only.
 *   - Never logs or stores secrets — only opaque notification ids.
 *   - The bell markup is server-rendered in src/server.tsx Layout;
 *     this script only attaches event listeners and fetches data.
 *
 * Read-this-with:
 *   - src/notifications.ts (the store this script reads from).
 *   - src/server.tsx (the Layout that renders the bell skeleton).
 *   - public/style.css (`.op-notify-*` rules).
 */

(function () {
  "use strict"

  const POLL_MS = 5000

  const bell = document.getElementById("op-notify-bell")
  const badge = document.getElementById("op-notify-badge")
  const panel = document.getElementById("op-notify-panel")
  const list = document.getElementById("op-notify-list")
  const empty = document.getElementById("op-notify-empty")
  const markReadBtn = document.getElementById("op-notify-mark-read")
  const dismissAllBtn = document.getElementById("op-notify-dismiss-all")

  if (!bell || !panel) return

  // ------------------------------------------------------------------
  // State
  // ------------------------------------------------------------------

  var open = false

  // ------------------------------------------------------------------
  // Rendering
  // ------------------------------------------------------------------

  function stateIcon(state) {
    if (state === "running") return '<span class="op-notify-dot op-notify-dot-running" aria-hidden="true"></span>'
    if (state === "done") return '<span class="op-notify-dot op-notify-dot-done" aria-hidden="true"></span>'
    return '<span class="op-notify-dot op-notify-dot-failed" aria-hidden="true"></span>'
  }

  function stateLabel(state) {
    if (state === "running") return "运行中"
    if (state === "done") return "已完成"
    return "失败"
  }

  function formatAgo(ms) {
    var age = Math.max(0, Date.now() - ms)
    var sec = Math.floor(age / 1000)
    if (sec < 60) return sec + "s"
    var min = Math.floor(sec / 60)
    if (min < 60) return min + "m"
    var hr = Math.floor(min / 60)
    if (hr < 24) return hr + "h"
    return Math.floor(hr / 24) + "d"
  }

  function renderItem(n) {
    var li = document.createElement("li")
    li.className = "op-notify-item"
    if (n.unread) li.classList.add("op-notify-item-unread")

    var left = document.createElement("div")
    left.className = "op-notify-item-left"
    left.innerHTML = stateIcon(n.state) +
      '<span class="op-notify-item-state">' + stateLabel(n.state) + '</span>'
    li.appendChild(left)

    var mid = document.createElement("div")
    mid.className = "op-notify-item-mid"
    mid.innerHTML = '<div class="op-notify-item-title">' + escapeHtml(n.title) + '</div>' +
      (n.subtitle ? '<div class="op-notify-item-sub muted small">' + escapeHtml(n.subtitle) + '</div>' : '') +
      '<div class="op-notify-item-time muted small">' + formatAgo(n.createdAt) + '前</div>'
    li.appendChild(mid)

    var right = document.createElement("div")
    right.className = "op-notify-item-right"

    if (n.actionHref && n.state !== "running") {
      var link = document.createElement("a")
      link.href = n.actionHref
      link.className = "op-notify-item-link"
      link.textContent = n.state === "done" ? "查看预览" : "查看详情"
      right.appendChild(link)
    }

    var dismissBtn = document.createElement("button")
    dismissBtn.type = "button"
    dismissBtn.className = "op-notify-item-dismiss"
    dismissBtn.setAttribute("aria-label", "关闭")
    dismissBtn.innerHTML = "&#x2715;"
    dismissBtn.onclick = function () { dismissOne(n.id) }
    right.appendChild(dismissBtn)

    li.appendChild(right)
    return li
  }

  function escapeHtml(s) {
    return String(s).replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;")
  }

  function renderPanel(notifications) {
    list.innerHTML = ""
    if (notifications.length === 0) {
      empty.hidden = false
      list.hidden = true
    } else {
      empty.hidden = true
      list.hidden = false
      notifications.forEach(function (n) {
        list.appendChild(renderItem(n))
      })
    }
  }

  // ------------------------------------------------------------------
  // API calls
  // ------------------------------------------------------------------

  function fetchList() {
    return fetch("/api/notifications", { cache: "no-store" })
      .then(function (res) { return res.ok ? res.json() : null })
      .catch(function () { return null })
  }

  function fetchCount() {
    return fetch("/api/notifications/unread-count", { cache: "no-store" })
      .then(function (res) { return res.ok ? res.json() : null })
      .catch(function () { return null })
  }

  function postDismiss(body) {
    return fetch("/api/notifications/dismiss", {
      method: "POST",
      body: body,
      headers: { "Content-Type": "application/x-www-form-urlencoded" },
    })
  }

  function postMarkRead() {
    return fetch("/api/notifications/mark-read", { method: "POST" })
  }

  // ------------------------------------------------------------------
  // Actions
  // ------------------------------------------------------------------

  function updateBadge() {
    fetchCount().then(function (data) {
      if (!data) return
      var count = data.count || 0
      if (count > 0) {
        badge.textContent = count > 99 ? "99+" : String(count)
        badge.hidden = false
      } else {
        badge.hidden = true
      }
    })
  }

  function refreshPanel() {
    if (!open) return
    fetchList().then(function (data) {
      if (!data) return
      renderPanel(data.notifications || [])
    })
  }

  function togglePanel() {
    open = !open
    if (open) {
      panel.hidden = false
      bell.setAttribute("aria-expanded", "true")
      refreshPanel()
    } else {
      panel.hidden = true
      bell.setAttribute("aria-expanded", "false")
    }
  }

  function dismissOne(id) {
    var body = new URLSearchParams()
    body.set("id", id)
    postDismiss(body).then(function () {
      refreshPanel()
      updateBadge()
    })
  }

  // ------------------------------------------------------------------
  // Event bindings
  // ------------------------------------------------------------------

  bell.addEventListener("click", function (ev) {
    ev.stopPropagation()
    togglePanel()
  })

  document.addEventListener("click", function (ev) {
    if (open) {
      var container = document.getElementById("op-notify")
      if (container && !container.contains(ev.target)) {
        open = false
        panel.hidden = true
        bell.setAttribute("aria-expanded", "false")
      }
    }
  })

  if (markReadBtn) {
    markReadBtn.addEventListener("click", function () {
      postMarkRead().then(function () {
        refreshPanel()
        updateBadge()
      })
    })
  }

  if (dismissAllBtn) {
    dismissAllBtn.addEventListener("click", function () {
      var body = new URLSearchParams()
      body.set("all", "1")
      postDismiss(body).then(function () {
        refreshPanel()
        updateBadge()
      })
    })
  }

  // ------------------------------------------------------------------
  // Init
  // ------------------------------------------------------------------

  updateBadge()
  setInterval(updateBadge, POLL_MS)

  // Also refresh the panel every 5s when it's open (e.g. while
  // watching a running job tick).
  setInterval(refreshPanel, POLL_MS)
})()