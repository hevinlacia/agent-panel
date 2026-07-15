---
req-id: DASH-002
title: 需求管理与 Hermes 集成
status: 自测中
project: opencode-dashboard
owner: hevin
start-date: 2026-06-20
plan-release: 2026-06-22
---

# DASH-002 需求管理与 Hermes 集成

## 改动范围摘要

将 dashboard 的需求管理改为从 Hermes `~/.agents/req/` 目录读取数据，dashboard 只负责 session 关联和可视化看板。Hermes 负责需求的全生命周期管理（登记、分支、DB、Apollo、测试、发布预检）。

## 相关人

- 负责人(开发): hevin
- 关联项目: ~/GitHub/opencode-dashboard
