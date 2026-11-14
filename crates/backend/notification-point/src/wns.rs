//! Windows Push Notification Service (WNS) forwarding for users without a live connection.
//!
//! Configure `REDHIVE__WNS__PACKAGE_SID` and `REDHIVE__WNS__CLIENT_SECRET` (from the Partner Center app
//! registration). The client registers its channel URI via `POST /api/notifications/channels`.

use redhive_backend_core::RedHiveOptions;
use redhive_contracts::notifications::NotificationEnvelope;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

pub struct WnsClient {
    http: reqwest::Client,
    sid: String,
    secret: String,
    token: Mutex<Option<(String, Instant)>>,
}

impl WnsClient {
    pub fn from_options(o: &RedHiveOptions) -> Option<WnsClient> {
        let sid = o.var("WNS__PACKAGE_SID")?;
        let secret = o.var("WNS__CLIENT_SECRET")?;
        Some(WnsClient { http: reqwest::Client::new(), sid, secret, token: Mutex::new(None) })
    }

    async fn access_token(&self) -> Result<String, String> {
        let mut guard = self.token.lock().await;
        if let Some((t, until)) = guard.as_ref() {
            if Instant::now() < *until {
                return Ok(t.clone());
            }
        }
        let resp = self
            .http
            .post("https://login.live.com/accesstoken.srf")
            .form(&[("grant_type", "client_credentials"), ("client_id", self.sid.as_str()), ("client_secret", self.secret.as_str()), ("scope", "notify.windows.com")])
            .send()
            .await
            .map_err(|e| e.to_string())?;
        let body: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
        let token = body["access_token"].as_str().ok_or("no access token")?.to_string();
        let ttl = body["expires_in"].as_u64().unwrap_or(3600).saturating_sub(120);
        *guard = Some((token.clone(), Instant::now() + Duration::from_secs(ttl)));
        Ok(token)
    }

    pub async fn send_toast(&self, channel_uri: &str, env: &NotificationEnvelope) -> Result<(), String> {
        let token = self.access_token().await?;
        let launch = format!("redhive://navigate?target={}&parameter={}", env.navigation_target.clone().unwrap_or_default(), env.navigation_parameter.clone().unwrap_or_default());
        let xml = format!(
            "<toast launch=\"{}\"><visual><binding template=\"ToastGeneric\"><text>{}</text><text>{}</text></binding></visual></toast>",
            escape(&launch),
            escape(&env.title),
            escape(&env.body)
        );
        let resp = self
            .http
            .post(channel_uri)
            .bearer_auth(token)
            .header("X-WNS-Type", "wns/toast")
            .header("Content-Type", "text/xml")
            .body(xml)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        if resp.status().is_success() {
            Ok(())
        } else {
            Err(format!("WNS answered {}", resp.status()))
        }
    }
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}
