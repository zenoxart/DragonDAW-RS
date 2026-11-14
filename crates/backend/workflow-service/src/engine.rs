//! The durable workflow engine.
//!
//! An instance is persisted after every step, so it can be resumed anywhere – after a restart, by another
//! person, or by the AI agent. Module steps and commands are **never** executed directly: they are HTTP calls
//! through the gateway with a short-lived *delegated* token of the RunAs user, so permissions, validation and
//! audit are the same as for a click in the UI. The engine reacts per error kind: retry on Unavailable/Timeout,
//! back to a person on Validation, stop on everything else.

use crate::catalog::Catalog;
use crate::store;
use chrono::{Duration, Utc};
use redhive_backend_core::hosting::Core;
use redhive_backend_core::{Auth, OutboxWriter, RedHiveUser};
use redhive_contracts::api::{DelegatedTokenRequest, TokenResponse, UserSummary};
use redhive_contracts::notifications::{event_types, NotificationAudience, NotificationEnvelope, Severity};
use redhive_contracts::workflows::api::{AiStepRequest, AiStepResponse, CompleteTaskRequest};
use redhive_contracts::workflows::expr::{self, Scope};
use redhive_contracts::workflows::input::{ChoiceOption, FieldType, InputField, InputSchema};
use redhive_contracts::workflows::*;
use redhive_monads::{Error, ErrorKind, Res};
use serde_json::{json, Map, Value};
use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use uuid::Uuid;

pub const SOURCE: &str = "workflow-service";
const MAX_STEPS_PER_ADVANCE: usize = 60;

#[derive(Clone)]
pub struct Engine {
    pub core: Core,
    pub catalog: Catalog,
    pub outbox: OutboxWriter,
    running: Arc<Mutex<HashSet<Uuid>>>,
}

enum Outcome {
    /// The step finished with this result.
    Done { result: Value, actor: ActorKind, actor_name: String, action: &'static str },
    /// The instance waits (task, event, retry); it was already updated.
    Wait,
    Fail(Error),
}

struct RunGuard {
    id: Uuid,
    running: Arc<Mutex<HashSet<Uuid>>>,
}

impl Drop for RunGuard {
    fn drop(&mut self) {
        if let Ok(mut r) = self.running.lock() {
            r.remove(&self.id);
        }
    }
}

pub fn scope(inst: &WorkflowInstance) -> Scope {
    let payload = inst.trigger.as_ref().map(|t| t.payload.clone()).unwrap_or(Value::Null);
    Scope::new()
        .with("vars", Value::Object(inst.vars.iter().filter(|(k, _)| !k.starts_with("__")).map(|(k, v)| (k.clone(), v.clone())).collect()))
        .with("steps", Value::Object(inst.steps.clone()))
        .with("last", inst.last.clone())
        .with("payload", payload.clone())
        .with("trigger", payload)
        .with("user", json!({ "id": inst.run_as, "name": inst.run_as_name }))
}

fn eval_params(params: &std::collections::BTreeMap<String, String>, sc: &Scope) -> Res<Map<String, Value>> {
    let mut out = Map::new();
    for (k, src) in params {
        let v = expr::eval_str(src, sc).map_err(|e| Error::validation("workflow.parameter.invalid", "workflow:parameter.invalid").with_metadata("parameter", k.clone()).with_message(e.message.unwrap_or_default()))?;
        out.insert(k.clone(), v);
    }
    Ok(out)
}

/// Prefill values: like parameters, but a failing expression just leaves the field empty.
fn prefill(params: &std::collections::BTreeMap<String, String>, sc: &Scope) -> Map<String, Value> {
    params.iter().filter_map(|(k, src)| expr::eval_str(src, sc).ok().filter(|v| !v.is_null()).map(|v| (k.clone(), v))).collect()
}

impl Engine {
    pub fn new(core: Core, catalog: Catalog) -> Self {
        Engine { core, catalog, outbox: OutboxWriter::new(SOURCE), running: Arc::new(Mutex::new(HashSet::new())) }
    }

    // ------------------------------------------------------------------ events

    async fn emit(&self, env: NotificationEnvelope) {
        if let Err(e) = self.outbox.add_now(&self.core, &env).await {
            tracing::warn!(error = %e, "workflow event could not be written");
        }
    }

    fn envelope(&self, inst: &WorkflowInstance, event: &str, audience: NotificationAudience) -> NotificationEnvelope {
        let mut e = NotificationEnvelope::new(
            event,
            SOURCE,
            inst.tenant_id,
            audience,
            json!({ "instanceId": inst.id, "workflowId": inst.workflow_id, "workflowName": inst.workflow_name, "state": inst.state, "stepId": inst.current_step_id }),
        );
        e.actor = Some("workflow-engine".into());
        e.text_args.insert("workflow".into(), json!(inst.workflow_name));
        e
    }

    async fn notify_changed(&self, inst: &WorkflowInstance) {
        self.emit(self.envelope(inst, event_types::WORKFLOW_INSTANCE_CHANGED, NotificationAudience::Tenant)).await;
    }

    async fn notify_task(&self, inst: &WorkflowInstance) {
        let Some(task) = &inst.pending else { return };
        let audience = match (task.assigned_to, task.approvers.first()) {
            (Some(u), _) => NotificationAudience::User(u),
            (None, Some(p)) => NotificationAudience::WithPermission(p.clone()),
            (None, None) => NotificationAudience::User(inst.run_as),
        };
        let (event, title) = if task.confirms_ai || task.kind == StepKind::Approval {
            (event_types::WORKFLOW_APPROVAL_REQUESTED, if task.confirms_ai { "workflow:notification.aiApproval" } else { "workflow:notification.approval" })
        } else {
            (event_types::WORKFLOW_TASK_ASSIGNED, "workflow:notification.task")
        };
        let mut e = self.envelope(inst, event, audience).titled(title, Severity::Info).navigate("workflows.inbox", Some(inst.id.to_string()));
        e.text_args.insert("step".into(), json!(task.title));
        self.emit(e).await;
    }

    // ------------------------------------------------------------------ start

    /// Starts an instance and runs it until it waits or ends.
    #[allow(clippy::too_many_arguments)]
    pub async fn start(&self, def: &WorkflowDefinition, tenant: Uuid, run_as: Uuid, executor: WorkflowExecutor, mut vars: Map<String, Value>, trigger: Option<TriggerInfo>, parent: Option<Uuid>) -> Res<WorkflowInstance> {
        if def.status != WorkflowStatus::Active && def.status != WorkflowStatus::Draft {
            return Err(Error::conflict("workflow.notRunnable", "workflow:definition.notRunnable").with_metadata("status", def.status.as_str()));
        }
        if executor == WorkflowExecutor::Ai && def.ai_policy.autonomy == Autonomy::Off {
            return Err(Error::forbidden("workflow.ai.off", "workflow:ai.off"));
        }
        let user: UserSummary = self.core.services.get_internal("identity-service", &format!("/internal/users/{run_as}")).await?;
        if !user.is_active {
            return Err(Error::forbidden("workflow.runAs.inactive", "workflow:runAs.inactive"));
        }
        for v in &def.variables {
            if let (false, Some(d)) = (vars.contains_key(&v.name), &v.default) {
                vars.insert(v.name.clone(), d.clone());
            }
        }
        let now = Utc::now();
        let mut inst = WorkflowInstance {
            id: Uuid::new_v4(),
            tenant_id: tenant,
            workflow_id: def.id,
            workflow_version: def.version,
            workflow_name: def.name.clone(),
            state: InstanceState::Running,
            current_step_id: Some(def.start_step_id.clone()),
            executor,
            run_as,
            run_as_name: user.display_name.clone(),
            vars,
            steps: Map::new(),
            last: Value::Null,
            history: Vec::new(),
            pending: None,
            trigger,
            error: None,
            attempts: 0,
            parent_instance_id: parent,
            ai_tokens: 0,
            confirmed_steps: Vec::new(),
            created_at: now,
            updated_at: now,
        };
        let how = inst.trigger.as_ref().map(|t| format!("{} ({})", t.trigger_id, t.kind)).unwrap_or_else(|| "manual".into());
        inst.push_history(&def.start_step_id, "", ActorKind::Engine, "workflow-engine", "started", json!({ "trigger": how, "executor": executor }));
        store::save_instance(&self.core, &inst).await?;
        tracing::info!(instance = %inst.id, workflow = %def.name, executor = executor.as_str(), "workflow started");
        self.advance(inst.id).await
    }

    // ------------------------------------------------------------------ run

    /// Runs the instance until it waits, completes or fails. Concurrent calls for the same instance are ignored.
    /// (Boxed: the engine is recursive through sub-workflows.)
    pub fn advance(&self, id: Uuid) -> std::pin::Pin<Box<dyn std::future::Future<Output = Res<WorkflowInstance>> + Send + '_>> {
        Box::pin(self.advance_inner(id))
    }

    async fn advance_inner(&self, id: Uuid) -> Res<WorkflowInstance> {
        let already_running = match self.running.lock() {
            Ok(mut r) => !r.insert(id),
            Err(_) => return Err(Error::unexpected("workflow.lock", "errors:unexpected")),
        };
        if already_running {
            return store::load_instance(&self.core, id).await;
        }
        let _guard = RunGuard { id, running: self.running.clone() };
        let mut inst = store::load_instance(&self.core, id).await?;
        let def = store::version(&self.core, inst.workflow_id, inst.workflow_version).await?;
        let mut steps = 0;
        while inst.state == InstanceState::Running {
            steps += 1;
            if steps > MAX_STEPS_PER_ADVANCE {
                self.fail(&mut inst, Error::conflict("workflow.loop", "workflow:loopDetected")).await?;
                break;
            }
            let Some(step) = inst.current_step_id.as_ref().and_then(|s| def.step(s)).cloned() else {
                self.fail(&mut inst, Error::not_found("workflow.step.missing", "workflow:step.missing")).await?;
                break;
            };
            match self.execute(&def, &mut inst, &step).await {
                Outcome::Done { result, actor, actor_name, action } => self.finish_step(&def, &mut inst, &step, result, actor, &actor_name, action).await?,
                Outcome::Wait => {
                    store::save_instance(&self.core, &inst).await?;
                    self.notify_task(&inst).await;
                    self.notify_changed(&inst).await;
                    break;
                }
                Outcome::Fail(e) => {
                    self.fail(&mut inst, e).await?;
                    break;
                }
            }
        }
        Ok(inst)
    }

    async fn fail(&self, inst: &mut WorkflowInstance, e: Error) -> Res<()> {
        tracing::warn!(instance = %inst.id, code = %e.code, "workflow failed");
        let step = inst.current_step_id.clone().unwrap_or_default();
        inst.push_history(&step, "", ActorKind::Engine, "workflow-engine", "failed", json!({ "code": e.code, "textKey": e.text_key, "metadata": e.metadata }));
        inst.state = InstanceState::Failed;
        inst.error = Some(e);
        inst.pending = None;
        store::save_instance(&self.core, inst).await?;
        let mut env = self.envelope(inst, event_types::WORKFLOW_FAILED, NotificationAudience::User(inst.run_as)).titled("workflow:notification.failed", Severity::Error).navigate("workflows.instances", Some(inst.id.to_string()));
        env.text_args.insert("error".into(), json!(inst.error.as_ref().map(|e| e.text_key.clone())));
        self.emit(env).await;
        self.notify_changed(inst).await;
        if let Some(parent) = inst.parent_instance_id {
            self.child_finished(parent, inst);
        }
        Ok(())
    }

    /// Records the result of a step, picks the next step and saves.
    #[allow(clippy::too_many_arguments)]
    pub async fn finish_step(&self, def: &WorkflowDefinition, inst: &mut WorkflowInstance, step: &WorkflowStep, result: Value, actor: ActorKind, actor_name: &str, action: &str) -> Res<()> {
        if matches!(step.kind, StepKind::Input | StepKind::Review | StepKind::Choice | StepKind::AiTask) {
            if let Value::Object(o) = &result {
                for (k, v) in o {
                    if !k.starts_with("__") {
                        inst.vars.insert(k.clone(), v.clone());
                    }
                }
            }
        }
        inst.steps.insert(step.id.clone(), result.clone());
        inst.last = result.clone();
        let sc = scope(inst);
        for (var, src) in &step.output_mapping {
            match expr::eval_str(src, &sc) {
                Ok(v) => {
                    inst.vars.insert(var.clone(), v);
                }
                Err(e) => tracing::warn!(step = %step.id, var, error = %e, "output mapping failed"),
            }
        }
        let mut entry = HistoryEntry {
            at: Utc::now(),
            step_id: step.id.clone(),
            step_title: step.title.clone(),
            actor_kind: actor,
            actor_name: actor_name.to_string(),
            action: action.to_string(),
            data: result,
            confidence: None,
            reasoning: None,
        };
        if let Some(p) = inst.pending.as_ref().and_then(|p| p.ai_proposal.as_ref()) {
            entry.confidence = Some(p.confidence);
        }
        inst.history.push(entry);
        inst.pending = None;
        inst.attempts = 0;
        inst.error = None;
        inst.vars.retain(|k, _| !(k.starts_with("__wait") || k == "__retryAt" || k == &format!("__override.{}", step.id) || k == &format!("__actAs.{}", step.id) || k == &format!("__actAsName.{}", step.id)));
        inst.confirmed_steps.retain(|s| s != &step.id);

        let sc = scope(inst);
        let mut next = END.to_string();
        for t in def.transitions_from(&step.id) {
            match expr::condition(t.condition.as_deref().unwrap_or(""), &sc) {
                Ok(true) => {
                    next = t.to.clone();
                    break;
                }
                Ok(false) => {}
                Err(e) => tracing::warn!(from = %t.from, to = %t.to, error = %e, "transition condition failed – skipped"),
            }
        }
        if next == END {
            inst.state = InstanceState::Completed;
            inst.current_step_id = None;
            inst.push_history(END, "", ActorKind::Engine, "workflow-engine", "completed", Value::Null);
            store::save_instance(&self.core, inst).await?;
            tracing::info!(instance = %inst.id, workflow = %inst.workflow_name, "workflow completed");
            let env = self.envelope(inst, event_types::WORKFLOW_COMPLETED, NotificationAudience::User(inst.run_as));
            let env = if inst.executor == WorkflowExecutor::Human { env.titled("workflow:notification.completed", Severity::Success).navigate("workflows.instances", Some(inst.id.to_string())) } else { env };
            self.emit(env).await;
            self.notify_changed(inst).await;
            if let Some(parent) = inst.parent_instance_id {
                self.child_finished(parent, inst);
            }
        } else {
            inst.current_step_id = Some(next);
            inst.state = InstanceState::Running;
            store::save_instance(&self.core, inst).await?;
        }
        Ok(())
    }

    /// A sub-workflow ended: continue the parent with the child's variables (or fail it).
    fn child_finished(&self, parent_id: Uuid, child: &WorkflowInstance) {
        let engine = self.clone();
        let child = child.clone();
        let task: std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>> = Box::pin(async move {
            let r: Res<()> = async {
                let mut parent = store::load_instance(&engine.core, parent_id).await?;
                if parent.state != InstanceState::WaitingForEvent || parent.vars.get("__waitChild").and_then(Value::as_str) != Some(&child.id.to_string()) {
                    return Ok(());
                }
                let def = store::version(&engine.core, parent.workflow_id, parent.workflow_version).await?;
                let step = parent.current_step_id.as_ref().and_then(|s| def.step(s)).cloned().ok_or_else(|| Error::not_found("workflow.step.missing", "workflow:step.missing"))?;
                parent.vars.remove("__waitChild");
                if child.state == InstanceState::Completed {
                    let mut result = Value::Object(child.vars.clone());
                    result["instanceId"] = json!(child.id);
                    engine.finish_step(&def, &mut parent, &step, result, ActorKind::Engine, "workflow-engine", "completed").await?;
                    engine.advance(parent.id).await.map(|_| ())
                } else {
                    let e = child.error.clone().unwrap_or_else(|| Error::conflict("workflow.child.cancelled", "workflow:child.cancelled"));
                    engine.fail(&mut parent, e).await
                }
            }
            .await;
            if let Err(e) = r {
                tracing::warn!(error = %e, "parent workflow could not be resumed");
            }
        });
        tokio::spawn(task);
    }

    async fn delegated_token(&self, inst: &WorkflowInstance, user: Uuid, actor: &str) -> Res<String> {
        let r: TokenResponse = self
            .core
            .services
            .post_internal("identity-service", "/internal/tokens/delegated", &DelegatedTokenRequest { user_id: user, actor: actor.into(), instance_id: Some(inst.id), minutes: 10 })
            .await?;
        Ok(r.access_token)
    }

    /// The user a module step runs as: the person who confirmed it (they take responsibility), else the RunAs user.
    fn acting_user(inst: &WorkflowInstance, step_id: &str) -> (Uuid, String) {
        let id = inst.vars.get(&format!("__actAs.{step_id}")).and_then(Value::as_str).and_then(|s| Uuid::parse_str(s).ok());
        let name = inst.vars.get(&format!("__actAsName.{step_id}")).and_then(Value::as_str).map(str::to_string);
        match (id, name) {
            (Some(id), Some(name)) => (id, name),
            _ => (inst.run_as, inst.run_as_name.clone()),
        }
    }

    /// Calls a module endpoint through the gateway as the acting user.
    async fn call_module(&self, inst: &WorkflowInstance, step_id: &str, endpoint: &str, inputs: &Map<String, Value>) -> Res<Value> {
        let actor = if inst.executor == WorkflowExecutor::Ai { "ai-agent" } else { "workflow-engine" };
        let (user, _) = Self::acting_user(inst, step_id);
        let token = self.delegated_token(inst, user, actor).await?;
        let url = self.core.services.url("api-gateway", endpoint);
        self.core.services.send(reqwest::Method::POST, &url, Some(&Value::Object(inputs.clone())), &Auth::Bearer(token), Some(&inst.id.simple().to_string())).await
    }

    fn task(step: &WorkflowStep, def: &WorkflowDefinition, inst: &WorkflowInstance) -> PendingTask {
        let approvers = if step.approvers.is_empty() && (step.kind == StepKind::Approval || step.critical) { def.ai_policy.approvers.clone() } else { step.approvers.clone() };
        PendingTask {
            step_id: step.id.clone(),
            kind: step.kind,
            title: step.title.clone(),
            description: step.description.clone(),
            input: step.input.clone(),
            options: step.options.clone(),
            assigned_to: if approvers.is_empty() { Some(inst.run_as) } else { None },
            approvers,
            prefill: Map::new(),
            ai_proposal: None,
            confirms_ai: false,
            created_at: Utc::now(),
            due_at: Some(Utc::now() + Duration::minutes(def.ai_policy.approval_timeout_minutes.max(1))),
        }
    }

    async fn ask_ai(&self, def: &WorkflowDefinition, inst: &WorkflowInstance, step: &WorkflowStep) -> Res<AiStepResponse> {
        let token = self.delegated_token(inst, inst.run_as, "ai-agent").await?;
        self.core.services.post_internal("ai-agent", "/internal/propose", &AiStepRequest { definition: def.clone(), instance: inst.clone(), step_id: step.id.clone(), access_token: token }).await
    }

    /// Executes the current step.
    async fn execute(&self, def: &WorkflowDefinition, inst: &mut WorkflowInstance, step: &WorkflowStep) -> Outcome {
        let sc = scope(inst);
        let ai_runs = inst.executor == WorkflowExecutor::Ai && def.ai_policy.autonomy != Autonomy::Off;
        match step.kind {
            StepKind::Input | StepKind::Review | StepKind::Choice | StepKind::AiTask => {
                let mut task = Self::task(step, def, inst);
                task.prefill = prefill(&step.parameters, &sc);
                if step.kind == StepKind::AiTask && task.input.is_none() {
                    task.input = Some(InputSchema {
                        schema_id: format!("workflow.aiTask.{}", step.id),
                        version: 1,
                        fields: vec![InputField { name: "result".into(), field_type: FieldType::MultiLine, label: "workflow:field.result".into(), required: true, ..Default::default() }],
                    });
                }
                if ai_runs && def.ai_policy.allows(step) {
                    match self.ask_ai(def, inst, step).await {
                        Ok(resp) => {
                            inst.ai_tokens += resp.tokens;
                            let p = resp.proposal.clone();
                            inst.history.push(HistoryEntry {
                                at: Utc::now(),
                                step_id: step.id.clone(),
                                step_title: step.title.clone(),
                                actor_kind: ActorKind::Ai,
                                actor_name: format!("AI agent ({})", p.model),
                                action: "proposed".into(),
                                data: json!({ "values": p.values, "decision": p.decision, "tools": resp.tool_calls.iter().map(|t| &t.tool).collect::<Vec<_>>() }),
                                confidence: Some(p.confidence),
                                reasoning: Some(p.reasoning.clone()),
                            });
                            let mut values = task.prefill.clone();
                            values.extend(p.values.clone());
                            if def.ai_policy.autonomy == Autonomy::Suggest {
                                task.prefill = values;
                                task.ai_proposal = Some(p);
                                inst.pending = Some(task);
                                inst.state = InstanceState::WaitingForInput;
                                return Outcome::Wait;
                            }
                            if def.ai_policy.needs_confirmation(step) {
                                task.prefill = values;
                                task.ai_proposal = Some(p);
                                task.confirms_ai = true;
                                if task.approvers.is_empty() {
                                    task.approvers = def.ai_policy.approvers.clone();
                                    task.assigned_to = if task.approvers.is_empty() { Some(inst.run_as) } else { None };
                                }
                                inst.pending = Some(task);
                                inst.state = InstanceState::WaitingForApproval;
                                return Outcome::Wait;
                            }
                            let validated = match &step.input {
                                Some(schema) => schema.validate(&values),
                                None => Ok(values.clone()),
                            };
                            match validated {
                                Ok(mut v) => {
                                    if let Some(d) = &p.decision {
                                        v.insert("decision".into(), json!(d));
                                    }
                                    return Outcome::Done { result: Value::Object(v), actor: ActorKind::Ai, actor_name: format!("AI agent ({})", p.model), action: "submitted" };
                                }
                                Err(e) => {
                                    inst.push_history(&step.id, &step.title, ActorKind::Engine, "workflow-engine", "handedOver", json!({ "reason": e.code }));
                                    task.prefill = values;
                                    task.ai_proposal = Some(p);
                                }
                            }
                        }
                        Err(e) => {
                            // Guardrail, low confidence, provider down: a person takes over (with the AI's hint, if any).
                            let mut h = HistoryEntry {
                                at: Utc::now(),
                                step_id: step.id.clone(),
                                step_title: step.title.clone(),
                                actor_kind: ActorKind::Ai,
                                actor_name: "AI agent".into(),
                                action: "handedOver".into(),
                                data: json!({ "code": e.code, "textKey": e.text_key }),
                                confidence: e.metadata.get("confidence").and_then(Value::as_f64),
                                reasoning: e.metadata.get("reasoning").and_then(Value::as_str).map(str::to_string),
                            };
                            if let Some(proposal) = e.metadata.get("proposal").and_then(|p| serde_json::from_value::<AiProposal>(p.clone()).ok()) {
                                task.prefill.extend(proposal.values.clone());
                                h.confidence = Some(proposal.confidence);
                                task.ai_proposal = Some(proposal);
                            }
                            inst.history.push(h);
                            if def.ai_policy.fallback == AiFallback::Fail && e.kind != ErrorKind::Unavailable {
                                return Outcome::Fail(e);
                            }
                        }
                    }
                }
                if ai_runs && task.approvers.is_empty() && !def.ai_policy.approvers.is_empty() {
                    // The AI handed over: the people who supervise the agent take the task.
                    task.approvers = def.ai_policy.approvers.clone();
                    task.assigned_to = None;
                }
                inst.state = InstanceState::WaitingForInput;
                inst.pending = Some(task);
                Outcome::Wait
            }
            StepKind::Approval => {
                let mut task = Self::task(step, def, inst);
                task.prefill = prefill(&step.parameters, &sc);
                if task.options.is_empty() {
                    task.options = vec![ChoiceOption { value: "approve".into(), label: "workflow:decision.approve".into() }, ChoiceOption { value: "reject".into(), label: "workflow:decision.reject".into() }];
                }
                inst.pending = Some(task);
                inst.state = InstanceState::WaitingForApproval;
                Outcome::Wait
            }
            StepKind::ModuleStep | StepKind::Command => {
                let override_key = format!("__override.{}", step.id);
                let inputs = match inst.vars.get(&override_key) {
                    Some(Value::Object(o)) => o.clone(),
                    _ => match eval_params(&step.parameters, &sc) {
                        Ok(i) => i,
                        Err(e) => return Outcome::Fail(e),
                    },
                };
                // AI runs: critical steps (or every step for approveEachStep/suggest) wait for a person.
                let needs_person = ai_runs && (def.ai_policy.needs_confirmation(step) || step.requires_human || !def.ai_policy.allows(step));
                if needs_person && !inst.confirmed_steps.contains(&step.id) {
                    let mut task = Self::task(step, def, inst);
                    if task.approvers.is_empty() {
                        task.approvers = def.ai_policy.approvers.clone();
                        task.assigned_to = if task.approvers.is_empty() { Some(inst.run_as) } else { None };
                    }
                    task.prefill = inputs;
                    task.confirms_ai = true;
                    task.options = vec![ChoiceOption { value: "approve".into(), label: "workflow:decision.confirm".into() }, ChoiceOption { value: "reject".into(), label: "workflow:decision.cancel".into() }];
                    inst.pending = Some(task);
                    inst.state = InstanceState::WaitingForApproval;
                    return Outcome::Wait;
                }
                let (endpoint, schema) = if step.kind == StepKind::ModuleStep {
                    match self.catalog.step_type(&self.core, step.step_type_id.as_deref().unwrap_or("")).await {
                        Ok(st) => (st.endpoint, Some(st.input)),
                        Err(e) => return Outcome::Fail(e),
                    }
                } else {
                    match self.catalog.command(&self.core, step.command_id.as_deref().unwrap_or("")).await {
                        Ok(c) if c.workflow_capable && c.endpoint.is_some() => (c.endpoint.unwrap_or_default(), c.input),
                        Ok(_) => return Outcome::Fail(Error::conflict("workflow.command.notHeadless", "workflow:command.notHeadless").with_metadata("command", step.command_id.clone())),
                        Err(e) => return Outcome::Fail(e),
                    }
                };
                let validated = match &schema {
                    Some(s) => s.validate(&inputs),
                    None => Ok(inputs.clone()),
                };
                let result = match validated {
                    Ok(v) => self.call_module(inst, &step.id, &endpoint, &v).await,
                    Err(e) => Err(e),
                };
                let actor_name = if inst.executor == WorkflowExecutor::Ai { "AI agent (via workflow engine)".to_string() } else { "workflow engine".to_string() };
                let (_, acting) = Self::acting_user(inst, &step.id);
                match result {
                    Ok(out) => Outcome::Done { result: out, actor: ActorKind::Engine, actor_name: format!("{actor_name} as {acting}"), action: "completed" },
                    Err(e) if e.kind.is_transient() && inst.attempts < step.max_retries => {
                        inst.attempts += 1;
                        let at = Utc::now() + Duration::seconds(5 * 2i64.pow(inst.attempts));
                        inst.vars.insert("__retryAt".into(), json!(at.to_rfc3339()));
                        inst.state = InstanceState::Retrying;
                        inst.push_history(&step.id, &step.title, ActorKind::Engine, "workflow-engine", "retry", json!({ "attempt": inst.attempts, "code": e.code, "at": at }));
                        Outcome::Wait
                    }
                    Err(e) if e.kind == ErrorKind::Validation => {
                        // Back to a person: show the step's inputs with the errors.
                        inst.push_history(&step.id, &step.title, ActorKind::Engine, "workflow-engine", "handedOver", json!({ "code": e.code, "fieldErrors": e.field_errors }));
                        let mut task = Self::task(step, def, inst);
                        task.kind = StepKind::Input;
                        task.input = schema;
                        task.prefill = inputs;
                        task.description = format!("{} [{}]", step.description, e.text_key).trim().to_string();
                        task.approvers.clear();
                        task.assigned_to = Some(inst.run_as);
                        inst.vars.insert("__fix".into(), json!(step.id));
                        inst.error = Some(e);
                        inst.pending = Some(task);
                        inst.state = InstanceState::WaitingForInput;
                        Outcome::Wait
                    }
                    Err(e) => Outcome::Fail(e),
                }
            }
            StepKind::WaitForEvent => {
                let until = Utc::now() + Duration::minutes(step.timeout_minutes.unwrap_or(7 * 24 * 60));
                inst.vars.insert("__waitEvent".into(), json!(step.event_type.clone().unwrap_or_default()));
                inst.vars.insert("__waitUntil".into(), json!(until.to_rfc3339()));
                inst.state = InstanceState::WaitingForEvent;
                Outcome::Wait
            }
            StepKind::SubWorkflow => {
                let Some(child_id) = step.workflow_id else { return Outcome::Fail(Error::validation("step.workflow.missing", "workflow:step.workflow.missing")) };
                let child_def = match store::latest(&self.core, inst.tenant_id, child_id).await {
                    Ok(d) => d,
                    Err(e) => return Outcome::Fail(e),
                };
                let vars = prefill(&step.parameters, &sc);
                let engine = self.clone();
                let (tenant, run_as, executor, parent) = (inst.tenant_id, inst.run_as, inst.executor, inst.id);
                // the child id is needed before the parent waits – start it after saving the parent
                let child_instance_id = Uuid::new_v4();
                inst.vars.insert("__waitChild".into(), json!(child_instance_id.to_string()));
                inst.vars.insert("__waitEvent".into(), json!("workflow.child"));
                inst.state = InstanceState::WaitingForEvent;
                let task: std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>> = Box::pin(async move {
                    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                    match engine.start(&child_def, tenant, run_as, executor, vars, None, Some(parent)).await {
                        Ok(child) => {
                            // re-point the parent at the real child id
                            if let Ok(mut p) = store::load_instance(&engine.core, parent).await {
                                p.vars.insert("__waitChild".into(), json!(child.id.to_string()));
                                let _ = store::save_instance(&engine.core, &p).await;
                                if child.state.is_finished() {
                                    engine.child_finished(parent, &child);
                                }
                            }
                        }
                        Err(e) => {
                            if let Ok(mut p) = store::load_instance(&engine.core, parent).await {
                                let _ = engine.fail(&mut p, e).await;
                            }
                        }
                    }
                });
                tokio::spawn(task);
                Outcome::Wait
            }
        }
    }

    // ------------------------------------------------------------------ tasks

    /// Whether `user` may work on the pending task.
    pub fn may_complete(user: &RedHiveUser, inst: &WorkflowInstance) -> bool {
        let Some(t) = &inst.pending else { return false };
        user.has("*") || t.assigned_to == Some(user.id) || (!t.approvers.is_empty() && t.approvers.iter().any(|p| user.has(p))) || (t.approvers.is_empty() && t.assigned_to.is_none() && user.id == inst.run_as)
    }

    /// A person completes the pending task (form, decision or confirmation of an AI proposal).
    pub async fn complete(&self, user: &RedHiveUser, id: Uuid, req: CompleteTaskRequest) -> Res<WorkflowInstance> {
        user.require("workflows.run")?;
        let mut inst = store::load_instance(&self.core, id).await?;
        if user.tenant_id != inst.tenant_id {
            return Err(Error::not_found("workflow.instance", "workflow:instance.notFound"));
        }
        if !inst.state.is_waiting_for_person() {
            return Err(Error::conflict("workflow.task.none", "workflow:task.none").with_metadata("state", inst.state.as_str()));
        }
        if !Self::may_complete(user, &inst) {
            return Err(Error::forbidden("workflow.task.notAssigned", "workflow:task.notAssigned"));
        }
        let task = inst.pending.clone().ok_or_else(|| Error::conflict("workflow.task.none", "workflow:task.none"))?;
        let def = store::version(&self.core, inst.workflow_id, inst.workflow_version).await?;
        let step = def.step(&task.step_id).cloned().ok_or_else(|| Error::not_found("workflow.step.missing", "workflow:step.missing"))?;
        let who = user.display_name.clone();
        let decision = req.decision.clone().unwrap_or_default();
        let comment = req.comment.clone().unwrap_or_default();

        // A fix for a module step that failed validation: retry the step with the corrected inputs.
        if inst.vars.get("__fix").and_then(Value::as_str) == Some(step.id.as_str()) {
            let values = match &task.input {
                Some(s) => s.validate(&req.values)?,
                None => req.values.clone(),
            };
            inst.vars.remove("__fix");
            inst.vars.insert(format!("__override.{}", step.id), Value::Object(values.clone()));
            inst.push_history(&step.id, &step.title, ActorKind::Human, &who, "corrected", Value::Object(values));
            inst.pending = None;
            inst.error = None;
            inst.state = InstanceState::Running;
            store::save_instance(&self.core, &inst).await?;
            return self.advance(id).await;
        }

        if task.confirms_ai {
            let approve = match decision.as_str() {
                "approve" | "confirm" => true,
                "reject" | "cancel" => false,
                _ => return Err(Error::validation("decision.required", "workflow:decision.required").for_field("decision")),
            };
            if step.kind.is_interactive() || step.kind == StepKind::AiTask {
                if approve {
                    let mut values = task.prefill.clone();
                    values.extend(req.values.clone());
                    let mut values = match &step.input {
                        Some(s) => s.validate(&values)?,
                        None => values,
                    };
                    if let Some(d) = task.ai_proposal.as_ref().and_then(|p| p.decision.clone()) {
                        values.entry("decision").or_insert(json!(d));
                    }
                    self.finish_step(&def, &mut inst, &step, Value::Object(values), ActorKind::Human, &who, "confirmed").await?;
                } else {
                    // Rejecting the AI's proposal: the person fills the form instead.
                    inst.push_history(&step.id, &step.title, ActorKind::Human, &who, "rejectedProposal", json!({ "comment": comment }));
                    let mut t = task.clone();
                    t.confirms_ai = false;
                    t.assigned_to = Some(user.id);
                    t.approvers.clear();
                    inst.pending = Some(t);
                    inst.state = InstanceState::WaitingForInput;
                    store::save_instance(&self.core, &inst).await?;
                    self.notify_changed(&inst).await;
                    return Ok(inst);
                }
            } else if approve {
                inst.confirmed_steps.push(step.id.clone());
                inst.vars.insert(format!("__actAs.{}", step.id), json!(user.id.to_string()));
                inst.vars.insert(format!("__actAsName.{}", step.id), json!(user.display_name));
                if !req.values.is_empty() {
                    let mut values = task.prefill.clone();
                    values.extend(req.values.clone());
                    inst.vars.insert(format!("__override.{}", step.id), Value::Object(values));
                }
                inst.push_history(&step.id, &step.title, ActorKind::Human, &who, "confirmed", json!({ "comment": comment }));
                inst.pending = None;
                inst.state = InstanceState::Running;
                store::save_instance(&self.core, &inst).await?;
            } else {
                inst.push_history(&step.id, &step.title, ActorKind::Human, &who, "rejected", json!({ "comment": comment }));
                inst.pending = None;
                inst.state = InstanceState::Cancelled;
                store::save_instance(&self.core, &inst).await?;
                self.notify_changed(&inst).await;
                return Ok(inst);
            }
            return self.advance(id).await;
        }

        let result = match step.kind {
            StepKind::Approval => {
                if !matches!(decision.as_str(), "approve" | "reject") {
                    return Err(Error::validation("decision.required", "workflow:decision.required").for_field("decision"));
                }
                if decision == "reject" && comment.trim().is_empty() {
                    return Err(Error::validation("comment.required", "workflow:comment.required").for_field("comment"));
                }
                json!({ "decision": decision, "comment": comment, "decidedBy": who })
            }
            StepKind::Choice => {
                if !task.options.iter().any(|o| o.value == decision) {
                    return Err(Error::validation("decision.invalid", "workflow:decision.invalid").for_field("decision"));
                }
                json!({ "decision": decision, "choice": decision, "comment": comment })
            }
            _ => {
                let mut values = match &task.input {
                    Some(s) => s.validate(&req.values)?,
                    None => req.values.clone(),
                };
                if !decision.is_empty() {
                    values.insert("decision".into(), json!(decision));
                }
                Value::Object(values)
            }
        };
        let action = if step.kind == StepKind::Approval { if decision == "approve" { "approved" } else { "rejected" } } else { "submitted" };
        self.finish_step(&def, &mut inst, &step, result, ActorKind::Human, &who, action).await?;
        self.advance(id).await
    }

    pub async fn cancel(&self, user: &RedHiveUser, id: Uuid) -> Res<WorkflowInstance> {
        user.require("workflows.run")?;
        let mut inst = store::load_instance(&self.core, id).await?;
        if inst.tenant_id != user.tenant_id {
            return Err(Error::not_found("workflow.instance", "workflow:instance.notFound"));
        }
        if inst.run_as != user.id && !user.has("workflows.edit") {
            return Err(Error::forbidden("workflow.cancel.notAllowed", "errors:forbidden"));
        }
        if inst.state.is_finished() {
            return Err(Error::conflict("workflow.instance.finished", "workflow:instance.finished"));
        }
        let step = inst.current_step_id.clone().unwrap_or_default();
        inst.push_history(&step, "", ActorKind::Human, &user.display_name, "cancelled", Value::Null);
        inst.state = InstanceState::Cancelled;
        inst.pending = None;
        store::save_instance(&self.core, &inst).await?;
        self.notify_changed(&inst).await;
        Ok(inst)
    }

    /// Resumes a paused, failed (retry) or retrying instance.
    pub async fn resume(&self, user: &RedHiveUser, id: Uuid) -> Res<WorkflowInstance> {
        user.require("workflows.edit")?;
        let mut inst = store::load_instance(&self.core, id).await?;
        if inst.tenant_id != user.tenant_id {
            return Err(Error::not_found("workflow.instance", "workflow:instance.notFound"));
        }
        match inst.state {
            InstanceState::Paused | InstanceState::Failed | InstanceState::Retrying => {
                // a paused instance may have had a pending task – keep waiting for it
                inst.state = if inst.pending.is_some() { if inst.pending.as_ref().is_some_and(|p| p.confirms_ai || p.kind == StepKind::Approval) { InstanceState::WaitingForApproval } else { InstanceState::WaitingForInput } } else { InstanceState::Running };
                inst.attempts = 0;
                inst.error = None;
                let step = inst.current_step_id.clone().unwrap_or_default();
                inst.push_history(&step, "", ActorKind::Human, &user.display_name, "resumed", Value::Null);
                store::save_instance(&self.core, &inst).await?;
                self.advance(id).await
            }
            _ => Err(Error::conflict("workflow.instance.notPaused", "workflow:instance.notPaused")),
        }
    }

    /// Resolves an awaited event for a waiting instance.
    pub async fn deliver_event(&self, inst_id: Uuid, payload: &Value, timed_out: bool) -> Res<()> {
        let mut inst = store::load_instance(&self.core, inst_id).await?;
        if inst.state != InstanceState::WaitingForEvent || inst.vars.contains_key("__waitChild") {
            return Ok(());
        }
        let def = store::version(&self.core, inst.workflow_id, inst.workflow_version).await?;
        let step = inst.current_step_id.as_ref().and_then(|s| def.step(s)).cloned().ok_or_else(|| Error::not_found("workflow.step.missing", "workflow:step.missing"))?;
        if !timed_out {
            if let Some(f) = step.event_filter.as_deref().filter(|f| !f.trim().is_empty()) {
                let sc = scope(&inst).with("payload", payload.clone());
                if !expr::condition(f, &sc).unwrap_or(false) {
                    return Ok(());
                }
            }
        }
        let result = if timed_out { json!({ "timedOut": true }) } else { json!({ "timedOut": false, "payload": payload }) };
        self.finish_step(&def, &mut inst, &step, result, ActorKind::Engine, "workflow-engine", if timed_out { "timedOut" } else { "eventReceived" }).await?;
        self.advance(inst.id).await.map(|_| ())
    }

    /// Pauses all running instances of the given workflows (impact analysis).
    pub async fn pause_instances(&self, workflow_id: Uuid, reason: &str) -> Res<usize> {
        let list = store::instances_where(&self.core, &format!("workflow_id = '{workflow_id}' and state not in ('completed', 'failed', 'cancelled', 'paused')"), None, 1000).await?;
        let n = list.len();
        for mut i in list {
            let step = i.current_step_id.clone().unwrap_or_default();
            i.push_history(&step, "", ActorKind::Engine, "workflow-engine", "paused", json!({ "reason": reason }));
            i.state = InstanceState::Paused;
            store::save_instance(&self.core, &i).await?;
        }
        Ok(n)
    }
}
