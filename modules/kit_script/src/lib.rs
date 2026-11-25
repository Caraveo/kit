//! # kit_script
//!
//! Runs x scripts attached to entities (Defold's script components).
//!
//! A script file declares a class; Kit creates one instance per entity and calls
//! its lifecycle methods while the game is playing:
//!
//! * `Init()` — once, before the first update.
//! * `Update(FLOAT dt)` — every frame.
//! * `Final()` — when the entity or script goes away, or play stops.
//! * `OnCollision(other)` / `OnCollisionEnd(other)` — touching another collider
//!   (`other` is an entity id usable with the `Scene` module).
//! * `OnTrigger(other)` / `OnTriggerEnd(other)` — overlapping a trigger.
//!
//! The class whose name matches the file name is used (`player.x` → `Player`),
//! otherwise the first class. Files without a class may define top-level
//! `Init`/`Update` functions instead.
//!
//! Fields are private by default. `PUBLIC` shows a field in the editor
//! (read-only); `EDIT` shows it and lets the editor set its value, saved
//! per object in [`Script::values`] and applied before `Init`.
//!
//! Engine modules available to `TAKE`: [`MODULES`].

mod host;

pub use host::MODULES;

use host::{Command, EntityHost};
use kit_core::{App, Console, Module, Stage, Time};
use kit_ecs::{Entity, World};
use kit_gamesys::{FieldValue, Script};
use kit_resource::Resources;
pub use kit_x::ast::Access;
use kit_x::{Interpreter, Value, XError};
use std::collections::HashMap;
use std::time::SystemTime;

struct ScriptState {
    path: String,
    interp: Interpreter,
    /// The class instance, or `None` for function-style scripts.
    object: Option<Value>,
    initialized: bool,
    failed: bool,
    modified: Option<SystemTime>,
}

/// A PUBLIC or EDIT field, for the inspector.
#[derive(Clone, Debug, PartialEq)]
pub struct FieldView {
    pub name: String,
    /// x type name (`FLOAT`, `VECTOR`, …).
    pub ty: String,
    pub access: Access,
    /// The value, when it's an editable kind.
    pub value: Option<FieldValue>,
    /// The value as text (any kind).
    pub text: String,
}

fn field_value(v: &Value) -> Option<FieldValue> {
    Some(match v {
        Value::Bool(b) => FieldValue::Bool(*b),
        Value::Int(i) => FieldValue::Int(*i),
        Value::Float(f) => FieldValue::Float(*f),
        Value::Vector(v) => FieldValue::Vector(*v),
        Value::Str(s) => FieldValue::Text(s.to_string()),
        _ => return None,
    })
}

/// Convert an editor value to a field's declared type.
fn to_x(ty: &kit_x::ast::Type, v: &FieldValue) -> Option<Value> {
    use kit_x::ast::Type as T;
    Some(match (ty, v) {
        (T::Float, FieldValue::Float(f)) => Value::Float(*f),
        (T::Float, FieldValue::Int(i)) => Value::Float(*i as f64),
        (T::Integer, FieldValue::Int(i)) => Value::Int(*i),
        (T::Integer, FieldValue::Float(f)) if f.fract() == 0.0 => Value::Int(*f as i64),
        (T::Bool, FieldValue::Bool(b)) => Value::Bool(*b),
        (T::String, FieldValue::Text(s)) => Value::Str(s.as_str().into()),
        (T::Vector, FieldValue::Vector(v)) => Value::Vector(*v),
        _ => return None,
    })
}

/// A script's PUBLIC / EDIT fields as declared in its source.
#[derive(Clone, Debug, PartialEq)]
pub struct Declared {
    pub fields: Vec<FieldView>,
    /// Set when the file can't be read or parsed.
    pub error: Option<String>,
}

/// Script instances per entity. Lives as a world resource.
#[derive(Default)]
pub struct ScriptRuntime {
    states: HashMap<Entity, ScriptState>,
    last_reload_check: f64,
    declared: HashMap<String, (Option<SystemTime>, Declared)>,
}

/// Read a script's declared PUBLIC / EDIT fields (with literal defaults).
pub fn declared_fields(res: &Resources, path: &str) -> Declared {
    let src = match res.read_text(path) {
        Ok(s) => s,
        Err(e) => return Declared { fields: Vec::new(), error: Some(format!("{e:#}")) },
    };
    let program = match kit_x::parse(&src) {
        Ok(p) => p,
        Err(e) => return Declared { fields: Vec::new(), error: Some(e.to_string()) },
    };
    let stem = std::path::Path::new(path).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let fields = kit_x::declared_fields(&program, &stem)
        .into_iter()
        .map(|f| FieldView {
            text: f.default.to_string(),
            value: field_value(&f.default),
            name: f.name,
            ty: f.ty.name(),
            access: f.access,
        })
        .collect();
    Declared { fields, error: None }
}

impl ScriptRuntime {
    /// PUBLIC and EDIT fields of an entity's running script, with live values
    /// (for the inspector). Private fields are never listed.
    pub fn fields(&self, e: Entity) -> Vec<FieldView> {
        match self.states.get(&e).and_then(|s| s.object.as_ref()) {
            Some(Value::Instance(i)) => i
                .snapshot()
                .into_iter()
                .map(|(name, ty, access, v)| FieldView {
                    name,
                    ty: ty.name(),
                    access,
                    value: field_value(&v),
                    text: v.to_string(),
                })
                .collect(),
            _ => Vec::new(),
        }
    }

    /// Change an EDIT field of a running script. Returns false if the field
    /// isn't EDIT or the value doesn't fit its type.
    pub fn set_field(&mut self, e: Entity, name: &str, value: &FieldValue) -> bool {
        let Some(Value::Instance(i)) = self.states.get(&e).and_then(|s| s.object.as_ref()) else { return false };
        if i.class.field_access(name) != Access::Edit {
            return false;
        }
        match i.field_type(name).and_then(|t| to_x(&t, value)) {
            Some(v) => i.set_raw(name, v),
            None => false,
        }
    }

    /// Declared PUBLIC / EDIT fields of a script file (cached until it changes).
    pub fn declared(&mut self, res: &Resources, path: &str) -> Declared {
        let modified = res.modified(path);
        if let Some((m, d)) = self.declared.get(path) {
            if *m == modified && modified.is_some() {
                return d.clone();
            }
        }
        let d = declared_fields(res, path);
        self.declared.insert(path.to_owned(), (modified, d.clone()));
        d
    }

    pub fn has_error(&self, e: Entity) -> bool {
        self.states.get(&e).is_some_and(|s| s.failed)
    }

    pub fn len(&self) -> usize {
        self.states.len()
    }

    pub fn is_empty(&self) -> bool {
        self.states.is_empty()
    }
}

fn report(world: &World, path: &str, err: &XError) {
    let text = if err.path.is_empty() { format!("{path}: {err}") } else { err.to_string() };
    world.resource_mut::<Console>().error("x", text);
}

fn load_state(world: &mut World, e: Entity, path: &str, res: &Option<Resources>) -> ScriptState {
    let mut state = ScriptState {
        path: path.to_string(),
        interp: Interpreter::new(path),
        object: None,
        initialized: false,
        failed: true,
        modified: res.as_ref().and_then(|r| r.modified(path)),
    };
    let Some(res) = res else {
        world.resource_mut::<Console>().error("x", format!("{path}: no project resources available"));
        return state;
    };
    let src = match res.read_text(path) {
        Ok(s) => s,
        Err(err) => {
            world.resource_mut::<Console>().error("x", format!("{path}: {err:#}"));
            return state;
        }
    };
    let program = match kit_x::parse(&src) {
        Ok(p) => p,
        Err(err) => {
            report(world, path, &XError { path: path.to_string(), ..err });
            return state;
        }
    };
    let mut commands = Vec::new();
    let result = {
        let mut host = EntityHost::new(world, e, Some(res.clone()), &mut commands);
        state.interp.load(&program, &mut host).and_then(|_| {
            let stem =
                std::path::Path::new(path).file_stem().map(|s| s.to_string_lossy().to_lowercase()).unwrap_or_default();
            let classes = state.interp.class_names();
            let class = classes.iter().find(|c| c.to_lowercase() == stem).or_else(|| classes.first()).cloned();
            match class {
                Some(c) => state.interp.instantiate(&c, Vec::new(), &mut host).map(Some),
                None => Ok(None),
            }
        })
    };
    apply_commands(world, commands);
    match result {
        Ok(obj) => {
            // Editor-set values for EDIT fields, before Init runs.
            if let (Some(Value::Instance(inst)), Some(script)) = (&obj, world.get_cloned::<Script>(e)) {
                for (name, v) in &script.values {
                    let access = inst.class.field_access(name);
                    let typed = inst.field_type(name).and_then(|t| to_x(&t, v));
                    match (access, typed) {
                        (Access::Edit, Some(x)) => {
                            inst.set_raw(name, x);
                        }
                        (Access::Edit, None) => world.resource_mut::<Console>().warn(
                            "x",
                            format!("{path}: the saved value for `{name}` doesn't fit its type any more; using the script's default"),
                        ),
                        _ => world.resource_mut::<Console>().warn(
                            "x",
                            format!("{path}: `{name}` has a saved value but isn't an EDIT field (any more); ignoring it"),
                        ),
                    }
                }
            }
            state.object = obj;
            state.failed = false;
        }
        Err(err) => report(world, path, &err),
    }
    state
}

/// Call a lifecycle hook if the script defines it.
fn call_hook(
    world: &mut World,
    e: Entity,
    state: &mut ScriptState,
    name: &str,
    args: Vec<Value>,
    res: &Option<Resources>,
) {
    if state.failed {
        return;
    }

    let mut commands = Vec::new();
    let result = {
        let mut host = EntityHost::new(world, e, res.clone(), &mut commands);
        match &state.object {
            Some(obj) if state.interp.has_method(obj, name) => {
                state.interp.call_method(obj, name, args, &mut host).map(|_| ())
            }
            Some(_) => Ok(()),
            None if state.interp.has_function(name) => state.interp.call_function(name, args, &mut host).map(|_| ()),
            None => Ok(()),
        }
    };
    apply_commands(world, commands);
    if let Err(err) = result {
        state.failed = true;
        report(world, &state.path, &err);
    }
}

/// Run one X file hook immediately for an event owned by `entity`.
///
/// Interface buttons use this for local UI actions without attaching a
/// permanent gameplay script component to the camera.
pub fn call_file_hook(world: &mut World, entity: Entity, path: &str, hook: &str) {
    let res = world.try_resource::<Resources>().map(|r| r.clone());
    let mut state = load_state(world, entity, path, &res);
    if state.failed {
        return;
    }
    call_hook(world, entity, &mut state, hook, Vec::new(), &res);
}

fn apply_commands(world: &mut World, commands: Vec<Command>) {
    for c in commands {
        match c {
            Command::Despawn(e) => kit_scene::despawn_recursive(world, e),
            Command::Scene(op) => kit_gamesys::scenes::request(world, op),
        }
    }
}

/// The per-frame script system.
pub fn run_scripts(world: &mut World) {
    let Some(mut rt) = world.remove_resource::<ScriptRuntime>() else { return };
    let res = world.try_resource::<Resources>().map(|r| r.clone());
    let (dt, elapsed) = {
        let t = world.resource::<Time>();
        (t.delta, t.elapsed)
    };

    // Hot reload: check script files for changes a few times per second.
    let check_reload = elapsed - rt.last_reload_check > 0.5 || elapsed < rt.last_reload_check;
    if check_reload {
        rt.last_reload_check = elapsed;
    }

    // Collision hooks for last frame's physics events:
    // OnCollision(other) / OnCollisionEnd(other) / OnTrigger(other) / OnTriggerEnd(other)
    let events: Vec<kit_gamesys::CollisionEvent> =
        world.try_resource::<kit_gamesys::CollisionEvents>().map(|ev| ev.events.clone()).unwrap_or_default();
    for ev in events {
        let hook = match (ev.trigger, ev.started) {
            (false, true) => "OnCollision",
            (false, false) => "OnCollisionEnd",
            (true, true) => "OnTrigger",
            (true, false) => "OnTriggerEnd",
        };
        for (me, other) in [(ev.a, ev.b), (ev.b, ev.a)] {
            if let Some(mut st) = rt.states.remove(&me) {
                if st.initialized && world.is_alive(me) {
                    call_hook(world, me, &mut st, hook, vec![Value::Int(other.to_bits() as i64)], &res);
                }
                if world.is_alive(me) {
                    rt.states.insert(me, st);
                }
            }
        }
    }

    let entities = world.query::<(Script,)>();
    // Drop states for entities that lost their script.
    let gone: Vec<Entity> = rt.states.keys().filter(|e| !entities.contains(e)).copied().collect();
    for e in gone {
        let mut st = rt.states.remove(&e).unwrap();
        if world.is_alive(e) {
            call_hook(world, e, &mut st, "Final", Vec::new(), &res);
        }
    }

    for e in entities {
        if !world.is_alive(e) {
            rt.states.remove(&e);
            continue;
        }
        let Some(path) = world.get::<Script>(e).map(|s| s.path.clone()) else { continue };
        if path.trim().is_empty() {
            rt.states.remove(&e);
            continue;
        }
        let stale = match rt.states.get(&e) {
            None => true,
            Some(s) if s.path != path => true,
            Some(s) => check_reload && res.as_ref().and_then(|r| r.modified(&path)) != s.modified,
        };
        if stale {
            if let Some(mut old) = rt.states.remove(&e) {
                call_hook(world, e, &mut old, "Final", Vec::new(), &res);
                if old.path == path {
                    world.resource_mut::<Console>().info("x", format!("reloaded {path}"));
                }
            }
            let st = load_state(world, e, &path, &res);
            rt.states.insert(e, st);
        }
        let mut st = rt.states.remove(&e).unwrap();
        if !st.initialized {
            st.initialized = true;
            call_hook(world, e, &mut st, "Init", Vec::new(), &res);
        }
        if world.is_alive(e) {
            call_hook(world, e, &mut st, "Update", vec![Value::Float(dt as f64)], &res);
        }
        if world.is_alive(e) {
            rt.states.insert(e, st);
        }
    }
    world.insert_resource(rt);
}

/// Call `Final` on every script and drop all instances (play stopped).
pub fn stop_scripts(world: &mut World) {
    let Some(mut rt) = world.remove_resource::<ScriptRuntime>() else { return };
    let res = world.try_resource::<Resources>().map(|r| r.clone());
    for (e, mut st) in rt.states.drain() {
        if world.is_alive(e) {
            call_hook(world, e, &mut st, "Final", Vec::new(), &res);
        }
    }
    world.insert_resource(ScriptRuntime::default());
}

/// Check a script for syntax errors without running it.
pub fn check_source(path: &str, src: &str) -> Result<(), XError> {
    kit_x::parse(src).map(|_| ()).map_err(|e| XError { path: path.to_string(), ..e })
}

pub struct ScriptModule;

impl Module for ScriptModule {
    fn name(&self) -> &'static str {
        "script"
    }
    fn dependencies(&self) -> &'static [&'static str] {
        &["scene", "input", "gamesys"]
    }
    fn build(&self, app: &mut App) {
        app.world.insert_resource(ScriptRuntime::default());
        app.add_play_system(Stage::Update, "x_scripts", run_scripts);
    }
}

#[cfg(test)]
mod tests;
