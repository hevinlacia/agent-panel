# DASH-002 开发笔记

## 已完成

- Hermes req-tracker skill meta.md 模板增加 status/project 字段
- Dashboard requirements.ts 改为从 ~/.agents/req/ 读取数据
- Dashboard JSON 只存 session 关联 (associations.json)
- /projects 页面改为只读展示
- /requirement 详情页读 Hermes 文件展示
- buildInjectionContext 改为 async，读 branch/notes/test 文件

## 注意事项

- Hermes 是需求管理的唯一写入入口
- Dashboard 不再有需求 CRUD 表单
- 旧 requirements.json 自动迁移为 associations.json
