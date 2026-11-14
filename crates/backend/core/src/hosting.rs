//! Hosting defaults.

use crate::db::Db;
use crate::http::ServiceClient;
use crate::outbox;
use crate::security::TokenService;
use axum::extract::{FromRef, Request, State};
use axum::http::{HeaderValue, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde_json::json;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tower_http::trace::TraceLayer;
use uuid::Uuid;

/// Service options, read from the environment (`REDHIVE__*`, Docker-friendly).
///
/// | Variable | Meaning | Default |
/// |---|---|---|
/// | `REDHIVE__PORT` | listen port | `8080` |
/// | `DATABASE_URL` / `REDHIVE__DATABASE` | PostgreSQL URL | `postgres://redhive:redhive@localhost:5432/redhive` |
/// | `REDHIVE__JWT_KEY` | HMAC key for tokens | dev key (warning) |
/// | `REDHIVE__INTERNAL_KEY` | key for `/internal/*` calls | dev key (warning) |
/// | `REDHIVE__SERVICE__<NAME>` | base URL of another service | `http://<name>:8080` |
/// | `REDHIVE__LOG_FORMAT` | `json` or `pretty` | `pretty` |
#[derive(Debug, Clone)]
pub struct RedHiveOptions {
    pub service_name: String,
    pub port: u16,
    pub database_url: String,
    pub jwt_key: String,
    pub internal_key: String,
    pub token_minutes: i64,
    pub services: HashMap<String, String>,
    pub vars: HashMap<String, String>,
}

impl RedHiveOptions {
    pub fn from_env(service_name: &str) -> Self {
        let vars: HashMap<String, String> = std::env::vars().collect();
        let get = |k: &str| vars.get(k).cloned().filter(|v| !v.is_empty());
        let mut services = HashMap::new();
        for (k, v) in &vars {
            if let Some(name) = k.strip_prefix("REDHIVE__SERVICE__") {
                services.insert(name.to_ascii_lowercase().replace('_', "-"), v.trim_end_matches('/').to_string());
            }
        }
        let jwt_key = get("REDHIVE__JWT_KEY").unwrap_or_else(|| {
            tracing::warn!("REDHIVE__JWT_KEY is not set – using the development key");
            "redhive-development-jwt-key-change-me-0123456789".into()
        });
        let internal_key = get("REDHIVE__INTERNAL_KEY").unwrap_or_else(|| "redhive-development-internal-key".into());
        RedHiveOptions {
            service_name: service_name.to_string(),
            port: get("REDHIVE__PORT").and_then(|p| p.parse().ok()).unwrap_or(8080),
            database_url: get("DATABASE_URL").or_else(|| get("REDHIVE__DATABASE")).unwrap_or_else(|| "postgres://redhive:redhive@localhost:5432/redhive".into()),
            jwt_key,
            internal_key,
            token_minutes: get("REDHIVE__TOKEN_MINUTES").and_then(|p| p.parse().ok()).unwrap_or(480),
            services,
            vars,
        }
    }

    /// Base URL of another service (`module-registry` → `http://module-registry:8080`).
    pub fn service_url(&self, name: &str) -> String {
        self.services.get(name).cloned().unwrap_or_else(|| format!("http://{name}:8080"))
    }

    /// Any other setting (`REDHIVE__AI__PROVIDER` → `var("AI__PROVIDER")`).
    pub fn var(&self, key: &str) -> Option<String> {
        self.vars.get(&format!("REDHIVE__{key}")).cloned().filter(|v| !v.is_empty())
    }
}

/// Shared infrastructure of a service; part of every service state (`Core: FromRef<S>`).
#[derive(Clone)]
pub struct Core {
    pub options: Arc<RedHiveOptions>,
    pub db: Db,
    pub tokens: TokenService,
    pub services: ServiceClient,
}

impl Core {
    /// Reads options, initializes tracing and connects to the database (with retries).
    pub async fn init(service_name: &str) -> anyhow_like::Result<Core> {
        init_tracing();
        let options = Arc::new(RedHiveOptions::from_env(service_name));
        let db = Db::connect(&options.database_url).await?;
        let tokens = TokenService::new(&options.jwt_key, options.token_minutes);
        let services = ServiceClient::new(options.clone());
        Ok(Core { options, db, tokens, services })
    }

    pub fn name(&self) -> &str {
        &self.options.service_name
    }
}

/// Minimal boxed error for startup code.
pub mod anyhow_like {
    pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
}

pub fn init_tracing() {
    use tracing_subscriber::{fmt, EnvFilter};
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info,sqlx=warn,hyper=warn,reqwest=warn"));
    let json = std::env::var("REDHIVE__LOG_FORMAT").map(|v| v == "json").unwrap_or(false);
    let _ = if json { fmt().with_env_filter(filter).json().try_init() } else { fmt().with_env_filter(filter).compact().try_init() };
}

pub const CORRELATION_HEADER: &str = "x-correlation-id";

/// Correlation id of the current request (taken from the caller or created).
#[derive(Debug, Clone)]
pub struct CorrelationId(pub String);

async fn correlation(mut request: Request, next: Next) -> Response {
    let id = request
        .headers()
        .get(CORRELATION_HEADER)
        .and_then(|v| v.to_str().ok())
        .filter(|v| !v.is_empty() && v.len() <= 100)
        .map(str::to_string)
        .unwrap_or_else(|| Uuid::new_v4().simple().to_string());
    request.extensions_mut().insert(CorrelationId(id.clone()));
    let span = tracing::info_span!("request", correlation = %id);
    let _enter = span.enter();
    drop(_enter);
    let mut response = next.run(request).await;
    if let Ok(v) = HeaderValue::from_str(&id) {
        response.headers_mut().insert(CORRELATION_HEADER, v);
    }
    response
}

async fn live(State(core): State<Core>) -> impl IntoResponse {
    Json(json!({ "status": "live", "service": core.name(), "version": crate::PLATFORM_VERSION }))
}

async fn ready(State(core): State<Core>) -> Response {
    match sqlx::query("select 1").execute(core.db.pool()).await {
        Ok(_) => Json(json!({ "status": "ready", "service": core.name() })).into_response(),
        Err(e) => (StatusCode::SERVICE_UNAVAILABLE, Json(json!({ "status": "unavailable", "error": e.to_string() }))).into_response(),
    }
}

/// Adds health endpoints, correlation ids, request tracing and a body limit, then serves until Ctrl+C /
/// SIGTERM. Starts the outbox dispatcher of this service.
pub async fn serve<S>(core: Core, router: Router<S>, state: S) -> anyhow_like::Result<()>
where
    S: Clone + Send + Sync + 'static,
    Core: FromRef<S>,
{
    outbox::spawn_dispatcher(core.clone());
    let app = router
        .route("/health/live", get(live))
        .route("/health/ready", get(ready))
        .layer(axum::middleware::from_fn(correlation))
        .layer(TraceLayer::new_for_http())
        .layer(tower_http::limit::RequestBodyLimitLayer::new(32 * 1024 * 1024))
        .with_state(state);
    let addr = SocketAddr::from(([0, 0, 0, 0], core.options.port));
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!(service = core.name(), %addr, "Red Hive service listening");
    axum::serve(listener, app).with_graceful_shutdown(shutdown_signal()).await?;
    tracing::info!(service = core.name(), "stopped");
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let terminate = async {
        if let Ok(mut s) = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            s.recv().await;
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! { _ = ctrl_c => {}, _ = terminate => {} }
}

/// Runs `f` every `period` until the process ends (background jobs).
pub fn every<F, Fut>(name: &'static str, period: Duration, mut f: F)
where
    F: FnMut() -> Fut + Send + 'static,
    Fut: std::future::Future<Output = ()> + Send,
{
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(period);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tick.tick().await;
            tracing::trace!(job = name, "tick");
            f().await;
        }
    });
}
