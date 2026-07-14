/**
 * public/req-detail.js
 *
 * Role: page-scoped script for the requirement detail page and the
 * "extract context from session" preview page. Handles four things:
 *   1. Intercepts `data-extract-trigger` POST forms so the spawn runs
 *      in the background; polls /api/extract/job/:id and shows a brief
 *      3-second flash toast on each state transition. Persistent state
 *      lives in the topbar notification center.
 *   2. Auto-poll on the preview page while the underlying job is still
 *      in `running` state — reloads once it transitions.
 *   3. Clipboard copy buttons (`data-copy-cmd`) for session commands.
 *   4. `req-new-session-btn` triggers background `opencode run` with
 *      the requirement's injection context; on success it renders a
 *      copyable `opencode -s <id>` command next to the button.
 *   5. Code-review forms get lightweight disabled labels while a PRO diff
 *      scan or verdict save is in-flight; the three-pane review workspace
 *      supports file search, file switching, current-file context, and a
 *      persistent diff font-size control (A− / A+ / reset).
 *
 * Why a separate file from public/app.js: app.js is a single-IIFE
 * report-page-only script that early-returns on other pages. Mixing
 * the requirement-page logic in would break that early-return.
 *
 * Constraints / safety:
 *   - No external deps; vanilla DOM only.
 *   - Never logs or stores secrets — only opaque job ids.
 *   - Clipboard copy falls back to a temporary <textarea> + execCommand
 *     when the page is loaded over an insecure context (no `navigator.
 *     clipboard`).
 *
 * Read-this-with:
 *   - src/server.tsx (route shape: POST … /extract-context, GET
 *     /api/extract/job/:id, GET /requirement/extract?jobId=…)
 *   - src/notifications.ts (the persistent store)
 *   - public/style.css `.op-toast*` / `.req-extract-*` / `.req-copy-cmd-*`
 */

(function () {
  "use strict"

  const POLL_INTERVAL_MS = 1500
  const POLL_MAX_MS = 5 * 60_000
  const FLASH_MS = 3000

  const toastHost = document.getElementById("op-toast-host")

  // ------------------------------------------------------------------
  // Toast helpers — brief flash only. Persistent state goes to the
  // notification center.
  // ------------------------------------------------------------------

  /** Show a transient toast that auto-disappears after FLASH_MS. */
  function flashToast(opts) {
    if (!toastHost) return
    const el = document.createElement("div")
    el.className = "op-toast op-toast-extract op-toast-" + (opts.state || "running")
    const titleEl = document.createElement("div")
    titleEl.className = "op-toast-title"
    titleEl.textContent = opts.title || ""
    el.appendChild(titleEl)
    if (opts.subtitle) {
      const subEl = document.createElement("div")
      subEl.className = "op-toast-sub muted small"
      subEl.textContent = opts.subtitle
      el.appendChild(subEl)
    }
    toastHost.appendChild(el)
    setTimeout(function () { el.remove() }, FLASH_MS)
  }

  function showError(message) {
    flashToast({ state: "failed", title: message })
  }

  // ------------------------------------------------------------------
  // Job lifecycle: poll silently in the background; flash only on
  // start, transition to done, transition to failed.
  // ------------------------------------------------------------------

  function pollJob(jobId) {
    const startedAt = Date.now()
    let lastState = "running"

    function tick() {
      fetch("/api/extract/job/" + encodeURIComponent(jobId), { cache: "no-store" })
        .then(function (res) {
          if (res.status === 404) {
            flashToast({ state: "failed", title: "任务已过期", subtitle: "请去通知中心或重新点提取上下文。" })
            return null
          }
          if (!res.ok) throw new Error("HTTP " + res.status)
          return res.json()
        })
        .then(function (job) {
          if (!job) return
          if (job.state === "running") {
            if (Date.now() - startedAt < POLL_MAX_MS) {
              setTimeout(tick, POLL_INTERVAL_MS)
            }
            return
          }
          if (job.state !== lastState) {
            lastState = job.state
            const dur = job.elapsedMs ? (job.elapsedMs / 1000).toFixed(1) + "s" : "完成"
            if (job.state === "done") {
              var title
              if (job.mode === "auto") {
                title = "✓ 上下文分析完成（" + dur + "）"
              } else if (job.salvagedFromFork) {
                title = "✓ 已从 fork 救回摘要（" + dur + "）"
              } else {
                title = "✓ 摘要生成完成（" + dur + "）"
              }
              flashToast({ state: "done", title: title, subtitle: "点右上角 🔔 查看，或在通知中心进入预览页" })
            } else {
              flashToast({ state: "failed", title: "✗ 生成失败", subtitle: "点右上角 🔔 查看详情" })
            }
          }
        })
        .catch(function () {
          if (Date.now() - startedAt < POLL_MAX_MS) {
            setTimeout(tick, POLL_INTERVAL_MS)
          }
        })
    }

    tick()
  }

  /**
   * Submit a `data-extract-trigger` form via fetch; on success
   * (202 or 409) start polling the returned jobId.
   */
  function startExtract(form) {
    const reqId = form.querySelector('input[name="reqId"]').value
    const sessionId = form.querySelector('input[name="sessionId"]').value
    const button = form.querySelector('button[type="submit"]')
    const action = form.getAttribute("action") || "/api/requirement/extract-context"
    const isAuto = action.includes("auto-extract")

    flashToast({
      state: "running",
      title: isAuto ? "已提交智能提取任务" : "已提交摘要任务",
      subtitle: "session " + sessionId + " · 完成后在 🔔 通知中心查看",
    })
    if (button) {
      button.disabled = true
      // Match the server-side debounce window for auto-extract; use a
      // shorter disable for summary mode since it has no debounce guard.
      var disableMs = isAuto ? 5 * 60_000 : 5_000
      setTimeout(function () { button.disabled = false }, disableMs)
    }

    const body = new URLSearchParams()
    body.set("reqId", reqId)
    body.set("sessionId", sessionId)

    fetch(action, {
      method: "POST",
      body: body,
      headers: { "Content-Type": "application/x-www-form-urlencoded" },
    })
      .then(function (res) {
        return res.json().then(function (data) { return { status: res.status, data: data } })
      })
      .then(function (r) {
        if ((r.status === 202 || r.status === 409) && r.data && r.data.jobId) {
          const jobId = r.data.jobId
          if (r.status === 409) {
            flashToast({ state: "running", title: "已有相同 session 的任务在跑", subtitle: "继续跟踪现有任务" })
          }
          pollJob(jobId)
        } else if (r.status === 202 && r.data && r.data.queued) {
          // Queued by the per-requirement delay queue.
          var mins = r.data.delayMs ? Math.round(r.data.delayMs / 60000) : 5
          var pos = r.data.queuePosition != null ? "（第 " + (r.data.queuePosition + 1) + " 位）" : ""
          flashToast({
            state: "running",
            title: "⏳ 智能提取已排队" + pos,
            subtitle: "预计 " + mins + " 分钟后自动开始，完成后在 🔔 通知中心查看",
          })
        } else if (r.status === 409 && r.data && r.data.message) {
          // Debounce or no-new-content rejection from the server.
          flashToast({ state: "failed", title: "已跳过", subtitle: r.data.message })
        } else {
          showError("提交失败：HTTP " + r.status + " " + (r.data && r.data.error ? r.data.error : ""))
        }
      })
      .catch(function (err) {
        showError("提交失败：" + (err && err.message ? err.message : err))
      })
  }

  // ------------------------------------------------------------------
  // Bind triggers (detail page)
  // ------------------------------------------------------------------

  document.querySelectorAll("form[data-extract-trigger]").forEach(function (form) {
    form.addEventListener("submit", function (ev) {
      ev.preventDefault()
      startExtract(form)
    })
  })

  // ------------------------------------------------------------------
  // Auto-poll on preview-running page
  // ------------------------------------------------------------------

  const runningSection = document.querySelector(".req-extract-running[data-job-id]")
  if (runningSection) {
    const jobId = runningSection.getAttribute("data-job-id")
    if (jobId) {
      const startedAt = Date.now()
      const tick = function () {
        fetch("/api/extract/job/" + encodeURIComponent(jobId), { cache: "no-store" })
          .then(function (res) { return res.ok ? res.json() : null })
          .then(function (job) {
            if (!job) return
            const elapsedEl = runningSection.querySelector(".js-extract-elapsed")
            if (elapsedEl) {
              elapsedEl.textContent = String(Math.round((Date.now() - startedAt) / 1000))
            }
            if (job.state !== "running") {
              window.location.reload()
              return
            }
            if (Date.now() - startedAt < POLL_MAX_MS) {
              setTimeout(tick, POLL_INTERVAL_MS)
            }
          })
          .catch(function () {
            if (Date.now() - startedAt < POLL_MAX_MS) setTimeout(tick, POLL_INTERVAL_MS)
          })
      }
      setTimeout(tick, POLL_INTERVAL_MS)
    }
  }

  // ------------------------------------------------------------------
  // "另开新 session" buttons — POST /api/requirement/new-session which
  // spawns a detached `opencode run "<context>"` and polls for the new
  // session id. On success we render a copyable command next to the
  // button (the user pastes it into their own terminal, the dashboard
  // does NOT keep a PTY open).
  // ------------------------------------------------------------------

  function buildResultCommandEl(cmd) {
    const code = document.createElement("code")
    code.textContent = cmd

    const copyBtn = document.createElement("button")
    copyBtn.type = "button"
    copyBtn.className = "req-copy-cmd-inline req-copy-cmd-inline-new-session"
    copyBtn.setAttribute("data-copy-cmd", cmd)
    copyBtn.title = "复制 \`" + cmd + "\` 到剪贴板"
    copyBtn.textContent = "📋 复制"

    const wrap = document.createElement("span")
    wrap.appendChild(code)
    wrap.appendChild(document.createTextNode(" "))
    wrap.appendChild(copyBtn)
    return wrap
  }

  function attachNewSessionHandler(btn) {
    btn.addEventListener("click", function (ev) {
      ev.preventDefault()
      if (btn.disabled) return
      const reqId = btn.getAttribute("data-req-id") || ""
      if (!reqId) return
      const resultSpan = document.querySelector(
        ".req-new-session-result[data-req-id=\"" + cssEscape(reqId) + "\"]"
      )
      btn.disabled = true
      const restoreBtn = function () { btn.disabled = false }
      if (resultSpan) {
        resultSpan.textContent = "创建中…"
      }

      const body = new URLSearchParams()
      body.set("reqId", reqId)

      fetch("/api/requirement/new-session", {
        method: "POST",
        body: body,
        headers: { "Content-Type": "application/x-www-form-urlencoded" },
      })
        .then(function (res) {
          return res.json().then(function (data) {
            return { status: res.status, data: data }
          })
        })
        .then(function (r) {
          if (r.status >= 200 && r.status < 300 && r.data && r.data.sessionId && r.data.command) {
            if (resultSpan) {
              resultSpan.textContent = ""
              resultSpan.appendChild(buildResultCommandEl(r.data.command))
            }
          } else {
            const msg = (r.data && r.data.error) ? r.data.error : ("HTTP " + r.status)
            if (resultSpan) {
              resultSpan.textContent = "✗ " + msg
            }
            restoreBtn()
          }
        })
        .catch(function (err) {
          const msg = (err && err.message) ? err.message : String(err)
          if (resultSpan) {
            resultSpan.textContent = "✗ " + msg
          }
          restoreBtn()
        })
    })
  }

  // Minimal CSS.escape polyfill — req ids are URL-safe today, but be
  // defensive against any future character that needs escaping. The
  // value is interpolated into a [data-req-id="..."] selector, so we
  // only need to escape characters that would close the string or the
  // attribute selector.
  function cssEscape(s) {
    if (typeof CSS !== "undefined" && CSS && typeof CSS.escape === "function") {
      return CSS.escape(s)
    }
    return String(s).replace(/([\\"])/g, "\\$1")
  }

  document.querySelectorAll(".req-new-session-btn").forEach(attachNewSessionHandler)

  document.querySelectorAll(".code-review-scan-form, .code-review-verdict-form").forEach(function (form) {
    form.addEventListener("submit", function () {
      const btn = form.querySelector('button[type="submit"]')
      if (!btn || btn.disabled) return
      btn.disabled = true
      btn.textContent = form.classList.contains("code-review-scan-form") ? "刷新中…" : "保存中…"
    })
  })

  const reviewFileButtons = Array.from(document.querySelectorAll("[data-review-file-button]"))
  const reviewFilePanels = Array.from(document.querySelectorAll("[data-review-file-panel]"))
  const reviewFileSearch = document.getElementById("code-review-file-search")
  const reviewFileEmpty = document.getElementById("code-review-file-empty")
  const reviewCurrentFile = document.getElementById("code-review-current-file")
  const reviewCurrentRepo = document.getElementById("code-review-current-repo")
  const reviewNoteFile = document.getElementById("code-review-note-file")
  const reviewNoteRepo = document.getElementById("code-review-note-repo")

  function selectReviewFile(button) {
    if (!button) return
    const key = button.getAttribute("data-review-file-button") || ""
    reviewFileButtons.forEach(function (candidate) {
      const active = candidate === button
      candidate.classList.toggle("is-active", active)
      candidate.setAttribute("aria-pressed", active ? "true" : "false")
    })
    reviewFilePanels.forEach(function (panel) {
      panel.hidden = panel.getAttribute("data-review-file-panel") !== key
    })
    const path = button.getAttribute("data-review-file-path") || ""
    const repo = button.getAttribute("data-review-file-repo") || ""
    if (reviewCurrentFile) reviewCurrentFile.textContent = path
    if (reviewCurrentRepo) reviewCurrentRepo.textContent = repo
    if (reviewNoteFile) reviewNoteFile.textContent = path
    if (reviewNoteRepo) reviewNoteRepo.textContent = repo
    const diffPane = document.querySelector(".code-review-diff-pane")
    if (diffPane) diffPane.scrollTop = 0
  }

  reviewFileButtons.forEach(function (button) {
    button.addEventListener("click", function () {
      selectReviewFile(button)
    })
  })

  if (reviewFileSearch) {
    reviewFileSearch.addEventListener("input", function () {
      const query = reviewFileSearch.value.trim().toLowerCase()
      let visibleCount = 0
      reviewFileButtons.forEach(function (button) {
        const haystack = button.getAttribute("data-review-file-filter") || ""
        const visible = !query || haystack.includes(query)
        button.hidden = !visible
        if (visible) visibleCount += 1
      })
      document.querySelectorAll("[data-review-file-group]").forEach(function (group) {
        group.hidden = !group.querySelector("[data-review-file-button]:not([hidden])")
      })
      if (reviewFileEmpty) reviewFileEmpty.hidden = visibleCount > 0
      const active = document.querySelector("[data-review-file-button].is-active:not([hidden])")
      if (!active) selectReviewFile(document.querySelector("[data-review-file-button]:not([hidden])"))
    })
  }

  // ------------------------------------------------------------------
  // Code review diff font size: A− / A+ / reset buttons in the diff pane
  // head. The scale is persisted in localStorage and applied as the CSS
  // custom property --code-review-font-scale, consumed only by
  // .code-review-table. An inline script on the review page restores the
  // value before the table paints (no flash); here we wire the buttons,
  // re-apply on load to sync the SSR "100%" label, and clamp 0.6–2.0.
  // ------------------------------------------------------------------

  var FONT_SCALE_KEY = "agent-panel:code-review:font-scale"
  var FONT_SCALE_MIN = 0.6
  var FONT_SCALE_MAX = 2.0
  var FONT_SCALE_STEP = 0.1
  var fontScaleLabel = document.getElementById("code-review-fontsize-label")
  var fontScaleGroup = document.querySelector(".code-review-fontsize")

  function clampFontScale(n) {
    if (!isFinite(n)) return 1
    return Math.min(FONT_SCALE_MAX, Math.max(FONT_SCALE_MIN, n))
  }

  function readFontScale() {
    var raw = null
    try { raw = localStorage.getItem(FONT_SCALE_KEY) } catch (e) {}
    return clampFontScale(parseFloat(raw))
  }

  function applyFontScale(scale) {
    document.documentElement.style.setProperty("--code-review-font-scale", String(scale))
    if (fontScaleLabel) fontScaleLabel.textContent = Math.round(scale * 100) + "%"
  }

  // Re-apply on load so the SSR label ("100%") matches a stored preference.
  applyFontScale(readFontScale())

  if (fontScaleGroup) {
    fontScaleGroup.addEventListener("click", function (ev) {
      var btn = ev.target && ev.target.closest ? ev.target.closest("[data-fontsize]") : null
      if (!btn) return
      var action = btn.getAttribute("data-fontsize")
      var next
      if (action === "up") {
        next = clampFontScale(+(readFontScale() + FONT_SCALE_STEP).toFixed(2))
      } else if (action === "down") {
        next = clampFontScale(+(readFontScale() - FONT_SCALE_STEP).toFixed(2))
      } else {
        next = 1
      }
      try { localStorage.setItem(FONT_SCALE_KEY, String(next)) } catch (e) {}
      applyFontScale(next)
    })
  }

  // ------------------------------------------------------------------
  // AI code review: the "AI 审查代码" button on the code-diff page POSTs
  // to /api/requirement/code-review/ai with the requirement id. The model
  // may take tens of seconds, so we show a loading state and render the
  // Markdown suggestions into the read-only textarea below the button.
  // ------------------------------------------------------------------
  var aiBtn = document.getElementById("code-review-ai-btn")
  var aiStatus = document.getElementById("code-review-ai-status")
  var aiResult = document.getElementById("code-review-ai-result")
  if (aiBtn && aiResult) {
    aiBtn.addEventListener("click", function () {
      var reqId = aiBtn.getAttribute("data-req-id") || ""
      if (!reqId) return
      aiBtn.disabled = true
      aiBtn.textContent = "AI 审查中…"
      if (aiStatus) { aiStatus.textContent = "正在调用模型，可能需要 10-60 秒…"; aiStatus.classList.remove("is-warn") }
      fetch("/api/requirement/code-review/ai", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ reqId: reqId }),
      }).then(function (res) { return res.json().then(function (data) { return { status: res.status, data: data } }) })
        .then(function (envelope) {
          var data = envelope.data || {}
          var review = data.aiReview || {}
          if (review.content) aiResult.value = review.content
          if (data.ok) {
            if (aiStatus) aiStatus.textContent = "完成 · " + (review.model || "") + (review.updatedAt ? " · " + formatReviewTime(review.updatedAt) : "")
          } else {
            if (aiStatus) { aiStatus.textContent = "失败：" + (data.error || "未知错误"); aiStatus.classList.add("is-warn") }
          }
        }).catch(function (err) {
          if (aiStatus) { aiStatus.textContent = "请求失败：" + (err && err.message ? err.message : err); aiStatus.classList.add("is-warn") }
        }).finally(function () {
          aiBtn.disabled = false
          aiBtn.textContent = "🤖 AI 审查代码"
        })
    })
  }

  function formatReviewTime(ts) {
    if (!ts) return ""
    try { return new Date(ts).toLocaleString("zh-CN") } catch (e) { return "" }
  }

  // ------------------------------------------------------------------
  // Clipboard copy: any element with `data-copy-cmd="..."` copies that
  // string when clicked, briefly swapping its label to "✓ 已复制".
  // ------------------------------------------------------------------

  document.addEventListener("click", function (ev) {
    const btn = ev.target && ev.target.closest ? ev.target.closest("[data-copy-cmd]") : null
    if (!btn) return
    ev.preventDefault()
    const cmd = btn.getAttribute("data-copy-cmd") || ""
    if (!cmd) return
    const finish = function () {
      const orig = btn.textContent
      btn.textContent = "✓ 已复制"
      btn.classList.add("req-copy-done")
      setTimeout(function () {
        btn.textContent = orig
        btn.classList.remove("req-copy-done")
      }, 1500)
    }
    if (navigator.clipboard && navigator.clipboard.writeText) {
      navigator.clipboard.writeText(cmd).then(finish).catch(function () {
        copyViaTextarea(cmd)
        finish()
      })
    } else {
      copyViaTextarea(cmd)
      finish()
    }
  })

  function copyViaTextarea(text) {
    const ta = document.createElement("textarea")
    ta.value = text
    ta.style.position = "fixed"
    ta.style.left = "-9999px"
    document.body.appendChild(ta)
    ta.select()
    try { document.execCommand("copy") } catch {}
    document.body.removeChild(ta)
  }
})()
