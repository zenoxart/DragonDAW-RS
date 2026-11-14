//! Request/response types of the workflow service and the AI agent.

use super::analysis::Issue;
use super::definition::*;
use super::instance::*;
use crate::modules::SurfaceChange;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveWorkflowRequest {
    pub definition: WorkflowDefinition,
    /// Publish (status active) instead of saving a draft.
    #[serde(default)]
    pub publish: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveWorkflowResponse {
    pub id: Uuid,
    pub version: i32,
    pub status: WorkflowStatus,
    pub issues: Vec<Issue>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowSummary {
    pub id: Uuid,
    pub version: i32,
    pub name: String,
    pub description: String,
    pub status: WorkflowStatus,
    pub origin: WorkflowOrigin,
    #[serde(default)]
    pub owning_module: Option<String>,
    pub triggers: Vec<String>,
    pub ai_autonomy: Autonomy,
    pub steps: usize,
    pub updated_at: DateTime<Utc>,
    pub running_instances: i64,
    #[serde(default)]
    pub review_reasons: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidateWorkflowResponse {
    pub issues: Vec<Issue>,
    pub dependencies: Vec<WorkflowDependency>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct StartWorkflowRequest {
    #[serde(default)]
    pub vars: Map<String, Value>,
    #[serde(default)]
    pub executor: Option<WorkflowExecutor>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CompleteTaskRequest {
    /// Form values (input/review steps).
    #[serde(default)]
    pub values: Map<String, Value>,
    /// `approve` / `reject` (approval steps, AI confirmations) or the chosen option (choice steps).
    #[serde(default)]
    pub decision: Option<String>,
    #[serde(default)]
    pub comment: Option<String>,
}

/// A task in the inbox.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InboxItem {
    pub instance_id: Uuid,
    pub workflow_id: Uuid,
    pub workflow_name: String,
    pub state: InstanceState,
    pub executor: WorkflowExecutor,
    pub run_as_name: String,
    pub task: PendingTask,
    pub vars: Map<String, Value>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstanceSummary {
    pub id: Uuid,
    pub workflow_id: Uuid,
    pub workflow_name: String,
    pub workflow_version: i32,
    pub state: InstanceState,
    #[serde(default)]
    pub current_step_id: Option<String>,
    pub executor: WorkflowExecutor,
    pub run_as_name: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Registry → workflow service: analyse a module update.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalyzeImpactRequest {
    pub module_id: String,
    pub from_version: String,
    pub to_version: String,
    pub changes: Vec<SurfaceChange>,
    /// Apply migrations / statuses (false = dry run).
    #[serde(default = "yes")]
    pub apply: bool,
}

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalyzeImpactResponse {
    pub report_id: Uuid,
    pub compatible: usize,
    pub auto_migrated: usize,
    pub review_required: usize,
    pub broken: usize,
}

/// Hot-swap / admin: pause or resume running instances of a module.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModuleInstancesRequest {
    pub module_id: String,
    pub pause: bool,
}

// ------------------------------------------------------------------ AI agent

/// Workflow service → AI agent: propose a result for a step.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiStepRequest {
    pub definition: WorkflowDefinition,
    pub instance: WorkflowInstance,
    pub step_id: String,
    /// Delegated token of the RunAs user (tools are called with it).
    pub access_token: String,
}

/// A tool call the agent made while working on a step.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiToolCall {
    pub tool: String,
    pub inputs: Map<String, Value>,
    #[serde(default)]
    pub outputs: Value,
    #[serde(default)]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiStepResponse {
    pub proposal: AiProposal,
    #[serde(default)]
    pub tool_calls: Vec<AiToolCall>,
    pub tokens: i64,
    pub audit_id: Uuid,
}
