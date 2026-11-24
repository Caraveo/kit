//! The Diagram component, and the narrow bridge between Diagram graphs and
//! x scripts. Neither side depends on the other: they meet in these queues.
//!
//! * x → graphs: a script calls `Diagram.Emit("Name", value)`, which pushes a
//!   [`GraphEvent`]; graphs' **On Custom Event** nodes receive it next frame.
//! * graphs → x: a **Run Script** node pushes a [`ScriptCall`]; the script
//!   module runs it on the target's script and posts the result (or error)
//!   back into [`ScriptCalls::results`].

use kit_ecs::Entity;
use serde::{Deserialize, Serialize};

/// Runs a Diagram graph (`.kdiagram` file) on this object. Any object can
/// have one, including an Empty for level-wide logic.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Diagram {
    /// Project-relative path to the graph.
    pub graph: String,
}

/// Values that can cross between graphs and scripts.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum BridgeValue {
    Bool(bool),
    Int(i64),
    Float(f64),
    Text(String),
    Vector3([f32; 3]),
    Color([f32; 4]),
    /// Entity bits (`None` = no object).
    Entity(Option<u64>),
}

/// A named event with an optional typed payload.
#[derive(Clone, Debug, PartialEq)]
pub struct GraphEvent {
    pub name: String,
    pub payload: Option<BridgeValue>,
    /// Who sent it (the script's or graph's object).
    pub sender: Option<Entity>,
}

/// Events waiting for graphs (delivered next frame, in order).
#[derive(Default, Debug)]
pub struct GraphEvents(pub Vec<GraphEvent>);

/// A graph asking a script to run one of its functions.
#[derive(Clone, Debug, PartialEq)]
pub struct ScriptCall {
    pub id: u64,
    pub target: Entity,
    pub function: String,
    pub args: Vec<BridgeValue>,
}

/// Script calls from graphs, and their results.
#[derive(Default, Debug)]
pub struct ScriptCalls {
    pub pending: Vec<ScriptCall>,
    /// (call id, result): the function's return value, or the error text.
    pub results: Vec<(u64, Result<Option<BridgeValue>, String>)>,
}
