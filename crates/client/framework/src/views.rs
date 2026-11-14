//! Renders view packs: dashboards, lists (search, filters, groups, detail pane, row and toolbar actions),
//! report runners and forms. A view keeps its state (search, filters, selection) across reloads and
//! hot-swaps; its spec is looked up fresh every frame, so a swapped module shows its new UI immediately.

use crate::context::AppContext;
use crate::runtime::{poll, Promise};
use crate::widgets::form::{form_ui, FormState};
use crate::widgets::format::{self, format_value, path};
use crate::widgets::{self, chart};
use crate::windowing::SizeClass;
use redhive_contracts::notifications::Severity;
use redhive_contracts::viewpack::*;
use redhive_contracts::workflows::expr::{self, Scope};
use redhive_contracts::workflows::input::{InputField, InputSchema};
use redhive_monads::{Error, Res};
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;

/// An open action dialog (form and/or confirmation, then the HTTP call).
pub struct ActionDialog {
    pub action: ActionSpec,
    pub row: Value,
    pub form: Option<FormState>,
    pub running: Option<Promise<Res<Value>>>,
    pub result: Option<Value>,
}

#[derive(Default)]
pub struct ReportState {
    catalog: Option<Promise<Res<Vec<Value>>>>,
    reports: Vec<Value>,
    pub selected: Option<String>,
    form: Option<(String, FormState)>,
    running: Option<Promise<Res<Value>>>,
    result: Option<Value>,
    error: Option<Error>,
}

/// State of one open module view. A new state is *dirty*, so the view loads its data on first render.
pub struct ViewState {
    pub data: Option<Value>,
    pub loading: Option<Promise<Res<Value>>>,
    pub error: Option<Error>,
    pub dirty: bool,
    pub search: String,
    pub filters: BTreeMap<String, String>,
    pub selected: Option<String>,
    pub dialog: Option<ActionDialog>,
    pub report: ReportState,
    pub form: Option<FormState>,
    pub size: SizeClass,
    last_query: String,
}

impl Default for ViewState {
    fn default() -> Self {
        ViewState {
            data: None,
            loading: None,
            error: None,
            dirty: true,
            search: String::new(),
            filters: BTreeMap::new(),
            selected: None,
            dialog: None,
            report: ReportState::default(),
            form: None,
            size: SizeClass::default(),
            last_query: String::new(),
        }
    }
}

impl ViewState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether an action of this view is still running (hot-swap waits for it).
    pub fn busy(&self) -> bool {
        self.dialog.as_ref().is_some_and(|d| d.running.is_some()) || self.report.running.is_some()
    }

    /// The selected row of a list view.
    pub fn selected_row(&self, spec: &ListSpec) -> Option<Value> {
        let id = self.selected.as_ref()?;
        rows(self.data.as_ref()?, &spec.items).into_iter().find(|r| format::text(path(r, &spec.id_field)) == *id)
    }
}

fn rows(data: &Value, items: &str) -> Vec<Value> {
    match path(data, items) {
        Value::Array(a) => a.clone(),
        _ => Vec::new(),
    }
}

fn german(ctx: &AppContext) -> bool {
    ctx.lang.language().starts_with("de")
}

fn user_value(ctx: &AppContext) -> Value {
    match &ctx.session {
        Some(s) => json!({ "id": s.profile.id, "name": s.profile.user_name, "displayName": s.profile.display_name, "roles": s.profile.roles }),
        None => Value::Null,
    }
}

fn visible(ctx: &AppContext, a: &ActionSpec, row: &Value) -> bool {
    if !a.permission.is_empty() && !ctx.has(&a.permission) {
        return false;
    }
    match a.visible_when.as_deref().filter(|w| !w.trim().is_empty()) {
        None => true,
        Some(w) => expr::parse(w).and_then(|e| e.eval(&Scope::new().with("row", row.clone()).with("user", user_value(ctx)))).map(|v| expr::truthy(&v)).unwrap_or(false),
    }
}

/// Translates a status value through `<prefix>:status.<value>` when such a text exists.
fn status_text(ctx: &AppContext, prefix: &str, value: &str) -> String {
    let key = format!("{prefix}:status.{value}");
    if ctx.lang.has(&key) {
        return ctx.t(&key);
    }
    let key = format!("{prefix}:category.{value}");
    if ctx.lang.has(&key) {
        ctx.t(&key)
    } else {
        value.to_string()
    }
}

fn cell(ui: &mut egui::Ui, ctx: &AppContext, prefix: &str, col: &ColumnSpec, row: &Value) {
    let v = path(row, &col.field);
    let currency = col.currency_field.as_deref().map(|f| format::text(path(row, f)));
    match col.format {
        ValueFormat::Badge => {
            let raw = format::text(v);
            if !raw.is_empty() {
                widgets::badge(ui, &status_text(ctx, prefix, &raw), &raw);
            }
        }
        f => {
            let text = format_value(v, f, currency.as_deref(), german(ctx));
            if matches!(f, ValueFormat::Number | ValueFormat::Money) {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| ui.label(text));
            } else {
                ui.label(text);
            }
        }
    }
}

/// Starts an action: opens a dialog for forms/confirmations, otherwise runs it right away.
pub fn start_action(ctx: &mut AppContext, state: &mut ViewState, action: &ActionSpec, row: Value) {
    if !action.permission.is_empty() && !ctx.has(&action.permission) {
        ctx.toast_error(&Error::forbidden("auth.permission.missing", "errors:forbidden").with_metadata("permission", action.permission.clone()));
        return;
    }
    let form = action.form.as_ref().map(|schema| {
        let sc = Scope::new().with("row", row.clone()).with("user", user_value(ctx));
        let mut prefill = Map::new();
        for (k, src) in &action.prefill {
            if let Ok(v) = expr::parse(src).and_then(|e| e.eval(&sc)) {
                prefill.insert(k.clone(), v);
            }
        }
        FormState::new(schema, &prefill)
    });
    let mut dialog = ActionDialog { action: action.clone(), row, form, running: None, result: None };
    if dialog.form.is_none() && dialog.action.confirm.is_none() {
        run_action(ctx, &mut dialog, Map::new());
    }
    state.dialog = Some(dialog);
}

fn run_action(ctx: &AppContext, d: &mut ActionDialog, form_values: Map<String, Value>) {
    let sc = Scope::new().with("row", d.row.clone()).with("form", Value::Object(form_values.clone())).with("user", user_value(ctx));
    let mut body = form_values;
    for (k, src) in &d.action.body {
        if let Ok(v) = expr::parse(src).and_then(|e| e.eval(&sc)) {
            body.insert(k.clone(), v);
        }
    }
    let url = fill_template(&d.action.endpoint, &d.row);
    let api = ctx.api.clone();
    let method = match d.action.method {
        HttpMethod::Get => reqwest::Method::GET,
        HttpMethod::Post => reqwest::Method::POST,
        HttpMethod::Put => reqwest::Method::PUT,
        HttpMethod::Delete => reqwest::Method::DELETE,
    };
    let body = Value::Object(body);
    let send = if method == reqwest::Method::GET || method == reqwest::Method::DELETE { None } else { Some(body) };
    d.running = Some(Promise::spawn(async move { api.request::<Value>(method, &url, send).await }));
}

/// Draws an open action dialog; returns true when the view should reload.
fn dialog_ui(ui: &mut egui::Ui, ctx: &mut AppContext, state: &mut ViewState, size: SizeClass, view_key: &str) -> bool {
    let Some(d) = state.dialog.as_mut() else { return false };
    let mut reload = false;
    let mut close = false;
    if let Some(result) = poll(&mut d.running) {
        match result {
            Ok(v) => {
                if let Some(s) = &d.action.success {
                    let t = ctx.t(s);
                    ctx.toast(t, Severity::Success);
                }
                if let Some(cmd) = &d.action.command {
                    let mut param = d.form.as_ref().and_then(|f| d.action.form.as_ref().map(|s| f.values(s))).unwrap_or_default();
                    if let Some(id) = d.row.get("id") {
                        param.insert("id".into(), id.clone());
                    }
                    ctx.executed_commands.push((cmd.clone(), Value::Object(param)));
                }
                reload = true;
                if d.action.show_result {
                    d.result = Some(v);
                } else {
                    close = true;
                }
            }
            Err(e) => match d.form.as_mut() {
                Some(f) => f.show_error(e, ctx),
                None => {
                    ctx.toast_error(&e);
                    close = true;
                }
            },
        }
    }
    let title = ctx.t(&d.action.label);
    let mut open = true;
    egui::Window::new(title)
        .id(egui::Id::new(("redhive.action", view_key, &d.action.id)))
        .collapsible(false)
        .resizable(true)
        .default_width(if size == SizeClass::Compact { 320.0 } else { 520.0 })
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .open(&mut open)
        .show(ui.ctx(), |ui| {
            if let Some(r) = &d.result {
                ui.label(ctx.t("shell:common.result"));
                let mut text = serde_json::to_string_pretty(r).unwrap_or_default();
                ui.add(egui::TextEdit::multiline(&mut text).code_editor().desired_rows(6).desired_width(f32::INFINITY));
                ui.horizontal(|ui| {
                    if ui.button(ctx.t("shell:common.copy")).clicked() {
                        ui.ctx().copy_text(text.clone());
                    }
                    if ui.button(ctx.t("shell:common.close")).clicked() {
                        close = true;
                    }
                });
                return;
            }
            if let Some(c) = &d.action.confirm {
                ui.label(ctx.t(c));
                ui.add_space(6.0);
            }
            if let (Some(schema), Some(form)) = (d.action.form.clone(), d.form.as_mut()) {
                form_ui(ui, ctx, &schema, form, size.columns().min(2), &d.action.id);
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                let busy = d.running.is_some();
                if ui.add_enabled(!busy, egui::Button::new(ctx.t(if d.action.confirm.is_some() && d.form.is_none() { "shell:common.yes" } else { "shell:common.ok" }))).clicked() {
                    let values = match (&d.action.form, d.form.as_mut()) {
                        (Some(schema), Some(f)) => f.collect(schema, ctx),
                        _ => Some(Map::new()),
                    };
                    if let Some(v) = values {
                        run_action(ctx, d, v);
                    }
                }
                if ui.button(ctx.t("shell:common.cancel")).clicked() {
                    close = true;
                }
                if busy {
                    ui.spinner();
                }
            });
        });
    if !open || close {
        state.dialog = None;
    }
    reload
}

fn query_url(base: &str, state: &ViewState, search: bool) -> String {
    let mut params: Vec<String> = Vec::new();
    if search && !state.search.trim().is_empty() {
        params.push(format!("search={}", urlencode(state.search.trim())));
    }
    for (k, v) in &state.filters {
        if !v.is_empty() && v != "all" {
            params.push(format!("{k}={}", urlencode(v)));
        }
    }
    if params.is_empty() {
        base.to_string()
    } else {
        format!("{base}{}{}", if base.contains('?') { "&" } else { "?" }, params.join("&"))
    }
}

fn urlencode(s: &str) -> String {
    s.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect()
}

fn load(ctx: &AppContext, state: &mut ViewState, url: String) {
    let api = ctx.api.clone();
    state.last_query = url.clone();
    state.loading = Some(Promise::spawn(async move { api.get::<Value>(&url).await }));
    state.dirty = false;
}

fn poll_data(ctx: &mut AppContext, state: &mut ViewState) {
    if let Some(r) = poll(&mut state.loading) {
        match r {
            Ok(v) => {
                state.data = Some(v);
                state.error = None;
            }
            Err(e) => {
                ctx.log.warn("views", format!("{} ({})", ctx.lang.error(&e), e.code));
                state.error = Some(e);
            }
        }
    }
}

/// Renders a module view.
pub fn render(ui: &mut egui::Ui, ctx: &mut AppContext, spec: &ViewSpec, prefix: &str, state: &mut ViewState) {
    state.size = SizeClass::with_hysteresis(ui.available_width(), state.size);
    poll_data(ctx, state);
    let reload = dialog_ui(ui, ctx, state, state.size, &spec.key);
    if reload {
        state.dirty = true;
    }
    match &spec.body {
        ViewBody::List(l) => list(ui, ctx, spec, l, prefix, state),
        ViewBody::Dashboard { endpoint, cards, chart, lists, actions } => dashboard(ui, ctx, endpoint, cards, chart.as_ref(), lists, actions, prefix, state),
        ViewBody::ReportRunner { catalog_endpoint, run_endpoint, runs_endpoint } => report(ui, ctx, catalog_endpoint, run_endpoint, runs_endpoint.as_deref(), state),
        ViewBody::Form { load_endpoint, schema, submit } => form_view(ui, ctx, load_endpoint.as_deref(), schema, submit, state),
    }
}

fn header(ui: &mut egui::Ui, ctx: &AppContext, spec: &ViewSpec) {
    ui.horizontal(|ui| {
        ui.heading(ctx.icons.label(&spec.icon, &ctx.t(&spec.title)));
    });
}

#[allow(clippy::too_many_arguments)]
fn list(ui: &mut egui::Ui, ctx: &mut AppContext, spec: &ViewSpec, l: &ListSpec, prefix: &str, state: &mut ViewState) {
    let url = query_url(&l.endpoint, state, l.search);
    if (state.dirty || url != state.last_query) && state.loading.is_none() {
        load(ctx, state, url);
    }
    header(ui, ctx, spec);
    // toolbar
    ui.horizontal_wrapped(|ui| {
        if l.search {
            ui.add(egui::TextEdit::singleline(&mut state.search).hint_text(ctx.t("shell:common.search")).desired_width(220.0));
        }
        for f in &l.filters {
            let current = state.filters.get(&f.param).cloned().unwrap_or_else(|| "all".into());
            let shown = f.options.iter().find(|o| o.value == current).map(|o| ctx.t(&o.label)).unwrap_or(current.clone());
            egui::ComboBox::from_id_salt((&spec.key, &f.param)).selected_text(format!("{}: {}", ctx.t(&f.label), shown)).show_ui(ui, |ui| {
                for o in &f.options {
                    if ui.selectable_label(current == o.value, ctx.t(&o.label)).clicked() {
                        state.filters.insert(f.param.clone(), o.value.clone());
                    }
                }
            });
        }
        if ui.button(ctx.icons.label("refresh", &ctx.t("shell:common.refresh"))).clicked() {
            state.dirty = true;
        }
        for a in &l.toolbar_actions {
            if visible(ctx, a, &Value::Null) && ui.button(ctx.icons.label(&a.icon, &ctx.t(&a.label))).clicked() {
                let a = a.clone();
                start_action(ctx, state, &a, Value::Null);
            }
        }
        if state.loading.is_some() {
            ui.spinner();
        }
    });
    ui.separator();
    if let Some(e) = &state.error {
        widgets::error_box(ui, ctx, e);
    }
    let Some(data) = state.data.clone() else {
        if state.loading.is_some() {
            widgets::busy(ui, ctx);
        }
        return;
    };
    let all_rows = rows(&data, &l.items);
    let total = path(&data, "total").as_i64().unwrap_or(all_rows.len() as i64);
    let mut args = Map::new();
    args.insert("count".into(), json!(total));
    ui.label(egui::RichText::new(ctx.lang.t_args("shell:list.items", &args)).small().weak());

    // Columns by size class: compact shows the first two.
    let columns: Vec<&ColumnSpec> = match state.size {
        SizeClass::Compact => l.columns.iter().take(2).collect(),
        SizeClass::Medium => l.columns.iter().take(4).collect(),
        _ => l.columns.iter().collect(),
    };
    let detail_side = matches!(state.size, SizeClass::Expanded | SizeClass::Wide) && !l.detail.is_empty();
    let selected_row = state.selected_row(l);
    if detail_side {
        egui::SidePanel::right(egui::Id::new(("detail", &spec.key))).resizable(true).default_width(300.0).show_inside(ui, |ui| {
            detail(ui, ctx, l, prefix, selected_row.as_ref(), state);
        });
    }
    let mut groups: Vec<(String, Vec<Value>)> = Vec::new();
    match &l.group_by {
        Some(g) => {
            for r in all_rows {
                let key = format::text(path(&r, g));
                match groups.iter_mut().find(|(k, _)| *k == key) {
                    Some((_, v)) => v.push(r),
                    None => groups.push((key, vec![r])),
                }
            }
            groups.sort_by(|a, b| a.0.cmp(&b.0));
        }
        None => groups.push((String::new(), all_rows)),
    }
    let mut clicked: Option<String> = None;
    let mut context_action: Option<(ActionSpec, Value)> = None;
    let height = if detail_side { ui.available_height() } else { (ui.available_height() * 0.65).max(160.0) };
    egui::ScrollArea::both().id_salt(("list", &spec.key)).max_height(height).auto_shrink([false, false]).show(ui, |ui| {
        let weights: f32 = columns.iter().map(|c| c.width).sum::<f32>().max(1.0);
        let avail = (ui.available_width() - 16.0).max(200.0);
        for (gname, grows) in &groups {
            if l.group_by.is_some() {
                let title = if gname.is_empty() { ctx.t("shell:list.groupNone") } else { gname.clone() };
                ui.label(egui::RichText::new(format!("{title}  ({})", grows.len())).strong());
            }
            egui::Grid::new(("grid", &spec.key, gname)).striped(true).num_columns(columns.len()).spacing([12.0, 6.0]).show(ui, |ui| {
                for c in &columns {
                    ui.add_sized([avail * c.width / weights - 12.0, 18.0], egui::Label::new(egui::RichText::new(ctx.t(&c.label)).strong()));
                }
                ui.end_row();
                for r in grows {
                    let id = format::text(path(r, &l.id_field));
                    let is_sel = state.selected.as_deref() == Some(id.as_str());
                    for (i, c) in columns.iter().enumerate() {
                        let resp = ui.scope(|ui| {
                            ui.set_min_width(avail * c.width / weights - 12.0);
                            if i == 0 {
                                let text = format_value(path(r, &c.field), c.format, None, german(ctx));
                                ui.selectable_label(is_sel, text)
                            } else {
                                cell(ui, ctx, prefix, c, r);
                                ui.interact(ui.min_rect(), ui.id().with(("cell", &id, i)), egui::Sense::click())
                            }
                        });
                        let resp = resp.inner;
                        if resp.clicked() {
                            clicked = Some(id.clone());
                        }
                        resp.context_menu(|ui| {
                            for a in &l.row_actions {
                                if visible(ctx, a, r) && ui.button(ctx.icons.label(&a.icon, &ctx.t(&a.label))).clicked() {
                                    context_action = Some((a.clone(), r.clone()));
                                    ui.close_menu();
                                }
                            }
                        });
                    }
                    ui.end_row();
                }
            });
            ui.add_space(6.0);
        }
    });
    if let Some(id) = clicked {
        state.selected = Some(id);
    }
    if let Some((a, r)) = context_action {
        start_action(ctx, state, &a, r);
    }
    if !detail_side && !l.detail.is_empty() {
        ui.separator();
        let row = state.selected_row(l);
        egui::ScrollArea::vertical().id_salt(("detail-below", &spec.key)).show(ui, |ui| detail(ui, ctx, l, prefix, row.as_ref(), state));
    } else if !detail_side && !l.row_actions.is_empty() {
        if let Some(row) = state.selected_row(l) {
            ui.separator();
            actions_row(ui, ctx, l, &row, state);
        }
    }
}

fn actions_row(ui: &mut egui::Ui, ctx: &mut AppContext, l: &ListSpec, row: &Value, state: &mut ViewState) {
    let mut chosen = None;
    ui.horizontal_wrapped(|ui| {
        for a in &l.row_actions {
            if visible(ctx, a, row) {
                let mut b = ui.button(ctx.icons.label(&a.icon, &ctx.t(&a.label)));
                if let Some(cmd) = &a.command {
                    if let Some(sc) = ctx.shortcuts.text(cmd, german(ctx)) {
                        b = b.on_hover_text(sc);
                    }
                }
                if b.clicked() {
                    chosen = Some(a.clone());
                }
            }
        }
    });
    if let Some(a) = chosen {
        start_action(ctx, state, &a, row.clone());
    }
}

fn detail(ui: &mut egui::Ui, ctx: &mut AppContext, l: &ListSpec, prefix: &str, row: Option<&Value>, state: &mut ViewState) {
    let Some(row) = row else {
        ui.label(egui::RichText::new(ctx.t("shell:list.select")).weak());
        return;
    };
    ui.label(egui::RichText::new(ctx.t("shell:common.details")).strong());
    egui::Grid::new(("detail-grid", &l.endpoint)).num_columns(2).spacing([10.0, 4.0]).show(ui, |ui| {
        for c in &l.detail {
            ui.label(egui::RichText::new(ctx.t(&c.label)).weak());
            cell(ui, ctx, prefix, c, row);
            ui.end_row();
        }
    });
    if !l.row_actions.is_empty() {
        ui.add_space(8.0);
        ui.label(egui::RichText::new(ctx.t("shell:list.actions")).strong());
        let row = row.clone();
        actions_row(ui, ctx, l, &row, state);
    }
}

#[allow(clippy::too_many_arguments)]
fn dashboard(ui: &mut egui::Ui, ctx: &mut AppContext, endpoint: &str, cards: &[StatCard], chart_spec: Option<&ChartSpec>, lists: &[DashboardList], actions: &[ActionSpec], prefix: &str, state: &mut ViewState) {
    if state.dirty && state.loading.is_none() {
        load(ctx, state, endpoint.to_string());
    }
    if let Some(e) = &state.error {
        widgets::error_box(ui, ctx, e);
    }
    let Some(data) = state.data.clone() else {
        widgets::busy(ui, ctx);
        return;
    };
    egui::ScrollArea::vertical().id_salt(("dash", endpoint)).show(ui, |ui| {
        ui.horizontal(|ui| {
            for a in actions {
                if visible(ctx, a, &Value::Null) && ui.button(ctx.icons.label(&a.icon, &ctx.t(&a.label))).clicked() {
                    let a = a.clone();
                    start_action(ctx, state, &a, Value::Null);
                }
            }
            if state.loading.is_some() {
                ui.spinner();
            }
        });
        // Cards: 1–4 per row by width.
        let per_row = match state.size {
            SizeClass::Compact => 1,
            SizeClass::Medium => 2,
            _ => cards.len().clamp(1, 4),
        };
        for chunk in cards.chunks(per_row) {
            ui.columns(per_row, |cols| {
                for (i, c) in chunk.iter().enumerate() {
                    let v = path(&data, &c.value);
                    let text = format_value(v, c.format, None, german(ctx));
                    widgets::card(&mut cols[i], "", |ui| {
                        ui.label(egui::RichText::new(format!("{} {}", ctx.icons.glyph(&c.icon), ctx.t(&c.label))).weak());
                        ui.label(egui::RichText::new(text).size(26.0).strong());
                    });
                    if let Some(nav) = &c.navigate {
                        let r = cols[i].interact(cols[i].min_rect(), cols[i].id().with(("card", i, &c.label)), egui::Sense::click());
                        if r.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                            ctx.open(nav, None);
                        }
                    }
                }
            });
        }
        if let Some(cs) = chart_spec {
            ui.add_space(8.0);
            widgets::card(ui, &ctx.t(&cs.title), |ui| {
                let items = rows(&data, &cs.items);
                let labels: Vec<String> = items.iter().map(|r| format::text(path(r, &cs.label_field))).collect();
                let values: Vec<f64> = items.iter().map(|r| format::as_f64(path(r, &cs.value_field)).unwrap_or(0.0)).collect();
                chart::chart(ui, cs.kind, chart::Series { labels: &labels, values: &values }, cs.format, german(ctx), 220.0);
            });
        }
        for dl in lists {
            ui.add_space(8.0);
            widgets::card(ui, &ctx.t(&dl.title), |ui| {
                let items = rows(&data, &dl.items);
                if items.is_empty() {
                    ui.label(egui::RichText::new(ctx.t("shell:common.empty")).weak());
                }
                egui::Grid::new(("dl", &dl.title)).striped(true).num_columns(dl.columns.len()).show(ui, |ui| {
                    for c in &dl.columns {
                        ui.label(egui::RichText::new(ctx.t(&c.label)).strong());
                    }
                    ui.end_row();
                    for r in &items {
                        for c in &dl.columns {
                            cell(ui, ctx, prefix, c, r);
                        }
                        ui.end_row();
                    }
                });
                if let Some(nav) = &dl.navigate {
                    if ui.link("⏵").clicked() {
                        ctx.open(nav, None);
                    }
                }
            });
        }
    });
}

fn report(ui: &mut egui::Ui, ctx: &mut AppContext, catalog: &str, run: &str, runs: Option<&str>, state: &mut ViewState) {
    let r = &mut state.report;
    if r.reports.is_empty() && r.catalog.is_none() && r.error.is_none() {
        let api = ctx.api.clone();
        let url = catalog.to_string();
        r.catalog = Some(Promise::spawn(async move { api.get::<Vec<Value>>(&url).await }));
    }
    if let Some(res) = poll(&mut r.catalog) {
        match res {
            Ok(v) => r.reports = v,
            Err(e) => r.error = Some(e),
        }
    }
    if let Some(res) = poll(&mut r.running) {
        match res {
            Ok(v) => {
                r.result = Some(v);
                r.error = None;
            }
            Err(e) => match r.form.as_mut() {
                Some((_, f)) => f.show_error(e, ctx),
                None => r.error = Some(e),
            },
        }
    }
    let _ = runs;
    let german = german(ctx);
    egui::SidePanel::left(egui::Id::new(("reports", catalog))).resizable(true).default_width(240.0).show_inside(ui, |ui| {
        let mut category = String::new();
        for rep in r.reports.clone() {
            let cat = format::text(path(&rep, "category"));
            if cat != category {
                ui.label(egui::RichText::new(&cat).weak());
                category = cat;
            }
            let id = format::text(path(&rep, "id"));
            if ui.selectable_label(r.selected.as_deref() == Some(id.as_str()), format::text(path(&rep, "name"))).on_hover_text(format::text(path(&rep, "description"))).clicked() {
                r.selected = Some(id.clone());
                r.result = None;
                let schema: InputSchema = serde_json::from_value(path(&rep, "parameters").clone()).unwrap_or_default();
                r.form = Some((id, FormState::new(&schema, &Map::new())));
            }
        }
        if r.catalog.is_some() {
            ui.spinner();
        }
    });
    if let Some(e) = &r.error {
        widgets::error_box(ui, ctx, e);
    }
    let Some(sel) = r.selected.clone() else {
        ui.label(egui::RichText::new(ctx.t("shell:report.choose")).weak());
        return;
    };
    let Some(rep) = r.reports.iter().find(|x| format::text(path(x, "id")) == sel).cloned() else { return };
    ui.heading(format::text(path(&rep, "name")));
    ui.label(egui::RichText::new(format::text(path(&rep, "description"))).weak());
    let schema: InputSchema = serde_json::from_value(path(&rep, "parameters").clone()).unwrap_or_default();
    if r.form.as_ref().map(|(id, _)| id != &sel).unwrap_or(true) {
        // selected from outside (deep link / notification): build the parameter form
        r.form = Some((sel.clone(), FormState::new(&schema, &Map::new())));
        r.result = None;
    }
    if !schema.fields.is_empty() {
        ui.label(egui::RichText::new(ctx.t("shell:report.parameters")).strong());
        if let Some((_, f)) = r.form.as_mut() {
            form_ui(ui, ctx, &schema, f, state.size.columns(), &sel);
        }
    }
    ui.horizontal(|ui| {
        if ui.add_enabled(r.running.is_none(), egui::Button::new(ctx.icons.label("play", &ctx.t("shell:common.run")))).clicked() {
            let values = match r.form.as_mut() {
                Some((_, f)) => f.collect(&schema, ctx),
                None => Some(Map::new()),
            };
            if let Some(values) = values {
                let api = ctx.api.clone();
                let url = run.replace("{id}", &sel);
                r.running = Some(Promise::spawn(async move { api.post::<Value, _>(&url, &json!({ "parameters": values })).await }));
            }
        }
        if r.running.is_some() {
            ui.spinner();
        }
    });
    let Some(result) = r.result.clone() else { return };
    ui.separator();
    let columns: Vec<(String, String, String)> = path(&result, "columns").as_array().cloned().unwrap_or_default().iter().map(|c| (format::text(path(c, "field")), format::text(path(c, "label")), format::text(path(c, "format")))).collect();
    let rows_v = rows(&result, "rows");
    let label_field = format::text(path(&result, "chart.labelField"));
    let value_field = format::text(path(&result, "chart.valueField"));
    let kind = if format::text(path(&result, "chart.kind")) == "line" { ChartKind::Line } else { ChartKind::Bar };
    let labels: Vec<String> = rows_v.iter().map(|x| format::text(path(x, &label_field))).collect();
    let values: Vec<f64> = rows_v.iter().map(|x| format::as_f64(path(x, &value_field)).unwrap_or(0.0)).collect();
    let fmt_of = |f: &str| match f {
        "money" => ValueFormat::Money,
        "number" => ValueFormat::Number,
        _ => ValueFormat::Text,
    };
    let value_fmt = columns.iter().find(|c| c.0 == value_field).map(|c| fmt_of(&c.2)).unwrap_or(ValueFormat::Number);
    chart::chart(ui, kind, chart::Series { labels: &labels, values: &values }, value_fmt, german, 240.0);
    ui.horizontal(|ui| {
        ui.label(format!("{}: {}", ctx.t("shell:report.total"), format_value(path(&result, "total"), value_fmt, None, german)));
        if ui.button(ctx.t("shell:report.copyCsv")).clicked() {
            let mut csv = columns.iter().map(|c| ctx.t(&c.1)).collect::<Vec<_>>().join(";");
            for x in &rows_v {
                csv.push('\n');
                csv.push_str(&columns.iter().map(|c| format::text(path(x, &c.0)).replace(';', ",")).collect::<Vec<_>>().join(";"));
            }
            ui.ctx().copy_text(csv);
            ctx.toast(ctx.t("shell:common.copied"), Severity::Info);
        }
    });
    egui::ScrollArea::vertical().id_salt(("report-rows", &sel)).show(ui, |ui| {
        egui::Grid::new(("report-grid", &sel)).striped(true).num_columns(columns.len()).show(ui, |ui| {
            for c in &columns {
                ui.label(egui::RichText::new(ctx.t(&c.1)).strong());
            }
            ui.end_row();
            for x in &rows_v {
                for c in &columns {
                    ui.label(format_value(path(x, &c.0), fmt_of(&c.2), None, german));
                }
                ui.end_row();
            }
        });
    });
}

fn form_view(ui: &mut egui::Ui, ctx: &mut AppContext, load_endpoint: Option<&str>, schema: &InputSchema, submit: &ActionSpec, state: &mut ViewState) {
    if let Some(ep) = load_endpoint {
        if state.dirty && state.loading.is_none() {
            load(ctx, state, ep.to_string());
        }
    } else if state.data.is_none() {
        state.data = Some(json!({}));
    }
    let Some(data) = state.data.clone() else {
        widgets::busy(ui, ctx);
        return;
    };
    if state.form.is_none() {
        state.form = Some(FormState::new(schema, data.as_object().unwrap_or(&Map::new())));
    }
    let size = state.size;
    if let Some(f) = state.form.as_mut() {
        form_ui(ui, ctx, schema, f, size.columns(), &submit.id);
    }
    if ui.button(ctx.icons.label(&submit.icon, &ctx.t(&submit.label))).clicked() {
        if let Some(values) = state.form.as_mut().and_then(|f| f.collect(schema, ctx)) {
            let mut d = ActionDialog { action: submit.clone(), row: data, form: None, running: None, result: None };
            run_action(ctx, &mut d, values);
            state.dialog = Some(d);
        }
    }
}

/// The default form schema for an action without one (used by the recorder to describe parameters).
pub fn parameter_schema(fields: &[&str]) -> InputSchema {
    InputSchema { schema_id: "recorded".into(), version: 1, fields: fields.iter().map(|f| InputField { name: f.to_string(), label: f.to_string(), ..Default::default() }).collect() }
}

#[cfg(test)]
mod tests {
    #[test]
    fn new_view_state_loads_on_first_render() {
        assert!(super::ViewState::default().dirty);
        assert!(super::ViewState::new().dirty);
    }
}
