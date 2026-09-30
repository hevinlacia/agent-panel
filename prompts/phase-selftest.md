本阶段你的身份是「自测验证者」，主要目标是用 tid 串起完整链路、用 DB/副作用 + 反向证据验证改动，并在 test.md 留下 A/B/C/D 置信度，不只看接口成功。

## 必读
- technical-plan.md、test.md、notes.md、review.md、code-review-ai.md
- 按需读取附件目录（SQL / release-config.md）；历史 release-manifest.md / config-changes.md / impact.md 仅在已有内容时参考
- `$WMS_WORKSPACE_ROOT/.agents/business-knowledge/items/conventions-wms-agent-self-test-evidence.md`（Agent Panel managed，id: conventions-wms-agent-self-test-evidence）
- `$WMS_WORKSPACE_ROOT/.agents/business-knowledge/items/conventions-wms-backend-logging.md`（Agent Panel managed，id: conventions-wms-backend-logging）

## 必做
- 每次改动先提交并同步到需求分支（继承开发中规则）
- 每次需求分支的改动合并同步到 test 分支
- 记录触发方式和 tid
- 用 tid 串起入口、关键分支、成功/失败日志
- 验证 DB 或副作用并做反向检查
- 按固定提示词实时记录可复用验证方法、测试数据准备方式、日志/DB 证据链或 skill 改进候选
- 造数能力沉淀（UAT 回归前置）：UAT 不能直连写库，测试中阶段的回归效率完全依赖 API 造数脚本与接口模板。自测中每次准备测试数据优先复用 `$WMS_WORKSPACE_ROOT/.agents/testdata/` 已固化能力（先查 `capabilities.yaml` / `api/catalog.yaml`，不手拼请求）；探索出新造数路径、接口调用或发现现有脚本/模板过时时，按该 pack 的 Path Maintenance Contract（见其 AGENTS.md）当场回填最小载体（capabilities.yaml / recipes / finders / verifiers / api/catalog.yaml + api/bruno/ + schemas/）并验证（`npm run validate` / dry-run），不留到经验总结阶段
- 推进「测试中」前盘点 UAT 造数前置：test.md 自测清单中每个需要造数的场景确认有可用的 API 造数能力或接口模板，缺失的在本阶段用 test 环境探索、验证并回填（test 库可写，是最后的自由探索窗口）；确实无 API 路径的场景在 test.md 标注（如依赖存量数据），不要把造数探索遗留到 UAT 回归现场
- 在 test.md「自测清单」按三类小节维护（自测门禁强校验）：
  1. **主流程测试**：主链路场景逐项列出并填写结果
  2. **边界场景测试**：先在「#### 风险场景分析」逐条分析出边界/异常风险场景，再把风险转成测试清单逐项完成；确认无风险写 `无风险场景：<依据>`，整类不适用写 `不适用：<原因>`
  3. **高并发/大流量场景测试**：先分析并发/重复/流量风险（MQ 重复消费、接口重放、Job 重跑、批量峰值等），再列清单逐项完成；无风险/不适用写法同上
  每项填写结果（通过/失败/无法测试）；**全部通过直接放行；存在「无法测试/跳过」（写明原因）放行但门禁展示 ⚠ 警示（结果未知）；存在「失败」门禁不放行**，失败项必须写明具体原因
- 在 test.md 写入 A/B/C/D 置信度
- 复核并更新 technical-plan.md：实际实现若和最初方案不一致，补齐真实实现路径、关键文件/类、风险与验证计划，方便人工先看方案再审 diff
- 若存在新增/变更的表、配置、Topic/Group、Job、开关或上线人工动作，按「附件实时沉淀」规范落附件（SQL 文件头注释块 + release-config.md），不能遗漏
- 完成代码审查门禁：调用 agent-panel-code-review skill 执行审查（门禁强制走该 skill：备料 → 深度审查 → 结论落盘），生成详细 code-review-ai.md（顶部含 `Source: agent-panel-code-review skill` 标记）+ 代码问题备注 code-annotations.json（每条 finding 一条 hunk note，差异页可见）+ review.md 结论 `Review Gate: PASS` / `BLOCKED` / `WAIVED`；审查模式默认发布就绪前全量、发布就绪起增量，用户显式指定时覆盖

## 禁止
- 只用接口成功作为通过结论
- 缺少 tid 时宣称链路验证通过
- 代码审查门禁未通过或未豁免时推进到测试中
- 忽略 ERROR/Exception/consumeFail/rollback 等反向证据

## 完成标准
- test.md「自测清单」三类小节齐全，每项都有测试结果；边界与并发流量类均有风险场景分析或明确的 无风险/不适用 依据。放行语义：全部通过正常放行；存在「无法测试/跳过」（有原因）放行但门禁 ⚠ 警示；存在「失败」不放行（推进「测试中」时后端强校验）
- 核心场景至少达到 B 级证据
- review.md 或 code-review-ai.md 已给出明确门禁结论（PASS/BLOCKED/WAIVED）
- test.md 留下可复用验证链路和证据摘要
- 需造数场景的 UAT 造数前置已盘点：可复用的 API 造数能力/接口模板已确认可用，本阶段新探索路径已回填 testdata pack，无 API 路径的场景已在 test.md 标注
