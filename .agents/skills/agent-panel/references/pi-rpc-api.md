# pi RPC 会话管理 API（远程 agent-runner）

用于：查看/管理 agent-runner（CT107）上运行中的 `pi --mode rpc` 会话——进程清单、内存占用、停止会话、并行上限。配套 Phase 2 远程 spawn：hub 按 `PANEL_PI_REMOTE_HOST` 经 ssh 在 agent-runner 上拉起 pi。

不适用：历史 session 浏览（`/api/sessions` 只读本机/当前 session root）；WS 协议本身（见 pi docs/rpc.md）。

## 配置（hub 进程环境变量）

| 变量 | 说明 | 默认 |
|---|---|---|
| `PANEL_PI_REMOTE_HOST` | ssh 目标（`user@host`）。设置后 `/ws/pi-chat` 的 pi 经 ssh 远程 spawn | 未设置 = 本机 |
| `PANEL_PI_REMOTE_CWD_MAP` | cwd 映射 `工作站前缀=远程前缀,...`，最长前缀优先 | 无 |
| `PANEL_PI_REMOTE_DEFAULT_CWD` | 无映射匹配时的远程回退 cwd | `/root/Developer` |
| `PANEL_PI_MAX_RPC_SESSIONS` | panel 管理的最大并行会话数 | `8` |

## GET /api/pi/rpc-sessions

运行中会话清单 + 宿主内存快照。响应：

```json
{
  "remote": true,
  "host": "root@192.168.6.137",
  "maxSessions": 8,
  "registry": [{"sessionId": "...", "running": true, "source": "panel"}],
  "processes": [{"pid": 123, "sessionId": "...", "managed": true, "rssKb": 409600, "rssMb": 400, "etime": "1h2m3s", "args": "..."}],
  "totalRssMb": 400,
  "memory": {"totalMb": 4096, "usedMb": 800}
}
```

- `processes` 来自宿主 `ps` 扫描（含 panel 外部拉起的会话，`managed=false`）
- `managed=true` 表示该会话由 panel 当前注册表持有

## POST /api/pi/rpc-sessions/stop

请求：`{"sessionId": "<uuid>"}`。行为：SIGTERM → 1.5s 后仍存活则 SIGKILL，并清理 panel 注册表残留。session JSONL 历史保留，之后可重新 attach 恢复上下文。无活进程时返回 `{"stopped": false, ...}`（幂等）。

## 并行上限

`/ws/pi-chat` attach 时若 panel 注册表运行数已达 `PANEL_PI_MAX_RPC_SESSIONS`，spawn 拒绝，前端收到 `panel_error` 事件（原因文案）。宿主上外部拉起的进程不计入上限，但会出现在清单里可手动停止。

## 前端入口

Sessions 页顶部「pi RPC 运行中会话」卡片：5s 轮询进程清单/内存/上限，行内停止按钮。
