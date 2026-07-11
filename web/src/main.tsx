/**
 * Role: bootstraps the React dashboard island inside the server-rendered shell.
 * Public surface: none; mounts <App /> into #dashboard-root.
 * Constraints: only owns the dashboard page and fetches data through local JSON APIs.
 * Read-this-with: web/src/App.tsx and src/server.tsx DashboardPage.
 */
import React from "react"
import { createRoot } from "react-dom/client"
import { App } from "./App"
import "./styles.css"

const root = document.getElementById("dashboard-root")
if (root) {
  createRoot(root).render(
    <React.StrictMode>
      <App apiPath={root.dataset.api || "/api/dashboard/stats"} />
    </React.StrictMode>,
  )
}
