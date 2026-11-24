//! Graph definitions (`.kdiagram` files): nodes, links and variables.

use crate::catalog::NodeKind;
use kit_gamedata::{Value, ValueType};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const EXTENSION: &str = "kdiagram";

pub type NodeId = u32;

/// One end of a link: a node's pin (by stable pin id).
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct PinRef {
    pub node: NodeId,
    pub pin: String,
}

impl PinRef {
    pub fn new(node: NodeId, pin: &str) -> Self {
        Self { node, pin: pin.to_owned() }
    }
}

/// A connection from an output pin to an input pin.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Link {
    pub from: PinRef,
    pub to: PinRef,
}

/// Which object an object node acts on when its Target pin isn't connected.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum TargetRef {
    /// The object the graph is attached to.
    #[default]
    SelfObject,
    /// An object picked in the scene, by its path in the hierarchy ("Room/Door").
    Picked(String),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Node {
    pub id: NodeId,
    pub kind: NodeKind,
    /// Canvas position (editor only).
    #[serde(default)]
    pub pos: [f32; 2],
    /// Values for unconnected input pins (pin id → value).
    #[serde(default)]
    pub inputs: BTreeMap<String, Value>,
    /// Target for object nodes when the Target pin isn't connected.
    #[serde(default)]
    pub target: TargetRef,
}

/// Where a variable lives.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Scope {
    /// One per object running the graph.
    #[default]
    Object,
    /// Shared by every graph in the scene (by name).
    Scene,
}

/// A declared variable with its authored default.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Variable {
    pub name: String,
    pub scope: Scope,
    pub default: Value,
}

impl Variable {
    pub fn value_type(&self) -> ValueType {
        self.default.value_type()
    }
}

/// A Diagram graph.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Graph {
    #[serde(default = "version")]
    pub version: u32,
    #[serde(default)]
    pub nodes: Vec<Node>,
    #[serde(default)]
    pub links: Vec<Link>,
    #[serde(default)]
    pub variables: Vec<Variable>,
    #[serde(default)]
    pub next_id: NodeId,
}

fn version() -> u32 {
    1
}

impl Graph {
    pub fn from_json(text: &str) -> anyhow::Result<Self> {
        Ok(serde_json::from_str(text)?)
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_default() + "\n"
    }

    pub fn node(&self, id: NodeId) -> Option<&Node> {
        self.nodes.iter().find(|n| n.id == id)
    }

    pub fn node_mut(&mut self, id: NodeId) -> Option<&mut Node> {
        self.nodes.iter_mut().find(|n| n.id == id)
    }

    /// Add a node; returns its id.
    pub fn add(&mut self, kind: NodeKind, pos: [f32; 2]) -> NodeId {
        self.next_id = self.next_id.max(self.nodes.iter().map(|n| n.id).max().unwrap_or(0)) + 1;
        let id = self.next_id;
        self.nodes.push(Node { id, kind, pos, inputs: BTreeMap::new(), target: TargetRef::SelfObject });
        id
    }

    /// Connect `from` (output) to `to` (input). An input takes one value
    /// link (replaced), an exec output one link (replaced); an exec input
    /// may have many.
    pub fn connect(&mut self, from: PinRef, to: PinRef, exec: bool) {
        if exec {
            self.links.retain(|l| l.from != from);
        } else {
            self.links.retain(|l| l.to != to);
        }
        self.links.push(Link { from, to });
    }

    pub fn disconnect_pin(&mut self, pin: &PinRef) {
        self.links.retain(|l| &l.from != pin && &l.to != pin);
    }

    pub fn remove_node(&mut self, id: NodeId) {
        self.nodes.retain(|n| n.id != id);
        self.links.retain(|l| l.from.node != id && l.to.node != id);
    }

    /// The link into an input pin.
    pub fn link_into(&self, to: &PinRef) -> Option<&Link> {
        self.links.iter().find(|l| &l.to == to)
    }

    /// Links out of an output pin (exec outputs: in link order).
    pub fn links_from<'a>(&'a self, from: &'a PinRef) -> impl Iterator<Item = &'a Link> + 'a {
        self.links.iter().filter(move |l| &l.from == from)
    }

    pub fn variable(&self, name: &str) -> Option<&Variable> {
        self.variables.iter().find(|v| v.name == name)
    }
}
