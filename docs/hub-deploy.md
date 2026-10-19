# Hub（docs 模式）容器部署指南

> 用于：把 agent-panel 以 `--mode docs` 部署进 LXC 容器，作为需求/知识文档 SSOT 与 agent 文档 API。
> 触发词：hub 部署、容器部署、docs 模式、PANEL_AUTH_TOKEN、LXC、tailscale。
> 不适用：本机 local 模式运行、阶段二 worker 部署。

## 0. 前置

- 本机构建产物：`cargo build --release` 后的 `target/release/agent-panel` + `public/dashboard-react/`（`bun run build:dashboard`）。
- 容器规格建议：2C / 1GB / 8GB rootfs（数据仓库另计，req/knowledge 当前量级 <2GB）。
- 网络约定：全部走 Tailscale；hub 仅在 tailnet 内可访问。

## 1. 创建 LXC 容器（incus/lxd 为例）

```bash
incus launch ubuntu:24.04 agent-panel-hub \
  -c limits.cpu=2 -c limits.memory=1GiB -d root,size=8GiB
# tailscaled 需要 /dev/net/tun：
incus config device add agent-panel-hub tun unix-char source=/dev/net/tun path=/dev/net/tun
incus exec agent-panel-hub -- bash
```

PVE（LXC）等价：创建 CT 时勾选「无特权的容器」，Options → Device passthrough 加 `/dev/net/tun`（或Features: nesting + tun）。

## 2. 容器内基础环境

```bash
apt update && apt install -y git curl
curl -fsSL https://tailscale.com/install.sh | sh
tailscale up --hostname=agent-panel-hub --accept-dns=false   # 记下 100.x IP / MagicDNS 名
```

## 3. 部署产物（本机执行）

```bash
HUB=agent-panel-hub.<tailnet>.ts.net   # 换成实际 MagicDNS 名
scp target/release/agent-panel "$HUB:/opt/agent-panel/agent-panel"
ssh "$HUB" "mkdir -p /opt/agent-panel/public /srv/agent-panel/state"
scp -r public/dashboard-react "$HUB:/opt/agent-panel/public/"
scp scripts/deploy/hub-agent-panel.service "$HUB:/etc/systemd/system/agent-panel.service"
scp scripts/deploy/hub-data-autocommit.sh "$HUB:/usr/local/bin/"
ssh "$HUB" "chmod +x /usr/local/bin/hub-data-autocommit.sh /opt/agent-panel/agent-panel"
```

## 4. 数据初始化（容器内）

数据仓库 = workspace-meta 的独立 clone（req/knowledge 天然在仓库内）：

```bash
mkdir -p /srv/agent-panel/data
git clone git@github.com:hevinlacia/Developer.git /srv/agent-panel/data   # 建议改用 deploy key
# 迁移首次数据：从本机把 req/knowledge 当前内容覆盖进 clone（rsync/tar），见下节
```

本机导出（一次性）：

```bash
rsync -a ~/Developer/.agents/req/        "$HUB:/srv/agent-panel/data/.agents/req/"
rsync -a ~/Developer/.agents/knowledge/  "$HUB:/srv/agent-panel/data/.agents/knowledge/"
rsync -a ~/Developer/company/云仓/WMS/.agents/req/ "$HUB:/srv/agent-panel/data/company/云仓/WMS/.agents/req/"
```

容器内首次提交并配置自动快照（systemd timer，每 15 分钟）：

```bash
cd /srv/agent-panel/data && git add -A && git -c user.name=hub -c user.email=hub@localhost commit -m "data: initial import" && git push
cat > /etc/systemd/system/hub-data-autocommit.service <<'EOF'
[Unit]
Description=Agent Panel hub data auto snapshot
[Service]
Type=oneshot
ExecStart=/usr/local/bin/hub-data-autocommit.sh /srv/agent-panel/data
EOF
cat > /etc/systemd/system/hub-data-autocommit.timer <<'EOF'
[Unit]
Description=Snapshot hub data every 15 minutes
[Timer]
OnBootSec=10min
OnUnitActiveSec=15min
[Install]
WantedBy=timers.target
EOF
systemctl daemon-reload && systemctl enable --now hub-data-autocommit.timer
```

## 5. 启动服务（容器内）

```bash
# 编辑 unit：把 __SET_ME__ 换成 openssl rand -hex 24 生成的 token；记录该 token
vim /etc/systemd/system/agent-panel.service
systemctl daemon-reload && systemctl enable --now agent-panel
curl -s http://127.0.0.1:8788/health && curl -s http://127.0.0.1:8788/api/mode
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

knowledge 目录通过 `PANEL_DATA_DIR` 同级约定或 config 对应字段指向 `/srv/agent-panel/data/.agents/knowledge`（以 config.rs 实际字段名为准，设置页可改）。

## 6. Tailscale ACL（tailnet 管理页）

```jsonc
{
  "acls": [
    // hub ↔ 所有开发机 worker（阶段二）；hub 对外仅 8788
    { "action": "accept", "src": ["group:dev"], "dst": ["agent-panel-hub:8788"] },
    { "action": "accept", "src": ["agent-panel-hub"], "dst": ["group:dev:8787"] }
  ]
}
```

阶段一先只放第一条；workers 之间默认不可见。

## 7. 本机 agent 切换调用面

- skills/脚本里 agent-panel 地址：`http://localhost:7331` → `http://agent-panel-hub.<tailnet>.ts.net:8788`
- 携带 token：请求头 `X-Panel-Token: <token>`（或 `Authorization: Bearer`）
- 本机 `~/.pi/agent/AGENTS.md`、WMS req skills 中「直接读写 `.agents/req/`」的约定改为「结构化写走 hub API」；本机 req 目录退化为只读镜像（定期 `git -C ~/Developer pull`）。

## 8. 验收清单

- [ ] 浏览器（任意 tailnet 设备）打开 `http://agent-panel-hub:8788`，输入 token 后可看需求看板/知识页
- [ ] `GET /api/requirements` 带 token 返回迁移后的需求列表
- [ ] agent 通过 API 创建测试需求成功，容器内 15 分钟内出现 auto snapshot commit
- [ ] worker 类端点（如 `/api/sessions`）返回 503 `requires local worker`
- [ ] 本机 `git -C ~/Developer pull` 能拉到容器提交的数据变更
