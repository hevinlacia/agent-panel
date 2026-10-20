# agent-panel 拆分方案：Hub（容器）/ Worker（本机）

> 用于：指导 agent-panel 从单二进制演进为「容器 hub + 本机 headless worker」两层架构。
> 触发词：拆分、panel-docs、worker、hub、容器部署、阶段一/二/三。
> 不适用：与本拆分无关的常规功能开发。

## 目标架构

```
┌─ LXC 容器：panel-docs（hub）──────────────┐
│ 状态/数据 SSOT：req、knowledge、任务记录     │
│ 人类 UI（文档管理全家）+ token 认证          │
│ agent 文档 API（读写/检索）                  │
│ 阶段二+：能力注册表 + 任务路由（Tailscale 直调）│
└──────────────────────────────────────────┘
         ↑ agent 调 API            ↑ 能力调用
┌────────┴────────┐    ┌───────────┴────────────┐
│ pi 会话 agent    │    │ panel-worker（headless） │
│ 文档一律走容器API │    │ Chrome/git 执行/造数/会话 │
└─────────────────┘    └────────────────────────┘
```

网络：全部走 Tailscale。worker 出站注册 + 心跳；hub 执行时按注册表 MagicDNS 地址直调。

## 二进制演进

采用**单二进制多模式**（同 crate 内符号全部可用，避免 lib/bin 拆分的 pub(crate) 可见性问题）：

| 阶段 | 模式 | 说明 |
|---|---|---|
| 阶段一 | `agent-panel`（默认 local，全功能不动）+ `agent-panel --mode docs`（hub） | docs 模式挂 hub 路由子集 + token 可选 + 0.0.0.0 |
| 阶段二 | `agent-panel --mode worker`（新增）；local 模式验证后退役 | worker 挂执行类路由 + 注册/心跳；hub 的 worker 类端点改代理 |
| 阶段三 | 同上 + WS 隧道（按需） | 执行面升级 WS 推流，注册面不变 |

`GET /api/mode`（免 token）返回 `{mode}`，前端据此裁剪菜单；`/api/config` 响应附带 mode。

## 路由归属清单（按 main.rs 完整路由表盘点）

### HUB 保留（文档/状态，容器版挂载）

- `/health`、`/api/dashboard/stats`
- 需求核心：`/api/requirements`、`/api/requirement`(GET/PATCH)、`update`、`schema`、`context`、`edit-plan`、`edit`、`events`、`sections/:section`、`notes`、`doc`、`doc-part`、`doc-parts`、`validate`、`status`、`compliance`、`status-flow`、`review-checklist`、`status-gate-detail`、`category`、`convert-issue`、`branch-registration`(GET/PUT)、`associate`、`dissociate`、`pending-session`、`by-session`、`attachments`、`effort-estimate`、`create-sub`
- 审查产物读取：`/api/requirement/code-review`(GET)、`review-gate`、`annotations`(GET/PUT)、`diff-snapshots`(GET)
- 发布登记（纯文件）：`/api/requirement/release-branch/create|list|mark-released`
- knowledge 全家：`/api/knowledge`、`/api/knowledge/item`、`/api/agent/knowledge/query`、`/api/agent/items`、`items/summary`、`items/full`、`items/section`
- 经验总结任务状态：`/api/experience-summary/jobs`(GET)、`jobs/complete`（agent 回写）
- 会话关联辅助（依赖 sessions 扫描，容器 session root 为空时自然降级为空结果）：`new-session`、`session-candidates`
- 配置：`/api/config`(GET/POST)（容器自己的 config.json）；`/api/config/skills` 降级为空列表
- `/api/notifications*`（stub）

### WORKER 迁移（本机执行，容器阶段一不挂或 503）

- git 执行类：`code-review`(POST 生成快照)、`code-review/incremental`(POST)、`review-materials`(POST)、`master-diff`(POST)、`sync-base`、`merge-options`*、`merge-branch`、`merge-status`*、`prod-mrs`、`sub/init-branches`、`sub/merge-to-parent`、`sub/sync-parent`*、`release-branch/merge-sub|sync-prod|prod-mr`
- git 只读辅助：`/api/git/file-lines`、`/api/git/blame`（阶段二 hub 经 worker 代理后，diff 页在容器 UI 仍可完整展示）
- pi 会话：`/api/sessions`、`/api/session`、`/api/session/log`、`/api/sessions/resolve`、`/api/requirement/branch-rounds`(POST 登记扫描)
- 经验总结派发：`/api/experience-summary/jobs/dispatch`、`jobs/retry`（`Command::new("pi")`）
- testdata/接口测试：`/api/capability/*`、`/api/capabilities`、`/api/testdata/*`、`/api/apitest/*`、`/api/agent/capabilities/query`
- 本机资源：`/api/pi-chat/open`、`/ws/pi-chat`、`/api/auth-sites*`、`/api/ones/*`、`/api/cainiao-mock/status`、`/api/pi-config*`、`/api/harness/*`

`*` 标记的端点需在实施时确认是纯文件还是需要 git（merge-options/merge-status/sync-parent/branch-rounds）。

## 模块归属

| 归属 | 模块 |
|---|---|
| 共享（两 bin 都编译） | `http` `util` `markdown` `config` `paths` `requirement_api` `requirement_index` `requirement_service` `requirement_context` `requirement_release` `requirement_sub` `attachments` `knowledge` `experience_summary` `sessions`（hub 降级依赖） `git_file_lines` `git_blame`（worker 用，编译共享） |
| Worker 为主 | `git_workflow` `browser_auth` `chrome_cookies` `ones` `ones_manhour`（档案读写 API 留 hub，push 准备留 worker） `capability` `api_catalog` `cainiao_mock` `pi_config` `pi_rpc` `harness` `dsh_client` |

## 阶段一实施步骤

1. `src/auth.rs`：token 中间件（`PANEL_AUTH_TOKEN` 环境变量；`Authorization: Bearer` / `X-Panel-Token` / `?token=`；`/health`、`/api/mode`、静态资源豁免；未配置 token 时不启用——本机模式不受影响）。
2. `src/main.rs`：路由组装抽成 `build_router(state, mode)`；`--mode docs` / `PANEL_MODE=docs` 启动 hub（PANEL_DATA_DIR 覆盖数据目录、PANEL_PORT 默认 8788、绑定 0.0.0.0、未设 token 拒绝启动）；WORKER 类端点挂显式 503 `{error: "requires local worker"}`；不 spawn cainiao mock / experience-summary 后台 loop。
3. 前端：`web/src/lib/api.ts` fetch 封装带 token（localStorage `panelToken`，401 时提示输入重试）；`App.tsx` 按 `/api/mode` 裁剪 worker 类页面菜单。
4. 验证：`cargo fmt --check && cargo check && cargo test && bun run typecheck && bun run build`。
5. **阶段 1.5（已落地）**：agent 远程文件编辑 API `GET/PUT /api/requirement/file`（路径白名单：顶层 .md + docs/<base>/<file>.md；meta.md/state.json 排除；sha1 乐观锁 409）——配合本机 SSHFS 只读挂载（`~/hub-data`，systemd user automount），形成「读走挂载原生工具、写走 hub API」的 agent 易用性方案。

## 阶段一部署与数据

- LXC 容器：2C/1GB/8GB rootfs，装 tailscaled（tun 设备）+ git；systemd 拉起 `panel-docs`（release 二进制 + `public/dashboard-react` 静态文件）。
- 数据：容器 clone workspace-meta 仓库（req/knowledge 路径天然在仓库内），hub `PANEL_DATA_DIR` 与 scan roots 指向容器内路径；容器内定时 commit + push（数据变更自动提交，本机定期 pull 作为只读镜像）。
- 本机 agent 调用点：req/knowledge skills 与脚本从 `localhost:PORT` 切到 `http://<hub-name>.<tailnet>.ts.net:8788` + token。

## 阶段二实施步骤（2026-10 已落地：单二进制 `--mode worker`，非独立 bin）

1. ~~`src/bin/panel_worker.rs`~~ → **`--mode worker` / `PANEL_MODE=worker`**（同二进制多模式，共享全部符号）：worker 挂全量路由（原 local 模式路由，local 退役）+ 注册/心跳：
   - 启动向 hub `POST /api/worker/register`（machineId、callbackBase、tags），30s 心跳续期（幂等 upsert，注册即心跳）。
   - hub 新增 `src/workers.rs`：注册表持久化（data_dir/workers.json）+ `GET /api/workers` + worker 代理中间件（`WORKER_PROXY_PATHS` 白名单 + `/api/auth-sites/*` 前缀，转发到最近心跳的在线 worker，失败 502，无在线 worker 落回 503 stub）。
2. hub 的 WORKER 类端点改为代理：白名单路径命中且有在线 worker → reqwest 原样转发（method/path/query/body/Content-Type，300s 超时）；无在线 worker → 503 stub。前端页面（sessions/testdata/diff/merge）不改，走代理即恢复。
3. 本机全功能 `local` 模式退役删除；systemd user service 托管 hub（docs）与 worker 两个进程（物料 `scripts/deploy/panel-worker.service`）。
4. 端到端验证：同机 hub(7331) + worker(7332)：注册落盘 workers.json、经 hub 代理调 sessions/review 类端点、需求 API 走 hub 本地处理。
5. 已知边界（阶段 2.5）：远端 hub 场景 worker 产物的写回（worker 只写本地 req 目录）需走 hub file API 回传；WS（/ws/pi-chat）不代理。

## 安全

- hub：token 认证；CORS 收紧为同源；容器零凭证。
- worker：接口 token；Tailscale ACL 只允许 hub 访问 worker 端口，workers 互不可见；worker 不监听真实局域网。
- 数据 git remote 用 deploy key（只写权限限定数据仓库）。
