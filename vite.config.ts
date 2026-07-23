/**
 * Role: Vite build entry for the React Agent Panel SPA.
 * Public surface: builds web/index.html into public/dashboard-react for the Rust server.
 * Constraints: the backend only serves static SPA files plus JSON APIs; no SSR or PTY bundle.
 * Read-this-with: web/src/App.tsx and src/main.rs.
 */
import { defineConfig } from "vite"
import react from "@vitejs/plugin-react"

export default defineConfig({
  root: "web",
  publicDir: false,
  plugins: [react()],
  build: {
    outDir: "../public/dashboard-react",
    emptyOutDir: true,
    sourcemap: false,
    rollupOptions: {
      output: {
        entryFileNames: "assets/[name]-[hash].js",
        chunkFileNames: "assets/[name]-[hash].js",
        assetFileNames: "assets/[name]-[hash][extname]",
      },
    },
  },
})
