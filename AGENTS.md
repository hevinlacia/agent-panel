# AGENTS.md — agent-panel

## Project Purpose

`agent-panel` is a local **React + Rust** web control panel for browsing pi coding-agent sessions and Hermes/Agent Panel requirement directories.

Current architecture:

- `src/main.rs` — Rust/Axum backend bootstrap, central router, shared `AppState` / query DTOs, SPA fallback, and dashboard stats endpoint. Runs in two modes（分离式架构，local 全功能模式已退役）: default `docs`（hub：文档/状态 SSOT + agent 文档 API + worker 代理，默认绑定 0.0.0.0:8788）and `worker`（`--mode worker` / `PANEL_MODE=worker`，本机执行节点全量路由，默认 127.0.0.1:7332，向 hub 注册心跳）。worker 类请求由 hub 代理转发到在线 worker（无在线 worker 落回 503 stub）；同机部署下 worker 与 hub 共享数据目录，worker 直接写 SSOT。拆分方案见 `docs/split-plan.md`，部署见 `docs/hub-deploy.md`。
- `src/workers.rs` — Worker 注册表与代理：worker 每 30s 向 hub `POST /api/worker/register`（幂等 upsert 兼心跳，`data_dir/workers.json` 原子写，90s 无心跳判 offline）；hub 侧 `GET /api/workers` 观测 + `worker_proxy_middleware` 按 `WORKER_PROXY_PATHS` 白名单转发（`/api/auth-sites/*` 前缀匹配），转发头 `X-Worker-Token`（`PANEL_WORKER_TOKEN` 优先回落 `PANEL_AUTH_TOKEN`）；worker 侧 `spawn_worker_heartbeat`（hub 不可达仅告警本地服务继续）。WS（/ws/pi-chat）暂不代理。
- `src/auth.rs` — token 鉴权中间件（所有模式统一挂载）：`PANEL_AUTH_TOKEN` 配置后启用，`Authorization: Bearer` / `X-Panel-Token` / `?token=`（支持 percent-encoding）任一匹配，常量时间比较；豁免 `/health`、`/api/mode` 与静态资源；未配置时中间件不拦截。
- `src/config.rs` — Runtime config DTOs, `/api/config` handlers（含 availableStatusGates/effectiveStatusGates 注入）, scan-root normalization, status-gate rules（StatusGateRule/内置默认/严格与宽容归一化）, and config persistence.
- `src/http.rs` — Shared HTTP infrastructure: `ApiError`, `ApiResult`, `FormOrJson`, health/notification stubs, and contextual API error help.
- `src/util.rs` — Cross-module utility helpers for time, string cleanup, status/category normalization, JSON/text atomic writes, path/list conversion, ONES parsing, and shell quoting.
- `src/markdown.rs` — Markdown/frontmatter parsing and HTML rendering helpers.
- `src/capability.rs` — Capability/testdata pack APIs（造数脚本能力索引/详情，dry-run 与执行 dispatch：`POST /api/testdata/run` 默认 dry-run，execute=true 且 dryRun=false 才真实执行，env 白名单 test/uat-cn/uat-sea）。
- `src/api_catalog.rs` — WMS 接口测试联动：解析 testdata pack 的 api/catalog.yaml + .bru 模板（占位符提取，vars 映射缺失时 camelCase→WMS_UPPER_SNAKE 启发式；kind 造数/测试/线上修数据标记，catalog 显式 kind 字段优先，prod_fix 仅显式）+ 消费 `api/profiles.yaml` 环境契约（网关域名/必需头/x_params 构造，缺失时内置 fallback）；`GET /api/apitest/apis|api`、`POST /api/apitest/trigger`（dry_run=true 返回脱敏构造预览；登录态 login.mjs 自动获取；三环境实测：test/uat-cn/uat-sea，SEA UAT 按路径前缀双域名，X-PDA-Version 派生，token 不回显不落盘；支持 curl=粘贴真实环境 curl 凭证/域名直接发起（线上修数，仅显式 X-Token，不携带 x-env/Cookie）、headerOverrides=请求头临时改值/停用）、`POST /api/apitest/curl-import`（解析粘贴 curl，凭证值脱敏回显，原文不落盘）。
- `src/cainiao_mock.rs` — Cainiao print WebSocket mock lifecycle and status API.
- `src/pi_config.rs` — Pi settings/model/agent config inspection and safe settings edits.
- `src/git_workflow.rs` — Module root: shared branch-scope/review forms and repo types; submodules under `src/git_workflow/`: `branch_scope`（分支登记轮次）、`code_review`（审查扫描/材料准备/漂移检测/风险标签）、`sync_base`（基线同步）、`prod_mr`（GitLab MR 与环境变量）、`gitlab_auth`（GitLab 三步式认证链：Chrome 登录态 → 账号密码登录 → GITLAB_TOKEN 兕底；会话态走 Cookie+CSRF，令牌走 PRIVATE-TOKEN）、`merge_options`（合并选项规范化）、`merge_exec`（合并执行/检查/worktree）、`release_branch`（整合需求发布分支 git 操作：建分支/推表、远端存在性检查、相对生产分支 diff 统计）、`scan`（分支快照/base-ref 解析）、`git_cmd`（git 命令执行器，超限输出按行边界截断）。
- `src/git_file_lines.rs` — Diff 页展开上下文用的文件行切片 API：`GET /api/git/file-lines`（projectPath+commit+path 校验后 `git show`，返回 totalLines/binary/行切片，单次最多 2500 行），供前端展开 diff 中被折叠的未修改区间。
- `src/git_blame.rs` — Diff 页行级 blame API：`GET /api/git/blame`（projectPath+commit+path 校验后 `git blame --line-porcelain`，返回每行 author/date），供 diff 文件卡「行信息」按钮展示/收起。
- `src/requirement_api.rs` — Requirement HTTP handlers and route-facing orchestration, including code-annotations read/save; effort-estimate API 为 read-modify-write：estimatedHours（agent 评估）与 manualHours（人工矫正）共存，排期统计取 manual ?? estimate。另含 **agent 远程文件编辑 API**（`GET/PUT /api/requirement/file`，容器 hub 挂载只读 + API 写的配套端点）：路径白名单（顶层 .md + docs/<base>/<file>.md 分册，meta.md/state.json 排除）+ sha1 乐观锁（409 冲突时重新 GET）。
- `src/requirement_index.rs` — Requirement directory scanning, session associations, lookup, and dashboard stats.
- `src/requirement_release.rs` — 整合需求发布分支（consolidated release branches）：登记文件 `release-branches.json` 读写与 API（create/list/merge-sub/sync-prod/prod-mr/mark-released）；发布分支以各仓生产分支为 base，子需求分支合入预集成，发布走发布分支 → 生产分支 MR，封版时把覆盖子需求推进「已发布」。
- `src/requirement_service.rs` — Module root: requirement API form DTOs; submodules under `src/requirement_service/`: `create`（创建/更新/备注/事件）、`events`（事件渲染与规范化）、`doc`（文档写入/编辑/章节）、`doc_parts`（文档分册：主文档索引化 + docs/<doc>/ 分册 + 40KB 拆分阈值告警）、`validate`（需求校验）、`paths`（路径安全与可写根解析）、`id_pool`（编号池/序号分配：REQ/WMS 主池、INC 线上问题、TST 测试问题、PERF 性能问题、GRP 组（退役）、FIX 修复需求（category=需求 且绑定 issues 自动路由，独立编号）、ROLLUP 整合需求）、`templates`（建单文件与文档模板）、`state`（状态写入/自动推进，含回到开发中的两类循环轮次：打回返工 reworkRounds（人工核查/测试中 → 开发中，history 标 rework/reworkRound/loop=rework）与发布就绪小循环 releaseReadyRounds（发布就绪 → 开发中，history 标 loop=release-ready/loopRound）；变体判定 dev_loop_kind_from_state 按最近一次进入开发中的来源）、`phase_prompt`（阶段 prompt 加载；开发中按循环语境加载变体：来源=打回 → phase-dev-rework.md 返工变体，来源=发布就绪 → phase-dev-release-ready.md 快速修复变体）、`status_gates`（状态流转门禁注册表与分发：agent API 推进强校验，via=ui 人工改状态跳过；evaluate_status_gate 供状态流转卡片只读评估三态；需求澄清/创建 → 开发中挂 effort-estimate 工时预估门禁，抢修需求豁免）、`selftest_gate`（自测清单门禁：test.md 自测清单三分类（主流程/边界/高并发大流量）逐项结果解析，边界与并发流量类强制先风险场景分析再列清单；放行语义：全部通过放行、存在无法测试/跳过（有原因）放行但警示、存在失败不放行）。
- `src/requirement_context.rs` — Module root: submodule declarations only; submodules under `src/requirement_context/`: `schema`（API schema/token 表）、`intent`（意图与 token 映射）、`context_html`（上下文构建与 HTML 渲染）、`phase`（阶段运行时上下文）、`review_gate`（代码审查门禁判定 + 循环审查记录：主流程/返工轮次/发布就绪小循环三种语境的历史结论按 review.md 轮次小节解析）、`review_drift`（审查快照漂移与风险读取）。
- `src/experience_summary.rs` — Experience-summary job state, auto-dispatch loops, completion fallback, and startup context injection; 触发条件统一为进入「经验总结」（问题家族旧「已复盘」经别名归并入，读路径自动升级）；实际派发受 DispatchWindow 时间窗口限制。分离架构（2026-10-21）：派发/过期兜底循环跑在 hub（决策配置 SSOT），spawn 经 `POST /api/worker/experience-summary/spawn` HTTP 下发在线 worker 本机执行 pi（cwd 优先 `PANEL_WORKSPACE_BASE/<projectRelDir>` 代码工作区）；prompt 内 hub 地址用 `{{HUB_BASE}}` 占位符由 worker 按 PANEL_HUB_URL 替换，报告/文档读写统一走 hub file API。
- `src/config.rs` DispatchWindow — 自动总结派发时间窗口：仅窗口内允许自动派发（start > end 跨午夜；手动派发不受限；窗口内已触发任务执行超时不受限；配置非法 fail-open），设置页「经验自动总结」维护。
- `src/sessions.rs` — Pi session JSONL scanning, timeline parsing, and session APIs.
- `src/knowledge.rs` — Knowledge/experience item search, read, save, and metadata APIs.
- `src/attachments.rs` — Requirement attachment listing, rendering, and context helpers; agent 附件读写 API（`GET/PUT /api/requirement/attachment`，路径白名单限 `attachments/<file>` 单层 + 文本后缀 sql/txt/md/yaml/yml/json/csv，sha1 乐观锁与 file API 同构）。
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
- `web/src/features/requirements/schedule-info-card.tsx` — 需求详情页排期信息卡：提测时间登记 + 倒计时 + Agent 推荐提测日（未登记时按全局容量填充推算，一键采用）；agent 预估展示 + 人工矫正（manualHours），排期统计优先人工矫正。
- `web/src/features/requirements/mermaid-flow.tsx` — Lazy mermaid renderer for annotation flow diagrams; degrades to source on syntax errors.
- `web/src/features/requirements/session-command.ts` — Shared "copy requirement terminal command" helper (pending reuse / force refresh via `/api/requirement/new-session`).
- `web/src/lib/api.ts` — Browser fetch helpers and generic `useFetch` hook.
- `web/src/lib/format.ts` — Date/duration formatting, ONES reference parsing, and CSV/list helpers.
- `web/src/lib/requirements.ts` — Requirement status/category constants and status color metadata.
- `web/src/lib/diff.ts` — Unified diff parsing/stat helpers；含 import 行识别与折叠区块（buildImportBlocks）、未修改区间扫描（computeFileGaps：文件头/hunk 间/文件尾 gap + 行号偏移）。
- `web/src/lib/highlight.ts` — Diff 页语法高亮（shiki + JS 正则引擎，无 WASM）：扩展名推断语言、语法包按需动态加载、按「连续可高亮行」单元整体分词（跨行注释/字符串状态正确），token 结果缓存，失败降级纯文本。
- `web/src/features/requirements/diff-file-card.tsx` — Diff 页单文件卡片：未修改区间可展开标记（短文件 ≤2000 行自动全展开、长文件分块按需展开）、import 区块默认折叠、行级点击选中/多选（选中态与 hunk 解耦）；备注锚定仍用解析后 diff 行下标，gap/折叠行仅是渲染层插入。
- `web/src/lib/annotations.ts` — Matching of code-annotations onto parsed diffs (file index, hunk anchoring, stale detection).
- `web/src/types.ts` — Shared browser-side API DTOs and feature payload types.
- `web/src/pages/testdata.tsx` — 接口测试页（路由 /testdata，侧边栏「接口测试」）：API 接口目录（造数/测试/线上修数据标记 + 关键字筛选 + 卡片点击自动滚动展开详情，占位符入参/模板请求头/请求体临时编辑不写回模板，test/UAT 切换，粘贴真实环境 curl 凭证发起线上修数请求）+ 造数脚本（目标状态/CLI 配置含 count/dry-run/执行）。
- `web/src/pages/schedule.tsx` — 需求排期页（路由 /schedule，侧边栏「需求排期」）：并行均摊模型（投入→提测区间摊派）的每日饱和度、接单评估（能不能接/能接多大）、行内快改工时与提测日、推荐提测日展示与一键采用；ONES 登记工时按折算系数换算真实口径。
- `web/src/lib/schedule.ts` — 排期算法纯函数：4h 单位制、均摊饱和度（utilization）、接单评估（checkAcceptance）、反算容量（maxAcceptable）、推荐提测日（未登记需求按净剩余容量填充推算开发完成日 + submitBufferDays 个工作日预留，默认排期即按推荐值；recommendSummary）；饱和度是容量账不锁定每日日程。
- `web/src/lib/schedule-input.ts` — 排期输入组装（排期页与详情页共用）：需求+ONES 登记工时+本机排期配置 → ScheduleReqInput 队列与 PanelConfig；虚拟工时优先级 = 人工矫正 > agent 预估 > ONES 剩余折算 > 默认 8h。
- `web/tests/schedule.test.ts` — 排期算法 bun test（`bun run test:web`）：推荐提测日容量填充/预留/跨周末/贪心多需求/逾期挤压场景。
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
cargo fmt            # 格式化（rustfmt）
cargo fmt --check    # 验证无格式漂移
cargo check
cargo test
bun run typecheck
bun run build
bun run start
```

Before declaring code changes complete, run at least:

```bash
cargo fmt --check
bun run typecheck
bun run build
cargo test
```

For docs-only changes, re-read the edited docs for stale Node/OpenCode/PTY references.

## Data Repo Principle: agent-panel-data Is Backup Only

数据仓 `agent-panel-data`（本机 clone `~/Developer/agent-panel-data` + GitHub 远端）**只承担备份与人工兜底职责**，任何功能链路不得强依赖它（2026-10-21 拍板）：

1. **agent 读数据一律走 hub API**（需求/知识/经验/审查材料），不读本机镜像、不读 git 仓；镜像符号链接仅供人工与兜底场景使用。
2. **跨副本数据移动一律走 HTTP**：worker→hub 用直传端点（如 `POST /api/requirement/review-snapshot`）；hub→worker 用按需拉取（如 `GET /api/requirement/dir-bundle` + `snapshot_sync::fetch_requirement_bundle_from_hub` 物化；分支登记缺失走 `fetch_branch_scope_from_hub` fallback）。
3. **git 通道（hub-data-autocommit.timer / freshen pull / push_snapshot_commit）只做备份**：失败仅产生 warning，不得阻断任何活链路；新增功能必须满足「git 不可用时照常工作」。
4. **worker 侧识别数据仓**以 remote URL 含 `agent-panel-data` 为准（`snapshot_sync::worker_data_repo_root`），防止误操作 workspace-meta 等工作区仓。
5. 新增跨副本功能前先自问：**git 断了这条链路还转吗？** 答不上来就先补 HTTP 直传/按需物化路径，再合入。

背景：容器 hub 与开发机 worker 各持一份数据副本，早期活链路靠 git 同步承载，暴露过「备料→门禁死结」「分支登记 15min 窗口」「hub 新建需求 worker 不可见」三类问题（INC-149 及 2026-10-21 断 git 回归），现已全部切换 HTTP 直传/按需物化，git 降级纯备份。

## Runtime Data

- Config: `~/.local/share/agent-panel/config.json`（docs 模式可 `PANEL_DATA_DIR` 覆盖，容器部署指向数据卷）
- Worker registry: `<data_dir>/workers.json`（hub 持有；worker 注册/心跳 upsert）
- Associations: `~/.local/share/agent-panel/associations.json`
- Generated pi context: `~/.local/share/agent-panel/ctx/*.md`
- Pi sessions: `~/.pi/agent/sessions/*/*.jsonl`（docs 模式 session root 为空 → 会话关联功能自然降级）
- Service topology（2026-10-20 分离架构落地）：
  - **hub**：远端 LXC 容器 `agent-panel.tail260d93.ts.net:8788`（`/etc/systemd/system/agent-panel.service`，PANEL_DATA_DIR=/srv/agent-panel/state，数据仓库 /srv/agent-panel/data = agent-panel-data clone）。数据自动快照：`hub-data-autocommit.timer`（15min，pull --rebase + commit + push，依赖容器 GitHub deploy key）——**仅备份职责，活链路不依赖 git**（见上方 Data Repo Principle）。
  - **worker**：本机 systemd user 单元 `panel-worker.service`（:7332，PANEL_HUB_URL 指向容器 hub）。hub→worker 回程当前走 ssh 反向隧道 `panel-worker-tunnel.service`（tailnet ACL 未放行 hub→dev 方向；ACL 放行后可改 callbackBase 为 tailscale IP 直连并撤隧道）。
  - **agent 调用面**：`http://agent-panel.tail260d93.ts.net:8788`（pi extensions/scripts/skill docs 已切换，`AGENT_PANEL_URL` 可覆盖）；本机 `agent-panel.service`（:7331）已退役。

## Personal Project Hooks

If `~/.config/opencode/project-overrides/agent-panel.md` exists, read it before making changes. Treat it as additive only; this file wins on architecture and safety rules.
