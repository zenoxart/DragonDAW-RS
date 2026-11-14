//! `InputForm`: a form generated from an [`InputSchema`]. Lays fields out in 1–3 columns by size class and
//! span, loads lookups for `entityRef` fields, validates on submit and shows field errors.

use crate::context::AppContext;
use crate::runtime::{poll, Promise};
use redhive_contracts::workflows::input::{ChoiceOption, FieldType, InputField, InputSchema, Span};
use redhive_monads::{Error, Res};
use serde_json::{Map, Value};
use std::collections::HashMap;

#[derive(Default)]
pub struct FormState {
    /// Edited text per field (all values are edited as text, except booleans).
    pub text: HashMap<String, String>,
    pub bools: HashMap<String, bool>,
    pub errors: HashMap<String, String>,
    pub general_error: Option<Error>,
    lookups: HashMap<String, LookupState>,
}

enum LookupState {
    Loading(Promise<Res<Vec<ChoiceOption>>>),
    Ready(Vec<ChoiceOption>),
    Failed,
}

impl FormState {
    /// A form pre-filled with values (defaults for missing fields).
    pub fn new(schema: &InputSchema, values: &Map<String, Value>) -> Self {
        let mut s = FormState::default();
        for f in &schema.fields {
            let v = values.get(&f.name).filter(|v| !v.is_null()).or(f.default.as_ref());
            match f.field_type {
                FieldType::Boolean => {
                    s.bools.insert(f.name.clone(), v.and_then(Value::as_bool).or_else(|| v.and_then(Value::as_str).map(|x| x == "true")).unwrap_or(false));
                }
                _ => {
                    s.text.insert(f.name.clone(), v.map(super::format::text).unwrap_or_default());
                }
            }
        }
        s
    }

    /// The raw values (to be validated by the schema).
    pub fn values(&self, schema: &InputSchema) -> Map<String, Value> {
        let mut m = Map::new();
        for f in &schema.fields {
            match f.field_type {
                FieldType::Boolean => {
                    m.insert(f.name.clone(), Value::Bool(*self.bools.get(&f.name).unwrap_or(&false)));
                }
                _ => {
                    let t = self.text.get(&f.name).cloned().unwrap_or_default();
                    if !t.trim().is_empty() {
                        m.insert(f.name.clone(), Value::String(t));
                    }
                }
            }
        }
        m
    }

    /// Validates with the schema; on failure the field errors are shown next to the fields.
    pub fn collect(&mut self, schema: &InputSchema, ctx: &AppContext) -> Option<Map<String, Value>> {
        self.errors.clear();
        self.general_error = None;
        match schema.validate(&self.values(schema)) {
            Ok(v) => Some(v),
            Err(e) => {
                self.show_error(e, ctx);
                None
            }
        }
    }

    /// Shows a server error: field errors next to their fields, the rest above the form.
    pub fn show_error(&mut self, e: Error, ctx: &AppContext) {
        self.errors.clear();
        for fe in &e.field_errors {
            if !fe.field.is_empty() {
                let mut text = ctx.lang.t_args(&fe.text_key, &e.metadata);
                if !ctx.lang.has(&fe.text_key) {
                    text = ctx.t("errors:invalidValue");
                }
                self.errors.insert(fe.field.clone(), text);
            }
        }
        if self.errors.is_empty() || e.field_errors.iter().any(|f| f.field.is_empty()) {
            self.general_error = Some(e);
        }
    }
}

fn units(span: Span, columns: usize) -> usize {
    if columns == 1 {
        return 6;
    }
    match span {
        Span::Full => 6,
        Span::Half => 3,
        Span::Third => {
            if columns >= 3 {
                2
            } else {
                3
            }
        }
    }
}

/// Draws the form. `columns`: 1–3 (from the size class).
pub fn form_ui(ui: &mut egui::Ui, ctx: &AppContext, schema: &InputSchema, state: &mut FormState, columns: usize, id_salt: &str) {
    if let Some(e) = &state.general_error {
        super::error_box(ui, ctx, e);
        ui.add_space(4.0);
    }
    // Pack fields into rows of 6 units.
    let mut rows: Vec<Vec<&InputField>> = Vec::new();
    let mut used = 6;
    for f in &schema.fields {
        let u = units(f.span, columns);
        if used + u > 6 {
            rows.push(Vec::new());
            used = 0;
        }
        used += u;
        if let Some(r) = rows.last_mut() {
            r.push(f);
        }
    }
    for row in rows {
        let n = row.len();
        ui.columns(n, |cols| {
            for (i, f) in row.iter().enumerate() {
                field_ui(&mut cols[i], ctx, f, state, id_salt);
            }
        });
        ui.add_space(2.0);
    }
}

fn field_ui(ui: &mut egui::Ui, ctx: &AppContext, f: &InputField, state: &mut FormState, id_salt: &str) {
    let label = format!("{}{}", ctx.t(&f.label), if f.required { " *" } else { "" });
    let resp_label = ui.label(egui::RichText::new(label).small());
    if let Some(h) = &f.help {
        resp_label.on_hover_text(ctx.t(h));
    }
    let id = egui::Id::new((id_salt, &f.name));
    ui.add_enabled_ui(!f.read_only, |ui| match f.field_type {
        FieldType::Boolean => {
            let v = state.bools.entry(f.name.clone()).or_insert(false);
            ui.checkbox(v, "");
        }
        FieldType::MultiLine => {
            let v = state.text.entry(f.name.clone()).or_default();
            ui.add(egui::TextEdit::multiline(v).id(id).desired_rows(3).desired_width(f32::INFINITY));
        }
        FieldType::Choice => {
            let current = state.text.get(&f.name).cloned().unwrap_or_default();
            let shown = f.options.iter().find(|o| o.value == current).map(|o| ctx.t(&o.label)).unwrap_or_else(|| if current.is_empty() { ctx.t("shell:form.choose") } else { current.clone() });
            egui::ComboBox::from_id_salt(id).selected_text(shown).width(ui.available_width() - 8.0).show_ui(ui, |ui| {
                for o in &f.options {
                    if ui.selectable_label(current == o.value, ctx.t(&o.label)).clicked() {
                        state.text.insert(f.name.clone(), o.value.clone());
                    }
                }
            });
        }
        FieldType::EntityRef => {
            let url = f.lookup.clone().unwrap_or_default();
            if !url.is_empty() && !state.lookups.contains_key(&url) {
                let api = ctx.api.clone();
                let u = url.clone();
                state.lookups.insert(url.clone(), LookupState::Loading(Promise::spawn(async move { api.get::<Vec<ChoiceOption>>(&u).await })));
            }
            if let Some(LookupState::Loading(p)) = state.lookups.get(&url) {
                if let Some(r) = p.take() {
                    state.lookups.insert(url.clone(), r.map(LookupState::Ready).unwrap_or(LookupState::Failed));
                }
            }
            let current = state.text.get(&f.name).cloned().unwrap_or_default();
            match state.lookups.get(&url) {
                Some(LookupState::Ready(options)) => {
                    let options = options.clone();
                    let shown = options.iter().find(|o| o.value == current).map(|o| o.label.clone()).unwrap_or_else(|| if current.is_empty() { ctx.t("shell:form.choose") } else { current.clone() });
                    egui::ComboBox::from_id_salt(id).selected_text(shown).width(ui.available_width() - 8.0).show_ui(ui, |ui| {
                        for o in &options {
                            if ui.selectable_label(current == o.value, &o.label).clicked() {
                                state.text.insert(f.name.clone(), o.value.clone());
                            }
                        }
                    });
                }
                Some(LookupState::Loading(_)) => {
                    ui.spinner();
                }
                _ => {
                    // No lookup (or it failed): free text.
                    let v = state.text.entry(f.name.clone()).or_default();
                    ui.add(egui::TextEdit::singleline(v).id(id).desired_width(f32::INFINITY));
                    if !url.is_empty() {
                        ui.label(egui::RichText::new(ctx.t("shell:form.lookupFailed")).small().weak());
                    }
                }
            }
        }
        FieldType::Date => {
            let v = state.text.entry(f.name.clone()).or_default();
            ui.add(egui::TextEdit::singleline(v).id(id).hint_text("YYYY-MM-DD").desired_width(f32::INFINITY));
        }
        FieldType::Number | FieldType::Money | FieldType::Text => {
            let v = state.text.entry(f.name.clone()).or_default();
            let mut edit = egui::TextEdit::singleline(v).id(id).desired_width(f32::INFINITY);
            if f.sensitive {
                edit = edit.password(true);
            }
            ui.add(edit);
        }
    });
    if let Some(e) = state.errors.get(&f.name) {
        ui.label(egui::RichText::new(e).small().color(egui::Color32::from_rgb(198, 40, 40)));
    }
}

/// Polls an optional promise and turns the result into a form error (helper for dialogs).
pub fn finish<T: Send + 'static>(p: &mut Option<Promise<Res<T>>>, state: &mut FormState, ctx: &AppContext) -> Option<T> {
    match poll(p)? {
        Ok(v) => Some(v),
        Err(e) => {
            state.show_error(e, ctx);
            None
        }
    }
}
