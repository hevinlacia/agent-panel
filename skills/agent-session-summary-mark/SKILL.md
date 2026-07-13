---
name: agent-session-summary-mark
description: 标记当前或指定 agent session 为待总结状态，让 Agent Panel 后台在空闲 1 小时后自动 fork 并生成经验报告。触发词：标记会话、标记总结、mark session、这个session有价值、值得总结。
allowed-tools: ["bash", "read"]
---

# Session Summary Mark

用于：标记一个 agent session 为"待经验总结"，让 Agent Panel 后台在 session 空闲 1 小时后自动 fork 并生成经验报告，用户在 Agent Panel /reports 页面审阅候选并确认后自动触发执行。

适用：用户感觉当前 session 有总结价值，但不想立即总结；希望 session 结束后自动处理。

不适用：立即总结（用 `/experience-summary`）；批量总结（用 `/experience-summary-batch`）；不需要总结的普通会话。

## Trigger

用户出现以下意图时使用：

- `标记这个 session`
- `标记会话`
- `mark session`
- `这个 session 有价值` / `值得总结`
- `标记总结`
- `mark for summary`

## Workflow

1. 获取当前 session ID：
   - 优先使用环境变量 `OPENCODE_SESSION_ID`（如果非空且匹配 `ses_*`）。
   - 如果环境变量为空，运行 `opencode export --sanitize 2>/dev/null | head -1` 尝试从导出 JSON 的 `id` 字段获取。
   - 如果仍无法获取，告知用户需要显式提供 session ID。

2. 验证 session ID 格式：必须匹配 `^ses_[A-Za-z0-9]+$`。

3. 调用 Agent Panel API 标记 session：
   ```bash
   curl -s -X POST http://localhost:7331/api/experience/mark \
     -H "Content-Type: application/json" \
     -d "{\"sessionId\": \"<sessionID>\"}"
   ```

4. 如果用户提供了备注（如"标记总结 这个 session 解决了 MQ 消费幂等问题"），把备注也传入：
   ```bash
   curl -s -X POST http://localhost:7331/api/experience/mark \
     -H "Content-Type: application/json" \
     -d "{\"sessionId\": \"<sessionID>\", \"note\": \"<备注>\"}"
   ```

5. 检查 API 响应：
   - `{"ok": true, "marker": {...}}` — 标记成功
   - `{"error": "..."}` — 标记失败，告知用户错误原因

6. 如果 Agent Panel 不可用（连接失败），告知用户：
   - 确认 `opencode-dashboard.service` 是否运行：`systemctl --user status opencode-dashboard.service`
   - 如果未运行，提示启动：`systemctl --user start opencode-dashboard.service`

## Required Checks

- Session ID 必须通过 `^ses_[A-Za-z0-9]+$` 正则校验
- Dashboard 必须在 `localhost:7331` 可达
- 不读取或打印任何 secret / .env 文件
- curl 命令不包含敏感信息

## Final Response

```text
已标记 session <sessionID> 为待总结。

Dashboard 后台将在该 session 空闲 1 小时后自动 fork 并生成经验报告。
报告生成后可在 http://localhost:7331/reports 查看。

查看标记状态：GET http://localhost:7331/api/experience/markers
取消标记：POST http://localhost:7331/api/experience/unmark {"sessionId": "<sessionID>"}
```
