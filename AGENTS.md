# AGENTS.md — opencode-dashboard

> AI / developer guide for the `opencode-dashboard` repo.
> This file is read before making changes. Project-specific rules win; personal
> overlay rules in the `<!-- personal-project-hooks:start -->` block are additive.

## 1. Project purpose

`opencode-dashboard` is a **local web control panel** for browsing and driving
OpenCode sessions on the same machine. It runs as a single Hono + `hono/jsx`
SSR process (no Vite / React build chain) and exposes:

- A **Sessions** dashboard (`/`) styled like an "Operator" console, listing
  recent OpenCode sessions with status, model, worktree, and token stats.
- A **session detail** page (`/session?id=<ses_…>`) that spawns a local
  `opencode --session <id>` TUI inside an embedded `xterm` terminal via
  `node-pty` and a WebSocket bridge.
- The original **experience report** functionality (`/reports`, `/report?path=…`,
  `/api/confirm`) so candidates from `experience-summarizer` runs can still
  be reviewed and confirmed.
- JSON APIs for both (`/api/sessions`, `/api/session`, `/api/reports`,
  `/api/report`, `/api/confirm`).

Default port: `7331` (overridable via `PORT`).

## 2. Architecture (one-paragraph map)

- `src/server.tsx` — Hono app, JSX pages, `upgradeWebSocket` for the terminal,
  static + vendor routes. **This is the only place that wires modules together.**
- `src/sessions.ts` — three-stage session scanner (SQLite → CLI → fs) and the
  `parseModelString` / `deriveWorktree` / `resolveCwd` / `isValidSessionId` helpers.
- `src/terminal.ts` — `node-pty` wrapper: `startSession / writeToSession /
  resizeSession / killSession`. Re-exports the pure parser for convenience.
- `src/terminalProtocol.ts` — **pure** parser `parseClientMessage`. Must not
  import `node-pty` or any native binding (see §3 Safety).
- `src/paths.ts` — `resolveHandoffPath` — the single gate for filesystem paths
  derived from user input on report endpoints.
- `src/parser.ts` + `src/scanner.ts` — `experience-summary` markdown report
  parser and the report scanner (pre-existing functionality, untouched).
- `public/terminal.js` — page-scoped browser script: loads xterm from
  `/vendor/xterm/*` and bridges the WebSocket.
- `public/app.js` — page-scoped browser script: report confirm/reject UI.
- `public/style.css` — single stylesheet, scoped by `.op-*` and `.report-*`
  class names so dashboard and report surfaces don't bleed into each other.
- `tests/*.test.ts` — `node --test + tsx` unit tests for the pure modules.

## 3. Safety rules (do not weaken)

1. **Never read or print secret / key files** — `.env`, `.env.*`,
   `opencode.env`, `credentials.json`, `secrets.json`, `*.pem`, `*.key`,
   `id_rsa*`, `id_ed25519*`. If you need config, use `*.example` files or
   environment variables that are already injected. Do not dump the
   environment as a whole; if a single variable is needed, check that
   specific variable is set, do not print its value alongside others.
2. **Do not shell-eval user input.** All CLI invocations (`sqlite3`,
   `opencode`) use `child_process.spawn` with a fixed argv array, never
   `exec` with a string. The terminal page passes the session id through
   `child_process.spawn` (`src/terminal.ts`) **after** `isValidSessionId`
   is called. No `..` is ever spliced into a path.
3. **SQLite access uses a fixed query and argument substitution by
   the binary, not string interpolation.** The query in
   `src/sessions.ts#SQLITE_QUERY` is the only SQL ever issued; the only
   runtime input is the DB path (a constant in the same file) and the
   `-json` output flag. Do not add user-driven `WHERE` clauses or string
   concatenation to it.
4. **Session id format is `^ses_[A-Za-z0-9]+$`.** Validate via
   `isValidSessionId` (or `SESSION_ID_RE.test(id)`) **before** any PTY
   spawn, before any CLI call, and before any URL builder. The detail page
   re-checks server-side; the WebSocket handler re-checks again.
5. **All report paths (`/report`, `/api/report`, `/api/confirm`) must
   pass through `resolveHandoffPath` in `src/paths.ts`.** The function
   resolves `..`, then enforces a strict prefix boundary against
   `/tmp/opencode/handoff/` (with trailing slash, so the sibling
   `/tmp/opencode/handoff-evil` cannot impersonate the root). Do not
   hand-roll path validation elsewhere.
6. **Static and vendor routes refuse `..`.** Both `app.get("/static/*")`
   and `vendorFile()` in `src/server.tsx` reject paths containing `..`
   or starting with `/` and serve only files under
   `public/` and `node_modules/<pkg>/` respectively.
7. **No git commit / push / PR / branch changes** without an explicit user
   request in the same session. Staging is also out of scope unless the
   user says so. `git status`, `git diff`, `git log` are fine for context.
8. **No edits to OpenCode config, skills, agent definitions, MCP servers,
   permission rules, or the skill registry** (`~/.config/opencode/**`,
   `opencode.jsonc`, `opencode-sync*`, etc.) without an explicit user
   request. Project docs live inside this repo.

## 4. Development conventions

- **Keep the stack as-is.** Hono + `hono/jsx` SSR + TypeScript + `tsx`,
  no Vite, no React, no Next.js. If a feature seems to need a real
  bundler, prefer adding a small ESM file under `public/` or extending
  the existing inline scripts. Do not introduce `npm` dependencies
  without a stated reason.
- **Keep report functionality intact.** `src/parser.ts`, `src/scanner.ts`,
  `/reports`, `/report?path=…`, and `/api/confirm` are part of the product
  surface even though the new dashboard is the front page.
- **Keep the terminal protocol pure parser separate from `node-pty`.**
  `src/terminalProtocol.ts` must stay importable on machines without a
  working PTY toolchain (no native binding imports, no side effects)
  so `tests/terminal.test.ts` can run anywhere.
- **Keep the SQLite → CLI → fs fallback chain.** The cache TTL is
  `CACHE_TTL_MS = 4_000`; do not change the order, and do not skip
  the `fs` fallback — it is what makes the page render when both
  `sqlite3` and `opencode` are missing.
- **Scope CSS by class prefix.** Dashboard styles use `.op-*`; report
  styles use `.report-*`, `.candidate-*`, `.action-bar`, etc. The
  single `public/style.css` must not introduce global selectors that
  could leak between the two surfaces. The detail page header uses
  `op-topbar` + a new `.terminal-wrap` block; keep them sibling-safe.
- **Vendor xterm via `/vendor/xterm/*` and `/vendor/xterm-addon-fit/*`
  directly from `node_modules`**, do not copy binaries into `public/`.
  The three vendor routes are the only xterm integration points.
- **Browser-harness is the visual check** for any UI change. See
  `docs/AI_DEVELOPMENT.md` §7 for the standard commands and the
  expected DOM invariants.
- **No business logic in JSX** beyond formatting. Heavy lifting
  (scanning, parsing, PTY management, path resolution) belongs in
  `src/*.ts` modules that can be unit-tested.

## 5. Verification checklist (run before declaring done)

Adjust toolchain calls to use the project's chosen runtime manager.
`mise` is the global default; if the repo does not pin a version, run
`mise list` then `mise current` to pick a Node + npm combination.

```bash
# 0. Pick the right toolchain (skip if you already know the active version).
mise list                  # confirm node/npm are installed
mise current               # see the active version

# 1. Compile-only check.
mise exec -- npm run typecheck

# 2. Unit tests (paths, sessions, terminalProtocol).
mise exec -- npm test

# 3. Manual / visual check (only when the change touched UI, CSS, or the
#    embedded terminal page).
npm start &                 # serves on http://localhost:7331
# Use the browser-harness skill to:
#   - screenshot /
#   - assert laneCount, no horizontal overflow, MODEL + WORKTREE labels visible
#   - open /session?id=<ses_…> and confirm xterm mounts + WS connects
#   - open /reports and confirm cards still render
```

For docs-only changes, `typecheck` and `npm test` are not required; just
re-read the created files for sanity (no broken links, no secrets).

## 6. House rules

- Do not add new top-level dependencies without an explicit reason in the
  PR description.
- Do not reformat or reorder existing code unrelated to the task.
- Keep the README a one-page tour; put the long-form handoff in
  `docs/AI_DEVELOPMENT.md`. Update both when the public surface changes.
- When in doubt, ask before guessing — return `Need main-agent decision`
  (or escalate to the human) instead of inventing a policy.

<!-- personal-project-hooks:start -->
## Personal Project Hooks

Before starting work in this project, check for personal project convention files and apply them as additive guidance:

1. If `~/.config/opencode/project-overrides/opencode-dashboard.md` exists, read it before making changes.
2. Treat personal project conventions as additive. They must not override repository safety, compliance, release, or team workflow rules in this `AGENTS.md`.
3. If there is a conflict, this project's `AGENTS.md` wins.
4. Use the personal conventions for exploration order, local environment defaults, recurring commands, and knowledge entrypoints only.
<!-- personal-project-hooks:end -->
