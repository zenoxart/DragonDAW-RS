//! Live notifications: a server-sent-events connection with catch-up after reconnect, the in-app
//! notification centre and (on Windows) system toasts.

use crate::api::ApiClient;
use futures::StreamExt;
use redhive_contracts::notifications::NotificationEnvelope;
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionState {
    Offline,
    Connecting,
    Connected,
}

/// The SSE connection. Events arrive through [`NotificationHub::drain`].
pub struct NotificationHub {
    rx: Receiver<NotificationEnvelope>,
    tx: Sender<NotificationEnvelope>,
    pub last_sequence: Arc<AtomicI64>,
    state: Arc<Mutex<ConnectionState>>,
    generation: Arc<AtomicU64>,
}

impl Default for NotificationHub {
    fn default() -> Self {
        let (tx, rx) = channel();
        NotificationHub { rx, tx, last_sequence: Arc::new(AtomicI64::new(-1)), state: Arc::new(Mutex::new(ConnectionState::Offline)), generation: Arc::new(AtomicU64::new(0)) }
    }
}

impl NotificationHub {
    pub fn state(&self) -> ConnectionState {
        self.state.lock().map(|s| *s).unwrap_or(ConnectionState::Offline)
    }

    /// Connects (and keeps reconnecting with back-off) until [`NotificationHub::stop`].
    pub fn start(&self, api: ApiClient) {
        let generation = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        let gen = self.generation.clone();
        let tx = self.tx.clone();
        let last = self.last_sequence.clone();
        let state = self.state.clone();
        crate::runtime::spawn(async move {
            let mut backoff = Duration::from_secs(1);
            while gen.load(Ordering::SeqCst) == generation {
                set(&state, ConnectionState::Connecting);
                let after = last.load(Ordering::SeqCst);
                let url = if after >= 0 { api.url(&format!("/api/notifications/stream?after={after}")) } else { api.url("/api/notifications/stream") };
                let mut req = api.http().get(url).header("accept", "text/event-stream").timeout(Duration::from_secs(60 * 60 * 24));
                if let Some(t) = api.token() {
                    req = req.bearer_auth(t);
                }
                match req.send().await {
                    Ok(resp) if resp.status().is_success() => {
                        set(&state, ConnectionState::Connected);
                        backoff = Duration::from_secs(1);
                        let mut stream = resp.bytes_stream();
                        let mut buf = String::new();
                        let mut event = String::new();
                        let mut data = String::new();
                        'read: while let Some(chunk) = tokio::time::timeout(Duration::from_secs(45), stream.next()).await.ok().flatten() {
                            let Ok(chunk) = chunk else { break };
                            buf.push_str(&String::from_utf8_lossy(&chunk));
                            while let Some(pos) = buf.find('\n') {
                                let line = buf[..pos].trim_end_matches('\r').to_string();
                                buf.drain(..=pos);
                                if line.is_empty() {
                                    if event == "notification" {
                                        if let Ok(env) = serde_json::from_str::<NotificationEnvelope>(&data) {
                                            last.fetch_max(env.sequence, Ordering::SeqCst);
                                            if tx.send(env).is_err() {
                                                break 'read;
                                            }
                                            crate::runtime::request_repaint();
                                        }
                                    } else if event == "hello" {
                                        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&data) {
                                            if let Some(seq) = v["sequence"].as_i64() {
                                                last.fetch_max(seq, Ordering::SeqCst);
                                            }
                                        }
                                    } else if event == "resync" {
                                        break 'read;
                                    }
                                    event.clear();
                                    data.clear();
                                } else if let Some(v) = line.strip_prefix("event:") {
                                    event = v.trim().to_string();
                                } else if let Some(v) = line.strip_prefix("data:") {
                                    if !data.is_empty() {
                                        data.push('\n');
                                    }
                                    data.push_str(v.trim_start());
                                }
                            }
                            if gen.load(Ordering::SeqCst) != generation {
                                break;
                            }
                        }
                    }
                    Ok(resp) if resp.status().as_u16() == 401 => {
                        set(&state, ConnectionState::Offline);
                        tracing::warn!("notification stream: unauthorized");
                    }
                    Ok(resp) => tracing::warn!(status = %resp.status(), "notification stream refused"),
                    Err(e) => tracing::debug!(error = %e, "notification stream unavailable"),
                }
                if gen.load(Ordering::SeqCst) != generation {
                    break;
                }
                set(&state, ConnectionState::Connecting);
                crate::runtime::request_repaint();
                tokio::time::sleep(backoff).await;
                backoff = (backoff * 2).min(Duration::from_secs(30));
            }
            set(&state, ConnectionState::Offline);
        });
    }

    pub fn stop(&self) {
        self.generation.fetch_add(1, Ordering::SeqCst);
        self.last_sequence.store(-1, Ordering::SeqCst);
        set(&self.state, ConnectionState::Offline);
    }

    /// Events received since the last frame.
    pub fn drain(&self) -> Vec<NotificationEnvelope> {
        self.rx.try_iter().collect()
    }
}

fn set(state: &Arc<Mutex<ConnectionState>>, s: ConnectionState) {
    if let Ok(mut g) = state.lock() {
        *g = s;
    }
}

#[derive(Debug, Clone)]
pub struct CenterItem {
    pub envelope: NotificationEnvelope,
    pub read: bool,
}

/// The notification centre (visible notifications, newest first).
#[derive(Default)]
pub struct NotificationCenter {
    pub items: Vec<CenterItem>,
}

impl NotificationCenter {
    pub fn add(&mut self, env: NotificationEnvelope, read: bool) {
        if !env.is_visible() || self.items.iter().any(|i| i.envelope.id == env.id) {
            return;
        }
        self.items.insert(0, CenterItem { envelope: env, read });
        self.items.truncate(300);
    }

    pub fn unread(&self) -> usize {
        self.items.iter().filter(|i| !i.read).count()
    }

    pub fn mark_all_read(&mut self) {
        for i in &mut self.items {
            i.read = true;
        }
    }

    pub fn clear(&mut self) {
        self.items.clear();
    }
}

/// A system toast (Windows notification centre). No-op elsewhere.
pub fn system_toast(title: &str, body: &str) {
    #[cfg(windows)]
    {
        let (t, b) = (title.to_string(), body.to_string());
        std::thread::spawn(move || {
            let _ = notify_rust::Notification::new().summary(&t).body(&b).appname("Red Hive").show();
        });
    }
    #[cfg(not(windows))]
    {
        let _ = (title, body);
    }
}
