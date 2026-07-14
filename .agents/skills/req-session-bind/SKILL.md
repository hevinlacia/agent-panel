---
name: req-session-bind
description: 在 pi session 中用自然语言将当前 session 关联到 Agent Panel 需求，并获取需求上下文。
allowed-tools: ["bash", "read", "glob", "grep", "get_session_info"]
---

# Requirement Session Bind

用于：在任意 pi session 中，把当前 session 关联到 Agent Panel 上的某个需求，并确认关联结果。

适用：
- 用户说“这个 session 归属到 WMS-001”或“把这个 session 关联到需求 XXX”
- 用户完成任务后想沉淀 session 到需求记录
- 用户想查看当前 session 已关联的需求

不适用：
- 创建新需求（用 `req-create`）
- 修改需求状态、查询状态更新接口或排查 API 路由（用 `agent-panel-requirement-api`）
- 批量上线检查或生成 `release-check.md`（用 `req-release-check`）

## Trigger

- “这个 session 归属到 / 关联到 / 绑定到 <需求>”
- “bind this session to <requirement>”
- “这个 session 属于 <需求>”
- “把这个任务记到 <需求> 名下”
- “查看这个 session 关联了哪个需求”

## Agent Panel API

Agent Panel 默认运行在 `http://localhost:7331`（可通过 `PORT` 环境变量覆盖）。它支持 pi / opencode harness：pi harness 下 session id 为 UUID，opencode harness 下通常为 `ses_xxx`。

| 端点 | 方法 | 用途 |
|---|---|---|
| `/api/requirements` | GET | 列出所有需求（含 id、title、status、project、sessionIds） |
| `/api/sessions` | GET | 列出最近 session（仅作为 fallback） |
| `/api/requirement/associate` | POST | 关联 session 到需求，body: `reqId=xxx&sessionId=xxx` |

状态更新接口由 `agent-panel-requirement-api` 维护；本 skill 不重复执行状态推进。

## 流程

### 1. 确认 Agent Panel 在运行

```bash
curl -sf --max-time 3 http://localhost:7331/api/requirements >/dev/null 2>&1 \
  && echo "OK" || echo "AGENT_PANEL_DOWN"
```

如果不可用，告知用户 Agent Panel 未运行，并检查 systemd unit：

```bash
systemctl --user status opencode-dashboard.service --no-pager
```

### 2. 找到当前 Session ID

优先调用 `get_session_info` 工具，读取当前 pi 会话的 `sessionId`。不要通过“最近活跃 session”猜当前 session，除非工具不可用且用户接受不确定性。

工具不可用时回退：

```bash
bash scripts/current-session.sh
```

脚本输出 session id 字符串或 `UNKNOWN`。

### 3. 匹配需求 ID

用户说的需求 ID 可能是缩写（如 `WMS-001`），实际 ID 可能是 `WMS-001-log-refactor`。

```bash
curl -sf http://localhost:7331/api/requirements | python3 -c '
import sys, json
keyword = sys.argv[1].lower() if len(sys.argv) > 1 else ""
data = json.load(sys.stdin)
for r in data.get("requirements", []):
    rid = r.get("id", "").lower()
    title = r.get("title", "").lower()
    if keyword in rid or keyword in title:
        print(f"{r['id']}  {r['title']}  [{r['status']}]")
' "<用户说的关键词>"
```

- 如果只有 1 个匹配，直接用。
- 如果有多个匹配，列出来让用户选。
- 如果 0 个匹配，列出相关候选，不猜测。

### 4. 执行关联

```bash
curl -s -o /dev/null -w "%{http_code}" \
  -X POST http://localhost:7331/api/requirement/associate \
  -d "reqId=<实际需求ID>" \
  -d "sessionId=<当前sessionID>"
```

返回码判断：

- `303` = 成功
- `400` = 缺少 reqId/sessionId 或 sessionId 格式不合法
- `404` = 需求不存在

不要用 `curl -f` 判断；成功默认是 303。

### 5. 确认结果

关联成功后，再 GET `/api/requirements` 验证该需求的 `sessionIds` 包含当前 session id。

## 查看当前 session 已关联的需求

先用 `get_session_info` 获取当前 sessionId，再查关联：

```bash
curl -sf http://localhost:7331/api/requirements | python3 -c '
import sys, json
sid = sys.argv[1]
data = json.load(sys.stdin)
for r in data.get("requirements", []):
    if sid in r.get("sessionIds", []):
        print(f"{r['id']}  {r['title']}  [{r['status']}]")
        break
else:
    print("未关联到任何需求")
' "<sessionId>"
```

## 需求文件维护提示

绑定 session 后，必须向用户输出以下维护要求，让 agent 在整个开发过程中持续更新需求文件：

```text
📋 当前 session 已关联到需求。后续开发中，以下事件发生后必须立即更新对应需求文件：
- 完成 PRD/需求口径澄清 -> memory.md + background.md
- 代码 push 或 merge 成功 -> branch.md
- 新增/修改 DB / Apollo / Nacos 配置 -> config-changes.md
- 明确测试场景或回归范围 -> test.md
- 编码前或影响面变化 -> impact.md
- 完成阶段性进展、关键决策、踩坑 -> notes.md

重要：更新需求文件是任务的一部分。代码 push 完成但需求文件未更新 = 任务未完成。
```

不修改 `meta.md` 的 status 字段（由 Agent Panel 管理）。

## Required Checks

- Agent Panel 必须在运行。
- Session ID 必须来自 `get_session_info` 或用户明确确认的 fallback。
- 需求 ID 必须精确匹配。
- 关联请求返回 303 后，还要读取需求列表确认 `sessionIds`。
- 不要修改需求状态；状态推进使用 `agent-panel-requirement-api`。

## Final Response

```text
已将当前 session 关联到需求：
- 需求：<title>（<id>）
- 状态：<status>
- Session：<sid>

可以在 Agent Panel 查看详情：http://localhost:7331/requirement?id=<id>
```

绑定成功后，紧接着输出「需求文件维护提示」中的维护要求。
