//! Red Hive identity service: sign-in, tokens (incl. delegated tokens for the workflow engine and the AI
//! agent), roles and permissions, profile pictures and password changes.

use axum::extract::{Path, Query, State};
use axum::http::header;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, put};
use axum::Router;
use base64::Engine;
use chrono::{DateTime, Utc};
use redhive_backend_core::db::DbErrorExt;
use redhive_backend_core::hosting::{serve, Core};
use redhive_backend_core::security::passwords;
use redhive_backend_core::{ApiResult, AuditLog, InternalCall, NoContent, RedHiveUser, RequestContext, ToHttp, ReqJson};
use redhive_contracts::api::*;
use redhive_monads::{Error, OptionExt, Res};
use serde::Deserialize;
use sqlx::{FromRow, Row};
use uuid::Uuid;

const SERVICE: &str = "identity-service";

#[derive(Clone)]
struct AppState {
    core: Core,
    audit: AuditLog,
}

impl axum::extract::FromRef<AppState> for Core {
    fn from_ref(s: &AppState) -> Core {
        s.core.clone()
    }
}

#[derive(FromRow)]
struct UserRow {
    id: Uuid,
    tenant_id: Uuid,
    tenant_name: String,
    user_name: String,
    display_name: String,
    email: String,
    password_hash: String,
    is_active: bool,
    approval_limit: f64,
    last_login_at: Option<DateTime<Utc>>,
    has_avatar: bool,
}

const USER_SELECT: &str = "select u.id, u.tenant_id, t.name as tenant_name, u.user_name, u.display_name, u.email, u.password_hash, u.is_active,
        u.approval_limit::float8 as approval_limit, u.last_login_at, exists(select 1 from identity.user_avatars a where a.user_id = u.id) as has_avatar
     from identity.users u join identity.tenants t on t.id = u.tenant_id";

async fn roles_and_permissions(core: &Core, user_id: Uuid) -> Res<(Vec<String>, Vec<String>)> {
    let roles: Vec<String> = sqlx::query_scalar("select r.name from identity.user_roles ur join identity.roles r on r.id = ur.role_id where ur.user_id = $1 order by r.name")
        .bind(user_id)
        .fetch_all(core.db.pool())
        .await
        .db_err("identity.roles")?;
    let perms: Vec<String> = sqlx::query_scalar(
        "select distinct rp.permission from identity.user_roles ur join identity.role_permissions rp on rp.role_id = ur.role_id where ur.user_id = $1 order by 1",
    )
    .bind(user_id)
    .fetch_all(core.db.pool())
    .await
    .db_err("identity.permissions")?;
    Ok((roles, perms))
}

async fn load_user(core: &Core, filter: &str, arg: impl ToString) -> Res<Option<UserRow>> {
    let sql = format!("{USER_SELECT} where {filter}");
    let arg = arg.to_string();
    let q = sqlx::query_as::<_, UserRow>(&sql);
    let q = match Uuid::parse_str(&arg) {
        Ok(id) if filter.contains("u.id") => q.bind(id),
        _ => q.bind(arg),
    };
    q.fetch_optional(core.db.pool()).await.db_err("identity.user")
}

async fn profile(core: &Core, u: &UserRow) -> Res<UserProfile> {
    let (roles, permissions) = roles_and_permissions(core, u.id).await?;
    Ok(UserProfile {
        id: u.id,
        tenant_id: u.tenant_id,
        tenant_name: u.tenant_name.clone(),
        user_name: u.user_name.clone(),
        display_name: u.display_name.clone(),
        email: u.email.clone(),
        roles,
        permissions,
        approval_limit: u.approval_limit,
        has_avatar: u.has_avatar,
        last_login_at: u.last_login_at,
    })
}

fn to_user(p: &UserProfile, actor: Option<String>, instance: Option<Uuid>) -> RedHiveUser {
    RedHiveUser {
        id: p.id,
        tenant_id: p.tenant_id,
        user_name: p.user_name.clone(),
        display_name: p.display_name.clone(),
        roles: p.roles.clone(),
        permissions: p.permissions.clone(),
        approval_limit: p.approval_limit,
        actor,
        instance_id: instance,
    }
}

// ------------------------------------------------------------------ public endpoints

/// Sign-in: validate → load → check active + password → issue token → record login.
async fn login(State(s): State<AppState>, ReqJson(req): ReqJson<LoginRequest>) -> ApiResult<LoginResponse> {
    async {
        if req.user_name.trim().is_empty() || req.password.is_empty() {
            return Err(Error::validation("identity.login.missing", "identity:login.missing"));
        }
        let invalid = || Error::unauthorized("identity.login.invalid", "identity:login.invalid");
        let row = load_user(&s.core, "lower(u.user_name) = lower($1)", req.user_name.trim()).await?.ok_or_else(invalid)?;
        if !passwords::verify(&req.password, &row.password_hash) {
            return Err(invalid());
        }
        if !row.is_active {
            return Err(Error::forbidden("identity.login.inactive", "identity:login.inactive"));
        }
        let p = profile(&s.core, &row).await?;
        let (access_token, expires_at) = s.core.tokens.issue(&to_user(&p, None, None), None)?;
        sqlx::query("update identity.users set last_login_at = now() where id = $1").bind(row.id).execute(s.core.db.pool()).await.db_err("identity.login.record")?;
        tracing::info!(user = %p.user_name, "signed in");
        Ok(LoginResponse { access_token, expires_at, user: p })
    }
    .await
    .http()
}

async fn me(State(s): State<AppState>, ctx: RequestContext) -> ApiResult<UserProfile> {
    async {
        let row = load_user(&s.core, "u.id = $1", ctx.user.id).await?.ok_or_not_found("identity.user", "errors:notFound")?;
        profile(&s.core, &row).await
    }
    .await
    .http()
}

/// Issues a fresh token with current roles/permissions (after role changes, before expiry).
async fn refresh(State(s): State<AppState>, ctx: RequestContext) -> ApiResult<TokenResponse> {
    async {
        if ctx.user.actor.is_some() {
            return Err(Error::forbidden("identity.refresh.delegated", "errors:forbidden"));
        }
        let row = load_user(&s.core, "u.id = $1", ctx.user.id).await?.ok_or_not_found("identity.user", "errors:notFound")?;
        if !row.is_active {
            return Err(Error::forbidden("identity.login.inactive", "identity:login.inactive"));
        }
        let p = profile(&s.core, &row).await?;
        let (access_token, expires_at) = s.core.tokens.issue(&to_user(&p, None, None), None)?;
        Ok(TokenResponse { access_token, expires_at })
    }
    .await
    .http()
}

async fn change_password(State(s): State<AppState>, ctx: RequestContext, ReqJson(req): ReqJson<ChangePasswordRequest>) -> Result<NoContent, redhive_backend_core::AppError> {
    async {
        if ctx.user.actor.is_some() {
            return Err(Error::forbidden("identity.password.delegated", "errors:forbidden"));
        }
        passwords::check_policy(&req.new_password)?;
        let row = load_user(&s.core, "u.id = $1", ctx.user.id).await?.ok_or_not_found("identity.user", "errors:notFound")?;
        if !passwords::verify(&req.current_password, &row.password_hash) {
            return Err(Error::validation("identity.password.current", "identity:password.currentWrong").for_field("currentPassword"));
        }
        if req.current_password == req.new_password {
            return Err(Error::validation("identity.password.same", "identity:password.same").for_field("newPassword"));
        }
        let hash = passwords::hash(&req.new_password);
        let mut tx = s.core.db.begin().await?;
        sqlx::query("update identity.users set password_hash = $2 where id = $1").bind(ctx.user.id).bind(hash).execute(&mut *tx).await.db_err("identity.password")?;
        s.audit.write(&mut tx, &ctx.user, "password.changed", "user", &ctx.user.id.to_string(), serde_json::json!({})).await?;
        tx.commit().await.db_err("identity.password.commit")?;
        Ok(NoContent)
    }
    .await
    .map_err(Into::into)
}

async fn put_avatar(State(s): State<AppState>, ctx: RequestContext, ReqJson(req): ReqJson<AvatarDto>) -> Result<NoContent, redhive_backend_core::AppError> {
    async {
        if !matches!(req.content_type.as_str(), "image/png" | "image/jpeg") {
            return Err(Error::validation("identity.avatar.type", "identity:avatar.type"));
        }
        let bytes = base64::engine::general_purpose::STANDARD.decode(&req.data_base64).map_err(|_| Error::validation("identity.avatar.data", "identity:avatar.data"))?;
        if bytes.len() > 512 * 1024 {
            return Err(Error::validation("identity.avatar.size", "identity:avatar.tooLarge").with_metadata("maxKb", 512));
        }
        let magic_ok = (req.content_type == "image/png" && bytes.starts_with(&[0x89, b'P', b'N', b'G'])) || (req.content_type == "image/jpeg" && bytes.starts_with(&[0xFF, 0xD8]));
        if !magic_ok {
            return Err(Error::validation("identity.avatar.data", "identity:avatar.data"));
        }
        sqlx::query(
            "insert into identity.user_avatars (user_id, content_type, data_base64) values ($1, $2, $3)
             on conflict (user_id) do update set content_type = excluded.content_type, data_base64 = excluded.data_base64, updated_at = now()",
        )
        .bind(ctx.user.id)
        .bind(&req.content_type)
        .bind(&req.data_base64)
        .execute(s.core.db.pool())
        .await
        .db_err("identity.avatar")?;
        Ok(NoContent)
    }
    .await
    .map_err(Into::into)
}

async fn delete_avatar(State(s): State<AppState>, ctx: RequestContext) -> Result<NoContent, redhive_backend_core::AppError> {
    sqlx::query("delete from identity.user_avatars where user_id = $1").bind(ctx.user.id).execute(s.core.db.pool()).await.db_err("identity.avatar.delete")?;
    Ok(NoContent)
}

async fn get_avatar(State(s): State<AppState>, _ctx: RequestContext, Path(id): Path<Uuid>) -> Result<Response, redhive_backend_core::AppError> {
    let row = sqlx::query("select content_type, data_base64 from identity.user_avatars where user_id = $1")
        .bind(id)
        .fetch_optional(s.core.db.pool())
        .await
        .db_err("identity.avatar")?
        .ok_or_not_found("identity.avatar", "identity:avatar.none")?;
    let ct: String = row.get("content_type");
    let data: String = row.get("data_base64");
    let bytes = base64::engine::general_purpose::STANDARD.decode(data).unwrap_or_default();
    Ok(([(header::CONTENT_TYPE, ct), (header::CACHE_CONTROL, "private, max-age=300".to_string())], bytes).into_response())
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct RoleDto {
    id: Uuid,
    name: String,
    description: String,
    permissions: Vec<String>,
    members: i64,
}

async fn roles(State(s): State<AppState>, ctx: RequestContext) -> ApiResult<Vec<RoleDto>> {
    async {
        ctx.user.require("users.view")?;
        let rows = sqlx::query(
            "select r.id, r.name, r.description,
                    coalesce(array(select rp.permission from identity.role_permissions rp where rp.role_id = r.id order by 1), '{}') as permissions,
                    (select count(*) from identity.user_roles ur where ur.role_id = r.id) as members
             from identity.roles r where r.tenant_id = $1 order by r.name",
        )
        .bind(ctx.user.tenant_id)
        .fetch_all(s.core.db.pool())
        .await
        .db_err("identity.roles")?;
        Ok(rows
            .into_iter()
            .map(|r| RoleDto { id: r.get("id"), name: r.get("name"), description: r.get("description"), permissions: r.get("permissions"), members: r.get("members") })
            .collect())
    }
    .await
    .http()
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct PermissionDto {
    name: String,
    module_id: String,
    description: String,
}

async fn permissions(State(s): State<AppState>, ctx: RequestContext) -> ApiResult<Vec<PermissionDto>> {
    async {
        ctx.user.require("users.view")?;
        let rows = sqlx::query("select name, module_id, description from identity.permissions order by module_id, name").fetch_all(s.core.db.pool()).await.db_err("identity.permissions")?;
        Ok(rows.into_iter().map(|r| PermissionDto { name: r.get("name"), module_id: r.get("module_id"), description: r.get("description") }).collect())
    }
    .await
    .http()
}

// ------------------------------------------------------------------ internal endpoints

/// Short-lived token of the user a workflow runs as (`act` = engine / AI agent). Refused for inactive users.
async fn delegated(State(s): State<AppState>, call: InternalCall, ReqJson(req): ReqJson<DelegatedTokenRequest>) -> ApiResult<TokenResponse> {
    async {
        let row = load_user(&s.core, "u.id = $1", req.user_id).await?.ok_or_not_found("identity.user", "errors:notFound")?;
        if !row.is_active {
            return Err(Error::forbidden("identity.delegated.inactive", "identity:login.inactive"));
        }
        let p = profile(&s.core, &row).await?;
        let minutes = req.minutes.clamp(1, 60);
        let (access_token, expires_at) = s.core.tokens.issue(&to_user(&p, Some(req.actor.clone()), req.instance_id), Some(minutes))?;
        tracing::debug!(user = %p.user_name, actor = %req.actor, correlation = %call.correlation_id, "delegated token issued");
        Ok(TokenResponse { access_token, expires_at })
    }
    .await
    .http()
}

fn summary(p: UserProfile, active: bool) -> UserSummary {
    UserSummary {
        id: p.id,
        tenant_id: p.tenant_id,
        user_name: p.user_name,
        display_name: p.display_name,
        email: p.email,
        is_active: active,
        roles: p.roles,
        permissions: p.permissions,
        approval_limit: p.approval_limit,
    }
}

async fn internal_user(State(s): State<AppState>, _call: InternalCall, Path(id): Path<Uuid>) -> ApiResult<UserSummary> {
    async {
        let row = load_user(&s.core, "u.id = $1", id).await?.ok_or_not_found("identity.user", "errors:notFound")?;
        let active = row.is_active;
        Ok(summary(profile(&s.core, &row).await?, active))
    }
    .await
    .http()
}

#[derive(Deserialize)]
struct UsersQuery {
    tenant: Uuid,
    #[serde(default)]
    permission: Option<String>,
}

/// Users of a tenant, optionally only those holding a permission (inbox routing, notifications).
async fn internal_users(State(s): State<AppState>, _call: InternalCall, Query(q): Query<UsersQuery>) -> ApiResult<Vec<UserSummary>> {
    async {
        let rows = sqlx::query_as::<_, UserRow>(&format!("{USER_SELECT} where u.tenant_id = $1 and u.is_active order by u.display_name"))
            .bind(q.tenant)
            .fetch_all(s.core.db.pool())
            .await
            .db_err("identity.users")?;
        let mut out = Vec::new();
        for r in rows {
            let p = profile(&s.core, &r).await?;
            if q.permission.as_deref().is_none_or(|perm| p.has(perm)) {
                out.push(summary(p, true));
            }
        }
        Ok(out)
    }
    .await
    .http()
}

/// Registers the permissions a module declares (called by the registry on publish).
async fn register_permissions(State(s): State<AppState>, _call: InternalCall, ReqJson(req): ReqJson<PermissionRegistration>) -> ApiResult<serde_json::Value> {
    async {
        let mut tx = s.core.db.begin().await?;
        for p in &req.permissions {
            sqlx::query("insert into identity.permissions (name, module_id, description) values ($1, $2, '') on conflict (name) do update set module_id = excluded.module_id")
                .bind(p)
                .bind(&req.module_id)
                .execute(&mut *tx)
                .await
                .db_err("identity.permissions.register")?;
        }
        tx.commit().await.db_err("identity.permissions.commit")?;
        Ok(serde_json::json!({ "registered": req.permissions.len() }))
    }
    .await
    .http()
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let core = Core::init(SERVICE).await?;
    let state = AppState { core: core.clone(), audit: AuditLog::new(SERVICE) };
    let router = Router::new()
        .route("/api/identity/login", post(login))
        .route("/api/identity/refresh", post(refresh))
        .route("/api/identity/me", get(me))
        .route("/api/identity/me/password", put(change_password))
        .route("/api/identity/me/avatar", put(put_avatar).delete(delete_avatar))
        .route("/api/identity/users/{id}/avatar", get(get_avatar))
        .route("/api/identity/roles", get(roles))
        .route("/api/identity/permissions", get(permissions))
        .route("/internal/tokens/delegated", post(delegated))
        .route("/internal/users", get(internal_users))
        .route("/internal/users/{id}", get(internal_user))
        .route("/internal/permissions", post(register_permissions));
    serve(core, router, state).await
}
