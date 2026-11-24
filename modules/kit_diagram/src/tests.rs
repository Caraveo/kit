//! Runtime tests: graphs run through a real App, with no x script anywhere.

use crate::catalog::{CompareOp, NodeKind, NumberType, Space};
use crate::graph::{Graph, PinRef, Scope, TargetRef, Variable};
use crate::validate::{Severity, validate};
use crate::{DiagramModule, DiagramRuntime};
use glam::Vec3;
use kit_core::App;
use kit_ecs::Entity;
use kit_gamedata::{GameData, GameDataModule, Value, ValueType};
use kit_gamesys::{Diagram, GameSysModule, Model, RigidBody};
use kit_scene::Transform;
use std::collections::HashMap;

struct Rig {
    app: App,
}

impl Rig {
    /// An app with an in-memory project holding `graphs` and `gd`.
    fn new(graphs: &[(&str, &Graph)], gd: &GameData) -> Self {
        Self::with_files(graphs, gd, &[])
    }

    fn with_files(graphs: &[(&str, &Graph)], gd: &GameData, extra: &[(&str, &str)]) -> Self {
        let mut files: HashMap<String, Vec<u8>> = HashMap::new();
        files.insert("kit.project".into(), b"[project]\ntitle = \"T\"\n".to_vec());
        files.insert(kit_gamedata::FILE.into(), serde_json::to_vec(gd).unwrap());
        for (path, g) in graphs {
            files.insert((*path).into(), g.to_json().into_bytes());
        }
        for (path, text) in extra {
            files.insert((*path).into(), text.as_bytes().to_vec());
        }
        let mut app = App::new();
        app.add_module(kit_resource::ResourceModule { root: "/game".into() })
            .add_module(kit_scene::SceneModule)
            .add_module(kit_input::InputModule)
            .add_module(GameSysModule)
            .add_module(kit_script::ScriptModule);
        app.world.insert_resource(kit_resource::Resources::in_memory(files));
        app.add_module(GameDataModule).add_module(DiagramModule);
        Self { app }
    }

    fn spawn(&mut self, name: &str, at: Vec3, graph: Option<&str>) -> Entity {
        let e = kit_scene::spawn_node(&mut self.app.world, name, Transform::from_xyz(at.x, at.y, at.z));
        if let Some(g) = graph {
            self.app.world.insert(e, Diagram { graph: g.into() });
        }
        e
    }

    fn frames(&mut self, n: usize) {
        for _ in 0..n {
            self.app.update(1.0 / 60.0);
        }
    }

    fn rt(&self) -> std::cell::Ref<'_, DiagramRuntime> {
        self.app.world.resource::<DiagramRuntime>()
    }
}

fn link(g: &mut Graph, from: (u32, &str), to: (u32, &str), exec: bool) {
    g.connect(PinRef::new(from.0, from.1), PinRef::new(to.0, to.1), exec);
}

/// GameData with "Room Presets" / "Low Gravity Workshop".
fn workshop_data() -> (GameData, String, String, String, String, String) {
    let mut gd = GameData::default();
    let ds = gd.add_dataset("Room Presets");
    let tint = gd.add_field(&ds, "Tint", ValueType::Color).unwrap();
    let grav = gd.add_field(&ds, "Gravity Scale", ValueType::Float).unwrap();
    let arrival = gd.add_field(&ds, "Arrival Point", ValueType::Vector3).unwrap();
    let row = gd.add_row(&ds, "Low Gravity Workshop").unwrap();
    gd.set_value(&ds, &row, &tint, Value::Color([0.2, 0.9, 0.6, 1.0]));
    gd.set_value(&ds, &row, &grav, Value::Float(0.25));
    gd.set_value(&ds, &row, &arrival, Value::Vector3([4.0, 1.0, 0.0]));
    (gd, ds, row, tint, grav, arrival)
}

#[test]
fn on_start_actions_target_self_and_read_datasets() {
    let (gd, ds, row, tint, grav, _) = workshop_data();
    let mut g = Graph::default();
    let start = g.add(NodeKind::OnStart, [0.0; 2]);
    let set_tint = g.add(NodeKind::SetTint, [0.0; 2]);
    let set_grav = g.add(NodeKind::SetGravityScale, [0.0; 2]);
    let data = g.add(NodeKind::DatasetRow { dataset: ds.clone(), row: Some(row.clone()) }, [0.0; 2]);
    link(&mut g, (start, "then"), (set_tint, "exec"), true);
    link(&mut g, (set_tint, "then"), (set_grav, "exec"), true);
    link(&mut g, (data, &tint), (set_tint, "color"), false);
    link(&mut g, (data, &grav), (set_grav, "value"), false);
    assert!(validate(&g, &gd).iter().all(|d| d.severity != Severity::Error), "{:?}", validate(&g, &gd));

    let mut rig = Rig::new(&[("logic.kdiagram", &g)], &gd);
    let e = rig.spawn("Box", Vec3::ZERO, Some("logic.kdiagram"));
    rig.app.world.insert(e, Model::default());
    rig.app.world.insert(e, RigidBody::default());
    rig.frames(1);
    assert_eq!(rig.app.world.get::<Model>(e).unwrap().color.to_array(), [0.2, 0.9, 0.6, 1.0]);
    assert_eq!(rig.app.world.get::<RigidBody>(e).unwrap().gravity_scale, 0.25);
    // Debug: both actions ran, and the value is on the pin.
    let rt = rig.rt();
    let inst = rt.instance(e).unwrap();
    assert!(inst.debug.node_time.contains_key(&set_tint) && inst.debug.node_time.contains_key(&set_grav));
    assert_eq!(inst.debug.values.get(&PinRef::new(set_grav, "value")), Some(&Value::Float(0.25)));
}

#[test]
fn renames_keep_wiring_and_deleted_fields_are_flagged_on_the_pin() {
    let (mut gd, ds, row, tint, _, _) = workshop_data();
    let mut g = Graph::default();
    let data = g.add(NodeKind::DatasetRow { dataset: ds.clone(), row: Some(row.clone()) }, [0.0; 2]);
    let set = g.add(NodeKind::SetTint, [0.0; 2]);
    let start = g.add(NodeKind::OnStart, [0.0; 2]);
    link(&mut g, (start, "then"), (set, "exec"), true);
    link(&mut g, (data, &tint), (set, "color"), false);
    // Rename the dataset, row and field: still valid, still titled right.
    {
        let d = gd.dataset_mut(&ds).unwrap();
        d.name = "Rooms".into();
        d.rows[0].name = "Workshop".into();
        d.fields[0].name = "Color".into();
    }
    assert!(!crate::validate::has_errors(&validate(&g, &gd)));
    assert_eq!(g.node(data).unwrap().kind.title(&gd), "Rooms");
    // Delete the field: the exact pin is reported.
    gd.dataset_mut(&ds).unwrap().fields.retain(|f| f.id != tint);
    let diags = validate(&g, &gd);
    let d = diags.iter().find(|d| d.severity == Severity::Error).expect("an error");
    assert_eq!((d.node, d.pin.as_deref()), (Some(data), Some(tint.as_str())));
    assert!(d.message.contains("field was deleted"), "{}", d.message);
    // Type changes are type errors on the input pin.
    let (mut gd2, ds2, _, tint2, _, _) = workshop_data();
    let mut g2 = g.clone();
    if let NodeKind::DatasetRow { dataset, .. } = &mut g2.node_mut(data).unwrap().kind {
        *dataset = ds2.clone();
    }
    g2.links.iter_mut().for_each(|l| {
        if l.from.node == data {
            l.from.pin = tint2.clone()
        }
    });
    gd2.dataset_mut(&ds2).unwrap().fields[0].value_type = ValueType::Float;
    let diags = validate(&g2, &gd2);
    assert!(
        diags.iter().any(|d| d.node == Some(set) && d.pin.as_deref() == Some("color") && d.message.contains("Float")),
        "{diags:?}"
    );
}

#[test]
fn delays_timers_and_move_over_time_resume_without_blocking() {
    let gd = GameData::default();
    let mut g = Graph::default();
    g.variables.push(Variable { name: "Ticks".into(), scope: Scope::Object, default: Value::Int(0) });
    g.variables.push(Variable { name: "Arrived".into(), scope: Scope::Scene, default: Value::Bool(false) });
    let start = g.add(NodeKind::OnStart, [0.0; 2]);
    let delay = g.add(NodeKind::Delay, [0.0; 2]);
    let hide = g.add(NodeKind::SetVisible, [0.0; 2]);
    let mv = g.add(NodeKind::MoveOverTime, [0.0; 2]);
    let arrived = g.add(NodeKind::SetVariable { name: "Arrived".into() }, [0.0; 2]);
    g.node_mut(delay).unwrap().inputs.insert("seconds".into(), Value::Float(0.5));
    g.node_mut(hide).unwrap().inputs.insert("visible".into(), Value::Bool(false));
    g.node_mut(mv).unwrap().inputs.insert("destination".into(), Value::Vector3([3.0, 0.0, 0.0]));
    g.node_mut(mv).unwrap().inputs.insert("duration".into(), Value::Float(0.5));
    g.node_mut(arrived).unwrap().inputs.insert("value".into(), Value::Bool(true));
    link(&mut g, (start, "then"), (delay, "exec"), true);
    link(&mut g, (delay, "then"), (hide, "exec"), true);
    link(&mut g, (hide, "then"), (mv, "exec"), true);
    link(&mut g, (mv, "completed"), (arrived, "exec"), true);
    // Timer: every 0.25 s, Ticks += 1.
    let timer = g.add(NodeKind::OnTimer, [0.0; 2]);
    g.node_mut(timer).unwrap().inputs.insert("interval".into(), Value::Float(0.25));
    let get = g.add(NodeKind::GetVariable { name: "Ticks".into() }, [0.0; 2]);
    let add = g.add(NodeKind::Math { op: crate::catalog::MathOp::Add, number: NumberType::Int }, [0.0; 2]);
    let set = g.add(NodeKind::SetVariable { name: "Ticks".into() }, [0.0; 2]);
    g.node_mut(add).unwrap().inputs.insert("b".into(), Value::Int(1));
    link(&mut g, (get, "value"), (add, "a"), false);
    link(&mut g, (add, "result"), (set, "value"), false);
    link(&mut g, (timer, "then"), (set, "exec"), true);
    assert!(!crate::validate::has_errors(&validate(&g, &gd)), "{:?}", validate(&g, &gd));

    let mut rig = Rig::new(&[("g.kdiagram", &g)], &gd);
    let e = rig.spawn("Mover", Vec3::ZERO, Some("g.kdiagram"));
    rig.app.world.insert(e, Model::default());
    rig.frames(10); // 1/6 s: the delay is still waiting
    assert!(rig.app.world.get::<Model>(e).unwrap().visible, "Delay doesn't run its action early");
    assert_eq!(rig.rt().pending(), 1);
    rig.frames(30); // past 0.5 s: hidden, moving
    assert!(!rig.app.world.get::<Model>(e).unwrap().visible);
    let x = rig.app.world.get::<Transform>(e).unwrap().translation.x;
    assert!(x > 0.0 && x < 3.0, "moving: x = {x}");
    rig.frames(40);
    assert!((rig.app.world.get::<Transform>(e).unwrap().translation.x - 3.0).abs() < 1e-4, "arrived");
    assert_eq!(rig.rt().scene_vars.get("Arrived"), Some(&Value::Bool(true)), "Completed fired");
    // Timer ran ~4 times per second for ~1.33 s.
    let ticks = rig.rt().instance(e).unwrap().vars_snapshot().get("Ticks").cloned();
    assert!(matches!(ticks, Some(Value::Int(n)) if (4..=6).contains(&n)), "{ticks:?}");
    // Stopping clears everything waiting.
    crate::reset(&mut rig.app.world);
    assert_eq!(rig.rt().pending(), 0);
    assert!(rig.rt().instances.is_empty());
}

#[test]
fn branches_compare_and_custom_events_from_anywhere() {
    let gd = GameData::default();
    let mut g = Graph::default();
    g.variables.push(Variable { name: "Score".into(), scope: Scope::Scene, default: Value::Float(0.0) });
    let on = g.add(NodeKind::OnCustomEvent { event: "Scored".into(), payload: Some(ValueType::Float) }, [0.0; 2]);
    let cmp = g.add(NodeKind::Compare { op: CompareOp::Greater, number: NumberType::Float }, [0.0; 2]);
    let br = g.add(NodeKind::Branch, [0.0; 2]);
    let set = g.add(NodeKind::SetVariable { name: "Score".into() }, [0.0; 2]);
    g.node_mut(cmp).unwrap().inputs.insert("b".into(), Value::Float(10.0));
    link(&mut g, (on, "payload"), (cmp, "a"), false);
    link(&mut g, (cmp, "result"), (br, "condition"), false);
    link(&mut g, (on, "then"), (br, "exec"), true);
    link(&mut g, (br, "true"), (set, "exec"), true);
    link(&mut g, (on, "payload"), (set, "value"), false);
    let mut rig = Rig::new(&[("g.kdiagram", &g)], &gd);
    rig.spawn("Level", Vec3::ZERO, Some("g.kdiagram"));
    rig.frames(1);
    for v in [5.0, 42.0] {
        rig.app.world.resource_mut::<kit_gamesys::GraphEvents>().0.push(kit_gamesys::GraphEvent {
            name: "Scored".into(),
            payload: Some(kit_gamesys::BridgeValue::Float(v)),
            sender: None,
        });
        rig.frames(1);
    }
    assert_eq!(rig.rt().scene_vars.get("Score"), Some(&Value::Float(42.0)), "only the > 10 event passed the branch");
}

#[test]
fn missing_components_name_the_node_and_object_and_loops_hit_the_budget() {
    let gd = GameData::default();
    let mut g = Graph::default();
    let start = g.add(NodeKind::OnStart, [0.0; 2]);
    let grav = g.add(NodeKind::SetGravityScale, [0.0; 2]);
    link(&mut g, (start, "then"), (grav, "exec"), true);
    // A runaway loop: Sequence feeding itself.
    let loop_start = g.add(NodeKind::OnTimer, [0.0; 2]);
    g.node_mut(loop_start).unwrap().inputs.insert("interval".into(), Value::Float(0.01));
    let seq = g.add(NodeKind::Sequence { outputs: 2 }, [0.0; 2]);
    link(&mut g, (loop_start, "then"), (seq, "exec"), true);
    link(&mut g, (seq, "then0"), (seq, "exec"), true);
    let mut rig = Rig::new(&[("g.kdiagram", &g)], &gd);
    let e = rig.spawn("Crate", Vec3::ZERO, Some("g.kdiagram"));
    rig.frames(1);
    let console: Vec<String> =
        rig.app.world.resource::<kit_core::Console>().lines().iter().map(|l| l.text.clone()).collect();
    assert!(
        console.iter().any(|t| t.contains("Set Gravity Scale") && t.contains("\"Crate\"") && t.contains("Rigid Body")),
        "{console:?}"
    );
    rig.frames(3);
    let rt = rig.rt();
    let (node, why) = rt.instance(e).unwrap().debug.error.clone().unwrap();
    assert_eq!(node, seq, "the looping node is flagged");
    assert!(why.contains("nodes ran this frame"), "{why}");
}

#[test]
fn picked_targets_and_parented_world_positions() {
    let gd = GameData::default();
    let mut g = Graph::default();
    let start = g.add(NodeKind::OnStart, [0.0; 2]);
    let set = g.add(NodeKind::SetPosition { space: Space::World }, [0.0; 2]);
    g.node_mut(set).unwrap().target = TargetRef::Picked("Room/Door".into());
    g.node_mut(set).unwrap().inputs.insert("position".into(), Value::Vector3([10.0, 0.0, 0.0]));
    link(&mut g, (start, "then"), (set, "exec"), true);
    let mut rig = Rig::new(&[("g.kdiagram", &g)], &gd);
    let room = rig.spawn("Room", Vec3::new(4.0, 0.0, 0.0), None);
    let door = rig.spawn("Door", Vec3::ZERO, None);
    kit_scene::set_parent(&mut rig.app.world, door, room);
    rig.spawn("Level", Vec3::ZERO, Some("g.kdiagram"));
    rig.frames(2);
    // World (10,0,0) under a parent at x = 4 → local x = 6.
    assert!((rig.app.world.get::<Transform>(door).unwrap().translation.x - 6.0).abs() < 1e-4);
}

#[test]
fn graphs_read_public_fields_and_write_edit_fields_only() {
    let script = r#"
CREATE Pump[
    EDIT FLOAT Power: 1.0
    PUBLIC INTEGER Level: 7
    FLOAT Secret: 2.0
]
"#;
    let gd = GameData::default();
    let mut g = Graph::default();
    let start = g.add(NodeKind::OnStart, [0.0; 2]);
    let get = g.add(NodeKind::GetScriptField { field: "Level".into(), value_type: ValueType::Int }, [0.0; 2]);
    let conv = g.add(NodeKind::Convert { conversion: crate::catalog::Conversion::IntToFloat }, [0.0; 2]);
    let set = g.add(NodeKind::SetScriptField { field: "Power".into(), value_type: ValueType::Float }, [0.0; 2]);
    let bad = g.add(NodeKind::SetScriptField { field: "Level".into(), value_type: ValueType::Int }, [0.0; 2]);
    link(&mut g, (get, "value"), (conv, "value"), false);
    link(&mut g, (conv, "result"), (set, "value"), false);
    link(&mut g, (start, "then"), (set, "exec"), true);
    link(&mut g, (set, "then"), (bad, "exec"), true);
    assert!(!crate::validate::has_errors(&validate(&g, &gd)), "{:?}", validate(&g, &gd));

    let mut rig = Rig::with_files(&[("g.kdiagram", &g)], &gd, &[("pump.x", script)]);
    let e = rig.spawn("Pump", Vec3::ZERO, Some("g.kdiagram"));
    rig.app.world.insert(e, kit_gamesys::Script::new("pump.x"));
    rig.frames(2);
    let fields = rig.app.world.resource::<kit_script::ScriptRuntime>().fields(e);
    let power = fields.iter().find(|f| f.name == "Power").unwrap();
    assert_eq!(power.value, Some(kit_gamesys::FieldValue::Float(7.0)), "EDIT field set from the PUBLIC one");
    let (node, why) = rig.rt().instance(e).unwrap().debug.error.clone().unwrap();
    assert_eq!(node, bad);
    assert!(why.contains("PUBLIC (read-only)"), "{why}");
    // The picker offers Get for PUBLIC and EDIT, Set only for EDIT, nothing for private.
    let titles: Vec<String> = NodeKind::script_field_nodes(&fields).iter().map(|k| k.title(&gd)).collect();
    assert_eq!(titles, ["Get Power", "Set Power", "Get Level"]);
}
