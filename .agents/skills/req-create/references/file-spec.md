# 需求文件规范

## 目录结构

```
~/.agents/req/
├── <project>/                    # 项目目录（如 WMS）
│   ├── <req-id>/                 # 叶子需求
│   │   ├── meta.md               # 必填
│   │   ├── background.md         # 强烈建议
│   │   ├── branch.md             # 必填
│   │   ├── notes.md              # 按需
│   │   ├── test.md               # 按需
│   │   ├── config-changes.md     # 按需
│   │   └── state.json            # dashboard 自动生成，不要手动创建
│   └── <parent-req-id>/          # 父需求（分组容器）
│       ├── meta.md               # 必填
│       ├── background.md         # 按需
│       └── <child-req-id>/       # 子需求（和叶子需求结构相同）
│           ├── meta.md
│           └── ...
├── <req-id>/                     # 顶层独立需求（无项目分组）
│   └── meta.md
└── README.md
```

## 文件说明

| 文件 | 必填 | 注入上下文 | 用途 |
| --- | --- | --- | --- |
| meta.md | 是 | 否（只解析 frontmatter） | 需求元信息：ID、标题、状态、项目、负责人 |
| background.md | 强烈建议 | 是（500字） | 需求背景：目标、背景、范围、关键决策 |
| branch.md | 是 | 是（300字） | 分支信息：源分支、目标分支、关键 commit |
| notes.md | 按需 | 是（300字） | 开发笔记：当前状态、待跟进事项 |
| test.md | 按需 | 否（仅列路径） | 测试场景清单、日志关键字（正常/异常）、自测记录（test 环境）、UAT 回归记录、回归范围、注意事项 |
| config-changes.md | 按需 | 否（仅列路径） | 配置变更：DB、Apollo、Nacos |
| state.json | 自动 | 否 | 状态持久化：dashboard 写入，不要手动创建或编辑 |

## meta.md frontmatter 字段

| 字段 | 类型 | 说明 |
| --- | --- | --- |
| req-id | string | 需求 ID，与目录名一致 |
| title | string | 需求标题，30字以内 |
| status | enum | 待设计 / 待开发 / 开发中 / 自测中 / 测试中 / 待上线 / 已完成 |
| project | string | 项目目录名（如 WMS） |
| owner | string | 负责人 |
| start-date | string | YYYY-MM-DD 或 unknown |
| plan-release | string | YYYY-MM-DD 或 unknown |

## 父需求 vs 叶子需求

- **父需求**：目录下有子需求子目录，本身相当于分组容器
  - meta.md status 通常填"进行中"（不用于状态跟踪）
  - 不需要 branch.md / notes.md / test.md / config-changes.md
  - background.md 可选（描述整体需求背景）
  - dashboard 详情页隐藏状态切换器、session 面板，只显示子需求卡片列表

- **叶子需求**：实际要开发的需求单元
  - 所有文件都可使用
  - dashboard 详情页显示完整的状态、session、上下文注入功能
  - 如果是子需求，页面顶部显示"← 返回父需求"链接

## 上下文注入格式

当用户通过 dashboard "另开新 session" 时，buildInjectionContext 会读取：
1. background.md（最多 500 字）— 需求背景
2. notes.md（最多 300 字）— 当前进展
3. branch.md（最多 300 字）— 分支与改动
4. 所有 5 个文件路径 — 让 agent 知道往哪写

注入结尾是"不要自行开始执行任何任务，等待用户下达具体任务安排。"
