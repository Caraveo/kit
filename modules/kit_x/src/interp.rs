//! Tree-walking interpreter for x.

use crate::XError;
use crate::ast::*;
use crate::value::{Class, Instance, Value};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

/// The bridge between a running script and whatever embeds it (the engine, a CLI, tests).
pub trait Host {
    /// Is `name` an engine module a script may `TAKE`?
    fn has_module(&self, name: &str) -> bool;
    /// Call `Module.Member(args)`. Property reads (`Time.Delta`) arrive with no args;
    /// property writes (`Transform.Position: v`) arrive as `SetPosition(v)`.
    fn call_module(&mut self, module: &str, member: &str, args: &[Value]) -> Result<Value, String>;
    /// Output from `DISPLAY[...]`.
    fn display(&mut self, text: &str);
    /// Source of another script for `TAKE "file.x"`.
    fn load_source(&mut self, _path: &str) -> Option<String> {
        None
    }
}

/// A host with no engine modules that prints `DISPLAY` output to stdout.
#[derive(Default)]
pub struct StdHost {
    /// Captured output lines (also printed).
    pub output: Vec<String>,
    pub quiet: bool,
}

impl Host for StdHost {
    fn has_module(&self, _: &str) -> bool {
        false
    }
    fn call_module(&mut self, module: &str, _: &str, _: &[Value]) -> Result<Value, String> {
        Err(format!("module `{module}` is not available"))
    }
    fn display(&mut self, text: &str) {
        if !self.quiet {
            println!("{text}");
        }
        self.output.push(text.to_string());
    }
    fn load_source(&mut self, path: &str) -> Option<String> {
        std::fs::read_to_string(path).ok()
    }
}

/// Modules implemented by the interpreter itself.
const INTERNAL_MODULES: &[&str] = &["Math"];
const BUILTINS: &[&str] = &["RANGE", "LENGTH", "TYPEOF"];

#[derive(Clone)]
struct Var {
    ty: Type,
    value: Value,
}

struct Frame {
    scopes: Vec<Vec<(Rc<str>, Var)>>,
    this: Option<Rc<Instance>>,
}

impl Frame {
    fn top() -> Self {
        Frame { scopes: Vec::new(), this: None }
    }
    fn is_top_level(&self) -> bool {
        self.scopes.is_empty() && self.this.is_none()
    }
    fn find(&mut self, name: &str) -> Option<&mut Var> {
        self.scopes.iter_mut().rev().find_map(|s| s.iter_mut().find(|(n, _)| &**n == name).map(|(_, v)| v))
    }
}

enum Flow {
    Normal,
    Give(Value),
    Exit,
    Skip,
}

type R<T> = Result<T, XError>;

fn rt(line: u32, msg: impl Into<String>) -> XError {
    XError::at(line, 0, msg)
}

/// Runs x programs. One interpreter holds one program's globals.
pub struct Interpreter {
    globals: HashMap<Rc<str>, Var>,
    class_decls: HashMap<Rc<str>, Rc<ClassDecl>>,
    classes: HashMap<Rc<str>, Rc<Class>>,
    path: Rc<str>,
    steps: u64,
    /// Maximum statements per top-level call before aborting (guards against hangs).
    pub step_budget: u64,
    depth: u32,
    loaded_files: HashSet<String>,
    rng: u64,
}

const MAX_DEPTH: u32 = 200;

/// Seed for `Math.Random` set by the host (the web player has no clock in Rust).
static SEED: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Seed new interpreters' random numbers (0 = use the clock).
pub fn set_random_seed(seed: u64) {
    SEED.store(seed, std::sync::atomic::Ordering::Relaxed);
}

fn random_seed() -> u64 {
    // Each interpreter gets a different seed from the host's.
    if SEED.load(std::sync::atomic::Ordering::Relaxed) != 0 {
        return SEED.fetch_add(0x9E37_79B9, std::sync::atomic::Ordering::Relaxed);
    }
    #[cfg(target_arch = "wasm32")]
    return 0x9E37_79B9_7F4A_7C15;
    #[cfg(not(target_arch = "wasm32"))]
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0x9E37_79B9_7F4A_7C15)
}

impl Interpreter {
    pub fn new(path: &str) -> Self {
        let seed = random_seed();
        Self {
            globals: HashMap::new(),
            class_decls: HashMap::new(),
            classes: HashMap::new(),
            path: path.into(),
            steps: 0,
            step_budget: 5_000_000,
            depth: 0,
            loaded_files: HashSet::new(),
            rng: seed | 1,
        }
    }

    pub fn path(&self) -> &str {
        &self.path
    }

    fn located(&self, e: XError) -> XError {
        if e.path.is_empty() { XError { path: self.path.to_string(), ..e } } else { e }
    }

    /// Declare a program's classes and functions, then run its top-level statements.
    pub fn load(&mut self, program: &Program, host: &mut dyn Host) -> R<()> {
        self.steps = 0;
        self.load_inner(program, host).map_err(|e| self.located(e))
    }

    fn load_inner(&mut self, program: &Program, host: &mut dyn Host) -> R<()> {
        // Hoist declarations so order in the file does not matter.
        for s in &program.stmts {
            match s {
                Stmt::Class(c) => {
                    if self.class_decls.insert(c.name.clone(), c.clone()).is_some() {
                        return Err(rt(c.line, format!("class `{}` is declared twice", c.name)));
                    }
                }
                Stmt::Func(f) => {
                    self.globals.insert(f.name.clone(), Var { ty: Type::Any, value: Value::Func(f.clone()) });
                }
                _ => {}
            }
        }
        let names: Vec<Rc<str>> = self.class_decls.keys().cloned().collect();
        for n in names {
            let c = self.resolve_class(&n, &mut Vec::new())?;
            self.globals.insert(n, Var { ty: Type::Any, value: Value::Class(c) });
        }
        let mut frame = Frame::top();
        for s in &program.stmts {
            if matches!(s, Stmt::Class(_) | Stmt::Func(_)) {
                continue;
            }
            match self.exec(s, &mut frame, host)? {
                Flow::Normal => {}
                Flow::Give(_) => return Err(rt(stmt_line(s), "GIVE is only allowed inside a function")),
                Flow::Exit | Flow::Skip => {
                    return Err(rt(stmt_line(s), "EXIT/SKIP are only allowed inside LOOP or GRAB"));
                }
            }
        }
        Ok(())
    }

    fn resolve_class(&mut self, name: &str, stack: &mut Vec<Rc<str>>) -> R<Rc<Class>> {
        if let Some(c) = self.classes.get(name) {
            return Ok(c.clone());
        }
        let decl = self.class_decls.get(name).cloned().ok_or_else(|| rt(0, format!("unknown class `{name}`")))?;
        if stack.iter().any(|s| &**s == name) {
            return Err(rt(decl.line, format!("class `{name}` RIDEs itself")));
        }
        stack.push(decl.name.clone());
        let parent = match &decl.parent {
            Some(p) => {
                if !self.class_decls.contains_key(p) {
                    return Err(rt(decl.line, format!("`{name}` RIDEs unknown class `{p}`")));
                }
                Some(self.resolve_class(p, stack)?)
            }
            None => None,
        };
        stack.pop();
        let c = Rc::new(Class { decl, parent });
        self.classes.insert(c.decl.name.clone(), c.clone());
        Ok(c)
    }

    // ---------------------------------------------------------- embedding API

    pub fn class_names(&self) -> Vec<String> {
        let mut v: Vec<String> = self.classes.keys().map(|k| k.to_string()).collect();
        v.sort();
        v
    }

    pub fn has_class(&self, name: &str) -> bool {
        self.classes.contains_key(name)
    }

    pub fn has_function(&self, name: &str) -> bool {
        matches!(self.globals.get(name), Some(Var { value: Value::Func(_), .. }))
    }

    pub fn global(&self, name: &str) -> Option<Value> {
        self.globals.get(name).map(|v| v.value.clone())
    }

    /// Create an instance of `class` (runs field initializers and the `New` method).
    pub fn instantiate(&mut self, class: &str, args: Vec<Value>, host: &mut dyn Host) -> R<Value> {
        self.steps = 0;
        let c = self.classes.get(class).cloned().ok_or_else(|| rt(0, format!("unknown class `{class}`")));
        let c = c.map_err(|e| self.located(e))?;
        self.construct(&c, args, 0, host).map_err(|e| self.located(e))
    }

    /// Call a top-level function.
    pub fn call_function(&mut self, name: &str, args: Vec<Value>, host: &mut dyn Host) -> R<Value> {
        self.steps = 0;
        let f = match self.globals.get(name) {
            Some(Var { value: Value::Func(f), .. }) => f.clone(),
            _ => return Err(self.located(rt(0, format!("no function `{name}`")))),
        };
        self.call_func(&f, None, args, 0, host).map_err(|e| self.located(e))
    }

    pub fn has_method(&self, instance: &Value, name: &str) -> bool {
        matches!(instance, Value::Instance(i) if i.class.method(name).is_some())
    }

    /// Call `instance.name(args)`.
    pub fn call_method(&mut self, instance: &Value, name: &str, args: Vec<Value>, host: &mut dyn Host) -> R<Value> {
        self.steps = 0;
        let Value::Instance(inst) = instance else {
            return Err(self.located(rt(0, "call_method on a non-instance")));
        };
        let f = inst
            .class
            .method(name)
            .ok_or_else(|| self.located(rt(0, format!("`{}` has no method `{name}`", inst.class.name()))))?;
        self.call_func(&f, Some(inst.clone()), args, 0, host).map_err(|e| self.located(e))
    }

    // ------------------------------------------------------------- execution

    fn tick(&mut self, line: u32) -> R<()> {
        self.steps += 1;
        if self.steps > self.step_budget {
            return Err(rt(line, "script ran too long without returning (infinite LOOP?)"));
        }
        Ok(())
    }

    fn construct(&mut self, class: &Rc<Class>, args: Vec<Value>, line: u32, host: &mut dyn Host) -> R<Value> {
        let inst = Rc::new(Instance { class: class.clone(), fields: RefCell::new(Vec::new()) });
        // Initialize fields from the root ancestor down, so children can override defaults.
        let mut chain = Vec::new();
        let mut cur = Some(class.clone());
        while let Some(c) = cur {
            cur = c.parent.clone();
            chain.push(c);
        }
        let mut frame = Frame { scopes: vec![Vec::new()], this: Some(inst.clone()) };
        for c in chain.iter().rev() {
            for f in &c.decl.fields {
                let v = match &f.init {
                    Some(e) => {
                        let v = self.eval(e, &mut frame, host)?;
                        self.coerce(&f.ty, v, f.line, &f.name)?
                    }
                    None => default_for(&f.ty),
                };
                let mut fields = inst.fields.borrow_mut();
                match fields.iter_mut().find(|(n, _, _)| *n == f.name) {
                    Some(slot) => *slot = (f.name.clone(), f.ty.clone(), v),
                    None => fields.push((f.name.clone(), f.ty.clone(), v)),
                }
            }
        }
        let value = Value::Instance(inst.clone());
        if let Some(ctor) = class.method("New") {
            self.call_func(&ctor, Some(inst), args, line, host)?;
        } else if !args.is_empty() {
            return Err(rt(line, format!("`{}` takes no arguments (add a `New` method)", class.name())));
        }
        Ok(value)
    }

    fn call_func(
        &mut self,
        f: &Rc<FuncDecl>,
        this: Option<Rc<Instance>>,
        args: Vec<Value>,
        line: u32,
        host: &mut dyn Host,
    ) -> R<Value> {
        if args.len() != f.params.len() {
            return Err(rt(line, format!("`{}` expects {} argument(s), got {}", f.name, f.params.len(), args.len())));
        }
        if self.depth >= MAX_DEPTH {
            return Err(rt(line, "too much recursion"));
        }
        let mut scope = Vec::with_capacity(args.len());
        for (p, a) in f.params.iter().zip(args) {
            let v = self.coerce(&p.ty, a, f.line, &p.name)?;
            scope.push((p.name.clone(), Var { ty: p.ty.clone(), value: v }));
        }
        let mut frame = Frame { scopes: vec![scope], this };
        self.depth += 1;
        let flow = self.exec_block(&f.body, &mut frame, host);
        self.depth -= 1;
        let result = match flow? {
            Flow::Give(v) => v,
            Flow::Normal => Value::Null,
            Flow::Exit | Flow::Skip => return Err(rt(f.line, "EXIT/SKIP used outside of LOOP or GRAB")),
        };
        match f.returns.len() {
            0 => match result {
                Value::Null => Ok(Value::Null),
                _ => Err(rt(f.line, format!("VOID function `{}` cannot GIVE a value", f.name))),
            },
            1 => self.coerce(&f.returns[0], result, f.line, "return value"),
            n => match &result {
                Value::List(l) if l.borrow().len() == n => {
                    let checked: R<Vec<Value>> = l
                        .borrow()
                        .iter()
                        .zip(&f.returns)
                        .map(|(v, t)| self.coerce(t, v.clone(), f.line, "return value"))
                        .collect();
                    Ok(Value::list(checked?))
                }
                _ => Err(rt(f.line, format!("`{}` must GIVE {n} values", f.name))),
            },
        }
    }

    fn exec_block(&mut self, body: &[Stmt], frame: &mut Frame, host: &mut dyn Host) -> R<Flow> {
        frame.scopes.push(Vec::new());
        let mut out = Flow::Normal;
        for s in body {
            match self.exec(s, frame, host) {
                Ok(Flow::Normal) => {}
                Ok(flow) => {
                    out = flow;
                    break;
                }
                Err(e) => {
                    frame.scopes.pop();
                    return Err(e);
                }
            }
        }
        frame.scopes.pop();
        Ok(out)
    }

    fn declare(&mut self, frame: &mut Frame, name: &Rc<str>, ty: Type, value: Value, line: u32) -> R<()> {
        if frame.is_top_level() {
            self.globals.insert(name.clone(), Var { ty, value });
            return Ok(());
        }
        let scope = frame.scopes.last_mut().expect("scope");
        if scope.iter().any(|(n, _)| n == name) {
            return Err(rt(line, format!("`{name}` is already declared in this block")));
        }
        scope.push((name.clone(), Var { ty, value }));
        Ok(())
    }

    fn exec(&mut self, s: &Stmt, frame: &mut Frame, host: &mut dyn Host) -> R<Flow> {
        self.tick(stmt_line(s))?;
        match s {
            Stmt::Take { targets, line } => {
                for t in targets {
                    self.take(t, *line, host)?;
                }
            }
            Stmt::Class(c) => return Err(rt(c.line, "classes must be declared at the top level")),
            Stmt::Func(f) => {
                self.declare(frame, &f.name.clone(), Type::Any, Value::Func(f.clone()), f.line)?;
            }
            Stmt::Var { ty, name, init, line } => {
                let v = match init {
                    Some(e) => {
                        let v = self.eval(e, frame, host)?;
                        self.coerce(ty, v, *line, name)?
                    }
                    None => default_for(ty),
                };
                self.declare(frame, name, ty.clone(), v, *line)?;
            }
            Stmt::Assign { target, value, line } => {
                let v = self.eval(value, frame, host)?;
                self.assign(target, v, *line, frame, host)?;
            }
            Stmt::If { branches, otherwise, .. } => {
                for b in branches {
                    if self.eval(&b.cond, frame, host)?.truthy() {
                        return self.exec_block(&b.body, frame, host);
                    }
                }
                if let Some(body) = otherwise {
                    return self.exec_block(body, frame, host);
                }
            }
            Stmt::Loop { cond, body, line } => {
                while self.eval(cond, frame, host)?.truthy() {
                    self.tick(*line)?;
                    match self.exec_block(body, frame, host)? {
                        Flow::Exit => break,
                        Flow::Give(v) => return Ok(Flow::Give(v)),
                        Flow::Normal | Flow::Skip => {}
                    }
                }
            }
            Stmt::Grab { var, iter, body, line } => {
                let items: Vec<Value> = match self.eval(iter, frame, host)? {
                    Value::List(l) => l.borrow().clone(),
                    Value::Str(s) => s.chars().map(|c| Value::Str(c.to_string().into())).collect(),
                    Value::Object(o) => o.borrow().iter().map(|(k, _)| Value::Str(k.clone())).collect(),
                    Value::Int(n) => (0..n.max(0)).map(Value::Int).collect(),
                    other => {
                        return Err(rt(*line, format!("cannot GRAB items from {}", other.type_name())));
                    }
                };
                for item in items {
                    self.tick(*line)?;
                    frame.scopes.push(vec![(var.clone(), Var { ty: Type::Any, value: item })]);
                    let flow = self.exec_block(body, frame, host);
                    frame.scopes.pop();
                    match flow? {
                        Flow::Exit => break,
                        Flow::Give(v) => return Ok(Flow::Give(v)),
                        Flow::Normal | Flow::Skip => {}
                    }
                }
            }
            Stmt::Give { values, .. } => {
                let v = match values.len() {
                    0 => Value::Null,
                    1 => self.eval(&values[0], frame, host)?,
                    _ => {
                        let mut out = Vec::new();
                        for e in values {
                            out.push(self.eval(e, frame, host)?);
                        }
                        Value::list(out)
                    }
                };
                return Ok(Flow::Give(v));
            }
            Stmt::Exit { .. } => return Ok(Flow::Exit),
            Stmt::Skip { .. } => return Ok(Flow::Skip),
            Stmt::Display { values, .. } => {
                let mut parts = Vec::new();
                for e in values {
                    parts.push(self.eval(e, frame, host)?.to_string());
                }
                host.display(&parts.join(" "));
            }
            Stmt::NewInstance { name, class, args, line } => {
                let c = self.lookup_class(class, *line)?;
                let mut a = Vec::new();
                for e in args {
                    a.push(self.eval(e, frame, host)?);
                }
                let v = self.construct(&c, a, *line, host)?;
                self.declare(frame, name, Type::Class(class.to_string()), v, *line)?;
            }
            Stmt::Expr(e) => {
                self.eval(e, frame, host)?;
            }
        }
        Ok(Flow::Normal)
    }

    fn take(&mut self, t: &TakeTarget, line: u32, host: &mut dyn Host) -> R<()> {
        match t {
            TakeTarget::Module(m) => {
                if INTERNAL_MODULES.contains(&&**m) || host.has_module(m) {
                    self.globals.insert(m.clone(), Var { ty: Type::Any, value: Value::Module(m.clone()) });
                    Ok(())
                } else {
                    Err(rt(line, format!("unknown module `{m}`")))
                }
            }
            TakeTarget::File(path) => {
                if !self.loaded_files.insert(path.to_string()) {
                    return Ok(());
                }
                let src = host
                    .load_source(path)
                    .ok_or_else(|| rt(line, format!("cannot TAKE \"{path}\": file not found")))?;
                let program = crate::parse(&src).map_err(|e| XError { path: path.to_string(), ..e })?;
                self.load_inner(&program, host)
                    .map_err(|e| if e.path.is_empty() { XError { path: path.to_string(), ..e } } else { e })
            }
        }
    }

    fn lookup_class(&self, name: &str, line: u32) -> R<Rc<Class>> {
        self.classes.get(name).cloned().ok_or_else(|| rt(line, format!("unknown class `{name}`")))
    }

    fn lookup(&mut self, name: &str, line: u32, frame: &mut Frame, host: &dyn Host) -> R<Value> {
        if let Some(v) = frame.find(name) {
            return Ok(v.value.clone());
        }
        if let Some(this) = &frame.this {
            if let Some(v) = this.get(name) {
                return Ok(v);
            }
            if let Some(m) = this.class.method(name) {
                return Ok(Value::Method(this.clone(), m));
            }
        }
        if let Some(v) = self.globals.get(name) {
            return Ok(v.value.clone());
        }
        if let Some(b) = BUILTINS.iter().find(|b| **b == name) {
            return Ok(Value::Builtin(b));
        }
        if INTERNAL_MODULES.contains(&name) || host.has_module(name) {
            return Err(rt(line, format!("`{name}` is a module; add `TAKE {name}` at the top of the file")));
        }
        Err(rt(line, format!("unknown name `{name}`")))
    }

    fn coerce(&self, ty: &Type, v: Value, line: u32, what: &str) -> R<Value> {
        let ok = match (ty, &v) {
            (Type::Any, _) => true,
            (Type::Float, Value::Int(i)) => return Ok(Value::Float(*i as f64)),
            (Type::Float, Value::Float(_)) => true,
            (Type::Integer, Value::Int(_)) => true,
            (Type::String, Value::Str(_)) => true,
            (Type::Bool, Value::Bool(_)) => true,
            (Type::Vector, Value::Vector(_)) => true,
            (Type::Index, Value::List(_) | Value::Null) => true,
            (Type::Object, Value::Object(_) | Value::Null) => true,
            (Type::Class(_), Value::Null) => true,
            (Type::Class(c), Value::Instance(i)) => i.class.is_a(c),
            _ => false,
        };
        if ok {
            Ok(v)
        } else {
            let hint = match (ty, &v) {
                (Type::Integer, Value::Float(_)) => " (use INTEGER(x) to convert)",
                (Type::String, _) => " (use STRING(x) to convert)",
                _ => "",
            };
            Err(rt(line, format!("{what} is {} but got {}{hint}", ty.name(), v.type_name())))
        }
    }

    fn assign(&mut self, target: &Expr, v: Value, line: u32, frame: &mut Frame, host: &mut dyn Host) -> R<()> {
        match &target.kind {
            ExprKind::Ident(name) => {
                if let Some(var) = frame.find(name) {
                    let ty = var.ty.clone();
                    let v = self.coerce(&ty, v, line, name)?;
                    frame.find(name).unwrap().value = v;
                    return Ok(());
                }
                if let Some(this) = frame.this.clone() {
                    if let Some(ty) = this.field_type(name) {
                        let v = self.coerce(&ty, v, line, name)?;
                        this.set_raw(name, v);
                        return Ok(());
                    }
                }
                if let Some(var) = self.globals.get(name) {
                    if matches!(var.value, Value::Func(_) | Value::Class(_) | Value::Module(_)) {
                        return Err(rt(line, format!("cannot assign to `{name}`")));
                    }
                    let ty = var.ty.clone();
                    let v = self.coerce(&ty, v, line, name)?;
                    self.globals.get_mut(name).unwrap().value = v;
                    return Ok(());
                }
                Err(rt(
                    line,
                    format!("`{name}` is not declared; declare it with a type first, e.g. `FLOAT {name}: 0.0`"),
                ))
            }
            ExprKind::Field { obj, name } => {
                let base = self.eval(obj, frame, host)?;
                match base {
                    Value::Instance(i) => {
                        let ty = i
                            .field_type(name)
                            .ok_or_else(|| rt(line, format!("`{}` has no field `{name}`", i.class.name())))?;
                        let v = self.coerce(&ty, v, line, name)?;
                        i.set_raw(name, v);
                        Ok(())
                    }
                    Value::Object(o) => {
                        let mut o = o.borrow_mut();
                        match o.iter_mut().find(|(k, _)| k == name) {
                            Some(slot) => slot.1 = v,
                            None => o.push((name.clone(), v)),
                        }
                        Ok(())
                    }
                    Value::Vector(mut vec) => {
                        let idx = vector_axis(name).ok_or_else(|| rt(line, format!("VECTOR has no field `{name}`")))?;
                        vec[idx] = v.as_f64().ok_or_else(|| rt(line, "VECTOR components are numbers"))?;
                        self.assign(obj, Value::Vector(vec), line, frame, host)
                    }
                    Value::Module(m) => {
                        host.call_module(&m, &format!("Set{name}"), &[v]).map_err(|e| rt(line, e))?;
                        Ok(())
                    }
                    other => Err(rt(line, format!("cannot set `{name}` on {}", other.type_name()))),
                }
            }
            ExprKind::Index { obj, index } => {
                let base = self.eval(obj, frame, host)?;
                let idx = self.eval(index, frame, host)?;
                match base {
                    Value::List(l) => {
                        let mut l = l.borrow_mut();
                        let i = list_index(&idx, l.len(), line)?;
                        l[i] = v;
                        Ok(())
                    }
                    Value::Object(o) => {
                        let key: Rc<str> = match idx {
                            Value::Str(s) => s,
                            other => other.to_string().into(),
                        };
                        let mut o = o.borrow_mut();
                        match o.iter_mut().find(|(k, _)| *k == key) {
                            Some(slot) => slot.1 = v,
                            None => o.push((key, v)),
                        }
                        Ok(())
                    }
                    other => Err(rt(line, format!("cannot index-assign into {}", other.type_name()))),
                }
            }
            _ => Err(rt(line, "invalid assignment target")),
        }
    }

    fn eval_args(&mut self, args: &[Expr], frame: &mut Frame, host: &mut dyn Host) -> R<Vec<Value>> {
        let mut out = Vec::with_capacity(args.len());
        for a in args {
            out.push(self.eval(a, frame, host)?);
        }
        Ok(out)
    }

    fn eval(&mut self, e: &Expr, frame: &mut Frame, host: &mut dyn Host) -> R<Value> {
        let line = e.line;
        Ok(match &e.kind {
            ExprKind::Int(i) => Value::Int(*i),
            ExprKind::Float(f) => Value::Float(*f),
            ExprKind::Str(s) => Value::Str(s.clone()),
            ExprKind::Bool(b) => Value::Bool(*b),
            ExprKind::Null => Value::Null,
            ExprKind::This => match &frame.this {
                Some(t) => Value::Instance(t.clone()),
                None => return Err(rt(line, "THIS is only available inside a class")),
            },
            ExprKind::Ident(n) => self.lookup(n, line, frame, host)?,
            ExprKind::List(items) => Value::list(self.eval_args(items, frame, host)?),
            ExprKind::Object(entries) => {
                let mut out = Vec::new();
                for (k, v) in entries {
                    out.push((k.clone(), self.eval(v, frame, host)?));
                }
                Value::Object(Rc::new(RefCell::new(out)))
            }
            ExprKind::Unary(op, inner) => {
                let v = self.eval(inner, frame, host)?;
                match op {
                    UnOp::Not => Value::Bool(!v.truthy()),
                    UnOp::Neg => match v {
                        Value::Int(i) => Value::Int(-i),
                        Value::Float(f) => Value::Float(-f),
                        Value::Vector([x, y, z]) => Value::Vector([-x, -y, -z]),
                        other => return Err(rt(line, format!("cannot negate {}", other.type_name()))),
                    },
                }
            }
            ExprKind::Exist(inner) => {
                let v = self.eval(inner, frame, host)?;
                Value::Bool(!matches!(v, Value::Null))
            }
            ExprKind::Binary(op, a, b) => {
                match op {
                    BinOp::And => {
                        let l = self.eval(a, frame, host)?;
                        return Ok(Value::Bool(l.truthy() && self.eval(b, frame, host)?.truthy()));
                    }
                    BinOp::Or => {
                        let l = self.eval(a, frame, host)?;
                        return Ok(Value::Bool(l.truthy() || self.eval(b, frame, host)?.truthy()));
                    }
                    _ => {}
                }
                let l = self.eval(a, frame, host)?;
                let r = self.eval(b, frame, host)?;
                binary(*op, l, r).map_err(|m| rt(line, m))?
            }
            ExprKind::Call { callee, args } => {
                if let ExprKind::Field { obj, name } = &callee.kind {
                    let base = self.eval(obj, frame, host)?;
                    let a = self.eval_args(args, frame, host)?;
                    return self.call_member(base, name, a, line, host);
                }
                let f = self.eval(callee, frame, host)?;
                let a = self.eval_args(args, frame, host)?;
                self.call_value(f, a, line, host)?
            }
            ExprKind::Field { obj, name } => {
                let base = self.eval(obj, frame, host)?;
                self.get_member(base, name, line, host)?
            }
            ExprKind::Index { obj, index } => {
                let base = self.eval(obj, frame, host)?;
                let idx = self.eval(index, frame, host)?;
                match base {
                    Value::List(l) => {
                        let l = l.borrow();
                        l[list_index(&idx, l.len(), line)?].clone()
                    }
                    Value::Str(s) => {
                        let chars: Vec<char> = s.chars().collect();
                        Value::Str(chars[list_index(&idx, chars.len(), line)?].to_string().into())
                    }
                    Value::Object(o) => {
                        let key = match &idx {
                            Value::Str(s) => s.to_string(),
                            other => other.to_string(),
                        };
                        o.borrow().iter().find(|(k, _)| **k == *key).map(|(_, v)| v.clone()).unwrap_or(Value::Null)
                    }
                    Value::Vector(v) => v[list_index(&idx, 3, line)?].into(),
                    other => return Err(rt(line, format!("cannot index into {}", other.type_name()))),
                }
            }
            ExprKind::New { class, args } => {
                let c = self.lookup_class(class, line)?;
                let a = self.eval_args(args, frame, host)?;
                self.construct(&c, a, line, host)?
            }
            ExprKind::Convert(ty, args) => {
                let a = self.eval_args(args, frame, host)?;
                convert(ty, a).map_err(|m| rt(line, m))?
            }
        })
    }

    fn call_value(&mut self, f: Value, args: Vec<Value>, line: u32, host: &mut dyn Host) -> R<Value> {
        match f {
            Value::Func(f) => self.call_func(&f, None, args, line, host),
            Value::Method(this, f) => self.call_func(&f, Some(this), args, line, host),
            Value::Class(c) => self.construct(&c, args, line, host),
            Value::Builtin(b) => builtin(b, args).map_err(|m| rt(line, m)),
            other => Err(rt(line, format!("{} is not callable", other.type_name()))),
        }
    }

    fn call_member(&mut self, base: Value, name: &str, args: Vec<Value>, line: u32, host: &mut dyn Host) -> R<Value> {
        match &base {
            Value::Module(m) => {
                if &**m == "Math" {
                    return self.math(name, &args).map_err(|e| rt(line, e));
                }
                host.call_module(m, name, &args).map_err(|e| rt(line, format!("{m}.{name}: {e}")))
            }
            Value::Instance(i) => {
                if let Some(f) = i.class.method(name) {
                    return self.call_func(&f, Some(i.clone()), args, line, host);
                }
                match i.get(name) {
                    Some(v) => self.call_value(v, args, line, host),
                    None => Err(rt(line, format!("`{}` has no method `{name}`", i.class.name()))),
                }
            }
            Value::Object(o) => {
                let field = o.borrow().iter().find(|(k, _)| &**k == name).map(|(_, v)| v.clone());
                match field {
                    Some(v @ (Value::Func(_) | Value::Method(..) | Value::Builtin(_))) => {
                        self.call_value(v, args, line, host)
                    }
                    _ => builtin_method(&base, name, args).map_err(|m| rt(line, m)),
                }
            }
            _ => builtin_method(&base, name, args).map_err(|m| rt(line, m)),
        }
    }

    fn get_member(&mut self, base: Value, name: &str, line: u32, host: &mut dyn Host) -> R<Value> {
        match &base {
            Value::Instance(i) => {
                if let Some(v) = i.get(name) {
                    return Ok(v);
                }
                if let Some(m) = i.class.method(name) {
                    return Ok(Value::Method(i.clone(), m));
                }
                Err(rt(line, format!("`{}` has no field `{name}`", i.class.name())))
            }
            Value::Object(o) => {
                Ok(o.borrow().iter().find(|(k, _)| &**k == name).map(|(_, v)| v.clone()).unwrap_or(Value::Null))
            }
            Value::Vector(v) => match vector_axis(name) {
                Some(i) => Ok(Value::Float(v[i])),
                None => builtin_method(&base, name, Vec::new()).map_err(|m| rt(line, m)),
            },
            // Property-style access: `Time.Delta`, `Math.PI`.
            Value::Module(_) => self.call_member(base, name, Vec::new(), line, host),
            _ => builtin_method(&base, name, Vec::new()).map_err(|m| rt(line, m)),
        }
    }

    fn next_random(&mut self) -> f64 {
        // xorshift64*
        let mut x = self.rng;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.rng = x;
        (x.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 11) as f64 / (1u64 << 53) as f64
    }

    fn math(&mut self, name: &str, a: &[Value]) -> Result<Value, String> {
        let num = |i: usize| -> Result<f64, String> {
            a.get(i)
                .and_then(|v| v.as_f64())
                .ok_or_else(|| format!("Math.{name} expects a number for argument {}", i + 1))
        };
        let want = |n: usize| -> Result<(), String> {
            if a.len() == n { Ok(()) } else { Err(format!("Math.{name} expects {n} argument(s)")) }
        };
        let f = |x: f64| Ok(Value::Float(x));
        match name {
            "PI" => f(std::f64::consts::PI),
            "TAU" => f(std::f64::consts::TAU),
            "E" => f(std::f64::consts::E),
            "Sin" => {
                want(1)?;
                f(num(0)?.sin())
            }
            "Cos" => {
                want(1)?;
                f(num(0)?.cos())
            }
            "Tan" => {
                want(1)?;
                f(num(0)?.tan())
            }
            "Asin" => {
                want(1)?;
                f(num(0)?.asin())
            }
            "Acos" => {
                want(1)?;
                f(num(0)?.acos())
            }
            "Atan" => {
                want(1)?;
                f(num(0)?.atan())
            }
            "Atan2" => {
                want(2)?;
                f(num(0)?.atan2(num(1)?))
            }
            "Sqrt" => {
                want(1)?;
                f(num(0)?.sqrt())
            }
            "Pow" => {
                want(2)?;
                f(num(0)?.powf(num(1)?))
            }
            "Radians" => {
                want(1)?;
                f(num(0)?.to_radians())
            }
            "Degrees" => {
                want(1)?;
                f(num(0)?.to_degrees())
            }
            "Abs" => {
                want(1)?;
                match &a[0] {
                    Value::Int(i) => Ok(Value::Int(i.abs())),
                    _ => f(num(0)?.abs()),
                }
            }
            "Sign" => {
                want(1)?;
                f(num(0)?.signum())
            }
            "Floor" => {
                want(1)?;
                Ok(Value::Int(num(0)?.floor() as i64))
            }
            "Ceil" => {
                want(1)?;
                Ok(Value::Int(num(0)?.ceil() as i64))
            }
            "Round" => {
                want(1)?;
                Ok(Value::Int(num(0)?.round() as i64))
            }
            "Min" | "Max" => {
                want(2)?;
                let (x, y) = (num(0)?, num(1)?);
                let pick_first = if name == "Min" { x <= y } else { x >= y };
                Ok(if pick_first { a[0].clone() } else { a[1].clone() })
            }
            "Clamp" => {
                want(3)?;
                f(num(0)?.clamp(num(1)?, num(2)?.max(num(1)?)))
            }
            "Lerp" => {
                want(3)?;
                let t = num(2)?;
                f(num(0)? + (num(1)? - num(0)?) * t)
            }
            "Random" => {
                want(0)?;
                f(self.next_random())
            }
            "RandomRange" => {
                want(2)?;
                let (lo, hi) = (num(0)?, num(1)?);
                let r = self.next_random();
                match (&a[0], &a[1]) {
                    (Value::Int(l), Value::Int(h)) if h >= l => Ok(Value::Int(l + (r * ((h - l + 1) as f64)) as i64)),
                    _ => f(lo + (hi - lo) * r),
                }
            }
            _ => Err(format!("Math has no member `{name}`")),
        }
    }
}

fn stmt_line(s: &Stmt) -> u32 {
    match s {
        Stmt::Take { line, .. }
        | Stmt::Var { line, .. }
        | Stmt::Assign { line, .. }
        | Stmt::If { line, .. }
        | Stmt::Loop { line, .. }
        | Stmt::Grab { line, .. }
        | Stmt::Give { line, .. }
        | Stmt::Exit { line }
        | Stmt::Skip { line }
        | Stmt::Display { line, .. }
        | Stmt::NewInstance { line, .. } => *line,
        Stmt::Class(c) => c.line,
        Stmt::Func(f) => f.line,
        Stmt::Expr(e) => e.line,
    }
}

fn default_for(ty: &Type) -> Value {
    match ty {
        Type::Integer => Value::Int(0),
        Type::Float => Value::Float(0.0),
        Type::String => Value::str(""),
        Type::Bool => Value::Bool(false),
        Type::Index => Value::list(Vec::new()),
        Type::Object => Value::Object(Rc::new(RefCell::new(Vec::new()))),
        Type::Vector => Value::Vector([0.0; 3]),
        _ => Value::Null,
    }
}

fn vector_axis(name: &str) -> Option<usize> {
    match name {
        "X" | "x" => Some(0),
        "Y" | "y" => Some(1),
        "Z" | "z" => Some(2),
        _ => None,
    }
}

impl From<f64> for Value {
    fn from(f: f64) -> Self {
        Value::Float(f)
    }
}

fn list_index(idx: &Value, len: usize, line: u32) -> R<usize> {
    let i = idx.as_i64().ok_or_else(|| rt(line, format!("index must be an INTEGER, got {}", idx.type_name())))?;
    let real = if i < 0 { len as i64 + i } else { i };
    if real < 0 || real as usize >= len {
        return Err(rt(line, format!("index {i} is out of range (length {len})")));
    }
    Ok(real as usize)
}

fn binary(op: BinOp, l: Value, r: Value) -> Result<Value, String> {
    use Value::*;
    let type_err =
        |l: &Value, r: &Value| Err(format!("cannot apply {:?} to {} and {}", op, l.type_name(), r.type_name()));
    match op {
        BinOp::Eq => return Ok(Bool(l.equals(&r))),
        BinOp::Ne => return Ok(Bool(!l.equals(&r))),
        _ => {}
    }
    // String concatenation: "Score: " + 10
    if op == BinOp::Add && (matches!(l, Str(_)) || matches!(r, Str(_))) {
        return Ok(Str(format!("{l}{r}").into()));
    }
    match (&l, &r) {
        (Int(a), Int(b)) => {
            let (a, b) = (*a, *b);
            Ok(match op {
                BinOp::Add => Int(a.wrapping_add(b)),
                BinOp::Sub => Int(a.wrapping_sub(b)),
                BinOp::Mul => Int(a.wrapping_mul(b)),
                BinOp::Div => {
                    if b == 0 {
                        return Err("division by zero".into());
                    }
                    Int(a.wrapping_div(b))
                }
                BinOp::Rem => {
                    if b == 0 {
                        return Err("division by zero".into());
                    }
                    Int(a.wrapping_rem(b))
                }
                BinOp::Lt => Bool(a < b),
                BinOp::Gt => Bool(a > b),
                BinOp::Le => Bool(a <= b),
                BinOp::Ge => Bool(a >= b),
                _ => unreachable!(),
            })
        }
        (Int(_) | Float(_), Int(_) | Float(_)) => {
            let (a, b) = (l.as_f64().unwrap(), r.as_f64().unwrap());
            Ok(match op {
                BinOp::Add => Float(a + b),
                BinOp::Sub => Float(a - b),
                BinOp::Mul => Float(a * b),
                BinOp::Div => Float(a / b),
                BinOp::Rem => Float(a % b),
                BinOp::Lt => Bool(a < b),
                BinOp::Gt => Bool(a > b),
                BinOp::Le => Bool(a <= b),
                BinOp::Ge => Bool(a >= b),
                _ => unreachable!(),
            })
        }
        (Str(a), Str(b)) => Ok(match op {
            BinOp::Lt => Bool(a < b),
            BinOp::Gt => Bool(a > b),
            BinOp::Le => Bool(a <= b),
            BinOp::Ge => Bool(a >= b),
            _ => return type_err(&l, &r),
        }),
        (Vector(a), Vector(b)) => Ok(match op {
            BinOp::Add => Vector([a[0] + b[0], a[1] + b[1], a[2] + b[2]]),
            BinOp::Sub => Vector([a[0] - b[0], a[1] - b[1], a[2] - b[2]]),
            BinOp::Mul => Vector([a[0] * b[0], a[1] * b[1], a[2] * b[2]]),
            _ => return type_err(&l, &r),
        }),
        (Vector(a), Int(_) | Float(_)) => {
            let s = r.as_f64().unwrap();
            Ok(match op {
                BinOp::Mul => Vector([a[0] * s, a[1] * s, a[2] * s]),
                BinOp::Div => Vector([a[0] / s, a[1] / s, a[2] / s]),
                _ => return type_err(&l, &r),
            })
        }
        (Int(_) | Float(_), Vector(b)) if op == BinOp::Mul => {
            let s = l.as_f64().unwrap();
            Ok(Vector([b[0] * s, b[1] * s, b[2] * s]))
        }
        (List(a), List(b)) if op == BinOp::Add => {
            let mut v = a.borrow().clone();
            v.extend(b.borrow().iter().cloned());
            Ok(Value::list(v))
        }
        _ => type_err(&l, &r),
    }
}

fn convert(ty: &Type, a: Vec<Value>) -> Result<Value, String> {
    let one = |a: &[Value]| -> Result<Value, String> {
        if a.len() == 1 { Ok(a[0].clone()) } else { Err(format!("{}(x) takes one argument", ty.name())) }
    };
    match ty {
        Type::Integer => match one(&a)? {
            Value::Int(i) => Ok(Value::Int(i)),
            Value::Float(f) => Ok(Value::Int(f as i64)),
            Value::Bool(b) => Ok(Value::Int(b as i64)),
            Value::Str(s) => {
                s.trim().parse::<i64>().map(Value::Int).map_err(|_| format!("cannot convert \"{s}\" to INTEGER"))
            }
            other => Err(format!("cannot convert {} to INTEGER", other.type_name())),
        },
        Type::Float => match one(&a)? {
            Value::Int(i) => Ok(Value::Float(i as f64)),
            Value::Float(f) => Ok(Value::Float(f)),
            Value::Str(s) => {
                s.trim().parse::<f64>().map(Value::Float).map_err(|_| format!("cannot convert \"{s}\" to FLOAT"))
            }
            other => Err(format!("cannot convert {} to FLOAT", other.type_name())),
        },
        Type::String => Ok(Value::Str(one(&a)?.to_string().into())),
        Type::Bool => Ok(Value::Bool(one(&a)?.truthy())),
        Type::Vector => match a.as_slice() {
            [] => Ok(Value::Vector([0.0; 3])),
            [Value::Vector(v)] => Ok(Value::Vector(*v)),
            [s] if s.as_f64().is_some() => {
                let s = s.as_f64().unwrap();
                Ok(Value::Vector([s, s, s]))
            }
            [x, y] => Ok(Value::Vector([num(x)?, num(y)?, 0.0])),
            [x, y, z] => Ok(Value::Vector([num(x)?, num(y)?, num(z)?])),
            _ => Err("VECTOR takes (x, y, z)".into()),
        },
        Type::Index => Ok(Value::list(a)),
        Type::Object if a.is_empty() => Ok(Value::Object(Rc::new(RefCell::new(Vec::new())))),
        _ => Err(format!("cannot convert to {}", ty.name())),
    }
}

fn num(v: &Value) -> Result<f64, String> {
    v.as_f64().ok_or_else(|| format!("expected a number, got {}", v.type_name()))
}

fn builtin(name: &str, a: Vec<Value>) -> Result<Value, String> {
    match name {
        "RANGE" => {
            let (lo, hi) = match a.as_slice() {
                [n] => (0, n.as_i64().ok_or("RANGE expects INTEGERs")?),
                [l, h] => (l.as_i64().ok_or("RANGE expects INTEGERs")?, h.as_i64().ok_or("RANGE expects INTEGERs")?),
                _ => return Err("RANGE(n) or RANGE(start, end)".into()),
            };
            if hi - lo > 10_000_000 {
                return Err("RANGE is too large".into());
            }
            Ok(Value::list((lo..hi).map(Value::Int).collect()))
        }
        "LENGTH" => match a.as_slice() {
            [v] => builtin_method(v, "Length", Vec::new()),
            _ => Err("LENGTH(x) takes one argument".into()),
        },
        "TYPEOF" => match a.as_slice() {
            [v] => Ok(Value::Str(v.type_name().into())),
            _ => Err("TYPEOF(x) takes one argument".into()),
        },
        _ => Err(format!("unknown builtin {name}")),
    }
}

/// Methods on built-in types: `List.Add(x)`, `Name.Upper()`, `Dir.Normalize()` ...
fn builtin_method(base: &Value, name: &str, a: Vec<Value>) -> Result<Value, String> {
    let argn = |n: usize| -> Result<(), String> {
        if a.len() == n { Ok(()) } else { Err(format!("{name} expects {n} argument(s)")) }
    };
    match base {
        Value::List(l) => match name {
            "Length" => Ok(Value::Int(l.borrow().len() as i64)),
            "Add" => {
                argn(1)?;
                l.borrow_mut().push(a[0].clone());
                Ok(Value::Null)
            }
            "Insert" => {
                argn(2)?;
                let mut l = l.borrow_mut();
                let i = a[0].as_i64().ok_or("Insert(index, value)")?;
                if i < 0 || i as usize > l.len() {
                    return Err(format!("index {i} is out of range"));
                }
                l.insert(i as usize, a[1].clone());
                Ok(Value::Null)
            }
            "Remove" => {
                argn(1)?;
                let mut l = l.borrow_mut();
                let len = l.len();
                let i = list_index(&a[0], len, 0).map_err(|e| e.message)?;
                Ok(l.remove(i))
            }
            "Pop" => Ok(l.borrow_mut().pop().unwrap_or(Value::Null)),
            "Clear" => {
                l.borrow_mut().clear();
                Ok(Value::Null)
            }
            "Contains" => {
                argn(1)?;
                Ok(Value::Bool(l.borrow().iter().any(|v| v.equals(&a[0]))))
            }
            "Find" => {
                argn(1)?;
                Ok(Value::Int(l.borrow().iter().position(|v| v.equals(&a[0])).map(|i| i as i64).unwrap_or(-1)))
            }
            "Join" => {
                let sep = a.first().map(|v| v.to_string()).unwrap_or_default();
                Ok(Value::Str(l.borrow().iter().map(|v| v.to_string()).collect::<Vec<_>>().join(&sep).into()))
            }
            "Reverse" => {
                l.borrow_mut().reverse();
                Ok(Value::Null)
            }
            "Copy" => Ok(Value::list(l.borrow().clone())),
            _ => Err(format!("INDEX has no method `{name}`")),
        },
        Value::Str(s) => match name {
            "Length" => Ok(Value::Int(s.chars().count() as i64)),
            "Upper" => Ok(Value::Str(s.to_uppercase().into())),
            "Lower" => Ok(Value::Str(s.to_lowercase().into())),
            "Trim" => Ok(Value::Str(s.trim().into())),
            "Contains" => {
                argn(1)?;
                Ok(Value::Bool(s.contains(&*a[0].to_string())))
            }
            "StartsWith" => {
                argn(1)?;
                Ok(Value::Bool(s.starts_with(&*a[0].to_string())))
            }
            "Split" => {
                argn(1)?;
                let sep = a[0].to_string();
                Ok(Value::list(s.split(&*sep).map(Value::str).collect()))
            }
            "Replace" => {
                argn(2)?;
                Ok(Value::Str(s.replace(&*a[0].to_string(), &a[1].to_string()).into()))
            }
            _ => Err(format!("STRING has no method `{name}`")),
        },
        Value::Object(o) => match name {
            "Length" => Ok(Value::Int(o.borrow().len() as i64)),
            "Keys" => Ok(Value::list(o.borrow().iter().map(|(k, _)| Value::Str(k.clone())).collect())),
            "Has" => {
                argn(1)?;
                let k = a[0].to_string();
                Ok(Value::Bool(o.borrow().iter().any(|(key, _)| **key == *k)))
            }
            "Remove" => {
                argn(1)?;
                let k = a[0].to_string();
                let mut o = o.borrow_mut();
                let pos = o.iter().position(|(key, _)| **key == *k);
                Ok(pos.map(|p| o.remove(p).1).unwrap_or(Value::Null))
            }
            _ => Err(format!("OBJECT has no method `{name}`")),
        },
        Value::Vector(v) => {
            let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
            match name {
                "Length" => Ok(Value::Float(len)),
                "Normalize" => {
                    Ok(if len > 0.0 { Value::Vector([v[0] / len, v[1] / len, v[2] / len]) } else { Value::Vector(*v) })
                }
                "Dot" => {
                    argn(1)?;
                    let b = a[0].as_vector().ok_or("Dot expects a VECTOR")?;
                    Ok(Value::Float(v[0] * b[0] + v[1] * b[1] + v[2] * b[2]))
                }
                "Cross" => {
                    argn(1)?;
                    let b = a[0].as_vector().ok_or("Cross expects a VECTOR")?;
                    Ok(Value::Vector([v[1] * b[2] - v[2] * b[1], v[2] * b[0] - v[0] * b[2], v[0] * b[1] - v[1] * b[0]]))
                }
                "Distance" => {
                    argn(1)?;
                    let b = a[0].as_vector().ok_or("Distance expects a VECTOR")?;
                    let d = [v[0] - b[0], v[1] - b[1], v[2] - b[2]];
                    Ok(Value::Float((d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()))
                }
                _ => Err(format!("VECTOR has no member `{name}`")),
            }
        }
        other => Err(format!("{} has no member `{name}`", other.type_name())),
    }
}
