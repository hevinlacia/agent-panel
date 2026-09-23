use super::*;
use std::path::{Path, PathBuf};

// 按业务域拆分的测试子模块（verbatim 移动，不改断言）。
mod status_gates;
mod id_seq;
mod groups;
mod experience_summary;
mod docs_render;
mod review_flow;
mod merge_options;
mod branch_registration;
mod sessions;
mod sub_requirement;
mod misc;

fn chrono_like_unique_suffix() -> u128 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0)
}


fn default_requirement_for_test(req_id: &str) -> Requirement {
    let mut req = default_requirement(Vec::new());
    req.id = req_id.to_string();
    req
}


fn write_file(path: &Path, content: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create parent dirs");
    }
    std::fs::write(path, content).expect("write file");
}


fn temp_app_state(data: &Path, pi_root: &Path, dsh_root: &Path) -> AppState {
    AppState {
        project_root: Arc::new(data.to_path_buf()),
        data_dir: Arc::new(data.to_path_buf()),
        pi_session_root: Arc::new(pi_root.to_path_buf()),
        dsh_session_root: Arc::new(dsh_root.to_path_buf()),
        cainiao_mock: Arc::new(Mutex::new(None)),
        experience_summary_dispatch: Arc::new(Mutex::new(())),
        requirement_create_lock: Arc::new(Mutex::new(())),
    }
}
