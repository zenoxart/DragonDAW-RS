//! Red Hive update orchestrator: staged rollouts (Pilot → Broad → Complete), holds while workflows would
//! break, rollback (cancel), and the `modules.version.available` notices that make clients hot-swap.

mod planner;

use axum::extract::{Path, State};
use axum::routing::{get, post};
use axum::Router;
use chrono::{DateTime, Utc};
use planner::*;
use redhive_backend_core::db::DbErrorExt;
use redhive_backend_core::hosting::{serve, Core};
use redhive_backend_core::{ApiResult, AuditLog, InternalCall, OutboxWriter, RequestContext, ToHttp, OptJson, ReqJson};
use redhive_contracts::modules::*;
use redhive_contracts::notifications::{event_types, NotificationAudience, NotificationEnvelope, Severity};
use redhive_contracts::primitives::ids;
use redhive_monads::{OptionExt, Res};
use serde_json::{json, Value};
use sqlx::Row;
use uuid::Uuid;

const SERVICE: &str = "update-orchestrator";

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

const SELECT: &str = "select id, module_id, from_version, to_version, stage, hold_reason, impact_report_id, broken_workflows, pilot_users, broad_roles, created_at, updated_at from updates.rollouts";

fn to_info(r: &sqlx::postgres::PgRow) -> RolloutInfo {
    RolloutInfo {
        id: r.get("id"),
        module_id: r.get("module_id"),
        from_version: r.get("from_version"),
        to_version: r.get("to_version"),
        stage: RolloutStage::parse(r.get::<String, _>("stage").as_str()).unwrap_or(RolloutStage::Held),
        hold_reason: r.get("hold_reason"),
        impact_report_id: r.get("impact_report_id"),
        broken_workflows: r.get("broken_workflows"),
        pilot_users: r.get("pilot_users"),
        broad_roles: r.get("broad_roles"),
        created_at: r.get::<DateTime<Utc>, _>("created_at"),
        updated_at: r.get::<DateTime<Utc>, _>("updated_at"),
    }
}

async fn load(core: &Core, id: Uuid) -> Res<RolloutInfo> {
    let r = sqlx::query(&format!("{SELECT} where id = $1")).bind(id).fetch_optional(core.db.pool()).await.db_err("updates.rollout")?;
    r.as_ref().map(to_info).ok_or_not_found("updates.rollout", "updates:rollout.notFound")
}

/// Pilot users and broad roles come from the configuration service (`updates.pilotUsers`, `updates.broadRoles`).
async fn audiences(core: &Core, tenant: Uuid) -> (Vec<Uuid>, Vec<String>) {
    let pilot: Value = core.services.get_internal("configuration-service", &format!("/internal/settings/updates.pilotUsers?tenant={tenant}")).await.unwrap_or(Value::Null);
    let broad: Value = core.services.get_internal("configuration-service", &format!("/internal/settings/updates.broadRoles?tenant={tenant}")).await.unwrap_or(Value::Null);
    (serde_json::from_value(pilot).unwrap_or_default(), serde_json::from_value(broad).unwrap_or_default())
}

/// Builds the version notice for the rollout's current stage.
fn notice(state: &AppState, r: &RolloutInfo, hot_swap: HotSwapMode) -> NotificationEnvelope {
    let (version, users) = match r.stage {
        RolloutStage::Pilot => (r.to_version.clone(), r.pilot_users.clone()),
        RolloutStage::Cancelled => (r.from_version.clone().unwrap_or_else(|| r.to_version.clone()), vec![]),
        _ => (r.to_version.clone(), vec![]),
    };
    let payload = VersionAvailable { module_id: r.module_id.clone(), version: version.clone(), previous_version: r.from_version.clone(), stage: r.stage, hot_swap, users: users.clone() };
    let audience = match users.as_slice() {
        [single] => NotificationAudience::User(*single),
        _ => NotificationAudience::Tenant,
    };
    let mut e = NotificationEnvelope::new(event_types::MODULE_VERSION_AVAILABLE, SERVICE, ids::DEMO_TENANT, audience, serde_json::to_value(payload).unwrap_or_default());
    e = e.titled("shell:updates.available", Severity::Info).navigate("shell.modules", Some(r.module_id.clone()));
    let mut args = serde_json::Map::new();
    args.insert("module".into(), json!(r.module_id));
    args.insert("version".into(), json!(version));
    let _ = state;
    e.with_args(args)
}

async fn hot_swap_mode(core: &Core, module_id: &str, version: &str) -> HotSwapMode {
    let r: Res<ModuleVersionInfo> = core.services.get_internal("module-registry", &format!("/internal/modules/{module_id}/{version}")).await;
    r.map(|m| m.manifest.client.hot_swap).unwrap_or_default()
}

/// Persists a stage change and emits the notices in one transaction.
async fn save_stage(s: &AppState, user: Option<&redhive_backend_core::RedHiveUser>, r: &RolloutInfo, stage: RolloutStage, hold_reason: Option<String>) -> Res<RolloutInfo> {
    let mut tx = s.core.db.begin().await?;
    sqlx::query("update updates.rollouts set stage = $2, hold_reason = $3, updated_at = now() where id = $1")
        .bind(r.id)
        .bind(stage.as_str())
        .bind(&hold_reason)
        .execute(&mut *tx)
        .await
        .db_err("updates.stage")?;
    let mut updated = r.clone();
    updated.stage = stage;
    updated.hold_reason = hold_reason;
    let changed = NotificationEnvelope::new(event_types::ROLLOUT_CHANGED, SERVICE, ids::DEMO_TENANT, NotificationAudience::WithPermission("updates.manage".into()), serde_json::to_value(&updated).unwrap_or_default());
    s.outbox.add(&mut tx, &changed).await?;
    if matches!(stage, RolloutStage::Pilot | RolloutStage::Broad | RolloutStage::Complete | RolloutStage::Cancelled) {
        let mode = hot_swap_mode(&s.core, &r.module_id, &r.to_version).await;
        s.outbox.add(&mut tx, &notice(s, &updated, mode)).await?;
    }
    if let Some(u) = user {
        s.audit.write(&mut tx, u, &format!("rollout.{}", stage.as_str().to_lowercase()), "rollout", &r.id.to_string(), json!({ "module": r.module_id, "version": r.to_version })).await?;
    }
    tx.commit().await.db_err("updates.commit")?;
    Ok(updated)
}

// ------------------------------------------------------------------ endpoints

async fn list(State(s): State<AppState>, ctx: RequestContext) -> ApiResult<Vec<RolloutInfo>> {
    async {
        ctx.user.require("modules.view")?;
        let rows = sqlx::query(&format!("{SELECT} order by created_at desc limit 200")).fetch_all(s.core.db.pool()).await.db_err("updates.list")?;
        Ok(rows.iter().map(to_info).collect())
    }
    .await
    .http()
}

async fn get_one(State(s): State<AppState>, ctx: RequestContext, Path(id): Path<Uuid>) -> ApiResult<RolloutInfo> {
    async {
        ctx.user.require("modules.view")?;
        load(&s.core, id).await
    }
    .await
    .http()
}

async fn advance(State(s): State<AppState>, ctx: RequestContext, Path(id): Path<Uuid>, OptJson(body): OptJson<AdvanceRolloutRequest>) -> ApiResult<RolloutInfo> {
    async {
        ctx.user.require("updates.manage")?;
        let r = load(&s.core, id).await?;
        let force = body.force;
        let next = advance_stage(&r, force)?;
        save_stage(&s, Some(&ctx.user), &r, next, None).await
    }
    .await
    .http()
}

async fn hold(State(s): State<AppState>, ctx: RequestContext, Path(id): Path<Uuid>) -> ApiResult<RolloutInfo> {
    async {
        ctx.user.require("updates.manage")?;
        let r = load(&s.core, id).await?;
        let next = hold_stage(&r)?;
        save_stage(&s, Some(&ctx.user), &r, next, Some(format!("Held by {}", ctx.user.display_name))).await
    }
    .await
    .http()
}

/// Cancels a rollout: clients that already run the new version swap back to the previous one (rollback).
async fn cancel(State(s): State<AppState>, ctx: RequestContext, Path(id): Path<Uuid>) -> ApiResult<RolloutInfo> {
    async {
        ctx.user.require("updates.manage")?;
        let r = load(&s.core, id).await?;
        let next = cancel_stage(&r)?;
        save_stage(&s, Some(&ctx.user), &r, next, Some(format!("Cancelled by {}", ctx.user.display_name))).await
    }
    .await
    .http()
}

/// Registry → orchestrator: start the rollout of a newly published version.
async fn start(State(s): State<AppState>, _call: InternalCall, ReqJson(req): ReqJson<StartRolloutRequest>) -> ApiResult<RolloutInfo> {
    async {
        let (pilot, broad) = audiences(&s.core, ids::DEMO_TENANT).await;
        let (stage, reason) = start_stage(req.broken_workflows, req.immediate || req.from_version.is_none());
        let id = Uuid::new_v4();
        let mut tx = s.core.db.begin().await?;
        // A newer rollout supersedes unfinished ones of the same module.
        sqlx::query("update updates.rollouts set stage = 'Cancelled', hold_reason = 'Superseded', updated_at = now() where module_id = $1 and stage in ('Pilot', 'Broad', 'Held')")
            .bind(&req.module_id)
            .execute(&mut *tx)
            .await
            .db_err("updates.supersede")?;
        sqlx::query(
            "insert into updates.rollouts (id, module_id, from_version, to_version, stage, hold_reason, impact_report_id, broken_workflows, pilot_users, broad_roles)
             values ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
        )
        .bind(id)
        .bind(&req.module_id)
        .bind(&req.from_version)
        .bind(&req.to_version)
        .bind(stage.as_str())
        .bind(&reason)
        .bind(req.impact_report_id)
        .bind(req.broken_workflows)
        .bind(&pilot)
        .bind(&broad)
        .execute(&mut *tx)
        .await
        .db_err("updates.start")?;
        tx.commit().await.db_err("updates.commit")?;
        let r = load(&s.core, id).await?;
        // Emit notices for the initial stage.
        let mode = hot_swap_mode(&s.core, &r.module_id, &r.to_version).await;
        let mut tx = s.core.db.begin().await?;
        let changed = NotificationEnvelope::new(event_types::ROLLOUT_CHANGED, SERVICE, ids::DEMO_TENANT, NotificationAudience::WithPermission("updates.manage".into()), serde_json::to_value(&r).unwrap_or_default());
        s.outbox.add(&mut tx, &changed).await?;
        if r.stage != RolloutStage::Held {
            s.outbox.add(&mut tx, &notice(&s, &r, mode)).await?;
        } else {
            let mut held = NotificationEnvelope::new("updates.rollout.held", SERVICE, ids::DEMO_TENANT, NotificationAudience::WithPermission("updates.manage".into()), serde_json::to_value(&r).unwrap_or_default())
                .titled("shell:updates.held", Severity::Warning)
                .navigate("workflows.impact", r.impact_report_id.map(|i| i.to_string()));
            held.text_args.insert("module".into(), json!(r.module_id));
            held.text_args.insert("count".into(), json!(r.broken_workflows));
            s.outbox.add(&mut tx, &held).await?;
        }
        tx.commit().await.db_err("updates.commit")?;
        tracing::info!(module = %r.module_id, version = %r.to_version, stage = r.stage.as_str(), "rollout started");
        Ok(r)
    }
    .await
    .http()
}

/// The latest rollout per module (registry uses it to pick the version per user).
async fn active(State(s): State<AppState>, _call: InternalCall) -> ApiResult<Vec<RolloutInfo>> {
    async {
        let rows = sqlx::query(&format!("select distinct on (module_id) * from ({SELECT}) r order by module_id, created_at desc")).fetch_all(s.core.db.pool()).await.db_err("updates.active")?;
        Ok(rows.iter().map(to_info).collect())
    }
    .await
    .http()
}

#[derive(serde::Deserialize)]
struct VersionsQuery {
    user: Uuid,
    #[serde(default)]
    roles: String,
}

/// `{moduleId: version}` the user gets according to the latest rollout of each module.
async fn versions(State(s): State<AppState>, _call: InternalCall, axum::extract::Query(q): axum::extract::Query<VersionsQuery>) -> ApiResult<serde_json::Map<String, Value>> {
    async {
        let roles: Vec<String> = q.roles.split(',').filter(|r| !r.is_empty()).map(str::to_string).collect();
        let rows = sqlx::query(&format!("select distinct on (module_id) * from ({SELECT}) r order by module_id, created_at desc")).fetch_all(s.core.db.pool()).await.db_err("updates.active")?;
        Ok(rows.iter().map(to_info).map(|r| (r.module_id.clone(), Value::String(version_for(&r, q.user, &roles)))).collect())
    }
    .await
    .http()
}

async fn internal_get(State(s): State<AppState>, _call: InternalCall, Path(id): Path<Uuid>) -> ApiResult<RolloutInfo> {
    load(&s.core, id).await.http()
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let core = Core::init(SERVICE).await?;
    let state = AppState { core: core.clone(), outbox: OutboxWriter::new(SERVICE), audit: AuditLog::new(SERVICE) };
    let router = Router::new()
        .route("/api/updates/rollouts", get(list))
        .route("/api/updates/rollouts/{id}", get(get_one))
        .route("/api/updates/rollouts/{id}/advance", post(advance))
        .route("/api/updates/rollouts/{id}/hold", post(hold))
        .route("/api/updates/rollouts/{id}/cancel", post(cancel))
        .route("/internal/rollouts", post(start))
        .route("/internal/rollouts/active", get(active))
        .route("/internal/rollouts/{id}", get(internal_get))
        .route("/internal/versions", get(versions));
    serve(core, router, state).await
}
