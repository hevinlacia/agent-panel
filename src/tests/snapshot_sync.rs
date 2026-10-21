use super::*;
use std::process::Command as StdCommand;
use std::time::Duration;

fn git(dir: &std::path::Path, args: &[&str]) -> String {
    let out = StdCommand::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "test")
        .env("GIT_AUTHOR_EMAIL", "test@localhost")
        .env("GIT_COMMITTER_NAME", "test")
        .env("GIT_COMMITTER_EMAIL", "test@localhost")
        .output()
        .expect("git run");
    assert!(
        out.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&out.stderr)
    );
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

/// INC-149 死结闭环回归：worker 副本生成快照 → push_snapshot_commit 自动 push →
/// hub 副本 freshen_git_copy_for_gate pull 后能看到新材料。
#[tokio::test]
async fn snapshot_push_then_gate_pull_roundtrip() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let remote = tmp.path().join("remote.git");
    let worker = tmp.path().join("worker");
    let hub = tmp.path().join("hub");
    // 裸远端 + worker 仓库（带 upstream）+ hub clone
    git(
        tmp.path(),
        &["init", "--bare", "-b", "main", remote.to_str().unwrap()],
    );
    std::fs::create_dir_all(&worker).expect("mkdir worker");
    git(&worker, &["init", "-b", "main"]);
    git(
        &worker,
        &["remote", "add", "origin", remote.to_str().unwrap()],
    );
    std::fs::write(worker.join("seed.txt"), "seed");
    git(&worker, &["add", "-A"]);
    git(&worker, &["commit", "-m", "seed"]);
    git(&worker, &["push", "-u", "origin", "main"]);
    git(
        tmp.path(),
        &["clone", remote.to_str().unwrap(), hub.to_str().unwrap()],
    );

    // worker 侧：需求目录生成快照（模拟 prepare_review_materials 落盘）
    let req_dir = worker.join("company/云仓/WMS/.agents/req/X-1");
    std::fs::create_dir_all(&req_dir).expect("mkdir req");
    std::fs::write(
        req_dir.join("code-review.json"),
        r#"{"targetCommit":"abc123"}"#,
    )
    .expect("write snapshot");
    let warning = push_snapshot_commit(&req_dir, "X-1").await;
    assert!(warning.is_none(), "push should succeed: {warning:?}");

    // hub 侧：pull 前没有该文件；节流置零强制拉取后可见
    let hub_snapshot = hub.join("company/云仓/WMS/.agents/req/X-1/code-review.json");
    assert!(
        !hub_snapshot.exists(),
        "hub copy should be stale before pull"
    );
    freshen_git_copy_for_gate_with(&hub, Duration::ZERO).await;
    let content = std::fs::read_to_string(&hub_snapshot).expect("snapshot pulled to hub copy");
    assert!(content.contains("abc123"));

    // 二次变更 → push 无警告 → pull 再追平（验证可重复同步）
    std::fs::write(
        req_dir.join("code-review.json"),
        r#"{"targetCommit":"def456"}"#,
    )
    .expect("update snapshot");
    assert!(push_snapshot_commit(&req_dir, "X-1").await.is_none());
    freshen_git_copy_for_gate_with(&hub, Duration::ZERO).await;
    assert!(std::fs::read_to_string(&hub_snapshot)
        .expect("reread")
        .contains("def456"));
}

#[tokio::test]
async fn snapshot_push_reports_warning_outside_git_repo() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let req_dir = tmp.path().join("plain/req/X-2");
    std::fs::create_dir_all(&req_dir).expect("mkdir");
    let warning = push_snapshot_commit(&req_dir, "X-2").await;
    let text = warning.expect("warning expected outside git repo");
    assert!(text.contains("不在 git 仓库"), "unexpected warning: {text}");
}

#[test]
fn data_repo_root_walks_up_to_git_dir() {
    let tmp = tempfile::tempdir().expect("tempdir");
    std::fs::create_dir_all(tmp.path().join(".git")).expect("mkdir .git");
    let deep = tmp.path().join("a/b/req/X");
    std::fs::create_dir_all(&deep).expect("mkdir deep");
    let root = data_repo_root(&deep).expect("root found");
    assert_eq!(root, tmp.path());
    // 无 .git 祖先 → None
    let plain = tempfile::tempdir().expect("tempdir2");
    assert!(data_repo_root(&plain.path().join("x/y")).is_none());
}
