//! View packs – the client part of a module.
//!
//! Golden Hive loaded module clients as .NET assemblies. Native code cannot be unloaded safely in Rust,
//! so a Red Hive module client is **data**: a signed package containing `views.json` (a [`ViewPack`]).
//! The host renders it with its own widgets (lists, forms, dashboards, charts, report runners) and runs its
//! actions against the module's backend. Swapping a module is therefore always safe: the host replaces
//! the pack and rebuilds the open views; nothing has to be unloaded.

use crate::commands::{CommandDescriptor, CommandGroupDescriptor};
use crate::workflows::input::InputSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ViewPack {
    pub module_id: String,
    pub version: String,
    /// `language → (key without prefix → text)`. Keys are looked up as `<prefix>:<key>`.
    #[serde(default)]
    pub texts: BTreeMap<String, BTreeMap<String, String>>,
    /// Module icons: name (without prefix) → glyph/emoji.
    #[serde(default)]
    pub icons: BTreeMap<String, String>,
    #[serde(default)]
    pub groups: Vec<CommandGroupDescriptor>,
    #[serde(default)]
    pub views: Vec<ViewSpec>,
    #[serde(default)]
    pub commands: Vec<CommandSpec>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum ViewRole {
    /// Opens as a tab in the document well.
    #[default]
    Document,
    /// Docks at a side of the window.
    ToolWindow,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum DockSide {
    Left,
    #[default]
    Right,
    Bottom,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ViewSpec {
    /// Navigation key, `<prefix>.<view>` (`documents.invoices`).
    pub key: String,
    pub title: String,
    #[serde(default)]
    pub icon: String,
    #[serde(default)]
    pub role: ViewRole,
    #[serde(default)]
    pub dock: DockSide,
    /// The module's home view (inner ring of the navigation wheel).
    #[serde(default)]
    pub home: bool,
    #[serde(default)]
    pub permission: String,
    /// Event patterns that refresh the view (`documents.*`).
    #[serde(default)]
    pub live_events: Vec<String>,
    #[serde(default)]
    pub order: i32,
    pub body: ViewBody,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase", tag = "kind")]
pub enum ViewBody {
    /// Stat cards, a chart and short lists.
    Dashboard {
        endpoint: String,
        #[serde(default)]
        cards: Vec<StatCard>,
        #[serde(default)]
        chart: Option<ChartSpec>,
        #[serde(default)]
        lists: Vec<DashboardList>,
        #[serde(default)]
        actions: Vec<ActionSpec>,
    },
    /// A searchable, filterable table with row and toolbar actions and a detail pane.
    List(ListSpec),
    /// Report catalogue → parameter form → chart + table (+ CSV copy).
    ReportRunner {
        catalog_endpoint: String,
        /// `{id}` is the report id.
        run_endpoint: String,
        #[serde(default)]
        runs_endpoint: Option<String>,
    },
    /// A single form (settings, wizards).
    Form {
        #[serde(default)]
        load_endpoint: Option<String>,
        schema: InputSchema,
        submit: ActionSpec,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StatCard {
    pub label: String,
    /// Path into the endpoint result (`invoices.submitted`).
    pub value: String,
    #[serde(default)]
    pub format: ValueFormat,
    #[serde(default)]
    pub icon: String,
    /// Navigation target when clicked.
    #[serde(default)]
    pub navigate: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChartSpec {
    pub title: String,
    /// Path of the array in the result.
    pub items: String,
    pub label_field: String,
    pub value_field: String,
    #[serde(default)]
    pub kind: ChartKind,
    #[serde(default)]
    pub format: ValueFormat,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum ChartKind {
    #[default]
    Bar,
    Line,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DashboardList {
    pub title: String,
    pub items: String,
    pub columns: Vec<ColumnSpec>,
    #[serde(default)]
    pub navigate: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum ValueFormat {
    #[default]
    Text,
    Number,
    Money,
    Percent,
    Date,
    DateTime,
    Bool,
    /// Coloured badge (status values).
    Badge,
    Bytes,
    List,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ColumnSpec {
    pub field: String,
    pub label: String,
    #[serde(default)]
    pub format: ValueFormat,
    /// Relative width (default 1).
    #[serde(default = "one")]
    pub width: f32,
    /// Currency field for money values.
    #[serde(default)]
    pub currency_field: Option<String>,
}

fn one() -> f32 {
    1.0
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FilterSpec {
    /// Query parameter.
    pub param: String,
    pub label: String,
    pub options: Vec<crate::workflows::input::ChoiceOption>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListSpec {
    pub endpoint: String,
    /// Path of the item array in the response (`items` for pages; empty = the response is the array).
    #[serde(default)]
    pub items: String,
    pub columns: Vec<ColumnSpec>,
    #[serde(default)]
    pub search: bool,
    #[serde(default)]
    pub filters: Vec<FilterSpec>,
    #[serde(default)]
    pub group_by: Option<String>,
    #[serde(default)]
    pub row_actions: Vec<ActionSpec>,
    #[serde(default)]
    pub toolbar_actions: Vec<ActionSpec>,
    /// Fields shown in the detail pane of the selected row.
    #[serde(default)]
    pub detail: Vec<ColumnSpec>,
    /// Field with the row id (default `id`).
    #[serde(default = "id_field")]
    pub id_field: String,
}

fn id_field() -> String {
    "id".into()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "UPPERCASE")]
pub enum HttpMethod {
    Get,
    #[default]
    Post,
    Put,
    Delete,
}

/// A button: optional form → HTTP call → refresh.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionSpec {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub icon: String,
    #[serde(default)]
    pub permission: String,
    #[serde(default)]
    pub method: HttpMethod,
    /// Endpoint with `{field}` placeholders filled from the row (`/api/documents/invoices/{id}/submit`).
    pub endpoint: String,
    /// Form shown before the call; its values become the JSON body.
    #[serde(default)]
    pub form: Option<InputSchema>,
    /// Form field → expression over `row` (prefill).
    #[serde(default)]
    pub prefill: BTreeMap<String, String>,
    /// Extra body values: name → expression over `row` / `form`.
    #[serde(default)]
    pub body: BTreeMap<String, String>,
    /// Text key of a confirmation question.
    #[serde(default)]
    pub confirm: Option<String>,
    /// Expression over `row` and `user`; the action is hidden when false.
    #[serde(default)]
    pub visible_when: Option<String>,
    /// Text key shown on success.
    #[serde(default)]
    pub success: Option<String>,
    /// Command id this action is registered as (palette, shortcuts, workflow recorder).
    #[serde(default)]
    pub command: Option<String>,
    /// Open the response (a file/text) instead of refreshing.
    #[serde(default)]
    pub show_result: bool,
}

/// A command contributed by a module.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandSpec {
    pub descriptor: CommandDescriptor,
    pub run: CommandRun,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase", tag = "kind")]
pub enum CommandRun {
    /// Open a view.
    Navigate { view: String },
    /// Run an action (form → HTTP).
    Action { action: ActionSpec },
}

impl ViewPack {
    /// All text keys the pack uses (for completeness checks).
    pub fn text_keys(&self) -> Vec<String> {
        let mut keys = Vec::new();
        let mut push = |k: &str| {
            if crate::TextKey::is_key(k) {
                keys.push(k.to_string());
            }
        };
        for v in &self.views {
            push(&v.title);
            match &v.body {
                ViewBody::List(l) => {
                    l.columns.iter().chain(l.detail.iter()).for_each(|c| push(&c.label));
                    l.row_actions.iter().chain(l.toolbar_actions.iter()).for_each(|a| {
                        push(&a.label);
                        a.form.iter().flat_map(|f| f.fields.iter()).for_each(|f| push(&f.label));
                    });
                }
                ViewBody::Dashboard { cards, lists, .. } => {
                    cards.iter().for_each(|c| push(&c.label));
                    lists.iter().for_each(|l| push(&l.title));
                }
                _ => {}
            }
        }
        for c in &self.commands {
            push(&c.descriptor.label.0);
        }
        keys.sort();
        keys.dedup();
        keys
    }

    /// Keys used but missing in a language (`<prefix>:` keys of this module only).
    pub fn missing_texts(&self, prefix: &str) -> Vec<(String, String)> {
        let mut missing = Vec::new();
        for (lang, texts) in &self.texts {
            for k in self.text_keys() {
                if let Some(rest) = k.strip_prefix(&format!("{prefix}:")) {
                    if !texts.contains_key(rest) {
                        missing.push((lang.clone(), k.clone()));
                    }
                }
            }
        }
        missing
    }
}

/// Fills `{field}` placeholders of an endpoint template from a JSON object.
pub fn fill_template(template: &str, row: &serde_json::Value) -> String {
    let mut out = String::new();
    let mut rest = template;
    while let Some(start) = rest.find('{') {
        out.push_str(&rest[..start]);
        match rest[start..].find('}') {
            Some(end) => {
                let field = &rest[start + 1..start + end];
                let v = field.split('.').fold(row.clone(), |cur, seg| cur.get(seg).cloned().unwrap_or(serde_json::Value::Null));
                out.push_str(&match v {
                    serde_json::Value::String(s) => s,
                    serde_json::Value::Null => String::new(),
                    other => other.to_string(),
                });
                rest = &rest[start + end + 1..];
            }
            None => {
                out.push_str(&rest[start..]);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn template() {
        assert_eq!(fill_template("/api/x/{id}/submit?v={meta.rv}", &json!({"id": "a1", "meta": {"rv": 3}})), "/api/x/a1/submit?v=3");
    }

    #[test]
    fn view_body_serde() {
        let v: ViewSpec = serde_json::from_value(json!({
            "key": "documents.invoices", "title": "documents:view.invoices",
            "body": {"kind": "list", "endpoint": "/api/documents/invoices", "items": "items",
                     "columns": [{"field": "number", "label": "documents:field.number"}]}
        }))
        .unwrap();
        assert!(matches!(v.body, ViewBody::List(_)));
    }
}
