---
name: agent-panel-requirement-api
description: 通过本机 Agent Panel API 创建/查询/更新需求、关联 session、合并需求分支和检查服务，避免误找接口、直接手写需求文件或自行执行 git merge。
allowed-tools: ["bash", "read", "write", "edit", "get_session_info"]
---

# Agent Panel Requirement API

用于：通过本机 Agent Panel API 操作需求，包括创建需求、查询需求、按 intent 获取低 token 上下文、结构化编辑需求文件、追加 notes、更新状态、合并需求分支、关联 session 和确认服务状态。

适用：
- 用户说“创建需求”/“新建需求”/“登记需求”时，通过 API 创建标准需求目录和文件
- 用户说“更新需求”/“修改需求标题/owner/计划上线/ONES/项目”等受控字段时，通过 API 更新
- 用户说“补充 notes/background/test/impact/config-changes”等需求文档时，优先走 `edit-plan -> context -> edit -> validate` 协议
- 用户要求“根据需求文件继续工作”/“看看需求上下文”/“补充自测证据/分支/配置变更/影响面”时，先用低 token context 接口，不直接读大文件
- 用户说“校验需求文件”/“检查需求结构”时，通过 API validate
- 用户说“把需求状态改成/推进到 <状态>”“推到经验总结”“状态更新接口在哪”
- 用户说“合并到 test / UAT”“需求分支合并”“把分支合到测试/测试中”“merge branch”“处理合并冲突”时，必须通过 Agent Panel 分支合并 API，不要自行手写 `git checkout/merge/push`
- 用户要求查询 Agent Panel 需求、按状态筛选需求、确认需求是否存在
- 用户要求把当前 session 关联到某个需求，且需要明确 API 调用细节
- 其他 skill 需要调用需求 API，避免临时翻源码找路由

不适用：
- Agent Panel 不可用且用户仍要求离线创建/重写需求文件模板（fallback 用 `req-create`）
- 批量发布前检查具体分支/配置/测试证据（用 `req-release-check`）
- 直接修改 `state.json`、`meta.md` 的状态字段或真实业务代码

## Trigger

- “更新需求状态” / “推进状态” / “设为经验总结” / “设为已完成”
- “合并到 test” / “合并到 UAT” / “需求分支合并” / “merge branch” / “冲突处理”
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
| 查协议 schema | GET | `/api/requirement/schema` | 返回稳定 token、intent、允许操作和推荐流程 |
| 获取编辑计划 | GET | `/api/requirement/edit-plan?id=<reqId>&intent=<intent>` | 返回本 intent 应读/写的 token、文件路径、大小和示例写法 |
| 获取低 token 上下文 | GET | `/api/requirement/context?id=<reqId>&intent=<intent>&budget=2000` | 按 intent 和 budget 返回压缩上下文；可选 `tokens=req.meta,req.test` |
| 获取 agent 专用上下文 | GET | `/api/requirement/context?id=<reqId>&for=agent&intent=<intent>&budget=2000` | 返回 `phaseRuntime`（当前状态独有提示词 `currentPhasePrompt`、`entryChecks`、跳状态风险 `phaseGaps`、状态历史 `transitionMemory`）、摘要文档、最近结构化事件、推荐写入 API；优先用于继续需求工作 |
| 记录结构化事件 | POST | `/api/requirement/events` | JSON body: `reqId`, `type`, `summary`; 可选 `details`, `evidence`, `decisions`, `todos`, `relatedFiles`, `testCases`, `idempotencyKey`, `appendNote`, `dryRun`；写 `events.jsonl`，默认同步追加 notes |
| 章节 upsert 别名 | POST/PATCH | `/api/requirement/sections/{section}` | JSON body: `reqId`, `content`; 可选 `heading`, `docType`, `token`, `dryRun`；section 如 `impact`, `test`, `boxCodeIssue` |
| 结构化编辑 | POST | `/api/requirement/edit` | JSON/form body: `reqId`, `operation`; 支持 `setStatus`, `setCategory`, `patchMeta`, `appendNote`, `writeDoc`, `upsertSection` |
| 创建需求 | POST | `/api/requirements` | JSON/form body: `reqId`, `title`; 可选 `project`, `projects`, `status`, `category`, `owner`, `startDate`, `planRelease`, `ones`, `summary`, `background`, `notes`, `parentReqId`, `root`, `groupPath`, `dryRun` |
| 查单需求 | GET | `/api/requirement?id=<reqId>` | 返回 `{ requirement }` |
| 更新受控字段 | PATCH | `/api/requirement` | JSON/form body: `reqId`; 可选 `title`, `project`, `projects`, `status`, `category`, `owner`, `startDate`, `planRelease`, `ones`, `note`, `dryRun`；兼容旧 skill，优先用 `/api/requirement/edit` |
| 更新受控字段兼容 | POST | `/api/requirement/update` | 同 PATCH `/api/requirement`，给不方便发 PATCH 的 agent/client 使用 |
| 追加 notes | POST | `/api/requirement/notes` | JSON/form body: `reqId`, `text`; 可选 `title`, `sessionId`, `dryRun`；兼容旧 skill，优先用 `/api/requirement/edit` + `operation=appendNote` |
| 写受控文档 | PUT/POST | `/api/requirement/doc` | JSON/form body: `reqId`, `docType`, `content`; 可选 `mode=replace|append`, `dryRun`；兼容旧 skill，优先用 `/api/requirement/edit` + `writeDoc/upsertSection` |
| 校验需求 | POST | `/api/requirement/validate` | JSON/form body: `reqId`，返回 problems/warnings/files |
| 获取合并分支选项 | GET | `/api/requirement/merge-options?id=<reqId>` | 返回前端/后端可选环境分支和按需求状态计算的默认选中值 |
| 合并需求分支 | POST | `/api/requirement/merge-branch` | JSON/form body: `reqId`, `repoKind=frontend|backend`, `targetBranch=<branch>`；可选 `target=test|uat` |
| 查询合并/冲突状态 | GET | `/api/requirement/merge-status?id=<reqId>&target=test|uat` | 返回未完成 merge worktree、冲突文件和状态 |
| 查询代码审查门禁 | GET | `/api/requirement/review-gate?id=<reqId>` | 返回 `PASS/BLOCKED/WAIVED/missing/pending`；推进到测试中前必须通过或豁免 |
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
    "status": "需求澄清",
    "summary": "...",
    "dryRun": true
  }'
# 返回 reqId: "WMS-043-rocketmq-fail-alarm"（序号由 API 分配）
```

状态值必须严格匹配：

```text
需求澄清 / 开发中 / 自测中 / 测试中 / 经验总结 / 已完成
```

## Agent File Protocol

需求文件统一通过稳定 token 访问，agent 不需要猜文件路径：

| Token | 文件 | 用途 | 推荐写操作 |
|---|---|---|---|
| `req.meta` | `meta.md` | 标题、owner、project、ONES 等稳定身份信息 | `patchMeta` |
| `req.state` | `state.json` | 状态/类别/历史的唯一真源 | `setStatus` / `setCategory` |
| `req.background` | `background.md` | 目标、背景、范围、关键决策 | `writeDoc` / `upsertSection` |
| `req.memory` | `memory.md` | 面向 agent 的短摘要上下文 | `writeDoc` / `upsertSection` |
| `req.branch` | `branch.md` | 人类可读分支说明 | `writeDoc` / `upsertSection` |
| `req.branchScope` | `branches.json` | 机器可读 repo/branch 范围 | `req-branches-update` 维护 |
| `req.configChanges` | `config-changes.md` | DB/Apollo/Nacos/RocketMQ 等配置明细 | `writeDoc` / `upsertSection` |
| `req.releaseManifest` | `release-manifest.md` | 上线清单：表、配置、Topic/Group、Job、接口、人工动作总览 | `writeDoc` / `upsertSection` |
| `req.impact` | `impact.md` | 影响面、核心链路风险、回滚方案 | `writeDoc` / `upsertSection` |
| `req.test` | `test.md` | 测试场景、自测/UAT 证据 | `writeDoc` / `upsertSection` |
| `req.notes` | `notes.md` | 追加型进展、决策、踩坑 | `appendNote` 优先 |
| `req.review` | `review.md` | 代码审查摘要 | `writeDoc` / `upsertSection` |
| `req.codeReview` | `code-review.json` | 大体积代码差异/审查产物 | 仅显式 review intent 读取 |

标准 intent：`overview` / `clarification` / `status` / `progress` / `branch` / `self-test` / `release-check` / `experience-summary` / `design` / `config` / `review`。

代码审查门禁：不新增主状态；作为 `自测中 → 测试中` 的强 gate。`review.md` 或 `code-review-ai.md` 必须明确写 `Review Gate: PASS`、`Review Gate: BLOCKED` 或 `Review Gate: WAIVED`，否则 `/api/requirement/status` 会拒绝推进到 `测试中`。

上线清单：`release-manifest.md` 是贯穿全流程维护的发布资产总览，需求详情页常驻展示；凡涉及 DB 表、Apollo/Nacos、Topic/Group、Job、开关、接口、外部依赖或上线人工动作，都要同步维护 `req.releaseManifest`。

推荐流程：

1. 每轮开始或状态切换后，先调 `/api/requirement/context?id=<reqId>&for=agent&intent=<intent>&budget=2000` 获取 agent 专用摘要；以返回的 `phaseRuntime.currentPhasePrompt` 作为当前阶段导航，**不要沿用 session 创建时注入的启动 prompt**。跳状态时 `phaseRuntime.phaseGaps` 暴露的缺失 entry checks 作为风险记录，不自动回退状态。不要默认直接读 `notes.md` 或 `code-review.json`。
2. 需要明确读/写 token 时，再调 `/api/requirement/edit-plan?id=<reqId>&intent=<intent>`。
3. 事实、证据、测试结果、决策、TODO 用 `/api/requirement/events` 记录结构化事件；默认同时追加 `notes.md`。
4. 目标文档更新用 `/api/requirement/sections/{section}` 或 `/api/requirement/edit` 的 `upsertSection`，避免整篇覆盖。
5. 写完调 `/api/requirement/validate`。

结构化编辑示例：

```bash
# 获取 agent 专用上下文（首选）
curl -sS 'http://localhost:7331/api/requirement/context?id=<req-id>&for=agent&intent=self-test&budget=2000'

# 获取自测编辑计划（需要精确 token 时）
curl -sS 'http://localhost:7331/api/requirement/edit-plan?id=<req-id>&intent=self-test'

# 记录结构化测试事件，同时追加 notes.md
curl -sS -H 'Content-Type: application/json' \
  -X POST http://localhost:7331/api/requirement/events \
  -d '{"reqId":"<req-id>","type":"testResult","summary":"回滚场景验证完成","testCases":[{"name":"未装箱单","result":"pass","evidence":"941373 Success"}]}'

# 追加自由进展
curl -sS -H 'Content-Type: application/json' \
  -X POST http://localhost:7331/api/requirement/edit \
  -d '{"reqId":"<req-id>","operation":"appendNote","title":"进展","text":"完成影响面梳理。"}'

# 按语义 section 更新 impact.md/test.md，不需要 agent 记文件名
curl -sS -H 'Content-Type: application/json' \
  -X POST http://localhost:7331/api/requirement/sections/impact \
  -d '{"reqId":"<req-id>","heading":"boxCode 问题","content":"- 问题描述...\n- 根因..."}'

# upsert test.md 的自测证据 section（通用 edit 入口）
curl -sS -H 'Content-Type: application/json' \
  -X POST http://localhost:7331/api/requirement/edit \
  -d '{"reqId":"<req-id>","operation":"upsertSection","token":"req.test","heading":"自测证据","content":"- Kibana tid=... 验证通过"}'

# 设置状态（写 state.json，不手改文件）
curl -sS -H 'Content-Type: application/json' \
  -X POST http://localhost:7331/api/requirement/edit \
  -d '{"reqId":"<req-id>","operation":"setStatus","status":"自测中","note":"开发自测开始"}'
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
    "status": "需求澄清",
    "summary": "通过 Agent Panel API 创建需求文件"
  }'
```

成功返回包含 `reqDir`、`files` 和 `validation`。如果返回 `validation.ok=false`，先处理 `problems` 再继续。

### 3A. Agent 专用上下文与结构化编辑（推荐）

当任务不是单纯创建需求，优先拿 agent 专用上下文，而不是直接读写需求目录：

```bash
curl -sS "http://localhost:7331/api/requirement/context?id=<req-id>&for=agent&intent=<intent>&budget=2000"
```

返回重点：`summaryDocs`（摘要文档）、`recentEvents`（最近结构化事件）、`recommendedWrites`（推荐写 API）。只有摘要不足时，才再按 token 获取全文片段：

```bash
curl -sS "http://localhost:7331/api/requirement/edit-plan?id=<req-id>&intent=<intent>"
curl -sS "http://localhost:7331/api/requirement/context?id=<req-id>&intent=<intent>&budget=2000"
```

根据上下文选择写入方式：事实/证据/决策/TODO 用 events，目标章节用 sections。

```bash
curl -sS -H 'Content-Type: application/json' \
  -X POST http://localhost:7331/api/requirement/events \
  -d '{"reqId":"<req-id>","type":"rootCause","summary":"boxCode 不匹配导致库存查询落空","evidence":["LocationInventoryService findList 对 boxCode=null 查 IS NULL"]}'

curl -sS -H 'Content-Type: application/json' \
  -X POST http://localhost:7331/api/requirement/sections/impact \
  -d '{"reqId":"<req-id>","heading":"boxCode 问题","content":"- 问题描述...\n- 根因..."}'

curl -sS -H 'Content-Type: application/json' \
  -X POST http://localhost:7331/api/requirement/validate \
  -d '{"reqId":"<req-id>"}'
```

```bash
curl -sS -H 'Content-Type: application/json' \
  -X POST http://localhost:7331/api/requirement/edit \
  -d '{"reqId":"<req-id>","operation":"upsertSection","token":"req.impact","heading":"影响面评估","content":"- ..."}'

curl -sS -H 'Content-Type: application/json' \
  -X POST http://localhost:7331/api/requirement/validate \
  -d '{"reqId":"<req-id>"}'
```

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

字段范围只包括受控字段；状态和类别也可走 PATCH，但单纯状态流转推荐 `/api/requirement/edit` 的 `setStatus`。

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

`docType` 仅支持：`background` / `memory` / `branch` / `config-changes` / `release-manifest` / `impact` / `test` / `notes` / `review` / `release-check` / `experience-summary` / `alignment` / `prd`。

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
{"ok":true,"status":"经验总结"}
```

若不用 JSON header，成功可能是 `303`，不要用 `curl -f` 误判。

### 8. 合并需求分支到 test / UAT

当用户要求把需求分支合并到 test 或 UAT，**必须优先调用 Agent Panel API**，不要让 agent 自己手写 `git checkout` / `git pull` / `git merge` / `git push`。Agent Panel 会根据 `branches.json` 固定仓库范围、目标分支、临时 worktree 和冲突返回格式，减少 token 和误操作。

标准流程：

1. 先确认需求 ID；不确定时按“解析或匹配需求 ID”查找。
2. 调 `merge-options` 获取前端/后端可选分支和默认值：

```bash
curl -sS 'http://localhost:7331/api/requirement/merge-options?id=<req-id>'
```

返回结构重点：

```json
{
  "status": "自测中",
  "options": {
    "frontend": {
      "repoKind": "frontend",
      "defaultValue": "test",
      "options": [
        {"value":"test","label":"前端 test","target":"test"},
        {"value":"master","label":"前端 UAT (master)","target":"uat"}
      ]
    },
    "backend": {
      "repoKind": "backend",
      "defaultValue": "test",
      "options": [
        {"value":"test","label":"后端 test","target":"test"},
        {"value":"UAT-2607","label":"后端 UAT (UAT-2607)","target":"uat"}
      ]
    }
  }
}
```

选择规则：

- 用户明确指定前端/后端和分支时，按用户指定执行。
- 未明确指定时，使用接口 `defaultValue`：
  - 非 `自测中` / `测试中`：默认不合并，必须先让用户确认或说明无默认选择。
  - `自测中`：前端/后端默认 `test`。
  - `测试中`：前端默认 `master`，后端默认最新 `UAT-*`。
- 前端下拉/选项只允许 `test` 和 `master`；后端只允许 `test` 和最新 `UAT-*`。
- 未选择的前端或后端不要合并。

执行合并：

```bash
curl -sS -H 'Content-Type: application/json' \
  -X POST http://localhost:7331/api/requirement/merge-branch \
  -d '{"reqId":"<req-id>","repoKind":"backend","targetBranch":"test","target":"test"}'
```

前端 UAT 示例：

```bash
curl -sS -H 'Content-Type: application/json' \
  -X POST http://localhost:7331/api/requirement/merge-branch \
  -d '{"reqId":"<req-id>","repoKind":"frontend","targetBranch":"master","target":"uat"}'
```

后端 UAT 示例（`targetBranch` 取 `merge-options` 返回的最新 `UAT-*`）：

```bash
curl -sS -H 'Content-Type: application/json' \
  -X POST http://localhost:7331/api/requirement/merge-branch \
  -d '{"reqId":"<req-id>","repoKind":"backend","targetBranch":"UAT-2607","target":"uat"}'
```

结果处理：

- `status=merged` / `upToDate`：合并已完成或目标已包含需求分支。
- `status=skipped`：对应 repo 类型或分支不适用；检查 `repoKind` / `targetBranch` / `branches.json`。
- `status=failed`：读取 `message` 和 `commands`，不要改用手写 git 绕过；先判断是分支不存在、push 失败还是 worktree 清理失败。
- `status=conflict`：必须使用接口返回的 `worktreePath` 和 `conflictFiles` 作为冲突处理入口。

冲突状态查询：

```bash
curl -sS 'http://localhost:7331/api/requirement/merge-status?id=<req-id>&target=test'
```

冲突处理原则：

- 人工触发页面时，可让用户打开 `/requirement-merge?id=<req-id>` 查看冲突 worktree 和文件列表。
- agent 调用接口时，若返回 `conflict`，后续冲突解决直接进入返回的 `worktreePath`，按 `conflictFiles` 列表处理；不要重新扫描所有仓库或重新推断分支。
- 冲突解决、提交、推送仍属于 git 写操作；如需要 agent 继续处理，遵循对应项目的 git/worktree 规则。

### 9. 关联当前 session

优先用 `get_session_info` 取当前 session id；拿不到时才使用已有脚本或让用户提供。执行：

```bash
curl -s -o /dev/null -w "%{http_code}" \
  -X POST http://localhost:7331/api/requirement/associate \
  -d "reqId=<req-id>" \
  -d "sessionId=<session-id>"
```

`303` 表示成功；`400` 检查参数和 harness；`404` 表示需求不存在。

### 10. release-check 配合规则

当用户同时要求“生成 release-check.md 并推进到经验总结”：

1. 先按 `req-release-check` 生成或刷新对应需求的 `release-check.md`。
2. 再调用 `/api/requirement/status` 设置为 `经验总结`。
3. 最后重新 GET `/api/requirements` 验证该需求状态已经变化。

若用户只要求状态更新，不要顺手生成 release-check.md。

## Required Checks

- 创建/修改需求优先走 Agent Panel API；Agent Panel 不可用时才 fallback 到 `req-create` 直接写文件。
- 非简单更新先走 `/api/requirement/edit-plan` + `/api/requirement/context`，不要默认直接读取大文件。
- 写入优先走 `/api/requirement/edit`；旧 `/notes` 和 `/doc` 仅作兼容。
- 不直接编辑 `state.json` 或 `meta.md` 的状态字段。
- `reqId` 只能使用 ASCII 字母、数字和单个连字符，不能包含空格、中文、路径分隔符或 `..`。
- 写文档只能使用 `/api/requirement/doc` 的白名单 `docType`；不要把任意文件路径传给 API 或直接写路径。
- 大段进展优先 `/api/requirement/notes` 追加，避免覆盖已有 notes。
- `dryRun:true` 适用于创建、更新字段、notes、doc 写入预览；正式写入后再 validate。
- `status` 必须是 7 个合法状态之一。
- `GET /api/requirements` 不保证服务端按 query 过滤；按状态筛选时必须客户端过滤。
- 状态更新优先使用 `Accept: application/json`；不用 `curl -f` 判断 303。
- 需求分支合并必须优先走 `/api/requirement/merge-options` + `/api/requirement/merge-branch`；不要自行手写 git merge/push，除非 Agent Panel 不可用且用户明确要求 fallback。
- merge-options 没有默认值时（非 `自测中`/`测试中`），不要擅自选择目标分支；向用户确认或说明未执行合并。
- 处理合并冲突时，使用接口返回的 `worktreePath` 和 `conflictFiles`；不要重新探索所有仓库。
- Agent Panel 不可用时先检查 systemd unit，不要花很久搜索源码。

## Final Response

```text
✅ Agent Panel 需求 API 已执行
- 操作: <创建 / 更新字段 / 追加 notes / 写文档 / 状态变更 / 校验>
- 需求: <title>（<req-id>）
- 写入: <files 或 无>
- 校验: <ok / problems 摘要>
```
