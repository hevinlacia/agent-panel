/**
 * Role: Vite build entry for the React dashboard island.
 * Public surface: builds web/src/main.tsx into public/dashboard-react for Fastify static serving.
 * Constraints: keeps the server stack intact while allowing React to own high-interaction pages.
 * Read-this-with: web/src/App.tsx and src/server.tsx dashboard routes.
 */
import { defineConfig } from "vite"
import react from "@vitejs/plugin-react"
import { resolve } from "node:path"

export default defineConfig({
  root: "web",
  publicDir: false,
  plugins: [react()],
  build: {
    outDir: "../public/dashboard-react",
    emptyOutDir: true,
    cssCodeSplit: false,
    sourcemap: false,
    rollupOptions: {
      input: resolve(__dirname, "web/src/main.tsx"),
      output: {
        entryFileNames: "dashboard.js",
        chunkFileNames: "chunks/[name].js",
        assetFileNames: "dashboard.[ext]",
      },
    },
  },
})
