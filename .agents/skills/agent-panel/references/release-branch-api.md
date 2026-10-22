# Release Branch API（整合需求发布分支）

> 上级入口：`../SKILL.md`。整合需求（ROLLUP）模型与创建方式见 `requirement-api.md`；本文件是发布分支 6 端点的完整契约。

用于：整合需求的发布分支全生命周期——按子需求 branches.json 并集创建预集成发布分支、逐个子需求合入、同步生产分支前进、生成生产 MR、封版并推进子需求状态。

适用：`consolidated=true` 的整合需求（ROLLUP 池）。普通需求/子需求单独发布不走本 API（普通需求合 test/UAT 用 `merge-branch`，生产合入走 GitLab MR 人工流程）。

不适用：WMS 生产分支的直接 push（红线：生产分支只能由用户走 GitLab MR；本 API 的 prod-mr 也只生成 MR，合入由人工完成）。

## 端点表

共用可选参数：`branch`（目标发布分支，id 如 `r1` 或完整分支名；**缺省取最新一条，active 优先**——多分支时建议显式传）、`confirm`（标注"必须"的操作 `confirm=true` 是硬前置，缺省 400）。所有端点 `reqId` 指父需求。

| 端点 | 专用参数 | 返回关键字段 |
|---|---|---|
| POST `/api/requirement/release-branch/create` | `reqId`(必填)、`repos`(可选，建分支范围过滤)、`branchPrefix`(可选)、`label`(可选) | `{ok, reqId, branch, branchId, repos[], skipped[], message}` |
| GET `/api/requirement/release-branch/list` | query `id` 或 `reqId` | `{ok, reqId, generatedAt, branches[]}`（每条含 mergedSubs + 每仓相对生产分支三点 diff 统计 diffFiles/diffAdditions/diffDeletions） |
| POST `/api/requirement/release-branch/merge-sub` | `reqId`、`subReqId`(必填)、`branch`、`repos`(可选，本次合并仓过滤)、`confirm=true`(必须) | `{ok, reqId, subReqId, branch, branchId, repos[], statusState}` |
| POST `/api/requirement/release-branch/sync-prod` | `reqId`、`branch`、`confirm=true`(必须) | `{ok, reqId, branch, branchId, repos[]}` |
| POST `/api/requirement/release-branch/prod-mr` | `reqId`、`branch`（无需 confirm） | `{ok, reqId, branch, branchId, generatedAt, results[]}` |
| POST `/api/requirement/release-branch/mark-released` | `reqId`、`branch`、`note`(可选发布备注)、`confirm=true`(必须) | `{ok, reqId, branch, branchId, status:"released", advancedSubs[]}` |

## create 语义

- 仓库范围 = 全部子需求 branches.json round-1 登记并集（按 repoName 去重）；PDA 客户端仓（role=="PDA" 或 path 含 `/pda/`）跳过（走 APK 发布）。
- 分支名 `{prefix}/{父reqId}[-label]-r{n}`：prefix 从子需求已登记分支推导（`x/feature/...` → `x/release`），显式 `branchPrefix` 优先，回退 `release`；序号 = registry 最大 r{n}+1，远端同名存在则继续跳号。
- 每仓 base = 生产分支（后端 `master`、前端 `production`），以 `origin/<base>` 建本地分支 + `push -u`，幂等（已存在不重建，branchStatus=exists）。
- **仅全部仓成功才写登记**；部分失败时不写登记，但已建分支依赖幂等重试。

## merge-sub 语义

- 校验：子需求属于父、非终态（已发布/已取消拒绝）、发布分支 active。
- 纯配置子需求（无 branches.json）仅登记不合并代码。
- 逐仓经隔离 merge worktree 合子需求分支 → 发布分支，结果状态 `merged|upToDate|conflict|failed|skipped`（发布分支未覆盖的仓 skipped）。
- 全部成功才：更新 mergedSubs + 子需求状态自动推进（`需求创建→开发中→已合入` 内部流转不走门禁；已在 自测中/测试中/发布就绪 的保留进度仅登记）+ 双向 note。冲突时按云仓规范在**需求分支侧**解决后重试。

## sync-prod 语义

逐仓把 `origin/<base_branch>`（生产分支最新）合入发布分支，冲突在发布分支侧解决；分支必须 active；全 merged|upToDate 才记 note。上线前 master 有新合入时先跑 sync-prod 再 MR。

## prod-mr 语义与前置检查

逐仓生成 发布分支 → 生产分支 的 GitLab MR（title=`{reqId} 生产发布：{repo} {source} -> {target}`，remove_source_branch=false）；已有同 source/target open MR 则复用（reused）；无差异 `no_diff` 跳过；MR webUrl 追加到父需求 note。

前置：①父需求 category 属 issue 家族（线上问题/测试问题/性能问题）→ 400 拒绝（复现代码禁止合生产）；②GitLab token 存在（`GITLAB_TOKEN`/`GL_TOKEN`/`.env.agent`），缺失整体失败；③逐仓 remote.origin.url 可解析出 GitLab 项目路径。

## mark-released 语义

分支必须非 released（重复封版 400）；置 status=released + releasedAt + note；mergedSubs 中非终态子需求统一推进「已发布」（内部流转不走门禁）。**MR 合入由人工在 GitLab 完成后**才执行封版。

## release-branches.json

位置：父需求（整合需求）目录下，与 branches.json 平行。agent 只读不手改（走 API 维护）：

```json
{
  "version": 1,
  "updatedAt": 0,
  "branches": [{
    "id": "r1",
    "name": "<prefix>/<父reqId>[-label]-r<n>",
    "createdAt": 0,
    "status": "active | released",
    "releasedAt": null,
    "note": null,
    "repos": [{ "repoName": "...", "role": null, "path": null, "baseBranch": "master|production", "baseCommit": "..." }],
    "mergedSubs": [{ "reqId": "...", "title": "...", "mergedAt": 0, "repos": ["仓1"], "note": null }]
  }]
}
```

## 典型流程

```
create（建发布分支）→ 逐子需求 merge-sub → （master 前进时）sync-prod
→ prod-mr（生成 MR）→ 人工在 GitLab 合 MR → mark-released（封版 + 子需求推进已发布）
```
