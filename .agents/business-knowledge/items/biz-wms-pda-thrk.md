# PDA 收货列表业务规则（WMS-020）

用于：PDA 收货列表/扫码收货相关的过滤规则、货主参数判断逻辑速查。
触发词：PDA收货列表、THRK排除、到货确认、卸货登记、isArrivalCheckEnabled、maintenanceStatistics。
不适用：PC 收货列表自身逻辑（仅作对齐参照）、退货入库流程。

## 列表可见状态范围
- 后端 `PdaReceiptListServiceImpl#getReceiptList`（inbound-api）。
- 默认（不传 status）只查可收货状态：`IN_POOL=100`(待收货)、`RECEIVEING=101`(收货中)、`LOCATE_PENDING=200`(待提交)。
- 待上架(300)/上架中(301)/部分入库(901)/已关闭(900)/未收货关闭(999)/已删除(1000) 一律不展示（notIn 排除）。
- status 筛选传数字（100/101/200）；非法/未识别值 `resolveStatusFilter` 返回 `[-1]` 查空，不静默返回全量。

## 退货入库单排除（THRK）
- 列表查询 `wrapper.ne(ReceiptHeader::getReceiptType, THRK)`，与 PC 收货列表（`ReceiptCustomMapper.xml` 全局 `ExcludeReturnType`）保持一致；退货单走退货入库流程。
- 扫码校验（scanReceipt）同样拦截 THRK。
- 非 THRK 的退货类编码（如 TT_THRK）不在排除范围，与 PC 一致。

## 到货确认（货主参数 CompanySettings）
- `isDeliveryConfirmedEnabled()`：货主参数 `deliveryConfirmed=1` 控制是否需要到货确认。
- 开启且单据未确认时阻断收货，提示「到货确认后再进行收货」。
- **易混淆 DB 字段**：`deliveryConfirmed` 字段对应 `isDeliveryConfirmedEnabled()`（货主级开关）；`deliveryConfirmedFlag` 对应单据 `isDeliveryConfirmed()`（单据级确认状态）。命名相近、语义不同。

## 卸货登记（货主参数 CompanySettings）
- `isArrivalCheckEnabled(receiptType)`：需**同时满足** `maintenanceStatistics=1` 且 `arrivalCheckDocumentType` 非空（含该单据类型）才开启卸货登记。只看方法名容易漏掉第二个条件（场景C 实测：BMS001 maintenanceStatistics=0 时卸货登记关闭直进详情）。
- 是否已登记由 `goods_receiving_record_datail_code` 按 receiptCode 查 `existRecords` 判断。
- 货主参数（CompanySettings）查不到时阻断收货（「未查询到货主参数」），不静默绕过到货确认/卸货登记校验（与 `ReceivingScanBizService` 行为一致）。

## 扫码匹配规则
- `/scan` 按入库单号（code，即 ASN 单号）和 ERP 单号（erpOrderCode）匹配取第一条。
- 扫码入口不做货主过滤（与老链路 web scanOrder 一致）；列表入口按 Session 仓库+货主权限过滤。两口径差异需业务知悉。

## 证据
- 单测：`PdaReceiptListServiceImplTest` 19/19（含 THRK 排除 LambdaQueryWrapper 断言、防御分支）。
- test 环境实测：CS001 到货确认拦截 ✅ / JCKHZ001 卸货登记弹窗 ✅ / BMS001 直进详情 ✅。
- THRK test 基准（SH.001）：THRK 可收货单 2621（100:568 / 101:682 / 200:1371），修复部署后应全部从列表消失。

## 代码位置
- inbound-api：`controller/pda/PdaReceiptListController`、`service/impl/PdaReceiptListServiceImpl`（分支 hevin.yang/feature/WMS-020-pda-receipt-list）。
- wms-web：`controller/pda/PdaReceiptListController` + `feign/inbound/PdaReceiptListFeignClient`（BFF 转发）。
- PDA：jt-cloudwarehouse `ReceiptEntryActivity` + `ReceiptEntryViewModel`。
