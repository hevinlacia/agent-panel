---
name: agent-panel
description: 本机 Agent Panel 平台 API 统一入口：需求 CRUD/状态推进/分支合并/检查服务 API、整合需求发布分支、上线资产附件制、ONES 工时联动、业务知识与经验库查询维护（meta 摘要优先）、AI 代码审查门禁（一键备料 + 多轮审查 + annotations 落盘）、接口测试联动与造数执行 dispatch、session 待总结标记。触发词：agent panel、面板 API、需求 API、需求状态推进、合并需求分支、发布分支、工时、附件、接口测试、造数执行、检查服务、知识 API、查业务知识、经验查询、代码审查门禁、review 材料、标记总结、mark session。
allowed-tools: ["bash", "read", "glob", "grep"]
---

# Agent Panel 平台 API（统一入口）

用于：与本机 Agent Panel（本地 Web 控制面板）的 API 交互——需求的接口层操作、知识库检索维护、代码审查门禁编排、会话总结标记。

不适用：需求业务工作流本身（何时建什么文件、生命周期规范，用 `req` skill）；直接手写需求文件（API 优先，文件兜底在 `req` skill create.md）。

## 能力路由

| 场景 | 读 |
|---|---|
| 需求 CRUD、状态推进、分支合并、检查服务、附件制、工时、门禁语义 | `references/requirement-api.md` |
| 整合需求（ROLLUP）发布分支：create/merge-sub/sync-prod/prod-mr/mark-released | `references/release-branch-api.md` |
| 业务知识/经验查询维护 → **知识库**（改名 2026-10-20）：索引/筛选/查看/编辑全能力（meta 摘要优先，按需展开 items 正文） | `references/knowledge-api.md` |
| AI 代码审查门禁：一键备料（`POST /api/requirement/review-materials`）、多轮审查、annotations 落盘 | `references/code-review.md`（配套方法论 `review-methodology.md`） |
| 接口测试联动（目录/详情/dry-run/trigger）与造数能力检索/执行（`/api/testdata/run`） | `references/testdata-api.md` |
| 标记当前/指定 session 为待总结（面板空闲 1 小时后自动 fork 总结） | `references/session-summary-mark.md` |

## 共享约定

- **端点/门禁/状态机契约以 `GET /api/schema` 为准**（版本/29 端点/写通道白名单与乐观锁/状态机/7 门禁格式契约/知识查重，与代码同仓演进）；本 skill 文档只写策略与组合，契约细节冲突时以 schema 为准。

- API base 与鉴权方式见各子文档；脚本与 curl 均只读优先，状态变更需用户确认。
- **UAT 部署前先 `GET /api/config` 检查 `deployFreeze.enabled`**（发版冻结期 ylops_deploy.py 会直接拒绝，见 requirement-api.md「人工核查阶段、状态门禁与发版冻结」节）。
- `req` skill 是需求业务工作流的上层入口：先判断业务场景，再按本 skill 查 API 细节（组间弱耦合，两组各自可独立使用）。
