//! Red Hive module backend **user-management** (`user-service`): user directory, create/edit users,
//! role assignment, password reset and the organization (departments).
//!
//! Users and roles live in the `identity` schema (owned by the identity service); this module manages
//! them through the same database and adds profiles and departments (`usermgmt` schema).

use axum::extract::{Path, Query, State};
use axum::routing::{get, post, put};
use axum::Router;
use chrono::NaiveDate;
use rand::Rng;
use redhive_backend_core::db::{ensure_updated, DbErrorExt, Tx};
use redhive_backend_core::hosting::{serve, Core};
use redhive_backend_core::security::passwords;
use redhive_backend_core::{ApiResult, AuditLog, Created, OutboxWriter, RedHiveUser, RequestContext, ToHttp, ReqJson};
use redhive_contracts::notifications::{NotificationAudience, Severity};
use redhive_monads::{Error, OptionExt, Res, Validator};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use sqlx::{FromRow, Row};
use uuid::Uuid;

const SERVICE: &str = "user-service";
const PERM_VIEW: &str = "users.view";
const PERM_EDIT: &str = "users.edit";
const PERM_ROLES: &str = "users.roles";

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

#[derive(Debug, Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
struct UserDto {
    id: Uuid,
    user_name: String,
    display_name: String,
    email: String,
    is_active: bool,
    approval_limit: f64,
    department_id: Option<Uuid>,
    department: Option<String>,
    cost_center: Option<String>,
    job_title: Option<String>,
    phone: Option<String>,
    location: Option<String>,
    manager_id: Option<Uuid>,
    manager: Option<String>,
    hired_on: Option<NaiveDate>,
    roles: Vec<String>,
    last_login_at: Option<chrono::DateTime<chrono::Utc>>,
    has_avatar: bool,
    row_version: Option<i32>,
}

const USER_SELECT: &str = "select u.id, u.user_name, u.display_name, u.email, u.is_active, u.approval_limit::float8 as approval_limit,
    p.department_id, d.name as department, d.cost_center, p.job_title, p.phone, p.location, p.manager_id, m.display_name as manager, p.hired_on,
    coalesce(array(select r.name from identity.user_roles ur join identity.roles r on r.id = ur.role_id where ur.user_id = u.id order by r.name), '{}') as roles,
    u.last_login_at, exists(select 1 from identity.user_avatars a where a.user_id = u.id) as has_avatar, p.row_version
  from identity.users u
  left join usermgmt.profiles p on p.user_id = u.id
  left join usermgmt.departments d on d.id = p.department_id
  left join identity.users m on m.id = p.manager_id";

async fn load(tx: &mut Tx<'static>, tenant: Uuid, id: Uuid) -> Res<UserDto> {
    sqlx::query_as::<_, UserDto>(&format!("{USER_SELECT} where u.id = $1 and u.tenant_id = $2"))
        .bind(id)
        .bind(tenant)
        .fetch_optional(&mut **tx)
        .await
        .db_err("users.user")?
        .ok_or_not_found("users.user", "users:user.notFound")
}

#[derive(Deserialize, Default)]
struct ListQuery {
    #[serde(default)]
    search: Option<String>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    department: Option<Uuid>,
}

async fn list(State(s): State<AppState>, ctx: RequestContext, Query(q): Query<ListQuery>) -> ApiResult<Vec<UserDto>> {
    async {
        ctx.user.require(PERM_VIEW)?;
        let search = q.search.filter(|s| !s.trim().is_empty()).map(|s| format!("%{}%", s.trim().to_lowercase()));
        let active = match q.status.as_deref() {
            Some("active") => Some(true),
            Some("inactive") => Some(false),
            _ => None,
        };
        sqlx::query_as::<_, UserDto>(&format!(
            "{USER_SELECT} where u.tenant_id = $1 and ($2::text is null or lower(u.display_name || ' ' || u.user_name || ' ' || u.email || ' ' || coalesce(p.job_title, '')) like $2)
               and ($3::bool is null or u.is_active = $3) and ($4::uuid is null or p.department_id = $4) order by u.display_name"
        ))
        .bind(ctx.user.tenant_id)
        .bind(search)
        .bind(active)
        .bind(q.department)
        .fetch_all(s.core.db.pool())
        .await
        .db_err("users.list")
    }
    .await
    .http()
}

async fn get_user(State(s): State<AppState>, ctx: RequestContext, Path(id): Path<Uuid>) -> ApiResult<UserDto> {
    async {
        ctx.user.require(PERM_VIEW)?;
        let mut tx = s.core.db.begin().await?;
        load(&mut tx, ctx.user.tenant_id, id).await
    }
    .await
    .http()
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateUser {
    display_name: String,
    user_name: String,
    email: String,
    #[serde(default)]
    department_id: Option<String>,
    #[serde(default)]
    job_title: Option<String>,
    #[serde(default)]
    role: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CreatedUser {
    user_id: Uuid,
    user_name: String,
    temporary_password: String,
}

fn temporary_password() -> String {
    const CHARS: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnpqrstuvwxyz23456789";
    let mut rng = rand::thread_rng();
    let body: String = (0..10).map(|_| CHARS[rng.gen_range(0..CHARS.len())] as char).collect();
    format!("Rh-{body}!7")
}

fn validate_create(c: &CreateUser) -> Res<()> {
    let user_name_ok = {
        let mut chars = c.user_name.chars();
        matches!(chars.next(), Some(ch) if ch.is_ascii_lowercase()) && c.user_name.len() >= 3 && c.user_name.len() <= 40 && c.user_name.chars().all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '.' || ch == '-')
    };
    let email_ok = c.email.split_once('@').is_some_and(|(a, b)| !a.is_empty() && b.contains('.') && !b.starts_with('.') && !b.ends_with('.'));
    Validator::new("users")
        .require("displayName", !c.display_name.trim().is_empty())
        .check("displayName", c.display_name.chars().count() <= 100, "tooLong")
        .check("userName", user_name_ok, "invalid")
        .check("email", email_ok, "invalid")
        .finish(())
}

async fn create_user_uc(s: &AppState, u: &RedHiveUser, c: CreateUser) -> Res<CreatedUser> {
    validate_create(&c)?;
    u.require(PERM_EDIT)?;
    let role = c.role.clone().filter(|r| !r.is_empty()).unwrap_or_else(|| "Viewer".into());
    if role == "Administrator" && !u.has("*") {
        return Err(Error::forbidden("users.role.admin", "users:role.adminOnly"));
    }
    let department: Option<Uuid> = match c.department_id.as_deref().filter(|d| !d.is_empty()) {
        Some(d) => Some(Uuid::parse_str(d).map_err(|_| Error::validation("departmentId.invalid", "errors:invalidValue").for_field("departmentId"))?),
        None => None,
    };
    let password = temporary_password();
    let id = Uuid::new_v4();
    let mut tx = s.core.db.begin().await?;
    let role_id: Uuid = sqlx::query_scalar("select id from identity.roles where tenant_id = $1 and name = $2")
        .bind(u.tenant_id)
        .bind(&role)
        .fetch_optional(&mut *tx)
        .await
        .db_err("users.role")?
        .ok_or_else(|| Error::validation("role.unknown", "users:role.unknown").for_field("role"))?;
    sqlx::query("insert into identity.users (id, tenant_id, user_name, display_name, email, password_hash) values ($1, $2, $3, $4, $5, $6)")
        .bind(id)
        .bind(u.tenant_id)
        .bind(c.user_name.trim())
        .bind(c.display_name.trim())
        .bind(c.email.trim())
        .bind(passwords::hash(&password))
        .execute(&mut *tx)
        .await
        .map_err(|e| match redhive_backend_core::db::map_db_error("users.create", e) {
            e if e.kind == redhive_monads::ErrorKind::Conflict => Error::conflict("users.userName.taken", "users:userName.taken").for_field("userName"),
            e => e,
        })?;
    sqlx::query("insert into usermgmt.profiles (user_id, department_id, job_title, hired_on) values ($1, $2, $3, current_date)")
        .bind(id)
        .bind(department)
        .bind(c.job_title.clone().unwrap_or_default())
        .execute(&mut *tx)
        .await
        .db_err("users.profile")?;
    sqlx::query("insert into identity.user_roles (user_id, role_id) values ($1, $2)").bind(id).bind(role_id).execute(&mut *tx).await.db_err("users.role")?;
    let mut ev = s
        .outbox
        .create("users.created", u, NotificationAudience::WithPermission(PERM_VIEW.into()), json!({ "userId": id, "userName": c.user_name, "displayName": c.display_name, "role": role }))
        .titled("users:notification.created", Severity::Success)
        .navigate("users.directory", Some(id.to_string()));
    ev.text_args.insert("name".into(), json!(c.display_name));
    s.outbox.add(&mut tx, &ev).await?;
    s.audit.write(&mut tx, u, "user.created", "user", &id.to_string(), json!({ "userName": c.user_name, "role": role })).await?;
    tx.commit().await.db_err("users.commit")?;
    Ok(CreatedUser { user_id: id, user_name: c.user_name, temporary_password: password })
}

async fn create_user(State(s): State<AppState>, ctx: RequestContext, ReqJson(c): ReqJson<CreateUser>) -> Result<Created<CreatedUser>, redhive_backend_core::AppError> {
    create_user_uc(&s, &ctx.user, c).await.map(Created).map_err(Into::into)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdateUser {
    display_name: String,
    email: String,
    #[serde(default)]
    department_id: Option<String>,
    #[serde(default)]
    job_title: String,
    #[serde(default)]
    phone: String,
    #[serde(default)]
    location: String,
    #[serde(default)]
    is_active: Option<bool>,
    #[serde(default)]
    approval_limit: Option<f64>,
    #[serde(default)]
    row_version: Option<i32>,
}

async fn update_user(State(s): State<AppState>, ctx: RequestContext, Path(id): Path<Uuid>, ReqJson(c): ReqJson<UpdateUser>) -> ApiResult<UserDto> {
    async {
        Validator::new("users")
            .require("displayName", !c.display_name.trim().is_empty())
            .check("email", c.email.contains('@'), "invalid")
            .check("approvalLimit", c.approval_limit.is_none_or(|l| l >= 0.0), "negative")
            .finish(())?;
        let u = &ctx.user;
        u.require(PERM_EDIT)?;
        if c.approval_limit.is_some() {
            u.require(PERM_ROLES)?;
        }
        if c.is_active == Some(false) && id == u.id {
            return Err(Error::conflict("users.deactivate.self", "users:deactivate.self"));
        }
        let department: Option<Uuid> = c.department_id.as_deref().filter(|d| !d.is_empty()).and_then(|d| Uuid::parse_str(d).ok());
        let mut tx = s.core.db.begin().await?;
        let before = load(&mut tx, u.tenant_id, id).await?;
        sqlx::query("update identity.users set display_name = $2, email = $3, is_active = coalesce($4, is_active), approval_limit = coalesce($5::numeric, approval_limit) where id = $1")
            .bind(id)
            .bind(c.display_name.trim())
            .bind(c.email.trim())
            .bind(c.is_active)
            .bind(c.approval_limit)
            .execute(&mut *tx)
            .await
            .db_err("users.update")?;
        let rows = sqlx::query(
            "insert into usermgmt.profiles (user_id, department_id, job_title, phone, location) values ($1, $2, $3, $4, $5)
             on conflict (user_id) do update set department_id = excluded.department_id, job_title = excluded.job_title, phone = excluded.phone,
                 location = excluded.location, row_version = usermgmt.profiles.row_version + 1
             where usermgmt.profiles.row_version = coalesce($6, usermgmt.profiles.row_version)",
        )
        .bind(id)
        .bind(department)
        .bind(&c.job_title)
        .bind(&c.phone)
        .bind(&c.location)
        .bind(c.row_version)
        .execute(&mut *tx)
        .await
        .db_err("users.profile")?
        .rows_affected();
        ensure_updated(rows, "users.profile")?;
        let after = load(&mut tx, u.tenant_id, id).await?;
        let ev = s.outbox.create("users.updated", u, NotificationAudience::WithPermission(PERM_VIEW.into()), json!({ "userId": id, "displayName": after.display_name, "isActive": after.is_active }));
        s.outbox.add(&mut tx, &ev).await?;
        s.audit.write(&mut tx, u, "user.updated", "user", &id.to_string(), json!({ "wasActive": before.is_active, "isActive": after.is_active })).await?;
        tx.commit().await.db_err("users.commit")?;
        Ok(after)
    }
    .await
    .http()
}

#[derive(Debug, Deserialize)]
struct RolesRequest {
    roles: Vec<String>,
}

async fn set_roles(State(s): State<AppState>, ctx: RequestContext, Path(id): Path<Uuid>, ReqJson(req): ReqJson<RolesRequest>) -> ApiResult<UserDto> {
    async {
        let u = &ctx.user;
        u.require(PERM_ROLES)?;
        if req.roles.is_empty() {
            return Err(Error::validation("roles.empty", "users:roles.empty").for_field("roles"));
        }
        if req.roles.iter().any(|r| r == "Administrator") && !u.has("*") {
            return Err(Error::forbidden("users.role.admin", "users:role.adminOnly"));
        }
        if id == u.id && !req.roles.iter().any(|r| r == "Administrator") && u.roles.iter().any(|r| r == "Administrator") {
            return Err(Error::conflict("users.roles.selfDemotion", "users:roles.selfDemotion"));
        }
        let mut tx = s.core.db.begin().await?;
        load(&mut tx, u.tenant_id, id).await?;
        let ids: Vec<Uuid> = sqlx::query_scalar("select id from identity.roles where tenant_id = $1 and name = any($2)")
            .bind(u.tenant_id)
            .bind(&req.roles)
            .fetch_all(&mut *tx)
            .await
            .db_err("users.roles")?;
        if ids.len() != req.roles.len() {
            return Err(Error::validation("roles.unknown", "users:role.unknown").for_field("roles"));
        }
        sqlx::query("delete from identity.user_roles where user_id = $1").bind(id).execute(&mut *tx).await.db_err("users.roles")?;
        for r in &ids {
            sqlx::query("insert into identity.user_roles (user_id, role_id) values ($1, $2)").bind(id).bind(r).execute(&mut *tx).await.db_err("users.roles")?;
        }
        let after = load(&mut tx, u.tenant_id, id).await?;
        let ev = s
            .outbox
            .create("users.roles.changed", u, NotificationAudience::User(id), json!({ "userId": id, "roles": after.roles }))
            .titled("users:notification.rolesChanged", Severity::Info);
        s.outbox.add(&mut tx, &ev).await?;
        s.audit.write(&mut tx, u, "user.rolesChanged", "user", &id.to_string(), json!({ "roles": req.roles })).await?;
        tx.commit().await.db_err("users.commit")?;
        Ok(after)
    }
    .await
    .http()
}

async fn reset_password(State(s): State<AppState>, ctx: RequestContext, Path(id): Path<Uuid>) -> ApiResult<Value> {
    async {
        let u = &ctx.user;
        u.require(PERM_EDIT)?;
        let password = temporary_password();
        let mut tx = s.core.db.begin().await?;
        let target = load(&mut tx, u.tenant_id, id).await?;
        sqlx::query("update identity.users set password_hash = $2 where id = $1").bind(id).bind(passwords::hash(&password)).execute(&mut *tx).await.db_err("users.reset")?;
        s.audit.write(&mut tx, u, "user.passwordReset", "user", &id.to_string(), json!({})).await?;
        tx.commit().await.db_err("users.commit")?;
        Ok(json!({ "userId": id, "userName": target.user_name, "temporaryPassword": password }))
    }
    .await
    .http()
}

async fn organization(State(s): State<AppState>, ctx: RequestContext) -> ApiResult<Vec<Value>> {
    async {
        ctx.user.require(PERM_VIEW)?;
        let rows = sqlx::query(
            "select d.id, d.name, d.cost_center,
                    count(p.user_id) as members,
                    coalesce(array_agg(u.display_name order by u.display_name) filter (where u.id is not null), '{}') as people
             from usermgmt.departments d
             left join usermgmt.profiles p on p.department_id = d.id
             left join identity.users u on u.id = p.user_id and u.is_active
             where d.tenant_id = $1 group by d.id order by d.cost_center",
        )
        .bind(ctx.user.tenant_id)
        .fetch_all(s.core.db.pool())
        .await
        .db_err("users.organization")?;
        Ok(rows
            .iter()
            .map(|r| json!({ "id": r.get::<Uuid, _>("id"), "name": r.get::<String, _>("name"), "costCenter": r.get::<String, _>("cost_center"), "members": r.get::<i64, _>("members"), "people": r.get::<Vec<String>, _>("people") }))
            .collect())
    }
    .await
    .http()
}

async fn departments(State(s): State<AppState>, ctx: RequestContext) -> ApiResult<Vec<Value>> {
    async {
        let rows = sqlx::query("select id, name, cost_center from usermgmt.departments where tenant_id = $1 order by name").bind(ctx.user.tenant_id).fetch_all(s.core.db.pool()).await.db_err("users.departments")?;
        Ok(rows.iter().map(|r| json!({ "value": r.get::<Uuid, _>("id"), "label": format!("{} ({})", r.get::<String, _>("name"), r.get::<String, _>("cost_center")) })).collect())
    }
    .await
    .http()
}

async fn roles_lookup(State(s): State<AppState>, ctx: RequestContext) -> ApiResult<Vec<Value>> {
    async {
        let rows = sqlx::query("select name, description from identity.roles where tenant_id = $1 order by name").bind(ctx.user.tenant_id).fetch_all(s.core.db.pool()).await.db_err("users.roles")?;
        Ok(rows.iter().map(|r| json!({ "value": r.get::<String, _>("name"), "label": r.get::<String, _>("name") })).collect())
    }
    .await
    .http()
}

async fn workflow_step(State(s): State<AppState>, ctx: RequestContext, Path(step): Path<String>, ReqJson(inputs): ReqJson<Map<String, Value>>) -> ApiResult<Value> {
    async {
        match step.as_str() {
            "users.createUser" => {
                let c: CreateUser = serde_json::from_value(Value::Object(inputs)).map_err(Error::from)?;
                let r = create_user_uc(&s, &ctx.user, c).await?;
                Ok(json!({ "userId": r.user_id, "userName": r.user_name, "temporaryPassword": r.temporary_password }))
            }
            other => Err(Error::not_found("workflow.stepType.unknown", "workflow:stepType.unknown").with_metadata("stepType", other.to_string())),
        }
    }
    .await
    .http()
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let core = Core::init(SERVICE).await?;
    let state = AppState { core: core.clone(), outbox: OutboxWriter::new(SERVICE), audit: AuditLog::new(SERVICE) };
    let router = Router::new()
        .route("/api/users", get(list).post(create_user))
        .route("/api/users/organization", get(organization))
        .route("/api/users/lookups/departments", get(departments))
        .route("/api/users/lookups/roles", get(roles_lookup))
        .route("/api/users/workflow-steps/{step}", post(workflow_step))
        .route("/api/users/{id}", get(get_user).put(update_user))
        .route("/api/users/{id}/roles", put(set_roles))
        .route("/api/users/{id}/reset-password", post(reset_password));
    serve(core, router, state).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_validation() {
        let mut c = CreateUser { display_name: "Anna".into(), user_name: "Anna".into(), email: "anna@x".into(), department_id: None, job_title: None, role: None };
        assert_eq!(validate_create(&c).unwrap_err().field_errors.len(), 2);
        c.user_name = "anna.k".into();
        c.email = "anna@redhive.demo".into();
        assert!(validate_create(&c).is_ok());
        let p = temporary_password();
        assert!(passwords::check_policy(&p).is_ok(), "{p}");
    }
}
