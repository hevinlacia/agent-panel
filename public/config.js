/**
 * public/config.js
 *
 * Role: page-scoped script for pages containing the dashboard config form.
 * Handles scheduler config saves (extract, valuation, sync toggles).
 *
 * Constraints / safety:
 *   - No external deps; vanilla DOM only.
 *
 * Read-this-with:
 *   - src/config.ts (the store this script writes to)
 *   - src/server.tsx (/schedulers route + /api/config)
 */

(function () {
  "use strict"

  var form = document.getElementById("config-form")
  var saved = document.getElementById("config-saved")

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

  if (form) {
    form.addEventListener("submit", function (ev) {
      ev.preventDefault()

      var data = {
        autoExtract: document.getElementById("cfg-auto-extract").checked,
        autoExtractSchedule: document.getElementById("cfg-auto-extract-schedule").checked,
        fullSyncSchedule: document.getElementById("cfg-full-sync-schedule").checked,
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
})()
