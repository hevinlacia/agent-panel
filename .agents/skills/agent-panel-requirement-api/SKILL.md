---
name: agent-panel-requirement-api
description: 通过本机 Agent Panel API 查询需求、更新状态、关联 session、检查服务，避免误找接口。
allowed-tools: ["bash", "read", "write", "edit", "get_session_info"]
---

# Agent Panel Requirement API

用于：通过本机 Agent Panel API 操作 `~/.agents/req/` 需求，包括查询需求、更新状态、关联 session 和确认服务状态。

适用：
- 用户说“把需求状态改成/推进到 <状态>”“推到待上线”“状态更新接口在哪”
- 用户要求查询 Agent Panel 需求、按状态筛选需求、确认需求是否存在
- 用户要求把当前 session 关联到某个需求，且需要明确 API 调用细节
- 其他 skill 需要调用需求 API，避免临时翻源码找路由

不适用：
- 创建或重写需求文件模板（用 `req-create`）
- 批量发布前检查具体分支/配置/测试证据（用 `req-release-check`）
- 直接修改 `state.json`、`meta.md` 的状态字段或真实业务代码

## Trigger

- “更新需求状态” / “推进状态” / “设为待上线” / “设为已完成”
- “查 Agent Panel 需求 API” / “requirements API” / “需求状态接口”
- “把 session 关联到需求”且需要 API 路径或自动执行
- agent 已知要调用本机需求 API，不应再搜索项目源码

## 术语和边界

- 产品/页面统一称 **Agent Panel**。
- 本机默认地址：`http://localhost:7331`，可由 `PORT` 覆盖。
- systemd unit 历史名称仍是 `opencode-dashboard.service`；只在服务检查命令中使用这个名字，不把它当产品名。
- API 契约以本 skill 为准；除非接口返回 404/400 且怀疑版本不一致，否则不要先去翻 `src/server.tsx`。

## API Contract

| 用途 | 方法 | 端点 | 说明 |
|---|---|---|---|
| 列需求 | GET | `/api/requirements` | 返回 `{ requirements: [...] }`，客户端自行按 `status` 过滤 |
| 更新状态 | POST | `/api/requirement/status` | form body: `reqId`, `status`, 可选 `note`, `redirect` |
| 关联 session | POST | `/api/requirement/associate` | form body: `reqId`, `sessionId`，成功默认 303 |

状态值必须严格匹配：

```text
需求对齐 / 方案设计 / 开发中 / 自测中 / 测试中 / 待上线 / 已完成
```

## Workflow

### 1. 检查 Agent Panel 是否运行

```bash
curl -sf --max-time 3 http://localhost:7331/api/requirements >/dev/null \
  && echo OK || echo AGENT_PANEL_DOWN
```

如果不可用，只做服务状态检查，不读日志中的 secret-like 内容：

```bash
systemctl --user status opencode-dashboard.service --no-pager
journalctl --user -u opencode-dashboard.service -n 80 --no-pager
```

### 2. 解析或匹配需求 ID

若用户给的是完整 req-id，直接使用；若是缩写或标题关键词，先拉取需求列表并匹配：

```bash
curl -sf http://localhost:7331/api/requirements | python3 -c '
import json, sys
kw = sys.argv[1].lower()
data = json.load(sys.stdin)
for r in data.get("requirements", []):
    text = " ".join(str(r.get(k, "")) for k in ("id", "title", "project", "status")).lower()
    if kw in text:
        print(f"{r.get('id')}\t{r.get('status')}\t{r.get('project')}\t{r.get('title')}")
' '<keyword>'
```

- 1 个匹配：直接执行。
- 多个匹配：列出候选，让用户选择。
- 0 个匹配：停止，不猜 req-id。

### 3. 更新需求状态

Agent 调用时优先要求 JSON，避免把 303 redirect 当失败：

```bash
curl -sS -H 'Accept: application/json' \
  -X POST http://localhost:7331/api/requirement/status \
  -d "reqId=<req-id>" \
  -d "status=<新状态>" \
  -d "note=<备注，可选>"
```

成功返回类似：

```json
{"ok":true,"status":"待上线"}
```

若不用 JSON header，成功可能是 `303`，不要用 `curl -f` 误判。

### 4. 关联当前 session

优先用 `get_session_info` 取当前 session id；拿不到时才使用已有脚本或让用户提供。执行：

```bash
curl -s -o /dev/null -w "%{http_code}" \
  -X POST http://localhost:7331/api/requirement/associate \
  -d "reqId=<req-id>" \
  -d "sessionId=<session-id>"
```

`303` 表示成功；`400` 检查参数和 harness；`404` 表示需求不存在。

### 5. release-check 配合规则

当用户同时要求“生成 release-check.md 并推进到待上线”：

1. 先按 `req-release-check` 生成或刷新对应需求的 `release-check.md`。
2. 再调用 `/api/requirement/status` 设置为 `待上线`。
3. 最后重新 GET `/api/requirements` 验证该需求状态已经变化。

若用户只要求状态更新，不要顺手生成 release-check.md。

## Required Checks

- 不直接编辑 `state.json` 或 `meta.md` 的状态字段。
- `status` 必须是 7 个合法状态之一。
- `GET /api/requirements` 不保证服务端按 query 过滤；按状态筛选时必须客户端过滤。
- 状态更新优先使用 `Accept: application/json`；不用 `curl -f` 判断 303。
- Agent Panel 不可用时先检查 systemd unit，不要花很久搜索源码。

## Final Response

```text
✅ Agent Panel 需求状态已更新
- 需求: <title>（<req-id>）
- 状态: <旧状态> → <新状态>
- 备注: <note 或 无>
- 验证: GET /api/requirements 已确认
```
