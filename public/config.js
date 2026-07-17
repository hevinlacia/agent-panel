/**
 * public/config.js
 *
 * Role: page-scoped script for the Settings page. Handles saves for the
 * scheduler config form (extract, valuation, sync toggles) and the AI
 * code-review config form (base URL / model / API key).
 *
 * Constraints / safety:
 *   - No external deps; vanilla DOM only.
 *   - The API key is type=password and intentionally left blank on load;
 *     an empty value tells the server to keep the existing key.
 *
 * Read-this-with:
 *   - src/config.ts (the store this script writes to)
 *   - src/server.tsx (/settings route + /api/config)
 */

(function () {
  "use strict"

  function showSaved(el) {
    if (!el) return
    el.hidden = false
    setTimeout(function () { el.hidden = true }, 2000)
  }

  function requestJson(url, data) {
    return fetch(url, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(data),
    }).then(function (res) {
      if (!res.ok) throw new Error("HTTP " + res.status)
      return res.json()
    })
  }

  function bindSchedulerForm() {
    var form = document.getElementById("config-form")
    var saved = document.getElementById("config-saved")
    if (!form) return
    form.addEventListener("submit", function (ev) {
      ev.preventDefault()

      var repoInputs = Array.prototype.slice.call(document.querySelectorAll(".cfg-github-repo"))
      var data = {
        autoExtract: document.getElementById("cfg-auto-extract").checked,
        autoExtractSchedule: document.getElementById("cfg-auto-extract-schedule").checked,
        fullSyncSchedule: document.getElementById("cfg-full-sync-schedule").checked,
        fullSyncTimes: document.getElementById("cfg-full-sync-times").value.split(/[，,\s]+/).map(function (s) { return s.trim() }).filter(Boolean),
        fullSyncGithubRepos: repoInputs.filter(function (el) { return el.checked }).map(function (el) { return el.value }),
        extractModel: document.getElementById("cfg-model").value.trim(),
        minChangeMessages: parseInt(document.getElementById("cfg-min-change").value, 10),
        autoValuation: document.getElementById("cfg-auto-valuation").checked,
        valuationThreshold: parseInt(document.getElementById("cfg-valuation-threshold").value, 10),
      }

      requestJson("/api/config", data)
        .then(function () { showSaved(saved) })
        .catch(function (err) {
          alert("保存失败：" + (err && err.message ? err.message : err))
        })
    })
  }

  // Model config form: each task picks a pi "provider/model". The server
  // resolves the provider's baseUrl + API key from pi's config, so there is
  // no separate Base URL / API Key to submit.
  function bindCodeReviewForm() {
    var form = document.getElementById("code-review-config-form")
    var saved = document.getElementById("code-review-config-saved")
    if (!form) return
    form.addEventListener("submit", function (ev) {
      ev.preventDefault()
      function val(id) {
        var el = document.getElementById(id)
        return el ? (el.value || "").trim() : ""
      }
      var data = {
        codeReviewPiModel: val("cfg-code-review-pi-model"),
        branchScopePiModel: val("cfg-branch-scope-pi-model"),
        effortEstimatePiModel: val("cfg-effort-estimate-pi-model"),
      }
      var btn = form.querySelector("button[type=\"submit\"]")
      if (btn) { btn.disabled = true; btn.textContent = "保存中…" }
      requestJson("/api/config", data)
        .then(function () { showSaved(saved) })
        .catch(function (err) {
          alert("保存失败：" + (err && err.message ? err.message : err))
        })
        .finally(function () {
          if (btn) { btn.disabled = false; btn.textContent = "保存模型配置" }
        })
    })
  }

  bindSchedulerForm()
  bindCodeReviewForm()
})()
