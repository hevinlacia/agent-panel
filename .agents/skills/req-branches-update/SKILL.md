---
name: req-branches-update
description: 更新或新建需求目录下的 branches.json（Agent Panel 代码差异比对用的精确分支标识文件），从 git 实测 remote 名、仓库路径和分支同步状态，保持 version 2 格式。
allowed-tools: ["bash", "read", "write", "edit", "glob", "grep"]
---

# Req Branches Update

用于：维护需求目录下的 `branches.json`，让 Agent Panel 做代码差异比对时精确读取项目仓库和分支，不再回退到从 `branch.md` 兜底解析。

适用：
- 用户说"更新 branches.json""补充 branches.json""生成 branches.json"
- 新需求分支确定后、发布预检前，需要给 Agent Panel 提供精确 diff 标识
- 需求涉及多个仓库/分支，需要补充或修正 branches.json

不适用：
- 创建或更新需求的其他文件（用 `req-create`）
- 读取 branches.json 做发布预检（用 `req-release-check`）
- 代码实现、仓库探索、调用链分析

## Trigger

- "更新 branches.json" / "补充 branches.json" / "生成 branches.json"
- "branches.json 不准确" / "branches.json 缺仓库"
- "给 Agent Panel 配 diff 分支" / "代码差异比对分支"

## branches.json 格式（version 2）

```json
{
  "version": 2,
  "updatedAt": 1783930493958,
  "repos": [
    {
      "repoName": "yl-cwhsea-wms-outbound-api",
      "branches": ["hevin.yang/feature/whole-order-allocation"],
      "role": "后端",
      "path": "~/Developer/company/WMS/backend/yl-cwhsea-wms-outbound-api/"
    }
  ]
}
```

| 字段 | 说明 |
| --- | --- |
| `version` | 固定 `2` |
| `updatedAt` | 当前 epoch 毫秒时间戳，`date +%s%3N` 获取 |
| `repos` | 数组，每个元素一个仓库 |
| `repoName` | 仓库名，取 `git remote -v` 的 origin URL 最后一段，去掉 `.git` |
| `branches` | 该需求在该仓库使用的分支数组（需求分支，用于 diff 比对；多分支时全部列出） |
| `role` | 角色描述，如 `后端`、`前端`、`BFF`、`后端-ES数据源`、`PDA` |
| `path` | 仓库本地路径，`~/` 开头，结尾带 `/`；由 `git rev-parse --show-toplevel` 实测后把 `/home/<user>` 替换为 `~` |
| `baseRef` | 可选。该仓库的 PRO diff 基线（如 `origin/production`）。省略时 Agent Panel 自动判断：前端（role=`前端` 或 path 含 `frontend/`）用 `origin/production`，其余用 `origin/master`。仅当自动判断不准时才显式填写 |

## Workflow

### 1. 定位需求目录

优先从当前 session 绑定的需求获取 `req-id` 和目录；否则由用户指定。路径形如 `~/.agents/req/<project>/<req-id>/`。

### 2. 确认涉及的仓库和分支

从 `branch.md`、`notes.md` 或用户说明收集本次需求涉及的仓库。对每个仓库执行：

```bash
cd <repo-path>
git remote -v | head -1          # 取 origin URL 最后段去 .git => repoName
git rev-parse --show-toplevel    # => path（/home/<user> 替换为 ~）
git fetch origin 2>&1 | tail -3  # 刷新远端引用，只读
```

分支同步确认：

```bash
git rev-parse <branch> origin/<branch>  # 两者相等 = 已 push
```

- `branches` 只放**需求分支**（用于 diff 比对的来源分支），不放目标分支（test/UAT/master）。
- 已 fast-forward 合入主分支的临时补丁分支**不单独列出**。
- 多仓库需求逐个收集，每个仓库一个 `repos` 元素。

### 3. 更新或新建

- **已存在** `branches.json`：读取现有内容。保留未变更仓库的条目，更新或追加变更仓库的条目；刷新 `updatedAt`。
- **不存在**：按 version 2 格式新建。

### 4. 写入并校验

写入 `<req-dir>/branches.json`，然后校验 JSON 合法性：

```bash
python3 -m json.tool <req-dir>/branches.json
```

校验通过即完成。

## Required Checks

- `version` 固定为 `2`
- `repoName` 必须从 `git remote -v` 实测，不要手填或照抄需求文件
- `path` 必须由 `git rev-parse --show-toplevel` 实测，`/home/<user>` 替换为 `~`，结尾带 `/`
- `branches` 中每个分支必须确认 `本地 SHA == origin SHA`（已 push），否则提示用户先 push
- `updatedAt` 使用 `date +%s%3N` 的当前时间戳
- 用 `python3 -m json.tool` 校验 JSON 合法后才算完成
- 不要在文件中写入真实 token、密码、Cookie、私钥
- `branches.json` 与 `branch.md` 互补：前者给机器做 diff，后者给人看合并轨迹，两者都保留
- `baseRef` 通常无需填写：前端仓库（role=`前端` 或 path 含 `frontend/`）自动用 `origin/production`，后端自动用 `origin/master`；仅当自动判断不准（如非 WMS 项目或特殊基线）时才显式写入

## Final Response

```text
✅ 已更新: <req-dir>/branches.json
- 仓库数: <n>
  - <repoName>: <branches> (<role>) @ <path>
- updatedAt: <ts>
- 校验: JSON 合法
```

若存在分支未 push，明确提示：

```text
⚠️ 分支 <branch> 本地与 origin 不一致，已写入但建议先 push 再比对 diff
```
