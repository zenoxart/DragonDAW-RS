//! Formatting of values for display (numbers, money, dates, bytes, lists) per language.

use redhive_contracts::viewpack::ValueFormat;
use serde_json::Value;

fn group(int: &str, sep: char) -> String {
    let neg = int.starts_with('-');
    let digits: Vec<char> = int.trim_start_matches('-').chars().collect();
    let mut out = String::new();
    for (i, c) in digits.iter().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(sep);
        }
        out.push(*c);
    }
    if neg {
        format!("-{out}")
    } else {
        out
    }
}

pub fn number(n: f64, decimals: usize, german: bool) -> String {
    let s = format!("{:.*}", decimals, n);
    let (int, frac) = s.split_once('.').map(|(a, b)| (a.to_string(), Some(b.to_string()))).unwrap_or((s.clone(), None));
    let (gs, ds) = if german { ('.', ',') } else { (',', '.') };
    match frac {
        Some(f) => format!("{}{}{}", group(&int, gs), ds, f),
        None => group(&int, gs),
    }
}

pub fn as_f64(v: &Value) -> Option<f64> {
    v.as_f64().or_else(|| v.as_str().and_then(|s| s.parse().ok()))
}

/// Text of a value in a format. `german` switches separators and date order.
pub fn format_value(v: &Value, fmt: ValueFormat, currency: Option<&str>, german: bool) -> String {
    if v.is_null() {
        return "–".into();
    }
    match fmt {
        ValueFormat::Number => as_f64(v).map(|n| number(n, if n.fract() == 0.0 { 0 } else { 2 }, german)).unwrap_or_else(|| text(v)),
        ValueFormat::Money => as_f64(v).map(|n| format!("{} {}", number(n, 2, german), currency.unwrap_or(if german { "€" } else { "EUR" }))).unwrap_or_else(|| text(v)),
        ValueFormat::Percent => as_f64(v).map(|n| format!("{} %", number(n * 100.0, 1, german))).unwrap_or_else(|| text(v)),
        ValueFormat::Date => {
            let s = text(v);
            match chrono::NaiveDate::parse_from_str(s.get(..10).unwrap_or(&s), "%Y-%m-%d") {
                Ok(d) => d.format(if german { "%d.%m.%Y" } else { "%Y-%m-%d" }).to_string(),
                Err(_) => s,
            }
        }
        ValueFormat::DateTime => {
            let s = text(v);
            match chrono::DateTime::parse_from_rfc3339(&s) {
                Ok(d) => d.with_timezone(&chrono::Local).format(if german { "%d.%m.%Y %H:%M" } else { "%Y-%m-%d %H:%M" }).to_string(),
                Err(_) => s,
            }
        }
        ValueFormat::Bool => match v {
            Value::Bool(true) => "✔".into(),
            Value::Bool(false) => "✖".into(),
            other => text(other),
        },
        ValueFormat::Bytes => as_f64(v)
            .map(|b| {
                let units = ["B", "KB", "MB", "GB"];
                let mut x = b;
                let mut u = 0;
                while x >= 1024.0 && u < 3 {
                    x /= 1024.0;
                    u += 1;
                }
                format!("{} {}", number(x, if u == 0 { 0 } else { 1 }, german), units[u])
            })
            .unwrap_or_else(|| text(v)),
        ValueFormat::List => match v {
            Value::Array(a) => a.iter().map(text).collect::<Vec<_>>().join(", "),
            other => text(other),
        },
        ValueFormat::Text | ValueFormat::Badge => text(v),
    }
}

pub fn text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        Value::Array(a) => a.iter().map(text).collect::<Vec<_>>().join(", "),
        other => other.to_string(),
    }
}

/// Reads `a.b.c` from a JSON value (empty path = the value itself).
pub fn path<'a>(v: &'a Value, p: &str) -> &'a Value {
    if p.is_empty() {
        return v;
    }
    p.split('.').fold(v, |cur, seg| cur.get(seg).unwrap_or(&Value::Null))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn formats() {
        assert_eq!(format_value(&json!(1234567.5), ValueFormat::Money, Some("EUR"), true), "1.234.567,50 EUR");
        assert_eq!(format_value(&json!(1234567.5), ValueFormat::Money, Some("EUR"), false), "1,234,567.50 EUR");
        assert_eq!(format_value(&json!("2026-11-30"), ValueFormat::Date, None, true), "30.11.2026");
        assert_eq!(format_value(&json!(1536), ValueFormat::Bytes, None, false), "1.5 KB");
        assert_eq!(format_value(&json!(["a", "b"]), ValueFormat::List, None, false), "a, b");
        assert_eq!(format_value(&json!(-1200), ValueFormat::Number, None, false), "-1,200");
        assert_eq!(path(&json!({"a": {"b": 3}}), "a.b"), &json!(3));
    }
}
