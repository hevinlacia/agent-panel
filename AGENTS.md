# AGENTS.md — agent-panel

## Project Purpose

`agent-panel` is a local **React + Rust** web control panel for browsing pi coding-agent sessions and Hermes/Agent Panel requirement directories.

Current architecture:

- `src/main.rs` — Rust/Axum backend bootstrap, central router, shared `AppState` / query DTOs, SPA fallback, and dashboard stats endpoint.
- `src/config.rs` — Runtime config DTOs, `/api/config` handlers（含 availableStatusGates/effectiveStatusGates 注入）, scan-root normalization, status-gate rules（StatusGateRule/内置默认/严格与宽容归一化）, and config persistence.
- `src/http.rs` — Shared HTTP infrastructure: `ApiError`, `ApiResult`, `FormOrJson`, health/notification stubs, and contextual API error help.
- `src/util.rs` — Cross-module utility helpers for time, string cleanup, status/category normalization, JSON/text atomic writes, path/list conversion, ONES parsing, and shell quoting.
- `src/markdown.rs` — Markdown/frontmatter parsing and HTML rendering helpers.
- `src/capability.rs` — Capability/testdata pack APIs（造数脚本能力索引/详情，dry-run 与执行 dispatch：`POST /api/testdata/run` 默认 dry-run，execute=true 且 dryRun=false 才真实执行，env 白名单 test/uat-cn/uat-sea）。
- `src/api_catalog.rs` — WMS 接口测试联动：解析 testdata pack 的 api/catalog.yaml + .bru 模板（占位符提取，vars 映射缺失时 camelCase→WMS_UPPER_SNAKE 启发式；kind 造数/测试标记，catalog 显式 kind 字段优先）+ 消费 `api/profiles.yaml` 环境契约（网关域名/必需头/x_params 构造，缺失时内置 fallback）；`GET /api/apitest/apis|api`、`POST /api/apitest/trigger`（dry_run=true 返回脱敏构造预览；登录态 login.mjs 自动获取；三环境实测：test/uat-cn/uat-sea，SEA UAT 按路径前缀双域名，X-PDA-Version 派生，token 不回显不落盘）。
- `src/cainiao_mock.rs` — Cainiao print WebSocket mock lifecycle and status API.
- `src/pi_config.rs` — Pi settings/model/agent config inspection and safe settings edits.
- `src/git_workflow.rs` — Module root: shared branch-scope/review forms and repo types; submodules under `src/git_workflow/`: `branch_scope`（分支登记轮次）、`code_review`（审查扫描/材料准备/漂移检测/风险标签）、`sync_base`（基线同步）、`prod_mr`（GitLab MR 与环境变量）、`merge_options`（合并选项规范化）、`merge_exec`（合并执行/检查/worktree）、`release_branch`（整合需求发布分支 git 操作：建分支/推表、远端存在性检查、相对生产分支 diff 统计）、`scan`（分支快照/base-ref 解析）、`git_cmd`（git 命令执行器）。
- `src/requirement_api.rs` — Requirement HTTP handlers and route-facing orchestration, including code-annotations read/save.
- `src/requirement_index.rs` — Requirement directory scanning, session associations, lookup, and dashboard stats.
- `src/requirement_release.rs` — 整合需求发布分支（consolidated release branches）：登记文件 `release-branches.json` 读写与 API（create/list/merge-sub/sync-prod/prod-mr/mark-released）；发布分支以各仓生产分支为 base，子需求分支合入预集成，发布走发布分支 → 生产分支 MR，封版时把覆盖子需求推进「已发布」。
- `src/requirement_service.rs` — Module root: requirement API form DTOs; submodules under `src/requirement_service/`: `create`（创建/更新/备注/事件）、`events`（事件渲染与规范化）、`doc`（文档写入/编辑/章节）、`doc_parts`（文档分册：主文档索引化 + docs/<doc>/ 分册 + 40KB 拆分阈值告警）、`validate`（需求校验）、`paths`（路径安全与可写根解析）、`id_pool`（编号池/序号分配）、`templates`（建单文件与文档模板）、`state`（状态写入/自动推进，含回到开发中的两类循环轮次：打回返工 reworkRounds（人工核查/测试中 → 开发中，history 标 rework/reworkRound/loop=rework）与发布就绪小循环 releaseReadyRounds（发布就绪 → 开发中，history 标 loop=release-ready/loopRound）；变体判定 dev_loop_kind_from_state 按最近一次进入开发中的来源）、`phase_prompt`（阶段 prompt 加载；开发中按循环语境加载变体：来源=打回 → phase-dev-rework.md 返工变体，来源=发布就绪 → phase-dev-release-ready.md 快速修复变体）、`status_gates`（状态流转门禁注册表与分发：agent API 推进强校验，via=ui 人工改状态跳过；evaluate_status_gate 供状态流转卡片只读评估三态）、`selftest_gate`（自测清单门禁：test.md 自测清单三分类（主流程/边界/高并发大流量）逐项结果解析，边界与并发流量类强制先风险场景分析再列清单；放行语义：全部通过放行、存在无法测试/跳过（有原因）放行但警示、存在失败不放行）。
- `src/requirement_context.rs` — Module root: submodule declarations only; submodules under `src/requirement_context/`: `schema`（API schema/token 表）、`intent`（意图与 token 映射）、`context_html`（上下文构建与 HTML 渲染）、`phase`（阶段运行时上下文）、`review_gate`（代码审查门禁判定 + 循环审查记录：主流程/返工轮次/发布就绪小循环三种语境的历史结论按 review.md 轮次小节解析）、`review_drift`（审查快照漂移与风险读取）。
- `src/experience_summary.rs` — Experience-summary job state, auto-dispatch loops, completion fallback, and startup context injection.
- `src/sessions.rs` — Pi session JSONL scanning, timeline parsing, and session APIs.
- `src/knowledge.rs` — Knowledge/experience item search, read, save, and metadata APIs.
- `src/attachments.rs` — Requirement attachment listing, rendering, and context helpers.
- `src/browser_auth.rs` — Chrome 登录态复用（Browser Auth）：CDP cookie 读取、站点白名单、代发请求、审计日志。
- `src/ones.rs` — ONES 任务候选与推荐：browser_auth 代理聚合消息通知+工时报表信号源，`GET /api/ones/tasks?reqId=` 按需求标题匹配度推荐可关联任务；候选带进程内存缓存（默认命中，refresh=true 才回源 ONES）；候选携带工时报表已登记工时（actualHoursRaw）。
- `src/ones_manhour.rs` — ONES 工时联动：需求工时档案 `ones-manhour.json`（人工预估/agent 实际/录入历史）读写、`GET /api/ones/manhour` 工时四联汇总（人工/自动/ONES 已登记/agent 实际+剩余）、`POST /api/ones/manhour/save`、`POST /api/ones/manhour/log`（雏形为准备模式：生成登记清单+任务链接；ONES 写入接口待抓包验证，接缝函数 push_manhour_to_ones）。
- `src/tests.rs` — Backend unit tests imported from `main.rs` via `#[cfg(test)] mod tests;`.
- `web/src/App.tsx` — React SPA router and remaining legacy page modules; still large, but first low-coupling helpers, DTOs, domain constants, shared UI chrome, requirement badges, and Sessions pages have been extracted.
- `web/src/pages/sessions.tsx` — Sessions list/detail pages and read-only session log viewer.
- `web/src/pages/auth-sites.tsx` — Chrome 登录态复用页：CDP 状态、站点登录状态、白名单请求、Auth 配置编辑。
- `web/src/pages/release-plan.tsx` — 发布计划页：按需求 `plan-release` 登记日期分组（当天/已过期/未来/未登记），发版当天快速查看。
- `web/src/pages/manhour.tsx` — 工时录入页：按状态/创建时间/剩余工时（预估>已录入）/已绑 ONES 筛选需求，勾选填明细后一键生成登记清单（准备模式）。
- `web/src/components/ui.tsx` — Shared page chrome, feedback cards, panel headers, KPI card, and motion variants.
- `web/src/features/requirements/badges.tsx` — Requirement status/experience-summary/ONES badges and requirement display helpers.
- `web/src/features/requirements/annotation-panel.tsx` — Diff page right-hand inspector: file summary, key variables, mermaid flow, hunk notes, JSON hand-editing.
- `web/src/features/requirements/ones-manhour-card.tsx` — 需求详情页 ONES 工时卡片：工时四联展示/编辑（人工预估、agent 实际可编辑）+ 快速登记（准备模式）+ 登记历史。
- `web/src/features/requirements/mermaid-flow.tsx` — Lazy mermaid renderer for annotation flow diagrams; degrades to source on syntax errors.
- `web/src/features/requirements/session-command.ts` — Shared "copy requirement terminal command" helper (pending reuse / force refresh via `/api/requirement/new-session`).
- `web/src/lib/api.ts` — Browser fetch helpers and generic `useFetch` hook.
- `web/src/lib/format.ts` — Date/duration formatting, ONES reference parsing, and CSV/list helpers.
- `web/src/lib/requirements.ts` — Requirement status/category constants and status color metadata.
- `web/src/lib/diff.ts` — Unified diff parsing/stat helpers.
- `web/src/lib/annotations.ts` — Matching of code-annotations onto parsed diffs (file index, hunk anchoring, stale detection).
- `web/src/types.ts` — Shared browser-side API DTOs and feature payload types.
- `web/src/pages/testdata.tsx` — 接口测试页（路由 /testdata，侧边栏「接口测试」）：API 接口目录（造数/测试标记 + 关键字筛选 + 详情触发，test/UAT 切换、入参临时修改不写回模板）+ 造数脚本（目标状态/CLI 配置含 count/dry-run/执行）。
- `web/src/styles.css` — SPA styles scoped under `.react-*`.
- `web/index.html` + `vite.config.ts` — Vite build into `public/dashboard-react/`.

Removed architecture:

- No Node/Fastify/Hono SSR backend.
- No OpenCode compatibility layer, SQLite scanner, experience reports, auto-summary, or report confirmation flow.
- No embedded terminal, PTY, `node-pty`, xterm, or `/ws/session-terminal`.

## Safety Rules

1. Never read or print secret/key files: `.env`, `.env.*`, `credentials.json`, `secrets.json`, `*.pem`, `*.key`, `id_rsa*`, `id_ed25519*`.
2. Do not shell-eval user input. When commands are needed, use fixed argv and validate IDs/paths first.
3. Requirement writes must stay inside the resolved requirement directory and currently target only `state.json`, `meta.md` ONES frontmatter, `effort-estimate.json`, `ones-manhour.json`（工时档案：manualHours/agentHours/logHistory）, `code-annotations.json`, branch-round scope files (`branches-round-*.json`), and generated context files.
4. Pi session ids are UUIDs. Do not reintroduce `ses_` OpenCode id handling.
5. Do not reintroduce PTY/terminal functionality unless the user explicitly asks for it.
6. No git commit/push/branch changes without explicit user request.

## Development Conventions

- Keep backend logic in Rust. Do not add a Node server back.
- Keep frontend as a Vite React SPA. Use browser fetches to `/api/*`; do not add SSR.
- Scope CSS with `.react-*` selectors.
- Prefer small JSON APIs and plain file formats that agents can inspect.
- When splitting large files, extract low-coupling leaf modules first (API helpers, formatters, DTOs, domain constants, shared UI chrome, pure markdown/frontmatter helpers, capability adapters, mock servers, config screens) and run `cargo test` / frontend typecheck after each step.
- Backend module-split pattern (established): keep `<mod>.rs` as module root (shared types/forms + `mod` decls + `pub(crate) use <sub>::*;` re-exports), put submodules in `<mod>/`; each submodule starts with `use super::*;` so crate-root glob paths (`main.rs` re-exports) keep working without touching callers. Move code verbatim, then let `cargo check` drive visibility fixes (`pub(super)` for cross-submodule private helpers).
- Keep shared frontend API DTOs in `web/src/types.ts`; feature utilities such as diff parsing should import those DTOs instead of duplicating near-miss types.
- Keep reusable frontend infrastructure in `web/src/lib/`, cross-page presentational UI in `web/src/components/`, route-level pages in `web/src/pages/`, and feature-specific UI/helpers under `web/src/features/`; page/feature splits should preserve these boundaries.
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
