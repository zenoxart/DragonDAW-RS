//! Red Hive module registry: module versions, assignment per user, publishing with contract-surface diff,
//! workflow impact analysis and rollout start, and the gateway's dynamic route table.

use axum::extract::{Path, State};
use axum::routing::{get, post, put};
use axum::Router;
use chrono::{DateTime, Utc};
use redhive_backend_core::db::DbErrorExt;
use redhive_backend_core::hosting::{serve, Core};
use redhive_backend_core::{ApiResult, AuditLog, Auth, InternalCall, OutboxWriter, RequestContext, ToHttp, PLATFORM_VERSION, ReqJson};
use redhive_contracts::api::PermissionRegistration;
use redhive_contracts::modules::*;
use redhive_contracts::notifications::{event_types, NotificationAudience, Severity};
use redhive_contracts::workflows::api::{AnalyzeImpactRequest, AnalyzeImpactResponse};
use redhive_monads::{Error, OptionExt, Res};
use serde_json::{json, Map, Value};
use sqlx::Row;
use std::time::Duration;

const SERVICE: &str = "module-registry";

#[derive(Clone)]
struct AppState {
    core: Core,
    outbox: OutboxWriter,
    audit: AuditLog,
}

impl axum::extract::FromRef<AppState> for Core {
    fn from_ref(s: &AppState) -> Core {
        s.core.clone()
    }
}

fn to_info(r: &sqlx::postgres::PgRow) -> Res<ModuleVersionInfo> {
    let manifest: ModuleManifest = serde_json::from_value(r.get("manifest")).map_err(|e| Error::unexpected("registry.manifest", "errors:unexpected").with_message(e.to_string()))?;
    Ok(ModuleVersionInfo {
        module_id: r.get("module_id"),
        version: r.get("version"),
        channel: r.get("channel"),
        status: r.get("status"),
        published_at: r.get::<DateTime<Utc>, _>("published_at"),
        published_by: r.get("published_by"),
        manifest,
    })
}

const COLUMNS: &str = "module_id, version, channel, status, manifest, published_at, published_by";

/// Latest published version of every module.
async fn latest_all(core: &Core) -> Res<Vec<ModuleVersionInfo>> {
    let rows = sqlx::query(&format!(
        "select distinct on (module_id) {COLUMNS} from registry.modules where status = 'published' order by module_id, major desc, minor desc, patch desc"
    ))
    .fetch_all(core.db.pool())
    .await
    .db_err("registry.latest")?;
    rows.iter().map(to_info).collect()
}

async fn latest(core: &Core, module_id: &str) -> Res<Option<ModuleVersionInfo>> {
    let row = sqlx::query(&format!("select {COLUMNS} from registry.modules where module_id = $1 and status = 'published' order by major desc, minor desc, patch desc limit 1"))
        .bind(module_id)
        .fetch_optional(core.db.pool())
        .await
        .db_err("registry.latest")?;
    row.as_ref().map(to_info).transpose()
}

async fn version(core: &Core, module_id: &str, version: &str) -> Res<ModuleVersionInfo> {
    let row = sqlx::query(&format!("select {COLUMNS} from registry.modules where module_id = $1 and version = $2"))
        .bind(module_id)
        .bind(version)
        .fetch_optional(core.db.pool())
        .await
        .db_err("registry.version")?;
    row.as_ref().map(to_info).transpose()?.ok_or_not_found("registry.version", "modules:version.notFound")
}

async fn insert(core: &Core, tx: &mut redhive_backend_core::db::Tx<'static>, m: &ModuleManifest, channel: &str, by: &str) -> Res<()> {
    let v = parse_version(&m.version)?;
    sqlx::query("insert into registry.modules (module_id, version, major, minor, patch, channel, manifest, published_by) values ($1, $2, $3, $4, $5, $6, $7, $8)")
        .bind(&m.module_id)
        .bind(&m.version)
        .bind(v.major as i32)
        .bind(v.minor as i32)
        .bind(v.patch as i32)
        .bind(channel)
        .bind(serde_json::to_value(m).unwrap_or_default())
        .bind(by)
        .execute(&mut **tx)
        .await
        .db_err("registry.insert")?;
    let _ = core;
    Ok(())
}

// ------------------------------------------------------------------ endpoints

async fn catalog(State(s): State<AppState>, ctx: RequestContext) -> ApiResult<Vec<ModuleVersionInfo>> {
    async {
        ctx.user.require("modules.view")?;
        latest_all(&s.core).await
    }
    .await
    .http()
}

async fn versions(State(s): State<AppState>, ctx: RequestContext, Path(module_id): Path<String>) -> ApiResult<Vec<ModuleVersionInfo>> {
    async {
        ctx.user.require("modules.view")?;
        let rows = sqlx::query(&format!("select {COLUMNS} from registry.modules where module_id = $1 order by major desc, minor desc, patch desc"))
            .bind(&module_id)
            .fetch_all(s.core.db.pool())
            .await
            .db_err("registry.versions")?;
        rows.iter().map(to_info).collect::<Res<Vec<_>>>()
    }
    .await
    .http()
}

/// The modules (and versions) the signed-in user gets: tenant enablement + rollout stage per user.
async fn assigned(State(s): State<AppState>, ctx: RequestContext) -> ApiResult<Vec<AssignedModule>> {
    async {
        let all = latest_all(&s.core).await?;
        let disabled: Vec<String> = sqlx::query_scalar("select module_id from updates.enablement where tenant_id = $1 and not enabled")
            .bind(ctx.user.tenant_id)
            .fetch_all(s.core.db.pool())
            .await
            .db_err("registry.enablement")?;
        let per_user: Map<String, Value> = s
            .core
            .services
            .get_internal("update-orchestrator", &format!("/internal/versions?user={}&roles={}", ctx.user.id, ctx.user.roles.join(",")))
            .await
            .unwrap_or_default();
        let mut out = Vec::new();
        for latest in all.into_iter().filter(|m| !disabled.contains(&m.module_id)) {
            let chosen = match per_user.get(&latest.module_id).and_then(Value::as_str) {
                Some(v) if v != latest.version => version(&s.core, &latest.module_id, v).await.unwrap_or_else(|_| latest.clone()),
                _ => latest.clone(),
            };
            let package: Option<PackageInfo> = s.core.services.get_internal("module-delivery", &format!("/internal/packages/{}/{}", chosen.module_id, chosen.version)).await.ok();
            let newer_version = (chosen.version != latest.version).then(|| latest.version.clone());
            out.push(AssignedModule { manifest: chosen.manifest, package, newer_version });
        }
        Ok(out)
    }
    .await
    .http()
}

/// Publish: validate → authorize → compare with the latest version → register permissions → dry-run
/// impact → store + event → apply impact (migrate / flag workflows) → start the rollout.
async fn publish(State(s): State<AppState>, ctx: RequestContext, ReqJson(req): ReqJson<PublishRequest>) -> ApiResult<PublishResult> {
    async {
        ctx.user.require("modules.publish")?;
        let m = req.manifest;
        check_manifest(&m, PLATFORM_VERSION)?;
        let new_v = parse_version(&m.version)?;
        let previous = latest(&s.core, &m.module_id).await?;
        if let Some(p) = &previous {
            if parse_version(&p.version)? >= new_v {
                return Err(Error::conflict("modules.version.notNewer", "modules:version.notNewer").with_metadata("latest", p.version.clone()));
            }
            if p.manifest.prefix != m.prefix {
                return Err(Error::conflict("modules.prefix.changed", "modules:prefix.changed"));
            }
        }
        let changes = previous.as_ref().map(|p| diff_surfaces(&p.manifest.contract_surface, &m.contract_surface)).unwrap_or_default();

        let _: Value = s.core.services.post_internal("identity-service", "/internal/permissions", &PermissionRegistration { module_id: m.module_id.clone(), permissions: m.permissions.clone() }).await?;

        // Dry run first: if the workflow service is unreachable, nothing has changed yet.
        let impact_req = |apply: bool| AnalyzeImpactRequest {
            module_id: m.module_id.clone(),
            from_version: previous.as_ref().map(|p| p.version.clone()).unwrap_or_default(),
            to_version: m.version.clone(),
            changes: changes.clone(),
            apply,
        };
        let dry: Option<AnalyzeImpactResponse> = match &previous {
            Some(_) => Some(s.core.services.post_internal("workflow-service", "/internal/impact", &impact_req(false)).await?),
            None => None,
        };

        let mut tx = s.core.db.begin().await?;
        insert(&s.core, &mut tx, &m, &req.channel, &ctx.user.audit_name()).await?;
        let ev = s
            .outbox
            .create(event_types::MODULE_PUBLISHED, &ctx.user, NotificationAudience::WithPermission("modules.view".into()), json!({ "moduleId": m.module_id, "version": m.version, "changes": changes.len() }))
            .titled("shell:modules.published", Severity::Success)
            .navigate("shell.modules", Some(m.module_id.clone()));
        let mut ev = ev;
        ev.text_args.insert("module".into(), json!(m.module_id));
        ev.text_args.insert("version".into(), json!(m.version));
        s.outbox.add(&mut tx, &ev).await?;
        s.audit.write(&mut tx, &ctx.user, "module.published", "module", &format!("{}@{}", m.module_id, m.version), json!({ "changes": changes })).await?;
        tx.commit().await.db_err("registry.commit")?;

        let (report_id, broken) = match &previous {
            Some(_) => match s.core.services.post_internal::<AnalyzeImpactResponse, _>("workflow-service", "/internal/impact", &impact_req(true)).await {
                Ok(r) => (Some(r.report_id), r.broken as i32),
                Err(e) => {
                    tracing::error!(error = %e, "impact analysis failed after publish – holding the rollout");
                    (dry.as_ref().map(|d| d.report_id), dry.as_ref().map(|d| d.broken as i32).unwrap_or(0).max(1))
                }
            },
            None => (None, 0),
        };
        let rollout: Res<RolloutInfo> = s
            .core
            .services
            .post_internal(
                "update-orchestrator",
                "/internal/rollouts",
                &StartRolloutRequest {
                    module_id: m.module_id.clone(),
                    from_version: previous.as_ref().map(|p| p.version.clone()),
                    to_version: m.version.clone(),
                    impact_report_id: report_id,
                    broken_workflows: broken,
                    immediate: req.immediate,
                },
            )
            .await;
        let rollout = rollout.map_err(|e| {
            tracing::error!(error = %e, "rollout could not be started");
            e
        })?;
        tracing::info!(module = %m.module_id, version = %m.version, changes = changes.len(), broken, "module published");
        Ok(PublishResult {
            module_id: m.module_id.clone(),
            version: m.version.clone(),
            previous_version: previous.map(|p| p.version),
            changes,
            impact_report_id: report_id,
            rollout_id: Some(rollout.id),
            broken_workflows: broken,
            rollout_stage: Some(rollout.stage),
        })
    }
    .await
    .http()
}

async fn withdraw(State(s): State<AppState>, ctx: RequestContext, Path((module_id, ver)): Path<(String, String)>) -> ApiResult<ModuleVersionInfo> {
    async {
        ctx.user.require("modules.publish")?;
        let mut tx = s.core.db.begin().await?;
        let n = sqlx::query("update registry.modules set status = 'withdrawn' where module_id = $1 and version = $2 and status = 'published'")
            .bind(&module_id)
            .bind(&ver)
            .execute(&mut *tx)
            .await
            .db_err("registry.withdraw")?
            .rows_affected();
        if n == 0 {
            return Err(Error::not_found("registry.version", "modules:version.notFound"));
        }
        s.audit.write(&mut tx, &ctx.user, "module.withdrawn", "module", &format!("{module_id}@{ver}"), json!({})).await?;
        tx.commit().await.db_err("registry.commit")?;
        version(&s.core, &module_id, &ver).await
    }
    .await
    .http()
}

#[derive(serde::Deserialize)]
struct EnabledRequest {
    enabled: bool,
}

async fn set_enabled(State(s): State<AppState>, ctx: RequestContext, Path(module_id): Path<String>, ReqJson(req): ReqJson<EnabledRequest>) -> ApiResult<Value> {
    async {
        ctx.user.require("settings.edit")?;
        sqlx::query("insert into updates.enablement (tenant_id, module_id, enabled) values ($1, $2, $3) on conflict (tenant_id, module_id) do update set enabled = excluded.enabled")
            .bind(ctx.user.tenant_id)
            .bind(&module_id)
            .bind(req.enabled)
            .execute(s.core.db.pool())
            .await
            .db_err("registry.enable")?;
        Ok(json!({ "moduleId": module_id, "enabled": req.enabled }))
    }
    .await
    .http()
}

// ------------------------------------------------------------------ internal

async fn routes(State(s): State<AppState>, _call: InternalCall) -> ApiResult<Vec<RouteInfo>> {
    async {
        Ok(latest_all(&s.core)
            .await?
            .into_iter()
            .filter_map(|m| m.manifest.backend.map(|b| RouteInfo { prefix: b.route, target: b.service, module_id: m.module_id }))
            .collect())
    }
    .await
    .http()
}

async fn manifests(State(s): State<AppState>, _call: InternalCall) -> ApiResult<Vec<ModuleManifest>> {
    async { Ok(latest_all(&s.core).await?.into_iter().map(|m| m.manifest).collect()) }.await.http()
}

async fn internal_version(State(s): State<AppState>, _call: InternalCall, Path((module_id, ver)): Path<(String, String)>) -> ApiResult<ModuleVersionInfo> {
    version(&s.core, &module_id, &ver).await.http()
}

/// Registers the development modules (`<modules dir>/*/manifest.json`) that the registry does not know yet,
/// as their first, immediately available version.
async fn seed(s: AppState) {
    let dir = s.core.options.var("MODULES_DIR").unwrap_or_else(|| "modules".into());
    let Ok(entries) = std::fs::read_dir(&dir) else { return };
    let mut manifests = Vec::new();
    for e in entries.flatten() {
        if let Ok(text) = std::fs::read_to_string(e.path().join("manifest.json")) {
            match serde_json::from_str::<ModuleManifest>(&text) {
                Ok(m) => manifests.push(m),
                Err(err) => tracing::warn!(path = %e.path().display(), error = %err, "manifest does not parse"),
            }
        }
    }
    for m in manifests {
        match latest(&s.core, &m.module_id).await {
            Ok(None) => {}
            _ => continue,
        }
        if let Err(e) = check_manifest(&m, PLATFORM_VERSION) {
            tracing::warn!(module = %m.module_id, error = %e, "development module rejected");
            continue;
        }
        let stored = async {
            let mut tx = s.core.db.begin().await?;
            insert(&s.core, &mut tx, &m, "stable", "system").await?;
            tx.commit().await.db_err("registry.seed")
        }
        .await;
        match stored {
            Ok(()) => tracing::info!(module = %m.module_id, version = %m.version, "development module registered"),
            Err(e) => {
                tracing::warn!(module = %m.module_id, error = %e, "development module could not be registered");
                continue;
            }
        }
        // Permissions: retry until the identity service is up.
        let reg = PermissionRegistration { module_id: m.module_id.clone(), permissions: m.permissions.clone() };
        let core = s.core.clone();
        tokio::spawn(async move {
            for _ in 0..30 {
                if core.services.post::<Value, _>("identity-service", "/internal/permissions", &reg, &Auth::Internal).await.is_ok() {
                    return;
                }
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
        });
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let core = Core::init(SERVICE).await?;
    let state = AppState { core: core.clone(), outbox: OutboxWriter::new(SERVICE), audit: AuditLog::new(SERVICE) };
    seed(state.clone()).await;
    let router = Router::new()
        .route("/api/modules", get(catalog).post(publish))
        .route("/api/modules/assigned", get(assigned))
        .route("/api/modules/{id}/versions", get(versions))
        .route("/api/modules/{id}/enabled", put(set_enabled))
        .route("/api/modules/{id}/{version}/withdraw", post(withdraw))
        .route("/internal/routes", get(routes))
        .route("/internal/manifests", get(manifests))
        .route("/internal/modules/{id}/{version}", get(internal_version));
    serve(core, router, state).await
}
