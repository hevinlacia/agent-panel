# agent-panel

本地常驻 Web 控制面，用浏览器查看 **pi coding agent** session 与需求进度。项目已重写为：

- **前端**：TypeScript + React + Vite + Framer Motion
- **后端**：Rust + Axum
- **数据源**：pi JSONL session、Hermes/Agent Panel 需求目录、`~/.local/share/agent-panel/*`

OpenCode 旧兼容代码、经验报告链路、Node/Fastify SSR、`node-pty`、xterm/embedded terminal 已移除。

## 快速开始

```bash
cd ~/Developer/tools/agent-panel
bun install
bun run build
bun run start
# -> http://localhost:7331
```

常用命令：

```bash
bun run build:dashboard   # Vite 构建 React SPA
bun run build:backend     # cargo build --release
bun run build             # 前后端完整构建
bun run typecheck         # TS 类型检查 + cargo check
bun run test              # cargo test
bun run start:backend     # cargo run（开发后端）
```

## 能力源（第一阶段 / 第二阶段）

Agent Panel 目前做的是**只读能力接入**：

- `GET /api/capability/sources` — 列出已接入的 capability source
- `GET /api/testdata/capabilities?project=WMS` — 按项目列出测试数据能力
- `GET /api/testdata/capability?id=<capability-id>&project=WMS` — 查看单个能力详情

当前接入规则：

- **优先源**：`/home/hevin/Developer/company/WMS/.agents/testdata`
- **旧源 fallback**：`/home/hevin/Developer/tools/wms-testdata-recipes`
- **项目根**：`/home/hevin/Developer/company/WMS`

说明：

- 第一阶段：只读索引，不执行脚本。
- 第二阶段：WMS 业务资产已迁到项目根下的 `.agents/testdata/`，Agent Panel 优先读取这里；旧工具仓只作为 fallback，便于回滚和对照。
- 第三阶段：Agent Panel 暴露通用 capability pack schema（`GET /api/capability/schema`），并把 WMS legacy 字段规范化为 `normalized` 视图，后续项目可按同一协议接入。
- 真正的数据仍留在项目目录或 capability pack 中，不写进 Agent Panel 仓库。

## 页面

| 路径 | 说明 |
| --- | --- |
| `/` `/dashboard` | 需求 KPI、状态分布、交付周期 |
| `/projects` | 需求进度看板 |
| `/requirement?id=<req>` | 需求详情、业务背景文档、经验总结、状态/类别/ONES、关联 session、新 pi session 命令 |
| `/sessions` | pi session 列表 |
| `/session?id=<uuid>` | pi session 元数据详情（无 terminal） |
| `/settings` | 需求扫描目录、模型偏好、Pi `settings.json` 编辑、菜鸟打印 Mock 开关 |

## 菜鸟打印 Mock

内置 mock 菜鸟云打印客户端（原 `tools/fake-cainiao` 功能，已整合为本进程内 WS 服务），让 WMS 前端以为打印成功。

- 在 Settings 页开启「启用菜鸟打印 Mock」并保存后立即生效，无需重启；关闭后立即停止。
- 监听 `ws://127.0.0.1:13528`（与 WMS 前端默认端口一致，可在设置里改）。
- 支持 `getPrinters`（5 台 Mock 打印机）与 `print`（返回 `status: success`）两个命令。
- 开关状态持久化在 `~/.local/share/agent-panel/config.json`（`cainiaoMockEnabled` / `cainiaoMockPort`），后端启动时自动按配置拉起。
- 运行状态可查 `GET /api/cainiao-mock/status`。

> 原 `tools/fake-cainiao` 仓库为 Node 独立实现，功能已并入 agent-panel 后不再需要单独启动。

旧页面 `/reports`、`/report`、`/schedulers`、`/env-vars`、`/git-ai` 会显示“已移除”说明。

## Rust API

核心接口：

- `GET /health`
- `GET /api/dashboard/stats`
- `GET /api/requirements`
- `POST /api/requirements` — 创建需求目录和标准文件；支持 JSON/form，`dryRun=true` 只返回计划写入路径
- `GET /api/requirement?id=<req>`
- `PATCH /api/requirement` / `POST /api/requirement/update` — 修改受控字段（title/project/projects/status/category/owner/startDate/planRelease/ones）
- `GET /api/requirement/schema` — 返回需求文件 token、intent、允许操作和推荐流程，供 agent 无需翻源码即可理解协议
- `GET /api/requirement/edit-plan?id=<req>&intent=<intent>` — 按意图返回应该读/写的 token、文件路径、大小和示例写法
- `GET /api/requirement/context?id=<req>&intent=<intent>&budget=2000` — 按 token budget 返回压缩上下文，避免 agent 直接读取超大 `notes.md` / `code-review.json`
- `GET /api/requirement/context?id=<req>&for=agent&intent=<intent>&budget=2000` — 返回 agent 专用摘要：关键文档摘要、最近结构化事件、推荐写入 API，以及按最新需求状态实时生成的 `phaseRuntime.currentPhasePrompt`
- `POST /api/requirement/events` — 记录结构化事件（问题、根因、测试、决策、TODO 等）到 `events.jsonl`，默认同步追加 `notes.md`
- `POST|PATCH /api/requirement/sections/{section}` — 按语义章节 upsert 文档块，例如 `impact`、`test`、`boxCodeIssue`
- `POST /api/requirement/edit` — 统一结构化编辑入口，支持 `setStatus`、`setCategory`、`patchMeta`、`appendNote`、`writeDoc`、`upsertSection`
- `POST /api/requirement/notes` — 追加 `notes.md` 进展块，不覆盖原文
- `GET|PUT|POST /api/requirement/doc` — GET 读取受控文档；PUT/POST 写入受控文档，`docType/file=background|memory|branch|config-changes|release-manifest|impact|test|notes|review|release-check|experience-summary|alignment|prd`，`mode=replace|append`
- `POST /api/requirement/validate` — 校验 `meta.md`、`state.json`、标准文件和 `branches.json` 基本结构
- `GET /api/requirement/review-gate?id=<req>` — 读取代码审查门禁状态；自测中推进到测试中时，后端会强制要求 PASS 或 WAIVED
- `POST /api/requirement/master-diff` — 按 `branches.json` 对比需求分支和指定基准分支
- `GET /api/requirement/merge-options?id=<req>` — 返回前端/后端可选环境分支和按需求状态计算的默认选中值
- `POST /api/requirement/merge-branch` — 按 `branches.json` 将需求分支合并到选择的 `targetBranch`；无冲突则自动推送，冲突则返回 `conflictFiles` 和保留的 `worktreePath`
- `GET /api/requirement/merge-status?id=<req>&target=test|uat` — 查看未完成合并 worktree / 冲突状态，供详情页和 agent 续处理
- `POST /api/requirement/prod-mrs` — 按 `branches.json` 创建或复用生产 MR
- `POST /api/requirement/status`
- `POST /api/requirement/category`
- `POST /api/requirement/ones`
- `POST /api/requirement/associate`
- `POST /api/requirement/dissociate`
- `POST /api/requirement/new-session`
- `GET /api/sessions?days=7`
- `GET /api/session?id=<uuid>`
- `GET/POST /api/config`
- `GET/POST /api/pi-config/file?file=settings`

示例：

```bash
# 创建需求（正式写入；加 dryRun=true 可预览）
curl -sS -H 'Content-Type: application/json' \
  -X POST http://localhost:7331/api/requirements \
  -d '{"reqId":"WMS-001-demo","title":"示例需求","project":"WMS","summary":"统一由 Agent Panel 创建需求文件"}'

# 修改需求字段，状态/类别会写入 state.json，meta 字段由服务端更新
curl -sS -H 'Content-Type: application/json' \
  -X PATCH http://localhost:7331/api/requirement \
  -d '{"reqId":"WMS-001-demo","status":"开发中","note":"进入开发","owner":"hevin"}'

# 追加 notes.md
curl -sS -H 'Content-Type: application/json' \
  -X POST http://localhost:7331/api/requirement/notes \
  -d '{"reqId":"WMS-001-demo","title":"进展","text":"完成方案梳理。"}'

# 写受控文档
curl -sS -H 'Content-Type: application/json' \
  -X PUT http://localhost:7331/api/requirement/doc \
  -d '{"reqId":"WMS-001-demo","docType":"test","mode":"replace","content":"# WMS-001-demo Test\\n\\n## 测试场景清单\\n- 待补充"}'

# Agent 专用上下文：推荐给 agent 继续需求工作，优先于全文读取；返回 phaseRuntime.currentPhasePrompt / entryChecks / phaseGaps
curl -sS 'http://localhost:7331/api/requirement/context?id=WMS-001-demo&for=agent&intent=self-test&budget=2000'

# 低 token 获取编辑计划：intent 可用 overview/clarification/status/progress/branch/self-test/release-check/experience-summary/design/config/review
curl -sS 'http://localhost:7331/api/requirement/edit-plan?id=WMS-001-demo&intent=self-test'

# 低 token 获取指定 token 内容：默认按 intent 选择文件，也可加 tokens=req.meta,req.test
curl -sS 'http://localhost:7331/api/requirement/context?id=WMS-001-demo&intent=self-test&budget=2000'

# 记录结构化事件：写 events.jsonl，默认同步追加 notes.md
curl -sS -H 'Content-Type: application/json' \
  -X POST http://localhost:7331/api/requirement/events \
  -d '{"reqId":"WMS-001-demo","type":"rootCause","summary":"boxCode 不匹配导致 cancel 库存扣减失败","evidence":["JHZC-01 有库存但 boxCode 条件不匹配"],"decisions":["先用备份字段恢复运单号"],"idempotencyKey":"WMS-001-demo-boxcode-root-cause"}'

# 语义章节 upsert：agent 不必记 impact.md/test.md 文件名
curl -sS -H 'Content-Type: application/json' \
  -X POST http://localhost:7331/api/requirement/sections/boxCodeIssue \
  -d '{"reqId":"WMS-001-demo","content":"- 问题描述...\n- 根因...\n- 治标方案..."}'

# 统一编辑入口：追加进展，不让 agent 自己拼 notes.md 路径
curl -sS -H 'Content-Type: application/json' \
  -X POST http://localhost:7331/api/requirement/edit \
  -d '{"reqId":"WMS-001-demo","operation":"appendNote","title":"进展","text":"完成方案梳理。"}'

# 统一编辑入口：按标题 upsert 文档 section，避免整篇覆盖
curl -sS -H 'Content-Type: application/json' \
  -X POST http://localhost:7331/api/requirement/edit \
  -d '{"reqId":"WMS-001-demo","operation":"upsertSection","token":"req.test","heading":"自测证据","content":"- Kibana tid=... 验证通过"}'

# 统一编辑入口：状态仍由 state.json 管理
curl -sS -H 'Content-Type: application/json' \
  -X POST http://localhost:7331/api/requirement/edit \
  -d '{"reqId":"WMS-001-demo","operation":"setStatus","status":"自测中","note":"开发自测开始"}'

# 查询代码审查门禁；推进到测试中前必须 PASS 或 WAIVED
curl -sS 'http://localhost:7331/api/requirement/review-gate?id=WMS-001-demo'

# 校验需求结构
curl -sS -H 'Content-Type: application/json' \
  -X POST http://localhost:7331/api/requirement/validate \
  -d '{"reqId":"WMS-001-demo"}'

# 查询前端/后端可选合并分支与默认选中值
curl -sS 'http://localhost:7331/api/requirement/merge-options?id=WMS-001-demo'

# 按 branches.json 合并所选类型到指定环境分支；repoKind=frontend|backend
curl -sS -H 'Content-Type: application/json' \
  -X POST http://localhost:7331/api/requirement/merge-branch \
  -d '{"reqId":"WMS-001-demo","repoKind":"backend","targetBranch":"test","target":"test"}'

# 查询合并/冲突状态；返回值中的 worktreePath + conflictFiles 可直接交给 agent 或人工处理
curl -sS 'http://localhost:7331/api/requirement/merge-status?id=WMS-001-demo&target=test'
```

## 环境分支合并

需求详情页提供“前端分支 / 后端分支”两个选择框，后端按 `branches.json` 固定执行合并，减少 agent 自行推断和手写 git 步骤：

- 默认选中：除 `自测中` / `测试中` 外不默认选择；`自测中` 默认前端/后端都选 `test`；`测试中` 默认前端选 `master`、后端选最新 `UAT-*`。
- 前端下拉框：仅展示 `test` 和 `master`。
- 后端下拉框：仅展示 `test` 和最新的 `UAT-*` 分支。
- 未选择的前端/后端不会合并；提交时接口传 `repoKind=frontend|backend` 和 `targetBranch=<branch>`。
- PDA 客户端默认跳过，因为 PDA 不通过环境分支部署。
- 合并在仓库兄弟目录 `.agent-panel-merge-worktrees/<repo>/<target>/...` 中创建临时 worktree；成功推送后自动清理。
- 发生冲突时接口返回 `status=conflict`、`conflictFiles`、`worktreePath` 并保留 worktree。人工可打开 `/requirement-merge?id=<req>` 查看冲突；agent 调用接口时可用同一结构化返回继续处理冲突。

## Agent 需求文件协议

推荐交互模型是“结构化事件流 + Markdown 展示层”：

- 事实、状态、证据、测试结果、决策、TODO 写入 `POST /api/requirement/events`，服务端保存 `events.jsonl` 并可同步追加 `notes.md`。
- 章节级文档更新使用 `POST /api/requirement/sections/{section}` 或 `/api/requirement/edit` 的 `upsertSection`。
- 自由说明仍可使用 `notes.md` / Markdown 文档，但 agent 默认先读 `context?for=agent` 的摘要上下文。
- 状态独有提示词不绑定 session 创建时刻；agent 每轮应重新读取 `context?for=agent`，以 `phaseRuntime.currentPhasePrompt` 作为当前导航。
- 状态跳转允许跨阶段推进；服务端会在 `state.json.history[].skippedStatuses` 和 `events.jsonl` 记录跳过阶段，并在 `phaseRuntime.phaseGaps` 暴露缺失 entry checks，作为风险提示而非自动回退。

Agent Panel 给需求文件提供稳定 token，agent 不需要记住真实文件名即可定位读写范围：

| Token | 文件 | 说明 | 推荐写法 |
| --- | --- | --- | --- |
| `req.meta` | `meta.md` | 标题、owner、project、ONES 等稳定身份信息 | `patchMeta` |
| `req.state` | `state.json` | 状态/类别和流转历史的唯一真源 | `setStatus` / `setCategory` |
| `req.background` | `background.md` | 面向开发/测试的业务背景文档：背景、目标、对象、现有行为、本次改变、关键规则 | `writeDoc` / `upsertSection` |
| `req.memory` | `memory.md` | 面向 agent 的短上下文摘要 | `writeDoc` / `upsertSection` |
| `req.branch` | `branch.md` | 人类可读分支说明 | `writeDoc` / `upsertSection` |
| `req.branchScope` | `branches.json` | 机器可读 repo/branch 范围 | 由分支扫描工具维护 |
| `req.configChanges` | `config-changes.md` | DB/Apollo/Nacos/RocketMQ 等配置明细 | `writeDoc` / `upsertSection` |
| `req.releaseManifest` | `release-manifest.md` | 上线清单：表、配置、Topic/Group、Job、接口、人工动作总览 | `writeDoc` / `upsertSection` |
| `req.impact` | `impact.md` | 影响面、核心链路风险、回滚方案 | `writeDoc` / `upsertSection` |
| `req.test` | `test.md` | 测试场景、自测/UAT 证据 | `writeDoc` / `upsertSection` |
| `req.notes` | `notes.md` | 追加型进展、决策和踩坑 | `appendNote` 优先 |
| `req.review` | `review.md` | 代码审查摘要 | `writeDoc` / `upsertSection` |
| `req.releaseCheck` | `release-check.md` | 发布预检、阻塞项、需关注项 | `writeDoc` / `upsertSection` |
| `req.experienceSummary` | `experience-summary.md` | 经验总结、知识/经验/skill 改进闭环 | `writeDoc` / `upsertSection` |
| `req.alignment` | `alignment.md` | 需求澄清、PRD 解读、范围、验收口径、待确认问题 | `writeDoc` / `upsertSection` |
| `req.codeReview` | `code-review.json` | 大体积生成结果，默认只读 | 显式 review intent 才读 |

推荐 agent 流程：

- **每轮刷新阶段上下文**：先调用 `GET /api/requirement/context?id=<req>&for=agent&intent=<intent>&budget=2000`；同一 session 从澄清推进到开发/测试时，以返回的 `phaseRuntime.currentPhasePrompt` 为准，不沿用 session 启动 prompt。
- **跳状态处理**：允许从任意状态跳到目标状态；进入新状态后执行 `phaseRuntime.entryChecks`，`phaseRuntime.phaseGaps.missingRequiredEntryChecks` 作为风险/待办记录，除代码审查等安全门禁外不阻塞当前任务。
- **需求澄清**：使用 `intent=clarification`，先查业务知识库/经验库，再初步调查代码，产出 `alignment.md`、`background.md`、`impact.md` 和 `memory.md`。
- **上线清单**：`release-manifest.md` 是贯穿全流程维护的发布资产总览，需求详情页常驻展示；开发、自测、发布前都要同步更新。
- **代码审查门禁**：不新增主状态；作为 `自测中 → 测试中` 的 gate。`review.md` 或 `code-review-ai.md` 需要明确写 `Review Gate: PASS` / `BLOCKED` / `WAIVED`，否则后端拒绝推进到 `测试中`。
- **经验总结**：使用 `intent=experience-summary`，把本次需求暴露的业务知识、经验和 skill 改进写入 `experience-summary.md`，并尽量落地到知识库/经验库/skill。

1. `GET /api/requirement/edit-plan?id=<req>&intent=<intent>` 获取读写计划。
2. `GET /api/requirement/context?id=<req>&intent=<intent>&budget=<n>` 获取压缩上下文。
3. `POST /api/requirement/edit` 执行结构化写入。
4. `POST /api/requirement/validate` 校验结构。

这样可以把“读哪个文件、改哪个文件、怎么改”固定到 API 协议里，减少 agent 全目录搜索和误写 Markdown 的概率。

## 数据约定

- 需求扫描目录来自 `~/.local/share/agent-panel/config.json` 的 `requirementScanRoots`。
- 每个扫描 root 下会查找 `.agents/req/` 和 `req/`。
- 需求目录以 `meta.md` 识别，`state.json` 管理状态和类别。
- 关联关系存储在 `~/.local/share/agent-panel/associations.json`。
- 新建 session 只生成命令，不再内嵌终端；生成的启动上下文只负责绑定需求和提示 agent 去实时刷新 `context?for=agent`，不再内嵌一次性的阶段提示词：

```bash
pi --session-id <uuid> --name '<需求标题>' --append-system-prompt @<ctx-file>
```

## 部署

```bash
./scripts/install-systemd.sh
```

更多见 [`docs/DEPLOYMENT.md`](./docs/DEPLOYMENT.md)。
