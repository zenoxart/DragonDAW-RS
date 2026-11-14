//! Input schemas: forms owned by modules, rendered by the client, validated on both sides.

use redhive_monads::{Error, Res};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Number, Value};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct InputSchema {
    pub schema_id: String,
    #[serde(default = "one")]
    pub version: u32,
    #[serde(default)]
    pub fields: Vec<InputField>,
}

fn one() -> u32 {
    1
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum FieldType {
    #[default]
    Text,
    MultiLine,
    Number,
    Money,
    Boolean,
    Date,
    Choice,
    /// A reference to an entity; options come from a lookup endpoint.
    EntityRef,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum Span {
    #[default]
    Full,
    Half,
    Third,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChoiceOption {
    pub value: String,
    /// Text key or literal.
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct InputField {
    pub name: String,
    #[serde(rename = "type", default)]
    pub field_type: FieldType,
    /// Text key or literal.
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub help: Option<String>,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub default: Option<Value>,
    #[serde(default)]
    pub min: Option<f64>,
    #[serde(default)]
    pub max: Option<f64>,
    #[serde(default)]
    pub max_length: Option<usize>,
    #[serde(default)]
    pub pattern: Option<String>,
    #[serde(default)]
    pub options: Vec<ChoiceOption>,
    /// Lookup endpoint for `entityRef` fields (returns `[{value, label}]`).
    #[serde(default)]
    pub lookup: Option<String>,
    /// Masked in logs and the AI prompt.
    #[serde(default)]
    pub sensitive: bool,
    #[serde(default)]
    pub span: Span,
    #[serde(default)]
    pub read_only: bool,
}

impl InputSchema {
    pub fn field(&self, name: &str) -> Option<&InputField> {
        self.fields.iter().find(|f| f.name == name)
    }

    /// Validates and normalizes `values`: applies defaults, converts numbers/booleans given as text,
    /// checks required, ranges, lengths, patterns and choices. All field errors are returned together.
    pub fn validate(&self, values: &Map<String, Value>) -> Res<Map<String, Value>> {
        let mut out = Map::new();
        let mut errors: Vec<Error> = Vec::new();
        for f in &self.fields {
            let raw = values.get(&f.name).cloned().filter(|v| !is_blank(v)).or_else(|| f.default.clone());
            let Some(raw) = raw else {
                if f.required {
                    errors.push(Error::validation(format!("{}.required", f.name), "errors:required").for_field(&f.name));
                }
                continue;
            };
            match normalize(f, raw) {
                Ok(v) => {
                    out.insert(f.name.clone(), v);
                }
                Err(e) => errors.push(e),
            }
        }
        // Unknown fields are passed through untouched (forward compatibility).
        for (k, v) in values {
            if !out.contains_key(k) && self.field(k).is_none() {
                out.insert(k.clone(), v.clone());
            }
        }
        match errors.len() {
            0 => Ok(out),
            1 => Err(errors.remove(0)),
            _ => Err(Error::merge(errors)),
        }
    }
}

fn is_blank(v: &Value) -> bool {
    match v {
        Value::Null => true,
        Value::String(s) => s.trim().is_empty(),
        _ => false,
    }
}

fn field_error(f: &InputField, code: &str) -> Error {
    Error::validation(format!("{}.{}", f.name, code), format!("errors:{code}")).for_field(&f.name)
}

fn normalize(f: &InputField, raw: Value) -> Res<Value> {
    match f.field_type {
        FieldType::Number | FieldType::Money => {
            let n = match &raw {
                Value::Number(n) => n.as_f64(),
                Value::String(s) => s.trim().replace(',', ".").parse::<f64>().ok(),
                _ => None,
            }
            .ok_or_else(|| field_error(f, "number"))?;
            if let Some(min) = f.min {
                if n < min {
                    return Err(field_error(f, "min").with_metadata("min", min));
                }
            }
            if let Some(max) = f.max {
                if n > max {
                    return Err(field_error(f, "max").with_metadata("max", max));
                }
            }
            let rounded = if f.field_type == FieldType::Money { (n * 100.0).round() / 100.0 } else { n };
            Ok(if rounded.fract() == 0.0 && rounded.abs() < 9.0e15 {
                Value::Number(Number::from(rounded as i64))
            } else {
                Number::from_f64(rounded).map(Value::Number).unwrap_or(Value::Null)
            })
        }
        FieldType::Boolean => match &raw {
            Value::Bool(b) => Ok(Value::Bool(*b)),
            Value::String(s) if matches!(s.to_ascii_lowercase().as_str(), "true" | "yes" | "1" | "ja") => Ok(Value::Bool(true)),
            Value::String(s) if matches!(s.to_ascii_lowercase().as_str(), "false" | "no" | "0" | "nein") => Ok(Value::Bool(false)),
            Value::Number(n) => Ok(Value::Bool(n.as_f64() != Some(0.0))),
            _ => Err(field_error(f, "boolean")),
        },
        FieldType::Date => {
            let s = text_of(&raw);
            let date_part = s.get(..10).unwrap_or(&s);
            chrono::NaiveDate::parse_from_str(date_part, "%Y-%m-%d").map_err(|_| field_error(f, "date"))?;
            Ok(Value::String(date_part.to_string()))
        }
        FieldType::Choice => {
            let s = text_of(&raw);
            if !f.options.is_empty() && !f.options.iter().any(|o| o.value == s) {
                return Err(field_error(f, "choice").with_metadata("value", s));
            }
            Ok(Value::String(s))
        }
        FieldType::Text | FieldType::MultiLine | FieldType::EntityRef => {
            let s = text_of(&raw);
            if let Some(max) = f.max_length {
                if s.chars().count() > max {
                    return Err(field_error(f, "maxLength").with_metadata("maxLength", max as u64));
                }
            }
            if let Some(p) = &f.pattern {
                let re = regex_lite::Regex::new(p).map_err(|_| field_error(f, "pattern"))?;
                if !re.is_match(&s) {
                    return Err(field_error(f, "pattern"));
                }
            }
            Ok(Value::String(s))
        }
    }
}

fn text_of(v: &Value) -> String {
    match v {
        Value::String(s) => s.trim().to_string(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn schema() -> InputSchema {
        serde_json::from_value(json!({
            "schemaId": "t", "fields": [
                {"name": "amount", "type": "money", "required": true, "min": 0},
                {"name": "decision", "type": "choice", "required": true, "options": [{"value": "approve", "label": "A"}, {"value": "reject", "label": "R"}]},
                {"name": "user", "type": "text", "pattern": "^[a-z][a-z0-9.\\-]{2,40}$"},
                {"name": "due", "type": "date"},
                {"name": "flag", "type": "boolean", "default": false}
            ]}))
        .unwrap()
    }

    #[test]
    fn normalizes_values() {
        let v = schema().validate(json!({"amount": "12,345", "decision": "approve", "due": "2026-11-30T00:00:00Z", "user": "maria.huber"}).as_object().unwrap()).unwrap();
        assert_eq!(v["amount"], json!(12.35));
        assert_eq!(v["due"], json!("2026-11-30"));
        assert_eq!(v["flag"], json!(false));
    }

    #[test]
    fn collects_all_errors() {
        let e = schema().validate(json!({"amount": -1, "decision": "maybe", "user": "X"}).as_object().unwrap()).unwrap_err();
        let fields: Vec<_> = e.field_errors.iter().map(|f| f.field.as_str()).collect();
        assert_eq!(fields, vec!["amount", "decision", "user"]);
    }
}
