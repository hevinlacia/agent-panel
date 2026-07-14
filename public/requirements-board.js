/**
 * Role: page-scoped interaction for the flat requirement board filters.
 * Public surface: none; binds the project/subproject cascade, date guard,
 * category auto-submit, and keyword search-as-you-type.
 * Constraints: only runs when #req-board-filter-form is present.
 * Read-this-with: src/server.tsx and public/style.css.
 */

(function () {
  "use strict"

  const form = document.getElementById("req-board-filter-form")
  if (!form) return

  const project = document.getElementById("req-board-project-filter")
  const subproject = document.getElementById("req-board-subproject-filter")
  const category = document.getElementById("req-board-category-filter")
  const createdFrom = form.querySelector('input[name="createdFrom"]')
  const createdTo = form.querySelector('input[name="createdTo"]')
  const keywordInput = form.querySelector('input[name="q"]')

  // Project cascade: changing the top-level project resets the subproject
  // and immediately submits so the subproject options refresh.
  if (project && subproject) {
    project.addEventListener("change", function () {
      subproject.value = ""
      form.requestSubmit()
    })
  }

  // Category filter auto-submits on change for instant filtering.
  if (category) {
    category.addEventListener("change", function () {
      form.requestSubmit()
    })
  }

  // Date guard: prevent "from" being later than "to".
  form.addEventListener("submit", function (event) {
    if (createdFrom && createdTo && createdFrom.value && createdTo.value && createdFrom.value > createdTo.value) {
      event.preventDefault()
      window.alert("创建时间起不能晚于创建时间止")
    }
  })

  // Keyword search: debounce auto-submit so the board filters as the user
  // types, without firing on every keystroke. Preserves the current cursor
  // position by not reloading until the user pauses typing.
  if (keywordInput) {
    var debounceMs = 500
    var timer = null
    var lastValue = keywordInput.value
    keywordInput.addEventListener("input", function () {
      if (timer) clearTimeout(timer)
      timer = setTimeout(function () {
        timer = null
        // Only submit when the value actually changed to avoid redundant reloads.
        if (keywordInput.value !== lastValue) {
          lastValue = keywordInput.value
          form.requestSubmit()
        }
      }, debounceMs)
    })
    // Submit immediately on Enter.
    keywordInput.addEventListener("keydown", function (ev) {
      if (ev.key === "Enter") {
        if (timer) clearTimeout(timer)
        form.requestSubmit()
      }
    })
  }
})()
