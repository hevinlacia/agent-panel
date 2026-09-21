use super::*;

const SELFTEST_SECTION: &str = "自测清单";

/// 「自测清单」结果单元格的分类。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SelftestResult {
    /// 通过
    Pass,
    /// 失败 / 无法测试 / 阻塞 / 跳过（必须写明具体原因）
    Blocked,
    /// 缺少测试结果（门禁不通过）
    Missing,
}

/// 结果单元格为这些值（trim + 小写后整格匹配）时视为缺少测试结果。
const MISSING_RESULT_CELLS: [&str; 15] = [
    "", "-", "—", "--", "/", "无", "空", "待测试", "待测", "未测试", "未测", "待执行", "待补充",
    "待定", "未执行",
];

/// 原因/说明单元格为这些值时视为没有实际内容。
const PLACEHOLDER_CELLS: [&str; 10] = [
    "", "-", "—", "--", "/", "无", "空", "待补充", "todo", "tbd",
];

/// 自测门禁：自测中推进「测试中」前，test.md 必须有「自测清单」且每项都有测试结果，
/// 失败/无法测试的项必须写明具体原因（如环境未部署、依赖缺失等确实无法测试的情况）。
pub(crate) async fn ensure_selftest_checklist_allows_testing(req: &Requirement) -> ApiResult<()> {
    let problems = selftest_checklist_problems(req).await;
    if problems.is_empty() {
        return Ok(());
    }
    Err(selftest_gate_error(problems))
}

/// 收集自测清单当前问题；空列表 = 通过。供门禁拦截与状态流转卡片只读评估复用。
pub(crate) async fn selftest_checklist_problems(req: &Requirement) -> Vec<String> {
    // 抢修模式：关联了线上问题的需求默认放行自测门禁（速度优先；用户明确要求自测时再按常规维护）。
    if req.is_hotfix() {
        return Vec::new();
    }
    let Some(dir) = req.req_dir.as_deref() else {
        return Vec::new();
    };
    let path = PathBuf::from(dir).join("test.md");
    if !path.is_file() {
        return vec!["未找到 test.md".to_string()];
    }
    let body = tokio::fs::read_to_string(&path).await.unwrap_or_default();
    validate_selftest_checklist(&body)
}

fn selftest_gate_error(problems: Vec<String>) -> ApiError {
    ApiError::bad_request(format!(
        "自测门禁未通过：自测中推进「测试中」前，必须在 test.md 的「## {SELFTEST_SECTION}」表格中列出测试项目并逐项填写测试结果（通过/失败/无法测试）；失败或无法测试的项必须写明具体原因（如环境未部署、依赖缺失等确实无法测试的情况）。当前问题：\n{}\n期望格式：\n| # | 自测项 | 结果 | 失败/无法测试原因 |\n| --- | --- | --- | --- |\n| 1 | 场景名 | 通过 | - |\n| 2 | 场景名 | 无法测试 | test 环境 OMS 未订阅 topic，无法联调 |",
        problems
            .iter()
            .map(|p| format!("- {p}"))
            .collect::<Vec<_>>()
            .join("\n")
    ))
}

/// 纯函数校验：返回问题列表，空列表表示通过。
pub(crate) fn validate_selftest_checklist(body: &str) -> Vec<String> {
    let Some(section) = extract_selftest_section(body) else {
        return vec![format!("未找到「## {SELFTEST_SECTION}」小节")];
    };
    let rows = parse_table_rows(&section);
    let Some(header) = rows.first() else {
        return vec![format!(
            "「## {SELFTEST_SECTION}」下没有表格，请按期望格式补一张测试项目清单表"
        )];
    };
    let Some(result_col) = find_result_column(header) else {
        return vec![format!(
            "自测清单表头缺少「结果」列，请使用表头：| # | 自测项 | 结果 | 失败/无法测试原因 |"
        )];
    };
    let mut problems = Vec::new();
    let mut item_count = 0usize;
    for row in rows.iter().skip(1) {
        if is_header_row(row) || row.iter().all(|c| c.is_empty()) {
            continue;
        }
        item_count += 1;
        let label = row_item_label(row, result_col, item_count);
        let Some(result_cell) = row.get(result_col) else {
            problems.push(format!("「{label}」缺少测试结果"));
            continue;
        };
        match classify_result_cell(result_cell) {
            SelftestResult::Pass => {}
            SelftestResult::Blocked => {
                let reason = row
                    .get(result_col + 1..)
                    .and_then(|cells| cells.iter().find_map(|c| meaningful_reason(c)))
                    .or_else(|| {
                        if has_inline_reason(result_cell) {
                            Some(String::new())
                        } else {
                            None
                        }
                    });
                if reason.is_none() {
                    problems.push(format!(
                        "「{label}」结果为「{}」，必须写明具体的失败/无法测试原因（如 test 环境缺依赖、服务未部署等确实无法完成的场景），不能只写「-」或留空",
                        result_cell.trim()
                    ));
                }
            }
            SelftestResult::Missing => {
                let shown = result_cell.trim();
                let shown = if shown.is_empty() { "空" } else { shown };
                problems.push(format!(
                    "「{label}」缺少测试结果（当前：{shown}）；每项必须填写 通过/失败/无法测试"
                ));
            }
        }
    }
    if item_count == 0 {
        problems.push(format!(
            "「## {SELFTEST_SECTION}」表格里没有任何测试项目，请逐项列出本次自测的测试项"
        ));
    }
    problems
}

/// 截取 test.md 中「自测清单」小节正文（从匹配标题到下一个同级或更高级标题之前）。
fn extract_selftest_section(body: &str) -> Option<String> {
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
                    if title.contains(SELFTEST_SECTION) {
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

/// 解析小节内的 markdown 表格行；跳过分隔行与全空行。
fn parse_table_rows(section: &str) -> Vec<Vec<String>> {
    let mut rows = Vec::new();
    for line in section.lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with('|') {
            continue;
        }
        let cells: Vec<String> = trimmed
            .trim_matches('|')
            .split('|')
            .map(|c| c.trim().to_string())
            .collect();
        if cells.iter().all(|c| c.is_empty() || is_separator_cell(c)) {
            continue;
        }
        rows.push(cells);
    }
    rows
}

fn is_separator_cell(cell: &str) -> bool {
    let inner = cell.trim_matches(':');
    !inner.is_empty() && inner.chars().all(|c| c == '-')
}

fn is_header_row(row: &[String]) -> bool {
    row.iter().any(|c| c.contains("自测项")) && row.iter().any(|c| c.contains("结果"))
}

fn find_result_column(header: &[String]) -> Option<usize> {
    header
        .iter()
        .position(|c| c.contains("结果") || c.to_lowercase().contains("result"))
}

fn classify_result_cell(cell: &str) -> SelftestResult {
    let v = cell.trim().to_lowercase();
    if MISSING_RESULT_CELLS.contains(&v.as_str())
        || matches!(v.as_str(), "todo" | "pending" | "n/a" | "na" | "⬜")
    {
        return SelftestResult::Missing;
    }
    let fail_markers = ["未通过", "不通过", "无法通过", "失败", "fail", "❌", "✖", "报错", "bug"];
    if fail_markers.iter().any(|m| v.contains(m)) {
        return SelftestResult::Blocked;
    }
    let blocked_markers = [
        "无法测试",
        "无法验证",
        "没法测试",
        "不能测试",
        "阻塞",
        "blocked",
        "跳过",
        "skip",
    ];
    if blocked_markers.iter().any(|m| v.contains(m)) {
        return SelftestResult::Blocked;
    }
    let pass_markers = ["通过", "成功", "pass", "ok", "✅", "✔"];
    if pass_markers.iter().any(|m| v.contains(m)) {
        return SelftestResult::Pass;
    }
    SelftestResult::Missing
}

/// 结果列内联写了原因（如「失败：test 环境未部署」）时也算有原因。
fn has_inline_reason(result_cell: &str) -> bool {
    let mut v = result_cell.trim().to_lowercase();
    for m in [
        "未通过", "不通过", "无法通过", "失败", "fail", "❌", "✖", "报错", "bug", "无法测试",
        "无法验证", "没法测试", "不能测试", "阻塞", "blocked", "跳过", "skip", "通过", "成功",
        "pass", "ok", "✅", "✔", "⬜",
    ] {
        v = v.replace(m, "");
    }
    let v = v.trim_matches(|c: char| {
        matches!(
            c,
            ':' | '：' | ',' | '，' | ';' | '；' | '、' | '(' | ')' | '（' | '）' | '-' | '—' | ' '
        )
    });
    v.chars().count() >= 4
}

fn meaningful_reason(cell: &str) -> Option<String> {
    let v = cell.trim();
    if PLACEHOLDER_CELLS.contains(&v.to_lowercase().as_str()) {
        return None;
    }
    if v.chars().count() < 4 {
        return None;
    }
    Some(v.to_string())
}

/// 从结果列之前的单元格中取测试项名称（跳过序号列和占位符）。
fn row_item_label(row: &[String], result_col: usize, row_no: usize) -> String {
    let end = result_col.min(row.len());
    row[..end]
        .iter()
        .rev()
        .find(|c| {
            !c.is_empty()
                && !PLACEHOLDER_CELLS.contains(&c.to_lowercase().as_str())
                && !c.chars().all(|ch| ch.is_ascii_digit())
        })
        .cloned()
        .unwrap_or_else(|| format!("第 {row_no} 项"))
}
