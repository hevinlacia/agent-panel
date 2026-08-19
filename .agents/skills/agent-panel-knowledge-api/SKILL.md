---
name: agent-panel-knowledge-api
description: 通过本机 Agent Panel API 查询和维护业务知识/经验，默认只取 meta 摘要，需要时再展开 items 正文，节省 agent token。
allowed-tools: ["bash", "read"]
---

# Agent Panel Knowledge API

用于：通过本机 Agent Panel API 查询和维护 `.agents/business-knowledge/` 与 `.agents/experiences/`，让 agent 默认读取低 token 的属性/概述，再按需展开正文。

适用：
- 用户问“业务知识/经验在哪里查”“查一下经验”“查 WMS 业务知识”“有没有相关踩坑”。
- WMS 或其他项目已使用 `.agents/business-knowledge/`、`.agents/experiences/` 管理知识/经验。
- agent 需要查询多条知识候选，但不应直接读大量 Markdown 正文。
- 需要新增或更新一条业务知识/经验，并自动维护 `created_at` / `updated_at`。

不适用：
- 稳定、通用、可执行的长期流程；这类内容应沉淀为 skill。
- WMS 测试造数、API 模板、状态图、finder/verifier/recipe；用 WMS 项目内的 `~/Developer/company/WMS/.agents/testdata/`，或用 Agent Panel `GET /api/capabilities?project=WMS` 检索能力。
- Agent Panel 服务不可用且任务要求离线直接改文件；此时可按项目 AGENTS.md 的文件格式手工处理。

## Trigger

- “查业务知识” / “查经验” / “查踩坑” / “有没有相关知识”。
- “用 Agent Panel 查一下知识/经验”。
- “新增一条经验/业务知识”。
- “把这个记录到 experiences / business-knowledge”。
- WMS 任务开始前需要低 token 获取相关业务上下文。

## Directory Model

项目级目录由项目路径暗示领域，不再额外嵌套 domain 目录。例如 WMS：

```text
$WMS_WORKSPACE_ROOT/.agents/business-knowledge/
  README.md
  meta/*.yaml     # 纯 YAML 属性/概述，程序默认读取
  items/*.md      # 完整正文，按需读取
  index.jsonl     # 应用/脚本生成缓存

$WMS_WORKSPACE_ROOT/.agents/experiences/
  README.md
  meta/*.yaml
  items/*.md
  index.jsonl
```

字段约定：

- `id`：稳定唯一 ID。
- `kind`：`businessKnowledge` 或 `experience`。
- `category`：旧前缀语义，如 `profile` / `biz` / `link` / `api` / `ref` / `conventions` / `pitfall`。
- `created_at` / `updated_at`：创建和更新时间。
- `status`：`active` / `stale` / `deprecated` / `draft`。
- `confidence`：`low` / `medium` / `high`。
- `summary`：Agent 默认返回的短概述。
- `trigger_terms`：检索触发词。
- `source_path`：从 `meta` 指向 `items` 正文的相对路径。
- `related_skills`：相关 agent skill 名称，如 `wms-test-api-call` / `kibana-log-query`。
- `related_repos`：相关代码仓库或部署应用名，如 `yl-cwhsea-wms-web`。
- `related_tables`：相关数据库表名，如 `shipment_header`。
- `related_apis`：相关 HTTP 接口或知识条目 ID，如 `POST /wms-web/outbound/shipment/dispatch` / `link-outbound-shipment-upload`。

## API Contract

默认地址：`http://localhost:7331`。

| 用途 | 方法 | 端点 | 说明 |
|---|---|---|---|
| 查询知识/经验候选 | POST | `/api/agent/knowledge/query` | 默认返回预算裁剪后的摘要、outline、命中解释，不返回正文 |
| 获取单条摘要 | GET | `/api/agent/items/summary?id=<id>` | 只取属性/概述 |
| 获取单条全文 | GET | `/api/agent/items/full?id=<id>&budget=20000` | 读取 `source_path` 指向正文，按 budget 截断 |
| 获取单条章节 | GET | `/api/agent/items/full?id=<id>&section=<heading>&budget=4000` 或 `/api/agent/items/section?id=<id>&section=<heading>` | 只读取指定 Markdown heading 下的章节 |
| 页面/通用列表 | GET | `/api/knowledge?kind=<kind>&q=<kw>&domain=<domain>&limit=...` | 给 UI 或调试使用 |
| 读取单条 | GET | `/api/knowledge/item?id=<id>` | 返回单条，通常含全文 |
| 新增/更新 | POST | `/api/knowledge` | JSON body，写入 `meta/` + `items/` |

`kind` 可取：

```text
businessKnowledge / experience
```

## Query Workflow

1. 先确认 Agent Panel 可用：

```bash
curl -sf --max-time 3 http://localhost:7331/health >/dev/null \
  && echo AGENT_PANEL_OK || echo AGENT_PANEL_DOWN
```

2. 使用 `POST /api/agent/knowledge/query` 查询候选。默认限制 3-5 条，避免 token 膨胀。`tokensBudget` 会影响默认 limit、summary 长度和 outline 数量；查询会返回 `score`、`whyMatched`、`matchedFields`、`matches` 和 `queryTokens`，用于判断为何命中。结果还会返回 `relatedSkills`、`relatedRepos`、`relatedTables`、`relatedApis`；agent 可据此决定下一步该查代码、日志、DB、接口还是其它知识条目。

支持中文自然句查询：后端会对连续中文做 n-gram 扩展，例如“出库单回传为什么没发mq”无需手动加空格也能命中“出库单 / 回传 / MQ”等相关知识。

```bash
curl -sS -H 'Content-Type: application/json' \
  -X POST http://localhost:7331/api/agent/knowledge/query \
  -d '{
    "kind": "businessKnowledge",
    "intent": "出库单创建",
    "domain": "wms",
    "limit": 5,
    "tokensBudget": 2200
  }'
```

经验查询示例：

```bash
curl -sS -H 'Content-Type: application/json' \
  -X POST http://localhost:7331/api/agent/knowledge/query \
  -d '{
    "kind": "experience",
    "intent": "路由前缀 HTTP 302 请求没到 Controller",
    "domain": "wms",
    "limit": 5,
    "tokensBudget": 2200
  }'
```

3. 只在命中结果明显相关时，再取全文或指定章节：

```bash
curl -sS 'http://localhost:7331/api/agent/items/full?id=<id>&budget=20000'
```

如果 query 返回的 `outline` 已经显示需要的章节，优先只取 section，节省 token：

```bash
curl -sS 'http://localhost:7331/api/agent/items/full?id=<id>&section=判断接口是否触发回传的步骤&budget=4000'
```

不要在查询阶段批量读取 `items/*.md`。

## Write Workflow

> **必须显式传 `root`**：`POST /api/knowledge` 不传 `root` 且 `scope=project` 时，服务端会取第一个 project storage root（可能是 agent-panel 自身仓库），导致 WMS 条目误落到 `~/Developer/tools/agent-panel/.agents/` 下。写 WMS 知识/经验时永远带 `"root": "/home/hevin/Developer/company/WMS"`；更新已有条目（带 `id`）会按 ID 定位原地更新，不受此影响。

新增业务知识：

```bash
curl -sS -H 'Content-Type: application/json' \
  -X POST http://localhost:7331/api/knowledge \
  -d '{
    "kind": "businessKnowledge",
    "root": "/home/hevin/Developer/company/WMS",
    "title": "WMS 出库单状态流转",
    "domain": "wms",
    "project": "WMS",
    "scope": "project",
    "category": "biz",
    "status": "active",
    "confidence": "medium",
    "tags": ["outbound", "shipment", "status"],
    "triggerTerms": ["出库单状态", "shipment status"],
    "relatedSkills": ["wms-test-api-call"],
    "relatedRepos": ["yl-cwhsea-wms-web"],
    "relatedTables": ["shipment_header"],
    "relatedApis": ["POST /wms-web/outbound/shipment/dispatch"],
    "summary": "说明出库单核心状态及常见流转入口。",
    "details": "## 概述\n\n..."
  }'
```

新增经验：

```bash
curl -sS -H 'Content-Type: application/json' \
  -X POST http://localhost:7331/api/knowledge \
  -d '{
    "kind": "experience",
    "root": "/home/hevin/Developer/company/WMS",
    "title": "WMS 路由前缀导致 302",
    "domain": "wms",
    "project": "WMS",
    "scope": "project",
    "category": "pitfall",
    "status": "active",
    "confidence": "medium",
    "tags": ["route", "gateway"],
    "triggerTerms": ["HTTP 302", "请求没到 Controller", "路由前缀"],
    "summary": "排查 WMS 接口 302 或请求未到 Controller 时优先确认网关和应用前缀。",
    "details": "## 现象\n\n..."
  }'
```

更新已有条目时带 `id`。如果不传 `details`，API 会保留现有 `items` 正文，只更新 meta 字段。

## Bulk Relation Enrichment

WMS 已有知识可用 agent-panel 仓库脚本批量补齐结构化关系字段：

```bash
cd /home/hevin/Developer/tools/agent-panel
python3 scripts/enrich-wms-knowledge-relations.py --limit 10      # dry-run 预览
python3 scripts/enrich-wms-knowledge-relations.py --write         # 写回 meta 并重建 index.jsonl
```

脚本会扫描 `$WMS_WORKSPACE_ROOT/.agents/business-knowledge/items/` 与 `.agents/experiences/items/`，按规则抽取：

- `related_repos`：`yl-cwhsea-wms-*` / `yl-cwh-wms-*` 仓库或应用名。
- `related_tables`：SQL / 反引号 / 相关表章节中的 snake_case 表名。
- `related_apis`：`GET|POST|PUT|DELETE|PATCH /...` 或相关文档里的 `api-*` / `biz-*` / `link-*` 条目 ID。
- `related_skills`：显式 skill 引用和维护的关键词→skill 映射。

运行 `--write` 后必须抽样检查 meta 与 `index.jsonl` 是否一致，并用 `/api/agent/knowledge/query` 验证 related 字段可命中。

## Fallback File Rules

当 Agent Panel 不可用且必须离线处理时：

1. 只先搜索 `meta/*.yaml` 或 `index.jsonl`。
2. 命中后再读取 `source_path` 对应的 `items/*.md`。
3. 新增条目时同时写：
   - `meta/<id>.yaml`
   - `items/<id>.md`
   - 必要时重建 `index.jsonl`
4. 不要恢复旧的 `knowledge/wms/*.md` 作为主入口。

## Required Checks

- 查询类任务：最终说明使用了哪些 `id`，是否展开全文/章节。
- 写入类任务：确认 `meta` 和 `items` 都存在，`source_path` 可解析；**新增条目必须检查返回的 `metaPath` 落在目标项目 root 下**（如 `/home/hevin/Developer/company/WMS/.agents/...`），发现落在 agent-panel 自身仓库立即用 `trash-put` 清理重写；批量补齐关系字段后确认 `index.jsonl` 已重建。
- WMS 任务：不要使用已弃用的 `.agents/knowledge/wms-test` / `.agents/knowledge/wms-graph` 作为默认入口。
- 服务不可用：报告 `AGENT_PANEL_DOWN`，再按 fallback 文件规则处理。

## Final Response

- 简述查询/写入结果。
- 列出命中的知识/经验 `id`。
- 如写入，列出 `meta` 和 `items` 路径。
- 如未展开全文，说明“仅使用 meta 摘要”。
