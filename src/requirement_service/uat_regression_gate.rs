use super::*;

const UAT_SECTION: &str = "UAT 回归";

/// 「证据」列被判定为计划/待办/流程动作（而非已发生的可核对事实）的标记词。
/// 出现在「通过」行的证据里 = 证据不可采：状态流转、计划、推断都不算回归证据
/// （WMS-136 实测：release-check 曾把「用户手工回归后推进发布就绪」当 UAT 证据）。
const EVIDENCE_PLAN_MARKERS: &[&str] = &[
    "待执行",
    "待回归",
    "待测试",
    "待部署",
    "待确认",
    "待验证",
    "待跑",
    "待补",
    "尚未",
    "还未",
    "还没",
    "计划",
    "后续",
    "回头",
    "由测试执行",
    "由测试人员",
    "测试人员执行",
    "后推进",
    "状态推进",
    "已推进",
    "推进到",
    "推进至",
    "状态流转",
];

/// UAT 回归校验结果：problems 非空 = 门禁不放行；warnings 非空 = 放行但需警示（结果未知）。
#[derive(Debug, Default)]
pub(crate) struct UatRegressionEval {
    pub(crate) problems: Vec<String>,
    pub(crate) warnings: Vec<String>,
}

/// UAT 回归条目详情：门禁详情页逐项展示用（复用自测清单门禁的详情结构）。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UatItemDetail {
    pub(crate) no: usize,
    pub(crate) item: String,
    /// pass / fail / cannot / missing
    pub(crate) result: &'static str,
    /// 原始结果单元格文本
    pub(crate) result_text: String,
    /// 证据列原文；空/占位为 None
    pub(crate) evidence: Option<String>,
}

/// UAT 回归门禁：测试中推进「人工核查/发布就绪」前，test.md 必须有「## UAT 回归」小节，
/// 逐项给出回归结果与证据。全部通过（证据为已发生事实）放行；「无法测试」（有原因）放行但警示；
/// 失败 / 未执行 / 缺结果 / 通过但证据不可采不放行；整节 `不适用：<原因>` 放行但警示。
pub(crate) async fn ensure_uat_regression_allows_release(req: &Requirement) -> ApiResult<()> {
    let eval = uat_regression_eval(req).await;
    if eval.problems.is_empty() {
        return Ok(());
    }
    Err(uat_gate_error(eval.problems))
}

/// 收集 UAT 回归当前问题；空列表 = 通过。供门禁拦截、只读评估与 compliance API 复用。
pub(crate) async fn uat_regression_eval(req: &Requirement) -> UatRegressionEval {
    if req.is_hotfix() {
        return UatRegressionEval::default();
    }
    let Some(dir) = req.req_dir.as_deref() else {
        return UatRegressionEval::default();
    };
    let path = PathBuf::from(dir).join("test.md");
    if !path.is_file() {
        return UatRegressionEval {
            problems: vec!["未找到 test.md".to_string()],
            warnings: Vec::new(),
        };
    }
    let body = tokio::fs::read_to_string(&path).await.unwrap_or_default();
    validate_uat_regression(&body).0
}

/// 门禁详情：读取 test.md 并返回结构化 UAT 回归清单（found=false 表示无文件/无小节）。
/// sections 结构与自测清单详情一致，前端可复用同一渲染组件。
pub(crate) async fn uat_regression_detail(req: &Requirement) -> Value {
    let missing = json!({ "found": false, "sections": [] });
    if req.is_hotfix() {
        return missing;
    }
    let Some(dir) = req.req_dir.as_deref() else {
        return missing;
    };
    let path = PathBuf::from(dir).join("test.md");
    if !path.is_file() {
        return missing;
    }
    let body = tokio::fs::read_to_string(&path).await.unwrap_or_default();
    let (eval, na_reason, items) = validate_uat_regression(&body);
    json!({
        "found": true,
        "sections": [{
            "category": "UAT 回归清单",
            "notApplicableReason": na_reason,
            "riskAnalysis": [],
            "items": items,
        }],
        "problems": eval.problems,
        "warnings": eval.warnings,
    })
}

/// 纯函数校验：返回（问题+警示，整节不适用原因，条目详情）。
/// problems 非空 = 不放行（缺小节/缺表列/缺结果/存在失败/通过但证据不可采）；
/// warnings 非空 = 放行但警示（无法测试结果未知 / 整节不适用）。
pub(crate) fn validate_uat_regression(body: &str) -> (UatRegressionEval, Option<String>, Vec<UatItemDetail>) {
    let mut eval = UatRegressionEval::default();
    let Some(section) = extract_uat_section(body) else {
        eval.problems.push(format!(
            "未找到「## {UAT_SECTION}」小节：UAT 回归结论必须落在 test.md 的「## {UAT_SECTION}」标题下；test 环境自测清单的结果不能替代 UAT 回归"
        ));
        return (eval, None, Vec::new());
    };
    // 整节不适用：写 `不适用：<原因>` 即放行，但警示留痕。
    if let Some(reason) = uat_na_reason(&section) {
        eval.warnings
            .push(format!("UAT 回归整节标注不适用：{reason}——请确认该结论仍符合当前改动范围"));
        return (eval, Some(reason), Vec::new());
    }
    let rows = parse_table_rows(&section);
    let Some(header) = rows.first() else {
        eval.problems.push(format!(
            "「## {UAT_SECTION}」里没有回归清单表格，请使用表头：| # | 场景 | 结果 | 证据 |"
        ));
        return (eval, None, Vec::new());
    };
    let Some(result_col) = find_result_column(header) else {
        eval.problems.push(format!(
            "「## {UAT_SECTION}」表头缺少「结果」列，请使用表头：| # | 场景 | 结果 | 证据 |"
        ));
        return (eval, None, Vec::new());
    };
    let Some(evidence_col) = header
        .iter()
        .position(|c| c.contains("证据") || c.to_lowercase().contains("evidence"))
    else {
        eval.problems.push(
            "「## UAT 回归」表头缺少「证据」列：每一项「通过」都必须有可核对的证据（tid/日志关键字/接口返回/DB 前后值）"
                .to_string(),
        );
        return (eval, None, Vec::new());
    };
    let mut count = 0usize;
    let mut items = Vec::new();
    for row in rows.iter().skip(1) {
        if row.iter().all(|c| c.is_empty()) {
            continue;
        }
        // 重复表头行跳过。
        if row.iter().any(|c| c.contains("场景")) && row.iter().any(|c| c.contains("结果")) {
            continue;
        }
        count += 1;
        let scenario = row_item_label(row, result_col, count);
        let label = format!("UAT 回归 · {scenario}");
        let result_text = row.get(result_col).cloned().unwrap_or_default();
        let evidence_text = row.get(evidence_col).cloned().unwrap_or_default();
        let has_reason_after = row
            .get(result_col + 1..)
            .and_then(|cells| cells.iter().find_map(|c| meaningful_reason(c)))
            .is_some()
            || has_inline_reason(&result_text);
        let evidence = meaningful_reason(&evidence_text);
        match classify_result_str(&result_text) {
            "pass" => {
                match evidence {
                    Some(ev) => {
                        if let Some(marker) = evidence_plan_marker(&ev) {
                            eval.problems.push(format!(
                                "「{label}」证据是计划/流程动作（含「{marker}」），不是已发生的回归证据：状态流转、计划、推断都不算证据；请先在 UAT 真实执行并记录结果，或把该项改为「未执行/无法测试」并写明原因"
                            ));
                        }
                        items.push(UatItemDetail {
                            no: count,
                            item: scenario,
                            result: "pass",
                            result_text: result_text.trim().to_string(),
                            evidence: Some(ev),
                        });
                    }
                    None => {
                        eval.problems.push(format!(
                            "「{label}」结果为通过但证据列为空/占位：通过必须有已发生的可核对证据（tid/日志关键字/接口返回/DB 前后值），不能只写「-」或留空"
                        ));
                        items.push(UatItemDetail {
                            no: count,
                            item: scenario,
                            result: "pass",
                            result_text: result_text.trim().to_string(),
                            evidence: None,
                        });
                    }
                }
            }
            "fail" => {
                if !has_reason_after {
                    eval.problems.push(format!(
                        "「{label}」结果为「{}」（失败），必须写明具体的失败原因与影响，不能只写「-」或留空",
                        result_text.trim()
                    ));
                } else {
                    eval.problems.push(format!(
                        "「{label}」存在失败项（原因已记录）：UAT 回归未通过门禁不放行，修复并复验后把结果改为「通过」再推进"
                    ));
                }
                items.push(UatItemDetail {
                    no: count,
                    item: scenario,
                    result: "fail",
                    result_text: result_text.trim().to_string(),
                    evidence,
                });
            }
            "cannot" => {
                if has_reason_after {
                    eval.warnings.push(format!(
                        "「{label}」无法测试，结果未知——门禁放行但发布前需确认该场景有替代覆盖（人工回归/测试同事兜底）"
                    ));
                } else {
                    eval.problems.push(format!(
                        "「{label}」结果为「{}」（无法测试），必须写明具体原因（如 UAT 缺依赖、场景无入口等确实无法完成的情形），不能只写「-」或留空",
                        result_text.trim()
                    ));
                }
                items.push(UatItemDetail {
                    no: count,
                    item: scenario,
                    result: "cannot",
                    result_text: result_text.trim().to_string(),
                    evidence,
                });
            }
            _ => {
                if result_text.trim() == "未执行" {
                    eval.problems.push(format!(
                        "「{label}」结果为「未执行」：先在 UAT 执行并记录证据后改为「通过」；确实无需 UAT 回归时整节写 `不适用：<原因>`"
                    ));
                } else {
                    let shown = if result_text.trim().is_empty() { "空" } else { result_text.trim() };
                    eval.problems.push(format!(
                        "「{label}」缺少回归结果（当前：{shown}）；每项必须填写 通过/失败/无法测试/未执行"
                    ));
                }
                items.push(UatItemDetail {
                    no: count,
                    item: scenario,
                    result: "missing",
                    result_text: result_text.trim().to_string(),
                    evidence,
                });
            }
        }
    }
    if count == 0 {
        eval.problems.push(format!(
            "「## {UAT_SECTION}」没有任何回归条目（只有表头或为空）：请把 UAT 回归场景逐项列出并填写结果与证据"
        ));
    }
    (eval, None, items)
}

/// 「通过」行的证据里命中计划/流程动作标记词时返回该标记。
fn evidence_plan_marker(evidence: &str) -> Option<String> {
    let hit = EVIDENCE_PLAN_MARKERS.iter().find(|m| evidence.contains(**m))?;
    Some((*hit).to_string())
}

/// 整节「不适用：<原因>」的原因文本（无标注或原因无实质内容返回 None）。
fn uat_na_reason(section: &str) -> Option<String> {
    let line = section.lines().find(|l| {
        let t = l.trim();
        !t.starts_with('|') && !t.starts_with('#') && t.contains("不适用")
    })?;
    let reason = line
        .trim()
        .trim_start_matches(['-', '*', ' '])
        .replacen("不适用", "", 1)
        .trim()
        .trim_start_matches([':', '：'])
        .trim()
        .to_string();
    meaningful_reason(&reason)?;
    Some(reason)
}

/// 截取 test.md 中「UAT 回归」小节正文（从匹配标题到下一个同级或更高级标题之前）。
/// 标题匹配忽略大小写与空格：「UAT 回归」「UAT回归」「UAT 回归结论」「UAT 回归记录」均命中。
fn extract_uat_section(body: &str) -> Option<String> {
    let mut section: Option<(usize, String)> = None;
    for line in body.lines() {
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed.strip_prefix('#') {
            let level = rest.chars().take_while(|c| *c == '#').count();
            let title = rest.trim_start_matches('#').trim();
            match section.as_mut() {
                Some((sec_level, buf)) => {
                    if level <= *sec_level {
                        break;
                    }
                    buf.push_str(line);
                    buf.push('\n');
                }
                None => {
                    if heading_is_uat(title) {
                        section = Some((level, String::new()));
                    }
                }
            }
            continue;
        }
        if let Some((_, buf)) = section.as_mut() {
            buf.push_str(line);
            buf.push('\n');
        }
    }
    section.map(|(_, buf)| buf)
}

fn heading_is_uat(title: &str) -> bool {
    let norm: String = title
        .to_lowercase()
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    norm.contains("uat回归")
}

fn uat_gate_error(problems: Vec<String>) -> ApiError {
    ApiError::bad_request(format!(
        "UAT 回归门禁未通过：进入「人工核查/发布就绪」前，test.md 必须有「## {UAT_SECTION}」小节，逐项给出回归结果与证据（结果=通过/失败/无法测试/未执行；证据=已发生的可核对事实 tid/日志关键字/接口返回/DB 前后值）。全部通过放行；「无法测试」（有原因）放行但警示；失败/未执行/缺结果/通过但证据不可采不放行；整节不适用写 `不适用：<原因>`。当前问题：\n{}\n期望格式：\n## {UAT_SECTION}\n\n| # | 场景 | 结果 | 证据 |\n| --- | --- | --- | --- |\n| 1 | 混合明细任务确认（缺货明细保留） | 通过 | UAT tid=abc123；DB 头 900、缺货明细 700 保留 |\n| 2 | 整单取消后明细行展示 | 无法测试 | UAT 无整单取消入口，由测试同事人工覆盖 |",
        problems
            .iter()
            .map(|p| format!("- {p}"))
            .collect::<Vec<_>>()
            .join("\n")
    ))
}
