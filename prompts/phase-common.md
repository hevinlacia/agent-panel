本固定提示词适用于 Agent Panel 需求全生命周期的每个状态；状态独有提示词只补充当前阶段身份、必读文件、必做动作和完成标准。

## 全阶段固定目标
- 一边完成当前任务，一边识别后续值得沉淀的业务知识、经验、流程问题和 skill 改进机会。
- 不要等到「经验总结」阶段才回忆；实时性强、只存在于当前 session 的细节，应当当场记录为结构化事件。
- 经验总结阶段会读取 `GET /api/requirement/experience-summary-context?id=<reqId>&limit=200` 汇总这些事件；当前 session 记录得越清楚，后续总结越可靠。

## 实时记录要求
- 参考过已有业务知识/经验时，记录 `knowledgeReference` 事件，只写 id/title/用途，避免重复沉淀。
- 发现可复用业务规则、接口/表关系、状态流转、排查路径、测试数据方法、验证证据链或踩坑时，记录 `learningCandidate` 事件。
- 发现 skill 缺失、触发词不准、必读材料缺失、流程冗余、验证标准不清或工具使用坑点时，记录 `skillImprovementCandidate` 事件。
- 如果候选来自当前 session 的具体操作，应写得更详细：触发场景、关键命令/API、相关文件、证据、失败现象、修复方式和适用边界。
- 候选不等于立刻落地：不能确定时标记 `confidence: needs-confirmation`；是否写入知识库、经验库或更新 skill，留到经验总结阶段决策。

## 推荐事件格式
- `POST /api/requirement/events`
- `type`: `knowledgeReference` / `learningCandidate` / `skillImprovementCandidate`
- `summary`: 一句话说明为什么值得记录
- `details`: 当前 session 的可复用细节，越实时越应写清楚
- `evidence`: 日志、tid、DB 前后、接口返回、文件路径、测试命令、失败/修复证据
- `triggerTerms`: 后续 agent 或用户可能提到的关键词
- `relatedFiles` / `relatedRepos` / `relatedTables` / `relatedApis`: 能定位到对象就填写
- `dedupeKey`: 推荐 `项目.主题.细分`，用于经验总结阶段去重
- `confidence`: `confirmed` / `inferred` / `needs-confirmation`
- `appendNote`: 重要候选可设为 `true`，同步压缩到 notes.md，避免当前 session 细节丢失

## 固定禁止项
- 不把未验证猜测写成 active 事实。
- 不为一次性、不可复用、没有触发词的细节强行创建 skill 或知识。
- 不因为记录候选而中断当前主任务；事件记录应短小、结构化、可后续处理。
- 不跳过当前状态提示词中的阶段目标、门禁、验证和文档维护要求。
