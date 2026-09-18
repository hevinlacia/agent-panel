use std::{
    collections::HashMap,
    env,
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::{anyhow, Result};
use regex::Regex;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::{process::Command, time::timeout};

use crate::*;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CodeReviewForm {
    pub(crate) req_id: String,
    #[serde(default)]
    pub(crate) base_ref: Option<String>,
    /// 分支登记轮次：1 = 原始 branches.json（默认），>=2 = 修复轮次文件
    /// branches-round-<n>.json。省略时按 1 处理，旧行为完全不变。
    #[serde(default)]
    pub(crate) round: Option<u32>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SyncBaseForm {
    pub(crate) req_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProdMrForm {
    pub(crate) req_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MergeBranchForm {
    pub(crate) req_id: String,
    #[serde(default)]
    pub(crate) target: Option<String>,
    #[serde(default)]
    pub(crate) target_branch: Option<String>,
    #[serde(default)]
    pub(crate) repo_kind: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BranchScope {
    #[serde(default)]
    pub(crate) version: i64,
    #[serde(default)]
    pub(crate) updated_at: i64,
    #[serde(default)]
    pub(crate) repos: Vec<BranchRepo>,
    #[serde(default)]
    pub(crate) fallback: bool,
    /// 分支登记轮次：1 = 原始需求分支（branches.json），>=2 = 合入生产后的修复轮次
    /// （branches-round-<n>.json）。旧文件无此字段时按 1 处理。
    #[serde(default)]
    pub(crate) round: u32,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BranchRepo {
    #[serde(default)]
    pub(crate) repo_name: String,
    #[serde(default)]
    pub(crate) branches: Vec<String>,
    #[serde(default)]
    pub(crate) role: Option<String>,
    #[serde(default, alias = "projectPath")]
    pub(crate) path: Option<String>,
    #[serde(default)]
    pub(crate) base_ref: Option<String>,
    #[serde(default)]
    pub(crate) test_target_branch: Option<String>,
    #[serde(default)]
    pub(crate) uat_target_branch: Option<String>,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CodeReviewFileStat {
    pub(crate) path: String,
    pub(crate) status: String,
    pub(crate) additions: i64,
    pub(crate) deletions: i64,
    pub(crate) risk_tags: Vec<String>,
}

#[derive(Debug)]
pub(crate) struct GitCommandResult {
    pub(crate) ok: bool,
    pub(crate) code: Option<i32>,
    pub(crate) command: String,
    pub(crate) stdout: String,
    pub(crate) stderr: String,
    pub(crate) output_truncated: bool,
    pub(crate) timed_out: bool,
}

#[derive(Debug)]
pub(crate) struct BaseRefInfo {
    pub(crate) base_ref: String,
    pub(crate) remote: String,
    pub(crate) remote_branch: String,
    pub(crate) local_branch: String,
}

mod branch_scope;
mod code_review;
mod git_cmd;
mod merge_exec;
mod merge_options;
mod prod_mr;
mod scan;
mod sync_base;

pub(crate) use branch_scope::*;
pub(crate) use code_review::*;
pub(crate) use git_cmd::*;
pub(crate) use merge_exec::*;
pub(crate) use merge_options::*;
pub(crate) use prod_mr::*;
pub(crate) use scan::*;
pub(crate) use sync_base::*;
