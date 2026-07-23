# AGENTS.md — agent-panel

## Project Purpose

`agent-panel` is a local **React + Rust** web control panel for browsing pi coding-agent sessions and Hermes/Agent Panel requirement directories.

Current architecture:

- `src/main.rs` — Rust/Axum backend, static SPA serving, JSON APIs, pi JSONL scanning, requirement file reads/writes.
- `web/src/App.tsx` — React SPA pages for dashboard, requirements, sessions, and settings.
- `web/src/styles.css` — SPA styles scoped under `.react-*`.
- `web/index.html` + `vite.config.ts` — Vite build into `public/dashboard-react/`.

Removed architecture:

- No Node/Fastify/Hono SSR backend.
- No OpenCode compatibility layer, SQLite scanner, experience reports, auto-summary, or report confirmation flow.
- No embedded terminal, PTY, `node-pty`, xterm, or `/ws/session-terminal`.

## Safety Rules

1. Never read or print secret/key files: `.env`, `.env.*`, `credentials.json`, `secrets.json`, `*.pem`, `*.key`, `id_rsa*`, `id_ed25519*`.
2. Do not shell-eval user input. When commands are needed, use fixed argv and validate IDs/paths first.
3. Requirement writes must stay inside the resolved requirement directory and currently target only `state.json`, `meta.md` ONES frontmatter, `effort-estimate.json`, and generated context files.
4. Pi session ids are UUIDs. Do not reintroduce `ses_` OpenCode id handling.
5. Do not reintroduce PTY/terminal functionality unless the user explicitly asks for it.
6. No git commit/push/branch changes without explicit user request.

## Development Conventions

- Keep backend logic in Rust. Do not add a Node server back.
- Keep frontend as a Vite React SPA. Use browser fetches to `/api/*`; do not add SSR.
- Scope CSS with `.react-*` selectors.
- Prefer small JSON APIs and plain file formats that agents can inspect.
- Generated bundle `public/dashboard-react/` and Rust `target/` are build outputs.

## Toolchain

- Package manager / frontend script dispatcher: Bun.
- Backend: Cargo/Rust.

Commands:

```bash
bun install
bun run build:dashboard
cargo check
cargo test
bun run typecheck
bun run build
bun run start
```

Before declaring code changes complete, run at least:

```bash
bun run typecheck
bun run build
cargo test
```

For docs-only changes, re-read the edited docs for stale Node/OpenCode/PTY references.

## Runtime Data

- Config: `~/.local/share/agent-panel/config.json`
- Associations: `~/.local/share/agent-panel/associations.json`
- Generated pi context: `~/.local/share/agent-panel/ctx/*.md`
- Pi sessions: `~/.pi/agent/sessions/*/*.jsonl`

## Personal Project Hooks

If `~/.config/opencode/project-overrides/agent-panel.md` exists, read it before making changes. Treat it as additive only; this file wins on architecture and safety rules.
