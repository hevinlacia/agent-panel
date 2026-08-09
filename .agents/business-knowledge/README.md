# Business Knowledge

用于：存放业务事实、规则、状态流转、接口/表关系等相对特化的知识。
触发词：业务知识、状态流转、业务规则、接口字段、表关系。
不适用：稳定且可执行的通用流程；这类内容应沉淀为 skill。

## Format

每条记录使用 Markdown + frontmatter，至少包含：

```yaml
---
id: biz-domain-topic
title: 标题
kind: businessKnowledge
type: business_knowledge
domain: general
project: agent-panel
scope: project
status: active
confidence: medium
created_at: 2026-08-01T00:00:00Z
updated_at: 2026-08-01T00:00:00Z
summary: 一句话摘要
trigger_terms: 关键词1, 关键词2
related_skills: skill-name
---
```

推荐正文结构：概述、规则、相关接口、相关表、注意事项。
