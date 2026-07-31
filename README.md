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

## 页面

| 路径 | 说明 |
| --- | --- |
| `/` `/dashboard` | 需求 KPI、状态分布、交付周期 |
| `/projects` | 需求进度看板 |
| `/requirement?id=<req>` | 需求详情、状态/类别/ONES、关联 session、新 pi session 命令 |
| `/sessions` | pi session 列表 |
| `/session?id=<uuid>` | pi session 元数据详情（无 terminal） |
| `/settings` | 需求扫描目录、模型偏好、Pi `settings.json` 编辑 |

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
- `POST /api/requirement/edit` — 统一结构化编辑入口，支持 `setStatus`、`setCategory`、`patchMeta`、`appendNote`、`writeDoc`、`upsertSection`
- `POST /api/requirement/notes` — 追加 `notes.md` 进展块，不覆盖原文
- `PUT|POST /api/requirement/doc` — 写入受控文档，`docType=background|memory|branch|config-changes|impact|test|notes|review|alignment|prd`，`mode=replace|append`
- `POST /api/requirement/validate` — 校验 `meta.md`、`state.json`、标准文件和 `branches.json` 基本结构
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

# 低 token 获取编辑计划：intent 可用 overview/status/progress/branch/self-test/release-check/design/config/review
curl -sS 'http://localhost:7331/api/requirement/edit-plan?id=WMS-001-demo&intent=self-test'

# 低 token 获取上下文：默认按 intent 选择文件，也可加 tokens=req.meta,req.test
curl -sS 'http://localhost:7331/api/requirement/context?id=WMS-001-demo&intent=self-test&budget=2000'

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

# 校验需求结构
curl -sS -H 'Content-Type: application/json' \
  -X POST http://localhost:7331/api/requirement/validate \
  -d '{"reqId":"WMS-001-demo"}'
```

## Agent 需求文件协议

Agent Panel 给需求文件提供稳定 token，agent 不需要记住真实文件名即可定位读写范围：

| Token | 文件 | 说明 | 推荐写法 |
| --- | --- | --- | --- |
| `req.meta` | `meta.md` | 标题、owner、project、ONES 等稳定身份信息 | `patchMeta` |
| `req.state` | `state.json` | 状态/类别和流转历史的唯一真源 | `setStatus` / `setCategory` |
| `req.background` | `background.md` | 背景、目标、范围、关键决策 | `writeDoc` / `upsertSection` |
| `req.memory` | `memory.md` | 面向 agent 的短上下文摘要 | `writeDoc` / `upsertSection` |
| `req.branch` | `branch.md` | 人类可读分支说明 | `writeDoc` / `upsertSection` |
| `req.branchScope` | `branches.json` | 机器可读 repo/branch 范围 | 由分支扫描工具维护 |
| `req.configChanges` | `config-changes.md` | DB/Apollo/Nacos/RocketMQ 等配置变更 | `writeDoc` / `upsertSection` |
| `req.impact` | `impact.md` | 影响面、核心链路风险、回滚方案 | `writeDoc` / `upsertSection` |
| `req.test` | `test.md` | 测试场景、自测/UAT 证据 | `writeDoc` / `upsertSection` |
| `req.notes` | `notes.md` | 追加型进展、决策和踩坑 | `appendNote` 优先 |
| `req.review` | `review.md` | 代码审查摘要 | `writeDoc` / `upsertSection` |
| `req.codeReview` | `code-review.json` | 大体积生成结果，默认只读 | 显式 review intent 才读 |

推荐 agent 流程：

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
- 新建 session 只生成命令，不再内嵌终端：

```bash
pi --session-id <uuid> --name '<需求标题>' --append-system-prompt @<ctx-file>
```

## 部署

```bash
./scripts/install-systemd.sh
```

更多见 [`docs/DEPLOYMENT.md`](./docs/DEPLOYMENT.md)。
