//! PostgreSQL access (sqlx, runtime-checked queries – no database needed at compile time).

use redhive_monads::{Error, Res};
use sqlx::postgres::{PgPoolOptions, PgPool};
use sqlx::{Postgres, Transaction};
use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

pub type Tx<'a> = Transaction<'a, Postgres>;

#[derive(Clone)]
pub struct Db {
    pool: PgPool,
}

impl Db {
    /// Connects with retries (the database container may still be starting).
    pub async fn connect(url: &str) -> Result<Db, sqlx::Error> {
        let mut attempt = 0;
        loop {
            attempt += 1;
            match PgPoolOptions::new().max_connections(10).acquire_timeout(Duration::from_secs(10)).connect(url).await {
                Ok(pool) => return Ok(Db { pool }),
                Err(e) if attempt < 30 => {
                    tracing::warn!(attempt, error = %e, "database not reachable yet – retrying");
                    tokio::time::sleep(Duration::from_secs(2)).await;
                }
                Err(e) => return Err(e),
            }
        }
    }

    pub fn from_pool(pool: PgPool) -> Db {
        Db { pool }
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    pub async fn begin(&self) -> Res<Tx<'static>> {
        self.pool.begin().await.db_err("db.begin")
    }

    /// Runs `f` in a transaction: commit on `Ok`, rollback on `Err`.
    ///
    /// ```ignore
    /// db.in_transaction(|tx| Box::pin(async move { repo.update(tx, ...).await?; outbox.add(tx, ...).await })).await
    /// ```
    pub async fn in_transaction<T, F>(&self, f: F) -> Res<T>
    where
        F: for<'t> FnOnce(&'t mut Tx<'static>) -> Pin<Box<dyn Future<Output = Res<T>> + Send + 't>>,
    {
        let mut tx = self.begin().await?;
        match f(&mut tx).await {
            Ok(v) => {
                tx.commit().await.db_err("db.commit")?;
                Ok(v)
            }
            Err(e) => {
                let _ = tx.rollback().await;
                Err(e)
            }
        }
    }
}

/// Maps sqlx errors to [`Error`]s: unique violations → Conflict, connection problems → Unavailable,
/// missing rows → NotFound, everything else → Unexpected.
pub trait DbErrorExt<T> {
    fn db_err(self, code: &str) -> Res<T>;
}

impl<T> DbErrorExt<T> for Result<T, sqlx::Error> {
    fn db_err(self, code: &str) -> Res<T> {
        self.map_err(|e| map_db_error(code, e))
    }
}

pub fn map_db_error(code: &str, e: sqlx::Error) -> Error {
    match &e {
        sqlx::Error::RowNotFound => Error::not_found(code, "errors:notFound"),
        sqlx::Error::PoolTimedOut | sqlx::Error::Io(_) | sqlx::Error::PoolClosed => Error::unavailable("db.unavailable", "errors:unavailable").with_message(e.to_string()),
        sqlx::Error::Database(d) => match d.code().as_deref() {
            Some("23505") => Error::conflict(format!("{code}.duplicate"), "errors:duplicate").with_message(d.message().to_string()),
            Some("23503") => Error::conflict(format!("{code}.reference"), "errors:reference").with_message(d.message().to_string()),
            Some("23514") | Some("22P02") => Error::validation(format!("{code}.invalid"), "errors:invalidValue").with_message(d.message().to_string()),
            Some("40001") | Some("40P01") => Error::unavailable("db.serialization", "errors:unavailable").with_message(d.message().to_string()),
            _ => Error::unexpected(code, "errors:unexpected").with_message(e.to_string()),
        },
        _ => Error::unexpected(code, "errors:unexpected").with_message(e.to_string()),
    }
}

/// Optimistic concurrency: an update that touched no row means the row version was stale (or the row is gone).
pub fn ensure_updated(rows: u64, code: &str) -> Res<()> {
    if rows == 0 {
        Err(Error::conflict(format!("{code}.stale"), "errors:staleVersion"))
    } else {
        Ok(())
    }
}
