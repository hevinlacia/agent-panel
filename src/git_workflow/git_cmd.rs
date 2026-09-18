use super::*;

pub(crate) async fn git(
    cwd: &Path,
    args: &[&str],
    timeout_ms: u64,
    max_output: usize,
) -> GitCommandResult {
    // 统一关闭 quotepath：非 ASCII 文件名输出原生 UTF-8，避免 numstat/name-status/diff
    // 产生 "\345\215\225..." 形式的引号+八进制转义路径，破坏前后端路径匹配。
    let mut full_args: Vec<&str> = Vec::with_capacity(args.len() + 2);
    full_args.push("-c");
    full_args.push("core.quotepath=false");
    full_args.extend_from_slice(args);
    let command = std::iter::once("git".to_string())
        .chain(full_args.iter().map(|a| shell_quote(a)))
        .collect::<Vec<_>>()
        .join(" ");
    let fut = Command::new("git")
        .args(&full_args)
        .current_dir(cwd)
        .output();
    match timeout(Duration::from_millis(timeout_ms), fut).await {
        Ok(Ok(output)) => {
            let (stdout, stdout_truncated) = limit_output(
                String::from_utf8_lossy(&output.stdout).to_string(),
                max_output,
            );
            let (stderr, stderr_truncated) = limit_output(
                String::from_utf8_lossy(&output.stderr).to_string(),
                max_output,
            );
            GitCommandResult {
                ok: output.status.success(),
                code: output.status.code(),
                command,
                stdout,
                stderr,
                output_truncated: stdout_truncated || stderr_truncated,
                timed_out: false,
            }
        }
        Ok(Err(err)) => GitCommandResult {
            ok: false,
            code: None,
            command,
            stdout: String::new(),
            stderr: err.to_string(),
            output_truncated: false,
            timed_out: false,
        },
        Err(_) => GitCommandResult {
            ok: false,
            code: None,
            command,
            stdout: String::new(),
            stderr: format!("timed out after {timeout_ms}ms"),
            output_truncated: false,
            timed_out: true,
        },
    }
}

pub(crate) fn limit_output(value: String, max: usize) -> (String, bool) {
    if value.len() <= max {
        return (value, false);
    }
    (value.chars().take(max).collect::<String>(), true)
}

pub(crate) fn compact(value: &str, max: usize) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.len() > max {
        Some(format!(
            "{}…",
            trimmed.chars().take(max).collect::<String>()
        ))
    } else {
        Some(trimmed.to_string())
    }
}

pub(crate) fn short_err(result: &GitCommandResult) -> String {
    compact(&result.stderr, 600)
        .or_else(|| compact(&result.stdout, 600))
        .unwrap_or_else(|| match result.code {
            Some(code) => format!("{} exited {code}", result.command),
            None if result.timed_out => format!("{} timed out", result.command),
            None => format!("{} failed", result.command),
        })
}
