//! Tokens, passwords, the signed-in user and internal calls.

use crate::hosting::{Core, CorrelationId};
use crate::http::AppError;
use axum::extract::{FromRef, FromRequestParts};
use axum::http::request::Parts;
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use chrono::{DateTime, Duration, Utc};
use jsonwebtoken::{decode, encode, Algorithm, DecodingKey, EncodingKey, Header, Validation};
use rand::RngCore;
use redhive_contracts::api::has_permission;
use redhive_monads::{Error, Res};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use std::sync::Arc;
use uuid::Uuid;

pub const ISSUER: &str = "redhive-identity";
pub const INTERNAL_KEY_HEADER: &str = "x-redhive-internal-key";

/// JWT claims of a Red Hive token.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    pub sub: Uuid,
    pub tid: Uuid,
    pub name: String,
    pub dname: String,
    #[serde(default)]
    pub roles: Vec<String>,
    #[serde(default)]
    pub perms: Vec<String>,
    #[serde(default)]
    pub limit: f64,
    /// Set for delegated tokens: who acts on behalf of the user (`workflow-engine`, `ai-agent`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub act: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inst: Option<Uuid>,
    pub iss: String,
    pub iat: i64,
    pub exp: i64,
}

/// The user a request runs as.
#[derive(Debug, Clone, PartialEq)]
pub struct RedHiveUser {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub user_name: String,
    pub display_name: String,
    pub roles: Vec<String>,
    pub permissions: Vec<String>,
    pub approval_limit: f64,
    /// `Some("workflow-engine")` when a service acts on behalf of the user.
    pub actor: Option<String>,
    pub instance_id: Option<Uuid>,
}

impl RedHiveUser {
    pub fn has(&self, permission: &str) -> bool {
        has_permission(&self.permissions, permission)
    }

    /// Authorize step of a use case.
    pub fn require(&self, permission: &str) -> Res<()> {
        if self.has(permission) {
            Ok(())
        } else {
            Err(Error::forbidden("auth.permission.missing", "errors:forbidden").with_metadata("permission", permission))
        }
    }

    /// Name for audit entries: `Maria Huber (via ai-agent)`.
    pub fn audit_name(&self) -> String {
        match &self.actor {
            Some(a) => format!("{} (via {a})", self.display_name),
            None => self.display_name.clone(),
        }
    }

    pub fn from_claims(c: Claims) -> Self {
        RedHiveUser {
            id: c.sub,
            tenant_id: c.tid,
            user_name: c.name,
            display_name: c.dname,
            roles: c.roles,
            permissions: c.perms,
            approval_limit: c.limit,
            actor: c.act,
            instance_id: c.inst,
        }
    }
}

#[derive(Clone)]
pub struct TokenService {
    encoding: Arc<EncodingKey>,
    decoding: Arc<DecodingKey>,
    minutes: i64,
}

impl TokenService {
    pub fn new(key: &str, minutes: i64) -> Self {
        TokenService { encoding: Arc::new(EncodingKey::from_secret(key.as_bytes())), decoding: Arc::new(DecodingKey::from_secret(key.as_bytes())), minutes }
    }

    pub fn issue(&self, user: &RedHiveUser, minutes: Option<i64>) -> Res<(String, DateTime<Utc>)> {
        let now = Utc::now();
        let exp = now + Duration::minutes(minutes.unwrap_or(self.minutes));
        let claims = Claims {
            sub: user.id,
            tid: user.tenant_id,
            name: user.user_name.clone(),
            dname: user.display_name.clone(),
            roles: user.roles.clone(),
            perms: user.permissions.clone(),
            limit: user.approval_limit,
            act: user.actor.clone(),
            inst: user.instance_id,
            iss: ISSUER.into(),
            iat: now.timestamp(),
            exp: exp.timestamp(),
        };
        let token = encode(&Header::new(Algorithm::HS256), &claims, &self.encoding).map_err(|e| Error::unexpected("auth.token.issue", "errors:unexpected").with_message(e.to_string()))?;
        Ok((token, exp))
    }

    pub fn validate(&self, token: &str) -> Res<RedHiveUser> {
        let mut v = Validation::new(Algorithm::HS256);
        v.set_issuer(&[ISSUER]);
        v.leeway = 30;
        decode::<Claims>(token, &self.decoding, &v)
            .map(|d| RedHiveUser::from_claims(d.claims))
            .map_err(|e| match e.kind() {
                jsonwebtoken::errors::ErrorKind::ExpiredSignature => Error::unauthorized("auth.token.expired", "errors:sessionExpired"),
                _ => Error::unauthorized("auth.token.invalid", "errors:unauthorized").with_message(e.to_string()),
            })
    }
}

/// PBKDF2-HMAC-SHA256 password hashes: `pbkdf2$<iterations>$<salt b64>$<hash b64>`.
pub mod passwords {
    use super::*;

    pub const ITERATIONS: u32 = 100_000;

    pub fn hash(password: &str) -> String {
        let mut salt = [0u8; 16];
        rand::thread_rng().fill_bytes(&mut salt);
        hash_with(password, &salt, ITERATIONS)
    }

    pub fn hash_with(password: &str, salt: &[u8], iterations: u32) -> String {
        let mut out = [0u8; 32];
        pbkdf2::pbkdf2_hmac::<Sha256>(password.as_bytes(), salt, iterations, &mut out);
        format!("pbkdf2${iterations}${}${}", B64.encode(salt), B64.encode(out))
    }

    /// Constant-time verification.
    pub fn verify(password: &str, stored: &str) -> bool {
        let parts: Vec<&str> = stored.split('$').collect();
        if parts.len() != 4 || parts[0] != "pbkdf2" {
            return false;
        }
        let (Ok(iter), Ok(salt), Ok(expected)) = (parts[1].parse::<u32>(), B64.decode(parts[2]), B64.decode(parts[3])) else { return false };
        let mut out = vec![0u8; expected.len()];
        pbkdf2::pbkdf2_hmac::<Sha256>(password.as_bytes(), &salt, iter, &mut out);
        out.iter().zip(expected.iter()).fold(0u8, |acc, (a, b)| acc | (a ^ b)) == 0 && out.len() == expected.len()
    }

    /// Password policy: at least 10 characters with upper case, lower case, digit and symbol.
    pub fn check_policy(password: &str) -> Res<()> {
        let ok = password.chars().count() >= 10
            && password.chars().any(|c| c.is_uppercase())
            && password.chars().any(|c| c.is_lowercase())
            && password.chars().any(|c| c.is_ascii_digit())
            && password.chars().any(|c| !c.is_alphanumeric());
        if ok {
            Ok(())
        } else {
            Err(Error::validation("identity.password.weak", "identity:password.weak").for_field("newPassword"))
        }
    }
}

/// Everything a use case needs to know about the request: the user, the raw token (for forwarding)
/// and the correlation id. Extracting it **requires** a valid bearer token.
#[derive(Debug, Clone)]
pub struct RequestContext {
    pub user: RedHiveUser,
    pub token: String,
    pub correlation_id: String,
}

impl<S> FromRequestParts<S> for RequestContext
where
    S: Send + Sync,
    Core: FromRef<S>,
{
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let core = Core::from_ref(state);
        let correlation_id = parts.extensions.get::<CorrelationId>().map(|c| c.0.clone()).unwrap_or_default();
        let header = parts.headers.get(axum::http::header::AUTHORIZATION).and_then(|v| v.to_str().ok()).unwrap_or("");
        let token = header
            .strip_prefix("Bearer ")
            .map(str::to_string)
            // EventSource clients cannot set headers: allow ?access_token= for SSE endpoints.
            .or_else(|| parts.uri.query().and_then(|q| q.split('&').find_map(|kv| kv.strip_prefix("access_token=").map(str::to_string))))
            .ok_or_else(|| AppError(Error::unauthorized("auth.token.missing", "errors:unauthorized")))?;
        let user = core.tokens.validate(&token).map_err(AppError)?;
        Ok(RequestContext { user, token, correlation_id })
    }
}

/// Marker extractor for `/internal/*` endpoints: requires the internal key header.
#[derive(Debug, Clone)]
pub struct InternalCall {
    pub correlation_id: String,
}

impl<S> FromRequestParts<S> for InternalCall
where
    S: Send + Sync,
    Core: FromRef<S>,
{
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let core = Core::from_ref(state);
        let key = parts.headers.get(INTERNAL_KEY_HEADER).and_then(|v| v.to_str().ok()).unwrap_or("");
        if key.is_empty() || key != core.options.internal_key {
            return Err(AppError(Error::unauthorized("auth.internal.key", "errors:unauthorized")));
        }
        Ok(InternalCall { correlation_id: parts.extensions.get::<CorrelationId>().map(|c| c.0.clone()).unwrap_or_default() })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user() -> RedHiveUser {
        RedHiveUser {
            id: Uuid::new_v4(),
            tenant_id: Uuid::nil(),
            user_name: "maria".into(),
            display_name: "Maria".into(),
            roles: vec!["Approver".into()],
            permissions: vec!["documents.*".into()],
            approval_limit: 10_000.0,
            actor: Some("ai-agent".into()),
            instance_id: None,
        }
    }

    #[test]
    fn tokens_round_trip() {
        let t = TokenService::new("k".repeat(40).as_str(), 5);
        let (token, _) = t.issue(&user(), None).unwrap();
        let back = t.validate(&token).unwrap();
        assert_eq!(back.user_name, "maria");
        assert!(back.has("documents.approve"));
        assert_eq!(back.audit_name(), "Maria (via ai-agent)");
        assert!(TokenService::new("other-key-xxxxxxxxxxxxxxxxxxxxxxx", 5).validate(&token).is_err());
    }

    #[test]
    fn passwords_verify() {
        let h = passwords::hash_with("RedHive!2026", b"0123456789abcdef", 1000);
        assert!(passwords::verify("RedHive!2026", &h));
        assert!(!passwords::verify("redhive!2026", &h));
        assert!(passwords::check_policy("short").is_err());
        assert!(passwords::check_policy("Long-enough-1").is_ok());
    }
}
