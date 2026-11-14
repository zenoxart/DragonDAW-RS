//! Red Hive API gateway – the only public entry point.
//!
//! * Static routes to the platform services; module routes come from the registry (`backend.route` →
//!   `backend.service`) and are refreshed every 30 seconds. Longest prefix wins.
//! * Streams responses (server-sent events pass straight through).
//! * Propagates the correlation id, applies a per-client rate limit, never routes `/internal/*`.

use axum::body::Body;
use axum::extract::{ConnectInfo, Request, State};
use axum::http::{HeaderMap, HeaderName, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{any, get};
use axum::{Json, Router};
use futures::TryStreamExt;
use redhive_backend_core::hosting::{init_tracing, RedHiveOptions, CORRELATION_HEADER};
use redhive_backend_core::http::problem;
use redhive_backend_core::{Error, ServiceClient};
use redhive_contracts::modules::RouteInfo;
use serde_json::json;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};
use uuid::Uuid;

#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct Route {
    prefix: String,
    target: String,
    source: String,
}

#[derive(Clone)]
struct Gateway {
    routes: Arc<RwLock<Vec<Route>>>,
    http: reqwest::Client,
    services: ServiceClient,
    limiter: Arc<Mutex<HashMap<String, (Instant, u32)>>>,
    limit_per_minute: u32,
    tokens: redhive_backend_core::security::TokenService,
    options: Arc<RedHiveOptions>,
}

/// A manifest names its backend by its container URL (`http://document-service:8080`). Outside Docker,
/// `REDHIVE__SERVICE__DOCUMENT_SERVICE` overrides it.
fn remap(options: &RedHiveOptions, target: &str) -> String {
    let host = target.split("://").nth(1).unwrap_or(target).split(['/', ':']).next().unwrap_or("");
    options.services.get(host).cloned().unwrap_or_else(|| target.trim_end_matches('/').to_string())
}

/// Platform routes (prefix → service). Override with `REDHIVE__ROUTES__<PREFIX>`-free env: the service
/// base URLs come from `REDHIVE__SERVICE__<NAME>`.
fn static_routes(o: &RedHiveOptions) -> Vec<Route> {
    [
        ("/api/identity", "identity-service"),
        ("/api/config", "configuration-service"),
        ("/api/modules", "module-registry"),
        ("/api/packages", "module-delivery"),
        ("/api/updates", "update-orchestrator"),
        ("/api/notifications", "notification-point"),
        ("/api/workflows", "workflow-service"),
        ("/api/ai", "ai-agent"),
    ]
    .into_iter()
    .map(|(prefix, svc)| Route { prefix: prefix.into(), target: o.service_url(svc), source: "platform".into() })
    .collect()
}

impl Gateway {
    fn resolve(&self, path: &str) -> Option<Route> {
        let routes = self.routes.read().ok()?;
        routes
            .iter()
            .filter(|r| path == r.prefix || path.starts_with(&format!("{}/", r.prefix)) || path.starts_with(&format!("{}?", r.prefix)))
            .max_by_key(|r| r.prefix.len())
            .cloned()
    }

    fn allow(&self, client: &str) -> bool {
        let Ok(mut map) = self.limiter.lock() else { return true };
        let now = Instant::now();
        let entry = map.entry(client.to_string()).or_insert((now, 0));
        if now.duration_since(entry.0) > Duration::from_secs(60) {
            *entry = (now, 0);
        }
        entry.1 += 1;
        if map.len() > 10_000 {
            map.retain(|_, (t, _)| now.duration_since(*t) < Duration::from_secs(60));
        }
        map.get(client).is_none_or(|e| e.1 <= self.limit_per_minute)
    }
}

const HOP_BY_HOP: &[&str] = &["connection", "keep-alive", "proxy-authenticate", "proxy-authorization", "te", "trailer", "transfer-encoding", "upgrade", "host", "content-length"];

async fn proxy(State(g): State<Gateway>, ConnectInfo(addr): ConnectInfo<SocketAddr>, req: Request) -> Response {
    let path = req.uri().path().to_string();
    if path.starts_with("/internal") {
        return problem(&Error::not_found("gateway.route", "errors:notFound"));
    }
    let client = req.headers().get("x-forwarded-for").and_then(|v| v.to_str().ok()).map(|v| v.split(',').next().unwrap_or("").trim().to_string()).unwrap_or_else(|| addr.ip().to_string());
    if !g.allow(&client) {
        let mut r = problem(&Error::unavailable("gateway.rateLimited", "errors:rateLimited"));
        *r.status_mut() = StatusCode::TOO_MANY_REQUESTS;
        r.headers_mut().insert("retry-after", HeaderValue::from_static("60"));
        return r;
    }
    let Some(route) = g.resolve(&path) else {
        return problem(&Error::not_found("gateway.route", "errors:notFound").with_metadata("path", path));
    };
    let correlation = req.headers().get(CORRELATION_HEADER).and_then(|v| v.to_str().ok()).map(str::to_string).unwrap_or_else(|| Uuid::new_v4().simple().to_string());
    let query = req.uri().query().map(|q| format!("?{q}")).unwrap_or_default();
    let url = format!("{}{}{}", route.target, path, query);
    let method = req.method().clone();
    let mut headers = HeaderMap::new();
    for (k, v) in req.headers() {
        if !HOP_BY_HOP.contains(&k.as_str()) {
            headers.insert(k.clone(), v.clone());
        }
    }
    headers.insert(HeaderName::from_static(CORRELATION_HEADER), HeaderValue::from_str(&correlation).unwrap_or(HeaderValue::from_static("invalid")));
    if let Ok(v) = HeaderValue::from_str(&client) {
        headers.insert("x-forwarded-for", v);
    }
    let body = match axum::body::to_bytes(req.into_body(), 32 * 1024 * 1024).await {
        Ok(b) => b,
        Err(_) => return problem(&Error::validation("gateway.body.tooLarge", "errors:tooLarge")),
    };
    let upstream = g.http.request(method, &url).headers(headers).body(body).send().await;
    let upstream = match upstream {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(target = %route.target, error = %e, "backend unavailable");
            return problem(&Error::unavailable("gateway.backend.unavailable", "errors:unavailable").with_metadata("service", route.target));
        }
    };
    let status = upstream.status();
    let mut builder = Response::builder().status(status.as_u16());
    for (k, v) in upstream.headers() {
        if !HOP_BY_HOP.contains(&k.as_str()) {
            builder = builder.header(k, v);
        }
    }
    builder = builder.header(CORRELATION_HEADER, correlation);
    let stream = upstream.bytes_stream().map_err(std::io::Error::other);
    builder.body(Body::from_stream(stream)).unwrap_or_else(|_| StatusCode::BAD_GATEWAY.into_response())
}

async fn root() -> Json<serde_json::Value> {
    Json(json!({ "service": "Red Hive API gateway", "version": env!("CARGO_PKG_VERSION"), "health": "/health/ready" }))
}

/// The current route table (signed-in users with `modules.view`).
async fn route_table(State(g): State<Gateway>, headers: HeaderMap) -> Response {
    let token = headers.get("authorization").and_then(|v| v.to_str().ok()).and_then(|v| v.strip_prefix("Bearer ")).unwrap_or("");
    match g.tokens.validate(token).and_then(|u| u.require("modules.view").map(|_| u)) {
        Ok(_) => Json(g.routes.read().map(|r| r.clone()).unwrap_or_default()).into_response(),
        Err(e) => problem(&e),
    }
}

async fn health() -> Json<serde_json::Value> {
    Json(json!({ "status": "ready", "service": "api-gateway" }))
}

/// Merges the platform routes with the module routes from the registry. Returns false when the registry
/// could not be reached (the caller retries sooner).
async fn refresh(g: &Gateway, base: &[Route]) -> bool {
    match g.services.get_internal::<Vec<RouteInfo>>("module-registry", "/internal/routes").await {
        Ok(dynamic) => {
            let mut routes = base.to_vec();
            for r in dynamic {
                if routes.iter().any(|x| x.prefix == r.prefix && x.source == "platform") {
                    tracing::warn!(prefix = %r.prefix, module = %r.module_id, "module route collides with a platform route – ignored");
                    continue;
                }
                routes.push(Route { prefix: r.prefix, target: remap(&g.options, &r.target), source: r.module_id });
            }
            if let Ok(mut w) = g.routes.write() {
                if w.len() != routes.len() {
                    tracing::info!(count = routes.len(), "route table updated");
                }
                *w = routes;
            }
            true
        }
        Err(e) => {
            tracing::warn!(error = %e, "module routes could not be loaded from the registry");
            false
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    init_tracing();
    let options = Arc::new(RedHiveOptions::from_env("api-gateway"));
    let base = static_routes(&options);
    let gateway = Gateway {
        routes: Arc::new(RwLock::new(base.clone())),
        http: reqwest::Client::builder().connect_timeout(Duration::from_secs(5)).pool_idle_timeout(Duration::from_secs(60)).build()?,
        services: ServiceClient::new(options.clone()),
        limiter: Arc::new(Mutex::new(HashMap::new())),
        limit_per_minute: options.var("RATE_LIMIT_PER_MINUTE").and_then(|v| v.parse().ok()).unwrap_or(1200),
        tokens: redhive_backend_core::security::TokenService::new(&options.jwt_key, options.token_minutes),
        options: options.clone(),
    };
    {
        let g = gateway.clone();
        tokio::spawn(async move {
            loop {
                let ok = refresh(&g, &base).await;
                tokio::time::sleep(Duration::from_secs(if ok { 30 } else { 3 })).await;
            }
        });
    }
    let app = Router::new()
        .route("/", get(root))
        .route("/health/live", get(health))
        .route("/health/ready", get(health))
        .route("/api/gateway/routes", get(route_table))
        .route("/api/{*rest}", any(proxy))
        .route("/internal/{*rest}", any(proxy))
        .with_state(gateway);
    let addr = SocketAddr::from(([0, 0, 0, 0], options.port));
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!(%addr, "Red Hive API gateway listening");
    axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>()).await?;
    Ok(())
}
