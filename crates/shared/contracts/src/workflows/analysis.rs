//! Definition validation and dependency extraction.

use super::definition::*;
use super::expr;
use crate::modules::ModuleManifest;
use redhive_monads::{Error, Res};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum IssueSeverity {
    Error,
    Warning,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Issue {
    pub severity: IssueSeverity,
    pub code: String,
    #[serde(default)]
    pub step_id: Option<String>,
    pub message: String,
}

fn err(code: &str, step: Option<&str>, message: impl Into<String>) -> Issue {
    Issue { severity: IssueSeverity::Error, code: code.into(), step_id: step.map(str::to_string), message: message.into() }
}
fn warn(code: &str, step: Option<&str>, message: impl Into<String>) -> Issue {
    Issue { severity: IssueSeverity::Warning, code: code.into(), step_id: step.map(str::to_string), message: message.into() }
}

/// Every expression of a definition with where it is used.
pub fn expressions(def: &WorkflowDefinition) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for t in &def.transitions {
        if let Some(c) = t.condition.as_ref().filter(|c| !c.trim().is_empty()) {
            out.push((format!("transition {}→{}", t.from, t.to), c.clone()));
        }
    }
    for s in &def.steps {
        for (k, v) in &s.parameters {
            out.push((format!("step {} parameter {}", s.id, k), v.clone()));
        }
        for (k, v) in &s.output_mapping {
            out.push((format!("step {} output {}", s.id, k), v.clone()));
        }
        if let Some(f) = s.event_filter.as_ref().filter(|f| !f.trim().is_empty()) {
            out.push((format!("step {} event filter", s.id), f.clone()));
        }
    }
    for t in &def.triggers {
        if let Some(f) = t.filter.as_ref().filter(|f| !f.trim().is_empty()) {
            out.push((format!("trigger {} filter", t.id), f.clone()));
        }
        if let Some(d) = t.deduplication_key.as_ref().filter(|f| !f.trim().is_empty()) {
            out.push((format!("trigger {} deduplication key", t.id), d.clone()));
        }
        for (k, v) in &t.payload_mapping {
            out.push((format!("trigger {} mapping {}", t.id, k), v.clone()));
        }
    }
    out
}

/// Validates a schedule spec: `every:<minutes>` or `daily:HH:MM`.
pub fn parse_schedule(spec: &str) -> Option<Schedule> {
    if let Some(m) = spec.strip_prefix("every:") {
        return m.trim().parse::<u32>().ok().filter(|m| *m >= 1).map(Schedule::Every);
    }
    if let Some(t) = spec.strip_prefix("daily:") {
        let (h, m) = t.trim().split_once(':')?;
        let (h, m) = (h.parse::<u32>().ok()?, m.parse::<u32>().ok()?);
        return (h < 24 && m < 60).then_some(Schedule::Daily(h, m));
    }
    None
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Schedule {
    Every(u32),
    Daily(u32, u32),
}

/// Checks the structure of a definition. Errors block saving; warnings are shown in the designer.
pub fn validate(def: &WorkflowDefinition) -> Vec<Issue> {
    let mut issues = Vec::new();
    if def.name.trim().is_empty() {
        issues.push(err("name.missing", None, "The workflow needs a name."));
    }
    if def.steps.is_empty() {
        issues.push(err("steps.empty", None, "The workflow has no steps."));
        return issues;
    }
    let mut ids = HashSet::new();
    for s in &def.steps {
        if s.id.trim().is_empty() || s.id == END {
            issues.push(err("step.id.invalid", Some(&s.id), format!("'{}' is not a valid step id.", s.id)));
        }
        if !ids.insert(s.id.as_str()) {
            issues.push(err("step.id.duplicate", Some(&s.id), format!("Step id '{}' is used twice.", s.id)));
        }
        match s.kind {
            StepKind::ModuleStep if s.step_type_id.as_deref().unwrap_or("").is_empty() => issues.push(err("step.stepType.missing", Some(&s.id), "A module step needs a step type.")),
            StepKind::Command if s.command_id.as_deref().unwrap_or("").is_empty() => issues.push(err("step.command.missing", Some(&s.id), "A command step needs a command.")),
            StepKind::Input | StepKind::Review if s.input.as_ref().is_none_or(|i| i.fields.is_empty()) => {
                issues.push(err("step.input.missing", Some(&s.id), "An input step needs a form with at least one field."))
            }
            StepKind::Choice if s.options.len() < 2 => issues.push(err("step.options.missing", Some(&s.id), "A choice step needs at least two options.")),
            StepKind::WaitForEvent if s.event_type.as_deref().unwrap_or("").is_empty() => issues.push(err("step.event.missing", Some(&s.id), "A wait step needs an event type.")),
            StepKind::SubWorkflow if s.workflow_id.is_none() => issues.push(err("step.workflow.missing", Some(&s.id), "A sub-workflow step needs a workflow.")),
            StepKind::SubWorkflow if s.workflow_id == Some(def.id) => issues.push(err("step.workflow.recursive", Some(&s.id), "A workflow cannot call itself.")),
            StepKind::AiTask if s.prompt.as_deref().unwrap_or("").is_empty() => issues.push(err("step.prompt.missing", Some(&s.id), "An AI task needs an instruction.")),
            StepKind::Approval if s.approvers.is_empty() && def.ai_policy.approvers.is_empty() => {
                issues.push(warn("step.approvers.missing", Some(&s.id), "No approver permission – the RunAs user approves."))
            }
            _ => {}
        }
        if s.kind == StepKind::Review && !s.tools.is_empty() && def.ai_policy.autonomy == Autonomy::Off {
            issues.push(warn("step.tools.unused", Some(&s.id), "Tools are only used by the AI agent, which is off for this workflow."));
        }
    }
    if def.step(&def.start_step_id).is_none() {
        issues.push(err("start.missing", None, format!("Start step '{}' does not exist.", def.start_step_id)));
    }
    for t in &def.transitions {
        if def.step(&t.from).is_none() {
            issues.push(err("transition.from", Some(&t.from), format!("Transition from unknown step '{}'.", t.from)));
        }
        if t.to != END && def.step(&t.to).is_none() {
            issues.push(err("transition.to", Some(&t.from), format!("Transition to unknown step '{}'.", t.to)));
        }
    }
    for (where_, src) in expressions(def) {
        if let Err(e) = expr::validate(&src) {
            issues.push(err("expression.invalid", None, format!("{where_}: {} ({})", e.message.unwrap_or_default(), src)));
        }
    }
    for t in &def.triggers {
        match t.kind {
            TriggerKind::Event if t.event_type.as_deref().unwrap_or("").is_empty() => issues.push(err("trigger.event.missing", None, format!("Trigger '{}' needs an event type.", t.id))),
            TriggerKind::Command if t.command_id.as_deref().unwrap_or("").is_empty() => issues.push(err("trigger.command.missing", None, format!("Trigger '{}' needs a command.", t.id))),
            TriggerKind::Schedule if t.schedule.as_deref().and_then(parse_schedule).is_none() => {
                issues.push(err("trigger.schedule.invalid", None, format!("Trigger '{}': use 'every:<minutes>' or 'daily:HH:MM'.", t.id)))
            }
            TriggerKind::Condition if t.condition_url.as_deref().unwrap_or("").is_empty() => issues.push(err("trigger.condition.missing", None, format!("Trigger '{}' needs a condition URL.", t.id))),
            _ => {}
        }
        if t.executor == WorkflowExecutor::Ai && def.ai_policy.autonomy == Autonomy::Off {
            issues.push(err("trigger.ai.off", None, format!("Trigger '{}' hands the workflow to the AI, but the AI policy is 'off'.", t.id)));
        }
    }
    if !(0.0..=1.0).contains(&def.ai_policy.required_confidence) {
        issues.push(err("ai.confidence.range", None, "Required confidence must be between 0 and 1."));
    }

    // Reachability: every step should be reachable from the start, and every non-end path should lead somewhere.
    let mut reachable = BTreeSet::new();
    let mut stack = vec![def.start_step_id.clone()];
    while let Some(id) = stack.pop() {
        if id == END || !reachable.insert(id.clone()) {
            continue;
        }
        for t in def.transitions_from(&id) {
            stack.push(t.to.clone());
        }
    }
    for s in &def.steps {
        if !reachable.contains(&s.id) {
            issues.push(warn("step.unreachable", Some(&s.id), format!("Step '{}' cannot be reached.", s.id)));
        } else if def.transitions_from(&s.id).is_empty() {
            issues.push(warn("step.deadEnd", Some(&s.id), format!("Step '{}' has no outgoing transition; the workflow ends after it.", s.id)));
        } else if def.transitions_from(&s.id).iter().all(|t| t.condition.as_deref().is_some_and(|c| !c.trim().is_empty())) {
            issues.push(warn("step.noDefault", Some(&s.id), format!("All transitions of '{}' have conditions; add one without a condition as fallback.", s.id)));
        }
    }
    issues
}

/// Fails with all errors (warnings are ignored).
pub fn ensure_valid(def: &WorkflowDefinition) -> Res<()> {
    let errors: Vec<_> = validate(def).into_iter().filter(|i| i.severity == IssueSeverity::Error).collect();
    if errors.is_empty() {
        return Ok(());
    }
    let mut e = Error::validation("workflow.invalid", "workflow:definition.invalid")
        .with_message(errors.iter().map(|i| i.message.clone()).collect::<Vec<_>>().join(" "));
    e.metadata.insert("issues".into(), serde_json::to_value(&errors).unwrap_or_default());
    Err(e)
}

/// Finds the module that owns an element id (by contract surface first, then by prefix).
pub fn owning_module<'a>(catalog: &'a [ModuleManifest], kind: DependencyKind, id: &str) -> Option<&'a ModuleManifest> {
    catalog
        .iter()
        .find(|m| {
            let s = &m.contract_surface;
            match kind {
                DependencyKind::StepType => s.step_types.iter().any(|x| x.id == id),
                DependencyKind::InputSchema => s.input_schemas.iter().any(|x| x.schema_id == id) || s.step_types.iter().any(|x| x.input.schema_id == id),
                DependencyKind::Command => s.commands.iter().any(|x| x.id == id),
                DependencyKind::Event => s.events.iter().any(|x| x.event_type == id),
                _ => false,
            }
        })
        .or_else(|| {
            let prefix = id.split('.').next().unwrap_or("");
            catalog.iter().find(|m| m.prefix == prefix)
        })
}

/// Extracts what a definition depends on. `catalog` maps ids to modules (and their versions).
pub fn dependencies(def: &WorkflowDefinition, catalog: &[ModuleManifest]) -> Vec<WorkflowDependency> {
    let mut deps: BTreeMap<(DependencyKind, String), (String, BTreeSet<String>)> = BTreeMap::new();
    let mut add = |kind: DependencyKind, id: &str, version: String, used_by: &str| {
        let e = deps.entry((kind, id.to_string())).or_insert_with(|| (version, BTreeSet::new()));
        e.1.insert(used_by.to_string());
    };
    for s in &def.steps {
        if let Some(st) = &s.step_type_id {
            add(DependencyKind::StepType, st, s.step_type_version.to_string(), &s.id);
            if let Some(m) = owning_module(catalog, DependencyKind::StepType, st) {
                add(DependencyKind::Module, &m.module_id, format!("^{}", m.version), &s.id);
            }
        }
        if let Some(c) = &s.command_id {
            add(DependencyKind::Command, c, "1".into(), &s.id);
            if let Some(m) = owning_module(catalog, DependencyKind::Command, c) {
                add(DependencyKind::Module, &m.module_id, format!("^{}", m.version), &s.id);
            }
        }
        if let Some(i) = &s.input {
            if catalog.iter().any(|m| m.contract_surface.input_schemas.iter().any(|x| x.schema_id == i.schema_id)) {
                add(DependencyKind::InputSchema, &i.schema_id, i.version.to_string(), &s.id);
                if let Some(m) = owning_module(catalog, DependencyKind::InputSchema, &i.schema_id) {
                    add(DependencyKind::Module, &m.module_id, format!("^{}", m.version), &s.id);
                }
            }
        }
        for tool in &s.tools {
            add(DependencyKind::StepType, tool, "1".into(), &s.id);
        }
        if let Some(ev) = &s.event_type {
            add(DependencyKind::Event, ev, "1".into(), &s.id);
        }
        if let Some(w) = s.workflow_id {
            add(DependencyKind::Workflow, &w.to_string(), String::new(), &s.id);
        }
    }
    for t in &def.triggers {
        let used = format!("trigger:{}", t.id);
        if let Some(ev) = &t.event_type {
            let version = catalog
                .iter()
                .flat_map(|m| m.contract_surface.events.iter())
                .find(|e| &e.event_type == ev)
                .map(|e| e.payload_version.to_string())
                .unwrap_or_else(|| "1".into());
            add(DependencyKind::Event, ev, version, &used);
            if let Some(m) = owning_module(catalog, DependencyKind::Event, ev) {
                add(DependencyKind::Module, &m.module_id, format!("^{}", m.version), &used);
            }
        }
        if let Some(c) = &t.command_id {
            add(DependencyKind::Command, c, "1".into(), &used);
        }
    }
    deps.into_iter()
        .map(|((kind, id), (version, used))| WorkflowDependency { kind, id, version, used_by_steps: used.into_iter().collect() })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    pub(crate) fn invoice_workflow() -> WorkflowDefinition {
        serde_json::from_value(json!({
            "name": "Invoice approval",
            "startStepId": "lookup",
            "steps": [
                {"id": "lookup", "kind": "moduleStep", "title": "Load", "stepTypeId": "documents.lookupInvoice", "parameters": {"invoiceId": "vars.invoiceId"}},
                {"id": "review", "kind": "review", "title": "Review", "tools": ["documents.lookupInvoice"],
                 "input": {"schemaId": "documents.invoiceReview", "fields": [{"name": "recommendation", "type": "choice", "required": true,
                    "options": [{"value": "approve", "label": "A"}, {"value": "reject", "label": "R"}]}]}},
                {"id": "approve", "kind": "moduleStep", "title": "Approve", "critical": true, "stepTypeId": "documents.approveInvoice",
                 "parameters": {"invoiceId": "vars.invoiceId", "comment": "coalesce(vars.note, 'ok')"}}
            ],
            "transitions": [
                {"from": "lookup", "to": "review"},
                {"from": "review", "to": "approve", "condition": "vars.recommendation == 'approve'", "order": 1},
                {"from": "review", "to": "end", "order": 2},
                {"from": "approve", "to": "end"}
            ],
            "triggers": [{"id": "submitted", "kind": "event", "eventType": "documents.invoice.submitted", "payloadMapping": {"invoiceId": "payload.invoiceId"}, "executor": "ai"}],
            "aiPolicy": {"autonomy": "approveCritical"}
        }))
        .unwrap()
    }

    #[test]
    fn valid_workflow_has_no_errors() {
        let issues = validate(&invoice_workflow());
        assert!(issues.iter().all(|i| i.severity == IssueSeverity::Warning), "{issues:?}");
        ensure_valid(&invoice_workflow()).unwrap();
    }

    #[test]
    fn detects_errors() {
        let mut w = invoice_workflow();
        w.transitions.push(WorkflowTransition { from: "approve".into(), to: "nowhere".into(), condition: Some("vars.x = 1".into()), order: 0 });
        w.ai_policy.autonomy = Autonomy::Off;
        let codes: Vec<_> = validate(&w).into_iter().filter(|i| i.severity == IssueSeverity::Error).map(|i| i.code).collect();
        assert!(codes.contains(&"transition.to".to_string()));
        assert!(codes.contains(&"expression.invalid".to_string()));
        assert!(codes.contains(&"trigger.ai.off".to_string()));
    }

    #[test]
    fn schedules() {
        assert_eq!(parse_schedule("every:30"), Some(Schedule::Every(30)));
        assert_eq!(parse_schedule("daily:07:30"), Some(Schedule::Daily(7, 30)));
        assert_eq!(parse_schedule("daily:25:00"), None);
    }

    #[test]
    fn extracts_dependencies() {
        let deps = dependencies(&invoice_workflow(), &[]);
        assert!(deps.iter().any(|d| d.kind == DependencyKind::StepType && d.id == "documents.approveInvoice"));
        assert!(deps.iter().any(|d| d.kind == DependencyKind::Event && d.used_by_steps == vec!["trigger:submitted"]));
    }
}
