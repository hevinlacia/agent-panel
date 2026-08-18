use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    time::Duration,
};

use axum::{
    extract::{Query, State},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::{fs, process::Command, time::timeout};

use crate::{
    agent_panel_skill_path, ApiError, ApiResult, AppState, FormOrJson, IdQuery,
    DEFAULT_WMS_PROJECT_ROOT, DEFAULT_WMS_TESTDATA_PACK_ROOT,
};

pub(crate) async fn api_capability_sources(
    State(state): State<AppState>,
) -> ApiResult<Json<Value>> {
    let sources = capability_sources(&state);
    Ok(Json(json!({
        "ok": true,
        "format": "agentPanel.capabilitySources.v1",
        "version": 1,
        "sources": sources,
        "schemaUrl": "/api/capability/schema",
        "rules": [
            "Agent Panel indexes project-owned capability packs and normalizes legacy adapters into a common schema.",
            "Project business data stays in the project root; Agent Panel does not store business assets in its own repo.",
            "Current APIs are read-only; run execution is intentionally out of scope for this phase."
        ]
    })))
}

pub(crate) async fn api_capability_schema() -> Json<Value> {
    Json(capability_pack_schema())
}

pub(crate) async fn api_testdata_capabilities(
    State(state): State<AppState>,
    Query(query): Query<IdQuery>,
) -> ApiResult<Json<Value>> {
    let project = query.project.as_deref().unwrap_or("WMS");
    let Some(source) = capability_sources(&state).into_iter().find(|source| {
        source
            .get("project")
            .and_then(Value::as_str)
            .map(|p| p.eq_ignore_ascii_case(project))
            .unwrap_or(false)
    }) else {
        return Err(ApiError::bad_request(format!(
            "capability source not found for project: {project}"
        )));
    };
    let source_path = PathBuf::from(
        source
            .get("sourcePath")
            .and_then(Value::as_str)
            .unwrap_or(""),
    );
    let pack = read_testdata_capability_pack(&source_path).await?;
    let raw_caps = pack
        .get("capabilities")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let maintenance_policy = pack
        .get("maintenance_policy")
        .cloned()
        .unwrap_or(Value::Null);
    let target = query
        .target
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty());
    let domain = query
        .domain
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty());
    let q = query.q.as_deref().map(str::trim).filter(|v| !v.is_empty());
    let capabilities: Vec<Value> = raw_caps
        .into_iter()
        .filter(|cap| capability_matches(cap, target, domain, q))
        .map(|cap| capability_summary_with_policy(&source_path, &cap, &maintenance_policy))
        .collect();
    Ok(Json(json!({
        "ok": true,
        "format": "agentPanel.capabilities.v1",
        "project": project,
        "source": source,
        "filters": { "target": target, "domain": domain, "q": q },
        "count": capabilities.len(),
        "maintenancePolicy": maintenance_policy,
        "capabilities": capabilities,
        "apis": {
            "schema": "/api/capability/schema",
            "detail": "/api/capability?id=<capability-id>&project=<project>",
            "legacyDetail": "/api/testdata/capability?id=<capability-id>&project=<project>",
            "sources": "/api/capability/sources"
        },
        "help": {
            "skillName": "wms-test-data-creation",
            "skillPath": agent_panel_skill_path("wms-test-data-creation"),
            "phase": "read-only-index",
            "note": "第一阶段只读索引；如需执行，按 detail.runnerHint 中的命令到 sourcePath 项目运行。"
        }
    })))
}

pub(crate) async fn api_testdata_capability(
    State(state): State<AppState>,
    Query(query): Query<IdQuery>,
) -> ApiResult<Json<Value>> {
    let id = query.id.or(query.req_id).unwrap_or_default();
    if id.trim().is_empty() {
        return Err(ApiError::bad_request("missing capability id"));
    }
    let project = query.project.as_deref().unwrap_or("WMS");
    let Some(source) = capability_sources(&state).into_iter().find(|source| {
        source
            .get("project")
            .and_then(Value::as_str)
            .map(|p| p.eq_ignore_ascii_case(project))
            .unwrap_or(false)
    }) else {
        return Err(ApiError::bad_request(format!(
            "capability source not found for project: {project}"
        )));
    };
    let source_path = PathBuf::from(
        source
            .get("sourcePath")
            .and_then(Value::as_str)
            .unwrap_or(""),
    );
    let pack = read_testdata_capability_pack(&source_path).await?;
    let capabilities = pack
        .get("capabilities")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let maintenance_policy = pack
        .get("maintenance_policy")
        .cloned()
        .unwrap_or(Value::Null);
    let Some(capability) = capabilities
        .into_iter()
        .find(|cap| cap.get("id").and_then(Value::as_str) == Some(id.as_str()))
    else {
        return Err(ApiError::bad_request(format!(
            "testdata capability not found: {id}"
        )));
    };
    Ok(Json(json!({
        "ok": true,
        "format": "agentPanel.capability.v1",
        "project": project,
        "source": source,
        "maintenancePolicy": maintenance_policy.clone(),
        "capability": capability_detail_with_policy(&source_path, &capability, &maintenance_policy),
        "help": {
            "skillName": "wms-test-data-creation",
            "skillPath": agent_panel_skill_path("wms-test-data-creation"),
            "phase": "read-only-index",
            "note": "第一阶段只读索引；Agent Panel 不执行造数脚本。"
        }
    })))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TestdataRunForm {
    project: String,
    capability_id: String,
    target: String,
    #[serde(default)]
    env: Option<String>,
    #[serde(default)]
    params: HashMap<String, String>,
    #[serde(default)]
    dry_run: Option<bool>,
    #[serde(default)]
    execute: Option<bool>,
}

pub(crate) async fn api_testdata_run(
    State(state): State<AppState>,
    form: FormOrJson<TestdataRunForm>,
) -> ApiResult<Json<Value>> {
    let body = form.0;
    let project = body.project.as_str();
    let Some(source) = capability_sources(&state).into_iter().find(|source| {
        source
            .get("project")
            .and_then(Value::as_str)
            .map(|p| p.eq_ignore_ascii_case(project))
            .unwrap_or(false)
    }) else {
        return Err(ApiError::bad_request(format!(
            "capability source not found for project: {project}"
        )));
    };
    let source_path = PathBuf::from(
        source
            .get("sourcePath")
            .and_then(Value::as_str)
            .unwrap_or(""),
    );
    if !source_path.is_dir() {
        return Err(ApiError::bad_request(format!(
            "capability pack missing: {}",
            source_path.display()
        )));
    }
    let pack = read_testdata_capability_pack(&source_path).await?;
    let capabilities = pack
        .get("capabilities")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let Some(capability) = capabilities
        .into_iter()
        .find(|cap| cap.get("id").and_then(Value::as_str) == Some(body.capability_id.as_str()))
    else {
        return Err(ApiError::bad_request(format!(
            "testdata capability not found: {}",
            body.capability_id
        )));
    };
    let script = capability
        .get("script")
        .and_then(Value::as_str)
        .unwrap_or("");
    if script.trim().is_empty() {
        return Err(ApiError::bad_request(format!(
            "capability {} has no runnable script",
            body.capability_id
        )));
    }
    let execution = capability
        .get("execution")
        .and_then(Value::as_str)
        .unwrap_or("script");
    if execution != "script" {
        return Err(ApiError::bad_request(format!(
            "capability {} execution is `{}`, Agent Panel only runs `script` capabilities",
            body.capability_id, execution
        )));
    }
    if body.target.trim().is_empty() {
        return Err(ApiError::bad_request("missing target"));
    }
    let target_valid = capability
        .get("targets")
        .and_then(Value::as_array)
        .map(|targets| {
            targets
                .iter()
                .any(|item| item.get("name").and_then(Value::as_str) == Some(body.target.as_str()))
        })
        .unwrap_or(false);
    if !target_valid {
        return Err(ApiError::bad_request(format!(
            "target `{}` is not supported by capability {}",
            body.target, body.capability_id
        )));
    }
    let mut args: Vec<String> = vec![
        "run".into(),
        "python".into(),
        script.into(),
        "--target".into(),
        body.target.clone(),
    ];
    if let Some(cli) = capability.get("cli").and_then(Value::as_object) {
        for (key, _spec) in cli {
            if key == "target" || key == "env" {
                continue;
            }
            if let Some(val) = body.params.get(key) {
                let trimmed = val.trim();
                if !trimmed.is_empty() {
                    args.push(format!("--{}", key));
                    args.push(trimmed.to_string());
                }
            }
        }
    }
    if let Some(env) = body.env.as_deref().map(str::trim).filter(|v| !v.is_empty()) {
        args.push("--env".into());
        args.push(env.to_string());
    }
    let command_preview = format!("uv {}", args.join(" "));
    let execute_flag = body.execute.unwrap_or(false);
    let dry_run = body.dry_run.unwrap_or(true);
    if !execute_flag || dry_run {
        return Ok(Json(json!({
            "ok": true,
            "dryRun": true,
            "executed": false,
            "project": project,
            "capabilityId": body.capability_id,
            "target": body.target,
            "env": body.env.clone(),
            "cwd": source_path.to_string_lossy(),
            "command": command_preview,
            "args": args,
            "safety": {
                "agentPanelExecutes": false,
                "env": "test",
                "note": "dry-run preview. Pass execute=true (and dryRun=false) to create real test data in the WMS test environment."
            }
        })));
    }
    let runner = Command::new("uv")
        .args(&args)
        .current_dir(&source_path)
        .output();
    let result = timeout(Duration::from_secs(180), runner).await;
    match result {
        Ok(Ok(output)) => {
            let stdout = String::from_utf8_lossy(&output.stdout).to_string();
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            Ok(Json(json!({
                "ok": output.status.success(),
                "dryRun": false,
                "executed": true,
                "project": project,
                "capabilityId": body.capability_id,
                "target": body.target,
                "env": body.env.clone(),
                "cwd": source_path.to_string_lossy(),
                "command": command_preview,
                "exitCode": output.status.code(),
                "stdout": stdout,
                "stderr": stderr,
                "safety": {
                    "agentPanelExecutes": true,
                    "env": body.env.clone().unwrap_or_else(|| "test".to_string()),
                    "note": "Real test data was created in the WMS test environment."
                }
            })))
        }
        Ok(Err(e)) => Err(ApiError::bad_request(format!(
            "failed to spawn runner for capability {}: {e}",
            body.capability_id
        ))),
        Err(_) => Err(ApiError::bad_request(format!(
            "runner timed out after 180s for capability {}",
            body.capability_id
        ))),
    }
}

pub(crate) fn capability_pack_schema() -> Value {
    json!({
        "format": "agentPanel.capabilityPackSchema.v1",
        "version": 1,
        "packFile": "capabilities.yaml",
        "pack": {
            "required": ["version", "capabilities"],
            "recommended": ["id", "title", "project", "kind", "description", "updated"],
            "fields": {
                "version": "integer schema version",
                "id": "stable pack id, e.g. wms-testdata",
                "project": "owning project id/name",
                "kind": "capability kind, e.g. testdata/api/debug/deploy/docs",
                "maintenance_policy": "optional pack-level rules agents must follow when using or updating capabilities",
                "capabilities": "array of capability items"
            }
        },
        "capability": {
            "required": ["id", "kind", "title", "runner"],
            "recommended": ["domain", "object", "description", "inputs", "outputs", "environments", "safety", "verification", "relatedArtifacts"],
            "fields": {
                "id": "stable id within pack",
                "kind": "testdata | api | debug | deploy | docs | custom",
                "title": "human-readable title",
                "description": "what this capability does",
                "domain": "business or technical domain",
                "object": "main target object/entity",
                "inputs": "parameter schema map",
                "outputs": "declared output schema map",
                "environments": "supported environments and policy",
                "runner": "how to run: command/script/recipe/api/dry-run; read-only APIs do not execute it",
                "safety": "side effects, DB write policy, env allowlist",
                "verification": "how to verify outputs or side effects",
                "relatedArtifacts": "recipes, schemas, state graphs, pitfalls, API templates, docs",
                "maintenancePolicy": "pack-level maintenance contract copied into each normalized capability when present"
            }
        },
        "legacyAdapters": {
            "wms-testdata-recipes": {
                "id": "id",
                "kind": "testdata",
                "title": "purpose or id",
                "description": "purpose",
                "runner.command": "invocation/script/execution",
                "inputs": "cli",
                "verification.targets": "targets",
                "relatedArtifacts.recipe": "recipe",
                "relatedArtifacts.stateGraph": "state_graph",
                "relatedArtifacts.pitfalls": "pitfalls"
            }
        }
    })
}

pub(crate) fn capability_sources(_state: &AppState) -> Vec<Value> {
    let pack = Path::new(DEFAULT_WMS_TESTDATA_PACK_ROOT);
    vec![json!({
        "id": "wms-testdata",
        "kind": "testdata",
        "project": "WMS",
        "title": "WMS Test Data Capability Pack",
        "adapter": "wms-testdata-recipes",
        "projectRoot": DEFAULT_WMS_PROJECT_ROOT,
        "sourcePath": DEFAULT_WMS_TESTDATA_PACK_ROOT,
        "capabilityFile": "capabilities.yaml",
        "knowledgeRoot": format!("{}/.agents/knowledge", DEFAULT_WMS_PROJECT_ROOT),
        "capabilityApi": "/api/capabilities?project=WMS",
        "detailApi": "/api/capability?id=<capability-id>&project=WMS",
        "status": if pack.is_dir() { "ok" } else { "missing" },
        "exists": pack.is_dir(),
        "projectExists": Path::new(DEFAULT_WMS_PROJECT_ROOT).is_dir(),
        "readOnly": true,
        "notes": [
            "WMS-owned test-data assets live under the WMS project root.",
            "These APIs are read-only; Agent Panel does not execute runners."
        ]
    })]
}

pub(crate) async fn read_testdata_capability_pack(path: &Path) -> ApiResult<Value> {
    let file = path.join("capabilities.yaml");
    let raw = fs::read_to_string(&file).await.map_err(|e| {
        ApiError::bad_request(format!(
            "failed to read capability pack {}: {e}",
            file.display()
        ))
    })?;
    let value: serde_yaml::Value = serde_yaml::from_str(&raw).map_err(|e| {
        ApiError::bad_request(format!("invalid capability pack {}: {e}", file.display()))
    })?;
    let json = serde_json::to_value(value).map_err(|e| {
        ApiError::bad_request(format!(
            "failed to convert capability pack {}: {e}",
            file.display()
        ))
    })?;
    Ok(json)
}

pub(crate) fn capability_matches(
    cap: &Value,
    target: Option<&str>,
    domain: Option<&str>,
    q: Option<&str>,
) -> bool {
    if let Some(domain) = domain {
        let cap_domain = cap.get("domain").and_then(Value::as_str).unwrap_or("");
        if !cap_domain.eq_ignore_ascii_case(domain) {
            return false;
        }
    }
    if let Some(target) = target {
        let matches_target = cap
            .get("targets")
            .and_then(Value::as_array)
            .map(|targets| {
                targets.iter().any(|item| {
                    item.get("name").and_then(Value::as_str) == Some(target)
                        || item
                            .get("label")
                            .and_then(Value::as_str)
                            .map(|v| v.eq_ignore_ascii_case(target))
                            .unwrap_or(false)
                        || item
                            .get("status_code")
                            .and_then(Value::as_i64)
                            .map(|code| code.to_string() == target)
                            .unwrap_or(false)
                })
            })
            .unwrap_or(false);
        if !matches_target {
            return false;
        }
    }
    if let Some(q) = q {
        let q = q.to_lowercase();
        let mut hay = vec![
            cap.get("id")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            cap.get("domain")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            cap.get("purpose")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            cap.get("execution")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            cap.get("script")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            cap.get("recipe")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            cap.get("invocation")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
        ];
        if let Some(notes) = cap.get("notes").and_then(Value::as_array) {
            hay.push(
                notes
                    .iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join(" "),
            );
        }
        if !hay.iter().any(|v| v.to_lowercase().contains(&q)) {
            return false;
        }
    }
    true
}

pub(crate) fn capability_summary(source_path: &Path, cap: &Value) -> Value {
    let targets = cap
        .get("targets")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let verified_targets = targets
        .iter()
        .filter(|item| {
            item.get("verified")
                .and_then(Value::as_bool)
                .unwrap_or(false)
        })
        .count();
    let normalized = normalize_capability(source_path, cap);
    json!({
        "id": cap.get("id").cloned().unwrap_or(Value::Null),
        "domain": cap.get("domain").cloned().unwrap_or(Value::Null),
        "object": cap.get("object").cloned().unwrap_or(Value::Null),
        "execution": cap.get("execution").cloned().unwrap_or(Value::Null),
        "purpose": cap.get("purpose").cloned().unwrap_or(Value::Null),
        "script": cap.get("script").cloned().unwrap_or(Value::Null),
        "recipe": cap.get("recipe").cloned().unwrap_or(Value::Null),
        "verifiedEnv": cap.get("verified_env").cloned().unwrap_or(Value::Null),
        "verifiedDate": cap.get("verified_date").cloned().unwrap_or(Value::Null),
        "stdoutJson": cap.get("stdout_json").cloned().unwrap_or(Value::Null),
        "exitCode": cap.get("exit_code").cloned().unwrap_or(Value::Null),
        "targetCount": targets.len(),
        "verifiedTargetCount": verified_targets,
        "runHint": capability_run_hint(source_path, cap),
        "sourcePath": source_path.to_string_lossy(),
        "status": capability_status(cap),
        "normalized": normalized,
    })
}

pub(crate) fn capability_summary_with_policy(
    source_path: &Path,
    cap: &Value,
    maintenance_policy: &Value,
) -> Value {
    let mut summary = capability_summary(source_path, cap);
    if !maintenance_policy.is_null() {
        if let Some(map) = summary.as_object_mut() {
            map.insert("maintenancePolicy".to_string(), maintenance_policy.clone());
            if let Some(normalized) = map.get_mut("normalized").and_then(Value::as_object_mut) {
                normalized.insert("maintenancePolicy".to_string(), maintenance_policy.clone());
            }
            if let Some(run_hint) = map.get_mut("runHint").and_then(Value::as_object_mut) {
                run_hint.insert("maintenancePolicy".to_string(), maintenance_policy.clone());
            }
        }
    }
    summary
}

pub(crate) fn capability_detail(source_path: &Path, cap: &Value) -> Value {
    let mut summary = capability_summary(source_path, cap);
    if let Some(map) = summary.as_object_mut() {
        map.insert("capability".to_string(), cap.clone());
        map.insert(
            "targets".to_string(),
            cap.get("targets").cloned().unwrap_or_else(|| json!([])),
        );
        map.insert(
            "cli".to_string(),
            cap.get("cli").cloned().unwrap_or_else(|| json!({})),
        );
        map.insert(
            "pitfalls".to_string(),
            cap.get("pitfalls").cloned().unwrap_or_else(|| json!([])),
        );
        map.insert(
            "notes".to_string(),
            cap.get("notes").cloned().unwrap_or_else(|| json!([])),
        );
        map.insert(
            "migration".to_string(),
            json!({
                "sourcePath": DEFAULT_WMS_TESTDATA_PACK_ROOT,
                "phase": "phase-3",
                "note": "Assets are owned by the WMS project; the legacy tools repo has been removed."
            })
        );
    }
    summary
}

pub(crate) fn capability_detail_with_policy(
    source_path: &Path,
    cap: &Value,
    maintenance_policy: &Value,
) -> Value {
    let mut detail = capability_detail(source_path, cap);
    if !maintenance_policy.is_null() {
        if let Some(map) = detail.as_object_mut() {
            map.insert("maintenancePolicy".to_string(), maintenance_policy.clone());
            if let Some(normalized) = map.get_mut("normalized").and_then(Value::as_object_mut) {
                normalized.insert("maintenancePolicy".to_string(), maintenance_policy.clone());
            }
            if let Some(run_hint) = map.get_mut("runHint").and_then(Value::as_object_mut) {
                run_hint.insert("maintenancePolicy".to_string(), maintenance_policy.clone());
            }
        }
    }
    detail
}

pub(crate) fn normalize_capability(source_path: &Path, cap: &Value) -> Value {
    let id = cap.get("id").and_then(Value::as_str).unwrap_or_default();
    let purpose = cap.get("purpose").and_then(Value::as_str).unwrap_or(id);
    let runner_type = cap
        .get("execution")
        .and_then(Value::as_str)
        .unwrap_or("custom");
    json!({
        "schemaVersion": 1,
        "id": id,
        "kind": "testdata",
        "title": purpose,
        "description": purpose,
        "domain": cap.get("domain").cloned().unwrap_or(Value::Null),
        "object": cap.get("object").cloned().unwrap_or(Value::Null),
        "inputs": cap.get("cli").cloned().unwrap_or_else(|| json!({})),
        "outputs": {
            "stdoutJson": cap.get("stdout_json").cloned().unwrap_or(Value::Null),
            "exitCode": cap.get("exit_code").cloned().unwrap_or(Value::Null)
        },
        "environments": {
            "verified": cap.get("verified_env").cloned().unwrap_or(Value::Null),
            "verifiedDate": cap.get("verified_date").cloned().unwrap_or(Value::Null)
        },
        "runner": {
            "type": runner_type,
            "script": cap.get("script").cloned().unwrap_or(Value::Null),
            "command": cap.get("invocation").cloned().unwrap_or(Value::Null),
            "cwd": source_path.to_string_lossy(),
            "readOnlyInAgentPanel": true
        },
        "safety": {
            "agentPanelExecutes": false,
            "writesDatabase": false,
            "uatDbWritesAllowed": false,
            "policy": "Capability pack may define executable runners, but Agent Panel phase 3 only indexes and normalizes them."
        },
        "verification": {
            "targets": cap.get("targets").cloned().unwrap_or_else(|| json!([])),
            "stateGraph": cap.get("state_graph").cloned().unwrap_or(Value::Null)
        },
        "relatedArtifacts": {
            "recipe": cap.get("recipe").cloned().unwrap_or(Value::Null),
            "stateGraph": cap.get("state_graph").cloned().unwrap_or(Value::Null),
            "pitfalls": cap.get("pitfalls").cloned().unwrap_or_else(|| json!([])),
            "notes": cap.get("notes").cloned().unwrap_or_else(|| json!([]))
        },
        "legacy": cap
    })
}

pub(crate) fn capability_run_hint(source_path: &Path, cap: &Value) -> Value {
    json!({
        "sourcePath": source_path.to_string_lossy(),
        "dryRun": cap.get("invocation").cloned().unwrap_or(Value::Null),
        "execute": cap.get("invocation").and_then(Value::as_str).unwrap_or_default(),
        "normalizedRunner": normalize_capability(source_path, cap).get("runner").cloned().unwrap_or(Value::Null),
        "notes": [
            "Phase 3 keeps Agent Panel read-only.",
            "Use normalized.runner for generic capability-pack consumers.",
            "To execute manually, run the command from sourcePath in the project owner repo or legacy fallback path."
        ]
    })
}

pub(crate) fn capability_status(cap: &Value) -> Value {
    json!({
        "verifiedEnv": cap.get("verified_env").cloned().unwrap_or(Value::Null),
        "verifiedDate": cap.get("verified_date").cloned().unwrap_or(Value::Null),
        "hasScript": cap.get("script").and_then(Value::as_str).map(|s| !s.trim().is_empty()).unwrap_or(false),
        "hasRecipe": cap.get("recipe").and_then(Value::as_str).map(|s| !s.trim().is_empty()).unwrap_or(false)
    })
}
