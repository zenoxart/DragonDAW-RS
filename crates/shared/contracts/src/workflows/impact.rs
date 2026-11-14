//! Impact of a module update on a workflow – classification and automatic migration.
//!
//! | Change | Result |
//! |---|---|
//! | Renames declared in `migrations` | **auto-migrated** (new workflow version, expressions rewritten) |
//! | New required input field, changed type, removed field/output that is used | **needs review** |
//! | Step type / command / event removed without a migration | **broken** |

use super::definition::*;
use super::expr;
use crate::modules::{ChangeKind, ChangeSeverity, ElementKind, SurfaceChange};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ImpactClass {
    Compatible,
    AutoMigrated,
    ReviewRequired,
    Broken,
}

impl ImpactClass {
    pub fn as_str(self) -> &'static str {
        match self {
            ImpactClass::Compatible => "compatible",
            ImpactClass::AutoMigrated => "autoMigrated",
            ImpactClass::ReviewRequired => "reviewRequired",
            ImpactClass::Broken => "broken",
        }
    }
}

/// What happened to one workflow.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowImpact {
    pub workflow_id: Uuid,
    pub workflow_name: String,
    pub from_version: i32,
    /// New version when the workflow was migrated.
    #[serde(default)]
    pub to_version: Option<i32>,
    pub class: ImpactClass,
    pub reasons: Vec<String>,
    /// Steps affected.
    pub steps: Vec<String>,
    /// What was done (`migrated to v3`, `AI triggers suspended`, `2 running instances paused`).
    #[serde(default)]
    pub actions: Vec<String>,
}

/// Impact report of one module update.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImpactReport {
    pub id: Uuid,
    pub module_id: String,
    pub from_version: String,
    pub to_version: String,
    pub analyzed_at: DateTime<Utc>,
    pub changes: Vec<SurfaceChange>,
    pub workflows: Vec<WorkflowImpact>,
}

impl ImpactReport {
    pub fn count(&self, class: ImpactClass) -> usize {
        self.workflows.iter().filter(|w| w.class == class).count()
    }
}

/// Result of analysing one workflow.
#[derive(Debug, Clone)]
pub struct Analysis {
    pub impact: WorkflowImpact,
    /// The migrated definition (only for `AutoMigrated`, and for `ReviewRequired` when renames were applied too).
    pub migrated: Option<WorkflowDefinition>,
}

fn uses(def: &WorkflowDefinition, element: ElementKind, id: &str) -> Vec<String> {
    let mut steps = Vec::new();
    for s in &def.steps {
        let hit = match element {
            ElementKind::StepType => s.step_type_id.as_deref() == Some(id) || s.tools.iter().any(|t| t == id),
            ElementKind::Command => s.command_id.as_deref() == Some(id),
            ElementKind::InputSchema => s.input.as_ref().is_some_and(|i| i.schema_id == id),
            ElementKind::Event => s.event_type.as_deref() == Some(id),
        };
        if hit {
            steps.push(s.id.clone());
        }
    }
    for t in &def.triggers {
        let hit = match element {
            ElementKind::Event => t.event_type.as_deref() == Some(id),
            ElementKind::Command => t.command_id.as_deref() == Some(id),
            _ => false,
        };
        if hit {
            steps.push(format!("trigger:{}", t.id));
        }
    }
    steps
}

/// Whether the workflow reads `field` produced by step `step_id` (`vars.f`, `steps.<id>.f`, `last.f` or parameters).
fn field_used(def: &WorkflowDefinition, step_id: &str, field: &str) -> bool {
    let candidates = [format!("vars.{field}"), format!("steps.{step_id}.{field}"), format!("last.{field}")];
    let in_expr = super::analysis::expressions(def).iter().any(|(_, src)| {
        expr::parse(src).map(|e| {
            let paths = e.paths();
            candidates.iter().any(|c| paths.iter().any(|p| p == c || p.starts_with(&format!("{c}."))))
        }).unwrap_or(false)
    });
    let in_params = def.step(step_id).is_some_and(|s| s.parameters.contains_key(field));
    in_expr || in_params
}

/// Classifies `def` against the changes of one module and migrates it where possible.
pub fn analyze(def: &WorkflowDefinition, changes: &[SurfaceChange]) -> Analysis {
    let mut class = ImpactClass::Compatible;
    let mut reasons = Vec::new();
    let mut affected_steps: Vec<String> = Vec::new();
    let mut migrated = def.clone();
    let mut did_migrate = false;

    for c in changes {
        let used_by = uses(def, c.element, &c.id);
        if used_by.is_empty() {
            continue;
        }
        let label = format!("{:?} '{}'", c.element, c.id);
        match (&c.kind, c.severity) {
            (ChangeKind::Removed, _) | (_, ChangeSeverity::Breaking) => {
                class = class.max(ImpactClass::Broken);
                reasons.push(format!("{label} was removed or is no longer usable ({})", c.details.join("; ")));
                affected_steps.extend(used_by);
            }
            (ChangeKind::Renamed { to }, sev) => {
                apply_rename(&mut migrated, c, to);
                did_migrate = true;
                class = class.max(ImpactClass::AutoMigrated);
                reasons.push(format!("{label} renamed to '{to}' – migrated"));
                affected_steps.extend(used_by.iter().cloned());
                if sev >= ChangeSeverity::ReviewRequired {
                    let needs = review_needed(&migrated, c, &used_by);
                    if !needs.is_empty() {
                        class = class.max(ImpactClass::ReviewRequired);
                        reasons.extend(needs);
                    }
                }
            }
            (_, ChangeSeverity::Migratable) => {
                // in-place field renames of a schema / step type
                if !c.field_renames.is_empty() || !c.output_renames.is_empty() {
                    let id = c.id.clone();
                    apply_rename(&mut migrated, c, &id);
                    did_migrate = true;
                    class = class.max(ImpactClass::AutoMigrated);
                    reasons.push(format!("{label}: fields renamed – migrated"));
                    affected_steps.extend(used_by);
                }
            }
            (_, ChangeSeverity::ReviewRequired) => {
                let needs = review_needed(def, c, &used_by);
                if !needs.is_empty() {
                    class = class.max(ImpactClass::ReviewRequired);
                    reasons.extend(needs);
                    affected_steps.extend(used_by);
                }
            }
            _ => {}
        }
    }

    affected_steps.sort();
    affected_steps.dedup();
    let migrated = if did_migrate && class != ImpactClass::Broken {
        migrated.version = def.version + 1;
        migrated.updated_at = Some(Utc::now());
        if class == ImpactClass::ReviewRequired {
            migrated.status = WorkflowStatus::NeedsReview;
            migrated.review_reasons = reasons.clone();
            suspend_ai_triggers(&mut migrated);
        }
        Some(migrated)
    } else {
        None
    };
    Analysis {
        impact: WorkflowImpact {
            workflow_id: def.id,
            workflow_name: def.name.clone(),
            from_version: def.version,
            to_version: migrated.as_ref().map(|m| m.version),
            class,
            reasons,
            steps: affected_steps,
            actions: Vec::new(),
        },
        migrated,
    }
}

/// Suspends AI triggers (used for workflows that need review).
pub fn suspend_ai_triggers(def: &mut WorkflowDefinition) -> usize {
    let mut n = 0;
    for t in def.triggers.iter_mut().filter(|t| t.executor == WorkflowExecutor::Ai && !t.suspended) {
        t.suspended = true;
        n += 1;
    }
    n
}

fn review_needed(def: &WorkflowDefinition, c: &SurfaceChange, used_by: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    for step_id in used_by {
        if let Some(trigger_id) = step_id.strip_prefix("trigger:") {
            // event payload changes: any filter/mapping reading a removed field, or unknown field list
            if c.element == ElementKind::Event {
                let t = def.triggers.iter().find(|t| t.id == trigger_id);
                let reads_removed = t.is_some_and(|t| {
                    let mut srcs: Vec<&String> = t.payload_mapping.values().collect();
                    srcs.extend(t.filter.iter());
                    srcs.extend(t.deduplication_key.iter());
                    srcs.iter().any(|src| expr::parse(src).map(|e| e.paths().iter().any(|p| c.affected_fields.is_empty() || c.affected_fields.iter().any(|f| p == &format!("payload.{f}")))).unwrap_or(false))
                });
                if reads_removed {
                    out.push(format!("trigger '{trigger_id}' reads the changed payload of '{}' ({})", c.id, c.details.join("; ")));
                }
            }
            if c.element == ElementKind::Command && c.affected_fields.iter().any(|f| f == "aiAllowed")
                && def.triggers.iter().any(|t| t.id == trigger_id && t.executor == WorkflowExecutor::Ai) {
                    out.push(format!("trigger '{trigger_id}': command '{}' is no longer allowed for the AI", c.id));
                }
            continue;
        }
        let Some(step) = def.step(step_id) else { continue };
        for f in &c.affected_fields {
            if let Some(new_required) = f.strip_prefix('+') {
                // new required input: fine when the step already supplies it
                let supplied = step.parameters.contains_key(new_required) || step.kind.is_interactive();
                if !supplied || step.kind == StepKind::Review && def.ai_policy.autonomy >= Autonomy::ApproveCritical {
                    out.push(format!("step '{}' does not supply the new required field '{new_required}' of '{}'", step.id, c.id));
                }
            } else if let Some(output) = f.strip_prefix("output:") {
                if field_used(def, &step.id, output) {
                    out.push(format!("step '{}' reads output '{output}' that '{}' no longer returns", step.id, c.id));
                }
            } else if f == "aiAllowed" {
                if def.triggers.iter().any(|t| t.executor == WorkflowExecutor::Ai) {
                    out.push(format!("step '{}': '{}' is no longer allowed for the AI", step.id, c.id));
                }
            } else if field_used(def, &step.id, f) || step.input.as_ref().is_some_and(|i| i.field(f).is_some()) {
                out.push(format!("step '{}' uses field '{f}' of '{}' ({})", step.id, c.id, c.details.join("; ")));
            }
        }
    }
    out
}

fn apply_rename(def: &mut WorkflowDefinition, c: &SurfaceChange, to: &str) {
    let old_id = c.id.clone();
    let step_ids: Vec<String> = def
        .steps
        .iter()
        .filter(|s| match c.element {
            ElementKind::StepType => s.step_type_id.as_deref() == Some(&old_id),
            ElementKind::Command => s.command_id.as_deref() == Some(&old_id),
            ElementKind::InputSchema => s.input.as_ref().is_some_and(|i| i.schema_id == old_id),
            ElementKind::Event => s.event_type.as_deref() == Some(&old_id),
        })
        .map(|s| s.id.clone())
        .collect();

    for s in def.steps.iter_mut() {
        match c.element {
            ElementKind::StepType => {
                if s.step_type_id.as_deref() == Some(&old_id) {
                    s.step_type_id = Some(to.to_string());
                    for (from, to_f) in &c.field_renames {
                        if let Some(v) = s.parameters.remove(from) {
                            s.parameters.insert(to_f.clone(), v);
                        }
                    }
                }
                for t in s.tools.iter_mut().filter(|t| **t == old_id) {
                    *t = to.to_string();
                }
            }
            ElementKind::Command if s.command_id.as_deref() == Some(&old_id) => {
                s.command_id = Some(to.to_string());
                for (from, to_f) in &c.field_renames {
                    if let Some(v) = s.parameters.remove(from) {
                        s.parameters.insert(to_f.clone(), v);
                    }
                }
            }
            ElementKind::InputSchema => {
                if let Some(i) = s.input.as_mut().filter(|i| i.schema_id == old_id) {
                    i.schema_id = to.to_string();
                    for f in i.fields.iter_mut() {
                        if let Some(n) = c.field_renames.get(&f.name) {
                            f.name = n.clone();
                        }
                    }
                    for (from, to_f) in &c.field_renames {
                        if let Some(v) = s.parameters.remove(from) {
                            s.parameters.insert(to_f.clone(), v);
                        }
                    }
                }
            }
            ElementKind::Event if s.event_type.as_deref() == Some(&old_id) => s.event_type = Some(to.to_string()),
            _ => {}
        }
    }

    // Rewrite expressions: input field renames show up as vars.<f> / last.<f> / steps.<id>.<f>;
    // output renames of step types as steps.<id>.<out> (and vars.<out> when outputs are copied).
    let mut renames: Vec<(String, String)> = c.field_renames.iter().map(|(a, b)| (a.clone(), b.clone())).collect();
    if c.element == ElementKind::StepType {
        renames = c.output_renames.iter().map(|(a, b)| (a.clone(), b.clone())).collect();
    }
    let rewrite = |src: &str| -> String {
        let mut out = src.to_string();
        for (from, to_f) in &renames {
            if c.element != ElementKind::Event {
                out = expr::rename_in(&out, "vars", &[], from, to_f);
                out = expr::rename_in(&out, "last", &[], from, to_f);
                for sid in &step_ids {
                    out = expr::rename_in(&out, "steps", &[sid.as_str()], from, to_f);
                }
            }
        }
        out
    };
    for t in def.transitions.iter_mut() {
        if let Some(cond) = t.condition.as_mut() {
            *cond = rewrite(cond);
        }
    }
    for s in def.steps.iter_mut() {
        for v in s.parameters.values_mut() {
            *v = rewrite(v);
        }
        for v in s.output_mapping.values_mut() {
            *v = rewrite(v);
        }
    }
    for t in def.triggers.iter_mut() {
        if c.element == ElementKind::Event && t.event_type.as_deref() == Some(&old_id) {
            t.event_type = Some(to.to_string());
            let payload_rewrite = |src: &str| -> String {
                let mut out = src.to_string();
                for (from, to_f) in &c.field_renames {
                    out = expr::rename_in(&out, "payload", &[], from, to_f);
                }
                out
            };
            for v in t.payload_mapping.values_mut() {
                *v = payload_rewrite(v);
            }
            if let Some(f) = t.filter.as_mut() {
                *f = payload_rewrite(f);
            }
            if let Some(d) = t.deduplication_key.as_mut() {
                *d = payload_rewrite(d);
            }
        }
        if c.element == ElementKind::Command && t.command_id.as_deref() == Some(&old_id) {
            t.command_id = Some(to.to_string());
        }
    }
    for d in def.dependencies.iter_mut() {
        if d.id == old_id {
            d.id = to.to_string();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::{diff_surfaces, ContractMigration, ContractSurface, EventContract, StepTypeContract};
    use crate::workflows::input::{FieldType, InputField, InputSchema};
    use serde_json::json;
    use std::collections::BTreeMap;

    fn wf() -> WorkflowDefinition {
        serde_json::from_value(json!({
            "name": "Reorder", "version": 2, "startStepId": "order",
            "steps": [
                {"id": "order", "kind": "moduleStep", "title": "Order", "stepTypeId": "inv.reorder", "parameters": {"qty": "vars.qty * 2"}},
                {"id": "notify", "kind": "moduleStep", "title": "Notify", "stepTypeId": "inv.notify", "parameters": {"text": "steps.order.orderNo"}}
            ],
            "transitions": [{"from": "order", "to": "notify", "condition": "steps.order.orderNo != null"}, {"from": "notify", "to": "end"}],
            "triggers": [{"id": "low", "kind": "event", "eventType": "inv.low", "payloadMapping": {"qty": "payload.qty"}, "executor": "ai"}],
            "aiPolicy": {"autonomy": "autonomous"}
        }))
        .unwrap()
    }

    fn step(id: &str, fields: &[(&str, bool)], outputs: &[&str]) -> StepTypeContract {
        StepTypeContract {
            id: id.into(),
            version: 1,
            title: id.into(),
            input: InputSchema { schema_id: id.into(), version: 1, fields: fields.iter().map(|(n, r)| InputField { name: n.to_string(), field_type: FieldType::Number, required: *r, ..Default::default() }).collect() },
            outputs: outputs.iter().map(|s| s.to_string()).collect(),
            headless: true,
            endpoint: String::new(),
        }
    }

    fn old_surface() -> ContractSurface {
        ContractSurface {
            step_types: vec![step("inv.reorder", &[("qty", true)], &["orderNo"]), step("inv.notify", &[("text", false)], &[])],
            events: vec![EventContract { event_type: "inv.low".into(), payload_version: 1, fields: vec!["qty".into()] }],
            ..Default::default()
        }
    }

    #[test]
    fn rename_is_migrated() {
        let mut new = old_surface();
        new.step_types[0] = step("inv.placeOrder", &[("amount", true)], &["orderNumber"]);
        new.migrations.push(ContractMigration {
            kind: ElementKind::StepType,
            from: "inv.reorder".into(),
            to: "inv.placeOrder".into(),
            field_renames: BTreeMap::from([("qty".into(), "amount".into())]),
            output_renames: BTreeMap::from([("orderNo".into(), "orderNumber".into())]),
        });
        let changes = diff_surfaces(&old_surface(), &new);
        let a = analyze(&wf(), &changes);
        assert_eq!(a.impact.class, ImpactClass::AutoMigrated, "{:?}", a.impact);
        let m = a.migrated.unwrap();
        assert_eq!(m.version, 3);
        let order = m.step("order").unwrap();
        assert_eq!(order.step_type_id.as_deref(), Some("inv.placeOrder"));
        assert!(order.parameters.contains_key("amount"));
        assert_eq!(m.step("notify").unwrap().parameters["text"], "steps.order.orderNumber");
        assert_eq!(m.transitions[0].condition.as_deref(), Some("steps.order.orderNumber != null"));
    }

    #[test]
    fn removal_breaks_and_required_field_needs_review() {
        let mut new = old_surface();
        new.step_types.remove(1);
        let a = analyze(&wf(), &diff_surfaces(&old_surface(), &new));
        assert_eq!(a.impact.class, ImpactClass::Broken);
        assert!(a.migrated.is_none());

        let mut new = old_surface();
        new.step_types[1] = step("inv.notify", &[("text", false), ("channel", true)], &[]);
        let a = analyze(&wf(), &diff_surfaces(&old_surface(), &new));
        assert_eq!(a.impact.class, ImpactClass::ReviewRequired, "{:?}", a.impact);
    }

    #[test]
    fn unrelated_change_is_compatible() {
        let mut new = old_surface();
        new.step_types.push(step("inv.extra", &[], &[]));
        let a = analyze(&wf(), &diff_surfaces(&old_surface(), &new));
        assert_eq!(a.impact.class, ImpactClass::Compatible);
    }

    #[test]
    fn event_payload_change_needs_review() {
        let mut new = old_surface();
        new.events[0] = EventContract { event_type: "inv.low".into(), payload_version: 2, fields: vec!["quantity".into()] };
        let a = analyze(&wf(), &diff_surfaces(&old_surface(), &new));
        assert_eq!(a.impact.class, ImpactClass::ReviewRequired);
    }
}
