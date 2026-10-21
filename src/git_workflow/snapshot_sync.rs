use super::*;

/// 审查快照 git 同步：worker 副本生成后自动 commit+push，hub 门禁读取前节流 pull。
///
/// 背景：容器分离架构下 worker 与 hub 各持一份数据仓库副本（worker =
/// `~/Developer/agent-panel-data`，hub = `/srv/agent-panel/data`）。code-review.json
/// 的生成是 git 执行类（只能在 worker 侧跑 diff），产物落在 worker 副本；而门禁
/// （review-gate / 状态推进强校验）读的是 hub 侧副本，且没有任何 API 能写
/// code-review.json（file API 仅允许 .md）——不补这条同步链路，就会出现
/// 「新材料只在本地、门禁拿旧快照判 stale」的死结（INC-149 实测）。
///
/// 闭环：worker 备料/刷新快照后 `push_snapshot_commit`（秒级 push）→ agent 调门禁
/// 时 hub 侧 `freshen_git_copy_for_gate` 节流 pull（秒级追平）→ 全链路无人工 git。

/// 单条 git 命令超时；push/pull 涉及网络，给足但不上不封顶。
const GIT_TIMEOUT: Duration = Duration::from_secs(15);
/// hub 门禁 pull 节流：避免同一分钟内多次门禁查询反复触发网络 pull。
const GATE_PULL_THROTTLE: Duration = Duration::from_secs(30);

/// 从 req_dir 向上找 `.git`（数据 SSOT 是 git clone，容器与本机两种部署都成立）。
/// 找不到返回 None（如内存临时目录），调用方按「未同步」处理。
pub(crate) fn data_repo_root(req_dir: &Path) -> Option<PathBuf> {
    let mut cur = Some(req_dir);
    while let Some(dir) = cur {
        if dir.join(".git").exists() {
            return Some(dir.to_path_buf());
        }
        cur = dir.parent();
    }
    None
}

async fn run_git(root: &Path, args: &[String]) -> Result<String> {
    let output = timeout(
        GIT_TIMEOUT,
        Command::new("git").args(args).current_dir(root).output(),
    )
    .await
    .map_err(|_| {
        anyhow!(
            "git {} 超时（{}s）",
            args.first().map(String::as_str).unwrap_or(""),
            GIT_TIMEOUT.as_secs()
        )
    })?
    .map_err(|e| {
        anyhow!(
            "git {} 启动失败: {e}",
            args.first().map(String::as_str).unwrap_or("")
        )
    })?;
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    if !output.status.success() {
        return Err(anyhow!("git {} 失败: {}", args.join(" "), combined.trim()));
    }
    Ok(combined)
}

/// 快照落盘后把需求目录的变更 commit 并 push 到远端（worker 侧调用，best-effort）。
/// 返回 None = 已同步或无变化；返回 Some(warning) = 同步失败，调用方把它放进响应
/// 提醒 agent 走手动 git 桥接（退化为旧行为，不阻断主流程）。
pub(crate) async fn push_snapshot_commit(req_dir: &Path, req_id: &str) -> Option<String> {
    let Some(root) = data_repo_root(req_dir) else {
        return Some("快照未自动同步 git（需求目录不在 git 仓库内）：hub 门禁将读到旧快照，请手动把 code-review.json 同步到数据仓库".to_string());
    };
    let stamp = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC");
    let msg = format!("review: sync snapshot for {req_id} ({stamp})");
    let dir_arg = req_dir.to_string_lossy().to_string();
    let steps: [(&str, Vec<String>); 3] = [
        (
            "add",
            vec![
                "add".to_string(),
                "-A".to_string(),
                "--".to_string(),
                dir_arg.clone(),
            ],
        ),
        (
            "commit",
            vec![
                "-c".to_string(),
                "user.name=agent-panel-worker".to_string(),
                "-c".to_string(),
                "user.email=worker@localhost".to_string(),
                "commit".to_string(),
                "-m".to_string(),
                msg.clone(),
                "--".to_string(),
                dir_arg,
            ],
        ),
        ("push", vec!["push".to_string()]),
    ];
    for (label, args) in steps {
        match run_git(&root, &args).await {
            Ok(out) => {
                // 无变更时 commit 以非零退出并提示 nothing to commit——视为已同步，不算失败。
                if label == "commit" && out.contains("nothing to commit") {
                    return None;
                }
            }
            Err(e) => {
                let text = e.to_string();
                if label == "commit" && text.contains("nothing to commit") {
                    return None;
                }
                return Some(format!(
                    "快照自动同步 git 在 {label} 步失败：{}；hub 门禁可能读到旧快照，请手动 commit+push 数据仓库",
                    text.lines().next().unwrap_or("").chars().take(200).collect::<String>()
                ));
            }
        }
    }
    None
}

fn gate_pull_last_at() -> &'static std::sync::Mutex<Option<std::time::Instant>> {
    static LAST: std::sync::OnceLock<std::sync::Mutex<Option<std::time::Instant>>> =
        std::sync::OnceLock::new();
    LAST.get_or_init(|| std::sync::Mutex::new(None))
}

/// hub 门禁读取前节流 pull（docs 模式专用；worker 模式自己刚生成完，无需拉）。
/// 失败静默——退化为旧副本，由下一次调用或 15min autocommit timer 追平。
pub(crate) async fn freshen_git_copy_for_gate(req_dir: &Path) {
    freshen_git_copy_for_gate_with(req_dir, GATE_PULL_THROTTLE).await;
}

/// 带 可注入节流间隔的内部实现（测试传 Duration::ZERO 绕过全局节流）。
pub(crate) async fn freshen_git_copy_for_gate_with(req_dir: &Path, min_interval: Duration) {
    if std::env::var("PANEL_MODE").as_deref() == Ok("worker") {
        return;
    }
    {
        let mut last = gate_pull_last_at().lock().expect("gate pull throttle lock");
        if let Some(t) = *last {
            if t.elapsed() < min_interval {
                return;
            }
        }
        *last = Some(std::time::Instant::now());
    }
    let Some(root) = data_repo_root(req_dir) else {
        return;
    };
    let _ = run_git(
        &root,
        &[
            "pull".to_string(),
            "--rebase".to_string(),
            "--autostash".to_string(),
        ],
    )
    .await;
}

/// worker 侧 git 执行类 handler（review-materials / code-review / merge-* 等）前置节流
/// pull：hub 侧 API 写入（新需求目录、annotations 等）经远端同步到 worker 副本，
/// 避免「hub 创建的需求 worker 解析不到」（本次回归实测 500：全局池根缺失 + 滞后）。
/// 只在 worker 模式生效（docs 模式这些 handler 本体不被调用）；与门禁 pull 共享节流。
pub(crate) async fn freshen_scan_roots_for_git_exec(state: &AppState) {
    if std::env::var("PANEL_MODE").as_deref() == Ok("docs") {
        return;
    }
    {
        let mut last = gate_pull_last_at().lock().expect("gate pull throttle lock");
        if let Some(t) = *last {
            if t.elapsed() < GATE_PULL_THROTTLE {
                return;
            }
        }
        *last = Some(std::time::Instant::now());
    }
    let Ok(cfg) = crate::config::read_config(state).await else {
        return;
    };
    let mut roots: Vec<PathBuf> = Vec::new();
    for r in crate::config::normalize_scan_roots(cfg.requirement_scan_roots) {
        if let Some(root) = data_repo_root(Path::new(&r)) {
            if !roots.contains(&root) {
                roots.push(root);
            }
        }
    }
    for root in roots {
        let _ = run_git(
            &root,
            &[
                "pull".to_string(),
                "--rebase".to_string(),
                "--autostash".to_string(),
            ],
        )
        .await;
    }
}
