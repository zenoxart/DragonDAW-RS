//! Session, token storage, connection settings and pinned package-signing keys.
//!
//! * The server URL and the remembered user name live in `Connection.json` (roaming config directory).
//! * The token lives in the Windows Credential Manager (other systems: a user-only file).
//! * The client pins the server's package-signing key on first contact (per server URL) and refuses
//!   packages if the key changes, until the user explicitly trusts the new key.

use crate::api::ApiClient;
use crate::context::Paths;
use redhive_contracts::api::{has_permission, LoginRequest, LoginResponse, UserProfile};
use redhive_monads::Res;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionSettings {
    pub server_url: String,
    #[serde(default)]
    pub remembered_user: Option<String>,
    #[serde(default)]
    pub remember: bool,
    /// Server URL → pinned public key (PEM).
    #[serde(default)]
    pub pinned_keys: BTreeMap<String, String>,
}

impl Default for ConnectionSettings {
    fn default() -> Self {
        ConnectionSettings { server_url: "http://localhost:8080".into(), remembered_user: None, remember: true, pinned_keys: BTreeMap::new() }
    }
}

impl ConnectionSettings {
    pub fn load(paths: &Paths) -> Self {
        paths.load_json("Connection.json").unwrap_or_default()
    }

    pub fn save(&self, paths: &Paths) {
        paths.save_json("Connection.json", self);
    }
}

/// The signed-in user.
#[derive(Debug, Clone)]
pub struct Session {
    pub profile: UserProfile,
    pub token: String,
    pub expires_at: chrono::DateTime<chrono::Utc>,
}

impl Session {
    pub fn has(&self, permission: &str) -> bool {
        has_permission(&self.profile.permissions, permission)
    }
}

/// Where the token is kept between runs.
pub struct TokenStore;

const SERVICE: &str = "RedHive";

impl TokenStore {
    #[cfg(windows)]
    pub fn save(paths: &Paths, server: &str, token: &str) {
        let _ = paths;
        if let Ok(e) = keyring::Entry::new(SERVICE, server) {
            let _ = e.set_password(token);
        }
    }

    #[cfg(windows)]
    pub fn load(paths: &Paths, server: &str) -> Option<String> {
        let _ = paths;
        keyring::Entry::new(SERVICE, server).ok().and_then(|e| e.get_password().ok())
    }

    #[cfg(windows)]
    pub fn clear(paths: &Paths, server: &str) {
        let _ = paths;
        if let Ok(e) = keyring::Entry::new(SERVICE, server) {
            let _ = e.delete_credential();
        }
    }

    #[cfg(not(windows))]
    pub fn save(paths: &Paths, server: &str, token: &str) {
        let mut all: BTreeMap<String, String> = paths.load_json("Tokens.json").unwrap_or_default();
        all.insert(server.to_string(), token.to_string());
        paths.save_json("Tokens.json", &all);
        restrict(&paths.config.join("Tokens.json"));
        let _ = SERVICE;
    }

    #[cfg(not(windows))]
    pub fn load(paths: &Paths, server: &str) -> Option<String> {
        let all: BTreeMap<String, String> = paths.load_json("Tokens.json").unwrap_or_default();
        all.get(server).cloned()
    }

    #[cfg(not(windows))]
    pub fn clear(paths: &Paths, server: &str) {
        let mut all: BTreeMap<String, String> = paths.load_json("Tokens.json").unwrap_or_default();
        all.remove(server);
        paths.save_json("Tokens.json", &all);
    }
}

#[cfg(unix)]
fn restrict(p: &std::path::Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o600));
}

#[cfg(all(not(unix), not(windows)))]
fn restrict(_p: &std::path::Path) {}

/// Signs in and returns the session.
pub async fn login(api: &ApiClient, user: &str, password: &str) -> Res<Session> {
    let r: LoginResponse = api.post("/api/identity/login", &LoginRequest { user_name: user.to_string(), password: password.to_string() }).await?;
    api.set_token(Some(r.access_token.clone()));
    Ok(Session { profile: r.user, token: r.access_token, expires_at: r.expires_at })
}

/// Resumes a stored token (valid → fresh token + profile).
pub async fn resume(api: &ApiClient, token: String) -> Res<Session> {
    api.set_token(Some(token));
    let t: redhive_contracts::api::TokenResponse = api.post("/api/identity/refresh", &serde_json::json!({})).await?;
    api.set_token(Some(t.access_token.clone()));
    let profile: UserProfile = api.get("/api/identity/me").await?;
    Ok(Session { profile, token: t.access_token, expires_at: t.expires_at })
}
