//! Red Hive module backend **reporting** (`reporting-service`): report catalogue, parameter forms,
//! report runs with chart + table results.

use axum::extract::{Path, State};
use axum::routing::{get, post};
use axum::Router;
use chrono::{DateTime, NaiveDate, Utc};
use redhive_backend_core::db::DbErrorExt;
use redhive_backend_core::hosting::{serve, Core};
use redhive_backend_core::{ApiResult, AuditLog, OutboxWriter, RedHiveUser, RequestContext, ToHttp, OptJson, ReqJson};
use redhive_contracts::notifications::{NotificationAudience, Severity};
use redhive_contracts::workflows::input::{InputField, InputSchema};
use redhive_monads::{Error, OptionExt, Res};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use sqlx::Row;
use uuid::Uuid;

const SERVICE: &str = "reporting-service";
const PERM_VIEW: &str = "reports.view";

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

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ReportDefinition {
    id: String,
    name: String,
    description: String,
    category: String,
    chart: String,
    permission: String,
    parameters: InputSchema,
}

async fn definitions(core: &Core) -> Res<Vec<ReportDefinition>> {
    let rows = sqlx::query("select id, name, description, category, chart, parameters, permission from reporting.report_definitions order by category, name")
        .fetch_all(core.db.pool())
        .await
        .db_err("reports.catalog")?;
    Ok(rows
        .into_iter()
        .map(|r| {
            let id: String = r.get("id");
            let fields: Vec<InputField> = serde_json::from_value(r.get("parameters")).unwrap_or_default();
            ReportDefinition {
                parameters: InputSchema { schema_id: format!("reports.{id}"), version: 1, fields },
                id,
                name: r.get("name"),
                description: r.get("description"),
                category: r.get("category"),
                chart: r.get("chart"),
                permission: r.get("permission"),
            }
        })
        .collect())
}

async fn catalog(State(s): State<AppState>, ctx: RequestContext) -> ApiResult<Vec<ReportDefinition>> {
    async {
        ctx.user.require(PERM_VIEW)?;
        Ok(definitions(&s.core).await?.into_iter().filter(|d| ctx.user.has(&d.permission)).collect())
    }
    .await
    .http()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Column {
    field: String,
    label: String,
    format: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReportResult {
    report_id: String,
    name: String,
    columns: Vec<Column>,
    rows: Vec<Value>,
    chart: Value,
    total: f64,
    row_count: usize,
}

fn col(field: &str, label: &str, format: &str) -> Column {
    Column { field: field.into(), label: label.into(), format: format.into() }
}

fn date_param(p: &Map<String, Value>, name: &str, default: NaiveDate) -> NaiveDate {
    p.get(name).and_then(Value::as_str).and_then(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d").ok()).unwrap_or(default)
}

/// Executes a report. Queries are fixed per report id; parameters are bound, never interpolated.
async fn execute(core: &Core, def: &ReportDefinition, p: &Map<String, Value>) -> Res<ReportResult> {
    let today = Utc::now().date_naive();
    let year_start = NaiveDate::from_ymd_opt(today.format("%Y").to_string().parse().unwrap_or(2026), 1, 1).unwrap_or(today);
    let pool = core.db.pool();
    let (columns, rows, label, value): (Vec<Column>, Vec<Value>, &str, &str) = match def.id.as_str() {
        "sales-by-region" => {
            let rows = sqlx::query("select region, sum(revenue)::float8 as revenue, sum(quantity)::int8 as units from reporting.sales where sold_on between $1 and $2 group by region order by revenue desc")
                .bind(date_param(p, "from", year_start))
                .bind(date_param(p, "to", today))
                .fetch_all(pool)
                .await
                .db_err("reports.run")?;
            (
                vec![col("region", "reports:col.region", "text"), col("revenue", "reports:col.revenue", "money"), col("units", "reports:col.units", "number")],
                rows.iter().map(|r| json!({ "region": r.get::<String, _>("region"), "revenue": r.get::<f64, _>("revenue"), "units": r.get::<i64, _>("units") })).collect(),
                "region",
                "revenue",
            )
        }
        "sales-by-month" => {
            let year = p.get("year").and_then(|v| v.as_f64().or_else(|| v.as_str().and_then(|s| s.parse().ok()))).unwrap_or(2026.0) as i32;
            let rows = sqlx::query("select to_char(sold_on, 'YYYY-MM') as month, sum(revenue)::float8 as revenue, sum(quantity)::int8 as units from reporting.sales where extract(year from sold_on) = $1 group by 1 order by 1")
                .bind(year as f64)
                .fetch_all(pool)
                .await
                .db_err("reports.run")?;
            (
                vec![col("month", "reports:col.month", "text"), col("revenue", "reports:col.revenue", "money"), col("units", "reports:col.units", "number")],
                rows.iter().map(|r| json!({ "month": r.get::<String, _>("month"), "revenue": r.get::<f64, _>("revenue"), "units": r.get::<i64, _>("units") })).collect(),
                "month",
                "revenue",
            )
        }
        "top-products" => {
            let top = p.get("top").and_then(|v| v.as_i64().or_else(|| v.as_str().and_then(|s| s.parse().ok()))).unwrap_or(5).clamp(1, 20);
            let rows = sqlx::query("select product, sum(revenue)::float8 as revenue, sum(quantity)::int8 as units from reporting.sales where sold_on between $1 and $2 group by product order by revenue desc limit $3")
                .bind(date_param(p, "from", year_start))
                .bind(date_param(p, "to", today))
                .bind(top)
                .fetch_all(pool)
                .await
                .db_err("reports.run")?;
            (
                vec![col("product", "reports:col.product", "text"), col("revenue", "reports:col.revenue", "money"), col("units", "reports:col.units", "number")],
                rows.iter().map(|r| json!({ "product": r.get::<String, _>("product"), "revenue": r.get::<f64, _>("revenue"), "units": r.get::<i64, _>("units") })).collect(),
                "product",
                "revenue",
            )
        }
        "invoice-aging" => {
            let rows = sqlx::query(
                "select bucket, count(*)::int8 as invoices, sum(amount)::float8 as amount, min(o) as o from (
                    select amount, case when due_on < current_date then 'overdue' when due_on - current_date <= 7 then '0-7 days'
                                        when due_on - current_date <= 30 then '8-30 days' else '> 30 days' end as bucket,
                                   case when due_on < current_date then 0 when due_on - current_date <= 7 then 1 when due_on - current_date <= 30 then 2 else 3 end as o
                    from documents.invoices where status in ('submitted', 'approved')) x group by bucket order by o",
            )
            .fetch_all(pool)
            .await
            .db_err("reports.run")?;
            (
                vec![col("bucket", "reports:col.bucket", "text"), col("invoices", "reports:col.invoices", "number"), col("amount", "reports:col.amount", "money")],
                rows.iter().map(|r| json!({ "bucket": r.get::<String, _>("bucket"), "invoices": r.get::<i64, _>("invoices"), "amount": r.get::<f64, _>("amount") })).collect(),
                "bucket",
                "amount",
            )
        }
        "approval-throughput" => {
            let rows = sqlx::query(
                "select coalesce(decided_by_name, '–') as approver, count(*) filter (where status in ('approved', 'paid'))::int8 as approved,
                        count(*) filter (where status = 'rejected')::int8 as rejected, coalesce(sum(amount) filter (where status in ('approved', 'paid')), 0)::float8 as volume
                 from documents.invoices where decided_by is not null group by 1 order by approved desc",
            )
            .fetch_all(pool)
            .await
            .db_err("reports.run")?;
            (
                vec![col("approver", "reports:col.approver", "text"), col("approved", "reports:col.approved", "number"), col("rejected", "reports:col.rejected", "number"), col("volume", "reports:col.volume", "money")],
                rows.iter().map(|r| json!({ "approver": r.get::<String, _>("approver"), "approved": r.get::<i64, _>("approved"), "rejected": r.get::<i64, _>("rejected"), "volume": r.get::<f64, _>("volume") })).collect(),
                "approver",
                "approved",
            )
        }
        other => return Err(Error::not_found("reports.unknown", "reports:report.notFound").with_metadata("report", other.to_string())),
    };
    let total = rows.iter().filter_map(|r| r.get(value).and_then(Value::as_f64)).sum::<f64>();
    Ok(ReportResult {
        report_id: def.id.clone(),
        name: def.name.clone(),
        row_count: rows.len(),
        columns,
        rows,
        chart: json!({ "labelField": label, "valueField": value, "kind": def.chart }),
        total: (total * 100.0).round() / 100.0,
    })
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct RunDto {
    id: Uuid,
    report_id: String,
    parameters: Value,
    requested_by_name: String,
    status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    created_at: DateTime<Utc>,
    finished_at: Option<DateTime<Utc>>,
}

#[derive(Deserialize, Default)]
struct RunRequest {
    #[serde(default)]
    parameters: Map<String, Value>,
}

/// validate parameters → authorize → run → store → notify.
async fn run(s: &AppState, user: &RedHiveUser, report_id: &str, params: Map<String, Value>) -> Res<(Uuid, ReportResult)> {
    let def = definitions(&s.core).await?.into_iter().find(|d| d.id == report_id).ok_or_not_found("reports.unknown", "reports:report.notFound")?;
    let params = def.parameters.validate(&params)?;
    user.require(&def.permission)?;
    let result = execute(&s.core, &def, &params).await?;
    let id = Uuid::new_v4();
    let mut tx = s.core.db.begin().await?;
    sqlx::query(
        "insert into reporting.report_runs (id, report_id, parameters, requested_by, requested_by_name, status, result, finished_at) values ($1, $2, $3, $4, $5, 'completed', $6, now())",
    )
    .bind(id)
    .bind(report_id)
    .bind(Value::Object(params.clone()))
    .bind(user.id)
    .bind(user.audit_name())
    .bind(serde_json::to_value(&result).unwrap_or_default())
    .execute(&mut *tx)
    .await
    .db_err("reports.store")?;
    let mut ev = s
        .outbox
        .create("reports.ready", user, NotificationAudience::User(user.id), json!({ "runId": id, "reportId": report_id, "rowCount": result.row_count, "total": result.total }))
        .titled("reports:notification.ready", Severity::Success)
        .navigate("reports.catalog", Some(report_id.to_string()));
    ev.text_args.insert("report".into(), json!(def.name));
    s.outbox.add(&mut tx, &ev).await?;
    s.audit.write(&mut tx, user, "report.run", "report", report_id, Value::Object(params)).await?;
    tx.commit().await.db_err("reports.commit")?;
    Ok((id, result))
}

async fn run_endpoint(State(s): State<AppState>, ctx: RequestContext, Path(id): Path<String>, OptJson(body): OptJson<RunRequest>) -> ApiResult<Value> {
    async {
        let (run_id, result) = run(&s, &ctx.user, &id, body.parameters).await?;
        let mut v = serde_json::to_value(result).unwrap_or_default();
        v["runId"] = json!(run_id);
        Ok(v)
    }
    .await
    .http()
}

async fn runs(State(s): State<AppState>, ctx: RequestContext) -> ApiResult<Vec<RunDto>> {
    async {
        ctx.user.require(PERM_VIEW)?;
        let rows = sqlx::query(
            "select id, report_id, parameters, requested_by_name, status, created_at, finished_at from reporting.report_runs where requested_by = $1 order by created_at desc limit 50",
        )
        .bind(ctx.user.id)
        .fetch_all(s.core.db.pool())
        .await
        .db_err("reports.runs")?;
        Ok(rows
            .into_iter()
            .map(|r| RunDto {
                id: r.get("id"),
                report_id: r.get("report_id"),
                parameters: r.get("parameters"),
                requested_by_name: r.get("requested_by_name"),
                status: r.get("status"),
                result: None,
                created_at: r.get("created_at"),
                finished_at: r.get("finished_at"),
            })
            .collect())
    }
    .await
    .http()
}

async fn get_run(State(s): State<AppState>, ctx: RequestContext, Path(id): Path<Uuid>) -> ApiResult<RunDto> {
    async {
        ctx.user.require(PERM_VIEW)?;
        let r = sqlx::query("select id, report_id, parameters, requested_by_name, status, result, created_at, finished_at from reporting.report_runs where id = $1 and requested_by = $2")
            .bind(id)
            .bind(ctx.user.id)
            .fetch_optional(s.core.db.pool())
            .await
            .db_err("reports.run")?
            .ok_or_not_found("reports.run", "reports:run.notFound")?;
        Ok(RunDto {
            id: r.get("id"),
            report_id: r.get("report_id"),
            parameters: r.get("parameters"),
            requested_by_name: r.get("requested_by_name"),
            status: r.get("status"),
            result: r.get("result"),
            created_at: r.get("created_at"),
            finished_at: r.get("finished_at"),
        })
    }
    .await
    .http()
}

async fn workflow_step(State(s): State<AppState>, ctx: RequestContext, Path(step): Path<String>, ReqJson(inputs): ReqJson<Map<String, Value>>) -> ApiResult<Value> {
    async {
        match step.as_str() {
            "reports.runReport" => {
                let report = inputs.get("reportId").and_then(Value::as_str).ok_or_else(|| Error::validation("reportId.required", "errors:required").for_field("reportId"))?;
                let params = match inputs.get("parameters") {
                    Some(Value::Object(o)) => o.clone(),
                    Some(Value::String(s)) if !s.trim().is_empty() => serde_json::from_str(s).map_err(|_| Error::validation("parameters.invalid", "errors:invalidJson").for_field("parameters"))?,
                    _ => Map::new(),
                };
                let (id, r) = run(&s, &ctx.user, report, params).await?;
                Ok(json!({ "runId": id, "rowCount": r.row_count, "total": r.total }))
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
        .route("/api/reports/catalog", get(catalog))
        .route("/api/reports/runs", get(runs))
        .route("/api/reports/runs/{id}", get(get_run))
        .route("/api/reports/workflow-steps/{step}", post(workflow_step))
        .route("/api/reports/{id}/run", post(run_endpoint));
    serve(core, router, state).await
}
