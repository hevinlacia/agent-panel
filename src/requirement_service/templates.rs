use super::*;

#[allow(clippy::too_many_arguments)]
pub(crate) fn requirement_create_files(
    req_id: &str,
    title: &str,
    status: &str,
    project: &str,
    projects: &[String],
    category: &str,
    source: &str,
    owner: &str,
    start_date: &str,
    plan_release: &str,
    ones: &str,
    issues: &[String],
    summary: &str,
    background: Option<&str>,
    notes: Option<&str>,
) -> Vec<(&'static str, String)> {
    let meta = build_meta_doc(
        req_id,
        title,
        status,
        project,
        projects,
        category,
        source,
        owner,
        start_date,
        plan_release,
        ones,
        issues,
        summary,
    );
    let mut files: Vec<(&'static str, String)> = vec![
        ("meta.md", meta),
        (STATE_FILE, template_state(status, category)),
    ];
    if is_issue_category(category) {
        // issue 家族专用文档集：问题档案（incident.md）+ 排查流水（notes.md）；
        // 根因与修复决策（root-cause.md）按需生成，不再脚手架 background/technical-plan。
        files.push(("incident.md", template_incident(req_id)));
    } else {
        files.push((
            "background.md",
            background
                .map(str::to_string)
                .unwrap_or_else(|| template_background(req_id)),
        ));
        files.push(("technical-plan.md", template_technical_plan(req_id)));
    }
    files.push((
        "notes.md",
        notes
            .map(str::to_string)
            .unwrap_or_else(|| template_notes(req_id)),
    ));
    files
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn build_meta_doc(
    req_id: &str,
    title: &str,
    status: &str,
    project: &str,
    projects: &[String],
    category: &str,
    source: &str,
    owner: &str,
    start_date: &str,
    plan_release: &str,
    ones: &str,
    issues: &[String],
    summary: &str,
) -> String {
    let mut fm = vec![
        format!("req-id: {}", yaml_quote(req_id)),
        format!("title: {}", yaml_quote(title)),
        format!("status: {}", yaml_quote(status)),
        format!("project: {}", yaml_quote(project)),
    ];
    if projects.len() > 1 {
        fm.push(format!("projects: {}", yaml_quote(&projects.join(", "))));
    }
    fm.push(format!("category: {}", yaml_quote(category)));
    fm.push(format!("source: {}", yaml_quote(source)));
    fm.push(format!("owner: {}", yaml_quote(owner)));
    fm.push(format!("start-date: {}", yaml_quote(start_date)));
    fm.push(format!("plan-release: {}", yaml_quote(plan_release)));
    if !ones.trim().is_empty() {
        fm.push(format!("ones: {}", yaml_quote(ones)));
    }
    if !issues.is_empty() {
        fm.push(format!("issues: {}", yaml_quote(&issues.join(", "))));
    }
    format!(
        "---\n{}\n---\n\n# {} {}\n\n## Summary\n- Title: {}\n- Status: {}\n- Owner: {}\n- Start date: {}\n- Planned release: {}\n- Project: {}\n\n{}\n\n## Scope\n- Include:\n  - 待补充\n- Exclude:\n  - 待补充\n\n## Open Questions\n- 待补充\n",
        fm.join("\n"),
        req_id,
        title,
        title,
        status,
        owner,
        start_date,
        plan_release,
        projects.join(" / "),
        summary.trim()
    )
}

pub(crate) fn template_state(status: &str, category: &str) -> String {
    serde_json::to_string_pretty(&json!({
        "version": 1,
        "status": status,
        "previousStatus": Value::Null,
        "changed": true,
        "lastTransition": Value::Null,
        "category": category,
        "updatedAt": now_ms(),
        "history": [{"status": status, "from": Value::Null, "at": now_ms(), "note": "created", "skippedStatuses": []}]
    }))
    .unwrap_or_else(|_| format!("{{\n  \"version\": 1,\n  \"status\": \"{}\"\n}}\n", status))
}

pub(crate) fn template_alignment(req_id: &str) -> String {
    format!("# {req_id} 需求澄清\n\n## 业务目标\n- 待补充：这次需求要解决的业务问题和成功标准。\n\n## 场景与角色\n- 待补充：涉及的业务角色、对象、入口和主流程。\n\n## PRD 解读\n- 来源：待补充\n- 已确认：待补充\n- 不确定：待补充\n\n## 初步代码调查\n- 相关仓库/模块：待补充\n- 现有系统行为：待补充\n- 初步实现方向：待补充\n\n## 范围与非目标\n- Include：待补充\n- Exclude：待补充\n\n## 待确认问题\n- [ ] 待补充\n")
}

pub(crate) fn template_background(req_id: &str) -> String {
    format!("# {req_id} 业务背景文档\n\n> 面向不熟悉业务的开发、测试和后续经验总结使用；尽量用业务语言说明为什么做、当前怎么运转、这次改变什么。\n\n## 一句话背景\n- 待补充\n\n## 业务目标\n- 待补充\n\n## 业务对象与角色\n- 对象：待补充\n- 角色：待补充\n- 入口：待补充\n\n## 当前系统行为\n- 待补充\n\n## 本次需求改变\n- 待补充\n\n## 关键业务规则\n- 待补充\n\n## 沟通口径\n- 产品/业务确认点：待补充\n- 测试重点：待补充\n\n## 关联知识与经验\n- 业务知识：待补充\n- 历史经验：待补充\n")
}

pub(crate) fn template_memory(req_id: &str, title: &str) -> String {
    format!("# {req_id} Memory\n\n## 当前目标\n- {title}\n\n## 当前进展\n- 已创建需求，待补充进展。\n\n## 关键决策\n- 待补充\n\n## 待办 / 风险\n- [ ] 待补充\n")
}

pub(crate) fn template_branch(req_id: &str) -> String {
    format!("# {req_id} Branches\n\n| Item | Value |\n| --- | --- |\n| Source branch | unknown |\n| Target branch | unknown |\n| Project path | unknown |\n| Merge status | 开发中 |\n\n## Commit / Diff Notes\n- 待补充\n")
}

pub(crate) fn template_config_changes(req_id: &str) -> String {
    format!("# {req_id} Config Changes\n\n> 低层配置明细；上线总览请同步维护 release-manifest.md。\n\n## DB 变更\n- 暂无\n\n## Apollo / Nacos 变更\n- 暂无\n\n## RocketMQ / Console 变更\n- 暂无\n")
}

pub(crate) fn template_release_manifest(req_id: &str) -> String {
    format!("# {req_id} 上线清单\n\n> 贯穿需求全流程维护；用于上线前快速确认本次改了哪些配置、表、Topic、Group、Job、接口和人工动作，避免发布遗漏。\n\n## Summary\n- 结论：暂无上线资产变更 / 待补充\n- 最后更新：待补充\n- 负责人：待补充\n\n## DB / 表变更\n| 类型 | 表/库 | 变更内容 | 环境 | 是否需上线执行 | 回滚/备注 |\n| --- | --- | --- | --- | --- | --- |\n| 无 | - | - | - | 否 | - |\n\n## 配置变更\n| 类型 | Namespace/配置源 | Key/名称 | 变更内容 | 环境 | 是否已发布 | 备注 |\n| --- | --- | --- | --- | --- | --- | --- |\n| 无 | - | - | - | - | 否 | - |\n\n## MQ / Topic / Group\n| 类型 | Topic | Group/Tag | 生产者 | 消费者 | 控制台动作 | 备注 |\n| --- | --- | --- | --- | --- | --- | --- |\n| 无 | - | - | - | - | 否 | - |\n\n## Job / 定时任务 / 开关\n| 类型 | 名称 | 动作 | 环境 | 是否需人工处理 | 备注 |\n| --- | --- | --- | --- | --- | --- |\n| 无 | - | - | - | 否 | - |\n\n## API / 外部依赖\n| 类型 | 接口/系统 | 变更 | 是否需通知 | 备注 |\n| --- | --- | --- | --- | --- |\n| 无 | - | - | 否 | - |\n\n## 上线人工动作\n- [ ] 暂无\n\n## 风险与回滚提醒\n- 待补充\n")
}

pub(crate) fn template_technical_plan(req_id: &str) -> String {
    format!("# {req_id} 技术方案\n\n> Agent 在执行需求过程中持续维护；用于人工在看代码差异前快速判断实现方向、影响范围、风险控制和验证路径。\n\n## 方案摘要\n- 当前结论：待补充\n- 最后更新：待补充\n- 实现状态：待设计 / 开发中 / 已实现 / 待验证\n\n## 实现目标与非目标\n- 目标：待补充\n- 非目标：待补充\n\n## 总体实现方案\n- 方案路径：待补充\n- 选择原因：待补充\n- 替代方案与取舍：待补充\n\n## 影响范围\n| 应用/模块 | 关键文件/类 | 改动类型 | 说明 |\n| --- | --- | --- | --- |\n| 待补充 | 待补充 | 新增/修改/删除 | - |\n\n## 核心流程变化\n- 改造前：待补充\n- 改造后：待补充\n- 关键状态/数据流：待补充\n\n## 数据、配置与兼容性\n- DB/表字段：暂无 / 待补充\n- 配置/Apollo/Nacos：暂无 / 待补充\n- MQ/Job/外部接口：暂无 / 待补充\n- 兼容性：待补充\n\n## 风险、灰度与回滚\n- 核心链路风险：待评估\n- 性能/并发/幂等风险：待评估\n- 灰度/开关：待补充\n- 回滚方案：待补充\n\n## 验证计划\n- 单测：待补充\n- 接口/链路自测：待补充\n- 回归范围：待补充\n- 观测日志/DB 证据：待补充\n\n## 人工审查关注点\n- 待补充\n\n## 待确认问题\n- 待补充\n")
}

pub(crate) fn template_impact(req_id: &str) -> String {
    format!("# {req_id} Impact\n\n## 风险等级\n- 待评估\n\n## 核心链路影响\n- 待补充\n\n## 回滚方案\n- 待补充\n")
}

pub(crate) fn template_test(req_id: &str) -> String {
    format!("# {req_id} Test\n\n## 测试场景清单\n\n| ID | 场景描述 | 触发方式 | 前置条件 | 预期结果 | 证据标准 |\n| --- | --- | --- | --- | --- | --- |\n| S1 | 待补充 | 待补充 | 待补充 | 待补充 | 日志 + DB + 副作用 + 反向检查 |\n\n## 自测记录\n- ⬜ 待执行\n\n## UAT 回归记录\n- ⬜ 待执行\n")
}

pub(crate) fn template_test_scenario(req_id: &str) -> String {
    format!("# {req_id} 测试场景\n\n> 用途：开发推动的需求，测试无法像产品需求那样向产品经理确认测试范围，本档由开发负责沉淀，让测试自主理解需求并评估/补充测试范围。进入「测试中」前必须填完。\n\n## 需求说明（这个需求是干嘛的）\n- 背景与目标：待补充（为什么做这个需求，解决什么问题）\n- 使用场景与角色：待补充（谁在什么入口/场景使用）\n- 功能点/变更点清单：待补充（新增/修改/删除了哪些功能点，涉及哪些页面/接口/表）\n\n## 开发评估的测试范围\n- 重点场景：待补充（开发认为必须覆盖的主链路）\n- 边界与异常：待补充（空值/并发/失败分支/回退/权限）\n- 影响面：待补充（受影响的既有功能、接口调用方、数据）\n- 不需要测的范围：待补充（明确排除，避免测试浪费）\n\n## 测试覆盖场景\n| # | 场景 | 前置数据 | 操作步骤 | 预期结果 | 优先级 |\n| --- | --- | --- | --- | --- | --- |\n| 1 | 待补充 | 待补充 | 待补充 | 待补充 | P0 |\n\n## 自测结论与证据\n- 已自测场景：待补充\n- 遗留风险：待补充\n")
}

pub(crate) fn template_troubleshooting(req_id: &str) -> String {
    format!("# {req_id} 排查经验\n\n> 用途：沉淀本线上问题的完整排查路径与修复方案，让同类问题下次直接复用；推进到「已复盘」前必须填完本档。\n\n## 现象与影响\n- 环境/时间窗口：待补充\n- 现象：待补充（报错、指标、用户反馈）\n- 影响范围：待补充（仓库/租户/单号/接口）\n\n## 怎么排查（可复用的定位路径）\n- 证据链：待补充（日志关键字/tid、DB 状态、接口返回、MQ/Job 状态）\n- 排查步骤：待补充（按顺序记录：先看什么、再看什么、在哪里分叉）\n- 用到的工具/命令/知识：待补充（Kibana 查询、SQL、相关业务知识/经验条目 id）\n\n## 根因\n- 直接原因：待补充\n- 深层原因：待补充（为什么会发生：流程/校验/变更遗漏）\n\n## 怎么修复\n- 修复方案：待补充（代码变更、配置、数据修复、人工动作）\n- 验证方式：待补充（如何确认修复生效、回归范围）\n- 是否转需求：待补充（需要常规开发承接时记录需求 id）\n\n## 复用清单\n- [ ] 下次遇到同类现象，按「怎么排查」逐步执行\n- [ ] 已落地到经验库：待补充（经验条目 id/链接，无则写「未落地」及原因）\n")
}

pub(crate) fn template_incident(req_id: &str) -> String {
    format!("# {req_id} 问题档案\n\n> 用途：回答这个问题「是什么」。排查中创建并持续更新，是问题身份证；根因和修复决策写 root-cause.md，过程流水写 notes.md。\n\n## 现象描述\n- 现象：待补充（报错、指标异常、功能不可用）\n- 发现渠道：待补充（用户反馈/告警/巡检/测试发现）\n\n## 环境与时间窗口\n- 环境：待补充（test / cn-uat / sea-uat / cn-pro / sea-pro）\n- 首次发生：待补充（带时区绝对时间）\n- 最近发生：待补充（带时区绝对时间）\n- 是否仍在发生：待补充\n\n## 影响范围\n- 业务影响：待补充（单量/订单/租户/用户数量级）\n- 关键对象：待补充（仓库/单号/接口/租户）\n- 紧急程度：待补充\n\n## 复现步骤\n- 待补充（能稳定复现时记录精确步骤；不能复现记录当时条件）\n\n## 时间线\n- 待补充（发现 → 定位 → 修复 → 恢复，逐条带时间）\n")
}

pub(crate) fn template_root_cause(req_id: &str) -> String {
    format!("# {req_id} 根因与修复决策\n\n> 用途：回答「为什么」和「怎么办」。每条证据必须附用户可独立复核的验证线索：日志证据=时间范围+tid/关键字；DB 证据=验证 SQL；代码证据=应用+文件+可搜关键字；配置证据=环境+key。推进到「已定位」前必须填完根因、证据链和修复路径决策。\n\n## 根因\n- 直接原因：待补充\n- 深层原因：待补充（为什么会发生：流程/校验/变更遗漏）\n\n## 证据链（每条必须可复核）\n- 日志证据：待补充（环境/索引/应用 + tid 或唯一关键字 + 带时区绝对时间范围）\n- DB 证据：待补充（验证 SQL：表、条件、预期结果；必要时附查询结果摘要）\n- 代码证据：待补充（应用名 + 文件路径 + 可全局搜索的关键字片段：日志文本/方法名/常量）\n- 配置证据：待补充（Apollo/Nacos namespace + key + 环境，如有）\n\n## 影响面\n- 待补充（哪些业务对象/数据受影响，与 incident.md 影响范围呼应）\n\n## 修复路径决策\n- 结论：待补充（数据修复直接推进已修复 / 代码修复转普通需求（需求 id）/ 复现代码已验证但仅测试环境使用（分支 id，不合入生产）/ 不修复）\n- 临时处置：待补充（应急 SQL 及影响行数、应急配置回滚，如有）\n- 回滚方案：待补充\n\n## 验证方式\n- 待补充（如何确认修复生效，附可复核的查询/日志线索）\n")
}

pub(crate) fn template_experience_summary(req_id: &str) -> String {
    format!("# {req_id} 经验总结\n\n## 本次需求结论\n- 待补充\n\n## 新发现的业务知识\n| 发现 | 是否已落地 | 目标位置 | 备注 |\n| --- | --- | --- | --- |\n| 待补充 | 否 | .agents/business-knowledge/ | - |\n\n## 新发现的经验 / 踩坑\n| 经验 | 是否已落地 | 目标位置 | 备注 |\n| --- | --- | --- | --- |\n| 待补充 | 否 | .agents/experiences/ | - |\n\n## Skill 改进机会\n| Skill | 问题 / 机会 | 动作 | 状态 |\n| --- | --- | --- | --- |\n| 待补充 | 待补充 | 新增/优化/不处理 | 待落地 |\n\n## 流程改进\n- 待补充\n\n## 已落地清单\n- [ ] 待补充\n\n## 待落地清单\n- [ ] 待补充\n")
}

pub(crate) fn template_notes(req_id: &str) -> String {
    format!("# {req_id} Notes\n\n## 当前状态\n- 需求已创建。\n\n## 待跟进\n- [ ] 补充需求背景、影响面、分支和测试证据。\n")
}

pub(crate) fn update_meta_summary_line(raw: &str, label: &str, value: &str) -> String {
    let prefix = format!("- {label}:");
    let mut changed = false;
    let lines: Vec<String> = raw
        .split('\n')
        .map(|line| {
            if line.trim_start().starts_with(&prefix) {
                changed = true;
                format!("- {label}: {value}")
            } else {
                line.to_string()
            }
        })
        .collect();
    if changed {
        lines.join("\n")
    } else {
        raw.to_string()
    }
}

pub(crate) fn requirement_doc_template(req: &Requirement, doc_file: &str) -> String {
    match doc_file {
        "alignment.md" => template_alignment(&req.id),
        "background.md" => template_background(&req.id),
        "memory.md" => template_memory(&req.id, &req.title),
        "branch.md" => template_branch(&req.id),
        "config-changes.md" => template_config_changes(&req.id),
        "release-manifest.md" => template_release_manifest(&req.id),
        "technical-plan.md" => template_technical_plan(&req.id),
        "impact.md" => template_impact(&req.id),
        "test.md" => template_test(&req.id),
        "experience-summary.md" => template_experience_summary(&req.id),
        "troubleshooting.md" => template_troubleshooting(&req.id),
        "incident.md" => template_incident(&req.id),
        "root-cause.md" => template_root_cause(&req.id),
        "test-scenario.md" => template_test_scenario(&req.id),
        "notes.md" => template_notes(&req.id),
        _ => String::new(),
    }
}

pub(crate) fn requirement_doc_file(doc_type: &str) -> ApiResult<&'static str> {
    match doc_type.trim() {
        "background" | "background.md" => Ok("background.md"),
        "memory" | "memory.md" => Ok("memory.md"),
        "branch" | "branch.md" => Ok("branch.md"),
        "config" | "config-changes" | "config-changes.md" => Ok("config-changes.md"),
        "release-manifest" | "releasemanifest" | "manifest" | "release-manifest.md" => {
            Ok("release-manifest.md")
        }
        "technical-plan"
        | "technicalplan"
        | "implementation-plan"
        | "implementationplan"
        | "tech-plan"
        | "techplan"
        | "solution"
        | "technical-plan.md" => Ok("technical-plan.md"),
        "impact" | "impact.md" => Ok("impact.md"),
        "test" | "test.md" => Ok("test.md"),
        "notes" | "notes.md" => Ok("notes.md"),
        "review" | "review.md" => Ok("review.md"),
        "release-check" | "releasecheck" | "release-check.md" => Ok("release-check.md"),
        "experience-summary" | "experiencesummary" | "experience-summary.md" => {
            Ok("experience-summary.md")
        }
        "troubleshooting" | "troubleshooting.md" | "postmortem" | "排查经验" => {
            Ok("troubleshooting.md")
        }
        "incident" | "incident.md" | "现象" => Ok("incident.md"),
        "root-cause" | "rootcause" | "root-cause.md" | "根因" => Ok("root-cause.md"),
        "test-scenario" | "testscenario" | "test-scenario.md" | "测试场景" => {
            Ok("test-scenario.md")
        }
        "alignment" | "alignment.md" => Ok("alignment.md"),
        "prd" | "prd.md" => Ok("prd.md"),
        other => Err(ApiError::bad_request(format!(
            "unsupported docType: {other}"
        ))),
    }
}

pub(crate) fn ensure_doc_heading(req_id: &str, doc_file: &str, content: &str) -> String {
    let clean = content.trim_start_matches('\u{feff}').trim_start();
    if clean.starts_with('#') {
        format!("{}\n", content.trim_end())
    } else {
        format!("# {} {}\n\n{}\n", req_id, doc_file, content.trim_end())
    }
}
