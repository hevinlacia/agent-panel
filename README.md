# agent-panel

本地常驻 Web 控制面，在浏览器里驱动本机的 coding agent session。当前以 **pi** 为主力
harness，同时保留 OpenCode 兼容代码（后续逐步清理）。

右上角 **OC / PI** 开关切换全局 harness：session 列表、终端 spawn、需求「新建并绑定
session」都按当前 harness 分派。

## 技术栈

- **Hono** + **@hono/node-server**（HTTP + WebSocket）
- **hono/jsx** SSR（无 Vite / React 构建链）
- **tsx** 直接运行 TypeScript
- **node-pty** 承载 agent TUI 子进程
- **@xterm/xterm** + addon-fit 浏览器内嵌终端

## 快速开始

```bash
cd ~/Developer/playground/agent-panel
npm install
npm start
# -> Agent Panel running at http://localhost:7331
```

开发模式（文件变更自动重启）：`npm run dev`
类型检查：`npm run typecheck`

后台常驻（Linux + systemd user service）：

```bash
./scripts/install-systemd.sh
# -> 安装并启动 agent-panel.service，默认端口 7331
# -> PORT=8080 ./scripts/install-systemd.sh 可改端口
```

详细部署 / 升级 / 卸载 / 排障见 [`docs/DEPLOYMENT.md`](./docs/DEPLOYMENT.md)。

## Harness 模式

全局 `config.harness`（`"opencode" | "pi"`），右上角切换，持久化到
`~/.local/share/agent-panel/config.json`。切换后所有路由重新按 harness 读取数据 /
spawn 终端。

| 能力 | pi 模式（主力） | opencode 模式（保留） |
| --- | --- | --- |
| session 数据源 | `~/.pi/agent/sessions/*/*.jsonl`（`src/piSessions.ts`） | `~/.local/share/opencode/opencode.db` SQLite → CLI → fs（`src/sessions.ts`） |
| session id | UUID | `ses_<base62>` |
| 继续会话 | spawn `pi --session <uuid>` | spawn `opencode --session <id>` |
| 新建 session | 预生成 UUID + 立即关联 + 返回 `pi --session-id <uuid> --name "<标题>"`，**立即返回** | 后台 `opencode run` + 轮询 + 返回 `opencode -s <id>` |
| 需求关联 | UUID 写入 associations | `ses_` id 写入 associations |

> pi 新建 session 利用 `pi --session-id <id>` 的特性：指定的 id 会直接成为 session 的
> header id，所以 dashboard 能预先生成 id、预先关联，用户跑命令时 pi 才真正创建
> session，dashboard 扫描自动识别。

## 页面

| 路径 | 说明 |
| --- | --- |
| `/` `/projects` | **Projects / Requirements** 需求管理（Hermes `~/.agents/req/`，按项目分组） |
| `/requirement?id=<req>` | 需求详情：记忆 / 上线包 / 测试 / Review / 关联 session；「新建并绑定 session」按钮 |
| `/sessions` | **Sessions** 仪表盘（Operator 风格 lane），按 harness 列 session |
| `/session?id=<id>` | 单 session 详情 + 内嵌 xterm 终端 |
| `/api/sessions` `/api/session?id=` `/api/config` | JSON API |
| `/ws/session-terminal?id=<id>` | 终端 WebSocket 端点 |

OpenCode 专属页面（`/reports` `/report` `/schedulers` `/env-vars` 及对应 API）当前
保留，pi 模式下不使用，后续清理。

## pi session 数据源

`src/piSessions.ts` 扫描 `~/.pi/agent/sessions/<project-dir>/<timestamp>_<id>.jsonl`：

- **header 行**：`id`（UUID）、`cwd`、`timestamp`
- **`session_info` 行**：display name（`pi --name` 设的值，优先作标题）
- **`model_change` 行**：provider / modelId
- **首条 user message**：标题 fallback
- `updated` 取文件 mtime；`status` 由 recency 派生（<5m `running` / <24h `idle` / 否则 `stale`）

## 内嵌终端

- 服务端 `src/terminal.ts`：`node-pty` 按 harness spawn `pi --session` / `opencode --session`
- `/ws/session-terminal`（`src/server.tsx`）用 `upgradeWebSocket` 桥接 PTY stdin/stdout
- 客户端 `public/terminal.js` 加载 `/vendor/xterm/*`，WebSocket 双向通信，支持输入 / resize / 退出提示
- 新建模式：WS spawn 后轮询新 session，把真实 id 推回页面并关联需求

## 需求生命周期（Hermes，harness 无关）

需求目录在 `~/.agents/req/<project>/.../<req-id>/`，dashboard 只维护 session 关联
（`~/.local/share/agent-panel/associations.json`）和状态写入。关键文件：

- `memory.md` — 新建 session 的首要记忆入口
- `alignment.md` — 需求对齐阶段的标准业务说明
- `branch.md` + `config-changes.md` — 上线包（分支 / DB / Apollo / Nacos / RocketMQ）
- `test.md` / `review.md` / `impact.md` — 测试 / Review / 影响评估

## 目录结构

```
src/
  server.tsx              - Hono 路由、JSX 页面、WS upgrade、harness 分派
  config.ts               - AppConfig（含 harness 字段）+ env 管理
  piSessions.ts           - pi JSONL session 扫描器
  sessions.ts             - OpenCode SQLite/CLI/fs 扫描（保留，待清理）
  dashboardSessions.ts    - harness 门面：按 harness 分派扫描/校验/命令
  terminal.ts             - node-pty 包装，按 harness spawn
  terminalProtocol.ts     - 纯 WS 帧解析（无原生依赖）
  terminalUrl.ts          - 终端 WS URL + 自动注入门控
  requirements.ts         - Hermes 需求 + session 关联存储
  requirementState.ts     - 需求 state.json 读写
  paths.ts                - 路径安全边界
  navigation.ts           - 导航项
  notifications.ts        - 通知中心持久化
  # OpenCode 专属（保留，后续清理）：
  forkSalvage.ts experienceMarkers.ts experienceAutoSummary.ts
  sessionExtract.ts extractJobs.ts extractQueue.ts autoExtractScheduler.ts
  autoExtract.ts autoValuation.ts sessionValuation.ts sessionTranscript.ts
  opencodeProcessQueue.ts
public/
  harness-switch.js       - 右上角 OC/PI 切换
  terminal.js             - 内嵌终端客户端
  app.js / req-detail.js  - 报告页 / 需求页交互
  style.css               - Operator 暗色主题
```

## 验证

```bash
npm run typecheck   # tsc --noEmit，覆盖 src 与 tests
npm test            # node --test + tsx
npm start           # 打开 http://localhost:7331
```

## 安全约束

- 不读取任何 `.env*`、`credentials.json`、`secrets.json`、私钥文件。
- session id 按 harness 严格校验（pi: UUID；opencode: `^ses_[A-Za-z0-9]+$`），spawn 前再校验一次。
- 所有 CLI 调用用 `child_process.spawn` 固定 argv，不 shell-eval 用户输入。
- 路径拒绝 `..`；vendor / static 路由强制边界。

## 后续路线

`main` 分支聚焦 pi。OpenCode 专属代码（experience summary / extract context / SQLite
扫描 / fork 救回等）保留但不再维护，后续按 roadmap 逐步移除。完整双 harness 版本存档
在 `archive/dual-harness` tag，随时可找回。

## 继续开发 / AI handoff

1. [`AGENTS.md`](./AGENTS.md) — 项目规则、安全约束、验证清单。**先读这个再动手。**
2. [`docs/AI_DEVELOPMENT.md`](./docs/AI_DEVELOPMENT.md) — 长篇交接文档。
3. 本 README — 用户视角的功能 / 路由 / 目录导览。

> 注：`AGENTS.md` 与 `docs/` 的部分描述仍基于 OpenCode 单 harness 时期，尚未同步到
> pi 为主力的现状，后续会一并更新。

本 README 保持一页篇幅；详细设计放在 `AGENTS.md` 和 `docs/AI_DEVELOPMENT.md`。
