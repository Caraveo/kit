//! Running graphs: events, execution flow, value evaluation, latent nodes
//! (Delay, Move Over Time, Run Script) and per-node debug state.
//!
//! **Frame order** (deterministic): every frame, in Play only,
//!
//! 1. waiting continuations that are due (in the order they were scheduled);
//! 2. **On Start** for new graph instances;
//! 3. **On Custom Event**, in the order events were emitted (by graphs and
//!    scripts during the previous frame);
//! 4. **On Trigger Enter / Exit**, in physics event order;
//! 5. **On Interact**;
//! 6. **On Timer**.
//!
//! Within a step, instances run in scene (hierarchy) order and event nodes in
//! node-id order. Execution follows white wires depth-first; a Sequence runs
//! its outputs top to bottom. A frame may run at most [`BUDGET`] nodes: a graph
//! that loops or re-enters itself is stopped there, with the node flagged.

use crate::catalog::{BoolOp, CompareOp, Conversion, MathOp, NodeKind, NumberType, PinType, Space};
use crate::graph::{Graph, NodeId, PinRef, Scope, TargetRef};
use crate::validate::{Diagnostic, has_errors, validate};
use glam::{Mat4, Vec3, Vec4};
use kit_core::Console;
use kit_ecs::{Entity, World};
use kit_gamedata::{GameData, Value};
use kit_gamesys::{
    AudioCommand, AudioCommands, AudioSource, BridgeValue, Diagram, GraphEvent, GraphEvents, Model, Physics, RigidBody,
    ScriptCall, ScriptCalls, Sprite,
};
use kit_scene::{GlobalTransform, Name, Parent, Transform};
use std::collections::HashMap;
use std::sync::Arc;

/// Nodes one frame may run (all graphs together).
pub const BUDGET: u32 = 10_000;
/// How deep value nodes may nest (guards against cycles that slip past validation).
const MAX_EVAL_DEPTH: u32 = 64;
/// On Interact: how far the player can reach.
pub const INTERACT_RANGE: f32 = 3.0;
/// On Interact: the key (Kit's input name).
pub const INTERACT_KEY: &str = "e";

/// What the editor shows while playing.
#[derive(Clone, Debug, Default)]
pub struct DebugInfo {
    /// Node → play time it last ran.
    pub node_time: HashMap<NodeId, f64>,
    /// Exec link (from, to) → play time it last carried execution.
    pub link_time: HashMap<(PinRef, PinRef), f64>,
    /// Most recent value seen on each pin.
    pub values: HashMap<PinRef, Value>,
    /// The node that failed most recently, and why.
    pub error: Option<(NodeId, String)>,
}

/// One object running a graph.
pub struct Instance {
    pub entity: Entity,
    pub path: String,
    pub graph: Arc<Graph>,
    vars: HashMap<String, Value>,
    /// Values produced by event and latent nodes (Other Entity, Result, …).
    outputs: HashMap<PinRef, Value>,
    timers: HashMap<NodeId, f64>,
    started: bool,
    pub debug: DebugInfo,
}

enum Wait {
    Until(f64),
    Move { target: Entity, from: Vec3, to: Vec3, start: f64, duration: f64, dynamic: bool },
    Script(u64),
}

struct Continuation {
    instance: Entity,
    node: NodeId,
    wait: Wait,
}

struct Loaded {
    graph: Arc<Graph>,
    ok: bool,
}

/// All running graphs (a world resource). Cleared when Play stops.
#[derive(Default)]
pub struct DiagramRuntime {
    pub instances: Vec<Instance>,
    pub scene_vars: HashMap<String, Value>,
    waits: Vec<Continuation>,
    graphs: HashMap<String, Loaded>,
    /// Validation results per graph path (for the editor).
    pub diagnostics: HashMap<String, Vec<Diagnostic>>,
    next_call: u64,
    budget: u32,
    now: f64,
    dt: f32,
}

impl Instance {
    /// Current object variables (for the editor's debug view and tests).
    pub fn vars_snapshot(&self) -> &HashMap<String, Value> {
        &self.vars
    }
}

impl DiagramRuntime {
    pub fn instance(&self, e: Entity) -> Option<&Instance> {
        self.instances.iter().find(|i| i.entity == e)
    }
    /// Pending Delay / Move / Run Script continuations.
    pub fn pending(&self) -> usize {
        self.waits.len()
    }
}

/// Stop every graph and forget all runtime state (Play stopped, scene reloaded).
pub fn reset(world: &mut World) {
    if let Some(mut rt) = world.try_resource_mut::<DiagramRuntime>() {
        *rt = DiagramRuntime::default();
    }
}

struct Ctx<'a> {
    world: &'a World,
    gd: &'a GameData,
}

type Run<T> = Result<T, String>;

fn entity_value(e: Entity) -> Value {
    Value::Entity(Some(e.to_bits()))
}

fn label(world: &World, e: Entity) -> String {
    world.get::<Name>(e).map(|n| n.0.clone()).unwrap_or_else(|| "object".into())
}

/// Scene objects in hierarchy order.
fn scene_order(world: &World) -> Vec<Entity> {
    let mut out = Vec::new();
    for r in kit_scene::roots(world) {
        out.extend(kit_scene::descendants_and_self(world, r));
    }
    out
}

/// An object by its hierarchy path ("Room/Door"), or a unique name.
pub fn find_path(world: &World, path: &str) -> Option<Entity> {
    let mut parts = path.split('/').filter(|p| !p.is_empty());
    let first = parts.next()?;
    let mut current = kit_scene::roots(world).into_iter().find(|e| world.get::<Name>(*e).is_some_and(|n| n.0 == first));
    for part in parts {
        let parent = current?;
        current = kit_scene::children(world, parent).into_iter().find(|e| world.get::<Name>(*e).is_some_and(|n| n.0 == part));
    }
    current.or_else(|| if !path.contains('/') { kit_scene::find_by_name(world, path) } else { None })
}

/// The hierarchy path of an object (what Pick from Scene stores).
pub fn path_of(world: &World, e: Entity) -> String {
    let mut parts = vec![label(world, e)];
    let mut cur = e;
    while let Some(p) = world.get::<Parent>(cur).map(|p| p.0) {
        parts.push(label(world, p));
        cur = p;
    }
    parts.reverse();
    parts.join("/")
}

fn world_position(world: &World, e: Entity) -> Vec3 {
    world.get::<GlobalTransform>(e).map(|g| g.0.w_axis.truncate()).unwrap_or_default()
}

/// Move an object so its world position is `p` (respecting its parent).
fn set_world_position(world: &World, e: Entity, p: Vec3) {
    let parent_inv = world
        .get::<Parent>(e)
        .and_then(|par| world.get::<GlobalTransform>(par.0).map(|g| g.0.inverse()))
        .unwrap_or(Mat4::IDENTITY);
    if let Some(mut t) = world.get_mut::<Transform>(e) {
        t.translation = parent_inv.transform_point3(p);
    }
    if let Some(mut g) = world.get_mut::<GlobalTransform>(e) {
        g.0.w_axis = p.extend(1.0);
    }
}

fn is_dynamic(world: &World, e: Entity) -> bool {
    world.get::<RigidBody>(e).is_some_and(|b| b.kind == kit_gamesys::BodyKind::Dynamic) && world.has_resource::<Physics>()
}

pub fn to_bridge(v: &Value) -> Option<BridgeValue> {
    Some(match v {
        Value::Bool(b) => BridgeValue::Bool(*b),
        Value::Int(i) => BridgeValue::Int(*i),
        Value::Float(f) => BridgeValue::Float(*f),
        Value::Text(t) => BridgeValue::Text(t.clone()),
        Value::Vector3(v) => BridgeValue::Vector3(*v),
        Value::Color(c) => BridgeValue::Color(*c),
        Value::Entity(e) => BridgeValue::Entity(*e),
        _ => return None,
    })
}

pub fn from_bridge(v: &BridgeValue) -> Value {
    match v {
        BridgeValue::Bool(b) => Value::Bool(*b),
        BridgeValue::Int(i) => Value::Int(*i),
        BridgeValue::Float(f) => Value::Float(*f),
        BridgeValue::Text(t) => Value::Text(t.clone()),
        BridgeValue::Vector3(v) => Value::Vector3(*v),
        BridgeValue::Color(c) => Value::Color(*c),
        BridgeValue::Entity(e) => Value::Entity(*e),
    }
}

fn from_field(v: &kit_gamesys::FieldValue) -> Value {
    use kit_gamesys::FieldValue as F;
    match v {
        F::Bool(b) => Value::Bool(*b),
        F::Int(i) => Value::Int(*i),
        F::Float(f) => Value::Float(*f),
        F::Text(t) => Value::Text(t.clone()),
        F::Vector(a) => Value::Vector3([a[0] as f32, a[1] as f32, a[2] as f32]),
    }
}

fn to_field(v: &Value) -> Option<kit_gamesys::FieldValue> {
    use kit_gamesys::FieldValue as F;
    Some(match v {
        Value::Bool(b) => F::Bool(*b),
        Value::Int(i) => F::Int(*i),
        Value::Float(f) => F::Float(*f),
        Value::Text(t) => F::Text(t.clone()),
        Value::Vector3(a) => F::Vector([a[0] as f64, a[1] as f64, a[2] as f64]),
        _ => return None,
    })
}

fn v3(v: &Value) -> Vec3 {
    match v {
        Value::Vector3(a) => Vec3::from_array(*a),
        _ => Vec3::ZERO,
    }
}

fn f(v: &Value) -> f64 {
    v.as_f64().unwrap_or(0.0)
}

impl DiagramRuntime {
    fn inst(&self, idx: usize) -> &Instance {
        &self.instances[idx]
    }

    fn scope_of(&self, idx: usize, name: &str) -> Scope {
        self.inst(idx).graph.variable(name).map_or(Scope::Object, |v| v.scope)
    }

    fn var(&self, idx: usize, name: &str) -> Option<Value> {
        match self.scope_of(idx, name) {
            Scope::Object => self.inst(idx).vars.get(name).cloned(),
            Scope::Scene => self.scene_vars.get(name).cloned(),
        }
    }

    fn set_var(&mut self, idx: usize, name: &str, value: Value) {
        match self.scope_of(idx, name) {
            Scope::Object => {
                self.instances[idx].vars.insert(name.to_owned(), value);
            }
            Scope::Scene => {
                self.scene_vars.insert(name.to_owned(), value);
            }
        }
    }

    fn note(&mut self, idx: usize, pin: PinRef, value: &Value) {
        self.instances[idx].debug.values.insert(pin, value.clone());
    }

    /// The value on an input pin: its wire, else its inline value, else the
    /// pin's default.
    fn input(&mut self, cx: &Ctx, idx: usize, node: NodeId, pin: &str, depth: u32) -> Run<Value> {
        let graph = self.inst(idx).graph.clone();
        let to = PinRef::new(node, pin);
        let n = graph.node(node).ok_or("missing node")?;
        let sig = n.kind.signature_in(cx.gd, &graph);
        let expected = match sig.input(pin).map(|p| &p.ty) {
            Some(PinType::Data(t)) => t.clone(),
            _ => return Err(format!("no input {pin}")),
        };
        let value = if let Some(link) = graph.link_into(&to) {
            let src = graph.node(link.from.node).ok_or("a wire points to a missing node")?;
            let v = if src.kind.is_pure(cx.gd) {
                self.pure(cx, idx, link.from.node, &link.from.pin, depth + 1)?
            } else {
                self.inst(idx).outputs.get(&link.from).cloned().unwrap_or_else(|| expected.default_value())
            };
            self.note(idx, link.from.clone(), &v);
            v
        } else {
            match n.inputs.get(pin) {
                Some(v) if v.value_type() == expected => v.clone(),
                _ => sig.input(pin).and_then(|p| p.default.clone()).unwrap_or_else(|| expected.default_value()),
            }
        };
        self.note(idx, to, &value);
        Ok(value)
    }

    /// Who an object node acts on: its Target wire, else Self or a picked object.
    fn target(&mut self, cx: &Ctx, idx: usize, node: NodeId, depth: u32) -> Run<Entity> {
        let graph = self.inst(idx).graph.clone();
        let n = graph.node(node).ok_or("missing node")?;
        let linked = graph.link_into(&PinRef::new(node, "target")).is_some();
        let e = if linked {
            match self.input(cx, idx, node, "target", depth)? {
                Value::Entity(Some(bits)) => Entity::from_bits(bits),
                _ => return Err("its Target input has no object".into()),
            }
        } else {
            match &n.target {
                TargetRef::SelfObject => self.inst(idx).entity,
                TargetRef::Picked(path) => find_path(cx.world, path).ok_or(format!("the picked object \"{path}\" isn't in the scene"))?,
            }
        };
        if !cx.world.is_alive(e) {
            return Err("its target object no longer exists".into());
        }
        Ok(e)
    }

    /// Evaluate a value-only node's output.
    fn pure(&mut self, cx: &Ctx, idx: usize, node: NodeId, out: &str, depth: u32) -> Run<Value> {
        if depth > MAX_EVAL_DEPTH {
            return Err("value nodes feed into each other in a loop".into());
        }
        let graph = self.inst(idx).graph.clone();
        let n = graph.node(node).ok_or("missing node")?.clone();
        let w = cx.world;
        let v = match &n.kind {
            NodeKind::GetSelf => entity_value(self.inst(idx).entity),
            NodeKind::GetPosition { space } => {
                let e = self.target(cx, idx, node, depth)?;
                let p = match space {
                    Space::World => world_position(w, e),
                    Space::Local => w.get::<Transform>(e).map(|t| t.translation).unwrap_or_default(),
                };
                Value::Vector3(p.to_array())
            }
            NodeKind::GetVelocity => {
                let e = self.target(cx, idx, node, depth)?;
                let physics = w.try_resource::<Physics>().ok_or("physics is off in this project")?;
                let v = physics.0.linear_velocity(e).ok_or(format!("\"{}\" has no Rigid Body", label(w, e)))?;
                Value::Vector3(v.to_array())
            }
            NodeKind::DatasetRow { dataset, row } => {
                let ds = cx.gd.dataset(dataset).ok_or("its dataset was deleted")?;
                let row_id = if graph.link_into(&PinRef::new(node, "row")).is_some() {
                    match self.input(cx, idx, node, "row", depth)? {
                        Value::Row { row: Some(r), .. } => r,
                        _ => return Err("its Row input has no row".into()),
                    }
                } else {
                    row.clone().ok_or("pick a row")?
                };
                let field = ds.field(out).ok_or("that field was deleted")?;
                ds.value(&row_id, &field.id).ok_or(format!("row not found in {}", ds.name))?
            }
            NodeKind::Constant { constant } => cx.gd.constant(constant).ok_or("its constant was deleted")?.value.clone(),
            NodeKind::GetVariable { name } => self.var(idx, name).ok_or(format!("no variable \"{name}\""))?,
            NodeKind::GetScriptField { field, value_type } => {
                let e = self.target(cx, idx, node, depth)?;
                let rt = w.try_resource::<kit_script::ScriptRuntime>().ok_or("scripts are off")?;
                let f = rt.fields(e).into_iter().find(|f| f.name == *field).ok_or_else(|| {
                    format!("\"{}\" has no running script with a PUBLIC or EDIT field `{field}`", label(w, e))
                })?;
                let v = f.value.as_ref().map(from_field).ok_or(format!("`{field}` isn't a value graphs can read"))?;
                if v.value_type() != *value_type {
                    return Err(format!("`{field}` is now {}, not {}", f.ty, value_type.label()));
                }
                v
            }
            NodeKind::Compare { op, number } => {
                let a = self.input(cx, idx, node, "a", depth)?;
                let b = self.input(cx, idx, node, "b", depth)?;
                let ord = match number {
                    NumberType::Vector3 => {
                        let eq = v3(&a).abs_diff_eq(v3(&b), 1e-5);
                        if eq { Some(std::cmp::Ordering::Equal) } else { None }
                    }
                    _ => f(&a).partial_cmp(&f(&b)),
                };
                use std::cmp::Ordering::*;
                Value::Bool(match op {
                    CompareOp::Equal => ord == Some(Equal),
                    CompareOp::NotEqual => ord != Some(Equal),
                    CompareOp::Less => ord == Some(Less),
                    CompareOp::LessOrEqual => matches!(ord, Some(Less | Equal)),
                    CompareOp::Greater => ord == Some(Greater),
                    CompareOp::GreaterOrEqual => matches!(ord, Some(Greater | Equal)),
                })
            }
            NodeKind::Math { op, number } => {
                let a = self.input(cx, idx, node, "a", depth)?;
                let b = self.input(cx, idx, node, "b", depth)?;
                match number {
                    NumberType::Int => {
                        let (a, b) = (if let Value::Int(i) = a { i } else { 0 }, if let Value::Int(i) = b { i } else { 0 });
                        Value::Int(match op {
                            MathOp::Add => a.wrapping_add(b),
                            MathOp::Subtract => a.wrapping_sub(b),
                            MathOp::Multiply => a.wrapping_mul(b),
                            MathOp::Divide => a.checked_div(b).ok_or("divided by zero")?,
                        })
                    }
                    NumberType::Float => {
                        let (a, b) = (f(&a), f(&b));
                        Value::Float(match op {
                            MathOp::Add => a + b,
                            MathOp::Subtract => a - b,
                            MathOp::Multiply => a * b,
                            MathOp::Divide if b == 0.0 => return Err("divided by zero".into()),
                            MathOp::Divide => a / b,
                        })
                    }
                    NumberType::Vector3 => {
                        let (a, b) = (v3(&a), v3(&b));
                        Value::Vector3(
                            match op {
                                MathOp::Add => a + b,
                                MathOp::Subtract => a - b,
                                MathOp::Multiply => a * b,
                                MathOp::Divide if b.cmpeq(Vec3::ZERO).any() => return Err("divided by zero".into()),
                                MathOp::Divide => a / b,
                            }
                            .to_array(),
                        )
                    }
                }
            }
            NodeKind::Logic { op } => {
                let a = matches!(self.input(cx, idx, node, "a", depth)?, Value::Bool(true));
                Value::Bool(match op {
                    BoolOp::Not => !a,
                    BoolOp::And => a && matches!(self.input(cx, idx, node, "b", depth)?, Value::Bool(true)),
                    BoolOp::Or => a || matches!(self.input(cx, idx, node, "b", depth)?, Value::Bool(true)),
                })
            }
            NodeKind::Convert { conversion } => {
                let v = self.input(cx, idx, node, "value", depth)?;
                match conversion {
                    Conversion::IntToFloat => Value::Float(f(&v)),
                    Conversion::FloatToInt => Value::Int(f(&v).round() as i64),
                    Conversion::FloatToText => Value::Text(format!("{:.2}", f(&v))),
                    Conversion::IntToText => Value::Text(format!("{}", f(&v) as i64)),
                }
            }
            NodeKind::MakeVector3 => {
                let x = f(&self.input(cx, idx, node, "x", depth)?) as f32;
                let y = f(&self.input(cx, idx, node, "y", depth)?) as f32;
                let z = f(&self.input(cx, idx, node, "z", depth)?) as f32;
                Value::Vector3([x, y, z])
            }
            NodeKind::BreakVector3 => {
                let v = v3(&self.input(cx, idx, node, "vector", depth)?);
                Value::Float(match out {
                    "x" => v.x,
                    "y" => v.y,
                    _ => v.z,
                } as f64)
            }
            other => return Err(format!("{} isn't a value node", other.title(cx.gd))),
        };
        self.note(idx, PinRef::new(node, out), &v);
        Ok(v)
    }

    /// Run one action node; returns the exec outputs to follow, in order.
    fn execute(&mut self, cx: &Ctx, idx: usize, node: NodeId) -> Run<Vec<String>> {
        let graph = self.inst(idx).graph.clone();
        let n = graph.node(node).ok_or("missing node")?.clone();
        let w = cx.world;
        let then = || Ok(vec!["then".to_owned()]);
        let missing = |e: Entity, what: &str| format!("\"{}\" has no {what}", label(w, e));
        match &n.kind {
            NodeKind::SetPosition { space } => {
                let e = self.target(cx, idx, node, 0)?;
                let p = v3(&self.input(cx, idx, node, "position", 0)?);
                match space {
                    Space::World => set_world_position(w, e, p),
                    Space::Local => {
                        w.get_mut::<Transform>(e).ok_or_else(|| missing(e, "Transform"))?.translation = p;
                    }
                }
                if is_dynamic(w, e) {
                    // Teleport the physics body too.
                    if let Some(mut ph) = w.try_resource_mut::<Physics>() {
                        ph.0.set_linear_velocity(e, Vec3::ZERO);
                    }
                }
                then()
            }
            NodeKind::SetRotation { space } => {
                let e = self.target(cx, idx, node, 0)?;
                let deg = v3(&self.input(cx, idx, node, "rotation", 0)?);
                let mut t = w.get_mut::<Transform>(e).ok_or_else(|| missing(e, "Transform"))?;
                t.set_euler_degrees(deg);
                if *space == Space::World {
                    // World rotation: undo the parent's rotation.
                    if let Some(parent) = w.get::<Parent>(e).and_then(|p| w.get::<GlobalTransform>(p.0).map(|g| g.0)) {
                        let (_, prot, _) = parent.to_scale_rotation_translation();
                        t.rotation = (prot.inverse() * t.rotation).normalize();
                    }
                }
                then()
            }
            NodeKind::SetScale => {
                let e = self.target(cx, idx, node, 0)?;
                let s = v3(&self.input(cx, idx, node, "scale", 0)?);
                w.get_mut::<Transform>(e).ok_or_else(|| missing(e, "Transform"))?.scale = s;
                then()
            }
            NodeKind::SetTransform { space } => {
                let e = self.target(cx, idx, node, 0)?;
                let Value::Transform(tv) = self.input(cx, idx, node, "transform", 0)? else { return Err("needs a Transform".into()) };
                {
                    let mut t = w.get_mut::<Transform>(e).ok_or_else(|| missing(e, "Transform"))?;
                    t.set_euler_degrees(Vec3::from_array(tv.rotation));
                    t.scale = Vec3::from_array(tv.scale);
                    t.translation = Vec3::from_array(tv.position);
                }
                if *space == Space::World {
                    set_world_position(w, e, Vec3::from_array(tv.position));
                }
                then()
            }
            NodeKind::SetVisible => {
                let e = self.target(cx, idx, node, 0)?;
                let visible = matches!(self.input(cx, idx, node, "visible", 0)?, Value::Bool(true));
                let mut any = false;
                if let Some(mut m) = w.get_mut::<Model>(e) {
                    m.visible = visible;
                    any = true;
                }
                if let Some(mut s) = w.get_mut::<Sprite>(e) {
                    s.visible = visible;
                    any = true;
                }
                if !any {
                    return Err(missing(e, "Model or Sprite"));
                }
                then()
            }
            NodeKind::SetTint => {
                let e = self.target(cx, idx, node, 0)?;
                let Value::Color(c) = self.input(cx, idx, node, "color", 0)? else { return Err("needs a Color".into()) };
                let c = Vec4::from_array(c);
                if let Some(mut m) = w.get_mut::<Model>(e) {
                    m.color = c;
                } else if let Some(mut s) = w.get_mut::<Sprite>(e) {
                    s.color = c;
                } else {
                    return Err(missing(e, "Model or Sprite to tint"));
                }
                then()
            }
            NodeKind::SetGravityScale => {
                let e = self.target(cx, idx, node, 0)?;
                let v = f(&self.input(cx, idx, node, "value", 0)?) as f32;
                w.get_mut::<RigidBody>(e).ok_or_else(|| missing(e, "Rigid Body (add Physics to it)"))?.gravity_scale = v;
                then()
            }
            NodeKind::SetSceneGravity => {
                let g = v3(&self.input(cx, idx, node, "gravity", 0)?);
                w.try_resource_mut::<Physics>().ok_or("physics is off in this project")?.0.set_gravity(g);
                then()
            }
            NodeKind::ApplyImpulse | NodeKind::SetVelocity => {
                let e = self.target(cx, idx, node, 0)?;
                if !is_dynamic(w, e) {
                    return Err(missing(e, "dynamic Rigid Body"));
                }
                let (pin, impulse) = if matches!(n.kind, NodeKind::ApplyImpulse) { ("impulse", true) } else { ("velocity", false) };
                let v = v3(&self.input(cx, idx, node, pin, 0)?);
                let mut ph = w.try_resource_mut::<Physics>().ok_or("physics is off in this project")?;
                if impulse {
                    ph.0.apply_impulse(e, v);
                } else {
                    ph.0.set_linear_velocity(e, v);
                }
                then()
            }
            NodeKind::MoveOverTime => {
                let e = self.target(cx, idx, node, 0)?;
                let to = v3(&self.input(cx, idx, node, "destination", 0)?);
                let duration = f(&self.input(cx, idx, node, "duration", 0)?).max(0.0);
                let dynamic = is_dynamic(w, e);
                if dynamic {
                    if let Some(mut ph) = w.try_resource_mut::<Physics>() {
                        ph.0.suspend_gravity(e, true);
                    }
                }
                // A new move on the same object replaces the old one.
                self.waits.retain(|c| !matches!(c.wait, Wait::Move { target, .. } if target == e));
                let inst = self.inst(idx).entity;
                self.waits.push(Continuation {
                    instance: inst,
                    node,
                    wait: Wait::Move { target: e, from: world_position(w, e), to, start: self.now, duration, dynamic },
                });
                then()
            }
            NodeKind::SetScriptField { field, .. } => {
                let e = self.target(cx, idx, node, 0)?;
                let v = self.input(cx, idx, node, "value", 0)?;
                let fv = to_field(&v).ok_or("this value can't be stored in a script field")?;
                let mut rt = w.try_resource_mut::<kit_script::ScriptRuntime>().ok_or("scripts are off")?;
                let access = rt.fields(e).into_iter().find(|f| f.name == *field).map(|f| f.access);
                match access {
                    None => return Err(format!("\"{}\" has no running script with an EDIT field `{field}`", label(w, e))),
                    Some(kit_script::Access::Public) => {
                        return Err(format!("`{field}` is PUBLIC (read-only); write EDIT in front of it in the script to set it"));
                    }
                    _ => {}
                }
                if !rt.set_field(e, field, &fv) {
                    return Err(format!("`{field}` can't take a {}", v.value_type().label()));
                }
                then()
            }
            NodeKind::SetVariable { name } => {
                let v = self.input(cx, idx, node, "value", 0)?;
                self.set_var(idx, name, v);
                then()
            }
            NodeKind::Branch => {
                let c = matches!(self.input(cx, idx, node, "condition", 0)?, Value::Bool(true));
                Ok(vec![if c { "true".into() } else { "false".into() }])
            }
            NodeKind::Sequence { outputs } => Ok((0..(*outputs).max(2)).map(|i| format!("then{i}")).collect()),
            NodeKind::Delay => {
                let s = f(&self.input(cx, idx, node, "seconds", 0)?).max(0.0);
                let inst = self.inst(idx).entity;
                self.waits.push(Continuation { instance: inst, node, wait: Wait::Until(self.now + s) });
                Ok(Vec::new())
            }
            NodeKind::PlaySound => {
                let e = self.target(cx, idx, node, 0)?;
                let clip = match self.input(cx, idx, node, "clip", 0)? {
                    Value::Text(t) => t,
                    _ => String::new(),
                };
                let volume = f(&self.input(cx, idx, node, "volume", 0)?) as f32;
                let mut q = w.try_resource_mut::<AudioCommands>().ok_or("audio is off")?;
                if clip.trim().is_empty() {
                    if !w.has::<AudioSource>(e) {
                        return Err(missing(e, "Audio Source (or give Play Sound a sound file)"));
                    }
                    q.0.push(AudioCommand::Play(e));
                } else {
                    let at = w.get::<AudioSource>(e).filter(|s| s.spatial).map(|s| (world_position(w, e), s.min_distance, s.max_distance));
                    q.0.push(AudioCommand::OneShot { clip, volume, pitch: 1.0, at });
                }
                then()
            }
            NodeKind::StopSound => {
                let e = self.target(cx, idx, node, 0)?;
                w.try_resource_mut::<AudioCommands>().ok_or("audio is off")?.0.push(AudioCommand::Stop(e));
                then()
            }
            NodeKind::EmitEvent { event, payload } => {
                let value = if payload.is_some() { Some(self.input(cx, idx, node, "payload", 0)?) } else { None };
                let payload = match &value {
                    Some(v) => Some(to_bridge(v).ok_or("this payload type can't be sent")?),
                    None => None,
                };
                let sender = Some(self.inst(idx).entity);
                w.try_resource_mut::<GraphEvents>().ok_or("events are off")?.0.push(GraphEvent { name: event.clone(), payload, sender });
                then()
            }
            NodeKind::RunScript { function, args } => {
                let e = self.target(cx, idx, node, 0)?;
                if !w.has::<kit_gamesys::Script>(e) {
                    return Err(missing(e, "Script to run"));
                }
                let mut values = Vec::new();
                for i in 0..args.len() {
                    let v = self.input(cx, idx, node, &format!("arg{i}"), 0)?;
                    values.push(to_bridge(&v).ok_or("this argument type can't be passed to a script")?);
                }
                self.next_call += 1;
                let id = self.next_call;
                w.try_resource_mut::<ScriptCalls>()
                    .ok_or("scripts are off")?
                    .pending
                    .push(ScriptCall { id, target: e, function: function.clone(), args: values });
                let inst = self.inst(idx).entity;
                self.waits.push(Continuation { instance: inst, node, wait: Wait::Script(id) });
                Ok(Vec::new())
            }
            other => Err(format!("{} can't be run", other.title(cx.gd))),
        }
    }

    /// Follow execution from an output pin (depth-first).
    fn run_from(&mut self, cx: &Ctx, idx: usize, start: PinRef) {
        let graph = self.inst(idx).graph.clone();
        let mut stack = vec![start];
        while let Some(out) = stack.pop() {
            let targets: Vec<PinRef> = graph.links_from(&out).map(|l| l.to.clone()).collect();
            for to in targets.into_iter().rev() {
                self.instances[idx].debug.link_time.insert((out.clone(), to.clone()), self.now);
                if self.budget == 0 {
                    self.fail(cx, idx, to.node, format!("stopped: more than {BUDGET} nodes ran this frame (a loop?)"));
                    return;
                }
                self.budget -= 1;
                self.instances[idx].debug.node_time.insert(to.node, self.now);
                match self.execute(cx, idx, to.node) {
                    Ok(next) => {
                        for pin in next.into_iter().rev() {
                            stack.push(PinRef::new(to.node, &pin));
                        }
                    }
                    Err(e) => self.fail(cx, idx, to.node, e),
                }
            }
        }
    }

    fn fail(&mut self, cx: &Ctx, idx: usize, node: NodeId, message: String) {
        let inst = &self.instances[idx];
        let title = inst.graph.node(node).map_or("?".into(), |n| n.kind.title(cx.gd));
        let text = format!("{} › {title} on \"{}\": {message}", inst.path, label(cx.world, inst.entity));
        if let Some(mut c) = cx.world.try_resource_mut::<Console>() {
            c.error("diagram", text);
        }
        self.instances[idx].debug.error = Some((node, message));
    }

    /// Fire an event node (its outputs already set).
    fn fire(&mut self, cx: &Ctx, idx: usize, node: NodeId) {
        self.instances[idx].debug.node_time.insert(node, self.now);
        self.run_from(cx, idx, PinRef::new(node, "then"));
    }

    fn event_nodes(&self, idx: usize, pred: impl Fn(&NodeKind) -> bool) -> Vec<NodeId> {
        let mut ids: Vec<NodeId> = self.inst(idx).graph.nodes.iter().filter(|n| pred(&n.kind)).map(|n| n.id).collect();
        ids.sort();
        ids
    }

    fn index_of(&self, e: Entity) -> Option<usize> {
        self.instances.iter().position(|i| i.entity == e)
    }
}

/// Keep one instance per object with a Diagram (in scene order).
fn sync(rt: &mut DiagramRuntime, world: &World, gd: &GameData) {
    let order = scene_order(world);
    let wanted: Vec<(Entity, String)> =
        order.iter().filter_map(|e| world.get::<Diagram>(*e).map(|d| (*e, d.graph.clone()))).filter(|(_, p)| !p.trim().is_empty()).collect();
    rt.instances.retain(|i| wanted.iter().any(|(e, p)| *e == i.entity && *p == i.path));
    rt.waits.retain(|c| wanted.iter().any(|(e, _)| *e == c.instance));
    for (e, path) in &wanted {
        if rt.instances.iter().any(|i| i.entity == *e) {
            continue;
        }
        let loaded = rt.graphs.entry(path.clone()).or_insert_with(|| {
            let text = world.try_resource::<kit_resource::Resources>().map(|r| r.read_text(path));
            let graph = match text {
                Some(Ok(t)) => Graph::from_json(&t).map_err(|e| e.to_string()),
                Some(Err(e)) => Err(format!("{e:#}")),
                None => Err("no project".into()),
            };
            match graph {
                Ok(g) => {
                    let diags = validate(&g, gd);
                    let ok = !has_errors(&diags);
                    rt.diagnostics.insert(path.clone(), diags);
                    Loaded { graph: Arc::new(g), ok }
                }
                Err(e) => {
                    rt.diagnostics.insert(
                        path.clone(),
                        vec![Diagnostic { severity: crate::validate::Severity::Error, node: None, pin: None, message: e }],
                    );
                    Loaded { graph: Arc::new(Graph::default()), ok: false }
                }
            }
        });
        if !loaded.ok {
            if let Some(mut c) = world.try_resource_mut::<Console>() {
                let first = rt.diagnostics.get(path).and_then(|d| d.iter().find(|d| d.severity == crate::validate::Severity::Error)).map(|d| d.message.clone()).unwrap_or_default();
                c.error("diagram", format!("{path} has errors and won't run: {first}"));
            }
            // Still tracked (so the error isn't repeated), but with no nodes.
        }
        let graph = if loaded.ok { loaded.graph.clone() } else { Arc::new(Graph::default()) };
        for v in &graph.variables {
            if v.scope == Scope::Scene {
                rt.scene_vars.entry(v.name.clone()).or_insert_with(|| v.default.clone());
            }
        }
        let vars = graph.variables.iter().filter(|v| v.scope == Scope::Object).map(|v| (v.name.clone(), v.default.clone())).collect();
        rt.instances.push(Instance {
            entity: *e,
            path: path.clone(),
            graph,
            vars,
            outputs: HashMap::new(),
            timers: HashMap::new(),
            started: false,
            debug: DebugInfo::default(),
        });
    }
    // Scene order.
    rt.instances.sort_by_key(|i| order.iter().position(|e| *e == i.entity).unwrap_or(usize::MAX));
}

/// On Interact: what the player is reaching for. The active camera casts a
/// ray up to [`INTERACT_RANGE`]; the hit object (or its nearest ancestor)
/// with an On Interact graph is the target. Missing that, the nearest such
/// object within range of the camera.
fn interact_target(rt: &DiagramRuntime, world: &World, gd: &GameData) -> Option<(Entity, Entity)> {
    let cam = kit_gamesys::active_camera(world)?;
    let g = world.get::<GlobalTransform>(cam)?.0;
    let origin = g.w_axis.truncate();
    let forward = g.transform_vector3(Vec3::NEG_Z).normalize_or(Vec3::NEG_Z);
    let mut interactor = cam;
    while let Some(p) = world.get::<Parent>(interactor).map(|p| p.0) {
        interactor = p;
    }
    let listens = |e: Entity| rt.instance(e).is_some_and(|i| i.graph.nodes.iter().any(|n| matches!(n.kind, NodeKind::OnInteract)));
    if let Some(hit) = world.try_resource::<Physics>().and_then(|p| p.0.raycast(origin, forward, INTERACT_RANGE, Some(interactor))) {
        let mut e = hit.entity;
        loop {
            if listens(e) {
                return Some((e, interactor));
            }
            match world.get::<Parent>(e) {
                Some(p) => e = p.0,
                None => break,
            }
        }
    }
    let _ = gd;
    rt.instances
        .iter()
        .filter(|i| listens(i.entity))
        .map(|i| (i.entity, world_position(world, i.entity).distance(origin)))
        .filter(|(_, d)| *d <= INTERACT_RANGE)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(e, _)| (e, interactor))
}

/// The per-frame graph system (Play only).
pub fn run_diagrams(world: &mut World) {
    let Some(mut rt) = world.remove_resource::<DiagramRuntime>() else { return };
    let gd = world.try_resource::<GameData>().map(|g| g.clone()).unwrap_or_default();
    {
        let world = &*world;
        let (now, dt) = world.try_resource::<kit_core::Time>().map_or((0.0, 0.0), |t| (t.elapsed, t.delta));
        rt.now = now;
        rt.dt = dt;
        rt.budget = BUDGET;
        sync(&mut rt, world, &gd);
        let cx = Ctx { world, gd: &gd };

        // 1. Continuations that are due.
        let waits = std::mem::take(&mut rt.waits);
        let mut keep = Vec::new();
        let mut resume: Vec<(Entity, NodeId, &'static str)> = Vec::new();
        let script_results: Vec<(u64, Result<Option<BridgeValue>, String>)> =
            world.try_resource_mut::<ScriptCalls>().map(|mut s| std::mem::take(&mut s.results)).unwrap_or_default();
        let mut unclaimed = Vec::new();
        for c in waits {
            match &c.wait {
                Wait::Until(t) if *t <= now => resume.push((c.instance, c.node, "then")),
                Wait::Move { target, from, to, start, duration, dynamic } => {
                    let alive = world.is_alive(*target);
                    let t = if *duration <= 0.0 { 1.0 } else { ((now - start) / duration).clamp(0.0, 1.0) as f32 };
                    if alive {
                        let desired = from.lerp(*to, t);
                        if *dynamic {
                            // Dynamic bodies are steered by velocity (they still collide);
                            // gravity is suspended for the move.
                            let current = world_position(world, *target);
                            let v = if t >= 1.0 || dt <= 0.0 { Vec3::ZERO } else { (from.lerp(*to, ((now + dt as f64 - start) / duration).min(1.0) as f32) - current) / dt };
                            if let Some(mut ph) = world.try_resource_mut::<Physics>() {
                                ph.0.set_linear_velocity(*target, v);
                                if t >= 1.0 {
                                    ph.0.suspend_gravity(*target, false);
                                }
                            }
                        } else {
                            set_world_position(world, *target, desired);
                        }
                    }
                    if t >= 1.0 || !alive {
                        resume.push((c.instance, c.node, "completed"));
                    } else {
                        keep.push(c);
                    }
                }
                Wait::Script(id) => match script_results.iter().find(|(r, _)| r == id) {
                    Some((_, result)) => {
                        if let Some(i) = rt.index_of(c.instance) {
                            let (result_text, error_text, pin) = match result {
                                Ok(v) => (v.as_ref().map(|v| from_bridge(v).display()).unwrap_or_default(), String::new(), "then"),
                                Err(e) => (String::new(), e.clone(), "failed"),
                            };
                            rt.instances[i].outputs.insert(PinRef::new(c.node, "result"), Value::Text(result_text));
                            rt.instances[i].outputs.insert(PinRef::new(c.node, "error"), Value::Text(error_text.clone()));
                            if let Err(e) = result {
                                rt.fail(&cx, i, c.node, format!("the script failed: {e}"));
                            }
                            resume.push((c.instance, c.node, pin));
                        }
                    }
                    None => keep.push(c),
                },
                _ => keep.push(c),
            }
        }
        for r in script_results {
            if !resume.iter().any(|_| true) {
                unclaimed.push(r);
            }
        }
        rt.waits = keep;
        for (e, node, pin) in resume {
            if let Some(i) = rt.index_of(e) {
                rt.instances[i].debug.node_time.insert(node, now);
                rt.run_from(&cx, i, PinRef::new(node, pin));
            }
        }

        // 2. On Start.
        for i in 0..rt.instances.len() {
            if rt.instances[i].started {
                continue;
            }
            rt.instances[i].started = true;
            for node in rt.event_nodes(i, |k| matches!(k, NodeKind::OnStart)) {
                rt.fire(&cx, i, node);
            }
        }

        // 3. Custom events (emitted last frame), in order.
        let events: Vec<GraphEvent> = world.try_resource_mut::<GraphEvents>().map(|mut q| std::mem::take(&mut q.0)).unwrap_or_default();
        for ev in &events {
            for i in 0..rt.instances.len() {
                let nodes = rt.event_nodes(i, |k| matches!(k, NodeKind::OnCustomEvent { event, .. } if *event == ev.name));
                for node in nodes {
                    let sender = ev.sender.map_or(Value::Entity(None), entity_value);
                    rt.instances[i].outputs.insert(PinRef::new(node, "sender"), sender);
                    if let Some(p) = &ev.payload {
                        rt.instances[i].outputs.insert(PinRef::new(node, "payload"), from_bridge(p));
                    }
                    rt.fire(&cx, i, node);
                }
            }
        }

        // 4. Triggers.
        let collisions: Vec<kit_gamesys::CollisionEvent> =
            world.try_resource::<kit_gamesys::CollisionEvents>().map(|e| e.events.clone()).unwrap_or_default();
        for ev in collisions.iter().filter(|e| e.trigger) {
            for (me, other) in [(ev.a, ev.b), (ev.b, ev.a)] {
                let Some(i) = rt.index_of(me) else { continue };
                let enter = ev.started;
                let nodes = rt.event_nodes(i, |k| if enter { matches!(k, NodeKind::OnTriggerEnter) } else { matches!(k, NodeKind::OnTriggerExit) });
                for node in nodes {
                    rt.instances[i].outputs.insert(PinRef::new(node, "other"), entity_value(other));
                    rt.fire(&cx, i, node);
                }
            }
        }

        // 5. Interact.
        let pressed = world.try_resource::<kit_input::Input>().is_some_and(|i| i.pressed(INTERACT_KEY));
        if pressed {
            if let Some((target, interactor)) = interact_target(&rt, world, &gd) {
                if let Some(i) = rt.index_of(target) {
                    for node in rt.event_nodes(i, |k| matches!(k, NodeKind::OnInteract)) {
                        rt.instances[i].outputs.insert(PinRef::new(node, "interactor"), entity_value(interactor));
                        rt.fire(&cx, i, node);
                    }
                }
            }
        }

        // 6. Timers.
        for i in 0..rt.instances.len() {
            for node in rt.event_nodes(i, |k| matches!(k, NodeKind::OnTimer)) {
                let interval = rt.input(&cx, i, node, "interval", 0).map(|v| f(&v)).unwrap_or(1.0).max(0.01);
                let repeat = matches!(rt.input(&cx, i, node, "repeat", 0), Ok(Value::Bool(true)));
                let next = *rt.instances[i].timers.entry(node).or_insert(now + interval);
                if now >= next {
                    rt.instances[i].timers.insert(node, if repeat { next.max(now - interval) + interval } else { f64::INFINITY });
                    rt.fire(&cx, i, node);
                }
            }
        }
        // Results nobody waited for (the graph stopped): drop them.
        drop(unclaimed);
    }
    world.insert_resource(rt);
}

#[cfg(test)]
mod value_tests {
    use super::*;
    use kit_gamedata::ValueType;

    #[test]
    fn bridge_values_round_trip() {
        for v in [Value::Bool(true), Value::Int(-3), Value::Float(0.25), Value::Text("hi".into()), Value::Vector3([1.0, 2.0, 3.0]), Value::Color([1.0, 0.0, 0.0, 1.0]), Value::Entity(Some(42))] {
            assert_eq!(from_bridge(&to_bridge(&v).unwrap()), v);
        }
        assert!(to_bridge(&Value::Vector2([0.0; 2])).is_none());
    }

    #[test]
    fn script_type_value_defaults_match() {
        assert_eq!(ValueType::Float.default_value(), Value::Float(0.0));
    }
}
