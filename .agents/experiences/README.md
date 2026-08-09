# Experiences

用于：存放短期、特化、带上下文的排障经验、踩坑记录和实践结论。
触发词：经验、踩坑、排障、过期风险、历史处理方式。
不适用：长期稳定、可重复执行的流程；这类内容应提升为 skill。

## Format

每条记录使用 Markdown + frontmatter，至少包含：

```yaml
---
id: exp-domain-topic
title: 标题
kind: experience
type: experience
domain: general
project: agent-panel
scope: project
status: active
confidence: medium
created_at: 2026-08-01T00:00:00Z
updated_at: 2026-08-01T00:00:00Z
last_verified_at: 2026-08-01T00:00:00Z
summary: 一句话摘要
trigger_terms: 关键词1, 关键词2
related_skills: skill-name
---
```

推荐正文结构：现象、根因、处理方式、验证方式、过期风险。
