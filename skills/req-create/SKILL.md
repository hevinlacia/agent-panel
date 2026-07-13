---
name: req-create
description: 创建和更新需求文件（meta.md/background.md/branch.md/notes.md/test.md/config-changes.md/state.json），确保目录结构、frontmatter 格式、父子关系和状态值符合 Agent Panel 解析规范。
allowed-tools: ["bash", "read", "write", "edit", "glob", "grep"]
---

# Requirement File Create & Update

用于：当需要创建新需求、更新需求信息或变更需求状态时，按统一规范生成和修改 `~/.agents/req/` 下的需求文件，确保 Agent Panel 能正确解析。

适用：
- 用户说"创建需求"/"新建需求"/"登记需求"时生成目录和初始文件
- 用户说"更新需求"/"改需求状态"/"补充需求信息"时修改已有文件
- 用户说"创建子需求"/"拆分子需求"时在父需求目录下新建子需求

不适用：
- 需求开发到上线的全流程跟踪和发布预检（走 `req-tracker` skill）
- session 绑定（用 `req-session-bind`）；状态/API 调用细节（用 `agent-panel-requirement-api`）
- 代码实现、仓库探索、调用链分析

## Directory Layout

需求根目录：`~/.agents/req/`

两种合法布局：

```text
# 项目分组布局（推荐）
~/.agents/req/
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
│   │   ├── review.md
│   │   └── state.json
│   └── WMS-003-rabbitmq-to-rocketmq/   # 父需求（分组容器）
│       ├── meta.md                      # 父需求自身 meta
│       ├── background.md                # 父需求背景
│       ├── WMS-003-after-picking-batch/ # 子需求
│       │   ├── meta.md
│       │   ├── branch.md
│       │   └── notes.md
│       └── WMS-003-stock-diff-adjust/   # 子需求
│           └── meta.md

# 旧版平铺布局（顶层直接放需求，无项目分组）
~/.agents/req/
├── legacy-req/
│   └── meta.md
```

### 父需求 vs 叶子需求

- **父需求**：目录里有 `meta.md` 且包含子目录（子目录有自己的 `meta.md`）。父需求只是一个分组容器，和项目目录作用一样。状态无意义，Agent Panel 不显示状态徽章和状态切换器。
- **叶子需求**：目录里有 `meta.md` 且不包含子需求目录。叶子需求有状态、session 绑定、上下文注入等完整功能。
- **子需求**：位于父需求目录下的叶子需求，`parentReqId` 由扫描器自动设置。

## File Specs

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

**frontmatter 字段规则：**

| 字段 | 必填 | 类型 | 说明 |
| --- | --- | --- | --- |
| `req-id` | 是 | string | 与目录名一致，ASCII 字母数字和连字符 |
| `title` | 是 | string | 一句话标题，30 字以内 |
| `status` | 是 | enum | 见下方状态值表 |
| `project` | 否 | string | 显示名覆盖；默认取父目录名 |
| `owner` | 否 | string | 负责人 |
| `start-date` | 否 | string | `YYYY-MM-DD` 或 `unknown` |
| `plan-release` | 否 | string | `YYYY-MM-DD` 或 `unknown` |

**状态值（7 个，严格匹配 Agent Panel）：**

| 值 | 含义 |
| --- | --- |
| `需求对齐` | 业务目标、范围和验收口径对齐中 |
| `方案设计` | 技术方案、影响面和验证路径设计中 |
| `开发中` | 正在开发 |
| `自测中` | 开发完成，开发自测 |
| `测试中` | 已提交测试，测试中 |
| `待上线` | 测试通过，等待上线 |
| `已完成` | 已上线，需求关闭 |

### background.md（推荐填写）

需求背景文件。`buildInjectionContext` 会读取此文件（最多 500 字符）注入到新 session 的上下文中。

```markdown
# 需求背景

## 目标
<这个需求要解决什么问题>

## 背景
<为什么要做这个需求，当前系统的什么状况需要改变>

## 范围
- 仓库：<仓库路径>
- 分支：<分支名>
- 关键改动文件：<文件列表>
- 测试文件：<文件列表>

## 关键决策
- <重要的技术决策、方案选择及原因>
```

### branch.md（推荐填写）

```markdown
# <req-id> Branches

| Item | Value |
| --- | --- |
| Source branch | <分支名> |
| Target branch | <目标分支> |
| Project path | <仓库路径> |
| Merge status | <开发中 / 已合并 / unknown> |

## Commit / Diff Notes
- 关键 commit: `<sha>` — <描述>
- 改动文件:
  - `<file path>` — <改动说明>
```

### memory.md（推荐填写）

需求生命周期记忆，供 Agent Panel 新 session 注入和智能提取维护。记录当前目标、当前进展、关键决策、已完成改动、待办/风险、影响范围和 session 摘要索引。

### impact.md（推荐填写）

编码前影响面评估，供 Agent Panel 和 agent 判断本次改动是否影响 WMS 入库、库存、出库、复核、发运、回传等核心链路。应记录风险等级、核心链路、影响入口、数据影响、阻塞风险、自测清单和回滚方案。

### review.md（按需）

待上线 Code Review 文件。只在本次需求包含待上线 review、用户确认的 review 处理结论或复查结果时维护。

### notes.md（按需）

```markdown
# <req-id> Notes

## 当前状态
- <当前进展、已知问题>

## 待跟进
- [ ] <待办项>
```

### test.md（按需）

测试场景清单 + 分阶段执行记录。支持两阶段测试工作流：开发自测（test 环境）和上线前 UAT 回归。WMS 需求应按 `~/.agents/knowledge/wms/conventions-wms-agent-self-test-evidence.md` 维护日志、DB、副作用和反向证据标准，并在自测结论中标注 A/B/C/D 置信度。

```markdown
# <req-id> Test

## 测试场景清单

| ID | 场景描述 | 触发方式 | 前置条件 | 预期结果 | 证据标准 |
| --- | --- | --- | --- | --- | --- |
| S1 | <一句话描述测试场景> | <API/UI/Job/MQ/curl> | <依赖数据或状态> | <预期行为和验证点> | <日志 + DB + 副作用 + 反向检查> |
| S2 | <...> | <...> | <...> | <...> | <...> |

## 日志关键字

| 类型 | 关键字 | 说明 |
| --- | --- | --- |
| 链路 | `tid=<tid>` | 首选链路关键字，必须能串起入口、关键分支、成功/失败 |
| 正常 | <正常链路日志关键词，如 "MQ消息发送" "MQ消息消费成功"> | <出现在哪条链路、什么阶段> |
| 异常 | <异常/错误日志关键词，如 "MQ消息消费失败" "rollback" "异常"> | <出现原因和排查方向> |

## DB / 副作用验证标准

| 场景 | 表/副作用 | 查询条件 | 预期结果 | 反向检查 |
| --- | --- | --- | --- | --- |
| S1 | <table/topic/外部target> | <bizNo/warehouse/timeRange> | <字段/数量/状态符合预期> | <无 ERROR/consumeFail/重复记录/误跳过> |

## 自测记录（test 环境）

### S1: <场景描述> — ⬜
- 触发: <实际执行的命令或操作>, tid=<tid>
- 日志: <Kibana 关键词或日志片段，确认 tid 链路走通>
- DB: <SQL 查询方向和结果摘要，确认数据变化>
- 副作用: <MQ/外部调用/DTS/回传，如无写“不涉及”>
- 反向检查: <同 tid / bizNo / topic 下无 ERROR、Exception、consumeFail、rollback>
- 置信度: <A/B/C/D>
- 结果: <✅ 通过 / ❌ 失败原因 / ⬜ 待测 / 证据不足>

### S2: <场景描述> — ⬜
- 触发: <...>
- 日志: <...>
- DB: <...>
- 结果: <...>

## UAT 回归记录

> 部署 master 到 UAT 后，逐场景回归。全 ✅ 方可上线。

### S1: <场景描述> — ⬜
- 结果: <✅ / ❌ / ⬜>

### S2: <场景描述> — ⬜
- 结果: <✅ / ❌ / ⬜>

## 回归范围
- <需要回归验证的模块或功能点>

## 注意事项
- <部署顺序、依赖项、已知风险等>
```

**各阶段填写时机：**

| 阶段 | 填写内容 | 触发时机 |
| --- | --- | --- |
| 测试场景清单 | 列出所有需要验证的场景 | 进入「自测中」状态前 |
| 日志关键字 | 正常/异常链路的日志搜索关键词 | 与场景清单一起填写 |
| 自测记录 | 每个场景的触发命令、日志、DB 验证结果 | 自测过程中逐条更新 |
| UAT 回归记录 | 每个场景在 UAT 环境的通过/失败 | 上线前 UAT 部署后 |
| 回归范围 + 注意事项 | 需要回归的模块、部署风险 | 与场景清单一起填写 |

**状态标记规则：**
- `⬜` 待测 — 尚未执行
- `✅` 通过 — 至少达到 B 级证据；A 级表示日志、DB、副作用和反向检查完整
- `❌` 失败 — 记录失败原因和复现步骤
- `证据不足` — 只有接口成功或单点日志/DB，链路不完整，不能认为测试到位

### config-changes.md（按需）

```markdown
# <req-id> Config Changes

## DB 变更
| 类型 | SQL | 备注 |
| --- | --- | --- |
| DDL | <SQL> | <说明> |

## Apollo / Nacos 变更
| Namespace | Key | 值 | 环境 |
| --- | --- | --- | --- |
| <ns> | <key> | <value> | <env> |
```

### state.json（由 Agent Panel 管理，不要手写）

状态变更后由 Agent Panel 的 `POST /api/requirement/status` 自动写入。手动创建需求时无需生成此文件，Agent Panel 会在首次状态切换时自动创建。

## Workflow

### 创建新需求

1. 确认信息：`req-id`、`title`、`project`（项目目录名）、是否为父需求（是否要包含子需求）
2. 确认目录路径：
   - 项目分组：`~/.agents/req/<project>/<req-id>/`
   - 旧版平铺：`~/.agents/req/<req-id>/`
3. 创建目录：`mkdir -p <path>`
4. 生成 `meta.md`（必填，按模板）
5. 生成 `background.md`（推荐，至少写目标和范围）
6. 生成 Agent Panel 维护的需求事实文件：`memory.md`、`branch.md`、`config-changes.md`、`impact.md`、`test.md`、`notes.md`、`review.md`
7. 文件可先写空模板或占位内容，但不要写真实 token、密码、Cookie、私钥、完整敏感 header
8. 不要创建 `state.json`，Agent Panel 会在首次状态切换时自动生成

### 创建子需求

1. 确认父需求目录已存在且有自己的 `meta.md`
2. 子需求创建在父需求目录下：`~/.agents/req/<parent-project>/<parent-req-id>/<child-req-id>/`
3. 子需求的 `meta.md` 中 `project` 字段与父需求保持一致
4. 其余步骤与创建普通需求相同

### 更新需求状态

状态变更统一走 `agent-panel-requirement-api`，不要直接改 `meta.md` 或 `state.json`。常用命令：

```bash
curl -sS -H 'Accept: application/json' \
  -X POST http://localhost:7331/api/requirement/status \
  -d "reqId=<req-id>" \
  -d "status=<新状态>" \
  -d "note=<备注，可选>"
```

状态会写入 `state.json`，Agent Panel 读取时 `state.json` 优先于 `meta.md` frontmatter。

### 更新需求文件

直接编辑对应文件即可。`background.md`、`memory.md`、`branch.md`、`config-changes.md`、`impact.md`、`test.md`、`notes.md`、`review.md` 都是普通 Markdown，无特殊解析要求；Agent Panel 智能提取会读取并维护这些文件。`state.json` 由 Agent Panel 管理，不要手写。

## Required Checks

- `req-id` 只用 ASCII 字母数字和连字符，不含空格、中文、路径分隔符、`..`
- `meta.md` frontmatter 的 `status` 严格匹配 7 个值之一
- `meta.md` frontmatter 的 `req-id` 与目录名一致
- 不要在文件里写真实 token、密码、Cookie、私钥
- 不要手动创建 `state.json`（Agent Panel 自动管理）
- 创建子需求前确认父需求目录和 `meta.md` 已存在

## Final Response

创建完成：

```text
✅ 已创建: <path>
- 需求: <title>
- 状态: <status>
- 已生成: <文件列表>
- 待补: <哪些文件还需要用户填>
```

更新完成：

```text
✅ 已更新: <file>
- 变更: <简述>
```

状态变更：

```text
✅ <req-id> 状态已变更
- <旧状态> → <新状态>
- 备注: <note>
```
