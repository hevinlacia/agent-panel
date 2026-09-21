---
name: req-branches-update
description: 更新或新建需求目录下的分支登记（branches.json 及修复轮次 branches-round-<n>.json），通过 Agent Panel API 全量替换登记：git 实测分支存在、去重拦截、原子写 version 2 格式。agent 不再手写登记文件。
allowed-tools: ["bash", "read", "glob", "grep"]
---

# Req Branches Update

用于：维护需求目录下的分支登记 `branches.json`（轮次 1）与修复轮次 `branches-round-<n>.json`，让 Agent Panel 做代码差异比对时精确读取项目仓库和分支。

**登记写入已收口到 Agent Panel API**：agent 不再手写登记文件，统一走 `PUT /api/requirement/branch-registration`（panel 负责 git 实测分支存在、role/path 推断、去重与原子写）。手写文件会被绕过全部校验，且与 panel 的 diff 去重保护脱节。

适用：
- 用户说"更新 branches.json""补充 branches.json""生成 branches.json"
- 新需求分支确定后、发布预检前，需要给 Agent Panel 提供精确 diff 标识
- 需求涉及多个仓库/分支，需要补充或修正登记
- 需求分支已合入生产后新起修复轮次，登记修复分支（`round` ≥ 2）

不适用：
- 创建或更新需求的其他文件（用 `req-create`）
- 读取登记做发布预检（用 `req-release-check`）
- 代码实现、仓库探索、调用链分析

## Trigger

- "更新 branches.json" / "补充 branches.json" / "生成 branches.json"
- "branches.json 不准确" / "branches.json 缺仓库"
- "给 Agent Panel 配 diff 分支" / "代码差异比对分支"
- "登记修复分支" / "新建修复轮次" / "round 2 分支登记"

## API

### 1. 查询当前登记

```bash
curl -s "http://127.0.0.1:7331/api/requirement/branch-registration?reqId=<req-id>&round=<n>"
```

返回 `scope.repos`（每仓 repoName/branches/role/path/baseRef）。PUT 是**全量替换**语义，先 GET 拿现状再构造完整清单。

### 2. 发现候选分支（复用扫描脚本，只 dry-run）

```bash
python3 ~/.agents/scripts/req-branches-scan.py <req-id> --dry-run
# 修复轮次：
python3 ~/.agents/scripts/req-branches-scan.py <req-id> --round 2 --dry-run
```

脚本输出候选仓库/分支/role 供构造清单。脚本的**写入模式已废弃**（绕过 panel 校验），只允许 `--dry-run` 做发现。

### 3. PUT 全量替换

```bash
curl -s -X PUT "http://127.0.0.1:7331/api/requirement/branch-registration" \
  -H "Content-Type: application/json" \
  -d '{
    "reqId": "<req-id>",
    "round": 1,
    "repos": [
      { "repoName": "yl-cwhsea-wms-outbound-api", "branch": "hevin.yang/feature/<req-id>-<desc>", "role": "后端" },
      { "repoName": "yl-cwhsea-wms-web-front", "branch": "hevin.yang/feature/<req-id>-<desc>", "role": "前端" }
    ]
  }'
```

| 字段 | 说明 |
| --- | --- |
| `round` | 轮次：省略 = 1（`branches.json`）；≥2 写修复轮次（`branches-round-<n>.json`） |
| `repos[].repoName` | 仓库名 |
| `repos[].branch` | 需求分支（diff 比对来源分支）；panel 会实测 `origin/<branch>` 存在性，写错分支名当场 400 |
| `repos[].role` | 可省略：panel 按路径推断（backend/ → 后端、frontend/ → 前端、pda/ → PDA、components → 后端-组件库）；需细分（如"后端-ES数据源"）时显式给 |
| `repos[].path` | 可省略：panel 在已登记条目的 workspace 下探测同名目录；解析不了会 400 要求显式提供（`~/` 开头） |
| `repos[].baseRef` | 可省略：前端自动 `origin/production`、后端自动 `origin/master`；仅自动判断不准时显式给 |
| `confirmBranchChange` | 同仓换分支默认 400（列出已登记分支）；确认替换带 `true` |
| `confirmRemoval` | 已登记仓库未出现在提交清单默认 400（防漏仓）；确认移除带 `true` |
| `verifyRemote` | 分支本地 `origin/<branch>` 不存在（可能未 fetch）时，带 `true` 走 `ls-remote` 联网确认 |

响应 `changes` = `{added, updated, unchanged, removed}` + `warnings`（分支变更记录）。

## 业务规则（agent 责任，panel 不校验的部分）

- **基线谱系**：`branches` 只登记以需求基线（后端 `origin/master` / 前端 `origin/production`）创建的分支；基于 `test`/`uat` 等环境分支的 fix 分支**不登记**（其上生产走独立 MR）。误登记后果：diff 视图把集成差异算进需求（出现他人提交）、merge 搭车带入无关文件（WMS-106 实证 112 文件 +8250/-4178）。
- **已 push**：panel 只能实测本地 `origin/<branch>` 追踪引用；分支未 push 时先 `git push` 再登记（或 PUT 时带 `verifyRemote: true` 由远端确认）。
- **修复轮次（round ≥ 2）**：不回改已封版的 `branches.json`；修复分支命名 `<username>/fix/<req-id>-<desc>` 或 `<username>/hotfix/...`（必须带 `/fix/`、`/hotfix/`，且不与旧轮次同名）；轮次文件在 diff 页用 Round 下拉切换。
- **废弃分支**：扫描可能列出早期废弃分支，对照 `branch.md` 逐个确认，不要照单全收。

## Required Checks

- PUT 前先 GET 现状；PUT 是全量替换，清单必须完整（漏仓会被 `confirmRemoval` 拦截）
- 每个分支确认基于需求基线（`git merge-base <branch> origin/master|origin/production`）且已 push
- 收到 400 时按错误信息修正（panel 会列出已登记分支/缺失仓库/非法分支名），不要绕过 API 手写文件
- `branches.json` 与 `branch.md` 互补：前者给机器做 diff，后者给人看合并轨迹，都保留
- 不要在登记中写入真实 token、密码、Cookie、私钥

## Final Response

```text
✅ 已登记: <req-dir>/<branches|branches-round-<n>>.json（轮次 <n>）
- changes: +<added> ~<updated> =<unchanged> -<removed>
  - <repoName>: <branch> (<role>)
- warnings: <分支变更/提示，无则省略>
```

存在未 push 分支时提示：

```text
⚠️ 分支 <branch> 本地与 origin 不一致，已登记但建议先 push 再比对 diff
```
