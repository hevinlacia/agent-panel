机制判读（代码：yl-cwhsea-wms-shipment-api ShipmentHeaderServiceImpl.inventoryCheck L500-615、InventoryCacheService.java L30-130；yl-cwhsea-wms-api InventoryCacheService.groovy L197-240、CompanySettingsService.groovy L471-520）：
1. usableQty 来自 Lua 脚本：field 不存在 return -qty（缺货=-需求量）；hv<qty 返回 hv-qty。availableQty = NumberUtils.toInt(redisService.hGetIgnoreMode(key,field)+"''")——field 缺失时 hGet=null → toInt('null')=0。故报错 [[SKU, ZP, -1, 0]] 中两个数字无法区分 field 缺失与 field=0，两种根因日志签名相同；判定必须拉重建 dump 日志「全量同步完成-数据库计算结果」看 field 实际写入值。
2. isCacheSucceed 每次调用都无条件打「缓存成功-----」日志：trace 无该日志 = isCacheSucceed 未被调用（syncStockout!=1 短路或 qh:succeed 标记缺失），校验整体未执行，不是校验通过。
3. 老系统 yl-cwh-wms-api updateRedisInventory = 查DB→deleteCache→initCache 先删后建成对执行；deleteCache 连 qh:succeed:{仓} 标记一起删 → 删除后未重建的秒级窗口内 succeed 缺失，实时校验跳过、订单照常创建（实证：15:40:32 删 → 15:45:04 新单创建成功且无任何缺货判断日志）。重建执行建议低峰。
4. 排查日志看到「单独删除缓存」时先找页面开关：/wms/settings/company/setItemRedis flag=1 开启实时缺货判断→成对删建重建；flag=0 关闭→只删不建（设计如此，无需找异常流程）。直接搜 requestLog requestUrl=/wms/settings/company/setItemRedis 的 params.flag + remoteAddr 即可定案每次删/建动作归属与操作者。
实证：flag=0 traceId=91089735450871516110462464089945（15:40:32，「关闭实时缺货判断, 触发删除Redis库存缓存」）；跳过校验创建 trace=4923960775530692023（15:45:04）。
