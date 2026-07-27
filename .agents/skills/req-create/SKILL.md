---
name: req-create
description: 创建和更新需求文件，优先通过 Agent Panel API 与需求文件交互（创建/改字段/写文档/校验），API 不可用时才直接写文件兜底，确保目录结构、frontmatter 格式、父子关系和状态值符合 Agent Panel 解析规范。
allowed-tools: ["bash", "read", "write", "edit", "get_session_info"]
---

# Requirement File Create & Update

用于：当需要创建新需求、更新需求信息、变更需求状态或补充需求文档时，**优先通过 Agent Panel API** 生成和修改需求文件；API 不可用时才退化为直接写文件兜底。

适用：
- 用户说"创建需求"/"新建需求"/"登记需求"时生成目录和初始文件（走 API）
- 用户说"更新需求"/"改需求标题/owner/计划上线/ONES/项目"等受控字段时（走 API）
- 用户说"补充需求背景/测试/影响面/配置变更/进展笔记"等文档时（走 API 的 doc/notes 接口）
- 用户说"创建子需求"/"拆分子需求"时（走 API，传 `parentReqId`）
- 用户说"校验需求文件结构"时（走 API validate）

不适用：
- 需求开发到上线的全流程跟踪和发布预检（走 `req-tracker` skill）
- 将已有 session 绑定到已有需求（用 `req-session-bind`）；创建需求时的自动绑定由本 skill 负责
- 代码实现、仓库探索、调用链分析

## 交互模式：API 优先，文件兜底

所有需求文件写操作默认走 Agent Panel API。只有当 Agent Panel 不可用时，才退化为直接写文件，并必须在最终回复里明确标注"兜底路径"。

**Agent Panel 默认地址**：`http://localhost:7331`（可由 `PORT` 覆盖）。

**健康检查**（每次写操作前先确认服务可用）：

```bash
curl -sf --max-time 3 http://localhost:7331/health >/dev/null \
  && echo API_UP || echo API_DOWN
```

- `API_UP` → 走 API 主路径。
- `API_DOWN` → 告知用户 Agent Panel 未运行，并按「兜底路径」直接写文件；不要静默走兜底。

API 契约详情见 `agent-panel-requirement-api` skill，本 skill 只列与创建/更新相关的端点：

| 用途 | 方法 | 端点 | 关键参数 |
|---|---|---|---|
| 创建需求 | POST | `/api/requirements` | `reqId`, `title`；可选 `project`, `projects`, `status`, `category`, `owner`, `startDate`, `planRelease`, `ones`, `summary`, `background`, `notes`, `parentReqId`, `root`, `groupPath`, `dryRun` |
| 更新受控字段 | PATCH | `/api/requirement` | `reqId`；可选 `title`, `project`, `projects`, `status`, `category`, `owner`, `startDate`, `planRelease`, `ones`, `note`, `dryRun`（不便发 PATCH 时用 `POST /api/requirement/update`） |
| 追加 notes | POST | `/api/requirement/notes` | `reqId`, `text`；可选 `title`, `sessionId`, `dryRun` |
| 写受控文档 | PUT/POST | `/api/requirement/doc` | `reqId`, `docType`, `content`；可选 `mode=replace\|append`, `dryRun` |
| 校验需求 | POST | `/api/requirement/validate` | `reqId` |
| 更新状态 | POST | `/api/requirement/status` | `reqId`, `status`, 可选 `note` |

`dryRun:true` 适用于创建、更新字段、notes、doc 写入预览；只返回计划写入的文件路径，不落盘。

## Directory Layout

需求根目录由 Agent Panel 配置的 `requirementScanRoots` 决定（见 `/settings`）。两种合法布局：

```text
# 项目分组布局（推荐）
<scanRoot>/.agents/req/
├── WMS/                          # 项目目录
│   ├── WMS-001-log-refactor/     # 叶子需求
│   │   ├── meta.md
│   │   ├── background.md
│   │   ├── memory.md
│   │   ├── branch.md
│   │   ├── config-changes.md
│   │   ├── impact.md
│   │   ├── test.md
│   │   ├── notes.md
│   │   └── state.json            # 由 Agent Panel 管理，不要手写
│   └── WMS-003-rabbitmq-to-rocketmq/   # 父需求（分组容器）
│       ├── meta.md
│       └── WMS-003-after-picking-batch/ # 子需求
│           └── meta.md

# 旧版平铺布局
<scanRoot>/.agents/req/<req-id>/meta.md
```

- **父需求**：有 `meta.md` 且包含子目录（子目录有自己的 `meta.md`），只是分组容器，状态无意义。
- **叶子需求**：有 `meta.md` 且不包含子需求目录，有完整状态、session、上下文注入功能。
- **子需求**：位于父需求目录下的叶子需求，`parentReqId` 由 API 创建时传入或由扫描器推断。

## File Specs（校验依据 + 兜底模板）

下面是 API 内置生成的文件规范，也是 `validate` 和兜底写文件时的依据。

### meta.md（必填）

```markdown
---
req-id: WMS-001-log-refactor
title: WMS 日志系统重构
status: 需求对齐
project: WMS
owner: hevin
start-date: 2026-06-11
plan-release: unknown
---

# <req-id> <需求标题>

## Summary
- Title: <需求标题>
- Status: <状态>
- Owner: <name>
- Start date: <YYYY-MM-DD 或 unknown>
- Planned release: <YYYY-MM-DD 或 unknown>
- Project: <项目名 (技术栈)>

## Scope
- Include:
  - <本次需求包含的内容>
- Exclude:
  - <本次需求不包含的内容>

## Open Questions
- <待确认的问题>
```

frontmatter 字段规则：

| 字段 | 必填 | 类型 | 说明 |
| --- | --- | --- | --- |
| `req-id` | 是 | string | 与目录名一致，ASCII 字母数字和连字符 |
| `title` | 是 | string | 一句话标题，30 字以内 |
| `status` | 是 | enum | 见下方状态值表 |
| `project` | 否 | string | 显示名覆盖；默认取父目录名 |
| `owner` | 否 | string | 负责人 |
| `start-date` | 否 | string | `YYYY-MM-DD` 或 `unknown` |
| `plan-release` | 否 | string | `YYYY-MM-DD` 或 `unknown` |
| `ones` | 否 | string | 关联的 ONES 任务编号或完整网址；留空或省略表示未关联 |

状态值（7 个，严格匹配 Agent Panel）：

| 值 | 含义 |
| --- | --- |
| `需求对齐` | 业务目标、范围和验收口径对齐中 |
| `方案设计` | 技术方案、影响面和验证路径设计中 |
| `开发中` | 正在开发 |
| `自测中` | 开发完成，开发自测 |
| `测试中` | 已提交测试，测试中 |
| `待上线` | 测试通过，等待上线 |
| `已完成` | 已上线，需求关闭 |

### 其它标准文件

| 文件 | 作用 | 填写时机 |
| --- | --- | --- |
| `background.md` | 目标、背景、范围、关键决策（注入新 session 上下文） | 创建时或需求口径澄清后 |
| `memory.md` | 需求生命周期记忆，供 session 注入 | 持续维护 |
| `branch.md` | 分支、commit、合并状态 | 代码 push/merge 后 |
| `config-changes.md` | DB / Apollo / Nacos / RocketMQ 变更 | 配置变更时 |
| `impact.md` | 编码前影响面评估、核心链路风险、回滚方案 | 编码前 |
| `test.md` | 测试场景清单 + 分阶段执行记录 | 进入自测前 / 自测中 / UAT |
| `notes.md` | 进展、决策、踩坑（追加，不覆盖） | 阶段性进展时 |
| `review.md` | 待上线 Code Review | 按需 |
| `state.json` | Agent Panel 管理，**不要手写** | 首次状态切换时 API 自动生成 |

## Workflow

### 0. 健康检查

先确认 Agent Panel 是否可用（见上方健康检查命令）。`API_UP` 走主路径，`API_DOWN` 走兜底路径并告知用户。

### 1. 创建新需求（API 主路径）

1. 确认信息：`req-id`、`title`、`project`（项目目录名）、是否为父需求、`parentReqId`（子需求时）。
2. **dryRun 预览**，确认目录和文件不会误覆盖：

```bash
curl -sS -H 'Content-Type: application/json' \
  -X POST http://localhost:7331/api/requirements \
  -d '{
    "reqId": "<req-id>",
    "title": "<需求标题>",
    "project": "<project>",
    "status": "需求对齐",
    "summary": "<一句话摘要>",
    "dryRun": true
  }'
```

3. **正式创建**（去掉 `dryRun`，可顺带传 `background`/`notes` 初始内容）：

```bash
curl -sS -H 'Content-Type: application/json' \
  -X POST http://localhost:7331/api/requirements \
  -d '{
    "reqId": "<req-id>",
    "title": "<需求标题>",
    "project": "<project>",
    "status": "需求对齐",
    "summary": "<一句话摘要>"
  }'
```

返回含 `reqDir`、`files`、`validation`。若 `validation.ok=false`，先处理 `problems` 再继续。

4. **补充文档**（按需，分多次调 `/doc` 写 `background`/`impact`/`test`，用 `mode=replace`）：

```bash
curl -sS -H 'Content-Type: application/json' \
  -X PUT http://localhost:7331/api/requirement/doc \
  -d '{"reqId":"<req-id>","docType":"background","mode":"replace","content":"# <req-id> 需求背景\n\n## 目标\n- ..."}'
```

5. **绑定当前 session**（见下方「Session 绑定」）。
6. **向用户输出需求文件维护提示**（见下方）。

> 不传 `status` 时默认 `需求对齐`；不要在创建后立刻手改 `meta.md` 的 status，状态流转走 `/api/requirement/status`。

### 2. 创建子需求

父需求目录已存在时，创建子需求走同一个 `POST /api/requirements`，传 `parentReqId`：

```bash
curl -sS -H 'Content-Type: application/json' \
  -X POST http://localhost:7331/api/requirements \
  -d '{
    "reqId": "<child-req-id>",
    "title": "<子需求标题>",
    "parentReqId": "<parent-req-id>",
    "summary": "<一句话摘要>"
  }'
```

API 会把子需求目录建在父需求目录下，`project` 默认继承父需求。

### 3. 更新需求字段（API 主路径）

```bash
curl -sS -H 'Content-Type: application/json' \
  -X PATCH http://localhost:7331/api/requirement \
  -d '{
    "reqId": "<req-id>",
    "title": "<新标题>",
    "owner": "hevin",
    "planRelease": "unknown",
    "ones": "https://..."
  }'
```

字段范围仅受控字段：`title`/`project`/`projects`/`owner`/`startDate`/`planRelease`/`ones`/`status`/`category`。状态和类别可走 PATCH，但单纯状态流转仍推荐专用 `/api/requirement/status`（会记录 history）。

### 4. 更新需求文档（API 主路径）

- **追加进展**（不要全文覆盖 `notes.md`）：

```bash
curl -sS -H 'Content-Type: application/json' \
  -X POST http://localhost:7331/api/requirement/notes \
  -d '{"reqId":"<req-id>","title":"进展","text":"完成方案梳理。","sessionId":"<可选>"}'
```

- **替换或追加白名单文档**（`docType` 仅支持 `background`/`memory`/`branch`/`config-changes`/`impact`/`test`/`notes`/`review`/`alignment`/`prd`）：

```bash
curl -sS -H 'Content-Type: application/json' \
  -X PUT http://localhost:7331/api/requirement/doc \
  -d '{"reqId":"<req-id>","docType":"test","mode":"replace","content":"# <req-id> Test\n\n## 测试场景清单\n- ..."}'
```

### 5. 状态变更

```bash
curl -sS -H 'Accept: application/json' \
  -X POST http://localhost:7331/api/requirement/status \
  -d "reqId=<req-id>" \
  -d "status=<新状态>" \
  -d "note=<备注，可选>"
```

### 6. 校验需求（推荐在创建/更新后跑一次）

```bash
curl -sS -H 'Content-Type: application/json' \
  -X POST http://localhost:7331/api/requirement/validate \
  -d '{"reqId":"<req-id>"}'
```

`ok=false` 时必须修复 `problems`；`warnings` 是推荐补齐项。

## 兜底路径：API 不可用时直接写文件

仅当健康检查返回 `API_DOWN` 时使用。必须先告知用户"Agent Panel 未运行，本次用直接写文件兜底"。

1. 确认目录路径：
   - 项目分组：`<scanRoot>/.agents/req/<project>/<req-id>/`
   - 旧版平铺：`<scanRoot>/.agents/req/<req-id>/`
   - 子需求：`<scanRoot>/.agents/req/<parent-project>/<parent-req-id>/<child-req-id>/`
2. `mkdir -p <path>`
3. 按「File Specs」生成 `meta.md`（必填，含 frontmatter，`status` 严格匹配 7 个值之一，`req-id` 与目录名一致）。
4. 生成 `background.md`、`memory.md`、`branch.md`、`config-changes.md`、`impact.md`、`test.md`、`notes.md`（可先写占位模板，不写真实 token/密码/Cookie/私钥）。
5. **不要创建 `state.json`**；`meta.md` 的 `status` 字段作为兜底状态来源，Agent Panel 恢复后会以 `state.json` 优先、`meta.md` 次之读取。
6. 绑定当前 session（API 恢复后再补绑，或本 skill 的 session 绑定脚本）。
7. 在最终回复标注「兜底路径」，并提示用户恢复 Agent Panel 后跑一次 `/api/requirement/validate` 校验兜底写入的文件。

兜底写文件时，`status`/`category` 仍只写到 `meta.md`，不要手写 `state.json`。

## Session 绑定

创建需求后，必须立即将当前 session 绑定到该需求。

### 1. 获取当前 Session ID

优先调用 `get_session_info` 工具，读取返回的 `sessionId`（UUID 格式）。

工具不可用时回退：

```bash
bash ~/Developer/tools/agent-panel/.agents/skills/req-session-bind/scripts/current-session.sh
```

### 2. 调用 Agent Panel 绑定接口（API 可用时）

```bash
curl -s -o /dev/null -w "%{http_code}" \
  -X POST http://localhost:7331/api/requirement/associate \
  -H 'Accept: application/json' \
  -d 'reqId=<req-id>' \
  -d 'sessionId=<sessionID>'
```

- `200`（JSON `{"ok":true}`）或 `303` = 成功
- `400` = sessionId 格式不合法
- `404` = 需求不存在
- 连接失败 = Agent Panel 未运行，告知用户但不阻塞需求创建（兜底路径下记录待绑定的 req-id + session-id，待 API 恢复后补绑）

### 3. 验证

```bash
curl -sf http://localhost:7331/api/requirements | python3 -c '
import sys, json
sid = sys.argv[1]
rid = sys.argv[2]
data = json.load(sys.stdin)
for r in data.get("requirements", []):
    if r["id"] == rid and sid in r.get("sessionIds", []):
        print("BOUND")
        break
else:
    print("NOT_BOUND")
' "<sessionID>" "<req-id>"
```

## 需求文件维护提示

绑定 session 后，必须向用户输出以下维护要求，让 agent 在整个开发过程中持续更新需求文件（**优先走 API**）：

```text
📋 需求已创建并绑定当前 session。

在后续开发过程中，以下事件发生后必须立即更新对应需求文件（优先走 Agent Panel API）：
- 完成 PRD/需求口径澄清 -> /doc 写 memory.md + background.md
- 代码 push 或 merge 成功 -> /doc 写 branch.md（分支名、commit、合并状态）
- 需求分支首次 push / 涉及仓库或分支变动 -> branches.json（加载 `req-branches-update` skill，或跑 `python3 ~/.agents/scripts/req-branches-scan.py <req-id>`）
- 新增/修改 DB / Apollo / Nacos 配置 -> /doc 写 config-changes.md
- 明确测试场景或回归范围 -> /doc 写 test.md
- 编码前或影响面变化 -> /doc 写 impact.md（核心链路风险）
- 完成阶段性进展、关键决策、踩坑 -> /notes 追加 notes.md（不要覆盖）

重要：更新需求文件是任务的一部分。代码 push 完成但需求文件未更新 = 任务未完成。
状态变更统一走 /api/requirement/status，不要直接改 meta.md 或 state.json 的状态字段。
```

## Required Checks

- 写操作优先走 Agent Panel API；健康检查 `API_DOWN` 才走兜底，且必须告知用户并标注「兜底路径」。
- `req-id` 只用 ASCII 字母数字和连字符，不含空格、中文、路径分隔符、`..`；`req-id` 与目录名一致。
- `meta.md` frontmatter 的 `status` 严格匹配 7 个值之一。
- 不要在文件里写真实 token、密码、Cookie、私钥。
- 不要手动创建/编辑 `state.json`（Agent Panel 管理）；兜底时状态只写 `meta.md`。
- 创建子需求前确认父需求目录和 `meta.md` 已存在。
- API 创建/更新后建议跑一次 `/api/requirement/validate`，`ok=false` 必须修复 `problems`。

## Final Response

创建完成（API 主路径）：

```text
✅ 已创建（Agent Panel API）: <reqDir>
- 需求: <title>
- 状态: <status>
- 已生成: <files>
- 待补: <哪些文档还需要填>
- 校验: <ok / problems 摘要>
- Session 绑定: <已绑定 / 未绑定（Agent Panel 未运行）>
```

创建完成（兜底路径）：

```text
⚠️ 已创建（兜底路径，Agent Panel 未运行）: <path>
- 需求: <title>
- 状态: <status>
- 已生成: <文件列表>
- 注意: Agent Panel 恢复后请跑一次 /api/requirement/validate 校验
- Session 绑定: <待补绑 / 已绑定>
```

更新完成：

```text
✅ 已更新（Agent Panel API）: <字段或文档>
- 变更: <changes 摘要>
- 写入: <files 或 无>
- 校验: <ok / problems 摘要>
```

状态变更：

```text
✅ <req-id> 状态已变更
- <旧状态> -> <新状态>
- 备注: <note>
```
