use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    time::SystemTime,
};

use anyhow::Result;
use axum::{
    extract::{Query, State},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::fs;
use walkdir::WalkDir;

use crate::markdown::{is_cjk, markdown_outline, markdown_section, yaml_quote, Frontmatter};
use crate::{
    atomic_write_text, clean_optional, clean_required, clean_required_opt, ensure_text_size,
    home_dir, normalize_path_string, normalize_scan_roots, normalize_user_path, now_ms,
    read_config, same_or_child_path, truncate_chars, unique_strings, ApiError, ApiResult, AppState,
    IdQuery, BUSINESS_KNOWLEDGE_DIR, EXPERIENCES_DIR, KNOWLEDGE_DEFAULT_CONFIDENCE,
    KNOWLEDGE_DEFAULT_STATUS,
};

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct KnowledgeAgentQuery {
    #[serde(default)]
    kind: Option<String>,
    #[serde(default)]
    intent: Option<String>,
    #[serde(default)]
    query: Option<String>,
    #[serde(default)]
    domain: Option<String>,
    #[serde(default)]
    project: Option<String>,
    #[serde(default)]
    scope: Option<String>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    limit: Option<usize>,
    #[serde(default)]
    tokens_budget: Option<usize>,
    #[serde(default)]
    budget: Option<usize>,
    #[serde(default)]
    include_outline: Option<bool>,
    #[serde(default)]
    outline: Option<bool>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct KnowledgeWriteForm {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    kind: Option<String>,
    title: String,
    #[serde(default)]
    domain: Option<String>,
    #[serde(default)]
    project: Option<String>,
    #[serde(default)]
    scope: Option<String>,
    #[serde(default)]
    root: Option<String>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    category: Option<String>,
    #[serde(default)]
    confidence: Option<String>,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    trigger_terms: Vec<String>,
    #[serde(default)]
    related_skills: Vec<String>,
    #[serde(default)]
    related_repos: Vec<String>,
    #[serde(default)]
    related_tables: Vec<String>,
    #[serde(default)]
    related_apis: Vec<String>,
    #[serde(default)]
    source: Option<String>,
    #[serde(default)]
    origin_path: Option<String>,
    #[serde(default)]
    summary: Option<String>,
    #[serde(default)]
    details: Option<String>,
    #[serde(default)]
    last_verified_at: Option<String>,
    #[serde(default)]
    valid_until: Option<String>,
    #[serde(default)]
    dry_run: Option<bool>,
}

#[derive(Debug, Default)]
struct KnowledgeQueryFilter {
    kind: Option<String>,
    query: Option<String>,
    domain: Option<String>,
    project: Option<String>,
    scope: Option<String>,
    status: Option<String>,
    limit: Option<usize>,
    include_full: bool,
    include_outline: bool,
    budget: Option<usize>,
}

#[derive(Debug, Clone)]
struct KnowledgePath {
    path: PathBuf,
    kind: String,
    scope: String,
    root: PathBuf,
}

pub(crate) async fn api_knowledge_list(
    State(state): State<AppState>,
    Query(query): Query<IdQuery>,
) -> ApiResult<Json<Value>> {
    let filter = KnowledgeQueryFilter {
        kind: normalize_knowledge_kind(query.kind.as_deref()).ok(),
        query: clean_optional(query.q.as_deref())
            .or_else(|| clean_optional(query.intent.as_deref())),
        domain: clean_optional(query.domain.as_deref()),
        project: clean_optional(query.project.as_deref()),
        scope: clean_optional(query.scope.as_deref()),
        status: clean_optional(query.status.as_deref()),
        limit: query.limit,
        include_full: query.include_full.unwrap_or(false),
        include_outline: true,
        budget: query.budget,
    };
    let items = query_knowledge_items(&state, &filter).await?;
    Ok(Json(json!({ "items": items, "generatedAt": now_ms() })))
}

pub(crate) async fn api_knowledge_item(
    State(state): State<AppState>,
    Query(query): Query<IdQuery>,
) -> ApiResult<Json<Value>> {
    let id = clean_required_opt(query.id.as_deref(), "id")?;
    let item = get_knowledge_item(&state, &id, true, query.budget, query.section.as_deref())
        .await?
        .ok_or_else(|| ApiError::bad_request(format!("knowledge item not found: {id}")))?;
    Ok(Json(json!({ "item": item })))
}

pub(crate) async fn api_agent_knowledge_query(
    State(state): State<AppState>,
    Json(payload): Json<KnowledgeAgentQuery>,
) -> ApiResult<Json<Value>> {
    let budget = payload.tokens_budget.or(payload.budget).or(Some(2200));
    let filter = KnowledgeQueryFilter {
        kind: normalize_knowledge_kind(payload.kind.as_deref()).ok(),
        query: clean_optional(payload.intent.as_deref())
            .or_else(|| clean_optional(payload.query.as_deref())),
        domain: clean_optional(payload.domain.as_deref()),
        project: clean_optional(payload.project.as_deref()),
        scope: clean_optional(payload.scope.as_deref()),
        status: clean_optional(payload.status.as_deref()),
        limit: payload
            .limit
            .or_else(|| default_knowledge_limit_for_budget(budget)),
        include_full: false,
        include_outline: payload.include_outline.or(payload.outline).unwrap_or(true),
        budget,
    };
    let items = query_knowledge_items(&state, &filter).await?;
    Ok(Json(json!({
        "results": items,
        "generatedAt": now_ms(),
        "usage": {
            "summary": "Query returns budgeted summaries, outline, score, whyMatched, matchedFields, and matches. Use ids with GET /api/agent/items/full?id=<id> only when more detail is required; use section=<heading> to fetch one Markdown section.",
            "fullItemEndpoint": "/api/agent/items/full?id=<id>",
            "sectionEndpoint": "/api/agent/items/full?id=<id>&section=<heading>",
            "summaryEndpoint": "/api/agent/items/summary?id=<id>",
            "tokensBudget": filter.budget
        }
    })))
}

pub(crate) async fn api_agent_item_summary(
    State(state): State<AppState>,
    Query(query): Query<IdQuery>,
) -> ApiResult<Json<Value>> {
    let id = clean_required_opt(query.id.as_deref(), "id")?;
    let item = get_knowledge_item(&state, &id, false, query.budget, None)
        .await?
        .ok_or_else(|| ApiError::bad_request(format!("knowledge item not found: {id}")))?;
    Ok(Json(json!({ "item": item })))
}

pub(crate) async fn api_agent_item_full(
    State(state): State<AppState>,
    Query(query): Query<IdQuery>,
) -> ApiResult<Json<Value>> {
    let id = clean_required_opt(query.id.as_deref(), "id")?;
    let item = get_knowledge_item(&state, &id, true, query.budget, query.section.as_deref())
        .await?
        .ok_or_else(|| ApiError::bad_request(format!("knowledge item not found: {id}")))?;
    Ok(Json(json!({ "item": item })))
}

pub(crate) async fn api_knowledge_save(
    State(state): State<AppState>,
    Json(payload): Json<KnowledgeWriteForm>,
) -> ApiResult<Json<Value>> {
    let item = save_knowledge_item(&state, payload).await?;
    Ok(Json(json!({ "ok": true, "item": item })))
}

fn all_knowledge_kinds() -> Vec<String> {
    vec!["businessKnowledge".to_string(), "experience".to_string()]
}

fn normalize_knowledge_kind(raw: Option<&str>) -> ApiResult<String> {
    let value = raw.unwrap_or("").trim().to_ascii_lowercase();
    match value.as_str() {
        "businessknowledge" | "business-knowledge" | "business_knowledge" | "business" | "biz"
        | "knowledge" | "业务知识" => Ok("businessKnowledge".to_string()),
        "experience" | "experiences" | "exp" | "经验" => Ok("experience".to_string()),
        "" => Err(ApiError::bad_request("missing knowledge kind")),
        _ => Err(ApiError::bad_request(format!(
            "unknown knowledge kind: {value}"
        ))),
    }
}

fn knowledge_kind_dir(kind: &str) -> &'static str {
    match kind {
        "businessKnowledge" => BUSINESS_KNOWLEDGE_DIR,
        "experience" => EXPERIENCES_DIR,
        _ => EXPERIENCES_DIR,
    }
}

fn knowledge_type_name(kind: &str) -> &'static str {
    match kind {
        "businessKnowledge" => "business_knowledge",
        "experience" => "experience",
        _ => "experience",
    }
}

async fn knowledge_storage_roots(
    state: &AppState,
    kind_filter: Option<&str>,
) -> Result<Vec<KnowledgePath>> {
    let home = home_dir()?;
    let mut bases: Vec<(String, PathBuf)> = vec![("global".to_string(), home.join(".agents"))];
    bases.push(("project".to_string(), state.project_root.as_ref().clone()));
    let cfg = read_config(state).await.unwrap_or_default();
    for root in normalize_scan_roots(cfg.requirement_scan_roots) {
        bases.push((
            "project".to_string(),
            normalize_knowledge_base(PathBuf::from(root)),
        ));
    }

    let kinds = kind_filter
        .map(|k| vec![k.to_string()])
        .unwrap_or_else(all_knowledge_kinds);
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for (scope, base) in bases {
        for kind in &kinds {
            let dir_name = knowledge_kind_dir(kind);
            let dir = if base.file_name().and_then(|v| v.to_str()) == Some(dir_name) {
                base.clone()
            } else if base.file_name().and_then(|v| v.to_str()) == Some(".agents") {
                base.join(dir_name)
            } else {
                base.join(".agents").join(dir_name)
            };
            let key = normalize_path_string(&dir);
            if seen.insert(format!("{kind}:{key}")) {
                out.push(KnowledgePath {
                    path: dir,
                    kind: kind.clone(),
                    scope: scope.clone(),
                    root: base.clone(),
                });
            }
        }
    }
    Ok(out)
}

fn normalize_knowledge_base(path: PathBuf) -> PathBuf {
    if path.file_name().and_then(|v| v.to_str()) == Some("req") {
        if path
            .parent()
            .and_then(|p| p.file_name())
            .and_then(|v| v.to_str())
            == Some(".agents")
        {
            return path
                .parent()
                .and_then(|p| p.parent())
                .map(Path::to_path_buf)
                .unwrap_or(path);
        }
    }
    if matches!(
        path.file_name().and_then(|v| v.to_str()),
        Some(BUSINESS_KNOWLEDGE_DIR) | Some(EXPERIENCES_DIR)
    ) {
        if let Some(parent) = path.parent().and_then(|p| p.parent()) {
            return parent.to_path_buf();
        }
    }
    path
}

async fn query_knowledge_items(
    state: &AppState,
    filter: &KnowledgeQueryFilter,
) -> ApiResult<Vec<Value>> {
    let roots = knowledge_storage_roots(state, filter.kind.as_deref()).await?;
    let budget = filter.budget.unwrap_or(2200).clamp(600, 60_000);
    let query_has_text = filter
        .query
        .as_deref()
        .map(|q| !q.trim().is_empty())
        .unwrap_or(false);
    let requested_limit = filter
        .limit
        .unwrap_or_else(|| default_knowledge_limit_for_budget(Some(budget)).unwrap_or(5));
    let effective_limit = requested_limit.clamp(1, max_knowledge_limit_for_budget(budget));
    let mut rows: Vec<(i64, String, Value)> = Vec::new();
    for root in roots {
        let meta_dir = root.path.join("meta");
        if !meta_dir.is_dir() {
            continue;
        }
        for entry in WalkDir::new(&meta_dir)
            .min_depth(1)
            .max_depth(1)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let path = entry.path();
            if !is_knowledge_meta_file(path) {
                continue;
            }
            let mut item = read_knowledge_item_file(
                &root,
                path,
                filter.include_full,
                filter.budget,
                None,
                filter.include_outline,
            )
            .await?;
            if !knowledge_item_matches(&item, filter) {
                continue;
            }
            let search = knowledge_search_score(&item, filter.query.as_deref());
            if query_has_text && search.score == 0 {
                continue;
            }
            apply_query_budget_to_item(&mut item, budget, effective_limit, filter.include_outline);
            if let Some(obj) = item.as_object_mut() {
                obj.insert("score".to_string(), json!(search.score));
                obj.insert("whyMatched".to_string(), json!(search.why_matched));
                obj.insert("matchedFields".to_string(), json!(search.matched_fields));
                obj.insert("matches".to_string(), json!(search.matches));
                obj.insert("queryTokens".to_string(), json!(search.query_tokens));
            }
            let updated = item
                .get("updatedAt")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            rows.push((search.score, updated, item));
        }
    }
    rows.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| b.1.cmp(&a.1)));
    Ok(rows
        .into_iter()
        .take(effective_limit)
        .map(|(_, _, item)| item)
        .collect())
}

fn knowledge_item_matches(item: &Value, filter: &KnowledgeQueryFilter) -> bool {
    value_filter_matches(item, "domain", filter.domain.as_deref())
        && value_filter_matches(item, "project", filter.project.as_deref())
        && value_filter_matches(item, "scope", filter.scope.as_deref())
        && value_filter_matches(item, "status", filter.status.as_deref())
}

fn is_knowledge_meta_file(path: &Path) -> bool {
    path.is_file()
        && path_has_component(path, "meta")
        && matches!(
            path.extension().and_then(|v| v.to_str()),
            Some("yaml") | Some("yml")
        )
}

fn path_has_component(path: &Path, name: &str) -> bool {
    path.components()
        .any(|c| c.as_os_str().to_string_lossy() == name)
}

fn value_filter_matches(item: &Value, key: &str, filter: Option<&str>) -> bool {
    let Some(filter) = filter.map(str::trim).filter(|v| !v.is_empty()) else {
        return true;
    };
    let value = item.get(key).and_then(Value::as_str).unwrap_or_default();
    value
        .to_ascii_lowercase()
        .contains(&filter.to_ascii_lowercase())
}

async fn get_knowledge_item(
    state: &AppState,
    id: &str,
    include_full: bool,
    budget: Option<usize>,
    section: Option<&str>,
) -> ApiResult<Option<Value>> {
    let Some((root, path, _)) = find_knowledge_file_by_id(state, id).await? else {
        return Ok(None);
    };
    read_knowledge_item_file(
        &root,
        &path,
        include_full,
        budget.or(Some(20_000)),
        section,
        true,
    )
    .await
    .map(Some)
}

async fn find_knowledge_file_by_id(
    state: &AppState,
    id: &str,
) -> ApiResult<Option<(KnowledgePath, PathBuf, String)>> {
    let target = id.trim();
    if target.is_empty() {
        return Ok(None);
    }
    for root in knowledge_storage_roots(state, None).await? {
        let meta_dir = root.path.join("meta");
        if !meta_dir.is_dir() {
            continue;
        }
        for entry in WalkDir::new(&meta_dir)
            .min_depth(1)
            .max_depth(1)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let path = entry.path();
            if !is_knowledge_meta_file(path) {
                continue;
            }
            let raw = fs::read_to_string(path).await.unwrap_or_default();
            let fm = parse_knowledge_meta_yaml(&raw, path)?;
            let item_id = fm.fields.get("id").cloned().unwrap_or_else(|| {
                path.file_stem()
                    .and_then(|v| v.to_str())
                    .unwrap_or_default()
                    .to_string()
            });
            if item_id == target {
                return Ok(Some((root.clone(), path.to_path_buf(), raw)));
            }
        }
    }
    Ok(None)
}

async fn read_knowledge_item_file(
    root: &KnowledgePath,
    path: &Path,
    include_full: bool,
    budget: Option<usize>,
    section: Option<&str>,
    include_outline: bool,
) -> ApiResult<Value> {
    let raw = fs::read_to_string(path).await.unwrap_or_default();
    let fm = parse_knowledge_meta_yaml(&raw, path)?;
    let source_path = resolve_knowledge_source_path(path, &fm);
    let source_body = if let Some(source) = source_path.as_ref().filter(|p| p.is_file()) {
        fs::read_to_string(source).await.unwrap_or_default()
    } else {
        String::new()
    };
    let meta = fs::metadata(path).await.ok();
    let file_stem = path.file_stem().and_then(|v| v.to_str()).unwrap_or("item");
    let kind = fm
        .fields
        .get("kind")
        .or_else(|| fm.fields.get("type"))
        .and_then(|v| normalize_knowledge_kind(Some(v)).ok())
        .unwrap_or_else(|| root.kind.clone());
    let title = fm
        .fields
        .get("title")
        .cloned()
        .unwrap_or_else(|| file_stem.to_string());
    let summary = clean_optional(fm.fields.get("summary").map(String::as_str))
        .unwrap_or_else(|| first_paragraph(&source_body));
    let summary_limit = summary_limit_for_budget(budget, include_full);
    let (summary, summary_truncated) = truncate_chars(&summary, summary_limit);
    let outline = if include_outline {
        markdown_outline(&source_body)
    } else {
        Vec::new()
    };
    let section_title = clean_optional(section);
    let section_body = section_title
        .as_deref()
        .map(|heading| markdown_section(&source_body, heading).unwrap_or_default())
        .unwrap_or_default();
    let detail_source = if section_title.is_some() {
        &section_body
    } else {
        &source_body
    };
    let (details, details_truncated) = if include_full {
        truncate_chars(detail_source, budget.unwrap_or(20_000).clamp(1_000, 60_000))
    } else {
        (String::new(), false)
    };
    let created_at = fm
        .fields
        .get("created_at")
        .or_else(|| fm.fields.get("createdAt"))
        .cloned()
        .or_else(|| {
            meta.as_ref()
                .and_then(|m| m.created().ok())
                .map(system_time_to_rfc3339)
        })
        .unwrap_or_else(rfc3339_now);
    let updated_at = fm
        .fields
        .get("updated_at")
        .or_else(|| fm.fields.get("updatedAt"))
        .cloned()
        .or_else(|| {
            meta.as_ref()
                .and_then(|m| m.modified().ok())
                .map(system_time_to_rfc3339)
        })
        .unwrap_or_else(rfc3339_now);
    Ok(json!({
        "id": fm.fields.get("id").cloned().unwrap_or_else(|| file_stem.to_string()),
        "title": title,
        "kind": kind,
        "type": knowledge_type_name(&kind),
        "category": fm.fields.get("category").cloned().unwrap_or_default(),
        "domain": fm.fields.get("domain").cloned().unwrap_or_else(|| "general".to_string()),
        "project": fm.fields.get("project").cloned().unwrap_or_default(),
        "scope": fm.fields.get("scope").cloned().unwrap_or_else(|| root.scope.clone()),
        "status": fm.fields.get("status").cloned().unwrap_or_else(|| KNOWLEDGE_DEFAULT_STATUS.to_string()),
        "confidence": fm.fields.get("confidence").cloned().unwrap_or_else(|| KNOWLEDGE_DEFAULT_CONFIDENCE.to_string()),
        "tags": frontmatter_list(fm.fields.get("tags")),
        "triggerTerms": frontmatter_list(fm.fields.get("trigger_terms").or_else(|| fm.fields.get("triggerTerms"))),
        "relatedSkills": frontmatter_list(fm.fields.get("related_skills").or_else(|| fm.fields.get("relatedSkills"))),
        "relatedRepos": frontmatter_list(fm.fields.get("related_repos").or_else(|| fm.fields.get("relatedRepos"))),
        "relatedTables": frontmatter_list(fm.fields.get("related_tables").or_else(|| fm.fields.get("relatedTables"))),
        "relatedApis": frontmatter_list(fm.fields.get("related_apis").or_else(|| fm.fields.get("relatedApis"))),
        "source": fm.fields.get("source").cloned().unwrap_or_default(),
        "originPath": fm.fields.get("origin_path").or_else(|| fm.fields.get("originPath")).cloned().unwrap_or_default(),
        "createdAt": created_at,
        "updatedAt": updated_at,
        "lastVerifiedAt": fm.fields.get("last_verified_at").or_else(|| fm.fields.get("lastVerifiedAt")).cloned().unwrap_or_default(),
        "validUntil": fm.fields.get("valid_until").or_else(|| fm.fields.get("validUntil")).cloned().unwrap_or_default(),
        "summary": summary,
        "summaryTruncated": summary_truncated,
        "outline": outline,
        "section": section_title.unwrap_or_default(),
        "sectionFound": if section.is_some() { !section_body.is_empty() } else { true },
        "details": if include_full { Value::String(details) } else { Value::Null },
        "detailsTruncated": details_truncated,
        "contentChars": source_body.chars().count(),
        "path": source_path.as_ref().map_or(path, |p| p.as_path()).to_string_lossy(),
        "sourcePath": source_path.as_ref().map(|p| p.to_string_lossy().to_string()).unwrap_or_default(),
        "metaPath": path.to_string_lossy(),
        "root": root.root.to_string_lossy(),
    }))
}

fn resolve_knowledge_source_path(meta_path: &Path, fm: &Frontmatter) -> Option<PathBuf> {
    let value = fm
        .fields
        .get("source_path")
        .or_else(|| fm.fields.get("sourcePath"))
        .map(String::as_str)
        .and_then(|v| clean_optional(Some(v)))?;
    let path = PathBuf::from(&value);
    if path.is_absolute() {
        Some(path)
    } else {
        Some(
            meta_path
                .parent()
                .unwrap_or_else(|| Path::new(""))
                .join(path),
        )
    }
}

#[derive(Debug, Default)]
struct KnowledgeSearchScore {
    score: i64,
    why_matched: Vec<String>,
    matched_fields: Vec<String>,
    matches: Vec<Value>,
    query_tokens: Vec<String>,
}

fn knowledge_search_score(item: &Value, query: Option<&str>) -> KnowledgeSearchScore {
    let Some(query) = query.map(str::trim).filter(|v| !v.is_empty()) else {
        return KnowledgeSearchScore {
            score: 1,
            ..Default::default()
        };
    };
    let tokens = knowledge_query_tokens(query);
    let fields = [
        (
            "title",
            12,
            item.get("title")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_ascii_lowercase(),
        ),
        (
            "triggerTerms",
            10,
            json_array_text(item.get("triggerTerms")),
        ),
        ("relatedApis", 10, json_array_text(item.get("relatedApis"))),
        (
            "relatedTables",
            9,
            json_array_text(item.get("relatedTables")),
        ),
        ("tags", 8, json_array_text(item.get("tags"))),
        ("relatedRepos", 7, json_array_text(item.get("relatedRepos"))),
        (
            "relatedSkills",
            7,
            json_array_text(item.get("relatedSkills")),
        ),
        (
            "summary",
            6,
            item.get("summary")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_ascii_lowercase(),
        ),
        ("outline", 5, outline_text(item.get("outline"))),
        (
            "id",
            4,
            item.get("id")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_ascii_lowercase(),
        ),
        (
            "domain",
            4,
            item.get("domain")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_ascii_lowercase(),
        ),
        (
            "project",
            4,
            item.get("project")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_ascii_lowercase(),
        ),
        (
            "category",
            3,
            item.get("category")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_ascii_lowercase(),
        ),
    ];
    let mut score = 0;
    let mut why = Vec::new();
    let mut matched_fields = Vec::new();
    let mut matches = Vec::new();
    for token in &tokens {
        let mut matched = false;
        for (field, weight, value) in &fields {
            if !token.is_empty() && value.contains(token) {
                score += *weight;
                matched = true;
                if !matched_fields.iter().any(|v| v == field) {
                    matched_fields.push((*field).to_string());
                }
                matches.push(json!({
                    "token": token,
                    "field": field,
                    "weight": weight,
                }));
            }
        }
        if matched && !why.iter().any(|v| v == token) {
            why.push(token.clone());
        }
    }
    KnowledgeSearchScore {
        score,
        why_matched: why,
        matched_fields,
        matches,
        query_tokens: tokens,
    }
}

fn default_knowledge_limit_for_budget(budget: Option<usize>) -> Option<usize> {
    let budget = budget.unwrap_or(2200);
    Some(match budget {
        0..=899 => 3,
        900..=1799 => 4,
        1800..=3999 => 5,
        4000..=7999 => 8,
        _ => 12,
    })
}

fn max_knowledge_limit_for_budget(budget: usize) -> usize {
    match budget {
        0..=899 => 3,
        900..=1799 => 5,
        1800..=3999 => 8,
        4000..=7999 => 12,
        _ => 20,
    }
}

fn summary_limit_for_budget(budget: Option<usize>, include_full: bool) -> usize {
    if include_full {
        return 700;
    }
    match budget.unwrap_or(2200) {
        0..=899 => 220,
        900..=1799 => 320,
        1800..=3999 => 420,
        4000..=7999 => 520,
        _ => 700,
    }
}

fn apply_query_budget_to_item(
    item: &mut Value,
    budget: usize,
    limit: usize,
    include_outline: bool,
) {
    let per_item_summary = (budget.saturating_mul(2) / limit.max(1)).clamp(140, 700);
    if let Some(obj) = item.as_object_mut() {
        if let Some(summary) = obj.get("summary").and_then(Value::as_str) {
            let already_truncated = obj
                .get("summaryTruncated")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let (next, truncated) = truncate_chars(summary, per_item_summary);
            obj.insert("summary".to_string(), Value::String(next));
            obj.insert(
                "summaryTruncated".to_string(),
                json!(already_truncated || truncated),
            );
        }
        if include_outline {
            let max_outline = match budget {
                0..=899 => 4,
                900..=1799 => 6,
                1800..=3999 => 10,
                _ => 16,
            };
            if let Some(outline) = obj.get_mut("outline").and_then(Value::as_array_mut) {
                if outline.len() > max_outline {
                    outline.truncate(max_outline);
                }
            }
        } else {
            obj.remove("outline");
        }
    }
}

fn knowledge_query_tokens(query: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut ascii = String::new();
    let mut cjk = String::new();
    for ch in query.chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
            flush_cjk_tokens(&mut tokens, &cjk);
            cjk.clear();
            ascii.push(ch.to_ascii_lowercase());
        } else if is_cjk(ch) {
            flush_ascii_token(&mut tokens, &ascii);
            ascii.clear();
            cjk.push(ch);
        } else {
            flush_ascii_token(&mut tokens, &ascii);
            flush_cjk_tokens(&mut tokens, &cjk);
            ascii.clear();
            cjk.clear();
        }
    }
    flush_ascii_token(&mut tokens, &ascii);
    flush_cjk_tokens(&mut tokens, &cjk);
    unique_strings(tokens).into_iter().take(80).collect()
}

fn flush_ascii_token(tokens: &mut Vec<String>, raw: &str) {
    let value = raw.trim().to_ascii_lowercase();
    if value.len() >= 2 || value.chars().any(|c| c.is_ascii_digit()) {
        tokens.push(value);
    }
}

fn flush_cjk_tokens(tokens: &mut Vec<String>, raw: &str) {
    let chars: Vec<char> = raw.chars().collect();
    if chars.is_empty() {
        return;
    }
    if chars.len() <= 8 {
        tokens.push(chars.iter().collect::<String>());
    }
    for n in [2usize, 3, 4, 5, 6] {
        if chars.len() < n {
            continue;
        }
        for window in chars.windows(n) {
            tokens.push(window.iter().collect());
        }
    }
}

fn outline_text(value: Option<&Value>) -> String {
    value
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.get("title").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join(" ")
        })
        .unwrap_or_default()
        .to_ascii_lowercase()
}

fn json_array_text(value: Option<&Value>) -> String {
    value
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(" ")
        })
        .unwrap_or_default()
        .to_ascii_lowercase()
}

async fn save_knowledge_item(state: &AppState, form: KnowledgeWriteForm) -> ApiResult<Value> {
    let title = clean_required(&form.title, "title")?;
    ensure_text_size(&title, "title")?;
    let now = rfc3339_now();
    let requested_kind =
        normalize_knowledge_kind(form.kind.as_deref()).unwrap_or_else(|_| "experience".to_string());
    let requested_id = clean_optional(form.id.as_deref());
    let existing = if let Some(id) = requested_id.as_deref() {
        find_knowledge_file_by_id(state, id).await?
    } else {
        None
    };
    let existing_fm = existing
        .as_ref()
        .map(|(_, path, raw)| parse_knowledge_meta_yaml(raw, path))
        .transpose()?
        .unwrap_or(Frontmatter {
            fields: HashMap::new(),
            body: String::new(),
        });
    let existing_body = if let Some((_, path, raw)) = existing.as_ref() {
        let fm = parse_knowledge_meta_yaml(raw, path)?;
        if let Some(source) = resolve_knowledge_source_path(path, &fm) {
            if source.is_file() {
                fs::read_to_string(source).await.unwrap_or_default()
            } else {
                String::new()
            }
        } else {
            String::new()
        }
    } else {
        String::new()
    };
    let kind = if form.kind.is_some() {
        requested_kind
    } else {
        existing_fm
            .fields
            .get("kind")
            .or_else(|| existing_fm.fields.get("type"))
            .and_then(|v| normalize_knowledge_kind(Some(v)).ok())
            .unwrap_or(requested_kind)
    };
    let id = if let Some(id) = requested_id {
        ensure_knowledge_id(&id)?
    } else {
        unique_knowledge_id(state, &kind, form.domain.as_deref(), &title).await?
    };
    let scope = clean_optional(form.scope.as_deref())
        .or_else(|| existing_fm.fields.get("scope").cloned())
        .unwrap_or_else(|| "global".to_string());
    let domain = clean_optional(form.domain.as_deref())
        .or_else(|| existing_fm.fields.get("domain").cloned())
        .unwrap_or_else(|| "general".to_string());
    let project = clean_optional(form.project.as_deref())
        .or_else(|| existing_fm.fields.get("project").cloned())
        .unwrap_or_default();
    let status = clean_optional(form.status.as_deref())
        .or_else(|| existing_fm.fields.get("status").cloned())
        .unwrap_or_else(|| KNOWLEDGE_DEFAULT_STATUS.to_string());
    let category = clean_optional(form.category.as_deref())
        .or_else(|| existing_fm.fields.get("category").cloned())
        .unwrap_or_else(|| category_from_knowledge_id(&id));
    let confidence = clean_optional(form.confidence.as_deref())
        .or_else(|| existing_fm.fields.get("confidence").cloned())
        .unwrap_or_else(|| KNOWLEDGE_DEFAULT_CONFIDENCE.to_string());
    let summary = clean_optional(form.summary.as_deref())
        .or_else(|| existing_fm.fields.get("summary").cloned())
        .unwrap_or_default();
    ensure_text_size(&summary, "summary")?;
    let details = clean_optional(form.details.as_deref())
        .or_else(|| clean_optional(Some(existing_body.as_str())))
        .unwrap_or_else(|| template_knowledge_body(&kind));
    ensure_text_size(&details, "details")?;
    let created_at = existing_fm
        .fields
        .get("created_at")
        .or_else(|| existing_fm.fields.get("createdAt"))
        .cloned()
        .unwrap_or_else(|| now.clone());
    let last_verified_at = clean_optional(form.last_verified_at.as_deref())
        .or_else(|| existing_fm.fields.get("last_verified_at").cloned())
        .unwrap_or_default();
    let valid_until = clean_optional(form.valid_until.as_deref())
        .or_else(|| existing_fm.fields.get("valid_until").cloned())
        .unwrap_or_default();
    let tags = if form.tags.is_empty() {
        frontmatter_list(existing_fm.fields.get("tags"))
    } else {
        unique_strings(form.tags)
    };
    let trigger_terms = if form.trigger_terms.is_empty() {
        frontmatter_list(existing_fm.fields.get("trigger_terms"))
    } else {
        unique_strings(form.trigger_terms)
    };
    let related_skills = if form.related_skills.is_empty() {
        frontmatter_list(existing_fm.fields.get("related_skills"))
    } else {
        unique_strings(form.related_skills)
    };
    let related_repos = if form.related_repos.is_empty() {
        frontmatter_list(existing_fm.fields.get("related_repos"))
    } else {
        unique_strings(form.related_repos)
    };
    let related_tables = if form.related_tables.is_empty() {
        frontmatter_list(existing_fm.fields.get("related_tables"))
    } else {
        unique_strings(form.related_tables)
    };
    let related_apis = if form.related_apis.is_empty() {
        frontmatter_list(existing_fm.fields.get("related_apis"))
    } else {
        unique_strings(form.related_apis)
    };
    let source = clean_optional(form.source.as_deref())
        .or_else(|| existing_fm.fields.get("source").cloned())
        .unwrap_or_default();
    let origin_path = clean_optional(form.origin_path.as_deref())
        .or_else(|| existing_fm.fields.get("origin_path").cloned())
        .or_else(|| existing_fm.fields.get("originPath").cloned())
        .unwrap_or_default();

    let (meta_path, source_path) = if let Some((_, path, raw)) = existing {
        let fm = parse_knowledge_meta_yaml(&raw, &path)?;
        let source = resolve_knowledge_source_path(&path, &fm).unwrap_or_else(|| {
            path.parent()
                .unwrap_or_else(|| Path::new(""))
                .join("../items")
                .join(format!("{id}.md"))
        });
        (path, source)
    } else {
        knowledge_paths_for_new_item(state, &kind, &scope, form.root.as_deref(), &domain, &id)
            .await?
    };
    if form.dry_run.unwrap_or(false) {
        return Ok(
            json!({ "id": id, "path": source_path, "sourcePath": source_path, "metaPath": meta_path, "dryRun": true }),
        );
    }
    let meta_source_path = relative_knowledge_source_path(&meta_path, &source_path);
    let meta_text = render_knowledge_meta_yaml(KnowledgeRenderInput {
        id: &id,
        kind: &kind,
        title: &title,
        category: &category,
        domain: &domain,
        project: &project,
        scope: &scope,
        status: &status,
        confidence: &confidence,
        tags: &tags,
        trigger_terms: &trigger_terms,
        related_skills: &related_skills,
        related_repos: &related_repos,
        related_tables: &related_tables,
        related_apis: &related_apis,
        source: &source,
        origin_path: &origin_path,
        source_path: &meta_source_path,
        summary: &summary,
        created_at: &created_at,
        updated_at: &now,
        last_verified_at: &last_verified_at,
        valid_until: &valid_until,
    });
    atomic_write_text(&source_path, &(details.trim().to_string() + "\n")).await?;
    atomic_write_text(&meta_path, &meta_text).await?;
    let root = KnowledgePath {
        path: meta_path
            .parent()
            .unwrap_or_else(|| Path::new(""))
            .to_path_buf(),
        kind,
        scope,
        root: meta_path
            .parent()
            .unwrap_or_else(|| Path::new(""))
            .to_path_buf(),
    };
    read_knowledge_item_file(&root, &meta_path, true, Some(20_000), None, true).await
}

async fn unique_knowledge_id(
    state: &AppState,
    kind: &str,
    domain: Option<&str>,
    title: &str,
) -> ApiResult<String> {
    let mut id = knowledge_id_from_title(kind, domain, title);
    if find_knowledge_file_by_id(state, &id).await?.is_none() {
        return Ok(id);
    }
    id = format!("{}-{}", id, chrono::Utc::now().format("%H%M%S"));
    ensure_knowledge_id(&id)
}

fn knowledge_id_from_title(kind: &str, domain: Option<&str>, title: &str) -> String {
    let prefix = if kind == "businessKnowledge" {
        "biz"
    } else {
        "exp"
    };
    let domain = slug_segment(domain.unwrap_or("general"), "general");
    let title_slug = slug_segment(
        title,
        &chrono::Utc::now().format("%Y%m%d%H%M%S").to_string(),
    );
    format!("{prefix}-{domain}-{title_slug}")
}

fn ensure_knowledge_id(value: &str) -> ApiResult<String> {
    let v = value.trim();
    if v.len() < 3 || v.len() > 160 {
        return Err(ApiError::bad_request("invalid knowledge id length"));
    }
    if !v
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
    {
        return Err(ApiError::bad_request(
            "knowledge id must be ASCII alphanumeric, dash, underscore, or dot",
        ));
    }
    Ok(v.to_string())
}

async fn resolve_knowledge_write_dir(
    state: &AppState,
    kind: &str,
    scope: &str,
    requested_root: Option<&str>,
) -> ApiResult<PathBuf> {
    let dir_name = knowledge_kind_dir(kind);
    if let Some(raw) = clean_optional(requested_root) {
        let base = normalize_user_path(&raw);
        let dir = if base.file_name().and_then(|v| v.to_str()) == Some(dir_name) {
            base
        } else if base.file_name().and_then(|v| v.to_str()) == Some(".agents") {
            base.join(dir_name)
        } else {
            base.join(".agents").join(dir_name)
        };
        let home = home_dir()?;
        if !same_or_child_path(&dir, &home) {
            return Err(ApiError::bad_request(format!(
                "knowledge root is outside home: {raw}"
            )));
        }
        return Ok(dir);
    }
    if scope == "project" {
        if let Some(root) = knowledge_storage_roots(state, Some(kind))
            .await?
            .into_iter()
            .find(|r| r.scope == "project")
        {
            return Ok(root.path);
        }
    }
    Ok(home_dir()?.join(".agents").join(dir_name))
}

struct KnowledgeRenderInput<'a> {
    id: &'a str,
    kind: &'a str,
    title: &'a str,
    category: &'a str,
    domain: &'a str,
    project: &'a str,
    scope: &'a str,
    status: &'a str,
    confidence: &'a str,
    tags: &'a [String],
    trigger_terms: &'a [String],
    related_skills: &'a [String],
    related_repos: &'a [String],
    related_tables: &'a [String],
    related_apis: &'a [String],
    source: &'a str,
    origin_path: &'a str,
    source_path: &'a str,
    summary: &'a str,
    created_at: &'a str,
    updated_at: &'a str,
    last_verified_at: &'a str,
    valid_until: &'a str,
}

fn render_knowledge_meta_yaml(input: KnowledgeRenderInput<'_>) -> String {
    let mut lines = vec![
        format!("id: {}", yaml_quote(input.id)),
        format!("title: {}", yaml_quote(input.title)),
        format!("kind: {}", yaml_quote(input.kind)),
        format!("type: {}", yaml_quote(knowledge_type_name(input.kind))),
        format!("category: {}", yaml_quote(input.category)),
        format!("domain: {}", yaml_quote(input.domain)),
        format!("project: {}", yaml_quote(input.project)),
        format!("scope: {}", yaml_quote(input.scope)),
        format!("status: {}", yaml_quote(input.status)),
        format!("confidence: {}", yaml_quote(input.confidence)),
        format!("created_at: {}", yaml_quote(input.created_at)),
        format!("updated_at: {}", yaml_quote(input.updated_at)),
        format!("source_path: {}", yaml_quote(input.source_path)),
    ];
    if !input.last_verified_at.is_empty() {
        lines.push(format!(
            "last_verified_at: {}",
            yaml_quote(input.last_verified_at)
        ));
    }
    if !input.valid_until.is_empty() {
        lines.push(format!("valid_until: {}", yaml_quote(input.valid_until)));
    }
    if !input.source.is_empty() {
        lines.push(format!("source: {}", yaml_quote(input.source)));
    }
    if !input.origin_path.is_empty() {
        lines.push(format!("origin_path: {}", yaml_quote(input.origin_path)));
    }
    if !input.summary.is_empty() {
        lines.push(format!("summary: {}", yaml_quote(input.summary)));
    }
    lines.push(format!("tags: {}", yaml_inline_list(input.tags)));
    lines.push(format!(
        "trigger_terms: {}",
        yaml_inline_list(input.trigger_terms)
    ));
    if !input.related_skills.is_empty() {
        lines.push(format!(
            "related_skills: {}",
            yaml_inline_list(input.related_skills)
        ));
    }
    if !input.related_repos.is_empty() {
        lines.push(format!(
            "related_repos: {}",
            yaml_inline_list(input.related_repos)
        ));
    }
    if !input.related_tables.is_empty() {
        lines.push(format!(
            "related_tables: {}",
            yaml_inline_list(input.related_tables)
        ));
    }
    if !input.related_apis.is_empty() {
        lines.push(format!(
            "related_apis: {}",
            yaml_inline_list(input.related_apis)
        ));
    }
    format!("{}\n", lines.join("\n"))
}

fn yaml_inline_list(values: &[String]) -> String {
    if values.is_empty() {
        "[]".to_string()
    } else {
        format!(
            "[{}]",
            values
                .iter()
                .map(|v| yaml_quote(v))
                .collect::<Vec<_>>()
                .join(", ")
        )
    }
}

async fn knowledge_paths_for_new_item(
    state: &AppState,
    kind: &str,
    scope: &str,
    requested_root: Option<&str>,
    _domain: &str,
    id: &str,
) -> ApiResult<(PathBuf, PathBuf)> {
    let dir = resolve_knowledge_write_dir(state, kind, scope, requested_root).await?;
    let base = dir;
    Ok((
        base.join("meta").join(format!("{id}.yaml")),
        base.join("items").join(format!("{id}.md")),
    ))
}

fn relative_knowledge_source_path(meta_path: &Path, source_path: &Path) -> String {
    if let Some(meta_dir) = meta_path.parent() {
        if let Some(base) = meta_dir.parent() {
            if let Ok(rel) = source_path.strip_prefix(base) {
                return format!("../{}", rel.to_string_lossy());
            }
        }
        if let Ok(rel) = source_path.strip_prefix(meta_dir) {
            return rel.to_string_lossy().to_string();
        }
    }
    source_path.to_string_lossy().to_string()
}

fn category_from_knowledge_id(id: &str) -> String {
    let first = id.split('-').next().unwrap_or_default();
    match first {
        "api" | "biz" | "conventions" | "link" | "pitfall" | "profile" | "ref" => first.to_string(),
        "exp" => "experience".to_string(),
        _ => "general".to_string(),
    }
}

fn frontmatter_list(value: Option<&String>) -> Vec<String> {
    value
        .map(|v| {
            v.trim()
                .trim_matches(|c| matches!(c, '[' | ']' | ' '))
                .split([',', '，'])
                .map(|s| s.trim().trim_matches('"').trim_matches('\'').to_string())
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

fn slug_segment(value: &str, fallback: &str) -> String {
    let mut out = String::new();
    let mut prev_dash = false;
    for ch in value.to_ascii_lowercase().chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch);
            prev_dash = false;
        } else if matches!(ch, '-' | '_' | '.') {
            out.push(ch);
            prev_dash = false;
        } else if !prev_dash {
            out.push('-');
            prev_dash = true;
        }
    }
    let compact = out.trim_matches('-').to_string();
    if compact.is_empty() {
        fallback.to_string()
    } else {
        compact
    }
}

fn rfc3339_now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

fn system_time_to_rfc3339(value: SystemTime) -> String {
    let dt: chrono::DateTime<chrono::Utc> = value.into();
    dt.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

fn template_knowledge_body(kind: &str) -> String {
    if kind == "businessKnowledge" {
        "## 概述\n\n## 规则\n\n## 相关接口\n\n## 相关表\n\n## 注意事项".to_string()
    } else {
        "## 现象\n\n## 根因\n\n## 处理方式\n\n## 验证方式\n\n## 过期风险".to_string()
    }
}

fn parse_knowledge_meta_yaml(text: &str, path: &Path) -> ApiResult<Frontmatter> {
    let value: serde_yaml::Value = serde_yaml::from_str(text).map_err(|e| {
        ApiError::bad_request(format!(
            "invalid knowledge meta yaml {}: {e}",
            path.display()
        ))
    })?;
    let mapping = value.as_mapping().ok_or_else(|| {
        ApiError::bad_request(format!(
            "knowledge meta yaml must be a mapping: {}",
            path.display()
        ))
    })?;
    let mut fields = HashMap::new();
    for (key, value) in mapping {
        let Some(key) = key.as_str() else {
            continue;
        };
        fields.insert(key.to_string(), yaml_value_to_field(value));
    }
    Ok(Frontmatter {
        fields,
        body: String::new(),
    })
}

fn yaml_value_to_field(value: &serde_yaml::Value) -> String {
    match value {
        serde_yaml::Value::Null => String::new(),
        serde_yaml::Value::Bool(v) => v.to_string(),
        serde_yaml::Value::Number(v) => v.to_string(),
        serde_yaml::Value::String(v) => v.clone(),
        serde_yaml::Value::Sequence(values) => values
            .iter()
            .map(yaml_value_to_field)
            .filter(|v| !v.is_empty())
            .collect::<Vec<_>>()
            .join(", "),
        serde_yaml::Value::Mapping(_) | serde_yaml::Value::Tagged(_) => {
            serde_yaml::to_string(value)
                .unwrap_or_default()
                .trim()
                .to_string()
        }
    }
}

pub(crate) fn first_paragraph(body: &str) -> String {
    for paragraph in body.trim_start().split("\n\n") {
        let cleaned = paragraph
            .lines()
            .filter(|l| !l.trim_start().starts_with('#'))
            .collect::<Vec<_>>()
            .join("\n")
            .trim()
            .to_string();
        if !cleaned.is_empty() {
            return cleaned;
        }
    }
    String::new()
}
