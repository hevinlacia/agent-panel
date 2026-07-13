# Release Check Items

用于：子 agent 执行 5 项检查时的详细步骤参考和输出规范。

## 1. 分支验证

### 步骤

```bash
# 刷新远端引用
git fetch origin --prune

# 确认远端 source branch 存在
git rev-parse --verify --quiet origin/<source-branch>

# 确认关键 commit 已在 target branch
git branch -r --contains <commit-hash>

# 确认无遗漏 commit（source 有但 target 没有的）
git log --oneline origin/<target>..origin/<source>
```

### 判断标准

| 情况 | 结论 |
|------|------|
| source 分支不存在 | ❌ 分支不存在 |
| 关键 commit 不在 target | ❌ commit 未合入 |
| source 有遗漏 commit | ⚠️ 有未合入 commit |
| 全部合入 | ✅ OK |

### 常见陷阱

- branch.md 写的 `feature/xxx` 实际远端是 `hevin.yang/feature/xxx`
- branch.md 的 target 写 `master` 但实际合入的是 `test`
- branch.md 的 merge status 仍写"开发中"但实际已合入

## 2. 应用归属验证

### 步骤

```bash
# 确认 project path 是 git 仓库
git -C <project-path> rev-parse --git-dir

# 确认改动文件属于该仓库
git -C <project-path> diff --name-only origin/master...origin/<source>
```

### 判断标准

| 情况 | 结论 |
|------|------|
| 路径不是 git 仓库 | ❌ 应用名与仓库不匹配 |
| 改动文件不在该仓库 | ❌ 应用名与仓库不匹配 |
| 改动文件在该仓库子模块内 | ✅ OK |

### 注意

- 旧单体（如 `yl-cwhsea-wms-api`）是 Gradle 多模块，子模块（`wms-task`、`wms-shipping`）不是独立应用
- 如果 branch.md 写了子模块名而非应用名，提示用户改为应用名

## 3. DB 变更检查

### 步骤

```bash
# 检查 SQL/migration 文件变更
git diff --name-only origin/master...origin/<source> -- '*.sql' '*.ddl' '*.dml' '*migration*' '*flyway*' '*liquibase*'

# 检查 diff 内容是否包含 DDL/DML 语句
git diff --unified=0 origin/master...origin/<source> | rg -i "alter table|create table|drop table|insert into|update [a-zA-Z0-9_]+ set|delete from"
```

### 判断标准

| 情况 | 结论 |
|------|------|
| 无 SQL 文件，diff 无 DDL/DML | ✅ 无 DB 变更 |
| 有 SQL 文件 | ⚠️ 需执行（附文件清单） |
| diff 含内联 DDL/DML | ⚠️ 需执行（附语句摘要） |

### 注意

- `@Value` 注解里的 `DELETE FROM` 等字符串不是 DB 变更，要排除
- config-changes.md 记录的 DB 变更要与实际 diff 交叉核对

## 4. Apollo/Nacos 配置检查

### 步骤

```bash
# 检查配置文件变更
git diff --name-only origin/master...origin/<source> -- '*.yml' '*.yaml' '*.properties'

# 检查 @Value 注解变更
git diff --unified=0 origin/master...origin/<source> | rg '@Value.*\$\{[^}]+\}'

# 提取新增/修改的配置 key 和默认值
# 格式: @Value("${<key>:<default>}") 或 @Value('${<key>:<default>}')
```

### 判断标准

| 情况 | 结论 |
|------|------|
| 无配置文件变更，无 @Value 变更 | ✅ 无配置变更 |
| @Value 有默认值 | ✅ 不必须手工配（默认值生效） |
| @Value 无默认值 | ⚠️ 必须配置（附 key 清单） |
| 有 .yml/.properties 变更 | ⚠️ 需检查是否需要推送配置 |

### 可选：Apollo 查询

用 `apollo-config-query` skill 查测试环境 Apollo 是否已有同名 key：
- key 已存在 → 配置已在 Apollo 管理，代码默认值不生效
- key 不存在 → 当前依赖代码默认值

**只检查 key 是否存在，不打印配置值。**

### 注意

- `@Value("${key:default}")` 有冒号和默认值 → 不必须手工配
- `@Value("${key}")` 无冒号无默认值 → 必须手工配
- config-changes.md 记录的 key/default 要与代码实际交叉核对
- 移除的 @Value key（如按仓白名单删除）不需要手工清理 Apollo，最多提示

## 5. 清单文档新鲜度

### 检查项

| 文件 | 检查内容 |
|------|---------|
| `meta.md` | frontmatter status 是否与 `state.json` 一致 |
| `branch.md` | source branch 名是否与远端实际分支名一致 |
| `branch.md` | merge status 是否反映实际 git 状态 |
| `branch.md` | target branch 是否正确（本批上线应统一指向 test 或 master） |
| `config-changes.md` | 记录的 key 和 default 是否与代码实际一致 |
| `notes.md` | 待跟进列表中已完成项是否已勾掉 |

### 判断标准

| 情况 | 结论 |
|------|------|
| 全部一致 | ✅ OK |
| 有过时字段 | ⚠️ 过时（附字段清单） |

## Subagent Prompt Template

```
你是发布检查子 agent。对以下需求执行只读检查，不修改任何文件。

需求信息：
- req-id: <req-id>
- title: <title>
- 仓库路径: <repo-path>
- source branch: <source>
- target branch: <target>
- 关键 commit: <commit-hash>
- config-changes.md 记录的 key: <key-list>

执行以下 5 项检查：
1. 分支验证：git fetch → 确认远端分支存在 → 确认 commit 已在 target
2. 应用归属：确认改动文件属于该仓库
3. DB 变更：检查 SQL/migration 文件和内联 DDL/DML
4. Apollo/Nacos：检查 @Value 变更，判断是否必须手工配
5. 清单文档新鲜度：对比 branch.md/meta.md 与实际状态

返回 JSON：
{
  "req_id": "<req-id>",
  "application": {"ok": true/false, "detail": "..."},
  "branch": {"ok": true/false, "detail": "..."},
  "db": {"required": true/false, "items": [...]},
  "config": {"required": true/false, "items": [...]},
  "checklist": {"ok": true/false, "stale_fields": [...]},
  "blockers": [...],
  "warnings": [...]
}
```

## Output Schema

子 agent 返回 JSON，主 agent 汇总为表格。

| 字段 | 类型 | 说明 |
|------|------|------|
| req_id | string | 需求 ID |
| application.ok | bool | 应用归属是否正确 |
| branch.ok | bool | 分支是否已正确合入 |
| db.required | bool | 是否有必须手工执行的 DB 变更 |
| db.items | array | DB 变更清单 |
| config.required | bool | 是否有必须手工配置的 Apollo/Nacos key |
| config.items | array | 配置变更清单 |
| checklist.ok | bool | 清单文档是否新鲜 |
| checklist.stale_fields | array | 过时字段列表 |
| blockers | array | 阻塞项（空数组 = 可发布） |
| warnings | array | 需关注项 |
