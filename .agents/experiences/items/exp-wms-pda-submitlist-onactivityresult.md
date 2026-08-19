# PDA 收货列表前端踩坑（jt-cloudwarehouse）

用于：jt-cloudwarehouse（PDA Android，BaseQuickAdapter4 体系）列表页开发时规避 WMS-020 review 轮发现的 4 个 UI/生命周期坑。
触发词：PDA列表不刷新、tab切换不刷新、submitList、notifyDataSetChanged、onActivityResult二次弹窗、onSaveInstanceState、Activity回收丢状态、卸货登记弹窗。
不适用：后端接口问题；PC 前端。

## 1. 自定义 submitList 不触发刷新（tab 切换后列表不更新）
- 现象：切换状态 tab 后列表 UI 不更新。
- 根因：ReceiptAdapter 自定义 submitList 只做 items 赋值，不触发 notifyDataSetChanged。
- 修复：删除自定义 submitList，改走父类 BaseQuickAdapter4.submitList。

## 2. 卸货登记完成返回后状态未更新（二次弹窗/不跳转）
- 现象：首次卸货登记完成返回后，弹窗状态与预期不符，未自动进入收货详情。
- 根因：onActivityResult RESULT_OK 后未把 unloadRegistered=true 回写到状态。
- 修复：RESULT_OK 时 copy(unloadRegistered=true) 更新状态，并自动调用 openCheckInTask 进入收货详情页。

## 3. 从收货详情返回列表不刷新
- 修复：onResume + isFirstResume 标志位触发列表刷新（避免首次进入时重复刷新）。

## 4. pendingScanResult 在 Activity 回收后丢失
- 现象：扫码确认弹窗等待期间 PDA Activity 被系统回收，恢复后待确认状态丢失。
- 修复：onSaveInstanceState/onRestoreInstanceState 持久化 pendingScanResult。

## 行为决策（按用户确认，不按 PRD 文字）
- 卸货登记「已登记 + 跳过」：直接进入收货详情页（PRD 写的是「返回操作页」）。

## 证据
- review.md 2026-08-01 审查轮：7 个生产问题中 4 个属 PDA 前端，全部修复并 ship 到需求分支；PDA 按项目规范不补单测，靠模拟器手工回归。
