//! The Red Hive expression language – small, safe and side-effect free.
//!
//! Used for trigger filters, transition conditions, parameter and payload mappings.
//!
//! * **Values:** numbers, `'text'` / `"text"`, `true`, `false`, `null`, paths.
//! * **Paths:** `vars.x`, `payload.a.b`, `steps.<stepId>.<output>`, `last.x`, `trigger.x`, `user.x`, `items[0]`.
//! * **Operators:** `== != < <= > >=`, `&& || !`, `+ - * / %` (`+` concatenates text).
//! * **Functions:** `coalesce`, `isEmpty`, `len`, `lower`, `upper`, `trim`, `contains`, `startsWith`,
//!   `endsWith`, `round`, `abs`, `min`, `max`, `number`, `text`, `if`, `split`, `join`.
//!
//! There are no assignments, loops or calls into the host: evaluating an expression can never change state.

use redhive_monads::{Error, Res};
use serde_json::{Map, Number, Value};
use std::collections::BTreeSet;
use std::fmt;

/// Roots a path may start with.
pub const ROOTS: &[&str] = &["vars", "payload", "steps", "last", "trigger", "user"];

/// Functions the language knows, with (min, max) arity.
pub const FUNCTIONS: &[(&str, usize, usize)] = &[
    ("coalesce", 1, 16),
    ("isEmpty", 1, 1),
    ("len", 1, 1),
    ("lower", 1, 1),
    ("upper", 1, 1),
    ("trim", 1, 1),
    ("contains", 2, 2),
    ("startsWith", 2, 2),
    ("endsWith", 2, 2),
    ("round", 1, 2),
    ("abs", 1, 1),
    ("min", 1, 16),
    ("max", 1, 16),
    ("number", 1, 1),
    ("text", 1, 1),
    ("if", 3, 3),
    ("split", 2, 2),
    ("join", 2, 2),
];

/// One path segment.
#[derive(Debug, Clone, PartialEq)]
pub enum Segment {
    Field(String),
    Index(usize),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Literal(Value),
    Path(Vec<Segment>),
    Unary(UnaryOp, Box<Expr>),
    Binary(BinaryOp, Box<Expr>, Box<Expr>),
    Call(String, Vec<Expr>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    Not,
    Neg,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOp {
    Or,
    And,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    Add,
    Sub,
    Mul,
    Div,
    Rem,
}

impl BinaryOp {
    fn symbol(self) -> &'static str {
        match self {
            BinaryOp::Or => "||",
            BinaryOp::And => "&&",
            BinaryOp::Eq => "==",
            BinaryOp::Ne => "!=",
            BinaryOp::Lt => "<",
            BinaryOp::Le => "<=",
            BinaryOp::Gt => ">",
            BinaryOp::Ge => ">=",
            BinaryOp::Add => "+",
            BinaryOp::Sub => "-",
            BinaryOp::Mul => "*",
            BinaryOp::Div => "/",
            BinaryOp::Rem => "%",
        }
    }
    fn precedence(self) -> u8 {
        match self {
            BinaryOp::Or => 1,
            BinaryOp::And => 2,
            BinaryOp::Eq | BinaryOp::Ne => 3,
            BinaryOp::Lt | BinaryOp::Le | BinaryOp::Gt | BinaryOp::Ge => 4,
            BinaryOp::Add | BinaryOp::Sub => 5,
            BinaryOp::Mul | BinaryOp::Div | BinaryOp::Rem => 6,
        }
    }
}

// ------------------------------------------------------------------ lexer

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Num(f64),
    Str(String),
    Ident(String),
    Op(&'static str),
    LParen,
    RParen,
    LBracket,
    RBracket,
    Comma,
    Dot,
}

fn syntax(code: &str, pos: usize, message: impl Into<String>) -> Error {
    Error::validation(format!("expression.{code}"), "workflow:expression.invalid").with_message(message.into()).with_metadata("position", pos as u64)
}

fn lex(src: &str) -> Res<Vec<(Tok, usize)>> {
    let chars: Vec<char> = src.chars().collect();
    let mut i = 0;
    let mut out = Vec::new();
    while i < chars.len() {
        let c = chars[i];
        let start = i;
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        if c.is_ascii_digit() {
            while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
                // a dot followed by a non-digit ends the number (path access is not allowed on numbers)
                if chars[i] == '.' && !(i + 1 < chars.len() && chars[i + 1].is_ascii_digit()) {
                    break;
                }
                i += 1;
            }
            let text: String = chars[start..i].iter().collect();
            let n = text.parse::<f64>().map_err(|_| syntax("number", start, format!("invalid number '{text}'")))?;
            out.push((Tok::Num(n), start));
            continue;
        }
        if c == '\'' || c == '"' {
            let quote = c;
            i += 1;
            let mut s = String::new();
            loop {
                if i >= chars.len() {
                    return Err(syntax("string", start, "unterminated text"));
                }
                let ch = chars[i];
                if ch == '\\' && i + 1 < chars.len() {
                    s.push(match chars[i + 1] {
                        'n' => '\n',
                        't' => '\t',
                        other => other,
                    });
                    i += 2;
                    continue;
                }
                if ch == quote {
                    i += 1;
                    break;
                }
                s.push(ch);
                i += 1;
            }
            out.push((Tok::Str(s), start));
            continue;
        }
        if c.is_alphabetic() || c == '_' || c == '$' {
            while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_' || chars[i] == '$' || chars[i] == '-') {
                i += 1;
            }
            out.push((Tok::Ident(chars[start..i].iter().collect()), start));
            continue;
        }
        let two: String = chars[i..(i + 2).min(chars.len())].iter().collect();
        let op2 = ["==", "!=", "<=", ">=", "&&", "||"].iter().find(|o| **o == two);
        if let Some(op) = op2 {
            out.push((Tok::Op(op), start));
            i += 2;
            continue;
        }
        let tok = match c {
            '<' => Tok::Op("<"),
            '>' => Tok::Op(">"),
            '!' => Tok::Op("!"),
            '+' => Tok::Op("+"),
            '-' => Tok::Op("-"),
            '*' => Tok::Op("*"),
            '/' => Tok::Op("/"),
            '%' => Tok::Op("%"),
            '(' => Tok::LParen,
            ')' => Tok::RParen,
            '[' => Tok::LBracket,
            ']' => Tok::RBracket,
            ',' => Tok::Comma,
            '.' => Tok::Dot,
            '=' => return Err(syntax("assignment", start, "assignments are not allowed – use '=='")),
            other => return Err(syntax("character", start, format!("unexpected character '{other}'"))),
        };
        out.push((tok, start));
        i += 1;
    }
    Ok(out)
}

// ------------------------------------------------------------------ parser (precedence climbing)

struct Parser {
    toks: Vec<(Tok, usize)>,
    pos: usize,
    len: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.pos).map(|t| &t.0)
    }
    fn at(&self) -> usize {
        self.toks.get(self.pos).map(|t| t.1).unwrap_or(self.len)
    }
    fn next(&mut self) -> Option<Tok> {
        let t = self.toks.get(self.pos).map(|t| t.0.clone());
        self.pos += 1;
        t
    }

    fn binary_op(&self) -> Option<BinaryOp> {
        match self.peek()? {
            Tok::Op(o) => Some(match *o {
                "||" => BinaryOp::Or,
                "&&" => BinaryOp::And,
                "==" => BinaryOp::Eq,
                "!=" => BinaryOp::Ne,
                "<" => BinaryOp::Lt,
                "<=" => BinaryOp::Le,
                ">" => BinaryOp::Gt,
                ">=" => BinaryOp::Ge,
                "+" => BinaryOp::Add,
                "-" => BinaryOp::Sub,
                "*" => BinaryOp::Mul,
                "/" => BinaryOp::Div,
                "%" => BinaryOp::Rem,
                _ => return None,
            }),
            _ => None,
        }
    }

    fn expr(&mut self, min_prec: u8) -> Res<Expr> {
        let mut lhs = self.unary()?;
        while let Some(op) = self.binary_op() {
            let prec = op.precedence();
            if prec < min_prec {
                break;
            }
            self.next();
            let rhs = self.expr(prec + 1)?;
            lhs = Expr::Binary(op, Box::new(lhs), Box::new(rhs));
        }
        Ok(lhs)
    }

    fn unary(&mut self) -> Res<Expr> {
        match self.peek() {
            Some(Tok::Op("!")) => {
                self.next();
                Ok(Expr::Unary(UnaryOp::Not, Box::new(self.unary()?)))
            }
            Some(Tok::Op("-")) => {
                self.next();
                Ok(Expr::Unary(UnaryOp::Neg, Box::new(self.unary()?)))
            }
            _ => self.primary(),
        }
    }

    fn primary(&mut self) -> Res<Expr> {
        let at = self.at();
        match self.next() {
            Some(Tok::Num(n)) => Ok(Expr::Literal(num(n))),
            Some(Tok::Str(s)) => Ok(Expr::Literal(Value::String(s))),
            Some(Tok::LParen) => {
                let e = self.expr(0)?;
                match self.next() {
                    Some(Tok::RParen) => Ok(e),
                    _ => Err(syntax("paren", self.at(), "missing ')'")),
                }
            }
            Some(Tok::Ident(name)) => match name.as_str() {
                "true" => Ok(Expr::Literal(Value::Bool(true))),
                "false" => Ok(Expr::Literal(Value::Bool(false))),
                "null" => Ok(Expr::Literal(Value::Null)),
                _ if self.peek() == Some(&Tok::LParen) => {
                    self.next();
                    let mut args = Vec::new();
                    if self.peek() != Some(&Tok::RParen) {
                        loop {
                            args.push(self.expr(0)?);
                            match self.next() {
                                Some(Tok::Comma) => continue,
                                Some(Tok::RParen) => break,
                                _ => return Err(syntax("call", self.at(), format!("missing ')' after arguments of {name}"))),
                            }
                        }
                    } else {
                        self.next();
                    }
                    let Some((_, lo, hi)) = FUNCTIONS.iter().find(|(f, _, _)| *f == name) else {
                        return Err(syntax("function", at, format!("unknown function '{name}'")).with_metadata("function", name));
                    };
                    if args.len() < *lo || args.len() > *hi {
                        return Err(syntax("arity", at, format!("{name} takes {lo}..{hi} arguments")));
                    }
                    Ok(Expr::Call(name, args))
                }
                _ => {
                    let mut segs = vec![Segment::Field(name)];
                    loop {
                        match self.peek() {
                            Some(Tok::Dot) => {
                                self.next();
                                match self.next() {
                                    Some(Tok::Ident(f)) => segs.push(Segment::Field(f)),
                                    Some(Tok::Num(n)) if n.fract() == 0.0 => segs.push(Segment::Field(format!("{}", n as i64))),
                                    _ => return Err(syntax("path", self.at(), "expected a name after '.'")),
                                }
                            }
                            Some(Tok::LBracket) => {
                                self.next();
                                match (self.next(), self.next()) {
                                    (Some(Tok::Num(n)), Some(Tok::RBracket)) if n >= 0.0 && n.fract() == 0.0 => segs.push(Segment::Index(n as usize)),
                                    (Some(Tok::Str(s)), Some(Tok::RBracket)) => segs.push(Segment::Field(s)),
                                    _ => return Err(syntax("index", self.at(), "expected [number] or ['name']")),
                                }
                            }
                            _ => break,
                        }
                    }
                    Ok(Expr::Path(segs))
                }
            },
            Some(t) => Err(syntax("token", at, format!("unexpected {t:?}"))),
            None => Err(syntax("end", at, "unexpected end of expression")),
        }
    }
}

fn num(n: f64) -> Value {
    if n.fract() == 0.0 && n.abs() < 9.0e15 {
        Value::Number(Number::from(n as i64))
    } else {
        Number::from_f64(n).map(Value::Number).unwrap_or(Value::Null)
    }
}

/// Parses an expression.
pub fn parse(src: &str) -> Res<Expr> {
    let toks = lex(src)?;
    if toks.is_empty() {
        return Err(syntax("empty", 0, "empty expression"));
    }
    let mut p = Parser { toks, pos: 0, len: src.len() };
    let e = p.expr(0)?;
    if p.pos < p.toks.len() {
        return Err(syntax("trailing", p.at(), "unexpected text after the expression"));
    }
    Ok(e)
}

/// Parses and checks roots (`vars`, `payload`, …). Returns the paths used.
pub fn validate(src: &str) -> Res<Expr> {
    let e = parse(src)?;
    let mut bad = None;
    e.visit_paths(&mut |segs| {
        if let Some(Segment::Field(root)) = segs.first() {
            if !ROOTS.contains(&root.as_str()) && bad.is_none() {
                bad = Some(root.clone());
            }
        }
    });
    match bad {
        Some(root) => Err(Error::validation("expression.root", "workflow:expression.unknownRoot").with_metadata("root", root).with_message(format!("in '{src}'"))),
        None => Ok(e),
    }
}

// ------------------------------------------------------------------ printing & rewriting

impl fmt::Display for Expr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.write(f, 0)
    }
}

impl Expr {
    fn write(&self, f: &mut fmt::Formatter<'_>, parent: u8) -> fmt::Result {
        match self {
            Expr::Literal(Value::String(s)) => write!(f, "'{}'", s.replace('\\', "\\\\").replace('\'', "\\'")),
            Expr::Literal(v) => write!(f, "{v}"),
            Expr::Path(segs) => {
                for (i, s) in segs.iter().enumerate() {
                    match s {
                        Segment::Field(name) if i == 0 => write!(f, "{name}")?,
                        Segment::Field(name) if is_ident(name) => write!(f, ".{name}")?,
                        Segment::Field(name) => write!(f, "['{name}']")?,
                        Segment::Index(n) => write!(f, "[{n}]")?,
                    }
                }
                Ok(())
            }
            Expr::Unary(UnaryOp::Not, e) => {
                write!(f, "!")?;
                e.write(f, 7)
            }
            Expr::Unary(UnaryOp::Neg, e) => {
                write!(f, "-")?;
                e.write(f, 7)
            }
            Expr::Binary(op, l, r) => {
                let p = op.precedence();
                if p < parent {
                    write!(f, "(")?;
                }
                l.write(f, p)?;
                write!(f, " {} ", op.symbol())?;
                r.write(f, p + 1)?;
                if p < parent {
                    write!(f, ")")?;
                }
                Ok(())
            }
            Expr::Call(name, args) => {
                write!(f, "{name}(")?;
                for (i, a) in args.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    a.write(f, 0)?;
                }
                write!(f, ")")
            }
        }
    }

    /// Calls `f` for every path in the expression.
    pub fn visit_paths(&self, f: &mut impl FnMut(&[Segment])) {
        match self {
            Expr::Path(segs) => f(segs),
            Expr::Unary(_, e) => e.visit_paths(f),
            Expr::Binary(_, l, r) => {
                l.visit_paths(f);
                r.visit_paths(f);
            }
            Expr::Call(_, args) => args.iter().for_each(|a| a.visit_paths(f)),
            Expr::Literal(_) => {}
        }
    }

    /// Rewrites every path with `f` (used by migrations).
    pub fn map_paths(&mut self, f: &mut impl FnMut(&mut Vec<Segment>)) {
        match self {
            Expr::Path(segs) => f(segs),
            Expr::Unary(_, e) => e.map_paths(f),
            Expr::Binary(_, l, r) => {
                l.map_paths(f);
                r.map_paths(f);
            }
            Expr::Call(_, args) => args.iter_mut().for_each(|a| a.map_paths(f)),
            Expr::Literal(_) => {}
        }
    }

    /// Dotted text of all paths (`vars.amount`, `steps.lookup.supplier`).
    pub fn paths(&self) -> BTreeSet<String> {
        let mut out = BTreeSet::new();
        self.visit_paths(&mut |segs| {
            out.insert(segs.iter().map(|s| match s { Segment::Field(n) => n.clone(), Segment::Index(i) => i.to_string() }).collect::<Vec<_>>().join("."));
        });
        out
    }
}

fn is_ident(s: &str) -> bool {
    let mut c = s.chars();
    matches!(c.next(), Some(ch) if ch.is_alphabetic() || ch == '_' || ch == '$') && c.all(|ch| ch.is_alphanumeric() || ch == '_' || ch == '$' || ch == '-')
}

/// Renames the first path segment after `root.` (e.g. `vars.qty` → `vars.amount`) in an expression text.
/// Returns the rewritten text (unchanged when nothing matched or the text does not parse).
pub fn rename_in(src: &str, root: &str, prefix: &[&str], from: &str, to: &str) -> String {
    let Ok(mut e) = parse(src) else { return src.to_string() };
    let mut changed = false;
    e.map_paths(&mut |segs| {
        if segs.len() > prefix.len() + 1 && matches!(&segs[0], Segment::Field(r) if r == root) {
            let prefix_ok = prefix.iter().enumerate().all(|(i, p)| matches!(&segs[i + 1], Segment::Field(s) if s == p));
            let idx = prefix.len() + 1;
            if prefix_ok && matches!(&segs[idx], Segment::Field(s) if s == from) {
                segs[idx] = Segment::Field(to.to_string());
                changed = true;
            }
        }
    });
    if changed {
        e.to_string()
    } else {
        src.to_string()
    }
}

// ------------------------------------------------------------------ evaluation

/// The values an expression can read.
#[derive(Debug, Clone, Default)]
pub struct Scope {
    pub roots: Map<String, Value>,
}

impl Scope {
    pub fn new() -> Self {
        Scope::default()
    }
    pub fn with(mut self, root: &str, value: Value) -> Self {
        self.roots.insert(root.to_string(), value);
        self
    }
    pub fn set(&mut self, root: &str, value: Value) {
        self.roots.insert(root.to_string(), value);
    }
}

/// Parses and evaluates.
pub fn eval_str(src: &str, scope: &Scope) -> Res<Value> {
    parse(src)?.eval(scope)
}

/// Evaluates a condition; empty text is `true`.
pub fn condition(src: &str, scope: &Scope) -> Res<bool> {
    if src.trim().is_empty() {
        return Ok(true);
    }
    Ok(truthy(&eval_str(src, scope)?))
}

pub fn truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|f| f != 0.0),
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}

fn as_num(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse::<f64>().ok(),
        Value::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
        _ => None,
    }
}

fn as_text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

fn loose_eq(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(_), Value::String(_)) | (Value::String(_), Value::Number(_)) => as_num(a).zip(as_num(b)).is_some_and(|(x, y)| x == y),
        (Value::Number(x), Value::Number(y)) => x.as_f64() == y.as_f64(),
        _ => a == b,
    }
}

fn compare(a: &Value, b: &Value) -> Option<std::cmp::Ordering> {
    match (as_num(a), as_num(b)) {
        (Some(x), Some(y)) if !(a.is_string() && b.is_string()) => x.partial_cmp(&y),
        _ => match (a, b) {
            (Value::String(x), Value::String(y)) => Some(x.cmp(y)),
            (Value::Null, _) | (_, Value::Null) => None,
            _ => None,
        },
    }
}

fn eval_err(code: &str, message: impl Into<String>) -> Error {
    Error::validation(format!("expression.{code}"), "workflow:expression.evaluation").with_message(message.into())
}

impl Expr {
    pub fn eval(&self, scope: &Scope) -> Res<Value> {
        match self {
            Expr::Literal(v) => Ok(v.clone()),
            Expr::Path(segs) => {
                let Some(Segment::Field(root)) = segs.first() else { return Ok(Value::Null) };
                let mut cur = scope.roots.get(root).cloned().unwrap_or(Value::Null);
                for s in &segs[1..] {
                    cur = match (s, cur) {
                        (Segment::Field(f), Value::Object(mut o)) => o.remove(f).unwrap_or(Value::Null),
                        (Segment::Index(i), Value::Array(mut a)) if *i < a.len() => a.swap_remove(*i),
                        (Segment::Field(f), Value::Array(mut a)) => match f.parse::<usize>() {
                            Ok(i) if i < a.len() => a.swap_remove(i),
                            _ => Value::Null,
                        },
                        _ => Value::Null,
                    };
                }
                Ok(cur)
            }
            Expr::Unary(UnaryOp::Not, e) => Ok(Value::Bool(!truthy(&e.eval(scope)?))),
            Expr::Unary(UnaryOp::Neg, e) => {
                let v = e.eval(scope)?;
                as_num(&v).map(|n| num(-n)).ok_or_else(|| eval_err("type", "'-' needs a number"))
            }
            Expr::Binary(BinaryOp::And, l, r) => Ok(Value::Bool(truthy(&l.eval(scope)?) && truthy(&r.eval(scope)?))),
            Expr::Binary(BinaryOp::Or, l, r) => {
                let lv = l.eval(scope)?;
                Ok(Value::Bool(truthy(&lv) || truthy(&r.eval(scope)?)))
            }
            Expr::Binary(op, l, r) => {
                let a = l.eval(scope)?;
                let b = r.eval(scope)?;
                Ok(match op {
                    BinaryOp::Eq => Value::Bool(loose_eq(&a, &b)),
                    BinaryOp::Ne => Value::Bool(!loose_eq(&a, &b)),
                    BinaryOp::Lt => Value::Bool(compare(&a, &b) == Some(std::cmp::Ordering::Less)),
                    BinaryOp::Le => Value::Bool(matches!(compare(&a, &b), Some(std::cmp::Ordering::Less | std::cmp::Ordering::Equal))),
                    BinaryOp::Gt => Value::Bool(compare(&a, &b) == Some(std::cmp::Ordering::Greater)),
                    BinaryOp::Ge => Value::Bool(matches!(compare(&a, &b), Some(std::cmp::Ordering::Greater | std::cmp::Ordering::Equal))),
                    BinaryOp::Add => match (&a, &b) {
                        (Value::Number(_), Value::Number(_)) => num(as_num(&a).unwrap_or(0.0) + as_num(&b).unwrap_or(0.0)),
                        _ => Value::String(as_text(&a) + &as_text(&b)),
                    },
                    BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div | BinaryOp::Rem => {
                        let (x, y) = as_num(&a).zip(as_num(&b)).ok_or_else(|| eval_err("type", format!("'{}' needs numbers", op.symbol())))?;
                        match op {
                            BinaryOp::Sub => num(x - y),
                            BinaryOp::Mul => num(x * y),
                            BinaryOp::Div if y == 0.0 => return Err(eval_err("divideByZero", "division by zero")),
                            BinaryOp::Div => num(x / y),
                            _ if y == 0.0 => return Err(eval_err("divideByZero", "division by zero")),
                            _ => num(x % y),
                        }
                    }
                    BinaryOp::And | BinaryOp::Or => unreachable!(),
                })
            }
            Expr::Call(name, args) => call(name, args, scope),
        }
    }
}

fn call(name: &str, args: &[Expr], scope: &Scope) -> Res<Value> {
    if name == "if" {
        return if truthy(&args[0].eval(scope)?) { args[1].eval(scope) } else { args[2].eval(scope) };
    }
    if name == "coalesce" {
        for a in args {
            let v = a.eval(scope)?;
            if !matches!(&v, Value::Null) && !matches!(&v, Value::String(s) if s.is_empty()) {
                return Ok(v);
            }
        }
        return Ok(Value::Null);
    }
    let vals: Vec<Value> = args.iter().map(|a| a.eval(scope)).collect::<Res<_>>()?;
    let text = |i: usize| as_text(&vals[i]);
    Ok(match name {
        "isEmpty" => Value::Bool(!truthy(&vals[0]) && !matches!(vals[0], Value::Bool(false) | Value::Number(_))),
        "len" => num(match &vals[0] {
            Value::String(s) => s.chars().count() as f64,
            Value::Array(a) => a.len() as f64,
            Value::Object(o) => o.len() as f64,
            Value::Null => 0.0,
            other => as_text(other).chars().count() as f64,
        }),
        "lower" => Value::String(text(0).to_lowercase()),
        "upper" => Value::String(text(0).to_uppercase()),
        "trim" => Value::String(text(0).trim().to_string()),
        "contains" => Value::Bool(match &vals[0] {
            Value::Array(a) => a.iter().any(|x| loose_eq(x, &vals[1])),
            _ => text(0).to_lowercase().contains(&text(1).to_lowercase()),
        }),
        "startsWith" => Value::Bool(text(0).to_lowercase().starts_with(&text(1).to_lowercase())),
        "endsWith" => Value::Bool(text(0).to_lowercase().ends_with(&text(1).to_lowercase())),
        "round" => {
            let x = as_num(&vals[0]).ok_or_else(|| eval_err("type", "round needs a number"))?;
            let digits = vals.get(1).and_then(as_num).unwrap_or(0.0).clamp(0.0, 10.0) as i32;
            let f = 10f64.powi(digits);
            num((x * f).round() / f)
        }
        "abs" => num(as_num(&vals[0]).ok_or_else(|| eval_err("type", "abs needs a number"))?.abs()),
        "min" | "max" => {
            let nums: Vec<f64> = flatten(&vals).iter().filter_map(as_num).collect();
            if nums.is_empty() {
                Value::Null
            } else if name == "min" {
                num(nums.iter().cloned().fold(f64::INFINITY, f64::min))
            } else {
                num(nums.iter().cloned().fold(f64::NEG_INFINITY, f64::max))
            }
        }
        "number" => as_num(&vals[0]).map(num).unwrap_or(Value::Null),
        "text" => Value::String(text(0)),
        "split" => Value::Array(text(0).split(text(1).as_str()).map(str::trim).filter(|p| !p.is_empty()).map(|p| Value::String(p.to_string())).collect()),
        "join" => Value::String(match &vals[0] {
            Value::Array(a) => a.iter().map(as_text).collect::<Vec<_>>().join(&text(1)),
            other => as_text(other),
        }),
        other => return Err(eval_err("function", format!("unknown function '{other}'"))),
    })
}

fn flatten(vals: &[Value]) -> Vec<Value> {
    vals.iter().flat_map(|v| match v {
        Value::Array(a) => a.clone(),
        other => vec![other.clone()],
    }).collect()
}

/// Evaluates a mapping `{name: expression}` into an object.
pub fn eval_mapping(mapping: &std::collections::BTreeMap<String, String>, scope: &Scope) -> Res<Map<String, Value>> {
    let mut out = Map::new();
    for (k, src) in mapping {
        out.insert(k.clone(), eval_str(src, scope)?);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn scope() -> Scope {
        Scope::new()
            .with("vars", json!({"amount": 12500, "note": "", "supplier": "ACME GmbH", "tags": ["a", "b"]}))
            .with("payload", json!({"invoiceId": "x1", "category": "invoice", "quantity": 3, "reorderLevel": 5}))
            .with("last", json!({"decision": "approve"}))
            .with("steps", json!({"lookup": {"supplier": "ACME"}}))
    }

    #[test]
    fn evaluates() {
        let s = scope();
        assert_eq!(eval_str("vars.amount > 10000 && payload.category == 'invoice'", &s).unwrap(), json!(true));
        assert_eq!(eval_str("payload.reorderLevel * 2", &s).unwrap(), json!(10));
        assert_eq!(eval_str("coalesce(vars.note, 'Approved in workflow')", &s).unwrap(), json!("Approved in workflow"));
        assert_eq!(eval_str("upper(steps.lookup.supplier) + '!'", &s).unwrap(), json!("ACME!"));
        assert_eq!(eval_str("round(vars.amount / 3, 2)", &s).unwrap(), json!(4166.67));
        assert_eq!(eval_str("contains(vars.tags, 'b')", &s).unwrap(), json!(true));
        assert_eq!(eval_str("if(len(vars.tags) > 1, 'many', 'few')", &s).unwrap(), json!("many"));
        assert_eq!(eval_str("vars.tags[1]", &s).unwrap(), json!("b"));
        assert_eq!(eval_str("split('Clerk, Approver ,', ',')", &s).unwrap(), json!(["Clerk", "Approver"]));
        assert_eq!(eval_str("join(vars.tags, ' + ')", &s).unwrap(), json!("a + b"));
        assert_eq!(eval_str("!isEmpty(vars.supplier) && vars.missing == null", &s).unwrap(), json!(true));
        assert_eq!(eval_str("max(1, 7, 3) - min(vars.amount, 2)", &s).unwrap(), json!(5));
        assert!(condition("", &s).unwrap());
        assert!(condition("last.decision == 'approve'", &s).unwrap());
    }

    #[test]
    fn rejects_bad_input() {
        assert!(parse("vars.a = 1").is_err());
        assert!(parse("system('rm')").is_err());
        assert!(parse("(1 + 2").is_err());
        assert!(validate("env.PATH == ''").is_err());
        assert!(eval_str("1 / 0", &Scope::new()).is_err());
    }

    #[test]
    fn prints_and_renames() {
        let e = parse("(vars.a + 1) * 2 == payload['x y'] || !last.ok").unwrap();
        let printed = e.to_string();
        assert_eq!(parse(&printed).unwrap(), e);
        assert_eq!(rename_in("vars.qty * 2 > steps.order.qty", "vars", &[], "qty", "amount"), "vars.amount * 2 > steps.order.qty");
        assert_eq!(rename_in("steps.order.qty", "steps", &["order"], "qty", "amount"), "steps.order.amount");
        assert_eq!(rename_in("vars.other", "vars", &[], "qty", "amount"), "vars.other");
    }

    #[test]
    fn paths_collected() {
        let p = parse("coalesce(vars.note, steps.clarify.comment)").unwrap().paths();
        assert!(p.contains("vars.note") && p.contains("steps.clarify.comment"));
    }
}
