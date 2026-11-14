//! # Red Hive backend core
//!
//! Everything a Red Hive service (platform or module backend) shares:
//!
//! * [`hosting`] – options from the environment, tracing, health endpoints, correlation ids, graceful shutdown.
//! * [`security`] – JWT tokens, PBKDF2 password hashing, the [`RequestContext`] extractor, internal-key calls.
//! * [`db`] – PostgreSQL pool, transactions, sqlx error mapping.
//! * [`http`] – `Result` → HTTP (problem details), the [`ServiceClient`] for service-to-service calls.
//! * [`outbox`] – transactional outbox + dispatcher to the NotificationPoint.
//! * [`audit`] – the platform audit log.
//!
//! A use case is a railway pipeline: **validate → authorize → load → decide (pure) → persist + event (one
//! transaction) → project**. Handlers only bind the request, call the use case and return its [`Res`].

pub mod audit;
pub mod db;
pub mod hosting;
pub mod http;
pub mod outbox;
pub mod security;

pub use audit::AuditLog;
pub use db::{Db, DbErrorExt};
pub use hosting::{Core, RedHiveOptions};
pub use http::{ApiResult, AppError, Auth, Created, NoContent, OptJson, ReqJson, ServiceClient, ToHttp};
pub use outbox::OutboxWriter;
pub use redhive_monads::{Error, ErrorKind, Res, ResultExt};
pub use security::{InternalCall, RedHiveUser, RequestContext};

/// The version every Red Hive service reports.
pub const PLATFORM_VERSION: &str = env!("CARGO_PKG_VERSION");
