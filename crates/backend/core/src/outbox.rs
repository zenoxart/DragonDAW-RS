//! Transactional outbox.
//!
//! A use case writes its data **and** its event in the same transaction ([`OutboxWriter::add`]). The
//! dispatcher of each service forwards pending rows to the NotificationPoint and marks them dispatched,
//! so an event is never lost and never sent for a rolled-back change.

use crate::db::{DbErrorExt, Tx};
use crate::hosting::Core;
use crate::security::RedHiveUser;
use redhive_contracts::notifications::{NotificationAudience, NotificationEnvelope};
use redhive_monads::Res;
use serde_json::Value;
use sqlx::Row;
use std::time::Duration;
use uuid::Uuid;

#[derive(Clone)]
pub struct OutboxWriter {
    source: String,
}

impl OutboxWriter {
    pub fn new(source: impl Into<String>) -> Self {
        OutboxWriter { source: source.into() }
    }

    /// Creates an envelope for the user's tenant, raised by this service.
    pub fn create(&self, event_type: &str, user: &RedHiveUser, audience: NotificationAudience, payload: Value) -> NotificationEnvelope {
        let mut e = NotificationEnvelope::new(event_type, &self.source, user.tenant_id, audience, payload);
        e.actor = Some(user.audit_name());
        e
    }

    /// Writes the envelope into `platform.outbox` inside the caller's transaction.
    pub async fn add(&self, tx: &mut Tx<'static>, envelope: &NotificationEnvelope) -> Res<()> {
        sqlx::query("insert into platform.outbox (id, source, type, envelope) values ($1, $2, $3, $4)")
            .bind(envelope.id)
            .bind(&self.source)
            .bind(&envelope.event_type)
            .bind(serde_json::to_value(envelope).unwrap_or_default())
            .execute(&mut **tx)
            .await
            .db_err("outbox.add")?;
        sqlx::query("select pg_notify('redhive_outbox', $1)").bind(&self.source).execute(&mut **tx).await.db_err("outbox.notify")?;
        Ok(())
    }

    /// Writes outside a transaction (for services without own data changes).
    pub async fn add_now(&self, core: &Core, envelope: &NotificationEnvelope) -> Res<()> {
        let mut tx = core.db.begin().await?;
        self.add(&mut tx, envelope).await?;
        tx.commit().await.db_err("outbox.commit")
    }
}

/// Starts the background dispatcher for this service's outbox rows. Wakes on `pg_notify` and every 2 s.
pub fn spawn_dispatcher(core: Core) {
    tokio::spawn(async move {
        let source = core.options.service_name.clone();
        let mut listener = match sqlx::postgres::PgListener::connect_with(core.db.pool()).await {
            Ok(mut l) => {
                let _ = l.listen("redhive_outbox").await;
                Some(l)
            }
            Err(e) => {
                tracing::warn!(error = %e, "outbox listener unavailable – polling only");
                None
            }
        };
        loop {
            match dispatch_pending(&core, &source).await {
                Ok(0) => {}
                Ok(n) => tracing::debug!(count = n, "outbox dispatched"),
                Err(e) => tracing::warn!(error = %e, "outbox dispatch failed – retrying"),
            }
            match listener.as_mut() {
                Some(l) => {
                    let _ = tokio::time::timeout(Duration::from_secs(2), l.recv()).await;
                }
                None => tokio::time::sleep(Duration::from_secs(2)).await,
            }
        }
    });
}

async fn dispatch_pending(core: &Core, source: &str) -> Res<usize> {
    let rows = sqlx::query("select id, envelope from platform.outbox where source = $1 and dispatched_at is null order by created_at limit 100")
        .bind(source)
        .fetch_all(core.db.pool())
        .await
        .db_err("outbox.pending")?;
    let mut sent = 0;
    for row in rows {
        let id: Uuid = row.get("id");
        let envelope: Value = row.get("envelope");
        // The NotificationPoint is idempotent on the envelope id.
        let result: Res<Value> = core.services.post_internal("notification-point", "/internal/events", &envelope).await;
        match result {
            Ok(_) => {
                sqlx::query("update platform.outbox set dispatched_at = now() where id = $1").bind(id).execute(core.db.pool()).await.db_err("outbox.mark")?;
                sent += 1;
            }
            Err(e) => return Err(e),
        }
    }
    Ok(sent)
}
