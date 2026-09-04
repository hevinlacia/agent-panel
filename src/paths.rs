//! Logical workspace root resolution — no hardcoded absolute paths.
//!
//! Resolution chain per logical name:
//!   1. environment variable (e.g. `WMS_WORKSPACE_ROOT`)
//!   2. `~/Developer/.agents/paths.env` literal `KEY=VALUE` lines
//!      (generated from `~/Developer/.agents/workspace-registry.toml` by
//!      `scripts/devpath.py --generate`)
//!   3. shallow discovery under `~/Developer/company` (`company/WMS`, then
//!      `company/*/WMS`, shallowest first)
//!
//! This keeps agent-panel working when the on-disk layout moves (e.g.
//! `company/WMS` -> `company/云仓/WMS`) without recompiling.

use std::fs;
use std::path::PathBuf;

use crate::util::home_dir;

fn from_paths_env(key: &str) -> Option<PathBuf> {
    let dev = home_dir().ok()?.join("Developer");
    let text = fs::read_to_string(dev.join(".agents").join("paths.env")).ok()?;
    let prefix = format!("{key}=");
    for line in text.lines() {
        let line = line.trim();
        if let Some(v) = line.strip_prefix(&prefix) {
            let v = v.trim().trim_matches('"');
            let p = PathBuf::from(v);
            if p.is_dir() {
                return Some(p);
            }
        }
    }
    None
}

fn discover(name: &str) -> Option<PathBuf> {
    let company = home_dir().ok()?.join("Developer").join("company");
    let mut hits: Vec<PathBuf> = Vec::new();
    let direct = company.join(name);
    if direct.is_dir() {
        hits.push(direct);
    }
    if let Ok(entries) = fs::read_dir(&company) {
        for e in entries.flatten() {
            let nested = e.path().join(name);
            if nested.is_dir() {
                hits.push(nested);
            }
        }
    }
    hits.sort_by_key(|p| p.components().count());
    hits.into_iter().next()
}

/// 解析 WMS 工作区根；全部失败时回退旧布局路径并打 stderr 警告。
pub fn wms_root() -> PathBuf {
    if let Ok(v) = std::env::var("WMS_WORKSPACE_ROOT") {
        let p = PathBuf::from(v);
        if p.is_dir() {
            return p;
        }
    }
    if let Some(p) = from_paths_env("WMS_WORKSPACE_ROOT") {
        return p;
    }
    if let Some(p) = discover("WMS") {
        return p;
    }
    eprintln!(
        "agent-panel: WMS root unresolved via env/paths.env/discovery; falling back to legacy layout"
    );
    home_dir()
        .unwrap_or_else(|_| PathBuf::from("/home/hevin"))
        .join("Developer")
        .join("company")
        .join("WMS")
}

/// WMS 项目本地 skills 目录（`<wms_root>/.agents/skills`）。
pub fn wms_skills_dir() -> PathBuf {
    wms_root().join(".agents").join("skills")
}

/// WMS testdata capability pack 根（`<wms_root>/.agents/testdata`）。
pub fn wms_testdata_pack_root() -> PathBuf {
    wms_root().join(".agents").join("testdata")
}
