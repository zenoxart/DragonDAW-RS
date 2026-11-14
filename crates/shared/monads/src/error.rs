use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::fmt;

/// The category of a failure. Decides the HTTP status and how callers (UI, workflow engine, AI agent) react.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ErrorKind {
    /// The input is wrong – show it to the user (400).
    Validation,
    /// The entity does not exist (404).
    NotFound,
    /// The state does not allow the operation, e.g. a stale row version (409).
    Conflict,
    /// The user is signed in but lacks a permission (403).
    Forbidden,
    /// No or an invalid sign-in (401).
    Unauthorized,
    /// A dependency is down – retry later (503).
    Unavailable,
    /// A dependency did not answer in time – retry later (504).
    Timeout,
    /// A bug (500).
    Unexpected,
}

impl ErrorKind {
    /// The HTTP status code of this kind.
    pub fn status(self) -> u16 {
        match self {
            ErrorKind::Validation => 400,
            ErrorKind::Unauthorized => 401,
            ErrorKind::Forbidden => 403,
            ErrorKind::NotFound => 404,
            ErrorKind::Conflict => 409,
            ErrorKind::Unavailable => 503,
            ErrorKind::Timeout => 504,
            ErrorKind::Unexpected => 500,
        }
    }

    /// The kind for an HTTP status code (used by API clients).
    pub fn from_status(status: u16) -> Self {
        match status {
            400 | 422 => ErrorKind::Validation,
            401 => ErrorKind::Unauthorized,
            403 => ErrorKind::Forbidden,
            404 => ErrorKind::NotFound,
            409 | 412 => ErrorKind::Conflict,
            408 | 504 => ErrorKind::Timeout,
            429 | 502 | 503 => ErrorKind::Unavailable,
            _ => ErrorKind::Unexpected,
        }
    }

    /// Whether a retry may succeed (the workflow engine retries these).
    pub fn is_transient(self) -> bool {
        matches!(self, ErrorKind::Unavailable | ErrorKind::Timeout)
    }
}

/// One invalid field of a request or form.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FieldError {
    pub field: String,
    pub code: String,
    pub text_key: String,
}

/// A failure as data.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Error {
    pub kind: ErrorKind,
    /// Stable machine-readable code, e.g. `documents.invoice.limitExceeded`.
    pub code: String,
    /// Translatable text, `scope:key`, e.g. `documents:invoice.limitExceeded`.
    pub text_key: String,
    /// Technical detail for logs (never shown as the user-facing message).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub field_errors: Vec<FieldError>,
    /// Values for text placeholders (`{limit}`) and diagnostics.
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    pub metadata: Map<String, Value>,
}

impl Error {
    pub fn new(kind: ErrorKind, code: impl Into<String>, text_key: impl Into<String>) -> Self {
        Error { kind, code: code.into(), text_key: text_key.into(), message: None, field_errors: Vec::new(), metadata: Map::new() }
    }

    pub fn validation(code: impl Into<String>, text_key: impl Into<String>) -> Self {
        Self::new(ErrorKind::Validation, code, text_key)
    }
    pub fn not_found(code: impl Into<String>, text_key: impl Into<String>) -> Self {
        Self::new(ErrorKind::NotFound, code, text_key)
    }
    pub fn conflict(code: impl Into<String>, text_key: impl Into<String>) -> Self {
        Self::new(ErrorKind::Conflict, code, text_key)
    }
    pub fn forbidden(code: impl Into<String>, text_key: impl Into<String>) -> Self {
        Self::new(ErrorKind::Forbidden, code, text_key)
    }
    pub fn unauthorized(code: impl Into<String>, text_key: impl Into<String>) -> Self {
        Self::new(ErrorKind::Unauthorized, code, text_key)
    }
    pub fn unavailable(code: impl Into<String>, text_key: impl Into<String>) -> Self {
        Self::new(ErrorKind::Unavailable, code, text_key)
    }
    pub fn timeout(code: impl Into<String>, text_key: impl Into<String>) -> Self {
        Self::new(ErrorKind::Timeout, code, text_key)
    }
    pub fn unexpected(code: impl Into<String>, text_key: impl Into<String>) -> Self {
        Self::new(ErrorKind::Unexpected, code, text_key)
    }

    /// Adds a technical message.
    pub fn with_message(mut self, message: impl Into<String>) -> Self {
        self.message = Some(message.into());
        self
    }

    /// Adds a metadata value (also usable as a text placeholder).
    pub fn with_metadata(mut self, key: impl Into<String>, value: impl Into<Value>) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }

    /// Marks this error as belonging to one field (adds a field error with the same code/text).
    pub fn for_field(mut self, field: impl Into<String>) -> Self {
        self.field_errors.push(FieldError { field: field.into(), code: self.code.clone(), text_key: self.text_key.clone() });
        self
    }

    /// Merges several errors into one validation error carrying all field errors.
    pub fn merge(errors: Vec<Error>) -> Self {
        let mut merged = Error::validation("validation.failed", "errors:validationFailed");
        for e in errors {
            if e.field_errors.is_empty() {
                merged.field_errors.push(FieldError { field: String::new(), code: e.code, text_key: e.text_key });
            } else {
                merged.field_errors.extend(e.field_errors);
            }
        }
        merged
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?} {} ({})", self.kind, self.code, self.text_key)?;
        if let Some(m) = &self.message {
            write!(f, ": {m}")?;
        }
        Ok(())
    }
}

impl std::error::Error for Error {}

impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        Error::validation("json.invalid", "errors:invalidJson").with_message(e.to_string())
    }
}
