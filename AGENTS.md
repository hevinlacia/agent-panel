# AGENTS.md — agent-panel

## Project Purpose

`agent-panel` is a local **React + Rust** web control panel for browsing pi coding-agent sessions and Hermes/Agent Panel requirement directories.

Current architecture:

- `src/main.rs` — Rust/Axum backend bootstrap, central router, shared `AppState` / query DTOs, SPA fallback, and dashboard stats endpoint.
- `src/config.rs` — Runtime config DTOs, `/api/config` handlers, scan-root normalization, and config persistence.
- `src/http.rs` — Shared HTTP infrastructure: `ApiError`, `ApiResult`, `FormOrJson`, health/notification stubs, and contextual API error help.
- `src/util.rs` — Cross-module utility helpers for time, string cleanup, status/category normalization, JSON/text atomic writes, path/list conversion, ONES parsing, and shell quoting.
- `src/markdown.rs` — Markdown/frontmatter parsing and HTML rendering helpers.
- `src/capability.rs` — Capability/testdata pack read-only APIs and runner preview/dispatch logic.
- `src/cainiao_mock.rs` — Cainiao print WebSocket mock lifecycle and status API.
- `src/pi_config.rs` — Pi settings/model/agent config inspection and safe settings edits.
- `src/git_ai.rs` — Git AI health checks, suspect record refresh, note re-push, and fix-note agent dispatch.
- `src/git_workflow.rs` — Requirement branch scope, code-review diff generation, sync-base, merge status, and GitLab MR helpers.
- `src/requirement_api.rs` — Requirement HTTP handlers and route-facing orchestration.
- `src/requirement_index.rs` — Requirement directory scanning, session associations, lookup, and dashboard stats.
- `src/requirement_service.rs` — Requirement create/update/edit/doc/event/state validation and write helpers.
- `src/requirement_context.rs` — Requirement schema, token context, phase runtime, review gate, and context HTML rendering.
- `src/experience_summary.rs` — Experience-summary job state, auto-dispatch loops, completion fallback, and startup context injection.
- `src/sessions.rs` — Pi session JSONL scanning, timeline parsing, and session APIs.
- `src/knowledge.rs` — Knowledge/experience item search, read, save, and metadata APIs.
- `src/attachments.rs` — Requirement attachment listing, rendering, and context helpers.
- `src/tests.rs` — Backend unit tests imported from `main.rs` via `#[cfg(test)] mod tests;`.
- `web/src/App.tsx` — React SPA router/pages; still large, but shared diff logic/types are being extracted.
- `web/src/lib/diff.ts` — Unified diff parsing/stat helpers.
- `web/src/types.ts` — Shared browser-side API DTOs.
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
- When splitting large files, extract low-coupling leaf modules first (pure markdown/frontmatter helpers, capability adapters, mock servers, config screens) and run `cargo test` / frontend typecheck after each step.
- Keep shared frontend API DTOs in `web/src/types.ts`; feature utilities such as diff parsing should import those DTOs instead of duplicating near-miss types.
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
