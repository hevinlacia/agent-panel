# Agent Panel 需求事件 API：*Vec 字段必须传 JSON 数组，字符串值会 400

用于：agent 调用 `POST /api/requirement/events` 记录结构化事件时正确构造 payload，避免 400 Failed to deserialize；需要验证 payload 时用 `dryRun` 而不是真实写入。
触发词：requirement events、evidence 字段、Failed to deserialize、dryRun、事件删除、405、recordEvent
不适用：/api/knowledge（知识/经验条目）或需求文档编辑接口。

## 已验证事实（2026-09-17，源码 + dryRun 探针）

- `RequirementEventForm`（src/requirement_service.rs，serde camelCase）：`evidence`/`decisions`/`todos`/`relatedFiles`/`relatedKnowledgeIds`/`triggerTerms`/`relatedRepos`/`relatedTables`/`relatedApis`/`tags` 均为 `Vec<String>`；`event_type` 带 alias `type`。
- `evidence` 传**字符串** → HTTP 400 "Failed to deserialize the JSON body into the target type"；传**字符串数组** → HTTP 200（2026-09-02 会话曾误判为「API 不接受 evidence 字段」，实为 payload 类型错误）。
- `dryRun: true` 走完整反序列化校验但不落盘（实测 events.jsonl 无写入），是验证 payload 的安全探针。
- `DELETE /api/requirement/events` → 405：误发事件无法经 API 删除，只能手工编辑 events.jsonl。

## 实践建议

- 事件类列表字段一律写 JSON 数组；不确定字段类型时先 `dryRun: true` 探测。
- 固定提示词推荐的 evidence/relatedFiles/confidence 等字段本身可用，按正确类型传参即可；不要因一次 400 就断言 API 不支持某字段。
