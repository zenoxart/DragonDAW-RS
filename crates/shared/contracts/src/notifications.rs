//! Notifications and domain events.
//!
//! A use case writes a [`NotificationEnvelope`] into the transactional outbox together with its data.
//! The outbox dispatcher forwards it to the NotificationPoint, which assigns a sequence number, stores it,
//! streams it to clients (server-sent events, with catch-up after reconnect) and hands it to subscribers
//! such as the workflow service.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

/// Who receives a notification.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "camelCase")]
pub enum NotificationAudience {
    /// Every user of the tenant.
    Tenant,
    /// One user.
    User(Uuid),
    /// Users with a permission.
    WithPermission(String),
    /// Only services (not streamed to clients).
    System,
}

impl NotificationAudience {
    pub fn kind_name(&self) -> &'static str {
        match self {
            NotificationAudience::Tenant => "tenant",
            NotificationAudience::User(_) => "user",
            NotificationAudience::WithPermission(_) => "permission",
            NotificationAudience::System => "system",
        }
    }

    pub fn value_text(&self) -> Option<String> {
        match self {
            NotificationAudience::User(u) => Some(u.to_string()),
            NotificationAudience::WithPermission(p) => Some(p.clone()),
            _ => None,
        }
    }

    /// Whether a user with the given id and permissions receives it.
    pub fn includes(&self, user_id: Uuid, permissions: &[String]) -> bool {
        match self {
            NotificationAudience::Tenant => true,
            NotificationAudience::User(u) => *u == user_id,
            NotificationAudience::WithPermission(p) => crate::api::has_permission(permissions, p),
            NotificationAudience::System => false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum Severity {
    #[default]
    Info,
    Success,
    Warning,
    Error,
}

impl Severity {
    pub fn as_str(self) -> &'static str {
        match self {
            Severity::Info => "info",
            Severity::Success => "success",
            Severity::Warning => "warning",
            Severity::Error => "error",
        }
    }
}

/// One event / notification.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotificationEnvelope {
    pub id: Uuid,
    /// Assigned by the NotificationPoint; 0 before that.
    #[serde(default)]
    pub sequence: i64,
    /// Event type, `<module>.<entity>.<what>`, e.g. `documents.invoice.submitted`.
    #[serde(rename = "type")]
    pub event_type: String,
    #[serde(default = "one")]
    pub payload_version: u32,
    /// Service that raised it.
    pub source: String,
    pub tenant_id: Uuid,
    pub audience: NotificationAudience,
    #[serde(default)]
    pub severity: Severity,
    /// Text key (or literal) shown as the notification title; empty = no visible notification.
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub body: String,
    /// Values for placeholders in title/body.
    #[serde(default)]
    pub text_args: serde_json::Map<String, Value>,
    /// Navigation target opened when the notification is clicked (`documents.invoices`).
    #[serde(default)]
    pub navigation_target: Option<String>,
    #[serde(default)]
    pub navigation_parameter: Option<String>,
    pub payload: Value,
    #[serde(default)]
    pub correlation_id: Option<String>,
    #[serde(default)]
    pub actor: Option<String>,
    pub created_at: DateTime<Utc>,
}

fn one() -> u32 {
    1
}

impl NotificationEnvelope {
    pub fn new(event_type: impl Into<String>, source: impl Into<String>, tenant_id: Uuid, audience: NotificationAudience, payload: Value) -> Self {
        NotificationEnvelope {
            id: Uuid::new_v4(),
            sequence: 0,
            event_type: event_type.into(),
            payload_version: 1,
            source: source.into(),
            tenant_id,
            audience,
            severity: Severity::Info,
            title: String::new(),
            body: String::new(),
            text_args: Default::default(),
            navigation_target: None,
            navigation_parameter: None,
            payload,
            correlation_id: None,
            actor: None,
            created_at: Utc::now(),
        }
    }

    pub fn titled(mut self, title: impl Into<String>, severity: Severity) -> Self {
        self.title = title.into();
        self.severity = severity;
        self
    }

    pub fn navigate(mut self, target: impl Into<String>, parameter: Option<String>) -> Self {
        self.navigation_target = Some(target.into());
        self.navigation_parameter = parameter;
        self
    }

    pub fn with_args(mut self, args: serde_json::Map<String, Value>) -> Self {
        self.text_args = args;
        self
    }

    /// Whether the client should show it as a visible notification (vs. a silent data event).
    pub fn is_visible(&self) -> bool {
        !self.title.is_empty()
    }

    /// Matches `documents.*`, `documents.invoice.submitted` or `*`.
    pub fn matches(&self, pattern: &str) -> bool {
        type_matches(pattern, &self.event_type)
    }
}

/// Event type pattern match: exact, `*`, or `prefix.*`.
pub fn type_matches(pattern: &str, event_type: &str) -> bool {
    pattern == "*" || pattern == event_type || (pattern.ends_with(".*") && event_type.starts_with(&pattern[..pattern.len() - 1]))
}

/// Well-known platform event types.
pub mod event_types {
    pub const MODULE_VERSION_AVAILABLE: &str = "modules.version.available";
    pub const MODULE_PUBLISHED: &str = "modules.published";
    pub const ROLLOUT_CHANGED: &str = "updates.rollout.changed";
    pub const WORKFLOW_TASK_ASSIGNED: &str = "workflow.task.assigned";
    pub const WORKFLOW_APPROVAL_REQUESTED: &str = "workflow.approval.requested";
    pub const WORKFLOW_INSTANCE_CHANGED: &str = "workflow.instance.changed";
    pub const WORKFLOW_COMPLETED: &str = "workflow.instance.completed";
    pub const WORKFLOW_FAILED: &str = "workflow.instance.failed";
    pub const WORKFLOW_IMPACT_ANALYZED: &str = "workflow.impact.analyzed";
    pub const COMMAND_EXECUTED: &str = "shell.command.executed";
    pub const SETTINGS_CHANGED: &str = "config.setting.changed";
    pub const AI_SWITCH_CHANGED: &str = "ai.switch.changed";
}

/// Report of a client command execution, posted to the workflow service for command triggers.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandExecutedEvent {
    pub command_id: String,
    pub parameter: Value,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audience_and_patterns() {
        let u = Uuid::new_v4();
        assert!(NotificationAudience::User(u).includes(u, &[]));
        assert!(NotificationAudience::WithPermission("documents.approve".into()).includes(Uuid::nil(), &["documents.*".into()]));
        assert!(!NotificationAudience::System.includes(u, &["*".into()]));
        assert!(type_matches("documents.*", "documents.invoice.submitted"));
        assert!(!type_matches("documents.*", "reports.ready"));
    }

    #[test]
    fn audience_serialization() {
        let json = serde_json::to_value(NotificationAudience::WithPermission("x.y".into())).unwrap();
        assert_eq!(json["kind"], "withPermission");
        let back: NotificationAudience = serde_json::from_value(serde_json::json!({"kind": "tenant"})).unwrap();
        assert_eq!(back, NotificationAudience::Tenant);
    }
}
