# Code Review 门禁（一键备料 + 多轮审查）
n
配套方法论见 `review-methodology.md`。

> 上级入口：`../SKILL.md`。需求工作流上下文见 req skill。

# Agent Panel Code Review

用于：对本机 Agent Panel 管理的需求执行代码审查并过 Code Review Gate。产出真正能发现问题的深度审查结论，而不是复述 diff。

适用：
- 状态推进被 Code Review Gate 拦截（missing / pending / stale / skill-required / checklist-missing / inventory-pending / blocked）
- 用户说“代码审查”/“过门禁”/“重新审查”/“增量审查”/“全量审查”
- 测试中追加提交后需要复审
- 需要产出 code-review-ai.md、review-checklist.json、code-annotations.json

不适用：
- 用户明确要求豁免：直接在 review.md 顶行写 `Review Gate: WAIVED` + 豁免原因，跳过本 skill
- 只看差异不走门禁（用需求详情页代码差异卡片）
- 审查非 Agent Panel 需求的任意 diff（直接派 code-reviewer 子 agent）

## Trigger

- “代码审查”、“审查一下这个需求的代码”、“过审查门禁”、“门禁说需要审查”
- 推进「测试中」被 Code Review Gate 拦截后系统指引调用本 skill
- review-gate 返回 skill-required / stale / missing

## 审查模式

默认按需求状态决定，用户显式指定时覆盖：

| 需求状态 | 默认模式 | 材料 |
|---|---|---|
| 需求澄清 / 开发中 / 自测中 / 测试中 | **全量**（base → HEAD 全部 diff） | code-review.json |
| 发布就绪 / 经验总结 / 已完成 | **增量**（上次已审 commit → 当前 HEAD） | code-review-incremental.json |

- 用户说“增量审查”传 `"mode":"incremental"`；说“全量审查”传 `"mode":"full"`。
- 增量的前提：已有全量快照 + HEAD 有推进 + 线性历史；不满足时面板自动回退全量，不要手工绕过。

## 流程

### 1. 备料（先于一切审查动作）

```bash
curl -sS -H 'Content-Type: application/json' -X POST \
  http://agent-panel.tail260d93.ts.net:8788/api/requirement/review-materials \
  -d '{"reqId":"<req-id>","mode":"auto"}'
```

`mode` 缺省或 `auto` = 按需求状态默认；`full` / `incremental` 强制指定。

返回 `materials`：`materialPath`（元数据 JSON）+ `diffPatchFile`（多行 patch）+ `handoffHints`。

铁律：
- **diff 全文读 patch 文件**（`read` + 超过 2000 行用 offset 续读）；**绝不 read JSON 的 diff 字段**——它是单行转义串，read 会被单行截断。
- files / commits / riskTags / inventoryRisk 等元数据读 `materialPath` JSON（跳过其 diff 字段）。
- `handoffHints` 逐条读并遵守；`warnings` 逐条处理。
- 增量材料只审 fromCommit → toCommit 的新增 diff；但新增风险涉及旧逻辑交互时，允许对相关旧代码扩大阅读。

### 2. 建立需求上下文

```bash
curl -sS 'http://agent-panel.tail260d93.ts.net:8788/api/requirement/context?id=<req-id>&for=agent&intent=review&budget=3000'
```

读 technical-plan.md 方案摘要与影响面 → 先知道“应该改什么”，再审 diff 判断“实际改了什么”。**方案与实现不一致本身就是 finding**。**影响范围落地核对（C12）**：对照影响范围清单双向核对 diff——清单声明「需要同步修改」的关联模块/消费方在 diff 中无对应改动且无不改依据 = 漏改 finding；diff 波及清单外模块 = 未声明的影响面扩散，结合 C11 枚举消费方评估。
**同时读 test.md「自测清单」三分类**（主流程 / 边界场景测试 / 高并发·大流量场景测试）：边界与并发流量的风险场景分析和测试结果是审查的交叉核对输入，不是背景材料。必要时按需读 background.md、release-manifest.md、config-changes.md。

### 3. 深度审查（核心）

按 `references/review-methodology.md` 执行四轮审查法：

- **Pass A 变更盘点**：按文件/风险标签建变更清单，标出 3-5 个最高风险文件优先深读；同时给每个改动资产打**共享度**标签（单消费方 / 多消费方共用：SQL statement、Service 方法、DTO 字段、常量枚举、配置 key、MQ topic）。多消费方共用 = 高风险项，Pass B 必须按 C11 枚举消费方。
- **Pass B 逐文件深读 + 扩大阅读**：读改动函数完整实现、调用方、被调方、Mapper XML、枚举与配置，不只看 diff。**全量审查（非增量）时扩大阅读是必做动作**：每个改动文件至少核对完整实现与直接调用方/被调方；涉及表变更读 Mapper XML 全文，涉及枚举/状态码读枚举定义与消费方，涉及配置读 Apollo/Nacos 实际值。只有增量审查才允许把阅读范围收敛到新增 diff 及其直接关联。**共用资产核对**：改动落在多模块共用的 SQL statement / Service 方法 / DTO 字段 / 常量 / 配置项上时，必须枚举全部消费方（仓内 + 跨仓 + 前端/PDA）并逐一判断旧语义是否被破坏，见 `references/review-methodology.md` §3-C11。
- **Pass C 横切检查清单**：空值边界 / 并发事务 / 幂等重试 / 状态机 / SQL 性能 / 跨仓契约 / 配置回滚 / 库存账本 / 日志可观测 / 安全 / **共用资产影响面收敛（C11）** / **影响范围计划核对（C12）**——每项都要写“查了什么 + 证据 + 结论”，不允许泛泛“无问题”。
- **Pass D 反向验证**：对关键分支构造反例推演（null / 空集合 / 重复调用 / 并发交错 / 边界值）。
- **自测清单交叉评估（必做，产出独立小节）**：拿 Pass A-D 的发现与 test.md 自测清单三分类交叉核对——
  1. **覆盖度**：diff 里的边界/并发流量风险，自测的风险场景分析是否覆盖？漏了 → P2 建议补充自测风险分析
  2. **印证**：自测“失败/无法测试”的项与审查发现互相印证，失败场景对应代码重点核查
  3. **矛盾**：自测声称通过、但代码审查发现该场景没有真实保护（无幂等/无锁/无判空）→ 直接 P1
  结论写入 code-review-ai.md「## 自测清单交叉评估」小节（门禁强制校验非空）。

大 diff 可按仓库并行派 `code-reviewer` 子 agent（high-model）：handoff 必须内联或给出 patch 文件绝对路径（**diff 材料必须含完整 commit 内容 src/main+src/test**——用 `-- src/main` 过滤会漏掉测试文件，reviewer 会据「引用本轮新增枚举却不在 diff」误判单测未提交报 BLOCKED，WMS-118 round2 实测；复核 reviewer 的 BLOCKED 级 finding 前先 `git show <commit> --stat` 核实测试是否随 commit 提交）+ 需求上下文摘要 + 横切清单；命中共用资产（C11）时 handoff 额外要求子 agent 输出「本仓消费方枚举 + 逐个影响判定」，跨仓/前端消费方由主 agent 汇总；**子 agent 只做分仓初筛与机械证据收集，判定、BLOCKED/PASS 结论与产物落盘全部由主 agent 负责**。

## 复审规则（门禁发现问题时强制）

只要 review-gate 返回任何非 passed 状态（blocked / stale / skill-required / checklist-* / annotations-* / inventory-pending），**主 agent 必须亲自做一次全面复审**，不能只派子 agent，也不能只修 gate 报的表面问题：

1. **用上下文换准确度**：主 agent 会话里有子 agent 没有的信息——需求背景与取舍、实现过程的关键决策、之前自测/排查的结论。复审先在会话内回顾这些信息，再结合 technical-plan.md / notes.md 交叉验证 diff，判断改动是否真正实现意图。
2. **复审范围是全部 diff**：四轮审查法从头过一遍当前材料，不是只看 gate 指出的那几处；并在审查概览里回答「这个问题为什么之前没被发现」，把漏判原因写清楚。
3. **问题必须闭环**：gate 报的问题逐条给出结论（已修复 / 确认不成立 + 依据 / 确认存在 → BLOCKED），不允许静默忽略。
4. **产物整体重出**：按「产物落盘」重新产出全部产物（含 annotations 指纹刷新），再走门禁自检。

### 4. 产物落盘（顺序固定，缺一会安门禁）

1. **code-review-ai.md**（write 覆盖，唯一自写文件，**详细审查文档**）：顶部必须有两行——
   ```
   Review Gate: PASS 或 BLOCKED
   Source: agent-panel-code-review skill
   ```
   `Source` 行是门禁识别标记，缺了会被判 skill-required。小节（缺一不可，某小节无内容写「无」并说明查了什么）：
   审查概览（模式/覆盖范围 from→to/扩大阅读清单）→ **自测清单交叉评估**（边界/并发流量风险覆盖度 + 测试结果矛盾点）→ **P0 阻断** → **P1 严重** → **P2 一般** → **P3 优化** → 横切检查结论 → 库存账本评估（命中时）→ 测试验收要点 → 亮点。

   问题分级标准（逐条核查、区分真实问题与误报，不输出无意义冗余告警）：
   - **P0 阻断**：线上故障、数据错乱、安全漏洞、库存账本不平衡——必须修复，禁止合并
   - **P1 严重**：性能问题（慢 SQL / 超 3 秒接口）、潜在数据风险、并发缺陷、幂等缺失——必须修复
   - **P2 一般**：代码可读性、冗余问题、防御不足——建议修复
   - **P3 优化**：可选重构，不阻断上线
   每条 finding 格式：`[P0/P1/P2/P3]` `[逻辑/性能]` `文件:行` + 问题描述 + 触发条件 + 具体修复建议；可疑告警先自己验证（读完整实现 / 推演反例）再定级，验证不了写进审查概览的 Open Question，不占 P 级。
   **P0 有内容 → Review Gate: BLOCKED；P0 无内容 → PASS**（P1 非空时门禁放行但展示 ⚠ 警示提醒用户，P1 仍必须尽快修复并跟踪；门禁按文档实际内容校验，结论与内容不一致时以内容为准）。
2. **PUT /api/requirement/annotations**（**代码问题备注，门禁硬要求**）：让人在差异页看代码时直接看到审查发现——
   - **请求体必须是包裹结构** `{"reqId":…, "annotations":{…}}`；把 `reviewedCommit`/`files`/`notes` 平铺在顶层会被**静默丢弃**（返回 ok 但落盘空文件，门禁随后报 annotations-required）。
   - `annotations.reviewedCommit`：`{"<repo>": "<材料里该仓 targetCommit 全量哈希>"}`（增量包取 coverageToCommit）；缺失或与最新材料不一致会被门禁判 annotations-required / annotations-stale 拦截
   - `annotations.files[]`：**改动文件全覆盖**（多仓大 diff 按核心仓优先，纯格式噪声文件可只写 summary；覆盖不完在 code-review-ai.md 写明原因，如机械同构/噪声）：`repo` + `path` + `summary` + **`variables`（必须是对象数组 `[{"name":…, "meaning":…, "why":…}]`，不能写成 `名 | 含义 | 为何重要` 管道符拼接字符串，panel 会 400 拒绝）** + `flow` + `notes`。每条讲解按**三要素**写，讲解质量本身就是审查深度的自证——写不出“改动前是什么”等于没读懂上下文，列不出风险点等于没做影响分析：
     - `summary`（全局）：这个类/文件是干嘛的、业务含义，再讲本次改了什么、为什么；
     - `notes[]`（逐 hunk）：每个实质 hunk 一条，note 写「**改动前**这段代码的含义 → **改动后**的含义与动机」；note 尾部必须带「**⚠ 风险点**」段：从并发/幂等/事务边界/性能（3 秒红线、慢 SQL）/上线顺序依赖/开关回退/兼容性/数据一致性中按适用项逐项评估——**无风险也要写「已评估无阻塞风险，依据 X」**，禁止留空；纯格式噪声（空行/行尾重排/两点 diff 混入 master 他人改动）在 summary 注明「无实质改动/非本需求改动，可忽略」，不写 notes；
     - `variables`：diff 中新出现或语义变化的关键字段/开关/常量，`why` 写其灰度或回退语义。
   - 讲解与 finding 的分工：finding 备注回答「有什么问题」，讲解回答「这段代码是什么、为什么这么改、有什么风险」。主 agent 用「每个实质 hunk 能否讲清改动前后」自检阅读完整性：讲不清的 hunk 回去重读实现与调用方，不允许跳过；测试文件可按「用例名即契约」合并为一条 summary 级说明。
   - `annotations.notes[]`：对象数组 `{repo, path, anchor:{hunkHeader}, title, note}`；**每条 finding（严重/建议）必须对应一条 hunk note**（`anchor.hunkHeader` 从 patch 的 `@@ -a,b +c,d @@` 行原样复制，无函数名后缀的 hunk 头行尾**无空格**，锚定前用 `hunkHeader in diff` 前缀校验一遍）
   - 快速自检：PUT 后 GET `/api/requirement/annotations?reqId=` 确认 files/notes 数量与 reviewedCommit 落盘正确
3. **PUT /api/requirement/review-checklist**：横切清单逐项落结构化清单 `{id,title,conclusion,note,evidence}`，conclusion ∈ pass/fail/na；存在 fail 项不允许 PASS。改动命中共用资产时，清单必须包含「共用资产影响面」项（`id` 如 `shared-asset-impact`，note 写消费方枚举数量与收敛结论）；technical-plan.md 有影响范围清单时，必须包含「影响范围计划核对」项（`id` 如 `impact-plan-vs-diff`，note 写清单落地情况：落地 x / 不改有据 y / 漏改 z）。
4. **review.md**：用 `/api/requirement/edit` `upsertSection` token=`req.review` 写审查摘要 + Review Gate 结论。

子 agent 无写权限：annotations / checklist 内容随子 agent 结论输出，主 agent 复核后调 API 落盘。

### 5. 门禁自检

```bash
curl -sS 'http://agent-panel.tail260d93.ts.net:8788/api/requirement/review-gate?id=<req-id>'
```

`gate.status` 必须是 `passed` 才算完成；否则按 `reason` / `actions` 修复后重查。常见拦截：skill-required（缺 skill 标记）、checklist-missing、annotations-required（缺代码问题备注）、annotations-stale（备注锚定旧 diff）、crosscheck-missing（缺自测清单交叉评估）、inventory-pending、stale。审查后分支又有新 commit 会被 stale 拦截：重新备料（增量）再审，不要手改 review.md 的覆盖范围文本。

### 门禁解析坑（实测，WMS-135）

写 code-review-ai.md 时避开，否则持续 blocked 或 400：

1. 「严重问题」小节必须**纯文本一行 `无`**。`review_section_is_empty` 只认 trim 后精确匹配的无/none/n/a/na/暂无/-/无。，写成 `**无。**`（加粗）或在该小节追加「> 第一轮曾发现…已修复」等闭环说明都会被判非空 → 持续 blocked；闭环/历史说明放「审查概览」或「亮点」小节。**「P0 阻断」小节同理（WMS-091 实测）**：只允许写「无。」，括号补充说明也会被判非空 blocked；顶部“某小节无内容写「无」并说明查了什么”指的是说明写在审查概览，不是写在同一小节内。
2. 文档全文**避免用 ❌ 做表格标记**（如「交接单可达」列用「否」）：门禁有 `raw.contains('❌') && contains('严重问题'/'阻塞'/'必须修复')` 兜底判定，文档任何位置同时出现即误判 blocked（「严重问题」标题本身就算命中）。
3. 修复代码后原审查结论触发 stale（reviewedTargetCommit != currentTargetCommit）：必须**重新 POST /api/requirement/review-materials 备料**并重写 code-review-ai.md，annotations 的 `reviewedCommit` 同步为新材料的 targetCommit，否则 annotations-stale 连锁拦截。
4. `PUT /api/requirement/review-checklist` 的 body 数组字段名是 **`items`**（不是 checklist），传错 400 且错误信息不提示映射。

## Required Checks

- 备料必须走 review-materials API，不自行拼 git diff 命令。
- code-review-ai.md 必须含 `Source: agent-panel-code-review skill` 标记，否则门禁不放行。
- 每条 finding 必须有：文件:行号 + 代码引用 + 触发条件 + 影响 + 最小修复建议；没有具体证据的“可能有问题”降级为改进建议或 Open Question，不冒充严重问题。
- 存在严重问题必须 `Review Gate: BLOCKED`，禁止“有问题但 PASS”。
- 材料命中库存风险（riskTags 含“库存”或 inventoryRisk=true）必须完成库存账本评估，缺账本矩阵的 PASS 会被门禁拦截。
- **共用资产影响面（C11，改动命中多模块共用资产时必做）**：改动 SQL statement / Service 方法 / DTO 字段 / 常量枚举 / 配置 key / MQ topic 时，必须给出「消费方枚举清单（N 个，含跨仓与前端/PDA）+ 逐个影响判定 + 只影响本需求模块（或影响可接受）的证据」。未枚举完就写“无影响”= 无效结论，不得 PASS。定级：旧消费方行为被非本需求预期地改变 → P1；波及其它模块既有功能 → P0/P1（按波及面定级）；为省 diff 直接改共用 statement 默认分支或 DTO 字段既有含义且未评估收敛改法 → 至少 P2。检索消费方必须加 `--no-ignore`（工作区 `.gitignore` 屏蔽子仓，否则静默漏检）。
- **影响范围计划核对（C12，technical-plan.md 有影响范围清单时必做）**：清单声明「需要同步修改」的关联模块/消费方在 diff 中无对应改动、又无不改依据 → 漏改，不得 PASS（P1 起）；diff 波及清单外模块未经评估 → 至少 P2 并建议回写 technical-plan.md。
- 每条 finding 必须同步产出一条 annotations hunk note（anchor.hunkHeader 从 patch @@ 行复制），保证差异页可见；annotations 顶层带 reviewedCommit 指纹，缺失/过期会被门禁拦截。
- **P0 有内容必须 `Review Gate: BLOCKED`**，禁止“有阻断问题但 PASS”；P1 非空时放行但门禁展示 ⚠ 警示；门禁会按文档实际内容拦截。
- test.md 有自测清单时，「自测清单交叉评估」小节必须有实质内容，缺了门禁判 crosscheck-missing 拦截。
- 结论必须基于当前材料实际 diff；某仓库 diff 为空 = 该仓库无改动，不要虚构问题。
- 只读不写：不修改任何业务代码；唯一直接写的文件是 code-review-ai.md，其余产物走 API。

## Final Response

```text
✅ 代码审查完成（<全量/增量>，覆盖 <from> → <to>）
- 需求: <req-id> <title>
- 结论: Review Gate PASS / BLOCKED（P0 x / P1 y / P2 z / P3 w）
- 严重问题: <逐条一行；无则纯文本一行：无>
- 横切清单: N 项（pass x / fail y / na z）
- 扩大阅读: <文件/调用链清单>
- 产物: code-review-ai.md、review-checklist.json、code-annotations.json、review.md
- 门禁自检: passed / 未过原因
```
