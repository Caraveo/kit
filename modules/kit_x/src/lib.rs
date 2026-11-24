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
mod value;

pub use interp::{Host, Interpreter, StdHost, set_random_seed};
pub use parser::parse;
pub use value::{Class, Instance, Value};

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
