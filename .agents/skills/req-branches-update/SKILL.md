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

### 2. 自动扫描多仓库（首选）

WMS 是多独立 git 仓库（`backend/`、`frontend/`、`pda/` 下各自 `.git`），手动逐仓库查分支易漏。优先用脚本统一扫描所有子仓库，找出含需求 ID 的分支：

```bash
# 预览将写入的内容（不实际写入）
python3 ~/.agents/scripts/req-branches-scan.py <req-id> --dry-run
```

脚本自动实测 `repoName`（`git remote`）、`path`（`git rev-parse --show-toplevel`），按规则推断 `role`。确认无误后写入：

```bash
python3 ~/.agents/scripts/req-branches-scan.py <req-id>
```

需求目录不在默认位置时用 `--req-dir` 指定：

```bash
python3 ~/.agents/scripts/req-branches-scan.py <req-id> --req-dir <req-dir>
```

> 脚本 `--check` 模式只对比不写入、输出 JSON 状态，供 `req-branch-watcher` 扩展自动检测缺失时调用，agent 一般不用。

### 3. 检查与补充

脚本输出后必须人工核对：

- **废弃分支**：脚本列出所有含需求 ID 的本地分支，可能含早期废弃分支。对照 `branch.md` 确认每个分支是否属于本次需求，多余的用 `edit` 从 `branches` 数组删除。
- **校正 `role`**：脚本按规则推断基础角色（`后端`/`前端`/`后端-BFF`/`PDA`/`后端-组件库`）。如需细分（如"后端-ES数据源"），用 `edit` 修正。
- **未 push 分支**：脚本会提示未 push 的分支。已 push 才能被 Agent Panel 做远端 diff，未 push 的先 `git push` 再登记。
- **未扫到的已有仓库**：脚本列出"本次未扫到分支的已有仓库"（可能分支已合并清理），确认是否仍需保留。

### 4. 写入并校验

脚本写入后自带 JSON 校验。手动写入时校验：

```bash
python3 -m json.tool <req-dir>/branches.json
```

### 手动方式（脚本不可用时）

从 `branch.md`、`notes.md` 收集涉及的仓库，对每个仓库执行：

```bash
cd <repo-path>
git remote -v | head -1          # origin URL 最后段去 .git => repoName
git rev-parse --show-toplevel    # => path（/home/<user> 替换为 ~）
git rev-parse <branch> origin/<branch>  # 两者相等 = 已 push
```

- `branches` 只放**需求分支**（diff 比对的来源分支），不放目标分支（test/UAT/master）。
- 已 fast-forward 合入主分支的临时补丁分支**不单独列出**。

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
