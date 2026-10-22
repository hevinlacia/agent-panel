//! `GET /api/schema` — agent 契约自描述端点。
//!
//! 定位：skill 文档是「策略与组合」层，端点/门禁/状态机的**契约事实源在这里**，
//! 与代码同仓同 review——改 API 的 commit 同 commit 改本文件，机制性消除文档漂移
//! （2026-10-22 前 agent-panel skill 手写副本曾漂移出 36 处失效地址）。
//!
//! 维护约定：
//! - 状态机/门禁直接引用 `REQ_STATUSES` / `ISSUE_STATUSES` / `STATUS_GATE_DEFS`
//!   等既有常量，自动跟随代码演进；
//! - `endpoints` 为 agent 高频契约手工表（axum 无运行时反射），新增/变更面向
//!   agent 的端点时同步维护；面板 UI 内部端点不收录。

use axum::Json;
use serde_json::{json, Value};

use crate::{ISSUE_STATUSES, REQ_FLOW_STATUSES, REQ_STATUSES, STATUS_GATE_DEFS};

/// agent 高频端点契约表（method / path / summary / 关键参数与语义）。
fn endpoint_entries() -> Vec<Value> {
    vec![
        json!({
            "method": "GET", "path": "/api/requirements",
            "summary": "需求列表（含 experienceSummaryJob 内嵌）",
        }),
        json!({
            "method": "POST", "path": "/api/requirements",
            "summary": "创建需求；body: reqId（可自定义，如 FIX/INC 前缀走独立编号池）、title（必填）；可选 project/category/source/issues/consolidated 等",
        }),
        json!({
            "method": "GET", "path": "/api/requirement/context",
            "summary": "agent 上下文摘要；query: id、for=agent、intent、budget；响应 phaseRuntime.currentPhasePrompt 是当前阶段导航，不要沿用 session 创建时的启动 prompt",
        }),
        json!({
            "method": "POST", "path": "/api/requirement/status",
            "summary": "状态推进（agent 门禁强校验，via=ui 人工改状态跳过）；body: reqId/status/note/via",
        }),
        json!({
            "method": "GET", "path": "/api/requirement/file",
            "summary": "读边角 .md（manual-check/自建分析文档等）；query: id+path；返回 content/exists/baseSha；顶层 .md（除 meta.md/state.json）+ docs/<base>/<file>.md 白名单",
        }),
        json!({
            "method": "PUT", "path": "/api/requirement/file",
            "summary": "写边角 .md；body: id/path/content/baseSha（新建可省；不匹配 409 重新 GET）；禁止覆盖 meta.md/state.json",
        }),
        json!({
            "method": "GET", "path": "/api/requirement/attachments",
            "summary": "附件清单（摘要，不带 sha）；query: id/reqId、budget、full=1（全文上限 200KB）",
        }),
        json!({
            "method": "GET", "path": "/api/requirement/attachment",
            "summary": "读单个附件拿 baseSha（更新前必做）；query: id+path（attachments/<file> 或裸文件名均可）",
        }),
        json!({
            "method": "PUT", "path": "/api/requirement/attachment",
            "summary": "写附件（上线资产唯一写通道，禁止直写本地 attachments/）；body: id/path/content/baseSha；白名单 attachments/ 单层 + 后缀 sql/txt/md/yaml/yml/json/csv；release-config.md 追加式更新=GET 全文后追加整文写回",
        }),
        json!({
            "method": "POST", "path": "/api/requirement/events",
            "summary": "结构化事件记录（默认同时追加 notes.md）；body: reqId/eventType/title/summary/details…",
        }),
        json!({
            "method": "PUT", "path": "/api/requirement/sections/:section",
            "summary": "受控文档按 heading 局部更新（background/test/impact 等）；是追加/更新单 section 而非全文替换，全量替换用 /api/requirement/doc 的 writeDoc",
        }),
        json!({
            "method": "GET", "path": "/api/requirement/validate",
            "summary": "需求校验；返回 problems（必须修复）/warnings（推荐补齐）",
        }),
        json!({
            "method": "GET", "path": "/api/requirement/compliance",
            "summary": "跨状态合规快照（状态历史 + 各契约产物齐缺 + 可执行 gaps）；中途接手的新会话/release-check/经验总结先拉它",
        }),
        json!({
            "method": "POST", "path": "/api/agent/knowledge/query",
            "summary": "知识/经验检索；body: intent/domain/kind/limit；默认返回 meta 摘要，按 id 展开全文",
        }),
        json!({
            "method": "POST", "path": "/api/knowledge",
            "summary": "知识/经验写入；body: kind(experience|businessKnowledge)/title/summary/details/triggerTerms(3-8 个,去 ASCII 冒号)/domain/scope；写入前先 query 查重，命中高相似条目时需 confirmDuplicate=true（见 dedupe 契约）",
        }),
        json!({
            "method": "GET", "path": "/api/knowledge/index",
            "summary": "知识库全量索引（meta 摘要）",
        }),
        json!({
            "method": "POST", "path": "/api/requirement/review-materials",
            "summary": "AI 代码审查一键备料；body: reqId、mode(full|incremental 缺省按需求状态)；响应 materialPath 为多行 patch 伴随文件，读 diff 用它而非 JSON 内嵌字段",
        }),
        json!({
            "method": "POST", "path": "/api/requirement/code-review",
            "summary": "刷新 code-review.json 快照（审查后又有 commit 时 stale 拦截的解法）；保留 previousReviewedSnapshot 供增量对比",
        }),
        json!({
            "method": "PUT", "path": "/api/requirement/annotations",
            "summary": "代码问题备注落盘；body.annotations 必须是对象 {files:[…],reviewedCommit,…}（传数组报错）；reviewedCommit 须与最新材料一致，否则门禁 stale 拦截",
        }),
        json!({
            "method": "GET", "path": "/api/requirement/session-context",
            "summary": "读需求 session 注入上下文原文（text/markdown）；query: sessionId（uuid）；复制命令内联 curl 的服务端——分离架构下 ctx 文件在 hub 容器内，本机 pi 无法 @容器路径",
        }),
        json!({
            "method": "GET", "path": "/api/requirement/branch-registration",
            "summary": "查询分支登记；query: reqId/round",
        }),
        json!({
            "method": "PUT", "path": "/api/requirement/branch-registration",
            "summary": "分支登记全量替换；body: reqId/round/repos:[{repoName,branch,role?,path?,baseRef?}]（repos 全量替换语义不可省略）；panel 实测分支存在性，分离部署时 path 不在本机则跳过实测并告警；换分支/删仓库需 confirmBranchChange/confirmRemoval",
        }),
        json!({
            "method": "GET", "path": "/api/experience-summary/jobs",
            "summary": "经验总结任务列表（items[].req 内嵌 experienceSummaryJob）",
        }),
        json!({
            "method": "POST", "path": "/api/experience-summary/jobs/dispatch",
            "summary": "手动派发总结；body: reqId 可选（缺省全量扫描 pending）",
        }),
        json!({
            "method": "POST", "path": "/api/experience-summary/jobs/retry",
            "summary": "重置指定需求总结任务为 pending 并立即派发；body: reqId",
        }),
        json!({
            "method": "POST", "path": "/api/experience-summary/jobs/complete",
            "summary": "总结完成回写（由总结 agent 调用）；body: reqId/sessionId/note；完成后 issue 家族自动「已关闭」、普通需求自动「已完成」",
        }),
        json!({
            "method": "GET", "path": "/api/config",
            "summary": "读运行配置（autoExperienceSummary/experienceSummaryDispatchWindow/experienceSummaryMaxAgents/deployFreeze 等）",
        }),
        json!({
            "method": "POST", "path": "/api/config",
            "summary": "更新运行配置（patch 语义，只传要改的字段）；UAT 部署前检查 deployFreeze.enabled",
        }),
        json!({
            "method": "POST", "path": "/api/requirement/notes",
            "summary": "追加进展到 notes.md（不要全文覆盖 notes.md）；body: reqId/title/text",
        }),
        json!({
            "method": "GET", "path": "/api/workers",
            "summary": "worker 在线状态（执行类端点依赖在线 worker，离线时 hub 返回 503 stub）",
        }),
    ]
}

/// file / attachment 写通道契约（白名单 + 乐观锁），agent 高频踩坑点集中声明。
fn write_channels() -> Value {
    json!({
        "optimisticLock": {
            "sha": "GET 返回 baseSha（兼容别名 sha），PUT 原样带回",
            "create": "文件不存在时可省 baseSha；GET 曾返回的空内容 sha（da39a3ee…）传回自动降级为新建",
            "conflict": "已存在且未带 baseSha、或 baseSha 不匹配 → 409；重新 GET 再写",
        },
        "requirementFile": {
            "whitelist": ["顶层单段 .md（meta.md/state.json 排除）", "docs/<base>/<file>.md 两层分册"],
            "reject": "绝对路径、.. 穿越、非 .md、超过两层",
        },
        "attachment": {
            "whitelist": ["attachments/<file> 单层（裸文件名兼容）"],
            "exts": ["sql", "txt", "md", "yaml", "yml", "json", "csv"],
            "sqlHeader": "文件头注释固定四项：用途/目标环境/上线是否需执行/回滚方式",
            "releaseConfig": "配置与人工动作统一追加 attachments/release-config.md（类型/对象/变更内容/环境/是否已发布/是否需上线动作/回滚方式）",
            "forbidden": "禁止直写本地 .agents/req/（本机无副本；写 hub API 是唯一通道）",
        },
    })
}

/// 状态机与自动行为契约。
fn state_machines() -> Value {
    json!({
        "requirementStatuses": REQ_STATUSES,
        "requirementFlow": REQ_FLOW_STATUSES,
        "issueStatuses": ISSUE_STATUSES,
        "autoBehaviors": {
            "experienceSummaryComplete": "总结 agent 调 complete 后：issue 家族（线上问题/测试问题/性能问题）自动「已关闭」（issue 状态机无「已完成」，完成即关闭无 grace）；普通需求自动「已完成」；expire 扫描（10min）兜底漏推场景",
            "experienceSummaryDispatch": "需求/问题进入「经验总结」即触发派发（受 autoExperienceSummary 开关与 DispatchWindow 窗口限制）；循环跑在 hub，spawn HTTP 下发在线 worker 本机执行 pi",
        },
    })
}

/// 状态流转门禁清单（动态引用 STATUS_GATE_DEFS，自动跟随代码）+ 门禁产物格式契约。
fn gates() -> Value {
    let defs: Vec<Value> = STATUS_GATE_DEFS
        .iter()
        .map(|d| {
            json!({
                "id": d.id,
                "label": d.label,
                "description": d.description,
            })
        })
        .collect();
    json!({
        "definitions": defs,
        "formats": {
            "selftestChecklist": "test.md「## 自测清单」二级标题块内：### 主流程测试 / ### 边界场景测试 / ### 高并发/大流量场景测试 三类小节；每类固定列表格 | # | 自测项 | 结果 | 失败/无法测试原因 |，结果列只允许 通过/失败/无法测试；边界与并发小节先 #### 风险场景分析（每行一条）再 #### 测试清单",
            "reviewGate": "review.md 或 code-review-ai.md 必须有独立成行的结论「Review Gate: PASS|BLOCKED|WAIVED」（不能加粗，加粗解析不出会被判 pending 拦截）；P0 小节非空不放行；「自测清单交叉评估」小节非空（test.md 有自测清单时）",
            "uatRegression": "test.md「## UAT 回归」表格 | # | 场景 | 结果 | 证据 |；证据须为已发生可核对事实（tid/日志关键字/接口返回/DB 前后值）；整节不适用写「不适用：<原因>」",
            "troubleshooting": "线上问题进入经验总结前必须沉淀 troubleshooting.md（怎么排查 + 怎么修复，至少一条非「待补充」记录）；无沉淀价值应改走「已关闭」",
        },
    })
}

/// knowledge 写入查重契约（与 knowledge 写入实现联动）。
fn knowledge_dedupe() -> Value {
    json!({
        "algorithm": "title 字符 bigram Jaccard（中文友好），仅比对同 kind 条目；仅新建（无 id）时触发，编辑既有条目不查",
        "highSimilarity": "最高相似度 >= 0.5 且未带 confirmDuplicate=true → 409，message 列出候选（id/score/title）；确属新知识带 confirmDuplicate=true 重发",
        "midSimilarity": "0.35 ~ 0.5 → 正常写入，响应 dedupeWarning 列出候选供人工复核",
        "workflow": "写入前先 POST /api/agent/knowledge/query 查重（prompt 级约定），API 级查重是最后防线",
    })
}

/// GET /api/schema：agent 契约自描述（端点/写通道/状态机/门禁/查重）。
pub(crate) async fn api_schema() -> Json<Value> {
    Json(json!({
        "version": env!("CARGO_PKG_VERSION"),
        "mode": "契约事实源：本端点与代码同仓同 review；skill 文档只写策略与组合，契约以本端点为准",
        "endpoints": endpoint_entries(),
        "writeChannels": write_channels(),
        "stateMachines": state_machines(),
        "gates": gates(),
        "knowledgeDedupe": knowledge_dedupe(),
    }))
}
