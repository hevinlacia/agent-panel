---
name: agent-panel-capability-api
description: 通过本机 Agent Panel API 检索项目 capability pack（造数能力、API 模板等），返回 normalized 通用视图，只读不执行。触发词：capability、能力源、造数能力查询、testdata capabilities、wms-testdata 能力、能力 schema。
allowed-tools: ["bash", "read"]
---

# Agent Panel Capability API

用于：通过本机 Agent Panel API 检索项目 capability pack，以通用 `normalized` 视图查看造数能力、API 模板等，不直接读取 pack 目录里的大文件。

适用：
- 用户问“有哪些造数能力”“能不能造 XX 状态的数据”“能力清单”。
- agent 需要在造数前低 token 确认某能力是否存在、支持哪些 target、runner 命令是什么。
- 需要知道 capability pack 的通用 schema 或 legacy 字段映射。
- 跨项目接入：新项目想按统一协议暴露 capability pack，先查 schema 再实现。

不适用：
- 真正执行造数脚本（Agent Panel 当前只读，不执行 runner；执行走 `wms-test-data-creation` 等项目 skill）。
- 查询业务知识/经验（用 `agent-panel-knowledge-api`）。
- 操作需求文件（用 `agent-panel-requirement-api`）。

## Trigger

- “查造数能力” / “capability 列表” / “有哪些 testdata capability”。
- “WMS 能造哪些状态” / “outbound 能力支持哪些 target”。
- “capability pack schema” / “能力 schema” / “legacy 字段怎么映射”。
- “能力源在哪” / “capability sources” / “testdata 迁移到哪了”。
- agent 已知要调用 capability API，不应再搜索项目源码。

## 术语和边界

- 产品/页面统一称 **Agent Panel**。
- 本机默认地址：`http://localhost:7331`，可由 `PORT` 覆盖。
- Agent Panel 当前阶段**只读索引**：检索、规范化、返回 schema，不执行 `runner.command`。
- `sourcePath` 指向项目内 capability pack；`preferredSourcePath` 是项目内优先路径，`legacySourcePath` 是旧工具仓 fallback。

## API Contract

| 用途 | 方法 | 端点 | 说明 |
|---|---|---|---|
| 查能力源 | GET | `/api/capability/sources` | 列出已接入的 capability source、迁移状态、优先/旧路径 |
| 查通用 schema | GET | `/api/capability/schema` | 返回 `agentPanel.capabilityPackSchema.v1`：pack/capability/legacyAdapter 字段定义 |
| 查能力列表 | GET | `/api/capabilities?project=<project>` | 按 `project`/`domain`/`target`/`q` 过滤；每条含 `normalized` 通用视图 |
| 查单能力详情 | GET | `/api/capability?id=<id>&project=<project>` | 返回单能力详情，含完整 `normalized`、`targets`、`cli`、`pitfalls`、`migration` |
| 旧兼容：能力列表 | GET | `/api/testdata/capabilities?project=<project>` | 同 `/api/capabilities`，向后兼容 |
| 旧兼容：单能力 | GET | `/api/testdata/capability?id=<id>&project=<project>` | 同 `/api/capability`，向后兼容 |
| agent 别名 | GET | `/api/agent/capabilities/query?project=<project>` | 同 `/api/capabilities`，给 agent 用 |

## normalized 视图

每条 capability 返回 `normalized` 字段，把 legacy 项目字段映射成通用结构：

| normalized 字段 | 来源（WMS legacy） | 含义 |
|---|---|---|
| `id` | `id` | 稳定能力 ID |
| `kind` | 固定 `testdata` | 能力类型 |
| `title` / `description` | `purpose` | 人类可读标题 |
| `domain` / `object` | `domain` / `object` | 业务域与目标对象 |
| `inputs` | `cli` | 参数 schema |
| `outputs` | `stdout_json` / `exit_code` | 输出声明 |
| `environments` | `verified_env` / `verified_date` | 已验证环境与日期 |
| `runner` | `execution` / `script` / `invocation` | 运行方式；`readOnlyInAgentPanel=true` |
| `safety` | 固定 | `agentPanelExecutes=false`，是否写库、UAT 是否允许写库 |
| `verification` | `targets` / `state_graph` | 验证目标与状态图 |
| `relatedArtifacts` | `recipe` / `state_graph` / `pitfalls` / `notes` | 关联资产 |
| `legacy` | 原始对象 | 保留原始 legacy 字段，便于回退 |

schema 详见 `GET /api/capability/schema`，或 WMS pack 内 `capability-pack.schema.md`。

## Query Workflow

1. 先确认 Agent Panel 可用：

```bash
curl -sf --max-time 3 http://localhost:7331/health >/dev/null \
  && echo AGENT_PANEL_OK || echo AGENT_PANEL_DOWN
```

2. 查能力源，确认项目已接入：

```bash
curl -sS 'http://localhost:7331/api/capability/sources'
```

关注 `sourceKind`：`project-pack` 表示已读项目内 pack，`legacy-tools-fallback` 表示项目内 pack 缺失、退回旧工具仓。

3. 按项目列能力，默认返回 `normalized`：

```bash
curl -sS 'http://localhost:7331/api/capabilities?project=WMS'
```

支持过滤：

```bash
# 按 target 状态反查哪些能力能到达
curl -sS 'http://localhost:7331/api/capabilities?project=WMS&target=shipped'

# 按 domain 过滤
curl -sS 'http://localhost:7331/api/capabilities?project=WMS&domain=outbound'

# 关键词搜索
curl -sS 'http://localhost:7331/api/capabilities?project=WMS&q=盘点'
```

4. 命中后取单能力详情，看完整 `runner` / `targets` / `pitfalls`：

```bash
curl -sS 'http://localhost:7331/api/capability?id=outbound-any-status&project=WMS'
```

5. 只在需要执行时，按 `normalized.runner.cwd` 和 `normalized.runner.command` 到对应项目目录运行脚本；Agent Panel 不代为执行。

## Safety Boundary

- Agent Panel 当前阶段只读：检索、规范化、返回 schema，不执行 `runner.command`。
- 任何执行都必须由 agent 明确选择对应项目目录和 skill 流程后进行（如 WMS 造数走 `wms-test-data-creation`）。
- `normalized.safety.agentPanelExecutes=false` 是硬约束，不要尝试通过 API 触发执行。

## Required Checks

- 查询类任务：说明使用了哪些 `id`，是否展开详情。
- `sourceKind=legacy-tools-fallback` 时，提醒用户项目内 pack 缺失，正在用旧工具仓。
- 不要把 `normalized.runner.command` 当成 Agent Panel 可执行的入口。

## Final Response

```text
✅ Capability API 已检索
- 项目: <project>
- 能力源: <sourceKind>（<sourcePath>）
- 命中: <count> 条 / 单能力 <id>
- 关键 normalized: kind=<kind>, runner.type=<type>, verifiedEnv=<env>
- 执行提示: Agent Panel 只读，执行请走 <对应项目 skill>
```
