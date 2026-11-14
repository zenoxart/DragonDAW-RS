//! Workflow UI: inbox (with AI proposals and confirmations), workflow list, designer (JSON + validation +
//! building blocks), recorder, runs, impact overview and the AI agent panel (status, kill switch, audit).

use crate::context::AppContext;
use crate::runtime::{poll, Promise};
use crate::widgets::form::{form_ui, FormState};
use crate::widgets::format::{format_value, text};
use crate::widgets::{self, badge};
use redhive_contracts::api::{AiAuditEntry, AiStatus, AiSwitchRequest};
use redhive_contracts::notifications::Severity;
use redhive_contracts::viewpack::ValueFormat;
use redhive_contracts::workflows::analysis::IssueSeverity;
use redhive_contracts::workflows::api::*;
use redhive_contracts::workflows::impact::{ImpactClass, ImpactReport};
use redhive_contracts::workflows::*;
use redhive_monads::{Error, Res};
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use uuid::Uuid;

#[derive(Default)]
pub struct InboxState {
    load: Option<Promise<Res<Vec<InboxItem>>>>,
    pub items: Vec<InboxItem>,
    pub selected: Option<Uuid>,
    forms: HashMap<Uuid, FormState>,
    comment: String,
    detail: Option<Promise<Res<WorkflowInstance>>>,
    instance: Option<WorkflowInstance>,
    submitting: Option<Promise<Res<WorkflowInstance>>>,
    pub dirty: bool,
    error: Option<Error>,
}

#[derive(Default)]
pub struct ListState {
    load: Option<Promise<Res<Vec<WorkflowSummary>>>>,
    items: Vec<WorkflowSummary>,
    action: Option<Promise<Res<Value>>>,
    pub dirty: bool,
}

#[derive(Default)]
pub struct DesignerState {
    pub json: String,
    issues: Vec<redhive_contracts::workflows::analysis::Issue>,
    validating: Option<Promise<Res<ValidateWorkflowResponse>>>,
    saving: Option<Promise<Res<SaveWorkflowResponse>>>,
    catalog: Option<Promise<Res<Value>>>,
    catalog_data: Option<Value>,
    loading: Option<Promise<Res<WorkflowDefinition>>>,
    error: Option<Error>,
    message: Option<String>,
}

#[derive(Default)]
pub struct InstancesState {
    load: Option<Promise<Res<Vec<InstanceSummary>>>>,
    items: Vec<InstanceSummary>,
    selected: Option<Uuid>,
    detail: Option<Promise<Res<WorkflowInstance>>>,
    instance: Option<WorkflowInstance>,
    action: Option<Promise<Res<WorkflowInstance>>>,
    filter: String,
    pub dirty: bool,
}

#[derive(Default)]
pub struct ImpactState {
    load: Option<Promise<Res<Vec<ImpactReport>>>>,
    reports: Vec<ImpactReport>,
    pub selected: Option<Uuid>,
    pub dirty: bool,
}

#[derive(Default)]
pub struct AiState {
    status: Option<Promise<Res<AiStatus>>>,
    current: Option<AiStatus>,
    audit: Option<Promise<Res<Vec<AiAuditEntry>>>>,
    entries: Vec<AiAuditEntry>,
    switching: Option<Promise<Res<AiStatus>>>,
    pub dirty: bool,
}

/// The workflow recorder: executed workflow-capable commands become command steps of a draft.
#[derive(Default)]
pub struct Recorder {
    pub active: bool,
    pub steps: Vec<(String, Value)>,
}

impl Recorder {
    pub fn record(&mut self, command_id: &str, parameter: Value) {
        if self.active {
            self.steps.push((command_id.to_string(), parameter));
        }
    }

    /// Builds a draft definition from the recorded commands.
    pub fn finish(&mut self, name: &str) -> WorkflowDefinition {
        self.active = false;
        let steps: Vec<WorkflowStep> = self
            .steps
            .iter()
            .enumerate()
            .map(|(i, (cmd, param))| WorkflowStep {
                id: format!("step{}", i + 1),
                kind: StepKind::Command,
                title: cmd.clone(),
                command_id: Some(cmd.clone()),
                parameters: param.as_object().map(|o| o.iter().map(|(k, v)| (k.clone(), literal(v))).collect()).unwrap_or_default(),
                max_retries: 2,
                step_type_version: 1,
                ..Default::default()
            })
            .collect();
        let transitions = steps.iter().enumerate().map(|(i, s)| WorkflowTransition { from: s.id.clone(), to: steps.get(i + 1).map(|n| n.id.clone()).unwrap_or_else(|| END.to_string()), condition: None, order: 0 }).collect();
        self.steps.clear();
        WorkflowDefinition {
            id: Uuid::new_v4(),
            version: 1,
            name: name.to_string(),
            description: "Recorded".into(),
            origin: WorkflowOrigin::Recorded,
            owning_module: None,
            start_step_id: steps.first().map(|s| s.id.clone()).unwrap_or_default(),
            steps,
            transitions,
            variables: vec![],
            triggers: vec![WorkflowTrigger { id: "manual".into(), kind: TriggerKind::Manual, ..serde_json::from_value(json!({"kind": "manual"})).unwrap_or_default() }],
            ai_policy: AiExecutionPolicy::default(),
            dependencies: vec![],
            status: WorkflowStatus::Draft,
            owner_id: None,
            updated_at: None,
            review_reasons: vec![],
        }
    }
}

/// A JSON value as an expression literal.
fn literal(v: &Value) -> String {
    match v {
        Value::String(s) => format!("'{}'", s.replace('\\', "\\\\").replace('\'', "\\'")),
        Value::Null => "null".into(),
        other => other.to_string(),
    }
}

#[derive(Default)]
pub struct WorkflowsUi {
    pub inbox: InboxState,
    pub list: ListState,
    pub designer: DesignerState,
    pub instances: InstancesState,
    pub impact: ImpactState,
    pub ai: AiState,
    pub recorder: Recorder,
}

fn german(ctx: &AppContext) -> bool {
    ctx.lang.language().starts_with("de")
}

fn state_badge(ui: &mut egui::Ui, s: InstanceState) {
    badge(ui, s.as_str(), s.as_str());
}

impl WorkflowsUi {
    /// Marks everything for reload (after workflow events).
    pub fn invalidate(&mut self) {
        self.inbox.dirty = true;
        self.list.dirty = true;
        self.instances.dirty = true;
    }

    pub fn inbox_count(&self) -> usize {
        self.inbox.items.len()
    }

    /// Loads the inbox in the background (badge in the status bar and the wheel).
    pub fn refresh_inbox(&mut self, ctx: &AppContext) {
        if self.inbox.load.is_none() && ctx.has("workflows.run") {
            let api = ctx.api.clone();
            self.inbox.load = Some(Promise::spawn(async move { api.get::<Vec<InboxItem>>("/api/workflows/inbox").await }));
            self.inbox.dirty = false;
        }
    }

    pub fn poll(&mut self, ctx: &mut AppContext) {
        if let Some(r) = poll(&mut self.inbox.load) {
            match r {
                Ok(items) => {
                    self.inbox.items = items;
                    self.inbox.error = None;
                }
                Err(e) => self.inbox.error = Some(e),
            }
        }
        if self.inbox.dirty {
            self.refresh_inbox(ctx);
        }
    }

    // ------------------------------------------------------------------ inbox

    pub fn inbox_ui(&mut self, ui: &mut egui::Ui, ctx: &mut AppContext, preselect: Option<String>) {
        if let Some(p) = preselect.and_then(|p| Uuid::parse_str(&p).ok()) {
            if self.inbox.selected != Some(p) && self.inbox.items.iter().any(|i| i.instance_id == p) {
                self.select(ctx, p);
            }
        }
        ui.horizontal(|ui| {
            ui.heading(ctx.icons.label("inbox", &ctx.t("workflow:inbox.title")));
            if ui.button(ctx.icons.glyph("refresh")).on_hover_text(ctx.t("shell:common.refresh")).clicked() {
                self.inbox.dirty = true;
            }
            if self.inbox.load.is_some() {
                ui.spinner();
            }
        });
        if let Some(e) = &self.inbox.error {
            widgets::error_box(ui, ctx, e);
        }
        if let Some(r) = poll(&mut self.inbox.submitting) {
            match r {
                Ok(inst) => {
                    ctx.toast(format!("{} – {}", inst.workflow_name, inst.state.as_str()), Severity::Success);
                    self.inbox.selected = None;
                    self.inbox.instance = None;
                    self.inbox.forms.remove(&inst.id);
                    self.inbox.dirty = true;
                }
                Err(e) => match self.inbox.selected.and_then(|id| self.inbox.forms.get_mut(&id)) {
                    Some(f) => f.show_error(e, ctx),
                    None => ctx.toast_error(&e),
                },
            }
        }
        if let Some(Ok(i)) = poll(&mut self.inbox.detail) {
            self.inbox.instance = Some(i);
        }
        if self.inbox.items.is_empty() {
            ui.label(egui::RichText::new(ctx.t("workflow:inbox.empty")).weak());
            return;
        }
        let narrow = ui.available_width() < 640.0;
        let items = self.inbox.items.clone();
        let mut clicked = None;
        let list = |ui: &mut egui::Ui, clicked: &mut Option<Uuid>, selected: Option<Uuid>| {
            egui::ScrollArea::vertical().id_salt("inbox-list").show(ui, |ui| {
                for it in &items {
                    let sel = selected == Some(it.instance_id);
                    let ai = if it.task.ai_proposal.is_some() || it.task.confirms_ai { "💡 " } else { "" };
                    let resp = ui.selectable_label(sel, format!("{ai}{}\n{}", it.task.title, it.workflow_name));
                    if resp.clicked() {
                        *clicked = Some(it.instance_id);
                    }
                }
            });
        };
        if narrow {
            if self.inbox.selected.is_none() {
                list(ui, &mut clicked, None);
            } else {
                if ui.button(format!("⏴ {}", ctx.t("shell:common.back"))).clicked() {
                    self.inbox.selected = None;
                }
                self.task_ui(ui, ctx);
            }
        } else {
            egui::SidePanel::left("inbox-side").resizable(true).default_width(260.0).show_inside(ui, |ui| list(ui, &mut clicked, self.inbox.selected));
            self.task_ui(ui, ctx);
        }
        if let Some(id) = clicked {
            self.select(ctx, id);
        }
    }

    fn select(&mut self, ctx: &AppContext, id: Uuid) {
        self.inbox.selected = Some(id);
        self.inbox.instance = None;
        self.inbox.comment.clear();
        let api = ctx.api.clone();
        self.inbox.detail = Some(Promise::spawn(async move { api.get::<WorkflowInstance>(&format!("/api/workflows/instances/{id}")).await }));
    }

    fn task_ui(&mut self, ui: &mut egui::Ui, ctx: &mut AppContext) {
        let Some(id) = self.inbox.selected else { return };
        let Some(item) = self.inbox.items.iter().find(|i| i.instance_id == id).cloned() else { return };
        let task = &item.task;
        egui::ScrollArea::vertical().id_salt("inbox-task").show(ui, |ui| {
            ui.heading(&task.title);
            let mut args = Map::new();
            args.insert("name".into(), json!(item.run_as_name));
            ui.label(egui::RichText::new(format!("{} · {}", item.workflow_name, ctx.lang.t_args("workflow:inbox.runAs", &args))).weak());
            if !task.description.is_empty() {
                ui.label(&task.description);
            }
            ui.add_space(6.0);
            if let Some(p) = &task.ai_proposal {
                widgets::card(ui, &format!("💡 {}", ctx.t("workflow:inbox.aiProposal")), |ui| {
                    let mut a = Map::new();
                    a.insert("value".into(), json!(format!("{:.0} %", p.confidence * 100.0)));
                    ui.horizontal(|ui| {
                        if let Some(d) = &p.decision {
                            badge(ui, d, d);
                        }
                        ui.label(ctx.lang.t_args("workflow:inbox.confidence", &a));
                        ui.label(egui::RichText::new(format!("({} · {})", p.provider, p.model)).small().weak());
                    });
                    ui.label(&p.reasoning);
                });
            }
            if task.confirms_ai {
                ui.label(egui::RichText::new(ctx.t("workflow:inbox.confirmAi")).italics());
                if task.input.is_none() && !task.prefill.is_empty() {
                    egui::Grid::new("confirm-values").num_columns(2).show(ui, |ui| {
                        for (k, v) in &task.prefill {
                            ui.label(egui::RichText::new(k).weak());
                            ui.label(text(v));
                            ui.end_row();
                        }
                    });
                }
            }
            let form = self.inbox.forms.entry(id).or_insert_with(|| match &task.input {
                Some(s) => FormState::new(s, &task.prefill),
                None => FormState::default(),
            });
            if let Some(schema) = &task.input {
                if !(task.confirms_ai && task.kind == StepKind::ModuleStep) {
                    form_ui(ui, ctx, schema, form, 2, &id.to_string());
                }
            }
            if task.kind == StepKind::Approval || task.confirms_ai || task.kind == StepKind::Choice {
                ui.label(ctx.t("workflow:field.result"));
                ui.add(egui::TextEdit::multiline(&mut self.inbox.comment).desired_rows(2).desired_width(f32::INFINITY).hint_text("…"));
            }
            ui.add_space(6.0);
            let busy = self.inbox.submitting.is_some();
            ui.horizontal(|ui| {
                let mut submit: Option<CompleteTaskRequest> = None;
                if task.kind == StepKind::Approval || task.confirms_ai || task.kind == StepKind::Choice {
                    let options = if task.options.is_empty() {
                        vec![("approve".to_string(), ctx.t("workflow:decision.approve")), ("reject".to_string(), ctx.t("workflow:decision.reject"))]
                    } else {
                        task.options.iter().map(|o| (o.value.clone(), ctx.t(&o.label))).collect()
                    };
                    for (value, label) in options {
                        if ui.add_enabled(!busy, egui::Button::new(label)).clicked() {
                            let values = match (&task.input, task.kind == StepKind::ModuleStep) {
                                (Some(schema), false) => form.collect(schema, ctx),
                                _ => Some(Map::new()),
                            };
                            if let Some(values) = values {
                                submit = Some(CompleteTaskRequest { values, decision: Some(value.clone()), comment: Some(self.inbox.comment.clone()).filter(|c| !c.is_empty()) });
                            }
                        }
                    }
                } else if ui.add_enabled(!busy, egui::Button::new(ctx.t("workflow:inbox.submit"))).clicked() {
                    if let Some(values) = task.input.as_ref().and_then(|s| form.collect(s, ctx)) {
                        submit = Some(CompleteTaskRequest { values, decision: None, comment: None });
                    }
                }
                if busy {
                    ui.spinner();
                }
                if let Some(req) = submit {
                    let api = ctx.api.clone();
                    self.inbox.submitting = Some(Promise::spawn(async move { api.post::<WorkflowInstance, _>(&format!("/api/workflows/instances/{id}/complete"), &req).await }));
                }
            });
            if let Some(inst) = &self.inbox.instance {
                ui.add_space(10.0);
                ui.collapsing(ctx.t("workflow:inbox.history"), |ui| history(ui, ctx, inst));
            }
        });
    }

    // ------------------------------------------------------------------ workflows list

    pub fn list_ui(&mut self, ui: &mut egui::Ui, ctx: &mut AppContext, open_designer: &mut Option<Uuid>) {
        if (self.list.dirty || self.list.items.is_empty()) && self.list.load.is_none() {
            let api = ctx.api.clone();
            self.list.load = Some(Promise::spawn(async move { api.get::<Vec<WorkflowSummary>>("/api/workflows").await }));
            self.list.dirty = false;
        }
        if let Some(r) = poll(&mut self.list.load) {
            match r {
                Ok(v) => self.list.items = v,
                Err(e) => ctx.toast_error(&e),
            }
        }
        if let Some(r) = poll(&mut self.list.action) {
            match r {
                Ok(_) => {
                    self.list.dirty = true;
                    self.inbox.dirty = true;
                    self.instances.dirty = true;
                }
                Err(e) => ctx.toast_error(&e),
            }
        }
        ui.horizontal(|ui| {
            ui.heading(ctx.icons.label("workflows", &ctx.t("workflow:list.title")));
            if ctx.has("workflows.edit") && ui.button(ctx.icons.label("add", &ctx.t("workflow:list.new"))).clicked() {
                *open_designer = Some(Uuid::nil());
            }
            if ui.button(ctx.icons.glyph("refresh")).clicked() {
                self.list.dirty = true;
            }
            if self.list.load.is_some() || self.list.action.is_some() {
                ui.spinner();
            }
        });
        egui::ScrollArea::vertical().id_salt("wf-list").show(ui, |ui| {
            for w in self.list.items.clone() {
                widgets::card(ui, "", |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(egui::RichText::new(&w.name).strong());
                        let st = w.status.as_str();
                        badge(ui, st, st);
                        ui.label(egui::RichText::new(format!("v{} · {} · {:?}", w.version, w.triggers.join(", "), w.ai_autonomy)).small().weak());
                        if w.running_instances > 0 {
                            let mut a = Map::new();
                            a.insert("count".into(), json!(w.running_instances));
                            ui.label(egui::RichText::new(ctx.lang.t_args("workflow:list.running", &a)).small());
                        }
                    });
                    if !w.description.is_empty() {
                        ui.label(egui::RichText::new(&w.description).small());
                    }
                    for r in &w.review_reasons {
                        ui.label(egui::RichText::new(format!("⚠ {r}")).small().color(widgets::severity_color(Severity::Warning)));
                    }
                    ui.horizontal(|ui| {
                        let active = w.status == WorkflowStatus::Active;
                        if ctx.has("workflows.run") && ui.add_enabled(active, egui::Button::new(ctx.icons.label("play", &ctx.t("workflow:list.start")))).clicked() {
                            let api = ctx.api.clone();
                            let id = w.id;
                            self.list.action = Some(Promise::spawn(async move { api.post::<Value, _>(&format!("/api/workflows/{id}/start"), &json!({})).await }));
                        }
                        if ctx.has("ai.run") && w.ai_autonomy != Autonomy::Off && ui.add_enabled(active, egui::Button::new(ctx.icons.label("ai", &ctx.t("workflow:list.startAi")))).clicked() {
                            let api = ctx.api.clone();
                            let id = w.id;
                            self.list.action = Some(Promise::spawn(async move { api.post::<Value, _>(&format!("/api/workflows/{id}/start"), &json!({ "executor": "ai" })).await }));
                        }
                        if ctx.has("workflows.edit") && ui.button(ctx.icons.label("edit", &ctx.t("workflow:list.edit"))).clicked() {
                            *open_designer = Some(w.id);
                        }
                        if ctx.has("workflows.edit") && w.status != WorkflowStatus::Retired && ui.button(ctx.t("workflow:list.retire")).clicked() {
                            let api = ctx.api.clone();
                            let id = w.id;
                            self.list.action = Some(Promise::spawn(async move { api.post::<Value, _>(&format!("/api/workflows/{id}/retire"), &json!({})).await }));
                        }
                    });
                });
                ui.add_space(4.0);
            }
        });
    }

    // ------------------------------------------------------------------ designer

    /// Opens a workflow in the designer (nil = new).
    pub fn open_in_designer(&mut self, ctx: &AppContext, id: Uuid) {
        self.designer.issues.clear();
        self.designer.error = None;
        self.designer.message = None;
        if id.is_nil() {
            self.designer.json = serde_json::to_string_pretty(&json!({
                "name": "New workflow", "startStepId": "input",
                "steps": [{ "id": "input", "kind": "input", "title": "Enter data", "input": { "schemaId": "workflow.input", "fields": [{ "name": "note", "type": "text", "label": "Note", "required": true }] } }],
                "transitions": [{ "from": "input", "to": "end" }],
                "triggers": [{ "id": "manual", "kind": "manual" }],
                "aiPolicy": { "autonomy": "off" }
            }))
            .unwrap_or_default();
        } else {
            let api = ctx.api.clone();
            self.designer.loading = Some(Promise::spawn(async move { api.get::<WorkflowDefinition>(&format!("/api/workflows/{id}")).await }));
        }
    }

    pub fn open_definition(&mut self, def: &WorkflowDefinition) {
        self.designer.json = serde_json::to_string_pretty(def).unwrap_or_default();
        self.designer.issues.clear();
        self.designer.message = None;
    }

    pub fn designer_ui(&mut self, ui: &mut egui::Ui, ctx: &mut AppContext) {
        if self.designer.json.trim().is_empty() && self.designer.loading.is_none() {
            // opened without a workflow: start from a blank draft instead of an empty editor
            self.open_in_designer(ctx, Uuid::nil());
        }
        let d = &mut self.designer;
        if let Some(r) = poll(&mut d.loading) {
            match r {
                Ok(def) => d.json = serde_json::to_string_pretty(&def).unwrap_or_default(),
                Err(e) => d.error = Some(e),
            }
        }
        if d.catalog_data.is_none() && d.catalog.is_none() {
            let api = ctx.api.clone();
            d.catalog = Some(Promise::spawn(async move { api.get::<Value>("/api/workflows/catalog").await }));
        }
        if let Some(Ok(c)) = poll(&mut d.catalog) {
            d.catalog_data = Some(c);
        }
        if let Some(r) = poll(&mut d.validating) {
            match r {
                Ok(v) => {
                    d.issues = v.issues;
                    d.error = None;
                    d.message = Some(if d.issues.is_empty() { ctx.t("workflow:designer.valid") } else { String::new() });
                }
                Err(e) => d.error = Some(e),
            }
        }
        if let Some(r) = poll(&mut d.saving) {
            match r {
                Ok(v) => {
                    let mut a = Map::new();
                    a.insert("version".into(), json!(v.version));
                    d.message = Some(ctx.lang.t_args("workflow:designer.saved", &a));
                    d.issues = v.issues;
                    d.error = None;
                    // keep the designer on the saved id
                    if let Ok(mut def) = serde_json::from_str::<WorkflowDefinition>(&d.json) {
                        def.id = v.id;
                        def.version = v.version;
                        def.status = v.status;
                        d.json = serde_json::to_string_pretty(&def).unwrap_or_default();
                    }
                    self.list.dirty = true;
                }
                Err(e) => {
                    if let Some(issues) = e.metadata.get("issues").and_then(|i| serde_json::from_value(i.clone()).ok()) {
                        d.issues = issues;
                    }
                    d.error = Some(e);
                }
            }
        }
        ui.horizontal(|ui| {
            ui.heading(ctx.icons.label("designer", &ctx.t("workflow:designer.title")));
        });
        let parsed: Result<WorkflowDefinition, String> = serde_json::from_str(&d.json).map_err(|e| e.to_string());
        ui.horizontal(|ui| {
            let ok = parsed.is_ok();
            if ui.add_enabled(ok, egui::Button::new(ctx.t("workflow:designer.validate"))).clicked() {
                if let Ok(def) = &parsed {
                    let api = ctx.api.clone();
                    let def = def.clone();
                    d.validating = Some(Promise::spawn(async move { api.post::<ValidateWorkflowResponse, _>("/api/workflows/validate", &def).await }));
                }
            }
            for (publish, key) in [(false, "workflow:designer.saveDraft"), (true, "workflow:designer.publish")] {
                if ui.add_enabled(ok && ctx.has("workflows.edit"), egui::Button::new(ctx.t(key))).clicked() {
                    if let Ok(def) = &parsed {
                        let api = ctx.api.clone();
                        let req = SaveWorkflowRequest { definition: def.clone(), publish };
                        d.saving = Some(Promise::spawn(async move { api.post::<SaveWorkflowResponse, _>("/api/workflows", &req).await }));
                    }
                }
            }
            if d.validating.is_some() || d.saving.is_some() || d.loading.is_some() {
                ui.spinner();
            }
            if let Some(m) = &d.message {
                ui.label(egui::RichText::new(m).color(widgets::severity_color(Severity::Success)));
            }
        });
        if let Err(e) = &parsed {
            ui.label(egui::RichText::new(format!("JSON: {e}")).color(widgets::severity_color(Severity::Error)));
        }
        if let Some(e) = &d.error {
            widgets::error_box(ui, ctx, e);
        }
        if !d.issues.is_empty() {
            ui.label(egui::RichText::new(ctx.t("workflow:designer.issues")).strong());
            for i in &d.issues {
                let (icon, sev) = if i.severity == IssueSeverity::Error { ("⛔", Severity::Error) } else { ("⚠", Severity::Warning) };
                ui.label(egui::RichText::new(format!("{icon} {}{}", i.step_id.as_ref().map(|s| format!("[{s}] ")).unwrap_or_default(), i.message)).color(widgets::severity_color(sev)));
            }
        }
        egui::SidePanel::right("designer-catalog").resizable(true).default_width(260.0).show_inside(ui, |ui| {
            ui.label(egui::RichText::new(ctx.t("workflow:designer.catalog")).strong());
            egui::ScrollArea::vertical().id_salt("designer-catalog-scroll").show(ui, |ui| {
                if let Some(c) = &d.catalog_data {
                    for m in c["modules"].as_array().cloned().unwrap_or_default() {
                        ui.collapsing(ctx.t(m["displayName"].as_str().unwrap_or("")), |ui| {
                            for st in m["stepTypes"].as_array().cloned().unwrap_or_default() {
                                let id = st["id"].as_str().unwrap_or("").to_string();
                                let resp = ui.button(format!("⏵ {}", ctx.t(st["title"].as_str().unwrap_or(&id)))).on_hover_text(format!("{id}\noutputs: {}", st["outputs"]));
                                if resp.clicked() {
                                    // Add a module step for this step type to the definition.
                                    if let Ok(mut def) = serde_json::from_str::<WorkflowDefinition>(&d.json) {
                                        let n = def.steps.len() + 1;
                                        let fields: Vec<String> = st["input"]["fields"].as_array().cloned().unwrap_or_default().iter().filter_map(|f| f["name"].as_str().map(str::to_string)).collect();
                                        def.steps.push(WorkflowStep {
                                            id: format!("step{n}"),
                                            kind: StepKind::ModuleStep,
                                            title: id.clone(),
                                            step_type_id: Some(id.clone()),
                                            parameters: fields.iter().map(|f| (f.clone(), format!("vars.{f}"))).collect(),
                                            step_type_version: 1,
                                            max_retries: 2,
                                            ..Default::default()
                                        });
                                        d.json = serde_json::to_string_pretty(&def).unwrap_or_default();
                                    }
                                }
                            }
                            for ev in m["events"].as_array().cloned().unwrap_or_default() {
                                ui.label(egui::RichText::new(format!("⚡ {}", ev["type"].as_str().unwrap_or(""))).small()).on_hover_text(ev["fields"].to_string());
                            }
                            for c in m["commands"].as_array().cloned().unwrap_or_default() {
                                ui.label(egui::RichText::new(format!("⌘ {}", c["id"].as_str().unwrap_or(""))).small());
                            }
                        });
                    }
                }
            });
        });
        ui.label(egui::RichText::new(ctx.t("workflow:designer.json")).weak());
        egui::ScrollArea::vertical().id_salt("designer-json").show(ui, |ui| {
            ui.add(egui::TextEdit::multiline(&mut d.json).code_editor().desired_width(f32::INFINITY).desired_rows(30));
        });
    }

    // ------------------------------------------------------------------ runs

    pub fn instances_ui(&mut self, ui: &mut egui::Ui, ctx: &mut AppContext) {
        let s = &mut self.instances;
        if (s.dirty || (s.items.is_empty() && s.load.is_none())) && s.load.is_none() {
            let api = ctx.api.clone();
            let f = s.filter.clone();
            s.load = Some(Promise::spawn(async move { api.get::<Vec<InstanceSummary>>(&format!("/api/workflows/instances{}", if f.is_empty() { String::new() } else { format!("?state={f}") })).await }));
            s.dirty = false;
        }
        if let Some(r) = poll(&mut s.load) {
            match r {
                Ok(v) => s.items = v,
                Err(e) => ctx.toast_error(&e),
            }
        }
        if let Some(Ok(i)) = poll(&mut s.detail) {
            s.instance = Some(i);
        }
        if let Some(r) = poll(&mut s.action) {
            match r {
                Ok(i) => {
                    s.instance = Some(i);
                    s.dirty = true;
                }
                Err(e) => ctx.toast_error(&e),
            }
        }
        ui.horizontal(|ui| {
            ui.heading(ctx.icons.label("workflows", &ctx.t("workflow:instances.title")));
            egui::ComboBox::from_id_salt("inst-filter").selected_text(if s.filter.is_empty() { ctx.t("shell:common.all") } else { s.filter.clone() }).show_ui(ui, |ui| {
                for f in ["", "running", "waitingForInput", "waitingForApproval", "waitingForEvent", "retrying", "paused", "completed", "failed", "cancelled"] {
                    if ui.selectable_label(s.filter == f, if f.is_empty() { ctx.t("shell:common.all") } else { f.to_string() }).clicked() {
                        s.filter = f.to_string();
                        s.dirty = true;
                    }
                }
            });
            if ui.button(ctx.icons.glyph("refresh")).clicked() {
                s.dirty = true;
            }
        });
        let items = s.items.clone();
        let mut clicked = None;
        egui::SidePanel::left("inst-list").resizable(true).default_width(320.0).show_inside(ui, |ui| {
            egui::ScrollArea::vertical().id_salt("inst-scroll").show(ui, |ui| {
                for i in &items {
                    ui.horizontal(|ui| {
                        state_badge(ui, i.state);
                        if ui.selectable_label(s.selected == Some(i.id), format!("{} (v{})", i.workflow_name, i.workflow_version)).on_hover_text(format!("{} · {}", i.run_as_name, format_value(&json!(i.updated_at), ValueFormat::DateTime, None, german(ctx)))).clicked() {
                            clicked = Some(i.id);
                        }
                    });
                }
            });
        });
        if let Some(id) = clicked {
            s.selected = Some(id);
            let api = ctx.api.clone();
            s.detail = Some(Promise::spawn(async move { api.get::<WorkflowInstance>(&format!("/api/workflows/instances/{id}")).await }));
        }
        if let Some(inst) = s.instance.clone() {
            ui.horizontal(|ui| {
                ui.heading(&inst.workflow_name);
                state_badge(ui, inst.state);
            });
            ui.label(egui::RichText::new(format!("{} · {:?} · {}", inst.run_as_name, inst.executor, inst.id)).small().weak());
            if let Some(e) = &inst.error {
                widgets::error_box(ui, ctx, e);
            }
            ui.horizontal(|ui| {
                if !inst.state.is_finished() && ui.button(ctx.t("workflow:instances.cancel")).clicked() {
                    let api = ctx.api.clone();
                    let id = inst.id;
                    s.action = Some(Promise::spawn(async move { api.post::<WorkflowInstance, _>(&format!("/api/workflows/instances/{id}/cancel"), &json!({})).await }));
                }
                if matches!(inst.state, InstanceState::Paused | InstanceState::Failed | InstanceState::Retrying) && ctx.has("workflows.edit") && ui.button(ctx.t("workflow:instances.resume")).clicked() {
                    let api = ctx.api.clone();
                    let id = inst.id;
                    s.action = Some(Promise::spawn(async move { api.post::<WorkflowInstance, _>(&format!("/api/workflows/instances/{id}/resume"), &json!({})).await }));
                }
            });
            egui::ScrollArea::vertical().id_salt("inst-detail").show(ui, |ui| {
                ui.collapsing("vars", |ui| {
                    let mut t = serde_json::to_string_pretty(&inst.vars.iter().filter(|(k, _)| !k.starts_with("__")).map(|(k, v)| (k.clone(), v.clone())).collect::<Map<_, _>>()).unwrap_or_default();
                    ui.add(egui::TextEdit::multiline(&mut t).code_editor().desired_width(f32::INFINITY));
                });
                history(ui, ctx, &inst);
            });
        }
    }

    // ------------------------------------------------------------------ impact

    pub fn impact_ui(&mut self, ui: &mut egui::Ui, ctx: &mut AppContext, preselect: Option<String>) {
        let s = &mut self.impact;
        if let Some(p) = preselect.and_then(|p| Uuid::parse_str(&p).ok()) {
            s.selected = Some(p);
        }
        if (s.dirty || (s.reports.is_empty() && s.load.is_none())) && s.load.is_none() {
            let api = ctx.api.clone();
            s.load = Some(Promise::spawn(async move { api.get::<Vec<ImpactReport>>("/api/workflows/impact").await }));
            s.dirty = false;
        }
        if let Some(r) = poll(&mut s.load) {
            match r {
                Ok(v) => {
                    if s.selected.is_none() {
                        s.selected = v.first().map(|r| r.id);
                    }
                    s.reports = v;
                }
                Err(e) => ctx.toast_error(&e),
            }
        }
        ui.heading(ctx.icons.label("impact", &ctx.t("workflow:impact.title")));
        if s.reports.is_empty() {
            ui.label(egui::RichText::new(ctx.t("workflow:impact.empty")).weak());
            return;
        }
        let reports = s.reports.clone();
        egui::SidePanel::left("impact-list").resizable(true).default_width(240.0).show_inside(ui, |ui| {
            for r in &reports {
                let label = format!("{} {} ⏵ {}\n{}", r.module_id, r.from_version, r.to_version, format_value(&json!(r.analyzed_at), ValueFormat::DateTime, None, german(ctx)));
                if ui.selectable_label(s.selected == Some(r.id), label).clicked() {
                    s.selected = Some(r.id);
                }
            }
        });
        let Some(r) = reports.iter().find(|r| Some(r.id) == s.selected) else { return };
        egui::ScrollArea::vertical().id_salt("impact-detail").show(ui, |ui| {
            ui.heading(format!("{} {} ⏵ {}", r.module_id, r.from_version, r.to_version));
            ui.horizontal(|ui| {
                for (class, key) in [(ImpactClass::AutoMigrated, "workflow:impact.autoMigrated"), (ImpactClass::ReviewRequired, "workflow:impact.reviewRequired"), (ImpactClass::Broken, "workflow:impact.broken"), (ImpactClass::Compatible, "workflow:impact.compatible")] {
                    badge(ui, &format!("{} {}", r.count(class), ctx.t(key)), class.as_str());
                }
            });
            ui.add_space(6.0);
            ui.label(egui::RichText::new(ctx.t("workflow:impact.workflows")).strong());
            for w in &r.workflows {
                widgets::card(ui, "", |ui| {
                    ui.horizontal(|ui| {
                        badge(ui, &ctx.t(&format!("workflow:impact.{}", w.class.as_str())), w.class.as_str());
                        ui.label(egui::RichText::new(&w.workflow_name).strong());
                        ui.label(egui::RichText::new(format!("v{}{}", w.from_version, w.to_version.map(|v| format!(" ⏵ v{v}")).unwrap_or_default())).weak());
                    });
                    for reason in &w.reasons {
                        ui.label(format!("• {reason}"));
                    }
                    for a in &w.actions {
                        ui.label(egui::RichText::new(format!("⏵ {a}")).small().weak());
                    }
                });
            }
            ui.add_space(6.0);
            ui.collapsing(ctx.t("workflow:impact.changes"), |ui| {
                for c in &r.changes {
                    ui.label(format!("{:?} {} – {:?} ({:?}) {}", c.element, c.id, c.kind, c.severity, c.details.join("; ")));
                }
            });
        });
    }

    // ------------------------------------------------------------------ AI panel

    pub fn ai_ui(&mut self, ui: &mut egui::Ui, ctx: &mut AppContext) {
        let s = &mut self.ai;
        if (s.dirty || s.current.is_none()) && s.status.is_none() {
            let api = ctx.api.clone();
            s.status = Some(Promise::spawn(async move { api.get::<AiStatus>("/api/ai/status").await }));
            if ctx.has("ai.approve") || ctx.has("ai.admin") || ctx.has("workflows.edit") {
                let api = ctx.api.clone();
                s.audit = Some(Promise::spawn(async move { api.get::<Vec<AiAuditEntry>>("/api/ai/audit").await }));
            }
            s.dirty = false;
        }
        if let Some(r) = poll(&mut s.status) {
            match r {
                Ok(v) => s.current = Some(v),
                Err(e) => ctx.toast_error(&e),
            }
        }
        if let Some(Ok(v)) = poll(&mut s.audit) {
            s.entries = v;
        }
        if let Some(r) = poll(&mut s.switching) {
            match r {
                Ok(v) => s.current = Some(v),
                Err(e) => ctx.toast_error(&e),
            }
        }
        ui.heading(ctx.icons.label("ai", &ctx.t("ai:panel.title")));
        if let Some(st) = s.current.clone() {
            egui::Grid::new("ai-status").num_columns(2).show(ui, |ui| {
                ui.label(ctx.t("ai:panel.status"));
                badge(ui, &ctx.t(if st.enabled { "ai:panel.enabled" } else { "ai:panel.disabled" }), if st.enabled { "active" } else { "failed" });
                ui.end_row();
                ui.label(ctx.t("ai:panel.provider"));
                ui.label(format!("{} · {}", st.provider, st.model));
                ui.end_row();
                ui.label(ctx.t("ai:panel.runsToday"));
                ui.label(st.runs_today.to_string());
                ui.end_row();
            });
            if ctx.has("ai.admin") {
                let label = if st.enabled { "ai:panel.switchOff" } else { "ai:panel.switchOn" };
                if ui.add_enabled(s.switching.is_none(), egui::Button::new(ctx.t(label))).clicked() {
                    let api = ctx.api.clone();
                    let req = AiSwitchRequest { enabled: !st.enabled, scope: "global".into() };
                    s.switching = Some(Promise::spawn(async move { api.post::<AiStatus, _>("/api/ai/switch", &req).await }));
                }
            }
        } else {
            widgets::busy(ui, ctx);
        }
        if !s.entries.is_empty() {
            ui.add_space(8.0);
            ui.label(egui::RichText::new(ctx.t("ai:panel.audit")).strong());
            egui::ScrollArea::vertical().id_salt("ai-audit").show(ui, |ui| {
                egui::Grid::new("ai-audit-grid").striped(true).num_columns(5).show(ui, |ui| {
                    for h in ["", "ai:panel.decision", "ai:panel.confidence", "ai:panel.reasoning", ""] {
                        ui.label(egui::RichText::new(if h.is_empty() { String::new() } else { ctx.t(h) }).strong());
                    }
                    ui.end_row();
                    for e in &s.entries {
                        ui.label(format_value(&json!(e.at), ValueFormat::DateTime, None, german(ctx)));
                        badge(ui, &e.decision, &e.decision);
                        ui.label(e.confidence.map(|c| format!("{:.0} %", c * 100.0)).unwrap_or_default());
                        ui.label(e.reasoning.clone().or(e.policy_code.clone()).unwrap_or_default());
                        ui.label(egui::RichText::new(format!("{} · {}", e.step_id, e.tokens)).small().weak());
                        ui.end_row();
                    }
                });
            });
        }
    }
}

/// The history of an instance (who did what, with AI confidence and reasoning).
pub fn history(ui: &mut egui::Ui, ctx: &AppContext, inst: &WorkflowInstance) {
    egui::Grid::new(("history", inst.id)).striped(true).num_columns(4).show(ui, |ui| {
        for h in &inst.history {
            ui.label(egui::RichText::new(format_value(&json!(h.at), ValueFormat::DateTime, None, german(ctx))).small());
            let who = match h.actor_kind {
                ActorKind::Ai => format!("💡 {}", h.actor_name),
                ActorKind::Human => format!("👤 {}", h.actor_name),
                ActorKind::Engine => format!("⚙ {}", h.actor_name),
            };
            ui.label(who);
            ui.label(format!("{} {}", h.action, if h.step_title.is_empty() { h.step_id.clone() } else { h.step_title.clone() }));
            let mut extra = h.confidence.map(|c| format!("{:.0} % ", c * 100.0)).unwrap_or_default();
            if let Some(r) = &h.reasoning {
                extra.push_str(r);
            }
            ui.label(egui::RichText::new(extra).small()).on_hover_text(serde_json::to_string_pretty(&h.data).unwrap_or_default());
            ui.end_row();
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recorder_builds_a_valid_draft() {
        let mut r = Recorder { active: true, steps: vec![] };
        r.record("documents.invoice.submit", json!({ "id": "a1", "note": "it's ok" }));
        r.record("documents.send", json!({ "documentId": "d1" }));
        let def = r.finish("Recorded");
        assert_eq!(def.steps.len(), 2);
        assert_eq!(def.steps[0].parameters["note"], "'it\\'s ok'");
        redhive_contracts::workflows::analysis::ensure_valid(&def).unwrap();
    }
}
