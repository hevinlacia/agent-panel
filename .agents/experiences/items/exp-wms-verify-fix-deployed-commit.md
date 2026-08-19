# 修复验证未生效：先核对环境部署 commit 再怀疑代码

用于：test/UAT 复验修复时接口行为仍与旧版本一致，快速判断是「环境没部署」还是「代码没改对」。
触发词：修复没生效、复验失败、test环境未部署、origin/test不含commit、部署版本核对、Ylops token过期、THRK过滤没生效。
不适用：接口直接报错、或已确认部署包含修复后的代码问题排查。

## 模式

复验时行为同旧版，第一步核对目标环境部署分支是否包含修复 commit：

1. `git fetch origin <env-branch>`（如 origin/test）
2. `git branch -r --contains <fix-commit>` 或在 GitLab 比对环境分支最新提交
3. 分支包含修复但行为未变 -> 再排查代码逻辑/配置/缓存/数据

不要一上来就重新读代码找 bug。

## 案例（WMS-020 THRK 过滤，2026-08-11）

- 现象：`POST /wms-web/pda/receipt-list/list`（SH.001，pageSize=50）返回含 14 条 receiptType=THRK，疑似修复无效。
- 核对：origin/test 原不含修复 commit 2c050138 -> 实为 test 环境未部署，代码无问题。
- 处理：Agent Panel 把需求分支合并到 test（origin/test 已含 2c050138），再触发部署。
- 阻塞：Ylops token 过期（access 约 36 天失效 + refresh 格式异常无法自动刷新），部署挂起，复验顺延；需用户从浏览器提供新 token。
- 复验基准：SH.001 test 库 THRK 可收货单 2621（100:568 / 101:682 / 200:1371），部署后应全部从列表消失；抽查 pageSize=50 回查 DB 无 THRK code。

## 关联

- exp-wms-dts-version-mismatch：同类模式（DTS 模板日志不生效，根因同样是部署版本不含修复）。
