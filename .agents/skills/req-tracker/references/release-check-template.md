# <req-id> 上线前预检

> 预检时间: <YYYY-MM-DD HH:MM>
> 预检人: <name>
> 目标环境: UAT / PRE / PROD

## 1. 代码分支

- [ ] 源分支所有 commit 已合入目标分支
- [ ] PR 状态: merged
- [ ] 没有遗留未合的 hotfix / 临时分支
- [ ] 目标分支 CI/CD 通过

证据:

```text
git status
git log <target>..HEAD --oneline
gh pr view <pr-id> --json state,mergedAt
```

结论: OK / 需关注 / 阻塞 — <说明>

## 2. DB 变更

- [ ] DDL 脚本已合并到主干
- [ ] DML 脚本 / 数据订正已准备好
- [ ] 回滚 SQL 已准备好
- [ ] 执行方式确认:Flyway 自动 / DBA 手动
- [ ] 已和 DBA / 值班对齐执行窗口

证据: `config-changes.md` 中 DB 章节

结论: OK / 需关注 / 阻塞 — <说明>

## 3. Apollo / Nacos 配置

- [ ] 所有 namespace 已在目标环境 UAT 验证
- [ ] 待发布配置已记录在 `config-changes.md`
- [ ] 生产配置未提前推送(只发 UAT / PRE)
- [ ] 灰度策略 / 白名单 / 开关已确认
- [ ] 回滚值已就绪(配置中心保留旧值 / Git 留底)

证据: Apollo / Nacos 后台截图或 API 查询

结论: OK / 需关注 / 阻塞 — <说明>

## 4. 测试结论

- [ ] 主流程用例全部通过
- [ ] 异常 / 边界用例已覆盖
- [ ] 回归范围已执行
- [ ] 性能 / 容量(如有)达标
- [ ] 已知未覆盖点已和测试 / 业务对齐

证据: `test.md`

结论: OK / 需关注 / 阻塞 — <说明>

## 5. 回滚方案

- [ ] 代码回滚方案(revert commit / 切旧版本)
- [ ] 配置回滚方案(Apollo / Nacos 切旧值)
- [ ] 数据回滚方案(回滚 SQL / 标记脏数据)
- [ ] 回滚触发条件(监控指标 / 错误率阈值)已和值班对齐
- [ ] 回滚决策人和审批流程已确认

证据: `notes.md` + `config-changes.md`

结论: OK / 需关注 / 阻塞 — <说明>

## 综合结论

- 阻塞项: <list,空表示可发布>
- 需关注项: <list,需要带上线,但有人盯>
- 决策: 可发布 / 暂缓发布 / 需 PM 决策

## 签发

- 开发: <name> <时间>
- 测试: <name> <时间>
- 运维 / DBA: <name> <时间>
- PM: <name> <时间>
