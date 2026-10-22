# Requirement API（需求 CRUD/状态/合并/检查服务）

> 上级入口：`../SKILL.md`。需求业务工作流见 `req` skill。

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
- Agent Panel 不可用且用户仍要求离线创建/重写需求文件模板（fallback 用组内 `create.md`）
- 批量发布前检查具体分支/配置/测试证据（组内 `release.md`）
- 直接修改 `state.json`、`meta.md` 的状态字段或真实业务代码

## Trigger

- “更新需求状态” / “推进状态” / “设为经验总结” / “设为已完成”
- “合并到 test” / “合并到 UAT” / “需求分支合并” / “需求代码合并到 test” / “合并到 test 分支” / “代码都合并 test 了吗” / “merge branch” / “冲突处理”
- “查 Agent Panel 需求 API” / “requirements API” / “需求状态接口”
- “把 session 关联到需求”且需要 API 路径或自动执行
- agent 已知要调用本机需求 API，不应再搜索项目源码

## 术语和边界

- 产品/页面统一称 **Agent Panel**。
- 本机默认地址：`http://agent-panel.tail260d93.ts.net:8788`，可由 `PORT` 覆盖。
- 本机服务为 systemd user unit `agent-panel.service`（历史名称 `opencode-dashboard.service` 已停用，仅老文档提及）。
- API 契约以本 skill 为准；除非接口返回 404/400 且怀疑版本不一致，否则不要先去翻 `src/requirement_api.rs` 等源码。

## API Contract

| 用途 | 方法 | 端点 | 说明 |
|---|---|---|---|
| 查协议 schema | GET | `/api/requirement/schema` | 返回稳定 token、intent、允许操作和推荐流程 |
| 获取编辑计划 | GET | `/api/requirement/edit-plan?id=<reqId>&intent=<intent>` | 返回本 intent 应读/写的 token、文件路径、大小和示例写法 |
| 获取低 token 上下文 | GET | `/api/requirement/context?id=<reqId>&intent=<intent>&budget=2000` | 按 intent 和 budget 返回压缩上下文；可选 `tokens=req.meta,req.test` |
| 获取 agent 专用上下文 | GET | `/api/requirement/context?id=<reqId>&for=agent&intent=<intent>&budget=2000` | 返回 `phaseRuntime`（当前状态独有提示词 `currentPhasePrompt`、`entryChecks`、跳状态风险 `phaseGaps`、状态历史 `transitionMemory`）、摘要文档、最近结构化事件、推荐写入 API；优先用于继续需求工作 |
| 记录结构化事件 | POST | `/api/requirement/events` | JSON body: `reqId`, `type`, `summary`; 可选 `details`, `evidence`, `decisions`, `todos`, `testCases`, `triggerTerms`, `relatedFiles`, `relatedKnowledgeIds`, `relatedRepos`, `relatedTables`, `relatedApis`, `tags`, `candidateType`, `dedupeKey`, `confidence`, `target`, `riskLevel`, `idempotencyKey`, `appendNote`, `notesMode`, `dryRun`；写 `events.jsonl`，默认同步追加 notes；数组字段必须传数组；未知字段直接 400（报错带合法字段清单）；type 拼错不拒绝但响应带 `unknownType: true`+knownTypes 警告；并发写已由服务端串行化，无需自行保证顺序 |
| 事件误发清理 | - | 无 DELETE | `/api/requirement/events` 只注册了 POST，`DELETE` 返回 405（其他方法错配会带 allowed methods 提示）；误发只能改文件兜底，见下方「events 字段类型与误发清理」 |
| 章节 upsert 别名 | POST/PATCH | `/api/requirement/sections/{section}` | JSON body: `reqId`, `content`; 可选 `heading`, `docType`, `token`, `dryRun`。section→docType 映射：`impact`/`risk`/`boxCodeIssue`→impact、`test`/`selftest`/`uat`→test、`background`/`design`→background、`root-cause`/`rootCause`→**root-cause**、`incident`/`issue`→**incident**、`memory`/`summary`→memory、`config`→config-changes、`technical-plan`/`solution`→technical-plan、`release`→release-manifest、`review`→review、`notes`/`progress`→notes；**未知 section 返回 400 并列出可用 key**（INC-127 修复：`root-cause` 曾被旧别名静默写入 impact.md，勿再依赖旧行为） |
| 结构化编辑 | POST | `/api/requirement/edit` | JSON/form body: `reqId`, `operation`; 支持 `setStatus`, `setCategory`, `patchMeta`, `appendNote`, `writeDoc`, `upsertSection` |
| 创建需求 | POST | `/api/requirements` | JSON/form body: `reqId`, `title`; 可选 `project`, `projects`, `status`, `category`, `source`, `issues`, `owner`, `startDate`, `planRelease`, `ones`, `region`, `summary`, `background`, `notes`, `parentReqId`, `root`, `groupPath`, `members`, `releasePolicy`, `consolidated`, `dryRun`；`members` 非空 = 创建引用式需求组；`consolidated=true` = 创建整合需求（ROLLUP 池，见下方「整合需求与发布分支」章节）；`region` 仅线上问题/测试问题/性能问题可传（cn=中国 / sea=东南亚，可扩展更多国家/区域，值统一小写），普通需求传入返回 400；响应 `sourceDefaulted:true` 表示 source 走了默认「产品推动」（未显式传），`reqDir`/`files` 是**容器内绝对路径**（本机不可直接用，映射用 `relDir` 仓库相对路径） |
| 查单需求 | GET | `/api/requirement?id=<reqId>` | 返回 `{ requirement }` |
| 更新受控字段 | PATCH | `/api/requirement` | JSON/form body: `reqId`; 可选 `title`, `project`, `projects`, `status`, `category`, `source`, `ones`, `issues`, `owner`, `startDate`, `planRelease`, `region`, `note`, `dryRun`；`region` 仅 issue 家族（Some(值) 设置 / Some("") 清空）；兼容旧 skill，优先用 `/api/requirement/edit` |
| 更新受控字段兼容 | POST | `/api/requirement/update` | 同 PATCH `/api/requirement`，给不方便发 PATCH 的 agent/client 使用 |
| 追加 notes | POST | `/api/requirement/notes` | JSON/form body: `reqId`, `text`; 可选 `title`, `sessionId`, `dryRun`；兼容旧 skill，优先用 `/api/requirement/edit` + `operation=appendNote` |
| 写受控文档 | PUT/POST | `/api/requirement/doc` | JSON/form body: `reqId`, `docType`, `content`; 可选 `mode=replace|append`, `dryRun`；兼容旧 skill，优先用 `/api/requirement/edit` + `writeDoc/upsertSection` |
| 校验需求 | POST | `/api/requirement/validate` | JSON/form body: `reqId`，返回 problems/warnings/files |
| **Agent 文件直写** | GET/PUT | `/api/requirement/file` | 边角文档/问题档案唯一可靠读写路径（本机镜像有滞后）；GET: `id`+`path` → `content`+`baseSha`（不存在时 `exists:false`+`baseSha:null`）；PUT: `id`+`path`+`content`+`baseSha`（**兼容别名 `sha`**），409 冲突重新 GET；详见下方「Agent 文件直写」节 |
| 获取合并分支选项 | GET | `/api/requirement/merge-options?id=<reqId>` | 返回前端/后端可选环境分支和按需求状态计算的默认选中值 |
| 合并需求分支 | POST | `/api/requirement/merge-branch` | JSON/form body: `reqId`, `repoKind=frontend|backend`, `targetBranch=<branch>`；可选 `target=test|uat` |
| 查询合并/冲突状态 | GET | `/api/requirement/merge-status?id=<reqId>&target=test|uat` | 返回未完成 merge worktree、冲突文件和状态 |
| 查询代码审查门禁 | GET | `/api/requirement/review-gate?id=<reqId>` | 返回 `PASS/BLOCKED/WAIVED/missing/pending/stale/skill-required/checklist-*`；推进到测试中前必须通过或豁免 |
| 跨状态合规快照 | GET | `/api/requirement/compliance?id=<reqId>` | 返回状态历史（含按状态序重算的 `missedStatuses` 与逐次 `gateCheck`）+ 各契约产物齐缺（selftestChecklist/uatRegression/reviewGate/releaseCheck/attachments）+ 可执行 `gaps` 清单；**中途接手的新会话/release-check/人工核查材料/经验总结先拉它**，不需要读过所有 phase prompt 即可验收上游产物 |
| 一键备审查材料 | POST | `/api/requirement/review-materials` | JSON body: `reqId`；可选 `mode=full|incremental|auto`（缺省按需求状态：发布就绪前全量、发布就绪起增量）；返回 `materialPath` + `diffPatchFile` + `handoffHints`，reviewer 直接 read，不要自己跑 git；快照落盘后**直传 hub**
**快照定位（2026-10-21 拍板）**：快照是**给人在面板上看的展示产物**（diff 页/备注锚定）+ 门禁的锚定基线（reviewedCommit 指纹），**审查 agent 本身不依赖快照内容**——审查在能访问代码库和公司 GitLab 的 worker 机器上直接读仓库进行。因此快照必须预生成并提交 hub（人类查看时可能没有公司网络，不做实时生成）；hub 侧门禁判定基于提交的材料集内部一致性（结论文档 + annotations 指纹 vs 快照 + checklist），与 worker 仓库实时状态无关（hub 无仓库，分支漂移检查仅在 worker 备料决策时生效） |（`deliverWarning` 非空 = 活链路失败；`syncWarning` = git 备份同步失败，不影响门禁）——备料到门禁秒级新鲜，**工作链路不依赖 git**（git 仅作数据备份） |
| 列需求 | GET | `/api/requirements` | 返回 `{ requirements: [...] }`，客户端自行按 `status` 过滤 |
| 更新状态 | POST | `/api/requirement/status` | form body: `reqId`, `status`, 可选 `note`, `redirect` |
| 更新类别 | POST | `/api/requirement/category` | form body: `reqId`, `category` |
| 更新 ONES | POST | `/api/requirement/ones` | form body: `reqId`, `ones`；推动方/绑定线上问题无专用端点，走 PATCH `source` / `issues` 字段 |
| 关联 session | POST | `/api/requirement/associate` | form body: `reqId`, `sessionId`；需求组自动扩展绑定组 + 所有成员，返回 `boundReqIds` |
| 附件清单（只读） | GET | `/api/requirement/attachments` | query: `id`/`reqId`，可选 `budget`、`full=1`（全文上限 200KB）；只给摘要不带 sha |
| **附件读写** | GET/PUT | `/api/requirement/attachment` | 单附件读写唯一通道：GET `id`+`path` → `content`+`baseSha`；PUT `id`+`path`+`content`+`baseSha`（新建可省，409 冲突重 GET）；路径白名单 `attachments/<file>` 单层（裸文件名兼容），后缀限 sql/txt/md/yaml/yml/json/csv；详见「上线资产附件制」节 |
| 工时四联汇总 | GET | `/api/ones/manhour` | query: `id`/`reqId`，可选 `team`、`refresh=true`；见「ONES 工时联动」节 |
| 工时档案写入 | POST | `/api/ones/manhour/save` | `reqId` + `manualHours`/`agentHours`/`agentHoursNote`（null=清除，缺省=不改） |
| 工时登记清单 | POST | `/api/ones/manhour/log` | `reqId` + `entries:[{date,hours,remark}]`；准备模式不写 ONES，`execute=true` 当前必然失败不要传 |
| ONES 任务推荐 | GET | `/api/ones/tasks` | query: `reqId`（带则按标题推荐）、`team`、`refresh=true`；候选 refText 可直接写 meta.ones |
| 发布分支管理 | POST/GET | `/api/requirement/release-branch/*` | 整合需求发布分支 6 端点（create/list/merge-sub/sync-prod/prod-mr/mark-released），完整契约见 `release-branch-api.md` |
| 创建发布分支 | POST | `/api/requirement/release-branch/create` | JSON body: `reqId`（整合需求）；可选 `repos`（repoName 过滤）、`branchPrefix`（缺省从子需求分支推导，如 `hevin.yang/release`）、`label`；以各仓生产分支为 base 建 `&lt;派生前缀&gt;/release/&lt;reqId&gt;-r&lt;n&gt;` 并 push（branchPrefix 传参时完整覆盖派生前缀），序号自动递增且远端撞名自动跳号（见下方「整合需求发布分支」章节） |
| 列发布分支 | GET | `/api/requirement/release-branch/list?id=<reqId>` | 返回登记 + 每分支 mergedSubs（带子需求当前状态）与每仓相对生产分支 diff 统计 |
| 子需求合入发布分支 | POST | `/api/requirement/release-branch/merge-sub` | JSON body: `reqId`, `subReqId`, `confirm=true`；可选 `branch`（id/分支名，缺省最新 active）、`repos`；逐仓把子需求分支合入发布分支；纯配置子需求（无分支登记）仅登记随分支上线；成功后子需求自动推进（需求创建→开发中→已合入，更高状态保留） |
| 同步生产分支 | POST | `/api/requirement/release-branch/sync-prod` | JSON body: `reqId`, `confirm=true`；可选 `branch`；把各仓生产分支最新成果合入发布分支（冲突保留 worktree 在发布分支侧解决） |
| 发布分支生产 MR | POST | `/api/requirement/release-branch/prod-mr` | JSON body: `reqId`；可选 `branch`；按发布分支生成生产 MR（复用 prod-mrs 逻辑，reused 去重）；MR 由用户在 GitLab 人工合入 |
| 封版发布分支 | POST | `/api/requirement/release-branch/mark-released` | JSON body: `reqId`, `confirm=true`；可选 `branch`, `note`；标记 released 并把 mergedSubs 中未终态子需求推进「已发布」 |

### `/api/requirement/events` 字段类型与误发清理

字段类型（serde camelCase）：

| 类型 | 字段 |
|---|---|
| 字符串（单值） | `reqId`（必填）、`type`、`summary`（建议必填）、`title`、`details`、`candidateType`、`dedupeKey`、`confidence`、`target`、`status`、`riskLevel`、`sessionId`、`idempotencyKey` |
| 布尔 | `appendNote`（不传默认 true，同步追加 notes.md）、`dryRun` |
| 数组（字符串） | `evidence`、`decisions`、`todos`、`triggerTerms`、`relatedFiles`、`relatedKnowledgeIds`、`relatedRepos`、`relatedTables`、`relatedApis`、`tags` |
| 数组（对象） | `testCases`：`[{"name":"...","result":"pass|fail","evidence":"..."}]`（只有对象内部的 `evidence` 是字符串） |

- 数组字段必须传 JSON 数组：`"evidence":["tid=xxx"]` 正确；`"evidence":"tid=xxx"` 返回 400。
- **`type` 合法枚举（13 个，规范形驼峰）**：`issueFound`发现问题 / `rootCause`根因确认 / `workaround`治标方案 / `solution`方案落地 / `testResult`测试验证 / `decision`关键决策 / `knowledgeReference`已参考知识 / `learningCandidate`沉淀候选 / `skillImprovementCandidate`Skill 改进候选 / `todo`后续事项 / `risk`风险记录 / `statusTransition`状态切换 / `progress`进展。后端会归一化（`root_cause`/`根因` 等别名可识别）；**未知 type 不报错但静默透传**（UI 显示为通用「需求事件」），所以拼错不会失败、只会降级——不确定时用规范形。
- 类型传错时返回 `{"error":"Failed to deserialize the JSON body into the target type"}`，**报错不含字段名**；按上表逐字段核对或二分 payload 定位。
- 事件无 DELETE：`DELETE /api/requirement/events` 返回 405，误发事件无法通过 API 撤回。兜底只能直接改文件：删除 `events.jsonl` 中对应事件行 + 删除 `notes.md` 中同步追加的事件块，然后调 `/api/requirement/validate`。
- 拿不准 payload 时先 `"dryRun": true` 预览（不写 `events.jsonl`/`notes.md`），确认后再正式提交。
- `notesMode: "summary"`：notes.md 只追加一行 `[事件 <id>] 标题（type）：summary`，全文留 events.jsonl——排查类长 evidence 事件用，防 notes.md 无限膨胀；默认 full 全字段渲染。
- 响应 `unknownType: true` + `knownTypes`：type 不在白名单（note/issueFound/rootCause/workaround/solution/testResult/decision/knowledgeReference/learningCandidate/skillImprovementCandidate/todo/risk/statusTransition/progress）时出现，事件已照写但按未知类型降级——立即核对拼写重发正确事件。
- 事件写入并发安全已由服务端保证（requirement_append_lock 串行化，2026-10-22）：同一 bash 块并发发多个 POST 也不会丢行，无需自行保证顺序、无需补救重发。

### `{seq}` 自动编号

`reqId` 支持用 `{seq}` 占位符让 API 自动分配序号，调用方不必预先查最大编号。格式：`<前缀>-{seq}-<描述>`，API 扫描同池现有需求的最大序号 +1，补零到 3 位。

**ID 前缀只表达类型，不表达项目**（项目归属由工作区/仓库名决定，分支名不带项目字）。自 2026-09 起新建用新式前缀，旧 `WMS-*` 前缀存量 id 继续有效且同一序号空间延续（如 `REQ-{seq}` 会从 WMS 主池最大序号后继续）：

| 类型 | 新式前缀（推荐） | 旧式（兼容，序号延续） |
|---|---|---|
| 普通需求 | `REQ-{seq}` | `WMS-{seq}` |
| 线上问题 | `INC-{seq}` | `WMS-INC-{seq}` |
| 测试问题 | `TST-{seq}` | `WMS-TST-{seq}` |
| 性能问题 | `PERF-{seq}` | 无（2026-10 新增） |
| 需求组 | `GRP-{seq}` | `WMS-GRP-{seq}` |
| 整合需求 | `ROLLUP-{seq}`（`consolidated=true`） | 无 |

- 占位符必须带非空前缀：`REQ-{seq}-demo` 合法，`{seq}-demo` 报错。
- 只允许一个 `{seq}`：`REQ-{seq}-{seq}` 报错。
- 非 `dryRun` 时用原子 `mkdir` 占号，并发安全；`dryRun` 只预览不占号。
- 不含 `{seq}` 的 `reqId` 走原校验逻辑，完全向后兼容。
- **独立编号池**（强制，同 `{seq}` 模板会被改写为对应池新式前缀保留 suffix，其它池前缀的具体 id 被拒）：`category=线上问题` → `INC-{seq}`；`category=测试问题` → `TST-{seq}`；`category=性能问题` → `PERF-{seq}`；需求组（`members` 非空，只能 `category=需求`）→ `GRP-{seq}`；整合需求（`consolidated=true`，只能 `category=需求`）→ `ROLLUP-{seq}`；**修复需求（`category=需求` 且绑定 `issues`，自动路由无需传参）→ `FIX-{seq}` 独立池**。需求主池不得使用其他池前缀。
- **修复需求（FIX 池）**：由线上问题/性能问题转出的代码修复（修复需求创建时绑定 issues 同样自动路由 FIX 池）。修复也是普通需求（category=需求，同一套 7 状态机与门禁，进入需求列表），仅前缀 `FIX-{seq}` + meta `req-kind: fix` 区分，列表卡片带「修复」标识；需求进入 ≥经验总结 时自动把关联问题推进到已修复。存量 REQ-/WMS- 前缀的绑定需求不改号；创建后补绑 issues 的需求不改 ID（软约束，以创建时是否绑定为准）。

示例：

```bash
curl -sS -H 'Content-Type: application/json' \
  -X POST http://agent-panel.tail260d93.ts.net:8788/api/requirements \
  -d '{
    "reqId": "REQ-{seq}-rocketmq-fail-alarm",
    "title": "RocketMQ 发送/消费失败告警",
    "project": "WMS",
    "status": "需求澄清",
    "summary": "...",
    "dryRun": true
  }'
# 返回 reqId: "REQ-146-rocketmq-fail-alarm"（序号由 API 分配，延续 WMS 主池谱系）
```

状态值必须严格匹配（需求流 7 个 + 线上问题专用 5 个）：

```text
需求澄清 / 开发中 / 自测中 / 测试中 / 经验总结 / 发布就绪 / 已完成
排查中 / 已定位 / 已修复 / 已复盘 / 已关闭
```

- `category=线上问题` 用轻流程：排查中 → 已定位 → 已修复 → 已复盘（有价值路径）/ 已关闭（无沉淀价值）；旧状态「已确认」自动映射到「已定位」。
- `category=测试问题`（独立编号池 `TST-{seq}`）：承接 UAT 测试反馈中不属于常规需求的轻量问题，与线上问题共用同一轻流程；硬门禁（已定位 root-cause / 已复盘 troubleshooting）只对线上问题/性能问题生效，测试问题按需沉淀；测试问题不参与 `issues` 绑定（白名单=线上问题/性能问题），需要常规开发承接时另建 category=需求 的需求。
- `category=性能问题`（独立编号池 `PERF-{seq}`，2026-10 新增）：代码/业务逻辑正确、仅性能不达标（慢接口/慢 SQL/高负载）的优化型问题，与故障类分开计数；与线上问题共用轻流程，**硬门禁同样强制**（已定位前 root-cause 须含可复核性能数据：慢日志时间范围+SQL/接口+耗时前后值、压测报告、监控数据源+时间窗口）；可被需求通过 `issues` 绑定、可 convert-issue 转修复需求、支持 region。
- **需求组**（独立编号池 `GRP-{seq}`）：`members` 非空即组，只能 `category=需求`；组是需求聚合视图（group.json 引用成员），不占用主需求池；组状态 = min(成员状态) 派生，不能手动 setStatus。
- **整合需求**（独立编号池 `ROLLUP-{seq}`，创建传 `consolidated=true`，meta.md 写 `req-kind: rollup`）：父需求 + 子需求 + 发布分支模型，不占用主需求池；子需求 id = `<父票号>-S<n>[-slug]`（继承父前缀 + 独立小前缀，如 `ROLLUP-001-S1-xxx`），发布走 `/api/requirement/release-branch/*`（见下方「整合需求与发布分支」章节）。
- 已定位→已修复 双路径：数据修复直接推进；代码修复创建普通需求并绑定问题（需求 meta.md `issues` 字段），需求进入 ≥经验总结 时系统自动把关联问题推进到已修复。
- **region 区域维度（issue 家族专用）**：线上问题/测试问题/性能问题在创建时传 `region`（cn=中国 / sea=东南亚，后续可扩展更多国家/区域，值统一小写规范化）写入 meta.md frontmatter；PATCH `/api/requirement` 可更新（空字符串清空）。普通需求不支持该字段（400）。前端问题页按 region distinct 动态生成筛选选项（含「未登记」），新增国家/区域零代码扩展。存量问题未登记 region 的，排查时可经 PATCH 补登记。
- 已复盘门禁：进入「已复盘」前必须已填写 `troubleshooting.md` 排查经验（怎么排查 + 怎么修复），否则状态接口拒绝。
- 开发推动门禁：`source=开发推动` 的需求进入「测试中」前必须先完成 `test-scenario.md` 测试场景文档，否则状态接口拒绝。
- `/api/requirement/convert-issue`（创建修复需求）：对线上问题/性能问题调用后会创建独立的修复需求（`FIX-{seq}` 独立编号池，`req-kind: fix`）并自动绑定该问题（返回 `fixReqId`），问题保持原类别，不再原地转换类别；测试问题调用会被拒绝。

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
| `req.configChanges` | `config-changes.md` | legacy：上线资产单一事实源已迁移 `attachments/`，存量只读 | 不新增内容 |
| `req.releaseManifest` | `release-manifest.md` | legacy：同上，仅存量需求维护 | 不新增内容 |
| `req.attachments` | `attachments/` | **上线资产单一事实源**：SQL/配置/人工动作附件 | 经附件 API `GET/PUT /api/requirement/attachment` 读写（**禁止直写本地文件**——本地 `.agents/req/` 已是只读镜像），见「上线资产附件制」节 |
| `req.impact` | `impact.md` | 影响面、核心链路风险、回滚方案 | `writeDoc` / `upsertSection` |
| `req.test` | `test.md` | 测试场景、自测/UAT 证据 | `writeDoc` / `upsertSection` |
| `req.notes` | `notes.md` | 追加型进展、决策、踩坑 | `appendNote` 优先 |
| `req.review` | `review.md` | 代码审查摘要 | `writeDoc` / `upsertSection` |
| `req.codeReview` | `code-review.json` | 大体积代码差异/审查产物 | 仅显式 review intent 读取 |

标准 intent：`overview` / `clarification` / `status` / `progress` / `branch` / `self-test` / `release-check` / `experience-summary` / `design` / `config` / `review`。

代码审查门禁：不新增主状态；作为 `自测中 → 测试中` 的强 gate。**审查必须加载 `agent-panel` skill（code-review） skill 执行**（备料 → 四轮深度审查 → 结论落盘 → 门禁自检；门禁强制结论文档带 `Source: agent-panel-code-review skill` 标记，缺标记判 skill-required 拦截）。`review.md` 或 `code-review-ai.md` 必须明确写 `Review Gate: PASS`、`Review Gate: BLOCKED` 或 `Review Gate: WAIVED`，否则 `/api/requirement/status` 会拒绝推进到 `测试中`。问题分级 P0 阻断/P1 严重/P2 一般/P3 优化：**P0 小节非空门禁不放行；P1 非空放行但门禁展示 ⚠ 警示**（均按文档实际内容校验）。门禁同时强制：**代码问题备注**（`code-annotations.json` files 非空 + `reviewedCommit` 指纹与最新材料一致；写入用 `PUT /api/requirement/annotations`，body 的 `annotations` 字段**必须是对象** `{"files":[...],"reviewedCommit":...,"mode":...}`，传数组报 `annotations must be a JSON object`，WMS-098 实测）、**自测清单交叉评估**（test.md 有自测清单时 review 必含非空「自测清单交叉评估」小节）、**详细审查文档**（code-review-ai.md）与审查清单（review-checklist.json）。结论行必须字面独立成行，**不能用 markdown 加粗**（如 `Review Gate: **PASS**` 解析不出结论，review-gate 判 pending 拦截状态推进，WMS-107 实测；门禁只拦状态推进，**不拦生产 MR 生成**）。另有两个状态接口硬门禁：开发推动需求的 test-scenario 门禁（见上）和线上问题已复盘的 troubleshooting 门禁（见上）。

**审查后又有 commit（stale 拦截）**：审查后分支又有 commit（如 review 修正）时，状态推进会被 Code Review Gate 以 stale 拒绝（`reviewedTargetCommit != currentTargetCommit`）。在 review.md 追加「覆盖至 <hash>」文字**无效**（门禁不解析文本）；正确路径：`POST /api/requirement/code-review` `{"reqId":...}` 刷新 code-review.json 快照（保留 previousReviewedSnapshot 供增量对比，**快照自动 commit+push，hub 门禁前自动 pull，无需手动 git**）后重推状态。review-gate GET 返回的 `staleRepos` 给出 reviewed vs current commit，可据此定位增量审查范围。（WMS-110 实测：状态 400 拒绝两次后按此流程通过）

自测门禁 test.md 格式契约（WMS-136 实测，3 次被拒后总结）：门禁解析以「## 自测清单」二级标题块为作用域——①三类小节 `### 主流程测试` / `### 边界场景测试` / `### 高并发/大流量场景测试` 必须写在该块内，写在块外的同类小节不参与校验；②每类小节用固定列表格 `| # | 自测项 | 结果 | 失败/无法测试原因 |`，结果列只允许 `通过` / `失败` / `无法测试`（其他文案判缺结果或失败项）；③边界/高并发小节需先 `#### 风险场景分析`（每行一条风险）再 `#### 测试清单`；④证据可放块外自由小节。首次写 test.md 直接按此契约产出，避免靠门禁报错试错。

上线资产：单一事实源是需求目录 `attachments/`（上线资产附件制，见下方专节）；`release-manifest.md`/`config-changes.md` 为 legacy 存量只读，不新增内容。

### Agent 文件直写（GET/PUT /api/requirement/file）

本机镜像有滞后（写后需等 hub autocommit + 本机 pull），**问题档案类文档（incident.md / root-cause.md / troubleshooting.md / manual-check.md 等边角 .md）的读写一律走本端点**，不要读本机路径。

**GET** `/api/requirement/file?id=<reqId>&path=<file.md>`：返回 `content` / `exists` / `baseSha`（兼容旧字段名 `sha`，同值）/ `size`。**文件不存在时 `exists:false` 且 `baseSha:null`**（不再返回空内容 sha 诱导误传）。

**PUT** `/api/requirement/file`：JSON/form body: `id`、`path`、`content`、`baseSha`（**兼容别名 `sha`**——GET 返回什么名就传什么名，两个都收）。乐观锁语义（2026-10-21 起）：

- 文件不存在 + `baseSha` 为空/省略/等于 GET `exists:false` 时的空标记 → **自动降级为新建**，不会 409
- 文件存在 + `baseSha` 匹配 → 更新；不匹配 → 409（重新 GET 再写）
- 文件存在 + 未带 `baseSha` → 409（防误覆盖）
- 响应：`{ok, id, path, sha, size, created}`；白名单：顶层 `.md`（除 meta.md/state.json）+ `docs/<base>/<file>.md` 分册

**写路径仲裁（减少选择成本）**：

- **问题档案类文档（incident/root-cause/troubleshooting/manual-check/自建分析文档）→ 只用本端点整文档替换**（干净、无模板残留）
- `sections/{section}` / `upsertSection` 只用于常规受控文档（background/test/impact 等）按 heading 局部更新；它不清理模板占位残留，**不要**用于问题档案

**读取新鲜度指引**：写后读、状态门禁判断、任何新鲜度敏感的读 → 一律走 API（`GET file` / `context` / `GET /api/requirements`）；**agent 读数据一律走 hub API，不读本机镜像、不读 git 仓**——`agent-panel-data` 仓（本机镜像 + GitHub）只是数据备份与人工兜底，不是 agent 的读路径；链路设计保证 git 不可用时备料/门禁/登记等核心流程照常工作。

示例：

```bash
# 读（不存在时 exists=false + baseSha=null）
curl -s 'http://agent-panel.tail260d93.ts.net:8788/api/requirement/file?id=<req-id>&path=root-cause.md'

# 新建：带不带 GET 返回的空标记 sha 均可
# 更新：原样传回 GET 的 baseSha（或字段名 sha）
curl -sS -H 'Content-Type: application/json' -X PUT \
  http://agent-panel.tail260d93.ts.net:8788/api/requirement/file \
  -d '{"id":"<req-id>","path":"root-cause.md","content":"# 根因与修复决策\n...","baseSha":"<GET 返回值>"}'
```

推荐流程：

1. 每轮开始或状态切换后，先调 `/api/requirement/context?id=<reqId>&for=agent&intent=<intent>&budget=2000` 获取 agent 专用摘要；以返回的 `phaseRuntime.currentPhasePrompt` 作为当前阶段导航，**不要沿用 session 创建时注入的启动 prompt**。跳状态时 `phaseRuntime.phaseGaps` 暴露的缺失 entry checks 作为风险记录，不自动回退状态。不要默认直接读 `notes.md` 或 `code-review.json`。
2. 需要明确读/写 token 时，再调 `/api/requirement/edit-plan?id=<reqId>&intent=<intent>`。
3. 事实、证据、测试结果、决策、TODO 用 `/api/requirement/events` 记录结构化事件；默认同时追加 `notes.md`。
4. 目标文档更新用 `/api/requirement/sections/{section}` 或 `/api/requirement/edit` 的 `upsertSection`，避免整篇覆盖。**契约注意（WMS-098 实测）**：`upsertSection` 是按 heading 追加/更新单个 section，**不是全文替换**——把整篇 markdown 传给 upsertSection 会被当作一个新 section 追加到文件尾造成内容重复（需手工重写修复）；全量替换用 `writeDoc` 或直接 write 文件后再 validate。
5. 写完调 `/api/requirement/validate`。

结构化编辑示例：

```bash
# 获取 agent 专用上下文（首选）
curl -sS 'http://agent-panel.tail260d93.ts.net:8788/api/requirement/context?id=<req-id>&for=agent&intent=self-test&budget=2000'

# 获取自测编辑计划（需要精确 token 时）
curl -sS 'http://agent-panel.tail260d93.ts.net:8788/api/requirement/edit-plan?id=<req-id>&intent=self-test'

# 记录结构化测试事件，同时追加 notes.md
curl -sS -H 'Content-Type: application/json' \
  -X POST http://agent-panel.tail260d93.ts.net:8788/api/requirement/events \
  -d '{"reqId":"<req-id>","type":"testResult","summary":"回滚场景验证完成","testCases":[{"name":"未装箱单","result":"pass","evidence":"941373 Success"}]}'

# 追加自由进展
curl -sS -H 'Content-Type: application/json' \
  -X POST http://agent-panel.tail260d93.ts.net:8788/api/requirement/edit \
  -d '{"reqId":"<req-id>","operation":"appendNote","title":"进展","text":"完成影响面梳理。"}'

# 按语义 section 更新 impact.md/test.md，不需要 agent 记文件名
curl -sS -H 'Content-Type: application/json' \
  -X POST http://agent-panel.tail260d93.ts.net:8788/api/requirement/sections/impact \
  -d '{"reqId":"<req-id>","heading":"boxCode 问题","content":"- 问题描述...\n- 根因..."}'

# upsert test.md 的自测证据 section（通用 edit 入口）
curl -sS -H 'Content-Type: application/json' \
  -X POST http://agent-panel.tail260d93.ts.net:8788/api/requirement/edit \
  -d '{"reqId":"<req-id>","operation":"upsertSection","token":"req.test","heading":"自测证据","content":"- Kibana tid=... 验证通过"}'

# 设置状态（写 state.json，不手改文件）
curl -sS -H 'Content-Type: application/json' \
  -X POST http://agent-panel.tail260d93.ts.net:8788/api/requirement/edit \
  -d '{"reqId":"<req-id>","operation":"setStatus","status":"自测中","note":"开发自测开始"}'
```

> **sections upsert 注意**：`POST /api/requirement/sections/<section>`（及 `upsertSection`）按标题匹配做 upsert 追加，**不会替换/清理旧模板占位内容**（如「待补充」「结论置信度 D」等残留会继续存在，并与新内容矛盾，干扰自测门禁与经验总结提取）。需要整文件重写或清理旧模板残留时，直接用 `read`/`edit` 编辑文件兜底；更新文档前先 grep 检查重复 section（如多处「结论置信度」）。

## 整合需求与发布分支（consolidated release branches）

多个小改动需要**各自独立发布**、又希望**合批发布**时，用「整合需求 + 子需求 + 发布分支」模型：

- **整合需求** = 一个普通需求（父），用 `POST /api/requirement/create-sub` 拆出多个子需求（每个子需求独立状态进度、可单独发布；登记发布分支仓库范围来自子需求 branches.json 的并集）。
- **子需求状态机**（独立发布进度）：需求创建 → 开发中 → 自测中/测试中/发布就绪（可跳级）→ 已合入（合入发布分支）→ 已发布（随发布分支生产 MR 合入后封版时自动推进）；已取消为终态。父集成模型（已合入即终点）仍兼容。agent API 推进 自测中→测试中 仍受 review/selftest 门禁。
- **发布分支**：以各仓生产分支（后端 master / 前端 production）为 base 创建预集成分支，登记在父需求目录 `release-branches.json`；发布流程：create → 逐子需求 merge-sub →（master 前进时）sync-prod → prod-mr 生成生产 MR（人工合入）→ mark-released 封版并推进子需求「已发布」。**6 端点完整契约（参数/confirm 硬前置/MR 前置检查/登记文件结构）见 `release-branch-api.md`**。
- PDA 客户端仓（APK 发布）不参与发布分支。

**拆分时机两条规则**（机制完全统一，前缀只表达拆分时机）：① 一开始就知道要拆 → 建整合需求 `ROLLUP-{seq}`（`consolidated=true`）再 `create-sub`；② 需求途中想拆 → 父需求前缀不变（如 `REQ-123`），直接 `create-sub` 拆出 `REQ-123-S1/S2/...`。无论父前缀是 ROLLUP 还是 REQ，子需求机制（ID 继承、文档快照、独立进度、发布分支预集成、独立或合批发布）完全相同——发布分支 API 对任何非组、非子需求的父需求都可用。

## 需求组（group.json）—— 已退役

> **2026-09 起需求组退役，不再新建**：拆分场景统一用「整合需求 + create-sub + 发布分支」（见上一节），事后关联展示场景由发布计划页覆盖。本节仅作存量数据（如 `WMS-GRP-001-remove-rabbitmq-combo`）的兼容说明；创建 API（members 参数）与聚合逻辑保留，勿在新需求引导中使用。

多个高关联的**独立需求**可组成引用式需求组：组需求是一个标准需求目录，额外用 `group.json` 通过 reqId 引用成员；成员需求目录、文件、分支、session、发布完全独立，不做搬家。

```json
{
  "version": 1,
  "releasePolicy": "independent",
  "members": [{"reqId": "WMS-042-xxx", "note": "库存占用改造"}]
}
```

规则：

- **组状态是派生值 = min(成员需求流状态序数)**（需求澄清 < 开发中 < 自测中 < 测试中 < 发布就绪 < 经验总结 < 已完成；线上问题/测试问题/性能问题轻流程状态不参与聚合）。组需求调用 `setStatus` / PATCH `status` 会被拒绝；推进组进度 = 推进瓶颈成员（API 字段 `groupBottleneck`）。
- **session 绑定自动扩展**：`/api/requirement/associate` 绑组时同时绑定组 + 所有成员（返回 `boundReqIds`）；`dissociate` 同步解绑。一个 session 仍只归属一个需求维度（绑新目标时从旧目标移除）。
- **创建组**：`POST /api/requirements` 传 `members`（可选 `releasePolicy`）；成员必须已存在、非自身、非组（不支持嵌套）；`releasePolicy` 仅在 `members` 非空时有效。
- **组级 context**：`GET /api/requirement/context?id=<组 reqId>`（含 `for=agent`）对组需求额外返回 `group` 块：`aggregatedStatus`/`bottleneck`/`releasePolicy`/`members`（含每个成员的 title/status/found）。
- **validate**：失效成员引用（成员不存在）报 warning；成员是组、自引用、`releasePolicy` 非法、`group.json` 解析失败报 problem。
- **加入/退出组** = 编辑需求目录下 `group.json` 的 `members` 列表（可直接改文件，改完跑 validate）；不要把成员需求搬进组目录。
- **文档边界**：组文档（background/technical-plan/release-manifest）只写跨成员内容（整体目标、协同计划、发布顺序、组级上线总览）；成员细节留在各自需求文件，避免双写漂移。
- `releasePolicy=together`（整体发布）时，发布预检应覆盖所有成员的分支/配置/测试证据；`independent`（默认）时成员独立走发布流程，组只是聚合视图。

## 上线资产附件制（attachments/ 为单一事实源）

上线资产（SQL、配置、人工动作）的单一事实源是需求目录 `attachments/`；`release-manifest.md`/`config-changes.md` 为 legacy（存量只读，不新增内容）。

- **落地时机**：开发/自测/测试/排查中当场落附件，不攒到发布前；纯测试造数 SQL 也落附件并标「上线是否需执行: 否」。
- **SQL**：`attachments/<序号>-<用途>.sql`，文件头注释固定四项：
  ```sql
  -- 用途: ...
  -- 目标环境: test|UAT-CN|UAT-SEA|生产-CN|生产-SEA
  -- 上线是否需执行: 是|否
  -- 回滚方式: ...|无需
  ```
- **配置与人工动作**：统一追加 `attachments/release-config.md`，每条含：类型（Apollo/Nacos/DTS/Topic/Group/Job/控制台/权限）、对象（namespace+key 或名称）、变更内容、环境、是否已发布、是否需上线动作、回滚方式。
- **notes.md / release-check**：只引用附件文件名，不贴全文。
- **API**：附件读写唯一通道 `GET/PUT /api/requirement/attachment`（列表 `GET /api/requirement/attachments` 只读不带 sha）。**禁止直写本地 `attachments/` 文件**——2026-10-20 起本地 `.agents/req/` 是 agent-panel-data 只读镜像符号链接，直写落在镜像工作区与 hub 永久漂移，面板/门禁/合规快照都看不到。
  - 新建：`PUT {id, path, content}`，`path` 传 `attachments/01-fix.sql` 或裸 `01-fix.sql` 均可，新建省 `baseSha`；响应 `{ok, path, sha, created}`
  - 更新：先 `GET ?id=&path=` 拿 `baseSha`，`PUT` 带回；不匹配 409（重新 GET）；`release-config.md` 追加式更新 = 旧文末追加后整文写回
  - 白名单：`attachments/` 单层 + 后缀 `sql/txt/md/yaml/yml/json/csv`；子目录、绝对路径、`..`、其他后缀一律 400

```bash
# 新建 SQL 附件
curl -sS -H 'Content-Type: application/json' -X PUT \
  http://agent-panel.tail260d93.ts.net:8788/api/requirement/attachment \
  -d '{"id":"<reqId>","path":"01-fix-inventory.sql","content":"-- 用途: ...\n-- 目标环境: test\n-- 上线是否需执行: 否\n-- 回滚方式: 无需\nUPDATE ...;"}'

# 读单附件拿 baseSha（更新前必做）
curl -s 'http://agent-panel.tail260d93.ts.net:8788/api/requirement/attachment?id=<reqId>&path=attachments/release-config.md'
```
- **阶段 entryCheck**：attachments/ 有任意文件即视为已沉淀（开发中/自测中/测试中等阶段 optional）。

## ONES 工时联动

前置：需求已关联 ONES（meta.ones 可提取 displayId）。

- **GET `/api/ones/manhour?id=<reqId>[&refresh=true]`**：四联汇总——`manualHours`（人工预估，档案）/ `autoHours`（effort-estimate.estimatedHours）/ `loggedHours`（ONES 已登记实时值）/ `agentHours`（agent 实际，档案）+ `remainingHours`（manual−logged）+ `logHistory`。工时报表窗口固定 120 天，超窗任务查不到（仅 warning）。
- **POST `/api/ones/manhour/save`**：写人工预估/agent 实际；参数 null=清除、缺省=不修改（double-option 语义）。
- **POST `/api/ones/manhour/log`**：`entries:[{date,hours,remark}]` 生成登记清单（**准备模式**，不写 ONES，返回每条 taskUrl）；`execute=true` 当前必然失败（ONES 写接口未抓包验证），不要传。
- **GET `/api/ones/tasks?reqId=&team=&refresh=`**：任务候选 + 按需求标题推荐（score ≥0.10，Top 8）；候选 `refText`（`编号 标题 URL`）可直接写入 meta.ones。数据源为 Chrome 登录态（browserAuth ones 站点）+ 进程内缓存，`refresh=true` 才回源。

## 人工核查阶段、状态门禁与发版冻结

- **人工核查阶段**：状态流 `…→测试中→人工核查→发布就绪→…`。人工负责视觉验收、主流程复测、人工代码审查；agent 负责准备 UAT 复测材料（manual-check.md：复测清单含操作步骤/测试数据/预期结果/验证方式、UAT 访问入口、测试数据清单、视觉验收要点）、确认需求分支已合并同步 UAT 环境分支、UAT 未部署时先部署并懒检查。复测出问题时代理复现/定位/修复并更新 manual-check.md 与 test.md。禁止代替人工推进发布就绪。
- **门禁语义**：`测试中→人工核查` 挂 **UAT 回归门禁（uat-regression，2026-10 新增）**；`人工核查→发布就绪`（及 `测试中→发布就绪` 直达）挂 review + uat-regression 双门禁——**agent API 推进被硬拦，人工在面板 UI 推进（via=ui）跳过门禁 = 人工确权**（状态流转卡片会实时评估门禁材料并展示三态，人工跳过是知情跳过）；agent 不得试图绕过。状态接口的 `via=ui` 是人工通道，agent 不许伪造。
- **UAT 回归门禁（uat-regression）**：test.md 必须有「## UAT 回归」小节，表格 `| # | 场景 | 结果 | 证据 |`（结果=通过/失败/无法测试/未执行）。放行语义：全通过（证据为已发生事实）放行；「无法测试」（有原因）放行但警示；失败/未执行/缺结果/通过但证据不可采不放行；整节 `不适用：<原因>` 放行但警示。**证据可采性**：证据必须是已发生的可核对事实（tid/日志关键字/接口返回/DB 前后值）；计划/待办、状态流转动作（“已推进状态”）、推断均不算——「通过」行证据含「待/未执行/计划/后续/后推进」等标记词会被拦（WMS-136 教训：agent 曾把「用户手工回归后推进发布就绪」当 UAT 证据）。抢修模式（已关联线上问题/性能问题）默认放行。
- **发布就绪小循环（fast-fix loop，2026-09 新增）**：需求已到发布就绪但临近上线还要改（漏改/小问题/匆忙上线遗留）时，由人工在面板 UI 把状态从「发布就绪」点回「开发中」（via=ui）；每次计一轮小循环：state.json `releaseReadyRounds` 递增、history 条目标注 `loop: "release-ready"`/`loopRound`。回退后 `phaseRuntime.loopKind="release-ready"`、`fastFixMode=true`，阶段提示词自动切换为 `phase-dev-release-ready.md` 快速修复变体：小改动、**默认不写单测**、**只部署 UAT（CN+SEA 成对）不发 test**、只在 UAT 测试（结果追加 test.md「发布就绪轮次 N」小节）、review.md 追加「发布就绪轮次 N」增量审查结论后**直通推进 发布就绪**（`开发中 → 发布就绪` 挂 review 门禁，改动必须增量审查刷新快照；不挂 selftest-checklist）。分支登记走 `branches-round-<N>.json` 与修复轮次联动。判定提示词变体看「最近一次进入开发中的来源」（`dev_loop_kind_from_state`），不是累计轮次——小循环之后正常打回仍会正确加载返工变体。`/api/requirement/status-flow` 的 `rework` 对象含 `loops: {rework, releaseReady}` 分轮次明细，列表/详情显示 `⚡ F{n}` 徽章；门禁详情页「循环审查记录」分区展示主流程/返工轮次/发布就绪小循环三种语境的历史审查结论（review.md 顶部=主流程，追加小节「返工轮次 N」「发布就绪轮次 N」= 各轮增量审查）。
- **打回返工（rework loop）**：测试/产品在人工核查（或测试中）发现问题打回时，由人工在面板 UI 把状态点回「开发中」（agent 仅在用户明确指令时代理执行）；`人工核查/测试中 → 开发中` 每发生一次计一轮返工：state.json `reworkRounds` 递增、history 条目标注 `rework/reworkRound`、状态切换事件带 `rework` 标签与返工指引。回退后 agent 的 `phaseRuntime.currentPhasePrompt` 自动切换为返工变体（`phaseRuntime.reworkMode=true`，`reworkRounds` 随 context 返回）：增量修复打回意见、test.md 追加「返工轮次 N 回归」小节、重新推进前用 `POST /api/requirement/review-materials`（mode=incremental）刷新审查快照（修复 commit 会使首轮快照 stale）。`/api/requirement/status-flow` 返回 `rework: {to, fromStatuses, rounds}`，需求详情/列表显示 `↺ R{n}` 徽章（≥3 次警示色，提示回头补澄清）。轻量小瑕疵（一两个小问题）可不回退状态，直接在人工核查内闭环。
- **发版冻结 deploy-freeze**：开关存 `~/.local/share/agent-panel/config.json` 的 `deployFreeze:{enabled,reason,since}`；`ylops_deploy.py` 在触发 UAT（SEA/CN）构建/部署前直读该字段，enabled=true 直接拒绝执行（test/PRO 不受影响）。**agent 触发 UAT 部署前先 `GET /api/config` 检查 `deployFreeze.enabled`**；开关由用户在面板 Settings/发布计划页控制，agent 不自行修改。

## Workflow

### 1. 检查 Agent Panel 是否运行

```bash
curl -sf --max-time 3 http://agent-panel.tail260d93.ts.net:8788/api/requirements >/dev/null \
  && echo OK || echo AGENT_PANEL_DOWN
```

如果不可用，只做服务状态检查，不读日志中的 secret-like 内容：

```bash
systemctl --user status agent-panel.service --no-pager
journalctl --user -u agent-panel.service -n 80 --no-pager
```

### 2. 解析或匹配需求 ID

若用户给的是完整 req-id，直接使用；若是缩写或标题关键词，先拉取需求列表并匹配：

```bash
curl -sf http://agent-panel.tail260d93.ts.net:8788/api/requirements | python3 -c '
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
  -X POST http://agent-panel.tail260d93.ts.net:8788/api/requirements \
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
curl -sS "http://agent-panel.tail260d93.ts.net:8788/api/requirement/context?id=<req-id>&for=agent&intent=<intent>&budget=2000"
```

返回重点：`summaryDocs`（摘要文档）、`recentEvents`（最近结构化事件）、`recommendedWrites`（推荐写 API——`phaseRuntime.entryChecks` 里 required 且缺失的检查会自动置顶为 PUT file 建议并带 `when` 使用时机，优先处理这些缺失项）。只有摘要不足时，才再按 token 获取全文片段：

```bash
curl -sS "http://agent-panel.tail260d93.ts.net:8788/api/requirement/edit-plan?id=<req-id>&intent=<intent>"
curl -sS "http://agent-panel.tail260d93.ts.net:8788/api/requirement/context?id=<req-id>&intent=<intent>&budget=2000"
```

根据上下文选择写入方式：事实/证据/决策/TODO 用 events，目标章节用 sections。

```bash
curl -sS -H 'Content-Type: application/json' \
  -X POST http://agent-panel.tail260d93.ts.net:8788/api/requirement/events \
  -d '{"reqId":"<req-id>","type":"rootCause","summary":"boxCode 不匹配导致库存查询落空","evidence":["LocationInventoryService findList 对 boxCode=null 查 IS NULL"]}'

curl -sS -H 'Content-Type: application/json' \
  -X POST http://agent-panel.tail260d93.ts.net:8788/api/requirement/sections/impact \
  -d '{"reqId":"<req-id>","heading":"boxCode 问题","content":"- 问题描述...\n- 根因..."}'

curl -sS -H 'Content-Type: application/json' \
  -X POST http://agent-panel.tail260d93.ts.net:8788/api/requirement/validate \
  -d '{"reqId":"<req-id>"}'
```

```bash
curl -sS -H 'Content-Type: application/json' \
  -X POST http://agent-panel.tail260d93.ts.net:8788/api/requirement/edit \
  -d '{"reqId":"<req-id>","operation":"upsertSection","token":"req.impact","heading":"影响面评估","content":"- ..."}'

curl -sS -H 'Content-Type: application/json' \
  -X POST http://agent-panel.tail260d93.ts.net:8788/api/requirement/validate \
  -d '{"reqId":"<req-id>"}'
```

### 4. 更新需求字段

```bash
curl -sS -H 'Content-Type: application/json' \
  -X PATCH http://agent-panel.tail260d93.ts.net:8788/api/requirement \
  -d '{
    "reqId": "<req-id>",
    "title": "新标题",
    "owner": "hevin",
    "planRelease": "unknown",
    "ones": "https://..."
  }'
```

字段范围只包括受控字段；状态和类别也可走 PATCH，但单纯状态流转推荐 `/api/requirement/edit` 的 `setStatus`。

### 4A. 存量需求纠正为线上问题（setCategory 操作序列与坑点，WMS-065 实测）

把存量需求（创建时无线上问题分类而误登记为 `REQ-/WMS-*`）纠正为线上问题时，按以下顺序操作（ID 不改号，沿用原 ID）：

```bash
# 1) 先改 category（后续步骤依赖 category=线上问题 已生效）
curl -sS -X PATCH http://agent-panel.tail260d93.ts.net:8788/api/requirement -H 'Content-Type: application/json' \
  -d '{"reqId":"<req-id>","category":"线上问题","dryRun":true}'   # 确认无误后去掉 dryRun
# 2) 再传 region（region 校验先于 category 生效：category 还是需求时传 region 会 400）
curl -sS -X PATCH http://agent-panel.tail260d93.ts.net:8788/api/requirement -H 'Content-Type: application/json' \
  -d '{"reqId":"<req-id>","region":"sea"}'
# 3) 显式 setStatus 到问题轻流程状态（setCategory 会把需求流状态重映射为开发中，不在轻流程内）
curl -sS -X POST http://agent-panel.tail260d93.ts.net:8788/api/requirement/status -H 'Content-Type: application/json' \
  -d '{"reqId":"<req-id>","status":"已定位"}'
```

坑点（实测）：
1. **region 校验先于 category 生效**：同一个 PATCH 里改 category + 传 region 会因「普通需求不支持 region」被拒，必须先单独改 category，再传 region。
2. **setCategory/PATCH category 只写 state.json，meta.md frontmatter 的 category/status 不同步**（`patchMeta` 不支持这两个字段，返回 no supported fields）；后续 agent 直接读 meta.md 会误判类别，需手工同步 meta.md frontmatter 或在 meta.md 头部加说明指向 state.json。
3. **状态被重映射为开发中**：需求流状态不在问题轻流程内，纠正后必须显式 `setStatus` 到目标轻流程状态（排查中/已定位）。
4. **已定位有 root-cause 硬门禁**：推进前先落 root-cause.md；已复盘 gate 需要 troubleshooting.md（怎么排查+怎么修复+根因+复用清单）。
5. **incident.md/root-cause.md 整文档写入不要走 sections/upsertSection**：upsertSection 会把整篇内容挂到默认 heading 下，应直接 `write` 文件。
6. 与 `/api/requirement/convert-issue` 的区别：convert-issue 是**从线上问题创建修复需求**（FIX 池），不是类别纠正入口，纠正存量需求类别走本节 PATCH 序列。

### 5. 追加 notes / 写受控文档

追加进展不要全文覆盖 `notes.md`：

```bash
curl -sS -H 'Content-Type: application/json' \
  -X POST http://agent-panel.tail260d93.ts.net:8788/api/requirement/notes \
  -d '{"reqId":"<req-id>","title":"进展","text":"完成影响面梳理。"}'
```

替换或追加白名单文档：

```bash
curl -sS -H 'Content-Type: application/json' \
  -X PUT http://agent-panel.tail260d93.ts.net:8788/api/requirement/doc \
  -d '{"reqId":"<req-id>","docType":"test","mode":"replace","content":"# <req-id> Test\n\n## 测试场景清单\n- ..."}'
```

`docType` 仅支持：`background` / `memory` / `branch` / `impact` / `test` / `test-scenario`（开发推动测试场景文档）/ `troubleshooting`（线上问题排查经验）/ `notes` / `review` / `release-check` / `experience-summary` / `alignment` / `prd`。**注意**：`config-changes` / `release-manifest` 不在 doc API 白名单（实测返回 unsupported docType）；这两个文档用 `/api/requirement/sections/config`、`/api/requirement/sections/release`（upsertSection）或直接 `read`/`edit` 文件兑底维护。

### 6. 校验需求

```bash
curl -sS -H 'Content-Type: application/json' \
  -X POST http://agent-panel.tail260d93.ts.net:8788/api/requirement/validate \
  -d '{"reqId":"<req-id>"}'
```

返回 `ok=false` 时必须修复 `problems`；`warnings` 是推荐补齐项。

### 7. 更新需求状态

Agent 调用时优先要求 JSON，避免把 303 redirect 当失败：

```bash
curl -sS -H 'Accept: application/json' \
  -X POST http://agent-panel.tail260d93.ts.net:8788/api/requirement/status \
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

**Round ≥ 2 修复分支的限制（WMS-137 实测，2026-09-28）**：`merge-options` / `merge-branch` 只认 round 1 登记（`branches.json`），`branchScope` 固定取 round 1，接口无 `round` / `sourceBranch` 参数。需求封版后走修复轮次（`branches-round-2.json` 登记 fix 分支）时，API 无法把 fix 分支合到 test/uat，此时按 git 合并红线用 **detached worktree fallback**（全程不触碰主仓）：

```bash
# 以合 uat 为例（test 同理替换分支名）；目标项目根目录执行
git worktree add --detach .worktrees/uat-merge origin/uat
git -C .worktrees/uat-merge merge --no-ff origin/hevin.yang/fix/<fix-branch>
# 解决冲突后
git -C .worktrees/uat-merge push origin HEAD:uat
git worktree remove .worktrees/uat-merge
```

冲突保留原则与需求分支合并一致：目标环境分支独有的未上线代码保留，master/需求分支已有的修正以 master 为准。

标准流程：

1. 先确认需求 ID；不确定时按“解析或匹配需求 ID”查找。
2. 调 `merge-options` 获取前端/后端可选分支和默认值：

```bash
curl -sS 'http://agent-panel.tail260d93.ts.net:8788/api/requirement/merge-options?id=<req-id>'
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
  -X POST http://agent-panel.tail260d93.ts.net:8788/api/requirement/merge-branch \
  -d '{"reqId":"<req-id>","repoKind":"backend","targetBranch":"test","target":"test"}'
```

前端 UAT 示例：

```bash
curl -sS -H 'Content-Type: application/json' \
  -X POST http://agent-panel.tail260d93.ts.net:8788/api/requirement/merge-branch \
  -d '{"reqId":"<req-id>","repoKind":"frontend","targetBranch":"master","target":"uat"}'
```

后端 UAT 示例（`targetBranch` 取 `merge-options` 返回的最新 `UAT-*`）：

```bash
curl -sS -H 'Content-Type: application/json' \
  -X POST http://agent-panel.tail260d93.ts.net:8788/api/requirement/merge-branch \
  -d '{"reqId":"<req-id>","repoKind":"backend","targetBranch":"UAT-2607","target":"uat"}'
```

结果处理：

- `status=merged` / `upToDate`：合并已完成或目标已包含需求分支。
- **核对每个 repo 结果的 `targetBranch` 实际值**：传 `targetBranch=uat` 时面板按仓解析（branches.json 的 `uat_target_branch` 显式覆盖优先，其次统一 `uat`）——若该仓 branches.json 遗留 legacy `UAT-*` 覆盖，会被重定向合到 `UAT-*` 而非 `uat`（WMS-098 实测 log-api 曾被合到 UAT-2608，需 revert 清理后手动合 uat）。响应 `targetBranch` 与预期不符时立即停止后续操作并向用户确认。
- 后端 UAT 分支已统一为小写 `uat`（WMS-098 起标准）；`UAT-*` 仅作 legacy 回退。
- `status=skipped`：对应 repo 类型或分支不适用；检查 `repoKind` / `targetBranch` / `branches.json`。
- `status=failed`：读取 `message` 和 `commands`，不要改用手写 git 绕过；先判断是分支不存在、push 失败还是 worktree 清理失败。
- `status=conflict`：必须使用接口返回的 `worktreePath` 和 `conflictFiles` 作为冲突处理入口。
- **merged 后远端验证（WMS-068 教训）**：历史上出现过接口返回 `merged` 但 merge commit 只创建在本地 merge station 分支、未推送远端的情况（本地分支领先远端数百 commit）。合并后用远端状态验证真正合入，不要信任本地分支或 merged 事件：
  ```bash
  git -C <repo> ls-remote origin <targetBranch>                                # 远端真实哈希
  git -C <repo> fetch origin <targetBranch>
  git -C <repo> merge-base --is-ancestor <req-branch-tip> origin/<targetBranch> && echo MERGED || echo NOT-MERGED
  ```
  若验证为 NOT-MERGED：用临时 worktree 从 origin/<targetBranch> 合并需求分支并 push（补推），并把该情况记录为需求事件。

冲突状态查询：

```bash
curl -sS 'http://agent-panel.tail260d93.ts.net:8788/api/requirement/merge-status?id=<req-id>&target=test'
```

冲突处理原则：

- 人工触发页面时，可让用户打开 `/requirement-merge?id=<req-id>` 查看冲突 worktree 和文件列表。
- agent 调用接口时，若返回 `conflict`，后续冲突解决直接进入返回的 `worktreePath`，按 `conflictFiles` 列表处理；不要重新扫描所有仓库或重新推断分支。
- 冲突解决、提交、推送仍属于 git 写操作；如需要 agent 继续处理，遵循对应项目的 git/worktree 规则。

### 9. 关联当前 session

优先用 `get_session_info` 取当前 session id；拿不到时才使用已有脚本或让用户提供。执行：

```bash
curl -s -o /dev/null -w "%{http_code}" \
  -X POST http://agent-panel.tail260d93.ts.net:8788/api/requirement/associate \
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

- 创建/修改需求优先走 Agent Panel API；Agent Panel 不可用时才 fallback 到 `req` skill（create.md）直接写文件。
- 非简单更新先走 `/api/requirement/edit-plan` + `/api/requirement/context`，不要默认直接读取大文件。
- 写入优先走 `/api/requirement/edit`；旧 `/notes` 和 `/doc` 仅作兼容。
- 不直接编辑 `state.json` 或 `meta.md` 的状态字段。
- `reqId` 只能使用 ASCII 字母、数字和单个连字符，不能包含空格、中文、路径分隔符或 `..`。
- 写文档只能使用 `/api/requirement/doc` 的白名单 `docType`；不要把任意文件路径传给 API 或直接写路径。
- 大段进展优先 `/api/requirement/notes` 追加，避免覆盖已有 notes。
- `dryRun:true` 适用于创建、更新字段、notes、doc 写入预览；正式写入后再 validate。
- `status` 必须是 12 个合法状态之一（需求流 7 个 + 线上问题 5 个）；线上问题状态只能用于 `category=线上问题`、`category=测试问题` 或 `category=性能问题`。
- `GET /api/requirements` 不保证服务端按 query 过滤；按状态筛选时必须客户端过滤。
- 状态更新优先使用 `Accept: application/json`；不用 `curl -f` 判断 303。
- 需求分支合并必须优先走 `/api/requirement/merge-options` + `/api/requirement/merge-branch`；不要自行手写 git merge/push，除非 Agent Panel 不可用且用户明确要求 fallback。
- merge-options 没有默认值时（非 `自测中`/`测试中`），不要擅自选择目标分支；向用户确认或说明未执行合并。
- 处理合并冲突时，使用接口返回的 `worktreePath` 和 `conflictFiles`；不要重新探索所有仓库。
- `/api/requirement/events` 的 `evidence`/`triggerTerms`/`related*`/`decisions`/`todos`/`tags`/`testCases` 必须传数组；传字符串报 `Failed to deserialize` 且错误不含字段名。
- 事件接口无 DELETE（405）；误发事件只能删 `events.jsonl` 对应行 + `notes.md` 事件块兜底；拿不准 payload 先 `dryRun:true`。
- Agent Panel 不可用时先检查 systemd unit，不要花很久搜索源码。

## Final Response

```text
✅ Agent Panel 需求 API 已执行
- 操作: <创建 / 更新字段 / 追加 notes / 写文档 / 状态变更 / 校验>
- 需求: <title>（<req-id>）
- 写入: <files 或 无>
- 校验: <ok / problems 摘要>
```
