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
- `GET /api/requirement?id=<req>`
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
