# <req-id> 配置变更

## DB 变更

### DDL(建表/改表)

| # | 操作 | 表名 | 脚本路径 | 目标环境执行方式 | 是否回滚脚本 | 备注 |
| - | --- | --- | --- | --- | --- | --- |
| 1 | CREATE TABLE | <table> | <repo 相对路径,如 wms-inbound/db/V20260608__add_field.sql> | Flyway 自动 / DBA 手动 | 是,见 V20260608__add_field_rollback.sql | <备注> |
| 2 | ALTER TABLE | <table> ADD COLUMN | <path> | DBA 手动 | 是,见 <path> | <备注> |

### DML(数据订正)

- <INSERT/UPDATE/DELETE 描述,影响行数,执行时间窗口>
- 必须放在低峰期 / 申请停机窗口:是 / 否

### 回滚 SQL

- <path 或文本位置>

## Apollo 变更

| # | appId | env | namespace | key | 旧值 | 新值 | 发布人 | 是否已发布 | 备注 |
| - | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | wms-inbound | UAT | application | xxx.enabled | false | true | <name> | 待发布 | <备注> |

## Nacos 变更

| # | dataId | group | env | 旧值 | 新值 | 发布人 | 是否已发布 | 备注 |
| - | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | wms-inbound.yaml | DEFAULT_GROUP | UAT | <旧值> | <新值> | <name> | 待发布 | <备注> |

## 灰度 / 回滚预案

- 灰度策略: <白名单 / 比例 / 开关>
- 回滚步骤:
  1. <Apollo/Nacos 配置回退到旧值>
  2. <DB 数据回退 / 标记脏数据>
  3. <代码 revert 或切回旧版本>
- 触发回滚的信号: <监控指标 / 错误率阈值>

## 风险点

- <例如:Apollo 配置改动会影响 5 个应用,需要逐个确认>
- <例如:DB 变更在生产可能锁表 30s,需要选低峰期>
