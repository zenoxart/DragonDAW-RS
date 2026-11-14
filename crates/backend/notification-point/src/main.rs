//! Red Hive NotificationPoint.
//!
//! * `POST /internal/events` – services' outbox dispatchers deliver events here (idempotent on the id).
//! * Events get a sequence number and are stored (`notify.events`).
//! * `GET /api/notifications/stream` – server-sent events per user (tenant + audience filter), with
//!   catch-up after reconnect (`?after=<sequence>` or `Last-Event-ID`).
//! * Subscribers (the workflow service) receive every event in sequence order with a stored cursor.
//! * Optional WNS push for users without a live connection.

mod wns;

use axum::extract::{Query, State};
use axum::http::HeaderMap;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::routing::{get, post};
use axum::{Json, Router};
use futures::stream::Stream;
use redhive_backend_core::db::DbErrorExt;
use redhive_backend_core::hosting::{serve, Core};
use redhive_backend_core::{ApiResult, InternalCall, NoContent, RequestContext, ToHttp, ReqJson};
use redhive_contracts::notifications::{NotificationAudience, NotificationEnvelope};
use redhive_monads::{Error, Res};
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::Row;
use std::collections::HashMap;
use std::convert::Infallible;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::{broadcast, Notify};
use uuid::Uuid;

const SERVICE: &str = "notification-point";

#[derive(Clone)]
struct AppState {
    core: Core,
    live: broadcast::Sender<NotificationEnvelope>,
    wake: Arc<Notify>,
    connections: Arc<Mutex<HashMap<Uuid, usize>>>,
    wns: Option<Arc<wns::WnsClient>>,
}

impl axum::extract::FromRef<AppState> for Core {
    fn from_ref(s: &AppState) -> Core {
        s.core.clone()
    }
}

/// Stores an event (idempotent), assigns its sequence and publishes it.
async fn receive(State(s): State<AppState>, _call: InternalCall, Json(mut env): Json<NotificationEnvelope>) -> ApiResult<Value> {
    async {
        if env.event_type.is_empty() {
            return Err(Error::validation("notify.type.missing", "errors:required").for_field("type"));
        }
        let row = sqlx::query(
            "insert into notify.events (id, type, source, tenant_id, audience_kind, audience_value, severity, envelope)
             values ($1, $2, $3, $4, $5, $6, $7, $8) on conflict (id) do nothing returning sequence",
        )
        .bind(env.id)
        .bind(&env.event_type)
        .bind(&env.source)
        .bind(env.tenant_id)
        .bind(env.audience.kind_name())
        .bind(env.audience.value_text())
        .bind(env.severity.as_str())
        .bind(serde_json::to_value(&env).unwrap_or_default())
        .fetch_optional(s.core.db.pool())
        .await
        .db_err("notify.store")?;
        let Some(row) = row else {
            // duplicate delivery – already stored
            return Ok(json!({ "duplicate": true }));
        };
        env.sequence = row.get("sequence");
        sqlx::query("update notify.events set envelope = jsonb_set(envelope, '{sequence}', to_jsonb(sequence)) where sequence = $1")
            .bind(env.sequence)
            .execute(s.core.db.pool())
            .await
            .db_err("notify.sequence")?;
        tracing::debug!(sequence = env.sequence, event = %env.event_type, "event stored");
        let _ = s.live.send(env.clone());
        s.wake.notify_waiters();
        if let Some(w) = &s.wns {
            push_offline(&s, w.clone(), env.clone());
        }
        Ok(json!({ "sequence": env.sequence }))
    }
    .await
    .http()
}

fn push_offline(s: &AppState, wns: Arc<wns::WnsClient>, env: NotificationEnvelope) {
    if !env.is_visible() {
        return;
    }
    let NotificationAudience::User(user) = env.audience else { return };
    let online = s.connections.lock().map(|c| c.get(&user).copied().unwrap_or(0) > 0).unwrap_or(true);
    if online {
        return;
    }
    let core = s.core.clone();
    tokio::spawn(async move {
        let channels: Vec<String> = sqlx::query_scalar("select channel_uri from notify.channels where user_id = $1").bind(user).fetch_all(core.db.pool()).await.unwrap_or_default();
        for c in channels {
            if let Err(e) = wns.send_toast(&c, &env).await {
                tracing::warn!(error = %e, "WNS push failed");
            }
        }
    });
}

#[derive(Deserialize, Default)]
struct StreamQuery {
    #[serde(default)]
    after: Option<i64>,
}

fn visible_to(env: &NotificationEnvelope, ctx: &RequestContext) -> bool {
    env.tenant_id == ctx.user.tenant_id && env.audience.includes(ctx.user.id, &ctx.user.permissions)
}

async fn catch_up(core: &Core, ctx: &RequestContext, after: i64, limit: i64) -> Res<Vec<NotificationEnvelope>> {
    let rows = sqlx::query("select envelope from notify.events where tenant_id = $1 and sequence > $2 and audience_kind <> 'system' order by sequence limit $3")
        .bind(ctx.user.tenant_id)
        .bind(after)
        .bind(limit)
        .fetch_all(core.db.pool())
        .await
        .db_err("notify.catchup")?;
    Ok(rows
        .into_iter()
        .filter_map(|r| serde_json::from_value::<NotificationEnvelope>(r.get("envelope")).ok())
        .filter(|e| visible_to(e, ctx))
        .collect())
}

fn sse_event(env: &NotificationEnvelope) -> Event {
    Event::default().id(env.sequence.to_string()).event("notification").json_data(env).unwrap_or_else(|_| Event::default().comment("serialization failed"))
}

/// Live stream for the signed-in user. Sends missed events first, then live ones.
async fn stream(State(s): State<AppState>, ctx: RequestContext, headers: HeaderMap, Query(q): Query<StreamQuery>) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, redhive_backend_core::AppError> {
    let after = q.after.or_else(|| headers.get("last-event-id").and_then(|v| v.to_str().ok()).and_then(|v| v.parse().ok()));
    // Subscribe before the catch-up query so nothing falls between the two.
    let mut rx = s.live.subscribe();
    let start = match after {
        Some(a) => a,
        None => sqlx::query_scalar::<_, Option<i64>>("select max(sequence) from notify.events").fetch_one(s.core.db.pool()).await.db_err("notify.max")?.unwrap_or(0),
    };
    let missed = catch_up(&s.core, &ctx, start, 500).await?;
    let user = ctx.user.id;
    if let Ok(mut c) = s.connections.lock() {
        *c.entry(user).or_default() += 1;
    }
    let connections = s.connections.clone();
    let guard = ConnectionGuard { user, connections };
    let stream = async_stream_like(move |yielder| async move {
        let _guard = guard;
        let mut last = start;
        yielder.send(Ok(Event::default().event("hello").data(json!({ "sequence": start }).to_string()))).await;
        for e in missed {
            last = last.max(e.sequence);
            yielder.send(Ok(sse_event(&e))).await;
        }
        loop {
            match rx.recv().await {
                Ok(e) if e.sequence > last && visible_to(&e, &ctx) => {
                    last = e.sequence;
                    if !yielder.send(Ok(sse_event(&e))).await {
                        break;
                    }
                }
                Ok(_) => {}
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    tracing::warn!(skipped = n, "stream lagged – client will catch up on reconnect");
                    yielder.send(Ok(Event::default().event("resync").data(last.to_string()))).await;
                    break;
                }
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    });
    Ok(Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(15)).text("keep-alive")))
}

struct ConnectionGuard {
    user: Uuid,
    connections: Arc<Mutex<HashMap<Uuid, usize>>>,
}

impl Drop for ConnectionGuard {
    fn drop(&mut self) {
        if let Ok(mut c) = self.connections.lock() {
            if let Some(n) = c.get_mut(&self.user) {
                *n = n.saturating_sub(1);
            }
        }
    }
}

/// Tiny generator: runs `f` with a channel sender and exposes the receiver as a stream.
fn async_stream_like<T, F, Fut>(f: F) -> impl Stream<Item = T>
where
    T: Send + 'static,
    F: FnOnce(Yielder<T>) -> Fut,
    Fut: std::future::Future<Output = ()> + Send + 'static,
{
    let (tx, rx) = tokio::sync::mpsc::channel(64);
    tokio::spawn(f(Yielder(tx)));
    futures::stream::unfold(rx, |mut rx| async move { rx.recv().await.map(|v| (v, rx)) })
}

struct Yielder<T>(tokio::sync::mpsc::Sender<T>);

impl<T> Yielder<T> {
    /// Returns false when the client is gone.
    async fn send(&self, v: T) -> bool {
        self.0.send(v).await.is_ok()
    }
}

#[derive(Deserialize)]
struct ListQuery {
    #[serde(default)]
    before: Option<i64>,
    #[serde(default = "fifty")]
    take: i64,
}

fn fifty() -> i64 {
    50
}

/// Recent visible notifications of the user (notification centre after sign-in).
async fn list(State(s): State<AppState>, ctx: RequestContext, Query(q): Query<ListQuery>) -> ApiResult<Vec<NotificationEnvelope>> {
    async {
        let rows = sqlx::query(
            "select envelope from notify.events where tenant_id = $1 and sequence < $2 and audience_kind <> 'system' and coalesce(envelope->>'title', '') <> ''
             order by sequence desc limit 500",
        )
        .bind(ctx.user.tenant_id)
        .bind(q.before.unwrap_or(i64::MAX))
        .fetch_all(s.core.db.pool())
        .await
        .db_err("notify.list")?;
        Ok(rows
            .into_iter()
            .filter_map(|r| serde_json::from_value::<NotificationEnvelope>(r.get("envelope")).ok())
            .filter(|e| visible_to(e, &ctx))
            .take(q.take.clamp(1, 200) as usize)
            .collect())
    }
    .await
    .http()
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ChannelRequest {
    channel_uri: String,
}

/// Registers a WNS channel of the user's device.
async fn register_channel(State(s): State<AppState>, ctx: RequestContext, ReqJson(req): ReqJson<ChannelRequest>) -> Result<NoContent, redhive_backend_core::AppError> {
    if !req.channel_uri.starts_with("https://") || !req.channel_uri.contains(".notify.windows.com") {
        return Err(Error::validation("notify.channel.invalid", "notify:channel.invalid").for_field("channelUri").into());
    }
    sqlx::query("insert into notify.channels (user_id, channel_uri) values ($1, $2) on conflict (user_id, channel_uri) do update set updated_at = now()")
        .bind(ctx.user.id)
        .bind(&req.channel_uri)
        .execute(s.core.db.pool())
        .await
        .db_err("notify.channel")?;
    Ok(NoContent)
}

#[derive(Deserialize)]
struct InternalEventsQuery {
    after: i64,
    #[serde(default = "fifty")]
    take: i64,
}

/// Raw event feed for services (catch-up of subscribers, diagnostics).
async fn internal_events(State(s): State<AppState>, _call: InternalCall, Query(q): Query<InternalEventsQuery>) -> ApiResult<Vec<NotificationEnvelope>> {
    async {
        let rows = sqlx::query("select envelope from notify.events where sequence > $1 order by sequence limit $2")
            .bind(q.after)
            .bind(q.take.clamp(1, 500))
            .fetch_all(s.core.db.pool())
            .await
            .db_err("notify.feed")?;
        Ok(rows.into_iter().filter_map(|r| serde_json::from_value(r.get("envelope")).ok()).collect())
    }
    .await
    .http()
}

/// Forwards every event in order to one subscriber, keeping a cursor in the database.
fn spawn_subscriber(core: Core, name: String, path: String, wake: Arc<Notify>) {
    tokio::spawn(async move {
        sqlx::query("insert into notify.subscriber_cursors (subscriber) values ($1) on conflict do nothing").bind(&name).execute(core.db.pool()).await.ok();
        let mut backoff = Duration::from_millis(500);
        loop {
            let delivered = deliver_batch(&core, &name, &path).await;
            match delivered {
                Ok(0) => {
                    backoff = Duration::from_millis(500);
                    let _ = tokio::time::timeout(Duration::from_secs(2), wake.notified()).await;
                }
                Ok(_) => backoff = Duration::from_millis(500),
                Err(e) => {
                    tracing::warn!(subscriber = %name, error = %e, "delivery failed – retrying");
                    tokio::time::sleep(backoff).await;
                    backoff = (backoff * 2).min(Duration::from_secs(30));
                }
            }
        }
    });
}

async fn deliver_batch(core: &Core, name: &str, path: &str) -> Res<usize> {
    let cursor: i64 = sqlx::query_scalar("select last_sequence from notify.subscriber_cursors where subscriber = $1").bind(name).fetch_one(core.db.pool()).await.db_err("notify.cursor")?;
    let rows = sqlx::query("select sequence, envelope from notify.events where sequence > $1 order by sequence limit 50").bind(cursor).fetch_all(core.db.pool()).await.db_err("notify.batch")?;
    let mut n = 0;
    for r in rows {
        let seq: i64 = r.get("sequence");
        let env: Value = r.get("envelope");
        let _: Value = core.services.post_internal(name, path, &env).await?;
        sqlx::query("update notify.subscriber_cursors set last_sequence = $2, updated_at = now() where subscriber = $1").bind(name).bind(seq).execute(core.db.pool()).await.db_err("notify.cursor.update")?;
        n += 1;
    }
    Ok(n)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let core = Core::init(SERVICE).await?;
    let (live, _) = broadcast::channel(1024);
    let wake = Arc::new(Notify::new());
    let wns = wns::WnsClient::from_options(&core.options).map(Arc::new);
    if wns.is_some() {
        tracing::info!("WNS push enabled");
    }
    let subscribers = core.options.var("SUBSCRIBERS").unwrap_or_else(|| "workflow-service=/internal/events".into());
    for entry in subscribers.split(',').map(str::trim).filter(|e| !e.is_empty()) {
        let (name, path) = entry.split_once('=').unwrap_or((entry, "/internal/events"));
        spawn_subscriber(core.clone(), name.to_string(), path.to_string(), wake.clone());
    }
    let state = AppState { core: core.clone(), live, wake, connections: Arc::new(Mutex::new(HashMap::new())), wns };
    let router = Router::new()
        .route("/api/notifications", get(list))
        .route("/api/notifications/stream", get(stream))
        .route("/api/notifications/channels", post(register_channel))
        .route("/internal/events", post(receive).get(internal_events));
    serve(core, router, state).await
}
