## 现象
WMS-133修复（outbound备份扩4字段+wms-api恢复SQL扩7字段）已部署test并生效（发送端代码+发送日志实证7字段已组装），但test精确复现INC-122后恢复warn仍打printDataId=null/carrierService=null：wms-trigger-cancel-topic消息里WaybillBackup只有{primaryWaybillCode, shipmentId, waybillGetStatus}3字段（WMS-072原结构）。

## 链路定位
outbound-api ShipmentRollbackBizService:399 senderTemplate.sendMessage发的是wms-cancel-shipment-topic（非直发wms-trigger-cancel-topic）；yl-cwhsea-wms-shipment-api仓shipment-core模块 com.yl.wms.shipment.core.stream.consumer.RocketConsumer.onCancelShipmentMessage（@StreamListener WMS_CANCEL_SHIPMENT_INPUT）消费后重构消息：注入session、shipmentIds→shipmentIdList改名，再发wms-trigger-cancel-topic。其备份类型是本模块com.yl.wms.shipment.core.model.dto.ShipmentCancelDTO内部类WaybillBackup，仅shipmentId/primaryWaybillCode/waybillGetStatus 3字段 → fastjson反序列化时4个新字段（secondWaybillCodes/printDataId/carrierService/carrierName）被丢弃，重序列化转发即瘦消息。

## 修复（已在WMS-133落地）
TriggerCancelMsgDTO.waybillBackups与ShipmentCancelDTO.WaybillBackup复用同一个类 → 只需在该内部类补4字段，一处改动两端生效。commit ec8ff343（分支hevin.yang/feature/WMS-133-cancel-restore-print-fields，已合并origin/test=7368af87）。兼容性：fastjson默认省略null字段+消费端null跳过set分支，旧消息（中转方未升级/备份值本为null）不受影响。

## 证据
- test消费开始日志22:20:26.342（group_5, tid c450fbf7f0864226a1e3636a9affa18c）：MQ1目标单1431224备份仅3字段；DTS证明回退前DB printDataId=inc122-pdd-order/carrierService=PDD存在
- outbound发送日志22:20:26.326/26.626 topic=wms-cancel-shipment-topic；22:08-22:09 shipment-admin RocketConsumer→『RocketMQ发送消息，topic: wms-trigger-cancel-topic』→wms-api消费开始毫秒级相接
- shipment-api仓master-260916无WMS-133分支无改动 → 影响面遗漏（technical-plan中转透传open question未验证即review PASS）

## 适用边界
- 所有『应用A发topic → 中转应用消费后重构/改名/加壳 → 转发topic → 最终消费方』的MQ链路，中转方只要持有强类型DTO（非raw JSON透传），新增字段都会在它那里被裁剪。
- 排查顺序：消费端收到字段不全 → 先看消息是否经过中转应用（grep中间topic发送日志）→ 读中转方DTO定义 → 修复在中转方DTO补字段。
- review检查清单应加一项：改动MQ消息体字段时，枚举该topic从生产者到最终消费者的完整转发链，逐跳核对中转方DTO字段集。
