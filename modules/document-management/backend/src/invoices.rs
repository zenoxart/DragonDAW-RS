//! Invoice use cases. Every one is a railway pipeline:
//! **validate → authorize → load → decide (pure) → persist + event (one transaction) → project**.
//! HTTP endpoints, workflow steps and AI tools all call these, so people, the engine and the AI get the
//! same rules and errors.

use crate::{AppState, PERM_APPROVE, PERM_EDIT, PERM_VIEW};
use chrono::{DateTime, NaiveDate, Utc};
use redhive_backend_core::db::{ensure_updated, DbErrorExt, Tx};
use redhive_backend_core::{RedHiveUser, RequestContext};
use redhive_contracts::notifications::{NotificationAudience, Severity};
use redhive_monads::{Error, OptionExt, Res, Validator};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::{FromRow, Row};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, FromRow, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct InvoiceDto {
    pub id: Uuid,
    pub number: String,
    pub supplier: String,
    pub amount: f64,
    pub currency: String,
    pub issued_on: NaiveDate,
    pub due_on: NaiveDate,
    pub cost_center: String,
    pub status: String,
    pub submitted_by: Option<Uuid>,
    pub submitted_by_name: Option<String>,
    pub submitted_at: Option<DateTime<Utc>>,
    pub decided_by: Option<Uuid>,
    pub decided_by_name: Option<String>,
    pub decided_at: Option<DateTime<Utc>>,
    pub comment: String,
    pub document_id: Option<Uuid>,
    pub row_version: i32,
}

pub const INVOICE_SELECT: &str = "select id, number, supplier, amount::float8 as amount, currency, issued_on, due_on, cost_center, status, submitted_by,
    submitted_by_name, submitted_at, decided_by, decided_by_name, decided_at, comment, document_id, row_version from documents.invoices";

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InvoiceInput {
    pub supplier: String,
    pub amount: f64,
    #[serde(default = "eur")]
    pub currency: String,
    pub issued_on: NaiveDate,
    pub due_on: NaiveDate,
    pub cost_center: String,
    #[serde(default)]
    pub comment: Option<String>,
    #[serde(default)]
    pub row_version: Option<i32>,
}

fn eur() -> String {
    "EUR".into()
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct DecisionInput {
    #[serde(default)]
    pub comment: Option<String>,
    #[serde(default)]
    pub row_version: Option<i32>,
}

// ------------------------------------------------------------------ pure rules

pub fn validate_input(i: &InvoiceInput) -> Res<()> {
    Validator::new("documents")
        .require("supplier", !i.supplier.trim().is_empty())
        .check("supplier", i.supplier.chars().count() <= 200, "tooLong")
        .check("amount", i.amount >= 0.0 && i.amount.is_finite(), "negative")
        .check("amount", i.amount <= 10_000_000.0, "tooHigh")
        .check("currency", i.currency.len() == 3 && i.currency.chars().all(|c| c.is_ascii_uppercase()), "invalid")
        .check("dueOn", i.due_on >= i.issued_on, "beforeIssued")
        .require("costCenter", !i.cost_center.trim().is_empty())
        .finish(())
}

/// May `user` approve or reject this invoice?
pub fn decide_approval(inv: &InvoiceDto, user: &RedHiveUser, approve: bool, comment: &str) -> Res<&'static str> {
    if inv.status != "submitted" {
        return Err(Error::conflict("documents.invoice.notSubmitted", "documents:invoice.notSubmitted").with_metadata("status", inv.status.clone()));
    }
    if inv.submitted_by == Some(user.id) {
        return Err(Error::forbidden("documents.invoice.selfApproval", "documents:invoice.selfApproval"));
    }
    if approve && inv.amount > user.approval_limit {
        return Err(Error::forbidden("documents.invoice.limitExceeded", "documents:invoice.limitExceeded")
            .with_metadata("limit", user.approval_limit)
            .with_metadata("amount", inv.amount));
    }
    if !approve && comment.trim().is_empty() {
        return Err(Error::validation("documents.invoice.reasonRequired", "documents:invoice.reasonRequired").for_field("comment"));
    }
    Ok(if approve { "approved" } else { "rejected" })
}

pub fn decide_submit(inv: &InvoiceDto) -> Res<()> {
    match inv.status.as_str() {
        "draft" | "rejected" => Ok(()),
        s => Err(Error::conflict("documents.invoice.notDraft", "documents:invoice.notDraft").with_metadata("status", s.to_string())),
    }
}

// ------------------------------------------------------------------ persistence

pub async fn find(tx: &mut Tx<'static>, tenant: Uuid, id: Uuid) -> Res<InvoiceDto> {
    sqlx::query_as::<_, InvoiceDto>(&format!("{INVOICE_SELECT} where id = $1 and tenant_id = $2 for update"))
        .bind(id)
        .bind(tenant)
        .fetch_optional(&mut **tx)
        .await
        .db_err("documents.invoice")?
        .ok_or_not_found("documents.invoice", "documents:invoice.notFound")
}

pub async fn get(s: &AppState, user: &RedHiveUser, id: Uuid) -> Res<InvoiceDto> {
    user.require(PERM_VIEW)?;
    sqlx::query_as::<_, InvoiceDto>(&format!("{INVOICE_SELECT} where id = $1 and tenant_id = $2"))
        .bind(id)
        .bind(user.tenant_id)
        .fetch_optional(s.core.db.pool())
        .await
        .db_err("documents.invoice")?
        .ok_or_not_found("documents.invoice", "documents:invoice.notFound")
}

fn payload(inv: &InvoiceDto) -> serde_json::Value {
    json!({
        "invoiceId": inv.id, "number": inv.number, "supplier": inv.supplier, "amount": inv.amount, "currency": inv.currency,
        "costCenter": inv.cost_center, "status": inv.status, "category": "invoice", "submittedBy": inv.submitted_by,
        "submittedByName": inv.submitted_by_name, "dueOn": inv.due_on, "decidedByName": inv.decided_by_name, "comment": inv.comment, "documentId": inv.document_id
    })
}

fn args(inv: &InvoiceDto) -> serde_json::Map<String, serde_json::Value> {
    let mut m = serde_json::Map::new();
    m.insert("number".into(), json!(inv.number));
    m.insert("supplier".into(), json!(inv.supplier));
    m.insert("amount".into(), json!(format!("{:.2} {}", inv.amount, inv.currency)));
    m
}

// ------------------------------------------------------------------ use cases

pub async fn create(s: &AppState, ctx: &RequestContext, input: InvoiceInput) -> Res<InvoiceDto> {
    validate_input(&input)?;
    ctx.user.require(PERM_EDIT)?;
    let user = ctx.user.clone();
    let mut tx = s.core.db.begin().await?;
    let year = Utc::now().format("%Y");
    let next: i64 = sqlx::query_scalar("select count(*) + 1 from documents.invoices where tenant_id = $1 and number like $2")
        .bind(user.tenant_id)
        .bind(format!("INV-{year}-%"))
        .fetch_one(&mut *tx)
        .await
        .db_err("documents.invoice.number")?;
    let mut number = format!("INV-{year}-{next:04}");
    // keep numbers unique even after deletions
    while sqlx::query_scalar::<_, i64>("select count(*) from documents.invoices where number = $1").bind(&number).fetch_one(&mut *tx).await.db_err("documents.invoice.number")? > 0 {
        number = format!("INV-{year}-{:04}", rand::random::<u16>() % 9000 + 1000);
    }
    let id = Uuid::new_v4();
    sqlx::query(
        "insert into documents.invoices (id, tenant_id, number, supplier, amount, currency, issued_on, due_on, cost_center, status, comment)
         values ($1, $2, $3, $4, $5::numeric, $6, $7, $8, $9, 'draft', $10)",
    )
    .bind(id)
    .bind(user.tenant_id)
    .bind(&number)
    .bind(input.supplier.trim())
    .bind(input.amount)
    .bind(&input.currency)
    .bind(input.issued_on)
    .bind(input.due_on)
    .bind(&input.cost_center)
    .bind(input.comment.unwrap_or_default())
    .execute(&mut *tx)
    .await
    .db_err("documents.invoice.create")?;
    let inv = find(&mut tx, user.tenant_id, id).await?;
    let ev = s.outbox.create("documents.invoice.created", &user, NotificationAudience::WithPermission(PERM_VIEW.into()), payload(&inv));
    s.outbox.add(&mut tx, &ev).await?;
    s.audit.write(&mut tx, &user, "invoice.created", "invoice", &id.to_string(), payload(&inv)).await?;
    tx.commit().await.db_err("documents.commit")?;
    Ok(inv)
}

pub async fn update(s: &AppState, ctx: &RequestContext, id: Uuid, input: InvoiceInput) -> Res<InvoiceDto> {
    validate_input(&input)?;
    ctx.user.require(PERM_EDIT)?;
    let user = ctx.user.clone();
    let mut tx = s.core.db.begin().await?;
    let inv = find(&mut tx, user.tenant_id, id).await?;
    decide_submit(&inv)?; // only drafts / rejected invoices can be edited
    let rows = sqlx::query(
        "update documents.invoices set supplier = $3, amount = $4::numeric, currency = $5, issued_on = $6, due_on = $7, cost_center = $8,
                comment = coalesce($9, comment), row_version = row_version + 1
         where id = $1 and row_version = $2",
    )
    .bind(id)
    .bind(input.row_version.unwrap_or(inv.row_version))
    .bind(input.supplier.trim())
    .bind(input.amount)
    .bind(&input.currency)
    .bind(input.issued_on)
    .bind(input.due_on)
    .bind(&input.cost_center)
    .bind(input.comment)
    .execute(&mut *tx)
    .await
    .db_err("documents.invoice.update")?
    .rows_affected();
    ensure_updated(rows, "documents.invoice")?;
    let inv = find(&mut tx, user.tenant_id, id).await?;
    let ev = s.outbox.create("documents.invoice.updated", &user, NotificationAudience::WithPermission(PERM_VIEW.into()), payload(&inv));
    s.outbox.add(&mut tx, &ev).await?;
    s.audit.write(&mut tx, &user, "invoice.updated", "invoice", &id.to_string(), json!({})).await?;
    tx.commit().await.db_err("documents.commit")?;
    Ok(inv)
}

pub async fn submit(s: &AppState, user: &RedHiveUser, id: Uuid) -> Res<InvoiceDto> {
    user.require(PERM_EDIT)?;
    let mut tx = s.core.db.begin().await?;
    let inv = find(&mut tx, user.tenant_id, id).await?;
    decide_submit(&inv)?;
    sqlx::query(
        "update documents.invoices set status = 'submitted', submitted_by = $2, submitted_by_name = $3, submitted_at = now(),
                decided_by = null, decided_by_name = null, decided_at = null, row_version = row_version + 1 where id = $1",
    )
    .bind(id)
    .bind(user.id)
    .bind(&user.display_name)
    .execute(&mut *tx)
    .await
    .db_err("documents.invoice.submit")?;
    let inv = find(&mut tx, user.tenant_id, id).await?;
    let ev = s
        .outbox
        .create("documents.invoice.submitted", user, NotificationAudience::WithPermission(PERM_APPROVE.into()), payload(&inv))
        .titled("documents:notification.submitted", Severity::Info)
        .navigate("documents.invoices", Some(id.to_string()))
        .with_args(args(&inv));
    s.outbox.add(&mut tx, &ev).await?;
    s.audit.write(&mut tx, user, "invoice.submitted", "invoice", &id.to_string(), json!({ "amount": inv.amount })).await?;
    tx.commit().await.db_err("documents.commit")?;
    Ok(inv)
}

/// Approve or reject: limit and self-approval are checked against the **acting** user – a workflow running
/// as Maria with a delegated token gets exactly Maria's limit.
pub async fn decide(s: &AppState, user: &RedHiveUser, id: Uuid, approve: bool, input: DecisionInput) -> Res<InvoiceDto> {
    user.require(PERM_APPROVE)?;
    let comment = input.comment.unwrap_or_default();
    let mut tx = s.core.db.begin().await?;
    let inv = find(&mut tx, user.tenant_id, id).await?;
    if let Some(rv) = input.row_version {
        if rv != inv.row_version {
            return Err(Error::conflict("documents.invoice.stale", "errors:staleVersion"));
        }
    }
    let new_status = match decide_approval(&inv, user, approve, &comment) {
        Ok(s) => s,
        Err(e) => {
            drop(tx);
            s.audit.failure(s.core.db.pool(), user, if approve { "invoice.approve" } else { "invoice.reject" }, "invoice", &id.to_string(), &e).await;
            return Err(e);
        }
    };
    sqlx::query("update documents.invoices set status = $2, decided_by = $3, decided_by_name = $4, decided_at = now(), comment = $5, row_version = row_version + 1 where id = $1")
        .bind(id)
        .bind(new_status)
        .bind(user.id)
        .bind(user.audit_name())
        .bind(&comment)
        .execute(&mut *tx)
        .await
        .db_err("documents.invoice.decide")?;
    let inv = find(&mut tx, user.tenant_id, id).await?;
    let audience = inv.submitted_by.map(NotificationAudience::User).unwrap_or(NotificationAudience::WithPermission(PERM_EDIT.into()));
    let (event, title, severity) = if approve {
        ("documents.invoice.approved", "documents:notification.approved", Severity::Success)
    } else {
        ("documents.invoice.rejected", "documents:notification.rejected", Severity::Warning)
    };
    let ev = s.outbox.create(event, user, audience, payload(&inv)).titled(title, severity).navigate("documents.invoices", Some(id.to_string())).with_args(args(&inv));
    s.outbox.add(&mut tx, &ev).await?;
    s.audit.write(&mut tx, user, &format!("invoice.{new_status}"), "invoice", &id.to_string(), json!({ "amount": inv.amount, "comment": comment })).await?;
    tx.commit().await.db_err("documents.commit")?;
    Ok(inv)
}

pub async fn pay(s: &AppState, user: &RedHiveUser, id: Uuid) -> Res<InvoiceDto> {
    user.require(PERM_APPROVE)?;
    let mut tx = s.core.db.begin().await?;
    let inv = find(&mut tx, user.tenant_id, id).await?;
    if inv.status != "approved" {
        return Err(Error::conflict("documents.invoice.notApproved", "documents:invoice.notApproved"));
    }
    sqlx::query("update documents.invoices set status = 'paid', row_version = row_version + 1 where id = $1").bind(id).execute(&mut *tx).await.db_err("documents.invoice.pay")?;
    let inv = find(&mut tx, user.tenant_id, id).await?;
    let ev = s.outbox.create("documents.invoice.paid", user, NotificationAudience::WithPermission(PERM_VIEW.into()), payload(&inv));
    s.outbox.add(&mut tx, &ev).await?;
    s.audit.write(&mut tx, user, "invoice.paid", "invoice", &id.to_string(), json!({})).await?;
    tx.commit().await.db_err("documents.commit")?;
    Ok(inv)
}

/// Archives the invoice's document (after a workflow finished).
pub async fn archive(s: &AppState, user: &RedHiveUser, id: Uuid) -> Res<bool> {
    user.require(PERM_EDIT)?;
    let mut tx = s.core.db.begin().await?;
    let inv = find(&mut tx, user.tenant_id, id).await?;
    if !matches!(inv.status.as_str(), "approved" | "rejected" | "paid") {
        return Err(Error::conflict("documents.invoice.open", "documents:invoice.stillOpen"));
    }
    let n = sqlx::query("update documents.documents set status = 'archived', updated_at = now(), row_version = row_version + 1 where id = $1 and status <> 'archived'")
        .bind(inv.document_id)
        .execute(&mut *tx)
        .await
        .db_err("documents.archive")?
        .rows_affected();
    s.audit.write(&mut tx, user, "invoice.archived", "invoice", &id.to_string(), json!({})).await?;
    tx.commit().await.db_err("documents.commit")?;
    Ok(n > 0)
}

#[derive(Debug, Deserialize, Default)]
pub struct InvoiceQuery {
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub search: Option<String>,
    #[serde(default)]
    pub skip: Option<i64>,
    #[serde(default)]
    pub take: Option<i64>,
}

pub async fn list(s: &AppState, user: &RedHiveUser, q: InvoiceQuery) -> Res<redhive_contracts::primitives::Page<InvoiceDto>> {
    user.require(PERM_VIEW)?;
    let skip = q.skip.unwrap_or(0).max(0);
    let take = q.take.unwrap_or(100).clamp(1, 500);
    let status = q.status.filter(|s| !s.is_empty() && s != "all");
    let search = q.search.filter(|s| !s.trim().is_empty()).map(|s| format!("%{}%", s.trim().to_lowercase()));
    let filter = "tenant_id = $1 and ($2::text is null or status = $2) and ($3::text is null or lower(number || ' ' || supplier || ' ' || cost_center) like $3)";
    let items = sqlx::query_as::<_, InvoiceDto>(&format!("{INVOICE_SELECT} where {filter} order by coalesce(submitted_at, created_at) desc limit $4 offset $5"))
        .bind(user.tenant_id)
        .bind(&status)
        .bind(&search)
        .bind(take)
        .bind(skip)
        .fetch_all(s.core.db.pool())
        .await
        .db_err("documents.invoices")?;
    let total: i64 = sqlx::query(&format!("select count(*) as n from documents.invoices where {filter}"))
        .bind(user.tenant_id)
        .bind(&status)
        .bind(&search)
        .fetch_one(s.core.db.pool())
        .await
        .db_err("documents.invoices.count")?
        .get("n");
    Ok(redhive_contracts::primitives::Page { items, total, skip, take })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inv(status: &str, amount: f64, submitted_by: Uuid) -> InvoiceDto {
        InvoiceDto {
            id: Uuid::nil(),
            number: "INV-1".into(),
            supplier: "ACME".into(),
            amount,
            currency: "EUR".into(),
            issued_on: NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
            due_on: NaiveDate::from_ymd_opt(2026, 1, 31).unwrap(),
            cost_center: "CC-1".into(),
            status: status.into(),
            submitted_by: Some(submitted_by),
            submitted_by_name: None,
            submitted_at: None,
            decided_by: None,
            decided_by_name: None,
            decided_at: None,
            comment: String::new(),
            document_id: None,
            row_version: 1,
        }
    }

    fn approver(limit: f64) -> RedHiveUser {
        RedHiveUser {
            id: Uuid::from_u128(2),
            tenant_id: Uuid::nil(),
            user_name: "maria".into(),
            display_name: "Maria".into(),
            roles: vec![],
            permissions: vec![PERM_APPROVE.into()],
            approval_limit: limit,
            actor: None,
            instance_id: None,
        }
    }

    #[test]
    fn approval_rules() {
        let clerk = Uuid::from_u128(3);
        assert_eq!(decide_approval(&inv("submitted", 900.0, clerk), &approver(10_000.0), true, "").unwrap(), "approved");
        assert_eq!(decide_approval(&inv("submitted", 24_500.0, clerk), &approver(10_000.0), true, "").unwrap_err().code, "documents.invoice.limitExceeded");
        assert_eq!(decide_approval(&inv("submitted", 10.0, Uuid::from_u128(2)), &approver(10_000.0), true, "").unwrap_err().code, "documents.invoice.selfApproval");
        assert_eq!(decide_approval(&inv("submitted", 10.0, clerk), &approver(10_000.0), false, " ").unwrap_err().code, "documents.invoice.reasonRequired");
        assert_eq!(decide_approval(&inv("draft", 10.0, clerk), &approver(10_000.0), true, "").unwrap_err().code, "documents.invoice.notSubmitted");
    }

    #[test]
    fn input_rules() {
        let mut i = InvoiceInput {
            supplier: " ".into(),
            amount: -5.0,
            currency: "eur".into(),
            issued_on: NaiveDate::from_ymd_opt(2026, 2, 1).unwrap(),
            due_on: NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
            cost_center: "CC".into(),
            comment: None,
            row_version: None,
        };
        let e = validate_input(&i).unwrap_err();
        assert_eq!(e.field_errors.len(), 4);
        i.supplier = "ACME".into();
        i.amount = 5.0;
        i.currency = "EUR".into();
        i.due_on = NaiveDate::from_ymd_opt(2026, 3, 1).unwrap();
        assert!(validate_input(&i).is_ok());
    }
}
