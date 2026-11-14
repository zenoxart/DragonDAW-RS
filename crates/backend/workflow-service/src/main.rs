//! Red Hive workflow service: workflow definitions (versioned, immutable once published), the durable
//! engine, triggers, the inbox, impact analysis of module updates and automatic migration.

mod catalog;
mod engine;
mod impact;
mod store;
mod triggers;

use axum::extract::{Path, Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use catalog::Catalog;
use chrono::Utc;
use engine::Engine;
use redhive_backend_core::db::DbErrorExt;
use redhive_backend_core::hosting::{every, serve, Core};
use redhive_backend_core::{ApiResult, AuditLog, InternalCall, RequestContext, ToHttp, OptJson, ReqJson};
use redhive_contracts::notifications::{CommandExecutedEvent, NotificationEnvelope};
use redhive_contracts::primitives::ids;
use redhive_contracts::workflows::analysis::{self, IssueSeverity};
use redhive_contracts::workflows::api::*;
use redhive_contracts::workflows::impact::ImpactReport;
use redhive_contracts::workflows::*;
use redhive_monads::{Error, Res};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use uuid::Uuid;

#[derive(Clone)]
struct AppState {
    core: Core,
    engine: Engine,
    audit: AuditLog,
}

impl axum::extract::FromRef<AppState> for Core {
    fn from_ref(s: &AppState) -> Core {
        s.core.clone()
    }
}

// ------------------------------------------------------------------ definitions

async fn summaries(s: &AppState, tenant: Uuid) -> Res<Vec<WorkflowSummary>> {
    let mut out = Vec::new();
    for d in store::latest_all(&s.core, tenant).await? {
        out.push(WorkflowSummary {
            id: d.id,
            version: d.version,
            name: d.name.clone(),
            description: d.description.clone(),
            status: d.status,
            origin: d.origin,
            owning_module: d.owning_module.clone(),
            triggers: d.triggers.iter().map(|t| format!("{:?}{}", t.kind, if t.suspended { " (suspended)" } else { "" }).to_lowercase()).collect(),
            ai_autonomy: d.ai_policy.autonomy,
            steps: d.steps.len(),
            updated_at: d.updated_at.unwrap_or_else(Utc::now),
            running_instances: store::running_count(&s.core, d.id).await?,
            review_reasons: d.review_reasons.clone(),
        });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

async fn list(State(s): State<AppState>, ctx: RequestContext) -> ApiResult<Vec<WorkflowSummary>> {
    async {
        ctx.user.require("workflows.view")?;
        summaries(&s, ctx.user.tenant_id).await
    }
    .await
    .http()
}

async fn get_one(State(s): State<AppState>, ctx: RequestContext, Path(id): Path<Uuid>) -> ApiResult<WorkflowDefinition> {
    async {
        ctx.user.require("workflows.view")?;
        store::latest(&s.core, ctx.user.tenant_id, id).await
    }
    .await
    .http()
}

async fn versions(State(s): State<AppState>, ctx: RequestContext, Path(id): Path<Uuid>) -> ApiResult<Vec<WorkflowDefinition>> {
    async {
        ctx.user.require("workflows.view")?;
        store::versions(&s.core, ctx.user.tenant_id, id).await
    }
    .await
    .http()
}

async fn validate(State(s): State<AppState>, ctx: RequestContext, ReqJson(def): ReqJson<WorkflowDefinition>) -> ApiResult<ValidateWorkflowResponse> {
    async {
        ctx.user.require("workflows.view")?;
        let catalog = s.engine.catalog.manifests(&s.core).await.unwrap_or_default();
        let mut issues = analysis::validate(&def);
        issues.extend(check_references(&def, &catalog));
        Ok(ValidateWorkflowResponse { dependencies: analysis::dependencies(&def, &catalog), issues })
    }
    .await
    .http()
}

/// References to step types / commands / events that no module declares.
fn check_references(def: &WorkflowDefinition, catalog: &[redhive_contracts::modules::ModuleManifest]) -> Vec<analysis::Issue> {
    let mut issues = Vec::new();
    if catalog.is_empty() {
        return issues;
    }
    let surfaces: Vec<_> = catalog.iter().map(|m| &m.contract_surface).collect();
    for st in def.steps.iter() {
        if let Some(id) = &st.step_type_id {
            if !surfaces.iter().any(|s| s.step_types.iter().any(|x| &x.id == id)) {
                issues.push(analysis::Issue { severity: IssueSeverity::Error, code: "stepType.unknown".into(), step_id: Some(st.id.clone()), message: format!("No module declares the step type '{id}'.") });
            }
        }
        if let Some(id) = &st.command_id {
            match surfaces.iter().flat_map(|s| s.commands.iter()).find(|c| &c.id == id) {
                None => issues.push(analysis::Issue { severity: IssueSeverity::Error, code: "command.unknown".into(), step_id: Some(st.id.clone()), message: format!("No module declares the command '{id}'.") }),
                Some(c) if !c.workflow_capable => issues.push(analysis::Issue { severity: IssueSeverity::Error, code: "command.notWorkflowCapable".into(), step_id: Some(st.id.clone()), message: format!("'{id}' is not workflow-capable.") }),
                Some(c) if c.endpoint.is_none() => issues.push(analysis::Issue { severity: IssueSeverity::Error, code: "command.notHeadless".into(), step_id: Some(st.id.clone()), message: format!("'{id}' has no headless endpoint.") }),
                _ => {}
            }
        }
    }
    for t in &def.triggers {
        if let Some(ev) = &t.event_type {
            if !ev.starts_with("workflow.") && !surfaces.iter().any(|s| s.events.iter().any(|e| &e.event_type == ev)) {
                issues.push(analysis::Issue { severity: IssueSeverity::Warning, code: "event.unknown".into(), step_id: None, message: format!("No module declares the event '{ev}'.") });
            }
        }
    }
    issues
}

/// Saves a new version. Published versions are immutable: every save creates version n+1.
async fn save(State(s): State<AppState>, ctx: RequestContext, ReqJson(req): ReqJson<SaveWorkflowRequest>) -> ApiResult<SaveWorkflowResponse> {
    async {
        ctx.user.require("workflows.edit")?;
        let mut def = req.definition;
        let catalog = s.engine.catalog.manifests(&s.core).await.unwrap_or_default();
        let mut issues = analysis::validate(&def);
        issues.extend(check_references(&def, &catalog));
        if req.publish && issues.iter().any(|i| i.severity == IssueSeverity::Error) {
            let mut e = Error::validation("workflow.invalid", "workflow:definition.invalid").with_message(issues.iter().filter(|i| i.severity == IssueSeverity::Error).map(|i| i.message.clone()).collect::<Vec<_>>().join(" "));
            e.metadata.insert("issues".into(), serde_json::to_value(&issues).unwrap_or_default());
            return Err(e);
        }
        if def.ai_policy.autonomy != Autonomy::Off && !ctx.user.has("ai.run") {
            return Err(Error::forbidden("workflow.ai.permission", "workflow:ai.permission"));
        }
        // Keep the owner of existing workflows; new ones belong to the author.
        if let Ok(existing) = store::latest(&s.core, ctx.user.tenant_id, def.id).await {
            def.owner_id = existing.owner_id.or(Some(ctx.user.id));
            if existing.origin == WorkflowOrigin::ModuleTemplate {
                def.origin = WorkflowOrigin::ModuleTemplate;
                def.owning_module = existing.owning_module;
            }
        } else {
            def.owner_id = Some(ctx.user.id);
        }
        def.status = if req.publish { WorkflowStatus::Active } else { WorkflowStatus::Draft };
        def.review_reasons.clear();
        if req.publish {
            // A reviewed workflow gets its suspended triggers back.
            for t in def.triggers.iter_mut() {
                t.suspended = false;
            }
        }
        def.updated_at = Some(Utc::now());
        def.dependencies = analysis::dependencies(&def, &catalog);
        let mut tx = s.core.db.begin().await?;
        def.version = store::next_version(&mut tx, def.id).await?;
        store::insert(&mut tx, ctx.user.tenant_id, &def).await?;
        s.audit.write(&mut tx, &ctx.user, if req.publish { "workflow.published" } else { "workflow.saved" }, "workflow", &def.id.to_string(), json!({ "version": def.version, "name": def.name })).await?;
        tx.commit().await.db_err("workflow.commit")?;
        Ok(SaveWorkflowResponse { id: def.id, version: def.version, status: def.status, issues })
    }
    .await
    .http()
}

async fn retire(State(s): State<AppState>, ctx: RequestContext, Path(id): Path<Uuid>) -> ApiResult<SaveWorkflowResponse> {
    async {
        ctx.user.require("workflows.edit")?;
        let mut def = store::latest(&s.core, ctx.user.tenant_id, id).await?;
        def.status = WorkflowStatus::Retired;
        def.updated_at = Some(Utc::now());
        let mut tx = s.core.db.begin().await?;
        def.version = store::next_version(&mut tx, id).await?;
        store::insert(&mut tx, ctx.user.tenant_id, &def).await?;
        s.audit.write(&mut tx, &ctx.user, "workflow.retired", "workflow", &id.to_string(), json!({ "version": def.version })).await?;
        tx.commit().await.db_err("workflow.commit")?;
        Ok(SaveWorkflowResponse { id, version: def.version, status: def.status, issues: vec![] })
    }
    .await
    .http()
}

async fn start(State(s): State<AppState>, ctx: RequestContext, Path(id): Path<Uuid>, OptJson(body): OptJson<StartWorkflowRequest>) -> ApiResult<WorkflowInstance> {
    async {
        ctx.user.require("workflows.run")?;
        let req = body;
        let def = store::latest(&s.core, ctx.user.tenant_id, id).await?;
        if def.status != WorkflowStatus::Active {
            return Err(Error::conflict("workflow.notActive", "workflow:definition.notActive").with_metadata("status", def.status.as_str()));
        }
        let executor = req.executor.unwrap_or_default();
        if executor == WorkflowExecutor::Ai {
            ctx.user.require("ai.run")?;
        }
        let info = TriggerInfo { trigger_id: "manual".into(), kind: "manual".into(), event_id: None, payload: json!({ "startedBy": ctx.user.display_name }) };
        s.engine.start(&def, ctx.user.tenant_id, ctx.user.id, executor, req.vars, Some(info), None).await
    }
    .await
    .http()
}

async fn designer_catalog(State(s): State<AppState>, ctx: RequestContext) -> ApiResult<catalog::DesignerCatalog> {
    async {
        ctx.user.require("workflows.view")?;
        Ok(catalog::designer(s.engine.catalog.manifests(&s.core).await?))
    }
    .await
    .http()
}

// ------------------------------------------------------------------ instances & inbox

fn summary(i: &WorkflowInstance) -> InstanceSummary {
    InstanceSummary {
        id: i.id,
        workflow_id: i.workflow_id,
        workflow_name: i.workflow_name.clone(),
        workflow_version: i.workflow_version,
        state: i.state,
        current_step_id: i.current_step_id.clone(),
        executor: i.executor,
        run_as_name: i.run_as_name.clone(),
        created_at: i.created_at,
        updated_at: i.updated_at,
    }
}

async fn inbox(State(s): State<AppState>, ctx: RequestContext) -> ApiResult<Vec<InboxItem>> {
    async {
        ctx.user.require("workflows.run")?;
        let waiting = store::instances_where(&s.core, "state in ('waitingForInput', 'waitingForApproval')", Some(ctx.user.tenant_id), 500).await?;
        Ok(waiting
            .into_iter()
            .filter(|i| Engine::may_complete(&ctx.user, i))
            .filter_map(|i| {
                let task = i.pending.clone()?;
                Some(InboxItem {
                    instance_id: i.id,
                    workflow_id: i.workflow_id,
                    workflow_name: i.workflow_name.clone(),
                    state: i.state,
                    executor: i.executor,
                    run_as_name: i.run_as_name.clone(),
                    vars: i.vars.iter().filter(|(k, _)| !k.starts_with("__")).map(|(k, v)| (k.clone(), v.clone())).collect(),
                    created_at: task.created_at,
                    task,
                })
            })
            .collect())
    }
    .await
    .http()
}

#[derive(Deserialize, Default)]
struct InstanceQuery {
    #[serde(default)]
    state: Option<String>,
    #[serde(default)]
    workflow: Option<Uuid>,
    #[serde(default)]
    mine: Option<bool>,
}

async fn instances(State(s): State<AppState>, ctx: RequestContext, Query(q): Query<InstanceQuery>) -> ApiResult<Vec<InstanceSummary>> {
    async {
        ctx.user.require("workflows.view")?;
        let mut filter = vec!["true".to_string()];
        if let Some(st) = q.state.as_deref().filter(|s| !s.is_empty() && *s != "all") {
            let st = InstanceState::parse(st).ok_or_else(|| Error::validation("state.invalid", "errors:invalidValue").for_field("state"))?;
            filter.push(format!("state = '{}'", st.as_str()));
        }
        if let Some(w) = q.workflow {
            filter.push(format!("workflow_id = '{w}'"));
        }
        if q.mine.unwrap_or(false) || !ctx.user.has("workflows.edit") {
            filter.push(format!("run_as = '{}'", ctx.user.id));
        }
        Ok(store::instances_where(&s.core, &filter.join(" and "), Some(ctx.user.tenant_id), 300).await?.iter().map(summary).collect())
    }
    .await
    .http()
}

async fn instance(State(s): State<AppState>, ctx: RequestContext, Path(id): Path<Uuid>) -> ApiResult<WorkflowInstance> {
    async {
        ctx.user.require("workflows.view")?;
        let i = store::load_instance(&s.core, id).await?;
        if i.tenant_id != ctx.user.tenant_id || (!ctx.user.has("workflows.edit") && i.run_as != ctx.user.id && !Engine::may_complete(&ctx.user, &i)) {
            return Err(Error::not_found("workflow.instance", "workflow:instance.notFound"));
        }
        Ok(i)
    }
    .await
    .http()
}

async fn complete(State(s): State<AppState>, ctx: RequestContext, Path(id): Path<Uuid>, ReqJson(req): ReqJson<CompleteTaskRequest>) -> ApiResult<WorkflowInstance> {
    s.engine.complete(&ctx.user, id, req).await.http()
}

async fn cancel(State(s): State<AppState>, ctx: RequestContext, Path(id): Path<Uuid>) -> ApiResult<WorkflowInstance> {
    s.engine.cancel(&ctx.user, id).await.http()
}

async fn resume(State(s): State<AppState>, ctx: RequestContext, Path(id): Path<Uuid>) -> ApiResult<WorkflowInstance> {
    s.engine.resume(&ctx.user, id).await.http()
}

/// Clients report executed workflow-capable commands here (command triggers, recorder).
async fn command_executed(State(s): State<AppState>, ctx: RequestContext, ReqJson(ev): ReqJson<CommandExecutedEvent>) -> ApiResult<Value> {
    async {
        let started = triggers::on_command(&s.engine, &ctx.user, &ev.command_id, ev.parameter).await?;
        Ok(json!({ "started": started }))
    }
    .await
    .http()
}

// ------------------------------------------------------------------ impact

async fn impact_reports(State(s): State<AppState>, ctx: RequestContext) -> ApiResult<Vec<ImpactReport>> {
    async {
        ctx.user.require("workflows.view")?;
        store::reports(&s.core).await
    }
    .await
    .http()
}

async fn impact_report(State(s): State<AppState>, ctx: RequestContext, Path(id): Path<Uuid>) -> ApiResult<ImpactReport> {
    async {
        ctx.user.require("workflows.view")?;
        store::report(&s.core, id).await
    }
    .await
    .http()
}

async fn analyze_impact(State(s): State<AppState>, _call: InternalCall, ReqJson(req): ReqJson<AnalyzeImpactRequest>) -> ApiResult<AnalyzeImpactResponse> {
    impact::run(&s.engine, req).await.http()
}

/// Every platform event, in order, from the NotificationPoint.
async fn events(State(s): State<AppState>, _call: InternalCall, ReqJson(env): ReqJson<NotificationEnvelope>) -> ApiResult<Value> {
    // Start triggered workflows in the background so the NotificationPoint is not blocked by long steps.
    let engine = s.engine.clone();
    let id = env.id;
    tokio::spawn(async move {
        if let Err(e) = triggers::on_event(&engine, env).await {
            tracing::warn!(error = %e, "event handling failed");
        }
    });
    Ok(Json(json!({ "accepted": id })))
}

/// Loads the workflow templates shipped with the modules (`<modules dir>/*/workflows/*.json`) that are not
/// in the database yet. Retries until the registry answers (dependencies need the catalogue).
fn seed_templates(s: AppState) {
    tokio::spawn(async move {
        let dir = s.core.options.var("MODULES_DIR").unwrap_or_else(|| "modules".into());
        let mut defs = Vec::new();
        if let Ok(modules) = std::fs::read_dir(&dir) {
            for m in modules.flatten() {
                if let Ok(files) = std::fs::read_dir(m.path().join("workflows")) {
                    for f in files.flatten() {
                        match std::fs::read_to_string(f.path()).map_err(|e| e.to_string()).and_then(|t| serde_json::from_str::<WorkflowDefinition>(&t).map_err(|e| e.to_string())) {
                            Ok(d) => defs.push(d),
                            Err(e) => tracing::warn!(file = %f.path().display(), error = %e, "workflow template does not parse"),
                        }
                    }
                }
            }
        }
        if defs.is_empty() {
            return;
        }
        let mut catalog = Vec::new();
        for _ in 0..60 {
            match s.engine.catalog.manifests(&s.core).await {
                Ok(c) if !c.is_empty() => {
                    catalog = c;
                    break;
                }
                _ => tokio::time::sleep(Duration::from_secs(2)).await,
            }
        }
        for mut d in defs {
            if store::latest(&s.core, ids::DEMO_TENANT, d.id).await.is_ok() {
                continue;
            }
            d.dependencies = analysis::dependencies(&d, &catalog);
            d.updated_at = Some(Utc::now());
            let r: Res<()> = async {
                let mut tx = s.core.db.begin().await?;
                store::insert(&mut tx, ids::DEMO_TENANT, &d).await?;
                tx.commit().await.db_err("workflow.seed")
            }
            .await;
            match r {
                Ok(()) => tracing::info!(workflow = %d.name, "workflow template installed"),
                Err(e) => tracing::warn!(workflow = %d.name, error = %e, "workflow template could not be installed"),
            }
        }
    });
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let core = Core::init("workflow-service").await?;
    let engine = Engine::new(core.clone(), Catalog::default());
    let state = AppState { core: core.clone(), engine: engine.clone(), audit: AuditLog::new("workflow-service") };
    seed_templates(state.clone());
    {
        let e = engine.clone();
        every("workflow.instances", Duration::from_secs(5), move || {
            let e = e.clone();
            async move { triggers::tick_instances(&e).await }
        });
        let e = engine.clone();
        let conditions = Arc::new(Mutex::new(HashMap::new()));
        every("workflow.triggers", Duration::from_secs(30), move || {
            let e = e.clone();
            let c = conditions.clone();
            async move { triggers::tick_triggers(&e, &c).await }
        });
    }
    let router = Router::new()
        .route("/api/workflows", get(list).post(save))
        .route("/api/workflows/validate", post(validate))
        .route("/api/workflows/catalog", get(designer_catalog))
        .route("/api/workflows/inbox", get(inbox))
        .route("/api/workflows/commands", post(command_executed))
        .route("/api/workflows/instances", get(instances))
        .route("/api/workflows/instances/{id}", get(instance))
        .route("/api/workflows/instances/{id}/complete", post(complete))
        .route("/api/workflows/instances/{id}/cancel", post(cancel))
        .route("/api/workflows/instances/{id}/resume", post(resume))
        .route("/api/workflows/impact", get(impact_reports))
        .route("/api/workflows/impact/{id}", get(impact_report))
        .route("/api/workflows/{id}", get(get_one))
        .route("/api/workflows/{id}/versions", get(versions))
        .route("/api/workflows/{id}/start", post(start))
        .route("/api/workflows/{id}/retire", post(retire))
        .route("/internal/impact", post(analyze_impact))
        .route("/internal/events", post(events));
    serve(core, router, state).await
}
