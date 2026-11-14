//! Workflow definitions.

use super::input::{ChoiceOption, InputSchema};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use uuid::Uuid;

/// Id of the implicit end step.
pub const END: &str = "end";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowDefinition {
    #[serde(default = "Uuid::new_v4")]
    pub id: Uuid,
    #[serde(default = "one")]
    pub version: i32,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub origin: WorkflowOrigin,
    /// Module that ships this workflow as a template.
    #[serde(default)]
    pub owning_module: Option<String>,
    pub start_step_id: String,
    pub steps: Vec<WorkflowStep>,
    #[serde(default)]
    pub transitions: Vec<WorkflowTransition>,
    #[serde(default)]
    pub variables: Vec<WorkflowVariable>,
    #[serde(default)]
    pub triggers: Vec<WorkflowTrigger>,
    #[serde(default)]
    pub ai_policy: AiExecutionPolicy,
    /// Extracted on save (see `analysis::dependencies`).
    #[serde(default)]
    pub dependencies: Vec<WorkflowDependency>,
    #[serde(default)]
    pub status: WorkflowStatus,
    #[serde(default)]
    pub owner_id: Option<Uuid>,
    #[serde(default)]
    pub updated_at: Option<DateTime<Utc>>,
    /// Why the workflow needs review (set by the impact analysis).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub review_reasons: Vec<String>,
}

fn one() -> i32 {
    1
}

impl WorkflowDefinition {
    pub fn step(&self, id: &str) -> Option<&WorkflowStep> {
        self.steps.iter().find(|s| s.id == id)
    }

    /// Transitions leaving `step_id`, ordered.
    pub fn transitions_from(&self, step_id: &str) -> Vec<&WorkflowTransition> {
        let mut t: Vec<_> = self.transitions.iter().filter(|t| t.from == step_id).collect();
        t.sort_by_key(|t| t.order);
        t
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum WorkflowOrigin {
    #[default]
    User,
    ModuleTemplate,
    Recorded,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum WorkflowStatus {
    #[default]
    Draft,
    Active,
    /// Set by the impact analysis; triggers are suspended until a person reviews it.
    NeedsReview,
    /// A dependency was removed; the workflow cannot run.
    Broken,
    Retired,
}

impl WorkflowStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            WorkflowStatus::Draft => "draft",
            WorkflowStatus::Active => "active",
            WorkflowStatus::NeedsReview => "needsReview",
            WorkflowStatus::Broken => "broken",
            WorkflowStatus::Retired => "retired",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum StepKind {
    /// A person (or the AI) fills a form.
    #[default]
    Input,
    /// Pick one of several options.
    Choice,
    /// Check data and fill a form; the AI may call `tools` (step types) to fetch data.
    Review,
    /// Run a workflow-capable command.
    Command,
    /// Run a module step type headlessly.
    ModuleStep,
    /// Wait until an event arrives (or a timeout).
    WaitForEvent,
    /// A person approves or rejects – never the AI.
    Approval,
    /// A free-form AI task (summarize, classify) whose result goes into variables.
    AiTask,
    /// Run another workflow and continue when it finishes.
    SubWorkflow,
}

impl StepKind {
    /// Whether the step waits for a person (or the AI acting as one).
    pub fn is_interactive(self) -> bool {
        matches!(self, StepKind::Input | StepKind::Choice | StepKind::Review | StepKind::Approval)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowStep {
    pub id: String,
    pub kind: StepKind,
    pub title: String,
    #[serde(default)]
    pub description: String,
    /// Form of input/review steps.
    #[serde(default)]
    pub input: Option<InputSchema>,
    /// Step type of module steps.
    #[serde(default)]
    pub step_type_id: Option<String>,
    #[serde(default = "one_u")]
    pub step_type_version: u32,
    /// Command of command steps.
    #[serde(default)]
    pub command_id: Option<String>,
    /// `{input name: expression}` for module steps, commands and sub-workflows; prefill for forms.
    #[serde(default)]
    pub parameters: BTreeMap<String, String>,
    /// `{variable: expression}` evaluated after the step (`last.x` is the step result); outputs are always
    /// available as `steps.<id>.<output>`.
    #[serde(default)]
    pub output_mapping: BTreeMap<String, String>,
    /// Options of choice steps.
    #[serde(default)]
    pub options: Vec<ChoiceOption>,
    /// Step types the AI may call in review steps.
    #[serde(default)]
    pub tools: Vec<String>,
    /// Permissions of people who may complete the step (approval/input); empty = the RunAs user.
    #[serde(default)]
    pub approvers: Vec<String>,
    /// Event the step waits for (`waitForEvent`).
    #[serde(default)]
    pub event_type: Option<String>,
    /// Condition on the awaited event (`payload.invoiceId == vars.invoiceId`).
    #[serde(default)]
    pub event_filter: Option<String>,
    #[serde(default)]
    pub timeout_minutes: Option<i64>,
    /// Sub-workflow to run.
    #[serde(default)]
    pub workflow_id: Option<Uuid>,
    /// AI task instruction.
    #[serde(default)]
    pub prompt: Option<String>,
    /// Never run by the AI.
    #[serde(default)]
    pub requires_human: bool,
    /// A person must confirm before this step runs when the AI executes the workflow (`approveCritical`).
    #[serde(default)]
    pub critical: bool,
    #[serde(default = "two")]
    pub max_retries: u32,
}

fn one_u() -> u32 {
    1
}
fn two() -> u32 {
    2
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowTransition {
    pub from: String,
    /// Step id or `end`.
    pub to: String,
    /// Expression; empty = always.
    #[serde(default)]
    pub condition: Option<String>,
    #[serde(default)]
    pub order: i32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowVariable {
    pub name: String,
    #[serde(rename = "type", default = "text")]
    pub var_type: String,
    #[serde(default)]
    pub sensitive: bool,
    #[serde(default)]
    pub default: Option<serde_json::Value>,
}

fn text() -> String {
    "text".into()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum TriggerKind {
    #[default]
    Manual,
    Command,
    Event,
    Schedule,
    Condition,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum WorkflowExecutor {
    #[default]
    Human,
    Ai,
}

impl WorkflowExecutor {
    pub fn as_str(self) -> &'static str {
        match self {
            WorkflowExecutor::Human => "human",
            WorkflowExecutor::Ai => "ai",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowTrigger {
    #[serde(default = "trigger_id")]
    pub id: String,
    pub kind: TriggerKind,
    #[serde(default)]
    pub event_type: Option<String>,
    #[serde(default)]
    pub command_id: Option<String>,
    /// `every:30` (minutes) or `daily:07:30` (UTC).
    #[serde(default)]
    pub schedule: Option<String>,
    /// Module API returning a JSON boolean (`condition` triggers).
    #[serde(default)]
    pub condition_url: Option<String>,
    #[serde(default = "five_minutes")]
    pub interval_seconds: u64,
    /// Expression over `payload` that must be true.
    #[serde(default)]
    pub filter: Option<String>,
    /// `{variable: expression over payload}`.
    #[serde(default)]
    pub payload_mapping: BTreeMap<String, String>,
    /// Expression; a second run with the same key is skipped.
    #[serde(default)]
    pub deduplication_key: Option<String>,
    #[serde(default)]
    pub executor: WorkflowExecutor,
    /// User the workflow runs as; default: the workflow owner.
    #[serde(default)]
    pub run_as: Option<Uuid>,
    #[serde(default = "yes")]
    pub enabled: bool,
    /// Set by the impact analysis (AI triggers of workflows that need review).
    #[serde(default)]
    pub suspended: bool,
    #[serde(default = "ten")]
    pub max_concurrent_runs: u32,
}

fn trigger_id() -> String {
    "trigger".into()
}
fn five_minutes() -> u64 {
    300
}
fn yes() -> bool {
    true
}
fn ten() -> u32 {
    10
}

/// How much the AI agent may do on its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum Autonomy {
    /// The AI never runs this workflow.
    #[default]
    Off,
    /// The AI pre-fills inputs; a person executes every step.
    Suggest,
    /// The AI proposes each step; a person approves it.
    ApproveEachStep,
    /// The AI runs; steps marked `critical` wait for a person.
    ApproveCritical,
    /// The AI runs every step it may; approval steps still need a person.
    Autonomous,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum AiFallback {
    #[default]
    HandOverToHuman,
    Fail,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiExecutionPolicy {
    #[serde(default)]
    pub autonomy: Autonomy,
    /// Steps the AI may run (empty = all that are not `requiresHuman`).
    #[serde(default)]
    pub allowed_steps: Vec<String>,
    /// Permissions of people who approve AI proposals.
    #[serde(default)]
    pub approvers: Vec<String>,
    #[serde(default = "day")]
    pub approval_timeout_minutes: i64,
    #[serde(default = "two_hundred")]
    pub max_runs_per_day: i64,
    #[serde(default = "twenty_five")]
    pub max_steps_per_run: i64,
    #[serde(default = "twenty_thousand")]
    pub max_tokens_per_run: i64,
    #[serde(default = "thirty")]
    pub time_limit_minutes: i64,
    /// Variables the AI may see (empty = all non-sensitive).
    #[serde(default)]
    pub data_scope: Vec<String>,
    #[serde(default = "default_profile")]
    pub model_profile: String,
    #[serde(default = "seventy")]
    pub required_confidence: f64,
    #[serde(default)]
    pub fallback: AiFallback,
}

fn day() -> i64 {
    1440
}
fn two_hundred() -> i64 {
    200
}
fn twenty_five() -> i64 {
    25
}
fn twenty_thousand() -> i64 {
    20000
}
fn thirty() -> i64 {
    30
}
fn default_profile() -> String {
    "default".into()
}
fn seventy() -> f64 {
    0.7
}

impl Default for AiExecutionPolicy {
    fn default() -> Self {
        serde_json::from_value(serde_json::json!({})).expect("defaults")
    }
}

impl AiExecutionPolicy {
    /// Whether the AI may run `step` at all.
    pub fn allows(&self, step: &WorkflowStep) -> bool {
        self.autonomy != Autonomy::Off
            && !step.requires_human
            && step.kind != StepKind::Approval
            && (self.allowed_steps.is_empty() || self.allowed_steps.contains(&step.id))
    }

    /// Whether a person must confirm the AI's proposal for `step` before it takes effect.
    pub fn needs_confirmation(&self, step: &WorkflowStep) -> bool {
        match self.autonomy {
            Autonomy::Off | Autonomy::Suggest | Autonomy::ApproveEachStep => true,
            Autonomy::ApproveCritical => step.critical,
            Autonomy::Autonomous => false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DependencyKind {
    Module,
    StepType,
    InputSchema,
    Command,
    Event,
    Workflow,
}

impl DependencyKind {
    pub fn as_str(self) -> &'static str {
        match self {
            DependencyKind::Module => "module",
            DependencyKind::StepType => "stepType",
            DependencyKind::InputSchema => "inputSchema",
            DependencyKind::Command => "command",
            DependencyKind::Event => "event",
            DependencyKind::Workflow => "workflow",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowDependency {
    pub kind: DependencyKind,
    pub id: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub used_by_steps: Vec<String>,
}
