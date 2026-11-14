//! Running workflow instances.

use super::definition::{StepKind, WorkflowExecutor};
use super::input::{ChoiceOption, InputSchema};
use chrono::{DateTime, Utc};
use redhive_monads::Error;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum InstanceState {
    /// The engine is (or will be) executing the current step.
    Running,
    /// Waiting for a person (or the AI) to fill a form.
    WaitingForInput,
    /// Waiting for an approval / a confirmation of an AI proposal.
    WaitingForApproval,
    WaitingForEvent,
    /// Retrying a transient failure later.
    Retrying,
    /// Paused by a module update / hot-swap or an administrator.
    Paused,
    Completed,
    Failed,
    Cancelled,
}

impl InstanceState {
    pub fn as_str(self) -> &'static str {
        match self {
            InstanceState::Running => "running",
            InstanceState::WaitingForInput => "waitingForInput",
            InstanceState::WaitingForApproval => "waitingForApproval",
            InstanceState::WaitingForEvent => "waitingForEvent",
            InstanceState::Retrying => "retrying",
            InstanceState::Paused => "paused",
            InstanceState::Completed => "completed",
            InstanceState::Failed => "failed",
            InstanceState::Cancelled => "cancelled",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        serde_json::from_value(Value::String(s.to_string())).ok()
    }

    pub fn is_finished(self) -> bool {
        matches!(self, InstanceState::Completed | InstanceState::Failed | InstanceState::Cancelled)
    }

    pub fn is_waiting_for_person(self) -> bool {
        matches!(self, InstanceState::WaitingForInput | InstanceState::WaitingForApproval)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ActorKind {
    Human,
    Ai,
    Engine,
}

/// One line of the instance history (the audit trail of a run).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryEntry {
    pub at: DateTime<Utc>,
    pub step_id: String,
    #[serde(default)]
    pub step_title: String,
    pub actor_kind: ActorKind,
    pub actor_name: String,
    /// `started`, `completed`, `submitted`, `approved`, `rejected`, `proposed`, `confirmed`, `failed`,
    /// `retry`, `handedOver`, `migrated`, `paused`, `resumed`, `cancelled`.
    pub action: String,
    #[serde(default)]
    pub data: Value,
    #[serde(default)]
    pub confidence: Option<f64>,
    #[serde(default)]
    pub reasoning: Option<String>,
}

/// What the AI suggests for an interactive step.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AiProposal {
    pub values: Map<String, Value>,
    #[serde(default)]
    pub decision: Option<String>,
    pub confidence: f64,
    pub reasoning: String,
    pub provider: String,
    pub model: String,
    pub created_at: DateTime<Utc>,
}

/// A task waiting for a person.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingTask {
    pub step_id: String,
    pub kind: StepKind,
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub input: Option<InputSchema>,
    #[serde(default)]
    pub options: Vec<ChoiceOption>,
    /// Permissions of people who may complete it (empty = the RunAs user only).
    #[serde(default)]
    pub approvers: Vec<String>,
    #[serde(default)]
    pub assigned_to: Option<Uuid>,
    /// Values the form starts with (parameters, AI proposal).
    #[serde(default)]
    pub prefill: Map<String, Value>,
    #[serde(default)]
    pub ai_proposal: Option<AiProposal>,
    /// The task confirms what the AI wants to do (instead of asking for input).
    #[serde(default)]
    pub confirms_ai: bool,
    pub created_at: DateTime<Utc>,
    #[serde(default)]
    pub due_at: Option<DateTime<Utc>>,
}

/// How an instance was started.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TriggerInfo {
    pub trigger_id: String,
    pub kind: String,
    #[serde(default)]
    pub event_id: Option<Uuid>,
    #[serde(default)]
    pub payload: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowInstance {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub workflow_id: Uuid,
    pub workflow_version: i32,
    pub workflow_name: String,
    pub state: InstanceState,
    #[serde(default)]
    pub current_step_id: Option<String>,
    pub executor: WorkflowExecutor,
    pub run_as: Uuid,
    pub run_as_name: String,
    #[serde(default)]
    pub vars: Map<String, Value>,
    /// Outputs per step id.
    #[serde(default)]
    pub steps: Map<String, Value>,
    /// Result of the previous step.
    #[serde(default)]
    pub last: Value,
    #[serde(default)]
    pub history: Vec<HistoryEntry>,
    #[serde(default)]
    pub pending: Option<PendingTask>,
    #[serde(default)]
    pub trigger: Option<TriggerInfo>,
    #[serde(default)]
    pub error: Option<Error>,
    #[serde(default)]
    pub attempts: u32,
    #[serde(default)]
    pub parent_instance_id: Option<Uuid>,
    #[serde(default)]
    pub ai_tokens: i64,
    /// Steps a person confirmed for the AI (critical steps run only after confirmation).
    #[serde(default)]
    pub confirmed_steps: Vec<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl WorkflowInstance {
    pub fn push_history(&mut self, step_id: &str, step_title: &str, actor_kind: ActorKind, actor_name: &str, action: &str, data: Value) {
        self.history.push(HistoryEntry {
            at: Utc::now(),
            step_id: step_id.to_string(),
            step_title: step_title.to_string(),
            actor_kind,
            actor_name: actor_name.to_string(),
            action: action.to_string(),
            data,
            confidence: None,
            reasoning: None,
        });
    }

    /// How often the AI entered `step_id` in this instance (loop detection).
    pub fn ai_entries(&self, step_id: &str) -> usize {
        self.history.iter().filter(|h| h.step_id == step_id && h.actor_kind == ActorKind::Ai && h.action == "proposed").count()
    }
}
