//! Platform API: problem details, identity, profile, settings, AI switches.

use chrono::{DateTime, Utc};
use redhive_monads::{Error, ErrorKind, FieldError};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use uuid::Uuid;

/// RFC 7807 problem details as sent by every Red Hive service. Round-trips with [`Error`].
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiError {
    #[serde(rename = "type")]
    pub type_: String,
    pub title: String,
    pub status: u16,
    pub kind: ErrorKind,
    pub code: String,
    pub text_key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub field_errors: Vec<FieldError>,
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    pub metadata: Map<String, Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub correlation_id: Option<String>,
}

impl ApiError {
    pub fn from_error(error: &Error, correlation_id: Option<String>) -> Self {
        // Unexpected errors never leak technical details to the caller.
        let detail = if error.kind == ErrorKind::Unexpected { None } else { error.message.clone() };
        ApiError {
            type_: format!("https://redhive.dev/errors/{}", error.code),
            title: error.code.clone(),
            status: error.kind.status(),
            kind: error.kind,
            code: error.code.clone(),
            text_key: error.text_key.clone(),
            detail,
            field_errors: error.field_errors.clone(),
            metadata: error.metadata.clone(),
            correlation_id,
        }
    }

    pub fn into_error(self) -> Error {
        Error {
            kind: self.kind,
            code: self.code,
            text_key: self.text_key,
            message: self.detail,
            field_errors: self.field_errors,
            metadata: self.metadata,
        }
    }
}

// ------------------------------------------------------------------ identity

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginRequest {
    pub user_name: String,
    pub password: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginResponse {
    pub access_token: String,
    pub expires_at: DateTime<Utc>,
    pub user: UserProfile,
}

/// The signed-in user as the client sees it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UserProfile {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub tenant_name: String,
    pub user_name: String,
    pub display_name: String,
    pub email: String,
    pub roles: Vec<String>,
    pub permissions: Vec<String>,
    pub approval_limit: f64,
    #[serde(default)]
    pub has_avatar: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_login_at: Option<DateTime<Utc>>,
}

impl UserProfile {
    /// Permission check with the `*` wildcard and `prefix.*` patterns.
    pub fn has(&self, permission: &str) -> bool {
        has_permission(&self.permissions, permission)
    }
}

/// Shared permission matcher: `*` grants everything, `documents.*` grants `documents.view`.
pub fn has_permission(granted: &[String], permission: &str) -> bool {
    permission.is_empty()
        || granted.iter().any(|g| {
            g == "*" || g == permission || (g.ends_with(".*") && permission.starts_with(&g[..g.len() - 1]))
        })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangePasswordRequest {
    pub current_password: String,
    pub new_password: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AvatarDto {
    pub content_type: String,
    pub data_base64: String,
}

/// Request of the workflow engine for a short-lived token of the user a workflow runs as.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DelegatedTokenRequest {
    pub user_id: Uuid,
    /// Who acts on behalf of the user (`workflow-engine`, `ai-agent`).
    pub actor: String,
    #[serde(default)]
    pub instance_id: Option<Uuid>,
    #[serde(default = "default_delegated_minutes")]
    pub minutes: i64,
}

fn default_delegated_minutes() -> i64 {
    10
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenResponse {
    pub access_token: String,
    pub expires_at: DateTime<Utc>,
}

/// Permission declared by a module manifest, registered with the identity service on publish.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionRegistration {
    pub module_id: String,
    pub permissions: Vec<String>,
}

/// A user as other services see it (`/internal/users/{id}`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserSummary {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub user_name: String,
    pub display_name: String,
    pub email: String,
    pub is_active: bool,
    pub roles: Vec<String>,
    pub permissions: Vec<String>,
    pub approval_limit: f64,
}

// ------------------------------------------------------------------ settings

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingDto {
    pub scope: String,
    pub owner: String,
    pub key: String,
    pub value: Value,
    pub updated_at: DateTime<Utc>,
    #[serde(default)]
    pub updated_by: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingUpdate {
    pub value: Value,
}

// ------------------------------------------------------------------ AI switches + audit

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiStatus {
    pub enabled: bool,
    pub provider: String,
    pub model: String,
    pub runs_today: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiSwitchRequest {
    pub enabled: bool,
    /// `global` or `tenant`.
    #[serde(default = "default_scope")]
    pub scope: String,
}

fn default_scope() -> String {
    "global".into()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiAuditEntry {
    pub id: Uuid,
    pub instance_id: Uuid,
    pub workflow_id: Uuid,
    pub step_id: String,
    pub at: DateTime<Utc>,
    pub provider: String,
    pub model: String,
    pub decision: String,
    #[serde(default)]
    pub reasoning: Option<String>,
    #[serde(default)]
    pub confidence: Option<f64>,
    #[serde(default)]
    pub proposal: Option<Value>,
    #[serde(default)]
    pub policy_code: Option<String>,
    pub tokens: i32,
}

/// Generic audit log entry (`platform.audit_log`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditEntry {
    pub id: Uuid,
    pub at: DateTime<Utc>,
    pub service: String,
    pub user_name: String,
    #[serde(default)]
    pub actor: Option<String>,
    pub action: String,
    pub entity_type: String,
    #[serde(default)]
    pub entity_id: Option<String>,
    pub outcome: String,
    pub details: Value,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permission_wildcards() {
        let p = vec!["documents.*".to_string(), "reports.view".to_string()];
        assert!(has_permission(&p, "documents.approve"));
        assert!(has_permission(&p, "reports.view"));
        assert!(!has_permission(&p, "reports.run"));
        assert!(has_permission(&["*".into()], "anything"));
        assert!(has_permission(&[], ""));
    }

    #[test]
    fn api_error_round_trip() {
        let e = Error::conflict("x.stale", "x:stale").with_metadata("rowVersion", 3);
        let api = ApiError::from_error(&e, Some("c1".into()));
        assert_eq!(api.status, 409);
        let back = api.into_error();
        assert_eq!(back.code, "x.stale");
        assert_eq!(back.metadata["rowVersion"], 3);
    }
}
