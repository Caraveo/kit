//! Runtime values.

use crate::ast::{ClassDecl, FuncDecl, Type};
use std::cell::RefCell;
use std::fmt;
use std::rc::Rc;

/// A runtime class (a declaration plus its resolved parent).
#[derive(Debug)]
pub struct Class {
    pub decl: Rc<ClassDecl>,
    pub parent: Option<Rc<Class>>,
}

impl Class {
    pub fn name(&self) -> &str {
        &self.decl.name
    }

    /// Find a method, searching parents (RIDE) as needed.
    pub fn method(&self, name: &str) -> Option<Rc<FuncDecl>> {
        self.decl
            .methods
            .iter()
            .find(|m| &*m.name == name)
            .cloned()
            .or_else(|| self.parent.as_ref().and_then(|p| p.method(name)))
    }

    /// A field's access, from the most derived class that declares it.
    pub fn field_access(&self, name: &str) -> crate::ast::Access {
        match self.decl.fields.iter().find(|f| &*f.name == name) {
            Some(f) => f.access,
            None => self.parent.as_ref().map_or(crate::ast::Access::Private, |p| p.field_access(name)),
        }
    }

    pub fn is_a(&self, name: &str) -> bool {
        self.name() == name || self.parent.as_ref().is_some_and(|p| p.is_a(name))
    }
}

/// An object created from a class.
#[derive(Debug)]
pub struct Instance {
    pub class: Rc<Class>,
    pub fields: RefCell<Vec<(Rc<str>, Type, Value)>>,
}

impl Instance {
    pub fn get(&self, name: &str) -> Option<Value> {
        self.fields.borrow().iter().find(|(n, _, _)| &**n == name).map(|(_, _, v)| v.clone())
    }
    pub fn field_type(&self, name: &str) -> Option<Type> {
        self.fields.borrow().iter().find(|(n, _, _)| &**n == name).map(|(_, t, _)| t.clone())
    }
    /// Set without type checking (the interpreter checks before calling this).
    pub fn set_raw(&self, name: &str, v: Value) -> bool {
        let mut f = self.fields.borrow_mut();
        match f.iter_mut().find(|(n, _, _)| &**n == name) {
            Some(slot) => {
                slot.2 = v;
                true
            }
            None => false,
        }
    }
    /// PUBLIC and EDIT fields with their current values, in declaration
    /// order (for the editor inspector). Private fields are left out.
    pub fn snapshot(&self) -> Vec<(String, Type, crate::ast::Access, Value)> {
        self.fields
            .borrow()
            .iter()
            .filter_map(|(n, t, v)| {
                let access = self.class.field_access(n);
                access.is_public().then(|| (n.to_string(), t.clone(), access, v.clone()))
            })
            .collect()
    }
}

/// Every value an x program can hold.
#[derive(Clone, Debug, Default)]
pub enum Value {
    #[default]
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(Rc<str>),
    Vector([f64; 3]),
    List(Rc<RefCell<Vec<Value>>>),
    Object(Rc<RefCell<Vec<(Rc<str>, Value)>>>),
    Instance(Rc<Instance>),
    Func(Rc<FuncDecl>),
    Method(Rc<Instance>, Rc<FuncDecl>),
    Class(Rc<Class>),
    /// An engine or built-in module brought in with `TAKE`.
    Module(Rc<str>),
    /// A built-in function such as `RANGE`.
    Builtin(&'static str),
    /// A feature (component) on an object, from `GetFeature`: one of its
    /// scripts or an engine feature such as `Model`. Members go to the host.
    Feature(Rc<FeatureRef>),
}

/// Which object and which of its features a [`Value::Feature`] refers to.
#[derive(Debug, Clone, PartialEq)]
pub struct FeatureRef {
    /// The object's id (as the host hands them out).
    pub target: i64,
    /// `"Model"`, `"Physics"`, … or a script's class name.
    pub name: Rc<str>,
}

impl Value {
    pub fn str(s: &str) -> Value {
        Value::Str(s.into())
    }
    pub fn list(v: Vec<Value>) -> Value {
        Value::List(Rc::new(RefCell::new(v)))
    }
    pub fn object(v: Vec<(&str, Value)>) -> Value {
        Value::Object(Rc::new(RefCell::new(v.into_iter().map(|(k, v)| (Rc::from(k), v)).collect())))
    }
    pub fn vector(x: f64, y: f64, z: f64) -> Value {
        Value::Vector([x, y, z])
    }

    pub fn type_name(&self) -> String {
        match self {
            Value::Null => "NULL".into(),
            Value::Bool(_) => "BOOL".into(),
            Value::Int(_) => "INTEGER".into(),
            Value::Float(_) => "FLOAT".into(),
            Value::Str(_) => "STRING".into(),
            Value::Vector(_) => "VECTOR".into(),
            Value::List(_) => "INDEX".into(),
            Value::Object(_) => "OBJECT".into(),
            Value::Instance(i) => i.class.name().to_string(),
            Value::Func(_) | Value::Method(..) | Value::Builtin(_) => "FUNCTION".into(),
            Value::Class(_) => "CLASS".into(),
            Value::Module(_) => "MODULE".into(),
            Value::Feature(f) => f.name.to_string(),
        }
    }

    pub fn truthy(&self) -> bool {
        match self {
            Value::Null => false,
            Value::Bool(b) => *b,
            Value::Int(i) => *i != 0,
            Value::Float(f) => *f != 0.0,
            Value::Str(s) => !s.is_empty(),
            _ => true,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Int(i) => Some(*i as f64),
            Value::Float(f) => Some(*f),
            _ => None,
        }
    }

    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Value::Int(i) => Some(*i),
            Value::Float(f) if f.fract() == 0.0 => Some(*f as i64),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_vector(&self) -> Option<[f64; 3]> {
        match self {
            Value::Vector(v) => Some(*v),
            _ => None,
        }
    }

    /// Equality: by value for primitives, by identity for reference types.
    pub fn equals(&self, other: &Value) -> bool {
        match (self, other) {
            (Value::Null, Value::Null) => true,
            (Value::Bool(a), Value::Bool(b)) => a == b,
            (Value::Str(a), Value::Str(b)) => a == b,
            (Value::Vector(a), Value::Vector(b)) => a == b,
            (Value::List(a), Value::List(b)) => Rc::ptr_eq(a, b),
            (Value::Object(a), Value::Object(b)) => Rc::ptr_eq(a, b),
            (Value::Instance(a), Value::Instance(b)) => Rc::ptr_eq(a, b),
            (Value::Class(a), Value::Class(b)) => Rc::ptr_eq(a, b),
            (Value::Module(a), Value::Module(b)) => a == b,
            (Value::Feature(a), Value::Feature(b)) => a == b,
            (a, b) => match (a.as_f64(), b.as_f64()) {
                (Some(x), Some(y)) => x == y,
                _ => false,
            },
        }
    }
}

fn fmt_float(f: f64, out: &mut fmt::Formatter<'_>) -> fmt::Result {
    if f.is_finite() && f.fract() == 0.0 && f.abs() < 1e15 { write!(out, "{f:.1}") } else { write!(out, "{f}") }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Null => write!(f, "NULL"),
            Value::Bool(b) => write!(f, "{}", if *b { "TRUE" } else { "FALSE" }),
            Value::Int(i) => write!(f, "{i}"),
            Value::Float(x) => fmt_float(*x, f),
            Value::Str(s) => write!(f, "{s}"),
            Value::Vector([x, y, z]) => {
                write!(f, "(")?;
                fmt_float(*x, f)?;
                write!(f, ", ")?;
                fmt_float(*y, f)?;
                write!(f, ", ")?;
                fmt_float(*z, f)?;
                write!(f, ")")
            }
            Value::List(l) => {
                write!(f, "[")?;
                for (i, v) in l.borrow().iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    match v {
                        Value::Str(s) => write!(f, "\"{s}\"")?,
                        other => write!(f, "{other}")?,
                    }
                }
                write!(f, "]")
            }
            Value::Object(o) => {
                write!(f, "{{")?;
                for (i, (k, v)) in o.borrow().iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    match v {
                        Value::Str(s) => write!(f, "\"{k}\": \"{s}\"")?,
                        other => write!(f, "\"{k}\": {other}")?,
                    }
                }
                write!(f, "}}")
            }
            Value::Instance(i) => write!(f, "<{}>", i.class.name()),
            Value::Func(func) | Value::Method(_, func) => write!(f, "<function {}>", func.name),
            Value::Class(c) => write!(f, "<class {}>", c.name()),
            Value::Module(m) => write!(f, "<module {m}>"),
            Value::Feature(r) => write!(f, "<{} feature of {}>", r.name, r.target),
            Value::Builtin(b) => write!(f, "<builtin {b}>"),
        }
    }
}
