---
name: req-release-check
description: 批量检查待上线需求的分支、配置、测试、Review 和回滚风险，并生成 release-check.md。
allowed-tools: ["bash", "read", "write", "edit", "glob", "grep"]
---

# Req Release Check

用于：批量或单个检查“待上线”需求的发布就绪状态，聚焦分支、应用归属、DB/Apollo/Nacos、测试证据、Review 结论和回滚方案，并把结论写入 `release-check.md`。

适用：
- 用户说“上线检查”“检查待上线需求”“上线前预检”“release check”
- 用户给了 Agent Panel `?status=待上线` 链接或要求检查某个待上线需求
- 用户说“生成 release-check.md 后把需求推进到待上线”

不适用：
- 单个需求的开发过程跟踪（用 `req-tracker`）
- 单独更新需求状态或查询需求 API（用 `agent-panel-requirement-api`）
- 实际执行 DB 写入、Apollo/Nacos 配置修改或生产发布

## Trigger

- “上线检查” / “检查待上线需求” / “release check”
- “生成 release-check.md”
- “这批上线 N 个需求，检查一下”
- “先做 release check，再推到待上线”

## Workflow

### Step 1: 发现目标需求

优先用 Agent Panel API 拉全量，再在客户端按状态过滤；不要依赖 `?status=` 服务端过滤：

```bash
curl -sf http://localhost:7331/api/requirements | python3 -c '
import json, sys
data = json.load(sys.stdin)
for r in data.get("requirements", []):
    if r.get("status") == "待上线":
        print(f"{r.get('id')}\t{r.get('project')}\t{r.get('title')}")
'
```

如果 Agent Panel 不可用，回退扫描 `~/.agents/req/**/state.json`，筛选 `status == "待上线"`。若用户指定单个 req-id，则只检查该需求。

### Step 2: 收集需求材料

从每个需求目录读取：

- `branch.md` / `branches.json`：source branch、target branch、repo path、commit 或合并状态
- `config-changes.md`：DB、Apollo、Nacos、MQ 或其他发布前配置
- `test.md`：自测/UAT 证据、失败项、反向检查
- `impact.md`：核心链路、风险等级、回滚方案
- `review.md` / `code-review.json`：待上线 review 结论和未关闭问题

### Step 3: 执行检查

对每个需求检查以下项：

| 检查项 | 结论取值 | 阻塞级别 |
|--------|---------|---------|
| 分支验证 | OK / 分支不存在 / commit 未合入 / unknown | 阻塞 |
| 应用归属 | OK / 应用名与仓库不匹配 / unknown | 阻塞 |
| DB 变更 | 无 / 需执行 / 待确认 | 需关注 |
| Apollo/Nacos | 无 / 需配置 / 待确认 | 需关注 |
| 测试证据 | OK / 证据不足 / 失败未关闭 | 阻塞 |
| Review 结论 | OK / 未执行 / 有未关闭项 | 阻塞或需关注 |
| 回滚方案 | OK / 缺失 / 不适用 | 需关注 |

需求数 ≤ 2 时直接检查；需求数较多时可并行拆分，但每个子任务只读代码和配置，不执行写操作。

### Step 4: 写入 release-check.md

将每个需求的检查结果写入 `<req-dir>/release-check.md`。推荐结构：

```markdown
# <req-id> Release Check

## Summary
- Result: ✅ 可上线 / ⚠️ 需关注 / ❌ 阻塞
- Checked at: <YYYY-MM-DD HH:mm>
- Scope: <仓库/分支/应用摘要>

## Checklist
| Item | Status | Evidence | Action |
| --- | --- | --- | --- |
| 分支验证 | OK/阻塞/unknown | <证据> | <动作> |
| DB 变更 | 无/需执行/待确认 | <证据> | <动作> |
| Apollo/Nacos | 无/需配置/待确认 | <证据> | <动作> |
| 测试证据 | OK/证据不足/失败 | <证据> | <动作> |
| Review 结论 | OK/未执行/有未关闭项 | <证据> | <动作> |
| 回滚方案 | OK/缺失/不适用 | <证据> | <动作> |

## Blocking Items
- <阻塞项；没有则写“无”>

## Attention Items
- <需关注项；没有则写“无”>
```

### Step 5: 汇总给用户

按需求汇总阻塞项和需关注项。若用户要求“生成后推进到待上线”，在 `release-check.md` 写入成功后调用 `agent-panel-requirement-api` 更新状态。

## Required Checks

- 执行前先在对应仓库 `git fetch origin --prune` 刷新远端引用；只读检查，不改代码。
- 分支名对比时注意个人命名空间前缀是否遗漏。
- `@Value` / 配置默认值判断：有默认值 → “不必须手工配”；无默认值且生产需要 → “必须手工配”。
- Apollo/Nacos 查询只检查 key 是否存在，不打印配置值。
- 必须写入或刷新 `release-check.md`，除非用户明确只要口头汇总。
- 不直接修改 `state.json`；状态流转走 Agent Panel API。

## Final Response

```text
🚀 上线检查汇总（N 个需求）

| req-id | 分支 | 配置 | 测试 | Review | 回滚 | 结论 |
|--------|------|------|------|--------|------|------|
| WMS-xxx | ✅ | 无 | ✅ | ✅ | ✅ | 可上线 |
| WMS-yyy | ❌ | ⚠️ | ✅ | ⚠️ | ⚠️ | 阻塞 |

已写入：
- ~/.agents/req/<project>/<req-id>/release-check.md

阻塞项：
1. <req-id>: <问题描述>

需关注：
1. <req-id>: <问题描述>
```

## Reference

- `references/check-items.md`：如存在，按其中更细的判断标准执行。
- `agent-panel-requirement-api`：需要推进状态或验证 API 时加载。
