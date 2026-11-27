//! # kit_x — the x scripting language
//!
//! x (eXtensible / X++, <https://github.com/Caraveo/x>) is a C-like hybrid with
//! ALL-CAPS types and keywords, `[ ]` blocks, classes and methods, and module
//! access through `TAKE`.
//!
//! ```x
//! TAKE Transform, Input
//!
//! CREATE Player[
//!     FLOAT Speed: 2.0
//!
//!     VOID CREATE Update(FLOAT dt)[
//!         IF(Input.Down("Space"))[
//!             Transform.Move(0.0, Speed * dt, 0.0)
//!         ]
//!     ]
//! ]
//! ```
//!
//! This crate is engine-agnostic: the engine exposes modules through the [`Host`] trait.

pub mod ast;
mod interp;
pub mod lexer;
mod parser;
mod rotation;
mod value;

pub use interp::{Coroutine, Host, Interpreter, StdHost, set_random_seed};
pub use parser::parse;
pub use value::{Class, FeatureRef, Instance, Value};

use std::fmt;

/// A syntax or runtime error with its location.
#[derive(Clone, Debug, PartialEq)]
pub struct XError {
    pub message: String,
    /// 1-based line (0 when unknown).
    pub line: u32,
    /// 1-based column (0 when unknown).
    pub col: u32,
    /// Script path, filled in by the interpreter.
    pub path: String,
}

impl XError {
    pub fn at(line: u32, col: u32, message: impl Into<String>) -> Self {
        Self { message: message.into(), line, col, path: String::new() }
    }
}

impl fmt::Display for XError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if !self.path.is_empty() {
            write!(f, "{}:", self.path)?;
        }
        match (self.line, self.col) {
            (0, _) => write!(f, " {}", self.message),
            (l, 0) => write!(f, "{l}: {}", self.message),
            (l, c) => write!(f, "{l}:{c}: {}", self.message),
        }
    }
}

impl std::error::Error for XError {}

/// Parse and run a whole program (top-level statements), returning the interpreter.
pub fn run(path: &str, src: &str, host: &mut dyn Host) -> Result<Interpreter, XError> {
    let program = parse(src).map_err(|e| XError { path: path.to_string(), ..e })?;
    let mut interp = Interpreter::new(path);
    interp.load(&program, host)?;
    Ok(interp)
}

#[cfg(test)]
mod tests;

/// A PUBLIC or EDIT field as declared in source (for the editor, before Play).
#[derive(Clone, Debug)]
pub struct DeclaredField {
    pub name: String,
    pub ty: ast::Type,
    pub access: ast::Access,
    /// The initializer when it's a plain literal (`5.0`, `"hi"`, `VECTOR(0, 1, 0)`),
    /// else the type's default.
    pub default: Value,
}

/// The class a script file attaches as: the one named like the file
/// (`player.x` → `Player`, any case), else the first alphabetically.
pub fn main_class<'a>(program: &'a ast::Program, file_stem: &str) -> Option<&'a ast::ClassDecl> {
    let mut classes: Vec<&ast::ClassDecl> =
        program.stmts.iter().filter_map(|s| if let ast::Stmt::Class(c) = s { Some(&**c) } else { None }).collect();
    classes.sort_by(|a, b| a.name.cmp(&b.name));
    let stem = file_stem.to_lowercase();
    classes.iter().find(|c| c.name.to_lowercase() == stem).or(classes.first()).copied()
}

/// PUBLIC and EDIT fields of a script's main class (inherited ones first,
/// for parents declared in the same file). Private fields are left out.
pub fn declared_fields(program: &ast::Program, file_stem: &str) -> Vec<DeclaredField> {
    let Some(class) = main_class(program, file_stem) else { return Vec::new() };
    let find = |name: &str| {
        program.stmts.iter().find_map(|s| match s {
            ast::Stmt::Class(c) if &*c.name == name => Some(c.clone()),
            _ => None,
        })
    };
    let mut chain = vec![class];
    let mut owned = Vec::new();
    let mut parent = class.parent.clone();
    while let Some(p) = parent {
        let Some(c) = find(&p) else { break };
        if owned.len() > 32 {
            break;
        }
        parent = c.parent.clone();
        owned.push(c);
    }
    chain.extend(owned.iter().map(|c| &**c));
    let mut out: Vec<DeclaredField> = Vec::new();
    for c in chain.iter().rev() {
        for f in &c.fields {
            let field = DeclaredField {
                name: f.name.to_string(),
                ty: f.ty.clone(),
                access: f.access,
                default: f.init.as_ref().and_then(|e| literal(e, &f.ty)).unwrap_or_else(|| interp::default_for(&f.ty)),
            };
            match out.iter_mut().find(|o| o.name == field.name) {
                Some(slot) => *slot = field,
                None => out.push(field),
            }
        }
    }
    out.retain(|f| f.access.is_public());
    out
}

/// A constant initializer's value, converted to the field's type.
fn literal(e: &ast::Expr, ty: &ast::Type) -> Option<Value> {
    use ast::{ExprKind as K, Type as T};
    let num = |e: &ast::Expr| -> Option<f64> {
        match &e.kind {
            K::Int(i) => Some(*i as f64),
            K::Float(f) => Some(*f),
            K::Unary(ast::UnOp::Neg, inner) => match inner.kind {
                K::Int(i) => Some(-(i as f64)),
                K::Float(f) => Some(-f),
                _ => None,
            },
            _ => None,
        }
    };
    match (ty, &e.kind) {
        (T::Float, _) => num(e).map(Value::Float),
        (T::Integer, K::Int(i)) => Some(Value::Int(*i)),
        (T::Integer, K::Unary(ast::UnOp::Neg, inner)) => match inner.kind {
            K::Int(i) => Some(Value::Int(-i)),
            _ => None,
        },
        (T::Bool, K::Bool(b)) => Some(Value::Bool(*b)),
        (T::String, K::Str(s)) => Some(Value::Str(s.clone())),
        (T::Vector, K::Convert(T::Vector, args)) if args.len() == 3 => {
            Some(Value::Vector([num(&args[0])?, num(&args[1])?, num(&args[2])?]))
        }
        _ => None,
    }
}
