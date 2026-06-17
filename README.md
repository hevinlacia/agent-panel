# opencode-dashboard

OpenCode 的常驻 Web 控制面。初期功能：浏览经验总结报告、勾选候选、确认/驳回。

## 技术栈

- **Hono** — 轻量 Web 框架
- **@hono/node-server** — Node.js 适配器
- **hono/jsx** — 服务端 JSX 渲染（无需 React 构建链）
- **tsx** — 直接运行 TypeScript

## 快速开始

```bash
cd ~/GitHub/opencode-dashboard
npm install
npm start
# → OpenCode Dashboard running at http://localhost:7331
```

开发模式（文件变更自动重启）：

```bash
npm run dev
```

## 功能

### 当前

- 扫描 `/tmp/opencode/handoff/` 下的经验总结报告（`report.md`）
- 报告列表页：显示 session、日期、候选统计
- 报告详情页：候选卡片，支持勾选
- 确认/驳回：POST `/api/confirm`，结果写入 `/tmp/opencode/handoff/confirmations/`
- 暗色主题，移动端适配

### 规划

- 历史报告浏览和搜索
- 确认状态追踪（已确认/已驳回/待处理）
- 批量操作
- 统计仪表盘（候选趋势、高频 skill、知识库覆盖率）
- 与 OpenCode 命令集成（从 dashboard 触发 `/experience-summary`）

## 架构

```
src/
  parser.ts    — 解析 report.md 为结构化数据
  scanner.ts   — 扫描报告目录 + 保存确认结果
  server.tsx   — Hono 路由 + JSX 页面模板
public/
  style.css    — 暗色主题样式
  app.js       — 客户端交互（勾选、确认、toast）
```

## 扩展指南

加新页面：在 `server.tsx` 加路由 + JSX 组件，无需修改构建配置。
加 API：在 `server.tsx` 加 Hono 路由，返回 JSON。
加前端交互：在 `public/app.js` 加事件监听，CSS 在 `public/style.css`。

后续如需升级到 Vite + React SPA，`src/` 的 parser 和 scanner 可直接复用，只需替换 views 层。
