/**
 * public/env-vars.js
 *
 * Role: page-scoped script for the /env-vars page. Handles environment
 * variable add/overwrite/delete. The "覆盖" button opens a modal dialog
 * for entering a new value instead of scrolling to the top form.
 *
 * Constraints / safety:
 *   - No external deps; vanilla DOM only.
 *   - Values are write-only; the server returns redacted previews.
 *
 * Read-this-with:
 *   - src/config.ts (upsertEnvVar / deleteEnvVar / safeEnvVarsByFile)
 *   - src/server.tsx (/env-vars route + /api/config/env)
 */

(function () {
  "use strict"

  var envForm = document.getElementById("env-form")
  var envSaved = document.getElementById("env-saved")
  var envName = document.getElementById("env-name")
  var envValue = document.getElementById("env-value")
  var envNote = document.getElementById("env-note")
  var envFile = document.getElementById("env-file")
  var envClear = document.getElementById("env-clear")

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

  /* ── Top form: add / overwrite ── */

  function resetEnvForm() {
    if (envName) envName.value = ""
    if (envValue) envValue.value = ""
    if (envNote) envNote.value = ""
    if (envName) envName.readOnly = false
    if (envValue) envValue.placeholder = "粘贴新值覆盖旧值"
    if (envFile) {
      var defaultOpt = envFile.querySelector('option[value="secrets"]')
      if (defaultOpt) envFile.value = "secrets"
    }
  }

  if (envForm) {
    envForm.addEventListener("submit", function (ev) {
      ev.preventDefault()
      var name = envName.value.trim()
      var value = envValue.value
      var note = envNote.value.trim()
      var file = envFile ? envFile.value : "secrets"
      requestJson("/api/config/env", { action: "upsert", name: name, value: value, note: note, file: file })
        .then(function () {
          showSaved(envSaved)
          window.location.reload()
        })
        .catch(function (err) {
          alert("保存变量失败：" + (err && err.message ? err.message : err))
        })
    })
  }

  if (envClear) {
    envClear.addEventListener("click", resetEnvForm)
  }

  /* ── Modal dialog for overwrite ── */

  function openEditModal(name, file, note, placeholder) {
    // Remove any existing modal
    var existing = document.getElementById("env-modal-overlay")
    if (existing) existing.remove()

    var overlay = document.createElement("div")
    overlay.id = "env-modal-overlay"
    overlay.className = "env-modal-overlay"

    var modal = document.createElement("div")
    modal.className = "env-modal"

    var title = document.createElement("div")
    title.className = "env-modal-title"
    title.textContent = "覆盖环境变量"

    var varName = document.createElement("div")
    varName.className = "env-modal-var-name"
    varName.textContent = name

    var label = document.createElement("label")
    label.className = "env-modal-label"
    label.textContent = "新值"
    label.htmlFor = "env-modal-input"

    var input = document.createElement("input")
    input.id = "env-modal-input"
    input.className = "env-modal-input"
    input.type = "password"
    input.placeholder = placeholder || "输入新值"
    input.autocomplete = "off"
    input.spellcheck = false

    var status = document.createElement("div")
    status.className = "env-modal-status"

    var actions = document.createElement("div")
    actions.className = "env-modal-actions"

    var cancelBtn = document.createElement("button")
    cancelBtn.type = "button"
    cancelBtn.className = "btn btn-secondary"
    cancelBtn.textContent = "取消"

    var confirmBtn = document.createElement("button")
    confirmBtn.type = "button"
    confirmBtn.className = "btn btn-primary"
    confirmBtn.textContent = "确认"

    actions.appendChild(cancelBtn)
    actions.appendChild(confirmBtn)
    modal.appendChild(title)
    modal.appendChild(varName)
    modal.appendChild(label)
    modal.appendChild(input)
    modal.appendChild(status)
    modal.appendChild(actions)
    overlay.appendChild(modal)
    document.body.appendChild(overlay)

    input.focus()

    function closeModal() {
      overlay.remove()
    }

    function doSave() {
      var value = input.value
      if (!value) {
        status.textContent = "请输入新值"
        status.className = "env-modal-status env-modal-status-error"
        input.focus()
        return
      }
      confirmBtn.disabled = true
      cancelBtn.disabled = true
      status.textContent = "保存中…"
      status.className = "env-modal-status"
      requestJson("/api/config/env", {
        action: "upsert",
        name: name,
        value: value,
        note: note || "",
        file: file || "secrets",
      })
        .then(function () {
          status.textContent = "✓ 已保存，正在刷新…"
          status.className = "env-modal-status env-modal-status-success"
          window.location.reload()
        })
        .catch(function (err) {
          status.textContent = "保存失败：" + (err && err.message ? err.message : err)
          status.className = "env-modal-status env-modal-status-error"
          confirmBtn.disabled = false
          cancelBtn.disabled = false
        })
    }

    cancelBtn.addEventListener("click", closeModal)
    confirmBtn.addEventListener("click", doSave)
    input.addEventListener("keydown", function (ev) {
      if (ev.key === "Enter") {
        ev.preventDefault()
        doSave()
      }
      if (ev.key === "Escape") {
        closeModal()
      }
    })
    overlay.addEventListener("click", function (ev) {
      if (ev.target === overlay) closeModal()
    })
  }

  /* ── Delegate clicks in variable lists ── */

  var groups = document.querySelectorAll(".env-file-group")
  groups.forEach(function (group) {
    group.addEventListener("click", function (ev) {
      var target = ev.target
      if (!target || !target.dataset) return
      if (target.classList.contains("env-edit")) {
        var name = target.dataset.name || ""
        var file = target.dataset.file || "secrets"
        var note = target.dataset.note || ""
        var placeholder = target.dataset.placeholder || "输入新值"
        openEditModal(name, file, note, placeholder)
        return
      }
      if (target.classList.contains("env-delete")) {
        var delName = target.dataset.name || ""
        if (!delName || !confirm("删除环境变量 " + delName + "？")) return
        requestJson("/api/config/env", { action: "delete", name: delName })
          .then(function () { window.location.reload() })
          .catch(function (err) {
            alert("删除变量失败：" + (err && err.message ? err.message : err))
          })
      }
    })
  })
})()
