//! Persistence of definitions, dependencies, instances and impact reports.

use redhive_backend_core::db::{DbErrorExt, Tx};
use redhive_backend_core::hosting::Core;
use redhive_contracts::workflows::impact::ImpactReport;
use redhive_contracts::workflows::{InstanceState, WorkflowDefinition, WorkflowInstance};
use redhive_monads::{Error, OptionExt, Res};
use sqlx::Row;
use uuid::Uuid;

pub fn parse_def(v: serde_json::Value) -> Res<WorkflowDefinition> {
    serde_json::from_value(v).map_err(|e| Error::unexpected("workflow.definition.corrupt", "errors:unexpected").with_message(e.to_string()))
}

pub fn parse_instance(v: serde_json::Value) -> Res<WorkflowInstance> {
    serde_json::from_value(v).map_err(|e| Error::unexpected("workflow.instance.corrupt", "errors:unexpected").with_message(e.to_string()))
}

/// Latest version of every workflow of a tenant.
pub async fn latest_all(core: &Core, tenant: Uuid) -> Res<Vec<WorkflowDefinition>> {
    let rows = sqlx::query("select distinct on (id) definition from workflow.definitions where tenant_id = $1 order by id, version desc")
        .bind(tenant)
        .fetch_all(core.db.pool())
        .await
        .db_err("workflow.list")?;
    rows.into_iter().map(|r| parse_def(r.get("definition"))).collect()
}

/// Latest versions of all tenants (trigger evaluation).
pub async fn latest_active_everywhere(core: &Core) -> Res<Vec<(Uuid, WorkflowDefinition)>> {
    let rows = sqlx::query(
        "select tenant_id, definition from (select distinct on (id) tenant_id, status, definition from workflow.definitions order by id, version desc) d where status = 'active'",
    )
    .fetch_all(core.db.pool())
    .await
    .db_err("workflow.active")?;
    rows.into_iter().map(|r| Ok((r.get("tenant_id"), parse_def(r.get("definition"))?))).collect()
}

pub async fn latest(core: &Core, tenant: Uuid, id: Uuid) -> Res<WorkflowDefinition> {
    let row = sqlx::query("select definition from workflow.definitions where id = $1 and tenant_id = $2 order by version desc limit 1")
        .bind(id)
        .bind(tenant)
        .fetch_optional(core.db.pool())
        .await
        .db_err("workflow.get")?
        .ok_or_not_found("workflow.definition", "workflow:definition.notFound")?;
    parse_def(row.get("definition"))
}

pub async fn version(core: &Core, id: Uuid, version: i32) -> Res<WorkflowDefinition> {
    let row = sqlx::query("select definition from workflow.definitions where id = $1 and version = $2")
        .bind(id)
        .bind(version)
        .fetch_optional(core.db.pool())
        .await
        .db_err("workflow.version")?
        .ok_or_not_found("workflow.definition", "workflow:definition.notFound")?;
    parse_def(row.get("definition"))
}

pub async fn versions(core: &Core, tenant: Uuid, id: Uuid) -> Res<Vec<WorkflowDefinition>> {
    let rows = sqlx::query("select definition from workflow.definitions where id = $1 and tenant_id = $2 order by version desc")
        .bind(id)
        .bind(tenant)
        .fetch_all(core.db.pool())
        .await
        .db_err("workflow.versions")?;
    rows.into_iter().map(|r| parse_def(r.get("definition"))).collect()
}

/// Inserts a definition version together with its dependency index.
pub async fn insert(tx: &mut Tx<'static>, tenant: Uuid, def: &WorkflowDefinition) -> Res<()> {
    sqlx::query(
        "insert into workflow.definitions (id, version, tenant_id, name, status, origin, owning_module, owner_id, definition)
         values ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
    )
    .bind(def.id)
    .bind(def.version)
    .bind(tenant)
    .bind(&def.name)
    .bind(def.status.as_str())
    .bind(serde_json::to_value(def.origin).ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default())
    .bind(&def.owning_module)
    .bind(def.owner_id)
    .bind(serde_json::to_value(def).unwrap_or_default())
    .execute(&mut **tx)
    .await
    .db_err("workflow.insert")?;
    for d in &def.dependencies {
        sqlx::query("insert into workflow.dependencies (workflow_id, version, kind, dep_id, dep_version, steps) values ($1, $2, $3, $4, $5, $6) on conflict do nothing")
            .bind(def.id)
            .bind(def.version)
            .bind(d.kind.as_str())
            .bind(&d.id)
            .bind(&d.version)
            .bind(&d.used_by_steps)
            .execute(&mut **tx)
            .await
            .db_err("workflow.dependencies")?;
    }
    Ok(())
}

pub async fn next_version(tx: &mut Tx<'static>, id: Uuid) -> Res<i32> {
    let v: Option<i32> = sqlx::query_scalar("select max(version) from workflow.definitions where id = $1").bind(id).fetch_one(&mut **tx).await.db_err("workflow.nextVersion")?;
    Ok(v.unwrap_or(0) + 1)
}

pub async fn running_count(core: &Core, id: Uuid) -> Res<i64> {
    sqlx::query_scalar("select count(*) from workflow.instances where workflow_id = $1 and state not in ('completed', 'failed', 'cancelled')")
        .bind(id)
        .fetch_one(core.db.pool())
        .await
        .db_err("workflow.running")
}

// ------------------------------------------------------------------ instances

pub async fn save_instance(core: &Core, i: &WorkflowInstance) -> Res<()> {
    let approver = i.pending.as_ref().and_then(|p| p.approvers.first().cloned());
    let waiting_event = (i.state == InstanceState::WaitingForEvent).then(|| i.vars.get("__waitEvent").and_then(|v| v.as_str()).unwrap_or("").to_string());
    let wait_until = i.vars.get("__waitUntil").and_then(|v| v.as_str()).and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok()).map(|d| d.with_timezone(&chrono::Utc));
    let retry_at = i.vars.get("__retryAt").and_then(|v| v.as_str()).and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok()).map(|d| d.with_timezone(&chrono::Utc));
    sqlx::query(
        "insert into workflow.instances (id, tenant_id, workflow_id, workflow_version, workflow_name, state, current_step_id, executor, run_as, run_as_name,
                approver_permission, waiting_event, wait_until, retry_at, attempts, parent_instance_id, instance, created_at, updated_at)
         values ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, now())
         on conflict (id) do update set workflow_version = excluded.workflow_version, state = excluded.state, current_step_id = excluded.current_step_id,
             approver_permission = excluded.approver_permission, waiting_event = excluded.waiting_event, wait_until = excluded.wait_until,
             retry_at = excluded.retry_at, attempts = excluded.attempts, instance = excluded.instance, updated_at = now()",
    )
    .bind(i.id)
    .bind(i.tenant_id)
    .bind(i.workflow_id)
    .bind(i.workflow_version)
    .bind(&i.workflow_name)
    .bind(i.state.as_str())
    .bind(&i.current_step_id)
    .bind(i.executor.as_str())
    .bind(i.run_as)
    .bind(&i.run_as_name)
    .bind(approver)
    .bind(waiting_event)
    .bind(wait_until)
    .bind(retry_at)
    .bind(i.attempts as i32)
    .bind(i.parent_instance_id)
    .bind(serde_json::to_value(i).unwrap_or_default())
    .bind(i.created_at)
    .execute(core.db.pool())
    .await
    .db_err("workflow.instance.save")?;
    Ok(())
}

pub async fn load_instance(core: &Core, id: Uuid) -> Res<WorkflowInstance> {
    let row = sqlx::query("select instance from workflow.instances where id = $1")
        .bind(id)
        .fetch_optional(core.db.pool())
        .await
        .db_err("workflow.instance")?
        .ok_or_not_found("workflow.instance", "workflow:instance.notFound")?;
    parse_instance(row.get("instance"))
}

pub async fn instances_where(core: &Core, filter: &str, tenant: Option<Uuid>, limit: i64) -> Res<Vec<WorkflowInstance>> {
    let sql = format!("select instance from workflow.instances where ({filter}) and ($1::uuid is null or tenant_id = $1) order by updated_at desc limit $2");
    let rows = sqlx::query(&sql).bind(tenant).bind(limit).fetch_all(core.db.pool()).await.db_err("workflow.instances")?;
    rows.into_iter().map(|r| parse_instance(r.get("instance"))).collect()
}

pub async fn save_report(core: &Core, r: &ImpactReport) -> Res<()> {
    sqlx::query("insert into workflow.impact_reports (id, module_id, from_version, to_version, report) values ($1, $2, $3, $4, $5)")
        .bind(r.id)
        .bind(&r.module_id)
        .bind(&r.from_version)
        .bind(&r.to_version)
        .bind(serde_json::to_value(r).unwrap_or_default())
        .execute(core.db.pool())
        .await
        .db_err("workflow.impact.save")?;
    Ok(())
}

pub async fn reports(core: &Core) -> Res<Vec<ImpactReport>> {
    let rows = sqlx::query("select report from workflow.impact_reports order by analyzed_at desc limit 50").fetch_all(core.db.pool()).await.db_err("workflow.impact.list")?;
    Ok(rows.into_iter().filter_map(|r| serde_json::from_value(r.get("report")).ok()).collect())
}

pub async fn report(core: &Core, id: Uuid) -> Res<ImpactReport> {
    let row = sqlx::query("select report from workflow.impact_reports where id = $1")
        .bind(id)
        .fetch_optional(core.db.pool())
        .await
        .db_err("workflow.impact.get")?
        .ok_or_not_found("workflow.impact", "workflow:impact.notFound")?;
    serde_json::from_value(row.get("report")).map_err(|e| Error::unexpected("workflow.impact.corrupt", "errors:unexpected").with_message(e.to_string()))
}
