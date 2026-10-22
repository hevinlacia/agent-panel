# Knowledge API（业务知识/经验查询维护）

> 上级入口：`../SKILL.md`。默认只取 meta 摘要，需要时展开 items 正文。

# Agent Panel Knowledge API

用于：通过本机 Agent Panel API 查询和维护 `.agents/business-knowledge/` 与 `.agents/experiences/`，让 agent 默认读取低 token 的属性/概述，再按需展开正文。

适用：
- 用户问“业务知识/经验在哪里查”“查一下经验”“查 WMS 业务知识”“有没有相关踩坑”。
- WMS 或其他项目已使用 `.agents/business-knowledge/`、`.agents/experiences/` 管理知识/经验。
- agent 需要查询多条知识候选，但不应直接读大量 Markdown 正文。
- 需要新增或更新一条业务知识/经验，并自动维护 `created_at` / `updated_at`。

不适用：
- 稳定、通用、可执行的长期流程；这类内容应沉淀为 skill。
- WMS 测试造数、API 模板、状态图、finder/verifier/recipe；用 WMS 项目内的 `$WMS_WORKSPACE_ROOT/.agents/testdata/`，或用 Agent Panel `GET /api/capabilities?project=WMS` 检索能力。
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
  index.jsonl     # 检索缓存（新版 Agent Panel POST /api/knowledge 会同步写入/更新对应行；API 查询不依赖它，动态扫 meta/）

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

默认地址：`http://agent-panel.tail260d93.ts.net:8788`。

| 用途 | 方法 | 端点 | 说明 |
|---|---|---|---|
| **知识库索引** | GET | `/api/knowledge/index`（别名 `/api/agent/knowledge/index`） | 面向 agent 的轻量全量列表 + facets 计数；支持 `kind/category/domain/tag/scope/status/q` 过滤与 `verbose=1`，见下方「知识库索引」节 |
| 查询知识/经验候选 | POST | `/api/agent/knowledge/query` | 默认返回 **lean 条目**（`{id,title,kind,category,domain,score,whyMatched≤80字,summary≤160字,updatedAt,stale}`，省 ~90% token）；`verbose=true` 回全量（含 outline/matches/matchedFields）；body 支持 `kind/intent/domain/category/tag/scope/status/observedAfter/observedBefore/limit/tokensBudget/verbose` |
| 获取单条摘要 | GET | `/api/agent/items/summary?id=<id>` | 只取属性/概述 |
| 获取单条全文 | GET | `/api/agent/items/full?id=<id>&budget=20000` | 读取 `source_path` 指向正文，按 budget 截断 |
| 获取单条章节 | GET | `/api/agent/items/full?id=<id>&section=<heading>&budget=4000` 或 `/api/agent/items/section?id=<id>&section=<heading>` | 只读取指定 Markdown heading 下的章节 |
| 页面/通用列表 | GET | `/api/knowledge?kind=&q=&domain=&category=&tag=&scope=&status=&limit=` | 给 UI 或调试使用（已支持 category/tag/scope/status 过滤） |
| 读取单条 | GET | `/api/knowledge/item?id=<id>` | 返回单条，通常含全文 |
| 新增/更新（编辑） | POST | `/api/knowledge` | JSON body，写入 `meta/` + `items/`；带已有 `id` = 原地更新（**不传 `details` 保留正文，只改 meta**） |
| 仅编辑 meta | PATCH | `/api/knowledge/item?id=<id>` | 同 POST 但语义为更新已有条目 |
| 删除 | DELETE | `/api/knowledge?id=<id>` | 删 meta+正文+索引行 |

**响应键名统一（2026-10-21 起）**：三个列表接口统一用 `items` 键（query / index / list）；单条接口统一 `item`。旧键 `results`（query）/`entries`（index）已移除，解析响应直接取 `.items`。 | | | |

## Agent 能力矩阵（提取/筛选/查看/编辑）

| 能力 | 端点 | 要点 |
|---|---|---|
| 提取（语义检索） | `POST /api/agent/knowledge/query` | `intent` 自然句（中文自动 n-gram），返回 score/whyMatched；默认 lean 条目（~600 tok/5条），需要 outline/matches 时传 `verbose=true` |
| 筛选（结构化） | `GET /api/knowledge/index?kind=&category=&domain=&tag=&scope=&status=&q=` | 先看 `counts` facets 分布再收窄；`q` 对 id/title/summary/tags 子串匹配（大小写不敏感）；条目默认不含服务端路径，id 即句柄 |
| 查看（摘要/全文/章节） | `GET /api/agent/items/summary\|full?id=` | outline 已知章节时优先 `&section=<heading>` 省 token |
| 编辑（新建/更新） | `POST /api/knowledge`（带 `id`） | 不传 `details` = 只改 meta；新建务必显式传语义化 `id` |
| 删除 | `DELETE /api/knowledge?id=` | 同步清理 index.jsonl 行 |

## 知识库索引

`GET /api/knowledge/index` 返回全库轻量索引（不受 query 端点的 budget/limit=20 钳制；2026-10-20 后已按 id 去重，同一条目不再因多发现根重复出现）：

```jsonc
{
  "generatedAt": 1792510040910,
  "total": 426,
  "skippedCorrupt": 0,          // 坏 meta 文件只跳过并计数，不拖垮索引
  "counts": {                    // facets：先看分布再决定下一步筛选
    "kinds": {"businessKnowledge": 189, "experience": 237},
    "categories": {"biz": 59, "pitfall": 215, "link": 40, "profile": 32, "conventions": 31, "ref": 27, "api": 17, "...": 0},
    "domains": {"wms": 386, "bootstrap": 10, "agent-system": 9, "infra": 12, "...": 0},
    "scopes": {"project": 418, "global": 8}
  },
  "items": [                     // 统一列表键（2026-10-21 起，旧键 entries 已移除）；紧凑条目，按 updatedAt 倒序
    { "id": "biz-wms-outbound-status", "kind": "businessKnowledge", "category": "biz", "domain": "wms",
      "scope": "project", "title": "WMS 出库单状态流转", "summary": "…（≤160 字符）",
      "status": "active", "confidence": "medium", "updatedAt": "…", "tags": ["…"] }
    // 默认不含 sourcePath/metaPath（服务端容器路径，agent 不可读；id 即句柄）
    // verbose=1 时附 sourcePath/metaPath/triggerTerms/related*/全量 summary
  ]
}
```

- 过滤参数：`kind`（businessKnowledge/experience）、`category`、`domain`、`tag`（tags 数组精确匹配）、`scope`（project/global）、`status`、`q`（id/title/summary/category/domain/tags 子串）、`limit`（默认 500，上限 2000）、`verbose=1`（附 `sourcePath`/`metaPath`/`triggerTerms`/`related*`/全量 summary）。
- 别名 `/api/agent/knowledge/index` 同义；agent 取索引优先用本端点，语义检索再走 query。

`kind` 可取：

```text
businessKnowledge / experience
```

## Query Workflow

1. 先确认 Agent Panel 可用：

```bash
curl -sf --max-time 3 http://agent-panel.tail260d93.ts.net:8788/health >/dev/null \
  && echo AGENT_PANEL_OK || echo AGENT_PANEL_DOWN
```

2. 使用 `POST /api/agent/knowledge/query` 查询候选。默认限制 3-5 条，避免 token 膨胀。响应在 `items` 键下，默认 **lean 条目**（`{id,title,kind,category,domain,score,whyMatched≤80字,summary≤160字,updatedAt,stale}`，5 条约 2KB）；需要 outline/matches/matchedFields/related*/路径字段时传 `verbose=true` 换全量条目。`tokensBudget` 影响 lean 模式的 summary 长度与默认 limit；verbose 模式下还会影响 outline 数量。

支持中文自然句查询：后端会对连续中文做 n-gram 扩展，例如“出库单回传为什么没发mq”无需手动加空格也能命中“出库单 / 回传 / MQ”等相关知识。

```bash
curl -sS -H 'Content-Type: application/json' \
  -X POST http://agent-panel.tail260d93.ts.net:8788/api/agent/knowledge/query \
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
  -X POST http://agent-panel.tail260d93.ts.net:8788/api/agent/knowledge/query \
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
curl -sS 'http://agent-panel.tail260d93.ts.net:8788/api/agent/items/full?id=<id>&budget=20000'
```

如果 query 返回的 `outline` 已经显示需要的章节，优先只取 section，节省 token：

```bash
curl -sS 'http://agent-panel.tail260d93.ts.net:8788/api/agent/items/full?id=<id>&section=判断接口是否触发回传的步骤&budget=4000'
```

不要在查询阶段批量读取 `items/*.md`。

## Write Workflow

> **必须显式传 `root`（hub 容器内路径）**：hub 自 2026-10-20 起容器化部署，root 安全校验要求落在服务进程 home（`/root`）内，传宿主机路径（`/home/hevin/...`）会被 400 拒绝。写 WMS 知识/经验时永远带 `"root": "/root/agent-panel-data/company/云仓/WMS"`（容器内 `/root/agent-panel-data` 软链指向 data clone `/srv/agent-panel/data`）；更新已有条目（带 `id`）会按 ID 定位原地更新，不受此影响。跨项目全局知识用 `"scope": "global"` 且不传 `root`（落在 data clone 全局 `.agents/{business-knowledge,experiences}`）。

> **无 dry-run**：当前部署版本 `POST /api/knowledge` 不支持 `dry_run` 预览（传了也会真写）；预览落点前先确认 root 正确，写错用 `DELETE /api/knowledge?id=<id>` 清理。

> **必须显式传 `id`（避免退化 id）**：新增 WMS 知识/经验时**务必显式传 `id`**（ASCII 字母数字 + `-`/`_`/`.`，如 `biz-wms-cycle-count-inventory-adjust`）。
> 原因：`POST /api/knowledge` 未传 `id` 时，服务端按 `{prefix}-{domain}-{titleSlug}` 自动生成 id，而 slug 化只保留 ASCII 字符，**中文标题会自动退化成只剩 domain**（如标题"WMS 盘点账实调整落表链路"→ `biz-wms-wms`），id 无语义、易混淆、不便引用。显式传 `id` 可得到稳定语义化 id，后续更新/引用也更可靠。

> **index.jsonl 同步行为（2026-09-18 实测）**：新版 Agent Panel `POST /api/knowledge` 在写 `meta/` + `items/` 的同时会**自动写入/更新 `index.jsonl` 对应行**（含 tags/triggerTerms/summary 等字段），本文早期“POST 不更新 index.jsonl”的描述已过时。API 查询动态扫 `meta/`，不依赖索引；保存后用 `grep <id> index.jsonl` 确认行已生成，缺失（旧版本）时幂等 append 一行（参考已有条目 JSON 行格式）或跑 `enrich-wms-knowledge-relations.py --write` 重建。注意：API 自动维护索引可能连带刷新其他条目的行（updatedAt 变化、补齐历史缺失行），git 提交前用 `git diff` 按条目 id 分离本任务行，只 stage 相关变更，不要整文件混提。

> **无 DELETE 端点**：清理错误/过期条目 = `trash-put meta/<id>.yaml items/<id>.md`，顺手从 `index.jsonl` 移除对应行（如已 append）。

新增业务知识：

```bash
curl -sS -H 'Content-Type: application/json' \
  -X POST http://agent-panel.tail260d93.ts.net:8788/api/knowledge \
  -d '{
    "kind": "businessKnowledge",
    "root": "/root/agent-panel-data/company/云仓/WMS",
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
  -X POST http://agent-panel.tail260d93.ts.net:8788/api/knowledge \
  -d '{
    "kind": "experience",
    "root": "/root/agent-panel-data/company/云仓/WMS",
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
- 写入类任务：确认 `meta` 和 `items` 都存在，`source_path` 可解析；**新增条目必须检查返回的 `metaPath` 落在目标项目 root 下**（如 `$WMS_WORKSPACE_ROOT/.agents/...`），发现落在 agent-panel 自身仓库立即用 `trash-put` 清理重写；新增后确认 `index.jsonl` 已含该 id（新版自动写入，缺失时幂等 append）。
- 写入类任务：确认 `id` 语义化（非 `biz-wms-wms` 这类退化 id）；若因未显式传 `id` 产生退化 id，用显式 id 重建并 `trash-put` 清理旧条目（含 index.jsonl 对应行）。
- WMS 任务：不要使用已弃用的 `.agents/knowledge/wms-test` / `.agents/knowledge/wms-graph` 作为默认入口。
- 服务不可用：报告 `AGENT_PANEL_DOWN`，再按 fallback 文件规则处理。

## Final Response

- 简述查询/写入结果。
- 列出命中的知识/经验 `id`。
- 如写入，列出 `meta` 和 `items` 路径。
- 如未展开全文，说明“仅使用 meta 摘要”。
