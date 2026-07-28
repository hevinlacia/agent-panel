---
name: agent-panel-requirement-api
description: 通过本机 Agent Panel API 创建/查询/更新需求、关联 session、检查服务，避免误找接口或直接手写需求文件。
allowed-tools: ["bash", "read", "write", "edit", "get_session_info"]
---

# Agent Panel Requirement API

用于：通过本机 Agent Panel API 操作需求，包括创建需求、查询需求、更新受控字段/文档、追加 notes、更新状态、关联 session 和确认服务状态。

适用：
- 用户说“创建需求”/“新建需求”/“登记需求”时，通过 API 创建标准需求目录和文件
- 用户说“更新需求”/“修改需求标题/owner/计划上线/ONES/项目”等受控字段时，通过 API 更新
- 用户说“补充 notes/background/test/impact/config-changes”等需求文档时，通过 API 追加或替换白名单文档
- 用户说“校验需求文件”/“检查需求结构”时，通过 API validate
- 用户说“把需求状态改成/推进到 <状态>”“推到待上线”“状态更新接口在哪”
- 用户要求查询 Agent Panel 需求、按状态筛选需求、确认需求是否存在
- 用户要求把当前 session 关联到某个需求，且需要明确 API 调用细节
- 其他 skill 需要调用需求 API，避免临时翻源码找路由

不适用：
- Agent Panel 不可用且用户仍要求离线创建/重写需求文件模板（fallback 用 `req-create`）
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
| 创建需求 | POST | `/api/requirements` | JSON/form body: `reqId`, `title`; 可选 `project`, `projects`, `status`, `category`, `owner`, `startDate`, `planRelease`, `ones`, `summary`, `background`, `notes`, `parentReqId`, `root`, `groupPath`, `dryRun` |
| 查单需求 | GET | `/api/requirement?id=<reqId>` | 返回 `{ requirement }` |
| 更新受控字段 | PATCH | `/api/requirement` | JSON/form body: `reqId`; 可选 `title`, `project`, `projects`, `status`, `category`, `owner`, `startDate`, `planRelease`, `ones`, `note`, `dryRun` |
| 更新受控字段兼容 | POST | `/api/requirement/update` | 同 PATCH `/api/requirement`，给不方便发 PATCH 的 agent/client 使用 |
| 追加 notes | POST | `/api/requirement/notes` | JSON/form body: `reqId`, `text`; 可选 `title`, `sessionId`, `dryRun` |
| 写受控文档 | PUT/POST | `/api/requirement/doc` | JSON/form body: `reqId`, `docType`, `content`; 可选 `mode=replace|append`, `dryRun` |
| 校验需求 | POST | `/api/requirement/validate` | JSON/form body: `reqId`，返回 problems/warnings/files |
| 列需求 | GET | `/api/requirements` | 返回 `{ requirements: [...] }`，客户端自行按 `status` 过滤 |
| 更新状态 | POST | `/api/requirement/status` | form body: `reqId`, `status`, 可选 `note`, `redirect` |
| 更新类别 | POST | `/api/requirement/category` | form body: `reqId`, `category` |
| 更新 ONES | POST | `/api/requirement/ones` | form body: `reqId`, `ones` |
| 关联 session | POST | `/api/requirement/associate` | form body: `reqId`, `sessionId`，成功默认 200 JSON，历史版本可能 303 |

### `{seq}` 自动编号

`reqId` 支持用 `{seq}` 占位符让 API 自动分配序号，调用方不必预先查最大编号。格式：`<前缀>-{seq}-<描述>`，API 扫描同前缀现有需求的最大序号 +1，补零到 3 位（如现有 `WMS-042-*` 则分配 `043`）。

- 占位符必须带非空前缀：`WMS-{seq}-demo` 合法，`{seq}-demo` 报错。
- 只允许一个 `{seq}`：`WMS-{seq}-{seq}` 报错。
- 非 `dryRun` 时用原子 `mkdir` 占号，并发安全；`dryRun` 只预览不占号。
- 不含 `{seq}` 的 `reqId` 走原校验逻辑，完全向后兼容。

示例：

```bash
curl -sS -H 'Content-Type: application/json' \
  -X POST http://localhost:7331/api/requirements \
  -d '{
    "reqId": "WMS-{seq}-rocketmq-fail-alarm",
    "title": "RocketMQ 发送/消费失败告警",
    "project": "WMS",
    "status": "方案设计",
    "summary": "...",
    "dryRun": true
  }'
# 返回 reqId: "WMS-043-rocketmq-fail-alarm"（序号由 API 分配）
```

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

### 3. 创建需求

优先 JSON 调用；`dryRun:true` 可先预览将写入的目录和文件：

```bash
curl -sS -H 'Content-Type: application/json' \
  -X POST http://localhost:7331/api/requirements \
  -d '{
    "reqId": "WMS-001-demo",
    "title": "示例需求",
    "project": "WMS",
    "status": "需求对齐",
    "summary": "通过 Agent Panel API 创建需求文件"
  }'
```

成功返回包含 `reqDir`、`files` 和 `validation`。如果返回 `validation.ok=false`，先处理 `problems` 再继续。

### 4. 更新需求字段

```bash
curl -sS -H 'Content-Type: application/json' \
  -X PATCH http://localhost:7331/api/requirement \
  -d '{
    "reqId": "<req-id>",
    "title": "新标题",
    "owner": "hevin",
    "planRelease": "unknown",
    "ones": "https://..."
  }'
```

字段范围只包括受控字段；状态和类别也可走 PATCH，但单纯状态流转仍推荐下方专用状态接口。

### 5. 追加 notes / 写受控文档

追加进展不要全文覆盖 `notes.md`：

```bash
curl -sS -H 'Content-Type: application/json' \
  -X POST http://localhost:7331/api/requirement/notes \
  -d '{"reqId":"<req-id>","title":"进展","text":"完成影响面梳理。"}'
```

替换或追加白名单文档：

```bash
curl -sS -H 'Content-Type: application/json' \
  -X PUT http://localhost:7331/api/requirement/doc \
  -d '{"reqId":"<req-id>","docType":"test","mode":"replace","content":"# <req-id> Test\n\n## 测试场景清单\n- ..."}'
```

`docType` 仅支持：`background` / `memory` / `branch` / `config-changes` / `impact` / `test` / `notes` / `review` / `alignment` / `prd`。

### 6. 校验需求

```bash
curl -sS -H 'Content-Type: application/json' \
  -X POST http://localhost:7331/api/requirement/validate \
  -d '{"reqId":"<req-id>"}'
```

返回 `ok=false` 时必须修复 `problems`；`warnings` 是推荐补齐项。

### 7. 更新需求状态

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

### 8. 关联当前 session

优先用 `get_session_info` 取当前 session id；拿不到时才使用已有脚本或让用户提供。执行：

```bash
curl -s -o /dev/null -w "%{http_code}" \
  -X POST http://localhost:7331/api/requirement/associate \
  -d "reqId=<req-id>" \
  -d "sessionId=<session-id>"
```

`303` 表示成功；`400` 检查参数和 harness；`404` 表示需求不存在。

### 9. release-check 配合规则

当用户同时要求“生成 release-check.md 并推进到待上线”：

1. 先按 `req-release-check` 生成或刷新对应需求的 `release-check.md`。
2. 再调用 `/api/requirement/status` 设置为 `待上线`。
3. 最后重新 GET `/api/requirements` 验证该需求状态已经变化。

若用户只要求状态更新，不要顺手生成 release-check.md。

## Required Checks

- 创建/修改需求优先走 Agent Panel API；Agent Panel 不可用时才 fallback 到 `req-create` 直接写文件。
- 不直接编辑 `state.json` 或 `meta.md` 的状态字段。
- `reqId` 只能使用 ASCII 字母、数字和单个连字符，不能包含空格、中文、路径分隔符或 `..`。
- 写文档只能使用 `/api/requirement/doc` 的白名单 `docType`；不要把任意文件路径传给 API 或直接写路径。
- 大段进展优先 `/api/requirement/notes` 追加，避免覆盖已有 notes。
- `dryRun:true` 适用于创建、更新字段、notes、doc 写入预览；正式写入后再 validate。
- `status` 必须是 7 个合法状态之一。
- `GET /api/requirements` 不保证服务端按 query 过滤；按状态筛选时必须客户端过滤。
- 状态更新优先使用 `Accept: application/json`；不用 `curl -f` 判断 303。
- Agent Panel 不可用时先检查 systemd unit，不要花很久搜索源码。

## Final Response

```text
✅ Agent Panel 需求 API 已执行
- 操作: <创建 / 更新字段 / 追加 notes / 写文档 / 状态变更 / 校验>
- 需求: <title>（<req-id>）
- 写入: <files 或 无>
- 校验: <ok / problems 摘要>
```
