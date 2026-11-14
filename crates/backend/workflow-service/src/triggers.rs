//! Triggers: events (from the NotificationPoint), commands (reported by clients), schedules, conditions;
//! plus the background jobs for retries, timeouts and recovery after a restart.

use crate::engine::Engine;
use crate::store;
use chrono::{DateTime, Datelike, Duration, TimeZone, Utc};
use redhive_backend_core::db::DbErrorExt;
use redhive_backend_core::Auth;
use redhive_contracts::notifications::NotificationEnvelope;
use redhive_contracts::workflows::analysis::{parse_schedule, Schedule};
use redhive_contracts::workflows::expr::{self, Scope};
use redhive_contracts::workflows::*;
use redhive_monads::Res;
use serde_json::{json, Value};
use sqlx::Row;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use uuid::Uuid;

/// Starts the workflow for one trigger hit, honouring filter, mapping, de-duplication and the concurrency limit.
pub async fn fire(engine: &Engine, tenant: Uuid, def: &WorkflowDefinition, trigger: &WorkflowTrigger, payload: Value, event_id: Option<Uuid>, fallback_run_as: Option<Uuid>) -> Res<Option<WorkflowInstance>> {
    let sc = Scope::new().with("payload", payload.clone()).with("trigger", payload.clone());
    if let Some(f) = trigger.filter.as_deref().filter(|f| !f.trim().is_empty()) {
        match expr::condition(f, &sc) {
            Ok(true) => {}
            Ok(false) => return Ok(None),
            Err(e) => {
                tracing::warn!(workflow = %def.name, trigger = %trigger.id, error = %e, "trigger filter failed");
                return Ok(None);
            }
        }
    }
    let vars = match expr::eval_mapping(&trigger.payload_mapping, &sc) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(workflow = %def.name, trigger = %trigger.id, error = %e, "payload mapping failed");
            return Ok(None);
        }
    };
    if let Some(key_expr) = trigger.deduplication_key.as_deref().filter(|k| !k.trim().is_empty()) {
        let key = expr::eval_str(key_expr, &sc).map(|v| match v {
            Value::String(s) => s,
            other => other.to_string(),
        });
        if let Ok(key) = key {
            let inserted = sqlx::query("insert into workflow.trigger_runs (workflow_id, trigger_id, dedup_key, instance_id) values ($1, $2, $3, $4) on conflict do nothing")
                .bind(def.id)
                .bind(&trigger.id)
                .bind(&key)
                .bind(Uuid::nil())
                .execute(engine.core.db.pool())
                .await
                .db_err("workflow.dedup")?
                .rows_affected();
            if inserted == 0 {
                tracing::debug!(workflow = %def.name, key, "duplicate trigger run skipped");
                return Ok(None);
            }
        }
    }
    let running = store::running_count(&engine.core, def.id).await?;
    if running >= trigger.max_concurrent_runs as i64 {
        tracing::warn!(workflow = %def.name, running, "concurrency limit reached – trigger skipped");
        return Ok(None);
    }
    let Some(run_as) = trigger.run_as.or(fallback_run_as).or(def.owner_id) else {
        tracing::warn!(workflow = %def.name, "trigger has no RunAs user and the workflow has no owner");
        return Ok(None);
    };
    let info = TriggerInfo { trigger_id: trigger.id.clone(), kind: format!("{:?}", trigger.kind).to_lowercase(), event_id, payload };
    let inst = engine.start(def, tenant, run_as, trigger.executor, vars, Some(info), None).await?;
    Ok(Some(inst))
}

/// Every event from the NotificationPoint: start event-triggered workflows and wake waiting instances.
pub async fn on_event(engine: &Engine, env: NotificationEnvelope) -> Res<usize> {
    if env.event_type.starts_with("modules.") {
        engine.catalog.invalidate().await;
    }
    let mut started = 0;
    // Events raised by the engine itself never trigger workflows (no feedback loops).
    if env.source != crate::engine::SOURCE {
        for (tenant, def) in store::latest_active_everywhere(&engine.core).await? {
            if tenant != env.tenant_id {
                continue;
            }
            for t in def.triggers.iter().filter(|t| t.kind == TriggerKind::Event && t.enabled && !t.suspended && t.event_type.as_deref() == Some(env.event_type.as_str())) {
                match fire(engine, tenant, &def, t, env.payload.clone(), Some(env.id), None).await {
                    Ok(Some(i)) => {
                        started += 1;
                        tracing::info!(workflow = %def.name, instance = %i.id, event = %env.event_type, "workflow triggered by event");
                    }
                    Ok(None) => {}
                    Err(e) => tracing::warn!(workflow = %def.name, error = %e, "event trigger failed"),
                }
            }
        }
    }
    let waiting = store::instances_where(&engine.core, &format!("state = 'waitingForEvent' and waiting_event = '{}'", env.event_type.replace('\'', "''")), Some(env.tenant_id), 500).await?;
    for i in waiting {
        if let Err(e) = engine.deliver_event(i.id, &env.payload, false).await {
            tracing::warn!(instance = %i.id, error = %e, "event could not be delivered to the instance");
        }
    }
    Ok(started)
}

/// A client executed a workflow-capable command.
pub async fn on_command(engine: &Engine, user: &redhive_backend_core::RedHiveUser, command_id: &str, parameter: Value) -> Res<Vec<Uuid>> {
    let mut started = Vec::new();
    for (tenant, def) in store::latest_active_everywhere(&engine.core).await? {
        if tenant != user.tenant_id {
            continue;
        }
        for t in def.triggers.iter().filter(|t| t.kind == TriggerKind::Command && t.enabled && !t.suspended && t.command_id.as_deref() == Some(command_id)) {
            let payload = json!({ "commandId": command_id, "parameter": parameter, "userId": user.id, "userName": user.user_name });
            if let Some(i) = fire(engine, tenant, &def, t, payload, None, Some(user.id)).await? {
                started.push(i.id);
            }
        }
    }
    Ok(started)
}

/// When a schedule is due (UTC).
pub fn schedule_due(spec: Schedule, last: Option<DateTime<Utc>>, now: DateTime<Utc>) -> bool {
    match spec {
        Schedule::Every(minutes) => last.is_none_or(|l| now - l >= Duration::minutes(minutes as i64)),
        Schedule::Daily(h, m) => {
            let Some(today) = Utc.with_ymd_and_hms(now.year(), now.month(), now.day(), h, m, 0).single() else { return false };
            now >= today && last.is_none_or(|l| l < today)
        }
    }
}

async fn last_run(engine: &Engine, workflow: Uuid, trigger: &str) -> Option<DateTime<Utc>> {
    sqlx::query("select last_run_at from workflow.schedule_state where workflow_id = $1 and trigger_id = $2")
        .bind(workflow)
        .bind(trigger)
        .fetch_optional(engine.core.db.pool())
        .await
        .ok()
        .flatten()
        .map(|r| r.get("last_run_at"))
}

async fn set_last_run(engine: &Engine, workflow: Uuid, trigger: &str, at: DateTime<Utc>) {
    let _ = sqlx::query("insert into workflow.schedule_state (workflow_id, trigger_id, last_run_at) values ($1, $2, $3) on conflict (workflow_id, trigger_id) do update set last_run_at = excluded.last_run_at")
        .bind(workflow)
        .bind(trigger)
        .bind(at)
        .execute(engine.core.db.pool())
        .await;
}

/// Schedule and condition triggers (every 30 s).
pub async fn tick_triggers(engine: &Engine, conditions: &Arc<Mutex<HashMap<(Uuid, String), bool>>>) {
    let now = Utc::now();
    let Ok(defs) = store::latest_active_everywhere(&engine.core).await else { return };
    for (tenant, def) in defs {
        for t in def.triggers.iter().filter(|t| t.enabled && !t.suspended) {
            match t.kind {
                TriggerKind::Schedule => {
                    let Some(spec) = t.schedule.as_deref().and_then(parse_schedule) else { continue };
                    let last = last_run(engine, def.id, &t.id).await;
                    // First sight of a daily schedule: don't fire retroactively for earlier today.
                    if last.is_none() && matches!(spec, Schedule::Daily(..)) {
                        set_last_run(engine, def.id, &t.id, now).await;
                        continue;
                    }
                    if schedule_due(spec, last, now) {
                        set_last_run(engine, def.id, &t.id, now).await;
                        match fire(engine, tenant, &def, t, json!({ "scheduledAt": now }), None, None).await {
                            Ok(Some(i)) => tracing::info!(workflow = %def.name, instance = %i.id, "scheduled workflow started"),
                            Ok(None) => {}
                            Err(e) => tracing::warn!(workflow = %def.name, error = %e, "scheduled start failed"),
                        }
                    }
                }
                TriggerKind::Condition => {
                    let Some(url) = t.condition_url.clone() else { continue };
                    let last = last_run(engine, def.id, &t.id).await;
                    if last.is_some_and(|l| now - l < Duration::seconds(t.interval_seconds.max(30) as i64)) {
                        continue;
                    }
                    set_last_run(engine, def.id, &t.id, now).await;
                    let Some(run_as) = t.run_as.or(def.owner_id) else { continue };
                    let token = match engine
                        .core
                        .services
                        .post_internal::<redhive_contracts::api::TokenResponse, _>(
                            "identity-service",
                            "/internal/tokens/delegated",
                            &redhive_contracts::api::DelegatedTokenRequest { user_id: run_as, actor: "workflow-engine".into(), instance_id: None, minutes: 2 },
                        )
                        .await
                    {
                        Ok(t) => t.access_token,
                        Err(e) => {
                            tracing::warn!(error = %e, "condition check: no token");
                            continue;
                        }
                    };
                    let value: Res<Value> = engine.core.services.get("api-gateway", &url, &Auth::Bearer(token)).await;
                    let now_true = matches!(value, Ok(Value::Bool(true)));
                    let key = (def.id, t.id.clone());
                    let was = conditions.lock().map(|m| m.get(&key).copied().unwrap_or(false)).unwrap_or(false);
                    if let Ok(mut m) = conditions.lock() {
                        m.insert(key, now_true);
                    }
                    // Fire on the rising edge only.
                    if now_true && !was {
                        if let Err(e) = fire(engine, tenant, &def, t, json!({ "condition": url, "checkedAt": now }), None, None).await {
                            tracing::warn!(workflow = %def.name, error = %e, "condition trigger failed");
                        }
                    }
                }
                _ => {}
            }
        }
    }
}

/// Retries due, event timeouts, and instances that were running when the service stopped (every 5 s).
pub async fn tick_instances(engine: &Engine) {
    let retry = store::instances_where(&engine.core, "state = 'retrying' and retry_at <= now()", None, 100).await.unwrap_or_default();
    for mut i in retry {
        i.state = InstanceState::Running;
        if store::save_instance(&engine.core, &i).await.is_ok() {
            let _ = engine.advance(i.id).await;
        }
    }
    let timed_out = store::instances_where(&engine.core, "state = 'waitingForEvent' and wait_until <= now()", None, 100).await.unwrap_or_default();
    for i in timed_out {
        let _ = engine.deliver_event(i.id, &Value::Null, true).await;
    }
    let stale = store::instances_where(&engine.core, "state = 'running' and updated_at < now() - interval '2 minutes'", None, 100).await.unwrap_or_default();
    for i in stale {
        tracing::info!(instance = %i.id, "resuming instance after restart");
        let _ = engine.advance(i.id).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schedules_are_due() {
        let now = Utc.with_ymd_and_hms(2026, 11, 14, 8, 0, 0).unwrap();
        assert!(schedule_due(Schedule::Every(30), None, now));
        assert!(!schedule_due(Schedule::Every(30), Some(now - Duration::minutes(10)), now));
        assert!(schedule_due(Schedule::Every(30), Some(now - Duration::minutes(31)), now));
        assert!(schedule_due(Schedule::Daily(7, 30), Some(now - Duration::days(1)), now));
        assert!(!schedule_due(Schedule::Daily(7, 30), Some(now - Duration::minutes(5)), now));
        assert!(!schedule_due(Schedule::Daily(9, 0), Some(now - Duration::days(1)), now));
    }
}
