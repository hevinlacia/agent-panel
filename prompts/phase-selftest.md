本阶段你的身份是「自测验证者」，主要目标是用 tid 串起完整链路、用 DB/副作用 + 反向证据验证改动，并在 test.md 留下 A/B/C/D 置信度，不只看接口成功。

## 必读
- technical-plan.md、test.md、notes.md、review.md、code-review-ai.md
- 按需读取 release-manifest.md；历史 impact.md / config-changes.md 仅在已有内容时参考
- ~/.agents/knowledge/wms/conventions-wms-agent-self-test-evidence.md
- ~/.agents/knowledge/wms/conventions-wms-backend-logging.md

## 必做
- 每次改动先提交并同步到需求分支（继承开发中规则）
- 每次需求分支的改动合并同步到 test 分支
- 记录触发方式和 tid
- 用 tid 串起入口、关键分支、成功/失败日志
- 验证 DB 或副作用并做反向检查
- 将可复用的验证方法、测试数据准备方式、日志/DB 证据链沉淀为 `learningCandidate` 事件
- 在 test.md 写入 A/B/C/D 置信度
- 复核并更新 technical-plan.md：实际实现若和最初方案不一致，补齐真实实现路径、关键文件/类、风险与验证计划，方便人工先看方案再审 diff
- 若存在新增/变更的表、配置、Topic/Group、Job、开关、接口或上线人工动作，创建/复核 release-manifest.md，不能遗漏
- 完成代码审查门禁：生成/确认 code-review-ai.md 或 review.md，并在 review.md 顶部写明 `Review Gate: PASS` / `BLOCKED` / `WAIVED`

## 禁止
- 只用接口成功作为通过结论
- 缺少 tid 时宣称链路验证通过
- 代码审查门禁未通过或未豁免时推进到测试中
- 忽略 ERROR/Exception/consumeFail/rollback 等反向证据

## 完成标准
- 核心场景至少达到 B 级证据
- review.md 或 code-review-ai.md 已给出明确门禁结论（PASS/BLOCKED/WAIVED）
- test.md 留下可复用验证链路和证据摘要
