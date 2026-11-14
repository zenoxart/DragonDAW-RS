//! Red Hive configuration service: tenant and user settings (user values override tenant values).

use axum::extract::{Path, Query, State};
use axum::routing::{get, put};
use axum::Router;
use redhive_backend_core::db::DbErrorExt;
use redhive_backend_core::hosting::{serve, Core};
use redhive_backend_core::{ApiResult, AuditLog, InternalCall, OutboxWriter, RequestContext, ToHttp, ReqJson};
use redhive_contracts::api::{SettingDto, SettingUpdate};
use redhive_contracts::notifications::{event_types, NotificationAudience};
use redhive_monads::{Error, OptionExt, Res};
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::Row;
use uuid::Uuid;

const SERVICE: &str = "configuration-service";

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

fn row_to_dto(r: &sqlx::postgres::PgRow) -> SettingDto {
    SettingDto { scope: r.get("scope"), owner: r.get("owner"), key: r.get("key"), value: r.get("value"), updated_at: r.get("updated_at"), updated_by: r.get("updated_by") }
}

/// Effective value: the user's own value, else the tenant's.
async fn resolve(core: &Core, tenant: Uuid, user: Option<Uuid>, key: &str) -> Res<Option<SettingDto>> {
    let row = sqlx::query(
        "select scope, owner, key, value, updated_at, updated_by from config.settings
         where tenant_id = $1 and key = $2 and (scope = 'tenant' or (scope = 'user' and owner = $3))
         order by case scope when 'user' then 0 else 1 end limit 1",
    )
    .bind(tenant)
    .bind(key)
    .bind(user.map(|u| u.to_string()).unwrap_or_default())
    .fetch_optional(core.db.pool())
    .await
    .db_err("config.get")?;
    Ok(row.as_ref().map(row_to_dto))
}

async fn list(State(s): State<AppState>, ctx: RequestContext) -> ApiResult<Vec<SettingDto>> {
    async {
        let rows = sqlx::query(
            "select scope, owner, key, value, updated_at, updated_by from config.settings
             where tenant_id = $1 and (scope = 'tenant' or (scope = 'user' and owner = $2)) order by key, scope",
        )
        .bind(ctx.user.tenant_id)
        .bind(ctx.user.id.to_string())
        .fetch_all(s.core.db.pool())
        .await
        .db_err("config.list")?;
        Ok(rows.iter().map(row_to_dto).collect())
    }
    .await
    .http()
}

async fn get_one(State(s): State<AppState>, ctx: RequestContext, Path(key): Path<String>) -> ApiResult<SettingDto> {
    async { resolve(&s.core, ctx.user.tenant_id, Some(ctx.user.id), &key).await?.ok_or_not_found("config.setting", "config:setting.notFound") }.await.http()
}

/// Sets a value. Tenant scope needs `settings.edit`; user scope always targets the caller.
async fn put_one(State(s): State<AppState>, ctx: RequestContext, Path((scope, key)): Path<(String, String)>, ReqJson(req): ReqJson<SettingUpdate>) -> ApiResult<SettingDto> {
    async {
        let owner = match scope.as_str() {
            "tenant" => {
                ctx.user.require("settings.edit")?;
                String::new()
            }
            "user" => ctx.user.id.to_string(),
            _ => return Err(Error::validation("config.scope.invalid", "config:scope.invalid").for_field("scope")),
        };
        if key.is_empty() || key.len() > 120 || !key.chars().all(|c| c.is_ascii_alphanumeric() || ".-_".contains(c)) {
            return Err(Error::validation("config.key.invalid", "config:key.invalid").for_field("key"));
        }
        if serde_json::to_vec(&req.value).map(|v| v.len()).unwrap_or(0) > 64 * 1024 {
            return Err(Error::validation("config.value.tooLarge", "config:value.tooLarge"));
        }
        let mut tx = s.core.db.begin().await?;
        let row = sqlx::query(
            "insert into config.settings (tenant_id, scope, owner, key, value, updated_by) values ($1, $2, $3, $4, $5, $6)
             on conflict (tenant_id, scope, owner, key) do update set value = excluded.value, updated_at = now(), updated_by = excluded.updated_by
             returning scope, owner, key, value, updated_at, updated_by",
        )
        .bind(ctx.user.tenant_id)
        .bind(&scope)
        .bind(&owner)
        .bind(&key)
        .bind(&req.value)
        .bind(ctx.user.audit_name())
        .fetch_one(&mut *tx)
        .await
        .db_err("config.put")?;
        let audience = if scope == "tenant" { NotificationAudience::Tenant } else { NotificationAudience::User(ctx.user.id) };
        let ev = s.outbox.create(event_types::SETTINGS_CHANGED, &ctx.user, audience, json!({ "scope": scope, "key": key, "value": req.value }));
        s.outbox.add(&mut tx, &ev).await?;
        s.audit.write(&mut tx, &ctx.user, "setting.changed", "setting", &format!("{scope}:{key}"), json!({ "value": req.value })).await?;
        tx.commit().await.db_err("config.commit")?;
        Ok(row_to_dto(&row))
    }
    .await
    .http()
}

#[derive(Deserialize)]
struct InternalQuery {
    tenant: Uuid,
    #[serde(default)]
    user: Option<Uuid>,
}

/// Effective value for other services; `null` when unset.
async fn internal_get(State(s): State<AppState>, _call: InternalCall, Path(key): Path<String>, Query(q): Query<InternalQuery>) -> ApiResult<Value> {
    async { Ok(resolve(&s.core, q.tenant, q.user, &key).await?.map(|d| d.value).unwrap_or(Value::Null)) }.await.http()
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let core = Core::init(SERVICE).await?;
    let state = AppState { core: core.clone(), outbox: OutboxWriter::new(SERVICE), audit: AuditLog::new(SERVICE) };
    let router = Router::new()
        .route("/api/config", get(list))
        .route("/api/config/{key}", get(get_one))
        .route("/api/config/{scope}/{key}", put(put_one))
        .route("/internal/settings/{key}", get(internal_get));
    serve(core, router, state).await
}
