# <req-id> 分支信息

## 当前分支

- 源分支(开发): <例如 feature/REQ-2026-001-inbound-allowpart>
- 目标分支(集成): <例如 dev / uat / main>
- 基线 commit: <目标分支最新 commit SHA,前 8 位>
- HEAD commit: <源分支最新 commit SHA,前 8 位>

## 提交区间

<!-- 用 git log target..HEAD --oneline 填充 -->
```text
<commit-sha> <author> <subject>
<commit-sha> <author> <subject>
```

## 关联 PR / CR

- GitLab/GitHub PR: <url>
- Code Review 结论: 未开始 / 进行中 / 通过
- CR 主要反馈: <如果有,简述>

## 合并记录

<!-- 上线后回填 -->
- 合入时间: <YYYY-MM-DD HH:MM>
- 合入方式: squash / merge commit / rebase
- 是否冲突: 是 / 否
- 是否回滚过: 否 / 是(<原因>)

## 受影响目录(预检时核对)

- <仓库相对路径,例如 wms-inbound/src/main/java>
- <仓库相对路径,例如 wms-inbound/src/main/resources/mapper>
