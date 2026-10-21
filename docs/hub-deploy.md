# Hub（docs 模式）容器部署指南

> 用于：把 agent-panel 以 `--mode docs` 部署进 LXC 容器，作为需求/知识文档 SSOT 与 agent 文档 API；以及日常发版更新与回滚。
> 触发词：hub 部署、容器部署、docs 模式、日常发版、部署更新、回滚、PANEL_AUTH_TOKEN、LXC、tailscale。
> 不适用：worker 首次安装（本机 dev 已装，见 §3 增量更新）。

## 0. 前置与产物

- 构建命令：`bun run build`（= `vite build` + `cargo build --release`），一次产出两份产物：
  - 后端二进制 `target/release/agent-panel`
  - 前端静态资源 `public/dashboard-react/`
- **产物必须来自已提交的 commit**：按 worktree 规范在 `worktree/<repo>-<slug>` 分支构建，避免部署半成品工作区。
- 只改后端时可不传 `dashboard-react`；改了 `web/src/**` 必须同步传静态资源（§2.3）。
- 容器规格：2C / 1GB / 8GB rootfs（现网 `agent-panel.tail260d93.ts.net`，数据仓库另计 <2GB）。
- 网络约定：全部走 Tailscale；hub 仅在 tailnet 内可访问。

## 1. 首次安装（新容器）

### 1.1 容器创建与基础环境

```bash
incus launch ubuntu:24.04 agent-panel-hub \
  -c limits.cpu=2 -c limits.memory=1GiB -d root,size=8GiB
# tailscaled 需要 /dev/net/tun：
incus config device add agent-panel-hub tun unix-char source=/dev/net/tun path=/dev/net/tun
incus exec agent-panel-hub -- bash
```

PVE（LXC）等价：创建 CT 时勾选「无特权的容器」，Options → Device passthrough 加 `/dev/net/tun`。

```bash
apt update && apt install -y git curl
curl -fsSL https://tailscale.com/install.sh | sh
tailscale up --hostname=agent-panel-hub --accept-dns=false   # 记下 100.x IP / MagicDNS 名
```

### 1.2 部署产物（本机执行）

> ⚠️ **正在运行的二进制不能直接 scp 覆盖**（目标文件 ETXTBSY，`scp` 报 `dest open Failure`）。
> 一律先传 `.new` 再 `mv` 原子替换（rename 覆盖运行中文件是允许的）。日常发版同此规则。

```bash
HUB=root@agent-panel.tail260d93.ts.net
ssh $HUB "mkdir -p /opt/agent-panel/public /srv/agent-panel/state"
scp target/release/agent-panel          $HUB:/opt/agent-panel/agent-panel.new
scp -r public/dashboard-react           $HUB:/opt/agent-panel/public/
scp scripts/deploy/hub-agent-panel.service $HUB:/etc/systemd/system/agent-panel.service
scp scripts/deploy/hub-data-autocommit.sh  $HUB:/usr/local/bin/
ssh $HUB "mv /opt/agent-panel/agent-panel.new /opt/agent-panel/agent-panel \
          && chmod +x /opt/agent-panel/agent-panel /usr/local/bin/hub-data-autocommit.sh"
```

### 1.3 数据仓库初始化（容器内）

数据 SSOT = 独立仓库 `hevinlacia/agent-panel-data`（2026-10-20 从 workspace-meta 拆出；req / business-knowledge / experiences 天然在仓库内）：

```bash
mkdir -p /srv/agent-panel/data
git clone git@github.com:hevinlacia/agent-panel-data.git /srv/agent-panel/data   # 建议配 deploy key（只读 push 权限按需）
cd /srv/agent-panel/data && git -c user.name=hub -c user.email=hub@localhost commit --allow-empty -m "data: hub init" && git push
```

> 历史备注：workspace-meta → agent-panel-data 的一次性数据迁移（2026-10-20）已完成，本节不再需要迁移步骤；本机 `~/Developer/agent-panel-data` 为只读镜像，靠手动 `git pull` 追平（本机**没有**自动 pull，读镜像前先确认新鲜度）。

配置自动快照（systemd timer，每 15 分钟 pull --rebase + commit + push）：

```bash
systemctl enable --now hub-data-autocommit.timer
systemctl list-timers | grep autocommit   # 确认排期
```

### 1.4 启动服务（容器内）

`scripts/deploy/hub-agent-panel.service` 已带安全加固（NoNewPrivileges）与默认环境；token 鉴权默认关闭（tailnet 内网），如需暴露更开放网络再启用：

```bash
# 可选加固：取消注释 Environment=PANEL_AUTH_TOKEN 并填 openssl rand -hex 24 生成的值
vim /etc/systemd/system/agent-panel.service
systemctl daemon-reload && systemctl enable --now agent-panel
curl -s http://127.0.0.1:8788/health && echo && curl -s http://127.0.0.1:8788/api/mode   # {"mode":"docs"}
```

hub 的 scan roots 配置（`/srv/agent-panel/state/config.json`）：

```json
{
  "requirementScanRoots": [
    "/srv/agent-panel/data/.agents/req",
    "/srv/agent-panel/data/company/云仓/WMS/.agents/req"
  ]
}
```

knowledge 扫描根由 `PANEL_DATA_DIR`（`/srv/agent-panel/state`）同级约定定位 `/srv/agent-panel/data` 下的 `.agents/{business-knowledge,experiences}` 与 `company/云仓/WMS/.agents/{business-knowledge,experiences}`；设置页可核对。

### 1.5 Tailscale ACL（tailnet 管理页）

```jsonc
{
  "acls": [
    // 所有开发机 → hub:8788（agent 调用面 + 浏览器 UI）
    { "action": "accept", "src": ["group:dev"], "dst": ["agent-panel-hub:8788"] }
  ]
}
```

现状：hub → worker 回程走 **ssh 反向隧道**（`panel-worker-tunnel.service` 把本机 :7332 映射为 hub 侧 `127.0.0.1:7332`），不需要 ACL 放行；tailnet ACL 放行 hub→dev:7332 后可把 worker `PANEL_CALLBACK_BASE` 改为 Tailscale 直连并撤隧道（见 `docs/split-plan.md` 阶段三）。

## 2. 日常发版（增量更新）

标准流程（2026-10-21 实测）：

```bash
cd <worktree or repo>          # 产物来自已提交 commit
bun run typecheck && cargo test && bun run build   # 验证 + 构建

HUB=root@agent-panel.tail260d93.ts.net
ssh $HUB "cp /opt/agent-panel/agent-panel /opt/agent-panel/agent-panel.bak-$(date +%m%d)"
scp target/release/agent-panel $HUB:/opt/agent-panel/agent-panel.new
ssh $HUB "mv /opt/agent-panel/agent-panel.new /opt/agent-panel/agent-panel"
# 改了 web/src/** 时才需要：
scp -r public/dashboard-react  $HUB:/opt/agent-panel/public/
ssh $HUB "systemctl restart agent-panel && sleep 1 && systemctl is-active agent-panel \
          && curl -sf http://127.0.0.1:8788/health"
```

**发版后回归**（按本次改动面挑接口抽查）：

- `GET /health`、`GET /api/mode` → `{"mode":"docs"}`
- 改动面接口抽查：如知识 API 改动后验证 `POST /api/agent/knowledge/query` 响应键为 `items`、`GET /api/knowledge/index` 无 `sourcePath`
- 前端改动：浏览器强刷确认 bundle 更新

**回滚**（线上异常时）：

```bash
ssh $HUB "mv /opt/agent-panel/agent-panel /opt/agent-panel/agent-panel.bad \
          && mv /opt/agent-panel/agent-panel.bak-<date> /opt/agent-panel/agent-panel \
          && systemctl restart agent-panel && curl -sf http://127.0.0.1:8788/health"
```

## 3. Worker 侧更新（开发机）

worker 与 hub 同仓库同二进制；worker 端点不被 hub 代理（白名单仅 `/api/auth-sites/*`），版本短暂落后无害，但建议发版后顺手刷新：

```bash
git -C ~/Developer/tools/agent-panel pull
cd ~/Developer/tools/agent-panel && bun run build:backend
systemctl --user restart panel-worker
# 心跳确认：hub 侧 workers.json 在线
curl -s http://agent-panel.tail260d93.ts.net:8788/api/workers | python3 -m json.tool | grep -E "machineId|online"
```

> 注意 `scripts/deploy/panel-worker.service` 是首次安装模板（`PANEL_HUB_URL` 需改成 hub 实际地址）；本机已装单元以 `systemctl --user cat panel-worker` 为准。

## 4. 验收清单

- [ ] 浏览器（任意 tailnet 设备）打开 `http://agent-panel-hub:8788`，可看需求看板/知识页
- [ ] `GET /api/requirements` 返回需求列表；`GET /api/knowledge/index` 返回 `items` 全量索引
- [ ] agent 通过 API 创建测试需求成功，容器内 15 分钟内出现 auto snapshot commit
- [ ] worker 类端点（如 `/api/sessions`）无在线 worker 时返回 503 `requires local worker`
- [ ] `GET /api/workers` 显示开发机 worker `online:true`
- [ ] 本机 `git -C ~/Developer/agent-panel-data pull` 能拉到容器提交的数据变更

## 5. 拓扑现状与历史备注

- **调用面**：agent/脚本统一走 `http://agent-panel.tail260d93.ts.net:8788`（`AGENT_PANEL_URL` 可覆盖）。
- **本机 local 模式已退役**（`agent-panel.service` :7331，2026-10-20）：执行能力由 `panel-worker`（:7332）承担，hub 经 ssh 隧道回程调用；hub 不可达时无本地兜底面板，agent 读写走本机只读镜像 + 恢复后追平。
- **数据 SSOT**：`hevinlacia/agent-panel-data`（容器内 clone + 15min autocommit）；本机 `~/Developer/agent-panel-data` 只读镜像，**无自动 pull**，读前手动 `git pull`。
- **worker 注册**：`data_dir/workers.json`（hub 持有），worker 每 30s `POST /api/worker/register` 幂等 upsert + 心跳，90s 无心跳判 offline。
