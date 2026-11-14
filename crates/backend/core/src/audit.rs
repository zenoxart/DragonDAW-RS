//! Platform audit log: who (user, workflow engine, AI agent) did what, with the outcome.

use crate::db::{DbErrorExt, Tx};
use crate::security::RedHiveUser;
use redhive_monads::Res;
use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;

#[derive(Clone)]
pub struct AuditLog {
    service: String,
}

impl AuditLog {
    pub fn new(service: impl Into<String>) -> Self {
        AuditLog { service: service.into() }
    }

    /// Writes an entry inside the caller's transaction.
    pub async fn write(&self, tx: &mut Tx<'static>, user: &RedHiveUser, action: &str, entity_type: &str, entity_id: &str, details: Value) -> Res<()> {
        sqlx::query(
            "insert into platform.audit_log (id, service, user_id, user_name, actor, action, entity_type, entity_id, outcome, details)
             values ($1, $2, $3, $4, $5, $6, $7, $8, 'success', $9)",
        )
        .bind(Uuid::new_v4())
        .bind(&self.service)
        .bind(user.id)
        .bind(&user.display_name)
        .bind(&user.actor)
        .bind(action)
        .bind(entity_type)
        .bind(entity_id)
        .bind(details)
        .execute(&mut **tx)
        .await
        .db_err("audit.write")?;
        Ok(())
    }

    /// Writes a failed attempt (outside any transaction, so it survives the rollback).
    pub async fn failure(&self, pool: &PgPool, user: &RedHiveUser, action: &str, entity_type: &str, entity_id: &str, error: &redhive_monads::Error) {
        let r = sqlx::query(
            "insert into platform.audit_log (id, service, user_id, user_name, actor, action, entity_type, entity_id, outcome, details)
             values ($1, $2, $3, $4, $5, $6, $7, $8, 'failure', $9)",
        )
        .bind(Uuid::new_v4())
        .bind(&self.service)
        .bind(user.id)
        .bind(&user.display_name)
        .bind(&user.actor)
        .bind(action)
        .bind(entity_type)
        .bind(entity_id)
        .bind(serde_json::json!({ "code": error.code, "kind": error.kind }))
        .execute(pool)
        .await;
        if let Err(e) = r {
            tracing::warn!(error = %e, "audit failure entry could not be written");
        }
    }
}
