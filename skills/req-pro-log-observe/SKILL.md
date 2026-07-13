---
name: req-pro-log-observe
description: 需求发布到生产后，从需求文件提取日志关键字，查询 CN/SEA PRO Kibana 日志，验证代码逻辑是否正常并排查异常。
allowed-tools: ["bash", "read", "glob", "grep"]
---

# Requirement PRO Log Observe

用于：需求发布到 CN/SEA PRO 后，自动从需求文件提取日志关键字并查询 Kibana，验证代码是否按预期执行、排查发布引入的异常。

适用：需求发布后观察日志、批量验证多个待上线需求的线上表现、按关键字确认开关/消费者/生产者日志是否出现。

不适用：发布前预检（用 `req-release-check`）、打捞日志 payload 做补偿（用 `wms-kibana-bsearch-payload-query`）、排查已知 bug 根因（用 `log-code-debug`）。

## Trigger

- "发布完了，看看日志有没有问题"
- "观察一下 PRO 环境日志"
- "这几个需求上线了，帮我查查日志"
- "看看有没有报错"

## Workflow

### 1. 确认需求和时间窗口

- 确认要观察的需求：用户指定 req-id，或从 dashboard 获取"待上线"需求。
- 确认发布时间（日志窗口起点）：用户告知或用 `date` 获取当前时间往前推。
- 默认窗口终点为当前时间。

### 2. 运行观测脚本

```bash
uv run python ~/Developer/tools/agent-panel/skills/req-pro-log-observe/scripts/observe_req_logs.py \
  --req-ids WMS-003,WMS-009 \
  --start-time "2026-06-30 21:09:00"
```

常用参数：
- `--req-ids`：逗号分隔的需求 ID（支持前缀匹配，如 `WMS-003` 匹配 `WMS-003-set-bi-picking-status`）
- `--status 待上线`：按状态自动筛选需求
- `--start-time`：北京时间，格式 `YYYY-MM-DD HH:MM:SS`
- `--end-time`：北京时间，默认当前时间
- `--envs`：`cn`、`sea` 或 `all`（默认 all）
- `--keywords`：手动追加关键字，逗号分隔
- `--no-error-check`：跳过 Exception 查询（默认查询 Exception）

脚本自动：
- 从 `test.md` / `notes.md` 提取日志关键字（`## 日志观测关键字` 小节或 `Kibana 搜索关键字` 模式）
- 查询 CN PRO (`pro-cwh*-applog*`) 和 SEA PRO (`pro-cwhsea*applog*`)
- 处理 CN bsearch 异步轮询
- 输出 JSON 报告到 `/tmp/opencode/req-log-observe/`

### 3. 分析结果

脚本输出包含：
- **关键字命中情况**：每个关键字在 CN/SEA 的命中数和样本日志
- **异常汇总**：ERROR/Exception 按应用和异常类型分类
- **结论建议**：哪些需求日志正常、哪些需要关注

AI 需要结合需求内容判断：
- 预期日志是否出现（开关关闭日志、消费者接收日志等）
- 异常是否与本次发布相关（对比历史已知问题）
- 是否需要进一步排查

## 日志关键字约定

建议在需求 `test.md` 中添加 `## 日志观测关键字` 小节：

```markdown
## 日志观测关键字
- `设置BI出库单状态为拣货中PC端`
- `开关已关闭，跳过发送RabbitMQ消息`
- `接收到更新BI出库单状态拣货中消息`
```

脚本也兼容旧格式 `Kibana 搜索关键字：`xxx`` 和 `notes.md` 中的 `flag:"..."` 模式。

## Required Checks

- 执行前用 `date` 确认当前绝对时间，确保时间窗口覆盖发布后时段。
- 确认 Kibana 环境变量（`OPENCODE_KIBANA_*_SID` 或 `OPENCODE_KIBANA_*_USERNAME/PASSWORD`）已配置。
- CN bsearch 首包通常 `isRunning=true`，脚本自动轮询，无需手动干预。
- 若关键字 0 命中，检查索引名是否正确（CN: `pro-cwh*-applog*`，SEA: `pro-cwhsea*applog*`）。
- 若 SID 过期（401/403），脚本自动用账号密码重新登录。

## Final Response

最终回复要包含：
- 观测的需求列表和北京时间窗口
- 每个需求的日志关键字命中情况（CN/SEA 分别列出）
- 异常汇总（按应用分类，标注是否与本次发布相关）
- 结论：正常 / 需关注 / 需排查
- 明确说明未保存或打印 SID
