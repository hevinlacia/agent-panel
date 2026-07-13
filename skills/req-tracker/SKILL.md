---
name: req-tracker
description: 记录需求分支、注意事项、测试用例与配置变更;在测试询问时汇总需求信息;在发布前预检分支合并、DB、Apollo/Nacos 变更,降低发布风险。
allowed-tools: ["bash", "read", "write", "edit", "glob", "grep"]
---

# Req Tracker

用于：在需求开发到上线的全流程中,集中保存需求号、分支、注意事项、测试方法、DB/Apollo/Nacos 变更,方便测试同事随时询问,也方便发布前自检。

适用：

- 接到新需求,需要登记分支、负责人、计划发布日期
- 测试同事在群里问"XX 需求的分支是哪个？测试方法在哪看？"
- 上线前需要自检:分支是否合并、有没有遗漏 SQL/Apollo/Nacos 配置
- 需求上线后做复盘,沉淀踩坑和注意事项

不适用：

- 仓库内源码改动、重构、bug 修复的实际编码工作(走普通开发流程)
- 跨多个需求批量管理(每个需求一个目录,一次只追踪一个)
- 不写代码只做纯知识查询(没有 req-id 上下文时,直接拒绝)

## Storage Layout

需求根目录:`~/.agents/req/`

需求按项目分组,每个项目一个顶层目录,每个需求在项目目录下的子目录:

```text
~/.agents/req/
├── README.md
├── WMS/
│   ├── REQ-2026-001/
│   │   ├── meta.md
│   │   ├── memory.md
│   │   ├── branch.md
│   │   ├── config-changes.md
│   │   ├── impact.md
│   │   ├── test.md
│   │   ├── notes.md
│   │   └── review.md
│   └── REQ-2026-002/
│       └── meta.md
├── opencode-dashboard/
│   └── DASH-001/
│       └── meta.md
└── _default/
    └── (未归类的需求)
```

项目目录名用 ASCII 字母数字和连字符(如 `WMS`、`opencode-dashboard`、`_default`)。需求目录名同理(用 PRD/工单号/日期简写,例如 `REQ-2026-001` 或 `0628-wms-inbound-allowpart`)。项目分组以目录名为准,`meta.md` frontmatter 的 `project` 字段为可选的显示名覆盖。

子目录文件约定(全部用 Markdown,UTF-8):

| 文件 | 必填 | 用途 |
| --- | --- | --- |
| `meta.md` | 是 | 顶部 YAML frontmatter(`req-id` / `title` / `status` / `project` / `owner` / `start-date` / `plan-release`)供 opencode-dashboard 解析,正文记录改动范围摘要、相关人/群;`status` 取值:`待设计` / `待开发` / `开发中` / `自测中` / `测试中` / `待上线` / `已完成` |
| `memory.md` | 是 | dashboard 新 session 的首要记忆入口:当前目标、当前进展、关键决策、已完成改动、待办/风险、影响范围、session 摘要索引 |
| `branch.md` | 是 | 源分支、目标分支、PR/CR 链接、关键 commit 区间 |
| `config-changes.md` | 是 | DB 变更(DDL/DML/数据订正)、Apollo / Nacos namespace+key、RocketMQ Topic/Group、阿里云控制台配置、灰度和回滚 |
| `impact.md` | 是 | 编码前影响面评估:风险等级、核心链路、影响入口、数据影响、阻塞风险、自测清单、回滚方案 |
| `test.md` | 是 | 测试用例、测试方法、测试环境、前置数据、自测记录、A/B/C/D 证据结论、回归范围 |
| `notes.md` | 是 | 注意事项、踩坑、改动动机、上线后复盘、session 过程摘要追加 |
| `review.md` | 按需 | 待上线 Code Review 范围、发现项、用户确认和复查结论 |
| `release-check.md` | 发布前生成 | 预检 checklist、是否合并、是否需要执行 SQL、是否需要推送配置 |

模板在 `references/` 下,首次创建需求时按模板生成空文件,再让用户填实际内容。

## Trigger Phases

加载本 skill 后,根据用户意图进入对应阶段,主动询问缺什么信息,不要自己猜。

### Phase 1: 登记(`登记` / `新建需求` / `req-id 记录一下`)

- 询问:需求号、需求标题、源分支、目标分支、负责人、计划发布日期、关联项目路径、项目分组(`project` 字段,例如 `WMS后端`)
- 询问需求归属哪个项目目录(例如 `WMS`、`opencode-dashboard`);若用户给的项目目录不存在,确认后新建;无明确归属时落到 `_default/`
- 在 `~/.agents/req/<project>/<req-id>/` 下创建目录:`mkdir -p ~/.agents/req/<project>/<req-id>/`,按模板生成 `meta.md`、`memory.md`、`branch.md`、`config-changes.md`、`impact.md`、`test.md`、`notes.md`,按需生成 `review.md`
- `meta.md` 顶部使用 YAML frontmatter,`status` 默认填 `开发中`(刚登记尚未进入开发可填 `待设计`),`project` 字段可选,留空时按父目录名作为项目分组
- meta.md 顶部的 YAML frontmatter 供 opencode-dashboard 解析需求状态和项目分组,正文部分供人阅读
- `memory.md` / `branch.md` / `config-changes.md` / `impact.md` / `test.md` / `notes.md` / `review.md` 会被 dashboard 智能提取读取并维护；文件可以先写占位模板,后续由 agent 按会话事实更新
- `branch.md` 里的关键 commit 区间可以后续在代码 push 后用 `git log` 回填
- 不在文件里写真实 token、密码、Cookie、私钥、SQL 文件名以外的敏感信息;测试账号放 `test.md` 时只写账号规则,不写明文密码

### Phase 2: 询问(`XX 需求的分支是？` / `测试方法在？` / `上线要注意啥？`)

- 如果上下文里没有 req-id,先问"哪个需求?",并提示可以用以下任一方式定位:
  - 直接给需求号
  - 当前 git 仓库里执行 `git rev-parse --abbrev-ref HEAD`,看分支名是否含需求关键字
  - 列 `~/.agents/req/*/*` 下两层目录(项目/需求),让用户挑
- 读 `meta.md` + `memory.md` + `branch.md` + `test.md` + `notes.md` + `config-changes.md` + `review.md`,按用户问题输出需要的部分；测试相关问题优先使用 `test.md` 中的证据标准和置信度结论
- 输出里只回答用户问的字段,不要把所有文件全文粘贴;用户没问的字段只在被问到时再展示

### Phase 3: 预检(`上线前检查` / `release preflight` / `能发了吗`)

按以下顺序逐项执行,每项必须给出结论(`OK` / `需关注` / `阻塞`)和证据路径:

1. **分支合并状态** — 在关联项目里跑 `git status` + `git log target..HEAD --oneline`,确认源分支所有 commit 都在目标分支,或 PR 状态为 merged;冲突/未合并要标红
2. **DB 变更** — 从 `config-changes.md` 读 DDL/DML 清单,核对仓库里 `*.sql` / `db/migration/` / `flyway/` / `liquibase/` 是否齐全;问用户是否需要在目标环境手动执行(还是通过 Flyway 自动执行)
3. **Apollo / Nacos 变更** — 从 `config-changes.md` 读 namespace+key 列表,提示用户到 Apollo/Nacos 后台确认:
   - 已发布到目标环境(uat / pre / prod)
   - 灰度策略是否正确
   - 是否需要回滚预案
4. **测试结论** — 读 `test.md`,按 `conventions-wms-agent-self-test-evidence.md` 检查核心 case 是否有触发证据、`tid` 日志链路、DB 结果、副作用、反向证据和 A/B/C/D 置信度；只有接口成功或单点日志/DB 时标为“需关注/证据不足”
5. **影响面和 Review** — 读 `impact.md` 和 `review.md`,确认高风险链路是否已覆盖自测/回归,待上线 review 发现项是否已关闭或用户确认
6. **回滚方案** — `notes.md` / `impact.md` / `config-changes.md` 里有没有写明回滚步骤(SQL 回滚、配置回滚、代码 revert)

把结果写回 `release-check.md`,并打印一份简洁的发布前 checklist 给用户。

## Required Checks

执行任何阶段前,先确认:

- 需求根目录 `~/.agents/req/` 存在,不存在就 `mkdir -p`;项目子目录(如 `~/.agents/req/<project>/`)不存在时与用户确认后再创建
- 项目/需求目录拆分在校验之前完成;用户给出的 req-id 与 project 都不含路径分隔符、`..`、空格,只用 ASCII 字母数字和连字符
- 关联项目路径存在且是 git 仓库(预检阶段才需要)
- 不要把仓库源代码、`.env`、SQL 备份、Apollo/Nacos 真实配置值写进 req 目录
- 跨 skill 工具时,主动加载相关 skill(例如查 Apollo 配置用 `apollo-config-query`,查 MySQL 数据用 `mysql-direct-query-write`,处理 PR/分支合并用 `git-github-workflow`)

## Final Response

每次响应结尾给一段结构化摘要,使用下面三段式之一:

登记完成:

```text
✅ 已创建: ~/.agents/req/<project>/<req-id>/
- 需求: <title>
- 源分支 → 目标分支: <src> → <dst>
- 待补: <哪些文件还需要用户填>
```

询问答复:

```text
📋 <req-id> <title>
- 分支: <src> → <dst> (PR: <url>)
- 测试: <一句话方法,详细见 test.md>
- 注意: <一句话提醒,详细见 notes.md>
```

预检结论:

```text
🚀 <req-id> 上线前预检
- 分支合并: OK / 需关注: <原因>
- DB 变更: OK / 需关注: <未执行项>
- Apollo/Nacos: OK / 需关注: <未发布项>
- 测试: OK / 需关注: <未覆盖项>
- 回滚: OK / 需关注: <缺失项>
- 阻塞项: <list,空表示可发布>
```

## Reference

- `references/meta-template.md`
- `references/branch-template.md`
- `references/notes-template.md`
- `references/test-template.md`
- `references/config-changes-template.md`
- `references/release-check-template.md`
