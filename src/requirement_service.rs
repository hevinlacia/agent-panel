use std::{
    collections::{HashMap, HashSet},
    env,
    path::{Path, PathBuf},
};

use anyhow::{anyhow, Result};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::fs;
use uuid::Uuid;

use crate::*;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StatusForm {
    pub(crate) req_id: String,
    pub(crate) status: String,
    #[serde(default)]
    pub(crate) note: Option<String>,
    /// 调用方标识："ui" = 人在 Panel 界面上修改，跳过状态门禁；不传或其它值 = agent/API 推进，强制校验门禁。
    #[serde(default)]
    pub(crate) via: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CategoryForm {
    pub(crate) req_id: String,
    pub(crate) category: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ConvertIssueForm {
    pub(crate) req_id: String,
    #[serde(default)]
    pub(crate) note: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct OnesForm {
    pub(crate) req_id: String,
    pub(crate) ones: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AssociateForm {
    pub(crate) req_id: String,
    pub(crate) session_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct NewSessionForm {
    pub(crate) req_id: String,
    /// Force-generate a fresh session id, discarding the pending command even
    /// if its session was never used (recovery hatch for "used" misdetection).
    #[serde(default)]
    pub(crate) force: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AnnotationsSaveForm {
    pub(crate) req_id: String,
    /// Full code-annotations document; missing fields are filled in by the handler.
    #[serde(default)]
    pub(crate) annotations: Option<Value>,
}

/// 引用式需求组创建入参的单个成员（reqId 必填，note 可选）。
#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GroupMemberInput {
    pub(crate) req_id: String,
    #[serde(default)]
    pub(crate) note: Option<String>,
}

/// 创建子需求入参：从父需求拆出并行执行单元。ID 由服务端分配（`<父票号>-S<n>[-slug]`），
/// 目录平铺；复制父需求 background/technical-plan/impact/test 文档作快照起点；
/// 不绑 ONES/plan-release/issues（由父需求承载），不建分支（P2 一键初始化）。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SubRequirementCreateForm {
    pub(crate) parent_req_id: String,
    pub(crate) title: String,
    /// slug 追加在 `-S<n>` 之后（ASCII 路径安全字符）；缺省时 ID 为 `<父票号>-S<n>`。
    #[serde(default)]
    pub(crate) slug: Option<String>,
    /// 覆盖 owner，默认继承父需求。
    #[serde(default)]
    pub(crate) owner: Option<String>,
    /// 子需求 scope 摘要（写入 meta Summary）。
    #[serde(default)]
    pub(crate) summary: Option<String>,
    #[serde(default)]
    pub(crate) dry_run: Option<bool>,
}

/// 创建文档分册入参：明细写入 `docs/<doc-base>/<NNN>-<slug>.md`，
/// 主文档末尾自动追加索引行（主文档索引化，避免单文件无限膨胀）。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DocPartCreateForm {
    pub(crate) req_id: String,
    /// 基础文档 docType（notes / technical-plan / background / test 等，见 requirement_doc_file）。
    pub(crate) doc_type: String,
    /// 分册 slug（ASCII 路径安全），文件名为 `<NNN>-<slug>.md`。
    pub(crate) slug: String,
    /// 分册标题（缺 H1 时用作标题行；缺省写入索引行的摘要兜底）。
    #[serde(default)]
    pub(crate) title: Option<String>,
    /// 一句话摘要（写入主文档索引行，agent ctx 借此感知分册内容）。
    #[serde(default)]
    pub(crate) summary: Option<String>,
    pub(crate) content: String,
    #[serde(default)]
    pub(crate) dry_run: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RequirementCreateForm {
    pub(crate) req_id: String,
    pub(crate) title: String,
    #[serde(default)]
    pub(crate) project: Option<String>,
    #[serde(default)]
    pub(crate) projects: Option<Vec<String>>,
    #[serde(default)]
    pub(crate) group_path: Option<Vec<String>>,
    #[serde(default)]
    pub(crate) parent_req_id: Option<String>,
    #[serde(default)]
    pub(crate) root: Option<String>,
    #[serde(default)]
    pub(crate) status: Option<String>,
    #[serde(default)]
    pub(crate) category: Option<String>,
    /// 需求推动方：产品推动（默认）/ 开发推动。
    pub(crate) source: Option<String>,
    #[serde(default)]
    pub(crate) owner: Option<String>,
    #[serde(default)]
    pub(crate) start_date: Option<String>,
    #[serde(default)]
    pub(crate) plan_release: Option<String>,
    #[serde(default)]
    pub(crate) ones: Option<String>,
    /// 创建时绑定的线上问题 req id 列表（仅 category=需求 时有意义）。
    pub(crate) issues: Option<Vec<String>>,
    /// 引用式需求组成员（reqId + 可选 note）；members 非空 = 创建需求组，
    /// 成员必须是已存在且非组的需求，不支持嵌套。
    pub(crate) members: Option<Vec<GroupMemberInput>>,
    /// 组发布策略：independent（默认）/ together（整体发布）。
    pub(crate) release_policy: Option<String>,
    #[serde(default)]
    pub(crate) summary: Option<String>,
    #[serde(default)]
    pub(crate) background: Option<String>,
    #[serde(default)]
    pub(crate) notes: Option<String>,
    #[serde(default)]
    pub(crate) dry_run: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RequirementPatchForm {
    pub(crate) req_id: String,
    #[serde(default)]
    pub(crate) title: Option<String>,
    #[serde(default)]
    pub(crate) project: Option<String>,
    #[serde(default)]
    pub(crate) projects: Option<Vec<String>>,
    #[serde(default)]
    pub(crate) status: Option<String>,
    #[serde(default)]
    pub(crate) category: Option<String>,
    #[serde(default)]
    pub(crate) source: Option<String>,
    #[serde(default)]
    pub(crate) owner: Option<String>,
    #[serde(default)]
    pub(crate) start_date: Option<String>,
    #[serde(default)]
    pub(crate) plan_release: Option<String>,
    #[serde(default)]
    pub(crate) ones: Option<String>,
    /// 绑定的线上问题 req id 列表；空列表表示清空绑定。
    pub(crate) issues: Option<Vec<String>>,
    #[serde(default)]
    pub(crate) note: Option<String>,
    #[serde(default)]
    pub(crate) dry_run: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RequirementNoteForm {
    pub(crate) req_id: String,
    pub(crate) text: String,
    #[serde(default)]
    pub(crate) title: Option<String>,
    #[serde(default)]
    pub(crate) session_id: Option<String>,
    #[serde(default)]
    pub(crate) dry_run: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RequirementDocForm {
    pub(crate) req_id: String,
    pub(crate) doc_type: String,
    pub(crate) content: String,
    #[serde(default)]
    pub(crate) mode: Option<String>,
    #[serde(default)]
    pub(crate) dry_run: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RequirementValidateForm {
    pub(crate) req_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RequirementEditForm {
    pub(crate) req_id: String,
    pub(crate) operation: String,
    #[serde(default)]
    pub(crate) token: Option<String>,
    #[serde(default)]
    pub(crate) doc_type: Option<String>,
    #[serde(default)]
    pub(crate) content: Option<String>,
    #[serde(default)]
    pub(crate) text: Option<String>,
    #[serde(default)]
    pub(crate) title: Option<String>,
    #[serde(default)]
    pub(crate) heading: Option<String>,
    #[serde(default)]
    pub(crate) mode: Option<String>,
    #[serde(default)]
    pub(crate) status: Option<String>,
    #[serde(default)]
    pub(crate) category: Option<String>,
    #[serde(default)]
    pub(crate) note: Option<String>,
    #[serde(default)]
    pub(crate) session_id: Option<String>,
    #[serde(default)]
    pub(crate) fields: Option<HashMap<String, String>>,
    #[serde(default)]
    pub(crate) dry_run: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RequirementSectionForm {
    pub(crate) req_id: String,
    pub(crate) content: String,
    #[serde(default)]
    pub(crate) token: Option<String>,
    #[serde(default)]
    pub(crate) doc_type: Option<String>,
    #[serde(default)]
    pub(crate) heading: Option<String>,
    #[serde(default)]
    pub(crate) dry_run: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RequirementEventForm {
    pub(crate) req_id: String,
    #[serde(default, alias = "type")]
    pub(crate) event_type: Option<String>,
    #[serde(default)]
    pub(crate) title: Option<String>,
    #[serde(default)]
    pub(crate) summary: Option<String>,
    #[serde(default)]
    pub(crate) details: Option<String>,
    #[serde(default)]
    pub(crate) evidence: Vec<String>,
    #[serde(default)]
    pub(crate) decisions: Vec<String>,
    #[serde(default)]
    pub(crate) todos: Vec<String>,
    #[serde(default)]
    pub(crate) related_files: Vec<String>,
    #[serde(default)]
    pub(crate) related_knowledge_ids: Vec<String>,
    #[serde(default)]
    pub(crate) trigger_terms: Vec<String>,
    #[serde(default)]
    pub(crate) related_repos: Vec<String>,
    #[serde(default)]
    pub(crate) related_tables: Vec<String>,
    #[serde(default)]
    pub(crate) related_apis: Vec<String>,
    #[serde(default)]
    pub(crate) candidate_type: Option<String>,
    #[serde(default)]
    pub(crate) dedupe_key: Option<String>,
    #[serde(default)]
    pub(crate) confidence: Option<String>,
    #[serde(default)]
    pub(crate) target: Option<String>,
    #[serde(default)]
    pub(crate) test_cases: Vec<RequirementEventTestCase>,
    #[serde(default)]
    pub(crate) status: Option<String>,
    #[serde(default)]
    pub(crate) risk_level: Option<String>,
    #[serde(default)]
    pub(crate) tags: Vec<String>,
    #[serde(default)]
    pub(crate) session_id: Option<String>,
    #[serde(default)]
    pub(crate) idempotency_key: Option<String>,
    #[serde(default)]
    pub(crate) append_note: Option<bool>,
    #[serde(default)]
    pub(crate) dry_run: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RequirementEventTestCase {
    #[serde(default)]
    pub(crate) name: String,
    #[serde(default)]
    pub(crate) result: String,
    #[serde(default)]
    pub(crate) evidence: Option<String>,
}

mod create;
mod doc;
mod doc_parts;
mod events;
mod id_pool;
mod paths;
mod phase_prompt;
mod selftest_gate;
mod state;
mod status_gates;
mod templates;
mod validate;

pub(crate) use create::*;
pub(crate) use doc::*;
pub(crate) use doc_parts::*;
pub(crate) use events::*;
pub(crate) use id_pool::*;
pub(crate) use paths::*;
pub(crate) use phase_prompt::*;
pub(crate) use selftest_gate::*;
pub(crate) use state::*;
pub(crate) use status_gates::*;
pub(crate) use templates::*;
pub(crate) use validate::*;
