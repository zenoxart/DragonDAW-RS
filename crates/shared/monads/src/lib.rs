//! # Red Hive monads
//!
//! The functional core shared by every Red Hive crate. Expected failures are values, never panics:
//!
//! * [`Error`] – a failure as data: [`ErrorKind`], a stable machine code, a translatable text key,
//!   field errors and metadata. It travels unchanged from the database layer to the HTTP mapper and
//!   back into the client, where the text key is translated.
//! * [`Res<T>`] – `Result<T, Error>`; railway pipelines are written with `?`, [`ResultExt`] adds the
//!   combinators that `std` lacks (`ensure`, `tap`, `recover`, …).
//! * [`Validation<T>`] – like `Result`, but collects **all** errors (for requests and smart constructors).
//! * [`OptionExt`] – `Option` → `Result` with a `NotFound` error.
//! * [`traverse`] / [`combine`] – collections of results.

mod error;
mod validation;

pub use error::{Error, ErrorKind, FieldError};
pub use validation::{Validation, Validator};

/// The result type of every Red Hive operation.
pub type Res<T> = Result<T, Error>;

/// Railway combinators on [`Result`] that `std` does not provide.
pub trait ResultExt<T, E>: Sized {
    /// Keeps the value when `predicate` holds, otherwise fails with `error`.
    fn ensure(self, predicate: impl FnOnce(&T) -> bool, error: impl FnOnce(&T) -> E) -> Result<T, E>;
    /// Runs a side effect on success and passes the value on.
    fn tap(self, f: impl FnOnce(&T)) -> Result<T, E>;
    /// Runs a side effect on failure and passes the error on.
    fn tap_err(self, f: impl FnOnce(&E)) -> Result<T, E>;
    /// Turns selected failures back into a success.
    fn recover(self, f: impl FnOnce(E) -> Result<T, E>) -> Result<T, E>;
    /// Discards the value.
    fn unit(self) -> Result<(), E>;
}

impl<T, E> ResultExt<T, E> for Result<T, E> {
    fn ensure(self, predicate: impl FnOnce(&T) -> bool, error: impl FnOnce(&T) -> E) -> Result<T, E> {
        match self {
            Ok(v) if predicate(&v) => Ok(v),
            Ok(v) => Err(error(&v)),
            Err(e) => Err(e),
        }
    }

    fn tap(self, f: impl FnOnce(&T)) -> Result<T, E> {
        if let Ok(v) = &self {
            f(v);
        }
        self
    }

    fn tap_err(self, f: impl FnOnce(&E)) -> Result<T, E> {
        if let Err(e) = &self {
            f(e);
        }
        self
    }

    fn recover(self, f: impl FnOnce(E) -> Result<T, E>) -> Result<T, E> {
        match self {
            Ok(v) => Ok(v),
            Err(e) => f(e),
        }
    }

    fn unit(self) -> Result<(), E> {
        self.map(|_| ())
    }
}

/// `Option` → `Result` conversions.
pub trait OptionExt<T> {
    /// `None` becomes `Error::not_found(code, text_key)`.
    fn ok_or_not_found(self, code: &str, text_key: &str) -> Res<T>;
}

impl<T> OptionExt<T> for Option<T> {
    fn ok_or_not_found(self, code: &str, text_key: &str) -> Res<T> {
        self.ok_or_else(|| Error::not_found(code, text_key))
    }
}

/// Applies `f` to every item and stops at the first failure.
pub fn traverse<T, U>(items: impl IntoIterator<Item = T>, f: impl FnMut(T) -> Res<U>) -> Res<Vec<U>> {
    items.into_iter().map(f).collect()
}

/// Collects a list of results; all failures are merged into one validation error.
pub fn combine<T>(results: impl IntoIterator<Item = Res<T>>) -> Res<Vec<T>> {
    let mut values = Vec::new();
    let mut errors = Vec::new();
    for r in results {
        match r {
            Ok(v) => values.push(v),
            Err(e) => errors.push(e),
        }
    }
    match errors.len() {
        0 => Ok(values),
        1 => Err(errors.remove(0)),
        _ => Err(Error::merge(errors)),
    }
}

/// Runs a fallible adapter call (database, I/O) and turns any `std::error::Error` into an
/// `Unexpected`/`Unavailable` [`Error`]. Use only at the edges of the system.
pub fn try_<T, E: std::fmt::Display>(code: &str, f: impl FnOnce() -> Result<T, E>) -> Res<T> {
    f().map_err(|e| Error::unexpected(code, "errors:unexpected").with_message(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ensure_and_recover() {
        let r: Res<i32> = Ok(5);
        let failed = r.ensure(|v| *v > 10, |_| Error::validation("too.small", "errors:tooSmall"));
        assert_eq!(failed.as_ref().unwrap_err().code, "too.small");
        let recovered = failed.recover(|e| if e.kind == ErrorKind::Validation { Ok(10) } else { Err(e) });
        assert_eq!(recovered.unwrap(), 10);
    }

    #[test]
    fn combine_merges_errors() {
        let all = combine(vec![Ok(1), Err(Error::validation("a", "x:a").for_field("a")), Err(Error::validation("b", "x:b").for_field("b"))]);
        let e = all.unwrap_err();
        assert_eq!(e.kind, ErrorKind::Validation);
        assert_eq!(e.field_errors.len(), 2);
    }

    #[test]
    fn option_to_not_found() {
        let none: Option<u8> = None;
        assert_eq!(none.ok_or_not_found("x.missing", "x:missing").unwrap_err().kind, ErrorKind::NotFound);
    }
}
