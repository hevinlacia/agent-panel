---
name: req-diff-annotate
description: 为需求代码差异生成结构化讲解（关键变量含义、设计思路、数据/状态流转图），通过 Agent Panel API 写入 code-annotations.json，在差异页右侧说明栏展示。触发词：diff 备注、代码差异说明、给 diff 加讲解、annotate diff、代码讲解面板。
allowed-tools: ["bash", "read", "get_session_info"]
---

# Req Diff Annotate

用于：为需求的代码差异（code-review 快照）生成 AI 讲解并保存，降低用户在 Agent Panel 差异页审查代码的负担。

适用：
- 用户说"给这个需求的 diff 加说明/备注/讲解"
- 用户说"生成代码差异讲解"、"解释一下这些改动"、"diff-annotate"
- 需求收尾阶段（code-review 通过后）为差异补充设计说明

不适用：
- 生成 code-review 快照本身（用差异页"生成代码差异"按钮或 `/api/requirement/master-diff`）
- 写正式审查结论（review.md / code-review-ai.md 由 review 流程产出）
- 需求文件结构问题（用 `req-create` / `agent-panel-requirement-api`）

## Trigger

- "给 diff 加备注" / "diff 加讲解" / "代码差异说明"
- "解释这些改动" / "变量是什么意思" / "画一下流转图"
- 差异页提示"该文件暂无说明"后的补注请求

## 术语和边界

- 本机 Agent Panel 默认地址 `http://localhost:7331`（`PORT` 可覆盖）。
- 讲解数据存需求目录 `code-annotations.json`；**必须通过 API 保存**，不要手写文件。
- 展示位置：Agent Panel 差异页（`/requirement-diff`）右侧说明栏；hunk 备注锚定到代码块。

## 流程

### 1. 定位需求

- reqId 已知则直接用；否则问用户，或用 `get_session_info` 确认当前 session 后经 `GET /api/requirement/by-session?sessionId=<id>` 反查。
- 确认需求存在：`GET /api/requirement?id=<reqId>`。

### 2. 读取代码差异快照栈

```bash
curl -s "http://localhost:7331/api/requirement/diff-snapshots?reqId=<reqId>"
```

- 返回 `snapshots[]`（新在前，最多 5 版）；取 `snapshots[0]` 作为讲解基准（与差异页默认展示一致）。
- 每个快照含 `repoName`、`targetCommit`、`diff`（unified diff 全文）、`files[].path`、`savedAt`。
- 若 `snapshots` 为空：提示用户先在差异页点「生成代码差异」，停止本 skill。
- 大需求可分批：先为改动最核心的 3-5 个文件生成，不要一次性读完整个 diff 再动手。

### 3. 生成讲解（遵守 schema）

对每个值得讲解的文件生成一个条目（schema 模板见下方）。生成要求：

- `repo` / `path` 必须与快照中 `repoName` / `files[].path` **逐字一致**，否则面板匹配不上。
- 快照是差异页展示的同一份数据：基于 `snapshots[0]` 生成的备注与面板锚定完全一致；用户刷新差异后快照变化，面板会提示备注过期，届时重跑本 skill 即可。
- `summary`：1-3 句话，讲清"改了什么 + 为什么"，中文。
- `variables`：只列 diff 中新出现或语义变化的关键变量/字段/配置项（5-10 个以内），不逐个罗列局部变量。`meaning` 讲业务含义，`why` 讲为什么需要它（可选）。
- `flow`：mermaid 源码（`graph LR` / `sequenceDiagram` / `stateDiagram-v2`），节点文字用中文，描述数据或状态如何流转（来源 → 处理 → 落点）。节点/边标签避免 `()` `[]` `{}` 等易破坏 mermaid 语法的裸字符，需要时用引号包裹。
- `notes[]`：针对具体代码块的解释。`anchor.hunkHeader` 必须**从 diff 文本逐字复制** `@@ -a,b +c,d @@`（到第二个 `@@` 含尾部空格为止，不含后面的函数名）；面板按前缀匹配定位。`note` 用 markdown。
- 不确定的设计意图宁可写"推测"，不要编造。

### 4. 保存

```bash
curl -s -X PUT "http://localhost:7331/api/requirement/annotations" \
  -H "Content-Type: application/json" \
  -d '{"reqId":"<reqId>","annotations":<完整文档>}'
```

- 每次保存传**完整文档**（先 GET 已有内容合并，再 PUT 覆盖），后端自动补 `updatedAt`。
- 建议先 `GET /api/requirement/annotations?reqId=<reqId>` 取已有文档，合并 `files[]`（同 repo+path 覆盖）后提交，避免覆盖他人条目。
- 在 `reviewedCommit` 中记录每个 repo 的 `targetCommit`，供面板标记备注是否过期；`generatedBy` 填当前 session id（`get_session_info`）。

### 5. 验证

- 回读：`GET /api/requirement/annotations?reqId=<reqId>`，确认文件条目数与 `files[].path` 正确。
- 提醒用户到差异页刷新查看：hunk 备注是否锚定到代码块（未锚定的会列在"段落备注"区并提示未定位）。

## Schema 模板

```json
{
  "version": 1,
  "reqId": "<reqId>",
  "generatedBy": "<session-uuid>",
  "baseRef": "origin/master",
  "reviewedCommit": { "<repoName>": "<targetCommit>" },
  "files": [
    {
      "repo": "<repoName>",
      "path": "<file path, 与 diff 逐字一致>",
      "summary": "1-3 句话讲清改了什么、为什么",
      "variables": [
        { "name": "sourceFallback", "kind": "flag", "meaning": "业务含义", "why": "为什么需要" }
      ],
      "flowTitle": "数据流转：下发 → 消费 → 落库",
      "flow": "graph LR\n  A[来源] --> B[处理] --> C[落点]",
      "notes": [
        { "anchor": { "type": "hunk", "hunkHeader": "@@ -10,7 +11,9 @@ ", "context": ["diff 中该 hunk 后的前 1-2 行上下文（可选）"] },
          "note": "markdown：这段为什么这么写、关键取舍" }
      ]
    }
  ]
}
```

## Required Checks

- [ ] `repo`/`path` 与 code-review 快照逐字一致
- [ ] 每个 `notes[].anchor.hunkHeader` 都能在 diff 文本中找到前缀匹配的 `@@` 行
- [ ] mermaid 源码无裸括号/特殊字符破坏语法；非法时宁可不写 `flow`
- [ ] 保存走 PUT API 且传完整文档；未覆盖删除他人条目
- [ ] `reviewedCommit` 已填各 repo 当前 `targetCommit`

## Final Response

汇报：讲解的文件数、每个文件的摘要一句话、备注锚定数、未锚定数，以及"到 Agent Panel 差异页刷新查看"的提示。未讲解的文件说明原因（改动机械/截断/用户指定范围外）。
