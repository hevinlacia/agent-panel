# PDA 收货列表接口（/wms-web/pda/receipt-list）

用于：PDA 收货列表/扫码校验两个接口（WMS-020 新增）的路径、参数、响应速查。
触发词：PDA收货列表接口、pda-receipt-list、receipt-list/scan、PDA扫码校验接口、canReceive、blockReason。
不适用：PC 收货列表接口、退货入库流程；字段级完整契约见需求目录 `.agents/req/WMS-020-pda-receipt-list/api-spec.md`。

## 路径与链路

| 层 | 路径 |
|---|---|
| PDA 前端调用（网关） | `POST /wms-web/pda/receipt-list/list`、`POST /wms-web/pda/receipt-list/scan` |
| wms-web BFF Controller | `/pda/receipt-list/*`（FeignClient path `/inbound-api/pda-receipt-list`） |
| inbound-api 业务层 | `/pda-receipt-list/*`（Controller: `controller/pda/PdaReceiptListController`） |

老路径 `wms-pda/pda-receipt-list/*`（pda-api 弃用前）已废弃。

## 列表查询 /list

- 请求：`status`（100 待收货 / 101 收货中 / 200 待提交；不传=全部可展示状态）、`pageNum`、`pageSize`。
- 响应：`totalCount` + `results[]`，字段含 `id`、`asnNo`、`ownerCode`、`ownerName`（已翻译，未匹配返回编码）、`erpOrderNo`、`expectedArrivalTime`、`asnStatus`、`asnStatusText`（@DictMapping 自动填充）、`totalSkuCount`（receipt_header.total_lines）、`totalQty`、`createTime`、`buttonType`（待收货→「去收货」，收货中/待提交→「继续收货」）。
- 默认排除：待上架(300)/上架中(301)/部分入库(901)/已关闭(900)/未收货关闭(999)/已删除(1000)，以及退货入库单（receiptType=THRK，与 PC 一致）。
- 非法/未识别 status：查空（[-1]），不静默返回全量。
- 数据权限：按登录 Session 自动过滤仓库+货主，前端不传仓库/货主。

## 扫码校验 /scan

- 请求：`scanCode`（入库单号 code（即 ASN 单号）或 ERP 单号 erpOrderCode，取第一条）。
- 响应核心字段：`canReceive`、`blockReason`、`needArrivalConfirm`/`arrivalConfirmed`、`needUnloadRegister`/`unloadRegistered`、`receiptId`（WMS-020 新增，免前端兜底查 id）、`receiptCode`、`erpOrderNo`、`inboundStsNew`/`inboundStsText`、`buttonType`。
- 列表点「去收货/继续收货」同样调用 /scan（传列表项 asnNo），canReceive=true 再跳收货详情页。
- blockReason 取值：未查询到符合条件的单号 / 到货确认后再进行收货 / 该单据已完成收货 / 该单据已关闭 / 该单据已取消 / 该单据当前状态不允许收货 / 扫描单号不能为空 / 未查询到货主参数（CompanySettings 缺失时阻断）。
- 扫码入口不做货主权限过滤（与老链路 web scanOrder 一致）；列表入口按货主过滤，口径差异需业务知悉。
- THRK 退货入库单在 /scan 同样拦截禁收。

## 业务规则与证据

- 状态过滤/THRK/到货确认/卸货登记判断逻辑详见 `biz-wms-pda-thrk`。
- 单测：inbound-api PdaReceiptListServiceImplTest 19/19、wms-web PdaReceiptListControllerTest 6/6（BFF 透传+异常透抛）。
- test 实测：S1-S6 通过（需求 test.md 自测记录，2026-08-01）。
