# DASH-001 开发笔记

## 已完成

- Operator 风格 session 仪表盘
- 内嵌 xterm 终端 (node-pty + ws)
- 子 agent session 折叠分组
- 时间范围筛选 (默认7天)
- SQLite → CLI → fs 三级数据源

## 注意事项

- xterm host 必须零 padding，否则滚动后层错位
- CJK 字体优先 Noto Sans Mono CJK SC
- lineHeight 必须为整数避免亚像素漂移
