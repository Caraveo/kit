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
mod save;

pub use host::{ENGINE_FEATURES, MODULES};

use host::{Command, EntityHost};
use kit_core::{App, Console, Module, Stage, Time};
use kit_ecs::{Entity, World};
use kit_gamesys::{FieldValue, Script};
use kit_resource::Resources;
pub use kit_x::ast::Access;
use kit_x::{Interpreter, Value, XError};
use std::collections::HashMap;
use std::time::SystemTime;

pub(crate) struct ScriptState {
    pub(crate) path: String,
    pub(crate) interp: Interpreter,
    /// The class instance, or `None` for function-style scripts.
    pub(crate) object: Option<Value>,
    initialized: bool,
    pub(crate) failed: bool,
    modified: Option<SystemTime>,
    /// `Time.After` / `Time.Every` calls waiting to happen.
    timers: Vec<host::Timer>,
    /// Coroutines (methods paused at a `WAIT`): id, state, game time to wake.
    coroutines: Vec<(u64, kit_x::Coroutine, f64)>,
    /// Coroutines asked for while the script ran, started once it returns.
    starting: Vec<(u64, String, Vec<Value>)>,
}

impl ScriptState {
    /// The script's class name (what `GetFeature` matches), or the file name.
    pub(crate) fn feature_name(&self) -> String {
        match &self.object {
            Some(Value::Instance(i)) => i.class.name().to_string(),
            _ => std::path::Path::new(&self.path)
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default(),
        }
    }
}

/// Scripts by object, handed to hosts so scripts can reach each other.
pub(crate) type States = HashMap<Entity, ScriptState>;

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
    states: States,
    last_reload_check: f64,
    /// Leftover time for `FixedUpdate`'s steady steps.
    fixed_time: f32,
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
        timers: Vec::new(),
        coroutines: Vec::new(),
        starting: Vec::new(),
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
        let mut host = EntityHost::new(world, e, Some(res.clone()), &mut commands, None);
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

/// Does the script define this hook?
fn has_hook(state: &ScriptState, name: &str) -> bool {
    match &state.object {
        Some(obj) => state.interp.has_method(obj, name),
        None => state.interp.has_function(name),
    }
}

/// Call a lifecycle hook if the script defines it.
fn call_hook(
    world: &mut World,
    e: Entity,
    state: &mut ScriptState,
    name: &str,
    args: Vec<Value>,
    res: &Option<Resources>,
    states: Option<&mut States>,
) {
    if state.failed || !has_hook(state, name) {
        return;
    }
    let mut commands = Vec::new();
    let result = {
        let mut host = EntityHost::new(world, e, res.clone(), &mut commands, states);
        match &state.object {
            Some(obj) => state.interp.call_method(obj, name, args, &mut host).map(|_| ()),
            None => state.interp.call_function(name, args, &mut host).map(|_| ()),
        }
    };
    take_timers(state, &mut commands);
    apply_commands(world, commands);
    if let Err(err) = result {
        state.failed = true;
        report(world, &state.path, &err);
    }
}

/// Call a script function for its result (diagrams' Run Script). An error goes
/// back to the caller instead of stopping the script.
fn call_for_result(
    world: &mut World,
    e: Entity,
    state: &mut ScriptState,
    function: &str,
    args: Vec<Value>,
    res: &Option<Resources>,
    states: &mut States,
) -> Result<Value, String> {
    if state.failed {
        return Err(format!("{} stopped after an error", state.path));
    }
    if !has_hook(state, function) {
        return Err(format!("{} has no function `{function}`", state.path));
    }
    let mut commands = Vec::new();
    let result = {
        let mut host = EntityHost::new(world, e, res.clone(), &mut commands, Some(states));
        match &state.object {
            Some(obj) => state.interp.call_method(obj, function, args, &mut host),
            None => state.interp.call_function(function, args, &mut host),
        }
    };
    take_timers(state, &mut commands);
    apply_commands(world, commands);
    result.map_err(|err| err.to_string())
}

/// Timer requests made by the script that just ran belong to its own state.
fn take_timers(state: &mut ScriptState, commands: &mut Vec<Command>) {
    commands.retain_mut(|c| match c {
        Command::AddTimer(t) => {
            state.timers.push(t.clone());
            false
        }
        Command::CancelTimer(id) => {
            state.timers.retain(|t| t.id != *id);
            false
        }
        Command::StartCoroutine { id, method, args } => {
            state.starting.push((*id, std::mem::take(method), std::mem::take(args)));
            false
        }
        Command::StopCoroutine(id) => {
            state.coroutines.retain(|(c, ..)| c != id);
            state.starting.retain(|(c, ..)| c != id);
            false
        }
        _ => true,
    });
}

/// Start requested coroutines and run every one that's due until its next WAIT.
fn run_coroutines(
    world: &mut World,
    e: Entity,
    st: &mut ScriptState,
    res: &Option<Resources>,
    states: &mut States,
    now: f64,
) {
    if st.failed {
        return;
    }
    for (id, method, args) in std::mem::take(&mut st.starting) {
        let obj = st.object.clone().unwrap_or(Value::Null);
        match st.interp.start_coroutine(&obj, &method, args) {
            Ok(co) => st.coroutines.push((id, co, now)),
            Err(err) => report(world, &st.path, &err),
        }
    }
    let mut i = 0;
    while i < st.coroutines.len() {
        if st.coroutines[i].2 > now || !world.is_alive(e) {
            i += 1;
            continue;
        }
        let (id, mut co, _) = st.coroutines.remove(i);
        let mut commands = Vec::new();
        let result = {
            let mut host = EntityHost::new(world, e, res.clone(), &mut commands, Some(&mut *states));
            st.interp.resume(&mut co, &mut host)
        };
        take_timers(st, &mut commands);
        apply_commands(world, commands);
        match result {
            // WAIT 0 waits for the next frame.
            Ok(Some(wait)) => {
                let at = if wait <= 0.0 { now + 1e-9 } else { now + wait };
                st.coroutines.insert(i, (id, co, at));
                i += 1;
            }
            Ok(None) => {}
            Err(err) => report(world, &st.path, &err),
        }
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
    call_hook(world, entity, &mut state, hook, Vec::new(), &res, None);
}

fn apply_commands(world: &mut World, commands: Vec<Command>) {
    for c in commands {
        match c {
            Command::Despawn(e) => kit_scene::despawn_recursive(world, e),
            Command::Scene(op) => kit_gamesys::scenes::request(world, op),
            // Timers are taken by the script that asked (see `take_timers`).
            Command::AddTimer(_)
            | Command::CancelTimer(_)
            | Command::StartCoroutine { .. }
            | Command::StopCoroutine(_) => {}
        }
    }
}

/// Should this object's script run? Not when the object (or a parent) is
/// switched off with `BeActive(FALSE)`, or its script feature is disabled.
fn runs(world: &World, e: Entity) -> bool {
    world.is_alive(e) && kit_scene::is_active(world, e) && !kit_gamesys::feature_disabled(world, e, "Script")
}

/// Run `f` on one object's script with the others reachable (it's taken out
/// of the map meanwhile, so it can't be called into while it runs).
fn with_state(
    rt: &mut ScriptRuntime,
    world: &mut World,
    e: Entity,
    f: impl FnOnce(&mut World, &mut ScriptState, &mut States),
) {
    let Some(mut st) = rt.states.remove(&e) else { return };
    f(world, &mut st, &mut rt.states);
    if world.is_alive(e) {
        rt.states.insert(e, st);
    }
}

/// Steady step for `FixedUpdate` (matches the physics step).
pub const FIXED_STEP: f32 = 1.0 / 60.0;

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
    // OnCollision(other[, hit]) / OnCollisionEnd(other) / OnTrigger(other) / OnTriggerEnd(other)
    let events: Vec<kit_gamesys::CollisionEvent> =
        world.try_resource::<kit_gamesys::CollisionEvents>().map(|ev| ev.events.clone()).unwrap_or_default();
    for ev in events {
        let hook = match (ev.trigger, ev.started) {
            (false, true) => "OnCollision",
            (false, false) => "OnCollisionEnd",
            (true, true) => "OnTrigger",
            (true, false) => "OnTriggerEnd",
        };
        for (me, other, sign) in [(ev.a, ev.b, 1.0f32), (ev.b, ev.a, -1.0)] {
            if !rt.states.get(&me).is_some_and(|s| s.initialized) || !runs(world, me) {
                continue;
            }
            with_state(&mut rt, world, me, |world, st, states| {
                let mut args = vec![Value::Int(other.to_bits() as i64)];
                // A second parameter receives where and how hard they hit.
                if ev.started && hook_arity(st, hook) >= 2 {
                    args.push(host::hit_value(&ev, sign));
                }
                call_hook(world, me, st, hook, args, &res, Some(states));
            });
        }
    }

    let entities = world.query::<(Script,)>();
    // Drop states for entities that lost their script.
    let gone: Vec<Entity> = rt.states.keys().filter(|e| !entities.contains(e)).copied().collect();
    for e in gone {
        let mut st = rt.states.remove(&e).unwrap();
        if world.is_alive(e) {
            call_hook(world, e, &mut st, "Final", Vec::new(), &res, Some(&mut rt.states));
        }
    }

    // 1. Load (or reload) every script and run Init, before any Update, so
    //    every object's features can be found from the very first frame.
    let mut active = Vec::new();
    for &e in &entities {
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
                call_hook(world, e, &mut old, "Final", Vec::new(), &res, Some(&mut rt.states));
                if old.path == path {
                    world.resource_mut::<Console>().info("x", format!("reloaded {path}"));
                }
            }
            let st = load_state(world, e, &path, &res);
            rt.states.insert(e, st);
        }
        if runs(world, e) {
            active.push(e);
        }
    }
    for &e in &active {
        if rt.states.get(&e).is_some_and(|s| !s.initialized) {
            with_state(&mut rt, world, e, |world, st, states| {
                st.initialized = true;
                call_hook(world, e, st, "Init", Vec::new(), &res, Some(states));
            });
        }
    }

    // Clicks: OnClick(hit) on the script of the object under the pointer.
    let click = world.try_resource::<kit_input::Input>().filter(|i| i.pressed("MouseLeft")).map(|i| i.mouse);
    if let Some(hit) = click.and_then(|px| host::pointer_hit(world, px)) {
        let e = hit.entity;
        if runs(world, e) && rt.states.get(&e).is_some_and(|s| s.initialized && has_hook(s, "OnClick")) {
            with_state(&mut rt, world, e, |world, st, states| {
                let info = Value::object(vec![
                    ("Point", Value::vector(hit.point.x as f64, hit.point.y as f64, hit.point.z as f64)),
                    ("Normal", Value::vector(hit.normal.x as f64, hit.normal.y as f64, hit.normal.z as f64)),
                    ("Distance", Value::Float(hit.distance as f64)),
                ]);
                let args = if hook_arity(st, "OnClick") >= 1 { vec![info] } else { Vec::new() };
                call_hook(world, e, st, "OnClick", args, &res, Some(states));
            });
        }
    }

    // 2. Diagrams' Run Script nodes: run the function, send the result back.
    let calls = world
        .try_resource_mut::<kit_gamesys::ScriptCalls>()
        .map(|mut c| std::mem::take(&mut c.pending))
        .unwrap_or_default();
    for call in calls {
        let e = call.target;
        let result = if !rt.states.contains_key(&e) || !world.is_alive(e) {
            Err(format!(
                "{} has no script running",
                world.get::<kit_scene::Name>(e).map(|n| n.0.clone()).unwrap_or_default()
            ))
        } else if !runs(world, e) {
            Err("that object or its script is switched off".to_owned())
        } else {
            let mut out = Err(String::new());
            let args: Vec<Value> = call.args.iter().map(host::from_bridge).collect();
            with_state(&mut rt, world, e, |world, st, states| {
                out = call_for_result(world, e, st, &call.function, args, &res, states);
            });
            out.map(|v| if matches!(v, Value::Null) { None } else { host::to_bridge(&v) })
        };
        if let Some(mut c) = world.try_resource_mut::<kit_gamesys::ScriptCalls>() {
            c.results.push((call.id, result));
        }
    }

    // 3. Timers that are due (Time.After / Time.Every).
    for &e in &active {
        let due: Vec<host::Timer> = match rt.states.get_mut(&e) {
            Some(st) => {
                let (due, wait): (Vec<_>, Vec<_>) =
                    std::mem::take(&mut st.timers).into_iter().partition(|t| t.due <= elapsed);
                st.timers = wait;
                for t in &due {
                    if let Some(every) = t.every {
                        st.timers.push(host::Timer { due: t.due + every.max(1e-3), ..t.clone() });
                    }
                }
                due
            }
            None => continue,
        };
        for t in due {
            if !runs(world, e) {
                break;
            }
            with_state(&mut rt, world, e, |world, st, states| {
                call_hook(world, e, st, &t.method, t.args.clone(), &res, Some(states));
            });
        }
    }

    // 3b. Coroutines whose WAIT is over (and ones started last frame).
    for &e in &active {
        if runs(world, e) && rt.states.get(&e).is_some_and(|s| !s.coroutines.is_empty() || !s.starting.is_empty()) {
            with_state(&mut rt, world, e, |world, st, states| run_coroutines(world, e, st, &res, states, elapsed));
        }
    }

    // 4. FixedUpdate at a steady rate (as many steps as this frame needs).
    rt.fixed_time = (rt.fixed_time + dt).min(FIXED_STEP * 5.0);
    while rt.fixed_time >= FIXED_STEP {
        rt.fixed_time -= FIXED_STEP;
        for &e in &active {
            if runs(world, e) && rt.states.get(&e).is_some_and(|s| has_hook(s, "FixedUpdate")) {
                with_state(&mut rt, world, e, |world, st, states| {
                    call_hook(world, e, st, "FixedUpdate", vec![Value::Float(FIXED_STEP as f64)], &res, Some(states));
                });
            }
        }
    }

    // 5. Update.
    for &e in &active {
        if runs(world, e) {
            with_state(&mut rt, world, e, |world, st, states| {
                call_hook(world, e, st, "Update", vec![Value::Float(dt as f64)], &res, Some(states));
            });
        }
    }
    world.insert_resource(rt);
}

/// How many parameters a hook takes (0 when it isn't defined).
fn hook_arity(state: &ScriptState, name: &str) -> usize {
    match &state.object {
        Some(Value::Instance(i)) => i.class.method(name).map(|f| f.params.len()).unwrap_or(0),
        _ => 0,
    }
}

/// `LateUpdate(dt)`: after every Update, physics and movement this frame, with
/// world positions brought up to date (cameras that follow things go here).
pub fn late_scripts(world: &mut World) {
    let Some(mut rt) = world.remove_resource::<ScriptRuntime>() else { return };
    let late: Vec<Entity> =
        rt.states.iter().filter(|(_, s)| s.initialized && has_hook(s, "LateUpdate")).map(|(e, _)| *e).collect();
    if !late.is_empty() {
        let res = world.try_resource::<Resources>().map(|r| r.clone());
        let dt = world.resource::<Time>().delta;
        kit_scene::propagate_transforms(world);
        for e in late {
            if runs(world, e) {
                with_state(&mut rt, world, e, |world, st, states| {
                    call_hook(world, e, st, "LateUpdate", vec![Value::Float(dt as f64)], &res, Some(states));
                });
            }
        }
        kit_scene::propagate_transforms(world);
    }
    world.insert_resource(rt);
}

/// Call `Final` on every script and drop all instances (play stopped).
pub fn stop_scripts(world: &mut World) {
    let Some(mut rt) = world.remove_resource::<ScriptRuntime>() else { return };
    let res = world.try_resource::<Resources>().map(|r| r.clone());
    let all: Vec<Entity> = rt.states.keys().copied().collect();
    for e in all {
        let Some(mut st) = rt.states.remove(&e) else { continue };
        if world.is_alive(e) {
            call_hook(world, e, &mut st, "Final", Vec::new(), &res, Some(&mut rt.states));
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
        app.add_play_system(Stage::PostUpdate, "x_late_update", late_scripts);
    }
}

#[cfg(test)]
mod tests;
