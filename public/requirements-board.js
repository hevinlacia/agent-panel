/**
 * Role: page-scoped interaction for the flat requirement board filters.
 * Public surface: none; binds the project/subproject cascade and date guard.
 * Constraints: only runs when #req-board-filter-form is present.
 * Read-this-with: src/server.tsx and public/style.css.
 */

(function () {
  "use strict"

  const form = document.getElementById("req-board-filter-form")
  if (!form) return

  const project = document.getElementById("req-board-project-filter")
  const subproject = document.getElementById("req-board-subproject-filter")
  const createdFrom = form.querySelector('input[name="createdFrom"]')
  const createdTo = form.querySelector('input[name="createdTo"]')

  if (project && subproject) {
    project.addEventListener("change", function () {
      subproject.value = ""
      form.requestSubmit()
    })
  }

  form.addEventListener("submit", function (event) {
    if (createdFrom && createdTo && createdFrom.value && createdTo.value && createdFrom.value > createdTo.value) {
      event.preventDefault()
      window.alert("创建时间起不能晚于创建时间止")
    }
  })
})()
