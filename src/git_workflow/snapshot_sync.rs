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
    // 先解析符号链接：WMS 需求目录经 `.agents/req -> agent-panel-data/...` symlink 进入
    // 数据仓库；不解析的话向上找 .git 会先撞进 workspace-meta 仓库，add/commit 落错仓
    // （git 同时拒绝「路径规格位于符号链接之后」，双保险）。
    let real = std::fs::canonicalize(req_dir).ok()?;
    let mut cur = Some(real.as_path());
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
        return Some("git 备份同步跳过（需求目录不在 git 仓库内）：快照已直传 hub，门禁不受影响；数据仓备份需人工处理".to_string());
    };
    let stamp = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC");
    let msg = format!("review: sync snapshot for {req_id} ({stamp})");
    // pathspec 用仓库内相对路径：req_dir 可能含符号链接段（git 拒绝「位于符号链接
    // 之后」的绝对路径规格），canonicalize 后相对 root 必然可写。
    let real_req_dir = match std::fs::canonicalize(req_dir) {
        Ok(p) => p,
        Err(e) => {
            return Some(format!(
                "快照自动同步 git 失败：无法解析需求目录真实路径: {e}"
            ));
        }
    };
    let rel = match real_req_dir.strip_prefix(&root) {
        Ok(rel) if !rel.as_os_str().is_empty() => rel.to_string_lossy().to_string(),
        Ok(_) => ".".to_string(),
        Err(_) => {
            return Some(format!(
                "快照自动同步 git 失败：需求目录 {} 不在数据仓库 {} 内",
                real_req_dir.display(),
                root.display()
            ));
        }
    };
    let dir_arg = rel;
    let msg_arc = msg.clone();
    let dir_arg2 = dir_arg.clone();
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
                msg_arc.clone(),
                "--".to_string(),
                dir_arg2,
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
                    "git 备份同步在 {label} 步失败：{}（不影响门禁——快照已直传 hub；备份链路请人工 commit+push 数据仓）",
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
            // 只 pull 数据仓库：WMS 工作区等扫描根 canonicalize 后落在 workspace-meta，
            // 语义是「符号链接桥接的工作区」而非数据副本，pull 错仓。
            if roots.contains(&root) {
                continue;
            }
            if repo_remote_is_data(&root).await {
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

/// 快照直传 hub（活链路，不经过 git）：worker 生成快照后直接 POST 给 hub 落盘，
/// 门禁立即可见；git push/pull 只是备份兜底。失败返回 warning（不阻断）。
pub(crate) async fn deliver_snapshots_to_hub(req_dir: &Path, req_id: &str) -> Option<String> {
    let full_path = req_dir.join(CODE_REVIEW_FILE);
    let incremental_path = req_dir.join(CODE_REVIEW_INCREMENTAL_FILE);
    let full = match read_json_if_exists(&full_path).await {
        Some(v) => v,
        None => {
            return Some("快照文件不存在，无法直传 hub".to_string());
        }
    };
    let incremental = read_json_if_exists(&incremental_path).await;
    let hub = std::env::var("PANEL_HUB_URL").unwrap_or_default();
    if hub.is_empty() {
        // 无 hub 地址（纯本机模式）：数据仓即本仓，无需直传
        return None;
    }
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()
        .ok()?;
    let mut body = json!({
        "reqId": req_id,
        "kind": "full",
        "content": full,
    });
    if let Some(inc) = &incremental {
        body["incremental"] = json!({ "kind": "incremental", "content": inc });
    }
    let mut request = client
        .post(format!("{hub}/api/requirement/review-snapshot"))
        .json(&body);
    if let Ok(token) = std::env::var("PANEL_WORKER_TOKEN") {
        if !token.is_empty() {
            request = request.header("X-Worker-Token", token);
        }
    }
    match request.send().await {
        Ok(resp) if resp.status().is_success() => None,
        Ok(resp) => Some(format!(
            "快照直传 hub 失败（HTTP {}）：门禁可能读到旧快照；git 备份链路仍可用",
            resp.status().as_u16()
        )),
        Err(e) => Some(format!(
            "快照直传 hub 失败（{e}）：门禁可能读到旧快照；git 备份链路仍可用"
        )),
    }
}

/// 校验仓库 remote 归属 agent-panel-data（数据仓），防止把工作区仓（workspace-meta）误当数据副本。
async fn repo_remote_is_data(root: &Path) -> bool {
    matches!(
        run_git(
            root,
            &[
                "remote".to_string(),
                "get-url".to_string(),
                "origin".to_string(),
            ],
        )
        .await,
        Ok(out) if out.contains("agent-panel-data")
    )
}

/// worker 的数据仓根：扫描根里第一个 remote 归属 agent-panel-data 的仓库。
/// 找不到返回 None（无法物化 hub 侧需求目录）。
async fn worker_data_repo_root(state: &AppState) -> Option<PathBuf> {
    let cfg = crate::config::read_config(state).await.ok()?;
    let mut seen = Vec::new();
    for r in crate::config::normalize_scan_roots(cfg.requirement_scan_roots) {
        let Some(root) = data_repo_root(Path::new(&r)) else {
            continue;
        };
        if seen.contains(&root) {
            continue;
        }
        seen.push(root.clone());
        if repo_remote_is_data(&root).await {
            return Some(root);
        }
    }
    None
}

/// 活链路 fallback：本地解析不到需求（hub 侧新建、git 备份未同步）时，从 hub
/// 拉需求目录包（dir-bundle）物化到本地数据仓，返回物化后的需求目录。
/// 只在 worker 模式调用方使用；失败返回 Err（调用方回落原有 not found 错误）。
pub(crate) async fn fetch_requirement_bundle_from_hub(
    state: &AppState,
    req_id: &str,
) -> anyhow::Result<PathBuf> {
    let hub = std::env::var("PANEL_HUB_URL")
        .ok()
        .filter(|v| !v.is_empty())
        .ok_or_else(|| anyhow!("PANEL_HUB_URL 未配置，无法从 hub 拉取需求目录"))?;
    let root = worker_data_repo_root(state)
        .await
        .ok_or_else(|| anyhow!("未找到 agent-panel-data 数据仓，无法物化需求目录"))?;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()?;
    let mut request = client.get(format!("{hub}/api/requirement/dir-bundle?id={req_id}"));
    if let Ok(token) = std::env::var("PANEL_WORKER_TOKEN") {
        if !token.is_empty() {
            request = request.header("X-Worker-Token", token);
        }
    }
    let resp = request.send().await?;
    if !resp.status().is_success() {
        return Err(anyhow!("hub dir-bundle 返回 HTTP {}", resp.status()));
    }
    let body: Value = resp.json().await?;
    let rel_dir = body
        .get("relDir")
        .and_then(Value::as_str)
        .filter(|v| !v.is_empty() && !v.starts_with('/') && !v.contains(".."))
        .ok_or_else(|| anyhow!("hub dir-bundle 缺少合法 relDir"))?;
    let files = body
        .get("files")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow!("hub dir-bundle 缺少 files"))?;
    let target = root.join(rel_dir);
    for file in files {
        let Some(rel_path) = file.get("path").and_then(Value::as_str) else {
            continue;
        };
        if rel_path.starts_with('/') || rel_path.contains("..") || rel_path.contains('\\') {
            continue;
        }
        let Some(content) = file.get("content").and_then(Value::as_str) else {
            continue;
        };
        let dest = target.join(rel_path);
        if let Some(parent) = dest.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        atomic_write_text(&dest, content).await?;
    }
    Ok(target)
}
