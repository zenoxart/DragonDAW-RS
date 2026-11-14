//! Red Hive module backend **document-management** (`document-service`): documents with versions and the
//! invoice approval process (draft → submitted → approved/rejected → paid).

mod invoices;

use axum::extract::{Path, Query, State};
use axum::routing::{get, post};
use axum::Router;
use chrono::{DateTime, Utc};
use invoices::{DecisionInput, InvoiceDto, InvoiceInput, InvoiceQuery};
use redhive_backend_core::db::{DbErrorExt, Tx};
use redhive_backend_core::hosting::{serve, Core};
use redhive_backend_core::{ApiResult, AuditLog, Created, OutboxWriter, RedHiveUser, RequestContext, ToHttp, OptJson, ReqJson};
use redhive_contracts::notifications::NotificationAudience;
use redhive_contracts::primitives::Page;
use redhive_monads::{Error, OptionExt, Res, Validator};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use sqlx::{FromRow, Row};
use uuid::Uuid;

pub const SERVICE: &str = "document-service";
pub const PERM_VIEW: &str = "documents.view";
pub const PERM_EDIT: &str = "documents.edit";
pub const PERM_APPROVE: &str = "documents.approve";

#[derive(Clone)]
pub struct AppState {
    pub core: Core,
    pub outbox: OutboxWriter,
    pub audit: AuditLog,
}

impl axum::extract::FromRef<AppState> for Core {
    fn from_ref(s: &AppState) -> Core {
        s.core.clone()
    }
}

// ------------------------------------------------------------------ documents

#[derive(Debug, Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
struct DocumentDto {
    id: Uuid,
    title: String,
    category: String,
    status: String,
    owner_id: Uuid,
    owner_name: String,
    file_name: String,
    content_type: String,
    size_bytes: i64,
    tags: Vec<String>,
    current_version: i32,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    row_version: i32,
}

const DOC_SELECT: &str = "select id, title, category, status, owner_id, owner_name, file_name, content_type, size_bytes, tags, current_version, created_at, updated_at, row_version from documents.documents";

#[derive(Debug, Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
struct VersionDto {
    version: i32,
    file_name: String,
    size_bytes: i64,
    comment: String,
    created_by: String,
    created_at: DateTime<Utc>,
}

#[derive(Deserialize, Default)]
struct DocQuery {
    #[serde(default)]
    search: Option<String>,
    #[serde(default)]
    category: Option<String>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    skip: Option<i64>,
    #[serde(default)]
    take: Option<i64>,
}

async fn list_documents(State(s): State<AppState>, ctx: RequestContext, Query(q): Query<DocQuery>) -> ApiResult<Page<DocumentDto>> {
    async {
        ctx.user.require(PERM_VIEW)?;
        let skip = q.skip.unwrap_or(0).max(0);
        let take = q.take.unwrap_or(100).clamp(1, 500);
        let cat = q.category.filter(|c| !c.is_empty() && c != "all");
        let status = q.status.filter(|c| !c.is_empty() && c != "all");
        let search = q.search.filter(|c| !c.trim().is_empty()).map(|c| format!("%{}%", c.trim().to_lowercase()));
        let filter = "tenant_id = $1 and ($2::text is null or category = $2) and ($3::text is null or status = $3)
                      and ($4::text is null or lower(title || ' ' || file_name || ' ' || owner_name || ' ' || array_to_string(tags, ' ')) like $4)";
        let items = sqlx::query_as::<_, DocumentDto>(&format!("{DOC_SELECT} where {filter} order by updated_at desc limit $5 offset $6"))
            .bind(ctx.user.tenant_id)
            .bind(&cat)
            .bind(&status)
            .bind(&search)
            .bind(take)
            .bind(skip)
            .fetch_all(s.core.db.pool())
            .await
            .db_err("documents.list")?;
        let total: i64 = sqlx::query(&format!("select count(*) as n from documents.documents where {filter}"))
            .bind(ctx.user.tenant_id)
            .bind(&cat)
            .bind(&status)
            .bind(&search)
            .fetch_one(s.core.db.pool())
            .await
            .db_err("documents.count")?
            .get("n");
        Ok(Page { items, total, skip, take })
    }
    .await
    .http()
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateDocument {
    title: String,
    category: String,
    file_name: String,
    #[serde(default = "pdf")]
    content_type: String,
    #[serde(default)]
    size_bytes: i64,
    #[serde(default)]
    tags: Vec<String>,
}

fn pdf() -> String {
    "application/pdf".into()
}

async fn load_doc(tx: &mut Tx<'static>, tenant: Uuid, id: Uuid) -> Res<DocumentDto> {
    sqlx::query_as::<_, DocumentDto>(&format!("{DOC_SELECT} where id = $1 and tenant_id = $2"))
        .bind(id)
        .bind(tenant)
        .fetch_optional(&mut **tx)
        .await
        .db_err("documents.document")?
        .ok_or_not_found("documents.document", "documents:document.notFound")
}

fn doc_event(s: &AppState, user: &RedHiveUser, event: &str, d: &DocumentDto) -> redhive_contracts::notifications::NotificationEnvelope {
    s.outbox.create(event, user, NotificationAudience::WithPermission(PERM_VIEW.into()), json!({ "documentId": d.id, "title": d.title, "category": d.category, "status": d.status, "version": d.current_version }))
}

async fn create_document(State(s): State<AppState>, ctx: RequestContext, ReqJson(req): ReqJson<CreateDocument>) -> Result<Created<DocumentDto>, redhive_backend_core::AppError> {
    async {
        Validator::new("documents")
            .require("title", !req.title.trim().is_empty())
            .check("category", ["contract", "report", "other", "invoice"].contains(&req.category.as_str()), "invalid")
            .require("fileName", !req.file_name.trim().is_empty())
            .check("sizeBytes", req.size_bytes >= 0, "negative")
            .finish(())?;
        ctx.user.require(PERM_EDIT)?;
        let u = &ctx.user;
        let id = Uuid::new_v4();
        let mut tx = s.core.db.begin().await?;
        sqlx::query(
            "insert into documents.documents (id, tenant_id, title, category, status, owner_id, owner_name, file_name, content_type, size_bytes, tags)
             values ($1, $2, $3, $4, 'draft', $5, $6, $7, $8, $9, $10)",
        )
        .bind(id)
        .bind(u.tenant_id)
        .bind(req.title.trim())
        .bind(&req.category)
        .bind(u.id)
        .bind(&u.display_name)
        .bind(&req.file_name)
        .bind(&req.content_type)
        .bind(req.size_bytes)
        .bind(&req.tags)
        .execute(&mut *tx)
        .await
        .db_err("documents.create")?;
        sqlx::query("insert into documents.document_versions (document_id, version, file_name, size_bytes, comment, created_by) values ($1, 1, $2, $3, 'Initial upload', $4)")
            .bind(id)
            .bind(&req.file_name)
            .bind(req.size_bytes)
            .bind(&u.display_name)
            .execute(&mut *tx)
            .await
            .db_err("documents.version")?;
        let d = load_doc(&mut tx, u.tenant_id, id).await?;
        s.outbox.add(&mut tx, &doc_event(&s, u, "documents.created", &d)).await?;
        s.audit.write(&mut tx, u, "document.created", "document", &id.to_string(), json!({ "title": d.title })).await?;
        tx.commit().await.db_err("documents.commit")?;
        Ok::<_, Error>(Created(d))
    }
    .await
    .map_err(Into::into)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DocumentDetail {
    #[serde(flatten)]
    document: DocumentDto,
    versions: Vec<VersionDto>,
}

async fn get_document(State(s): State<AppState>, ctx: RequestContext, Path(id): Path<Uuid>) -> ApiResult<DocumentDetail> {
    async {
        ctx.user.require(PERM_VIEW)?;
        let mut tx = s.core.db.begin().await?;
        let document = load_doc(&mut tx, ctx.user.tenant_id, id).await?;
        let versions = sqlx::query_as::<_, VersionDto>("select version, file_name, size_bytes, comment, created_by, created_at from documents.document_versions where document_id = $1 order by version desc")
            .bind(id)
            .fetch_all(&mut *tx)
            .await
            .db_err("documents.versions")?;
        Ok(DocumentDetail { document, versions })
    }
    .await
    .http()
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct NewVersion {
    file_name: String,
    #[serde(default)]
    size_bytes: i64,
    #[serde(default)]
    comment: String,
    #[serde(default)]
    row_version: Option<i32>,
}

async fn add_version(State(s): State<AppState>, ctx: RequestContext, Path(id): Path<Uuid>, ReqJson(req): ReqJson<NewVersion>) -> ApiResult<DocumentDto> {
    async {
        Validator::new("documents").require("fileName", !req.file_name.trim().is_empty()).finish(())?;
        ctx.user.require(PERM_EDIT)?;
        let u = &ctx.user;
        let mut tx = s.core.db.begin().await?;
        let d = load_doc(&mut tx, u.tenant_id, id).await?;
        if d.status == "archived" {
            return Err(Error::conflict("documents.document.archived", "documents:document.archived"));
        }
        if req.row_version.is_some_and(|rv| rv != d.row_version) {
            return Err(Error::conflict("documents.document.stale", "errors:staleVersion"));
        }
        let v = d.current_version + 1;
        sqlx::query("insert into documents.document_versions (document_id, version, file_name, size_bytes, comment, created_by) values ($1, $2, $3, $4, $5, $6)")
            .bind(id)
            .bind(v)
            .bind(&req.file_name)
            .bind(req.size_bytes)
            .bind(&req.comment)
            .bind(&u.display_name)
            .execute(&mut *tx)
            .await
            .db_err("documents.version")?;
        sqlx::query("update documents.documents set current_version = $2, file_name = $3, size_bytes = $4, status = 'active', updated_at = now(), row_version = row_version + 1 where id = $1")
            .bind(id)
            .bind(v)
            .bind(&req.file_name)
            .bind(req.size_bytes)
            .execute(&mut *tx)
            .await
            .db_err("documents.update")?;
        let d = load_doc(&mut tx, u.tenant_id, id).await?;
        s.outbox.add(&mut tx, &doc_event(&s, u, "documents.updated", &d)).await?;
        s.audit.write(&mut tx, u, "document.versionAdded", "document", &id.to_string(), json!({ "version": v })).await?;
        tx.commit().await.db_err("documents.commit")?;
        Ok(d)
    }
    .await
    .http()
}

async fn set_status(s: &AppState, u: &RedHiveUser, id: Uuid, status: &str) -> Res<DocumentDto> {
    u.require(PERM_EDIT)?;
    let mut tx = s.core.db.begin().await?;
    let d = load_doc(&mut tx, u.tenant_id, id).await?;
    if d.status == status {
        return Ok(d);
    }
    sqlx::query("update documents.documents set status = $2, updated_at = now(), row_version = row_version + 1 where id = $1").bind(id).bind(status).execute(&mut *tx).await.db_err("documents.status")?;
    let d = load_doc(&mut tx, u.tenant_id, id).await?;
    s.outbox.add(&mut tx, &doc_event(s, u, "documents.updated", &d)).await?;
    s.audit.write(&mut tx, u, &format!("document.{status}"), "document", &id.to_string(), json!({})).await?;
    tx.commit().await.db_err("documents.commit")?;
    Ok(d)
}

async fn archive_document(State(s): State<AppState>, ctx: RequestContext, Path(id): Path<Uuid>) -> ApiResult<DocumentDto> {
    set_status(&s, &ctx.user, id, "archived").await.http()
}

async fn activate_document(State(s): State<AppState>, ctx: RequestContext, Path(id): Path<Uuid>) -> ApiResult<DocumentDto> {
    set_status(&s, &ctx.user, id, "active").await.http()
}

// ------------------------------------------------------------------ dashboard & lookups

async fn stats(State(s): State<AppState>, ctx: RequestContext) -> ApiResult<Value> {
    async {
        ctx.user.require(PERM_VIEW)?;
        let t = ctx.user.tenant_id;
        let docs = sqlx::query(
            "select count(*) as total, count(*) filter (where status = 'active') as active, count(*) filter (where status = 'draft') as draft,
                    count(*) filter (where status = 'archived') as archived from documents.documents where tenant_id = $1 and category <> 'invoice'",
        )
        .bind(t)
        .fetch_one(s.core.db.pool())
        .await
        .db_err("documents.stats")?;
        let inv = sqlx::query(
            "select count(*) filter (where status = 'draft') as draft, count(*) filter (where status = 'submitted') as submitted,
                    count(*) filter (where status = 'approved') as approved, count(*) filter (where status = 'rejected') as rejected,
                    count(*) filter (where status = 'paid') as paid,
                    coalesce(sum(amount) filter (where status in ('submitted', 'approved')), 0)::float8 as open_amount,
                    count(*) filter (where status in ('submitted', 'approved') and due_on < current_date) as overdue
             from documents.invoices where tenant_id = $1",
        )
        .bind(t)
        .fetch_one(s.core.db.pool())
        .await
        .db_err("documents.stats")?;
        let by_cost_center = sqlx::query("select cost_center as label, sum(amount)::float8 as value from documents.invoices where tenant_id = $1 and status <> 'draft' group by cost_center order by cost_center")
            .bind(t)
            .fetch_all(s.core.db.pool())
            .await
            .db_err("documents.stats")?;
        let recent = sqlx::query_as::<_, InvoiceDto>(&format!("{} where tenant_id = $1 and status = 'submitted' order by submitted_at desc limit 8", invoices::INVOICE_SELECT))
            .bind(t)
            .fetch_all(s.core.db.pool())
            .await
            .db_err("documents.stats")?;
        Ok(json!({
            "documents": { "total": docs.get::<i64, _>("total"), "active": docs.get::<i64, _>("active"), "draft": docs.get::<i64, _>("draft"), "archived": docs.get::<i64, _>("archived") },
            "invoices": {
                "draft": inv.get::<i64, _>("draft"), "submitted": inv.get::<i64, _>("submitted"), "approved": inv.get::<i64, _>("approved"),
                "rejected": inv.get::<i64, _>("rejected"), "paid": inv.get::<i64, _>("paid"), "openAmount": inv.get::<f64, _>("open_amount"),
                "overdue": inv.get::<i64, _>("overdue")
            },
            "byCostCenter": by_cost_center.iter().map(|r| json!({ "label": r.get::<String, _>("label"), "value": r.get::<f64, _>("value") })).collect::<Vec<_>>(),
            "awaitingApproval": recent,
        }))
    }
    .await
    .http()
}

async fn cost_centers(State(s): State<AppState>, ctx: RequestContext) -> ApiResult<Vec<Value>> {
    async {
        let rows = sqlx::query("select cost_center, name from usermgmt.departments where tenant_id = $1 order by cost_center")
            .bind(ctx.user.tenant_id)
            .fetch_all(s.core.db.pool())
            .await
            .db_err("documents.costCenters")?;
        Ok(rows.iter().map(|r| json!({ "value": r.get::<String, _>("cost_center"), "label": format!("{} – {}", r.get::<String, _>("cost_center"), r.get::<String, _>("name")) })).collect())
    }
    .await
    .http()
}

// ------------------------------------------------------------------ invoices endpoints

async fn list_invoices(State(s): State<AppState>, ctx: RequestContext, Query(q): Query<InvoiceQuery>) -> ApiResult<Page<InvoiceDto>> {
    invoices::list(&s, &ctx.user, q).await.http()
}
async fn get_invoice(State(s): State<AppState>, ctx: RequestContext, Path(id): Path<Uuid>) -> ApiResult<InvoiceDto> {
    invoices::get(&s, &ctx.user, id).await.http()
}
async fn create_invoice(State(s): State<AppState>, ctx: RequestContext, ReqJson(i): ReqJson<InvoiceInput>) -> Result<Created<InvoiceDto>, redhive_backend_core::AppError> {
    invoices::create(&s, &ctx, i).await.map(Created).map_err(Into::into)
}
async fn update_invoice(State(s): State<AppState>, ctx: RequestContext, Path(id): Path<Uuid>, ReqJson(i): ReqJson<InvoiceInput>) -> ApiResult<InvoiceDto> {
    invoices::update(&s, &ctx, id, i).await.http()
}
async fn submit_invoice(State(s): State<AppState>, ctx: RequestContext, Path(id): Path<Uuid>) -> ApiResult<InvoiceDto> {
    invoices::submit(&s, &ctx.user, id).await.http()
}
async fn approve_invoice(State(s): State<AppState>, ctx: RequestContext, Path(id): Path<Uuid>, OptJson(body): OptJson<DecisionInput>) -> ApiResult<InvoiceDto> {
    invoices::decide(&s, &ctx.user, id, true, body).await.http()
}
async fn reject_invoice(State(s): State<AppState>, ctx: RequestContext, Path(id): Path<Uuid>, OptJson(body): OptJson<DecisionInput>) -> ApiResult<InvoiceDto> {
    invoices::decide(&s, &ctx.user, id, false, body).await.http()
}
async fn pay_invoice(State(s): State<AppState>, ctx: RequestContext, Path(id): Path<Uuid>) -> ApiResult<InvoiceDto> {
    invoices::pay(&s, &ctx.user, id).await.http()
}

// ------------------------------------------------------------------ workflow steps

fn input_uuid(inputs: &Map<String, Value>, name: &str) -> Res<Uuid> {
    inputs
        .get(name)
        .and_then(Value::as_str)
        .and_then(|s| Uuid::parse_str(s.trim()).ok())
        .ok_or_else(|| Error::validation(format!("{name}.invalid"), "errors:invalidValue").for_field(name))
}

fn input_text(inputs: &Map<String, Value>, name: &str) -> Option<String> {
    inputs.get(name).and_then(|v| match v {
        Value::String(s) => Some(s.clone()),
        Value::Null => None,
        other => Some(other.to_string()),
    })
}

fn outputs(inv: &InvoiceDto) -> Value {
    json!({
        "invoiceId": inv.id, "number": inv.number, "supplier": inv.supplier, "amount": inv.amount, "currency": inv.currency,
        "costCenter": inv.cost_center, "status": inv.status, "submittedBy": inv.submitted_by_name, "dueOn": inv.due_on,
        "decidedBy": inv.decided_by_name
    })
}

/// Headless step handlers for the workflow engine (called with a delegated token of the RunAs user).
async fn workflow_step(State(s): State<AppState>, ctx: RequestContext, Path(step_type): Path<String>, ReqJson(inputs): ReqJson<Map<String, Value>>) -> ApiResult<Value> {
    async {
        let u = &ctx.user;
        match step_type.as_str() {
            "documents.lookupInvoice" => invoices::get(&s, u, input_uuid(&inputs, "invoiceId")?).await.map(|i| outputs(&i)),
            "documents.submitInvoice" => invoices::submit(&s, u, input_uuid(&inputs, "invoiceId")?).await.map(|i| outputs(&i)),
            "documents.approveInvoice" => {
                let i = invoices::decide(&s, u, input_uuid(&inputs, "invoiceId")?, true, DecisionInput { comment: input_text(&inputs, "comment"), row_version: None }).await?;
                Ok(json!({ "status": i.status, "decidedBy": i.decided_by_name }))
            }
            "documents.rejectInvoice" => {
                let i = invoices::decide(&s, u, input_uuid(&inputs, "invoiceId")?, false, DecisionInput { comment: input_text(&inputs, "comment"), row_version: None }).await?;
                Ok(json!({ "status": i.status, "decidedBy": i.decided_by_name }))
            }
            "documents.archiveInvoice" => invoices::archive(&s, u, input_uuid(&inputs, "invoiceId")?).await.map(|a| json!({ "archived": a })),
            "documents.send" => {
                u.require(PERM_VIEW)?;
                let id = input_uuid(&inputs, "documentId")?;
                let to = input_text(&inputs, "recipient").filter(|r| r.contains('@')).ok_or_else(|| Error::validation("recipient.invalid", "documents:recipient.invalid").for_field("recipient"))?;
                let mut tx = s.core.db.begin().await?;
                let d = load_doc(&mut tx, u.tenant_id, id).await?;
                let ev = s.outbox.create("documents.sent", u, NotificationAudience::System, json!({ "documentId": d.id, "title": d.title, "recipient": to }));
                s.outbox.add(&mut tx, &ev).await?;
                s.audit.write(&mut tx, u, "document.sent", "document", &id.to_string(), json!({ "recipient": to })).await?;
                tx.commit().await.db_err("documents.commit")?;
                Ok(json!({ "sent": true, "recipient": to }))
            }
            other => Err(Error::not_found("workflow.stepType.unknown", "workflow:stepType.unknown").with_metadata("stepType", other.to_string())),
        }
    }
    .await
    .http()
}

/// Condition endpoint for condition triggers: true when invoices are overdue.
async fn overdue_condition(State(s): State<AppState>, ctx: RequestContext) -> ApiResult<bool> {
    async {
        ctx.user.require(PERM_VIEW)?;
        let n: i64 = sqlx::query_scalar("select count(*) from documents.invoices where tenant_id = $1 and status in ('submitted', 'approved') and due_on < current_date")
            .bind(ctx.user.tenant_id)
            .fetch_one(s.core.db.pool())
            .await
            .db_err("documents.overdue")?;
        Ok(n > 0)
    }
    .await
    .http()
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let core = Core::init(SERVICE).await?;
    let state = AppState { core: core.clone(), outbox: OutboxWriter::new(SERVICE), audit: AuditLog::new(SERVICE) };
    let router = Router::new()
        .route("/api/documents", get(list_documents).post(create_document))
        .route("/api/documents/stats", get(stats))
        .route("/api/documents/lookups/cost-centers", get(cost_centers))
        .route("/api/documents/conditions/overdue", get(overdue_condition))
        .route("/api/documents/invoices", get(list_invoices).post(create_invoice))
        .route("/api/documents/invoices/{id}", get(get_invoice).put(update_invoice))
        .route("/api/documents/invoices/{id}/submit", post(submit_invoice))
        .route("/api/documents/invoices/{id}/approve", post(approve_invoice))
        .route("/api/documents/invoices/{id}/reject", post(reject_invoice))
        .route("/api/documents/invoices/{id}/pay", post(pay_invoice))
        .route("/api/documents/workflow-steps/{step_type}", post(workflow_step))
        .route("/api/documents/{id}", get(get_document))
        .route("/api/documents/{id}/versions", post(add_version))
        .route("/api/documents/{id}/archive", post(archive_document))
        .route("/api/documents/{id}/activate", post(activate_document));
    serve(core, router, state).await
}
