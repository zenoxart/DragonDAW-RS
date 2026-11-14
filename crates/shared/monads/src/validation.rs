use crate::{Error, Res};

/// A value or **all** the errors found while building it.
#[derive(Debug, Clone, PartialEq)]
pub enum Validation<T> {
    Valid(T),
    Invalid(Vec<Error>),
}

impl<T> Validation<T> {
    pub fn map<U>(self, f: impl FnOnce(T) -> U) -> Validation<U> {
        match self {
            Validation::Valid(v) => Validation::Valid(f(v)),
            Validation::Invalid(e) => Validation::Invalid(e),
        }
    }

    /// Combines two validations; errors of both sides are kept.
    pub fn and<U>(self, other: Validation<U>) -> Validation<(T, U)> {
        match (self, other) {
            (Validation::Valid(a), Validation::Valid(b)) => Validation::Valid((a, b)),
            (Validation::Invalid(mut a), Validation::Invalid(b)) => {
                a.extend(b);
                Validation::Invalid(a)
            }
            (Validation::Invalid(a), _) | (_, Validation::Invalid(a)) => Validation::Invalid(a),
        }
    }

    /// Converts to a [`Res`]; several errors are merged into one validation error.
    pub fn into_result(self) -> Res<T> {
        match self {
            Validation::Valid(v) => Ok(v),
            Validation::Invalid(mut e) if e.len() == 1 => Err(e.remove(0)),
            Validation::Invalid(e) => Err(Error::merge(e)),
        }
    }
}

impl<T> From<Res<T>> for Validation<T> {
    fn from(r: Res<T>) -> Self {
        match r {
            Ok(v) => Validation::Valid(v),
            Err(e) => Validation::Invalid(vec![e]),
        }
    }
}

/// Collects field checks for a request; ends with [`Validator::finish`].
///
/// ```
/// use redhive_monads::Validator;
/// let name = "";
/// let amount = -1.0;
/// let r = Validator::new("invoice")
///     .require("supplier", !name.is_empty())
///     .check("amount", amount >= 0.0, "negative")
///     .finish(());
/// assert!(r.is_err());
/// ```
#[derive(Debug, Default)]
pub struct Validator {
    scope: String,
    errors: Vec<Error>,
}

impl Validator {
    /// `scope` is the text scope used for generated keys (`<scope>:<field>.<code>`).
    pub fn new(scope: impl Into<String>) -> Self {
        Validator { scope: scope.into(), errors: Vec::new() }
    }

    /// A required field (`errors:required`).
    pub fn require(mut self, field: &str, present: bool) -> Self {
        if !present {
            self.errors.push(Error::validation(format!("{field}.required"), "errors:required").for_field(field));
        }
        self
    }

    /// A rule on a field; the text key is `<scope>:<field>.<code>`.
    pub fn check(mut self, field: &str, ok: bool, code: &str) -> Self {
        if !ok {
            let key = format!("{}:{}.{}", self.scope, field, code);
            self.errors.push(Error::validation(format!("{}.{}", field, code), key).for_field(field));
        }
        self
    }

    /// Adds an error built elsewhere.
    pub fn push(mut self, error: Error) -> Self {
        self.errors.push(error);
        self
    }

    pub fn is_valid(&self) -> bool {
        self.errors.is_empty()
    }

    /// Returns `value` when no rule failed.
    pub fn finish<T>(self, value: T) -> Res<T> {
        if self.errors.is_empty() {
            Ok(value)
        } else {
            Validation::<T>::Invalid(self.errors).into_result()
        }
    }
}
