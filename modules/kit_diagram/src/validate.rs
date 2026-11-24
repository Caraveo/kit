//! Checking a graph before it runs: pin types, references into GameData and
//! variables, and cycles among value-only nodes. Problems name the exact
//! node and pin, so the editor can flag them.

use crate::catalog::{NodeKind, PinType, Signature};
use crate::graph::{Graph, NodeId};
use kit_gamedata::GameData;
use std::collections::{HashMap, HashSet};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    /// The graph won't run until it's fixed.
    Error,
    /// It runs, but probably not as intended.
    Warning,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Diagnostic {
    pub severity: Severity,
    pub node: Option<NodeId>,
    pub pin: Option<String>,
    pub message: String,
}

impl Diagnostic {
    fn error(node: NodeId, pin: Option<&str>, message: impl Into<String>) -> Self {
        Self { severity: Severity::Error, node: Some(node), pin: pin.map(str::to_owned), message: message.into() }
    }
    fn warning(node: NodeId, pin: Option<&str>, message: impl Into<String>) -> Self {
        Self { severity: Severity::Warning, node: Some(node), pin: pin.map(str::to_owned), message: message.into() }
    }
}

/// Any errors?
pub fn has_errors(diagnostics: &[Diagnostic]) -> bool {
    diagnostics.iter().any(|d| d.severity == Severity::Error)
}

pub fn validate(graph: &Graph, gd: &GameData) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    let sigs: HashMap<NodeId, Signature> = graph.nodes.iter().map(|n| (n.id, n.kind.signature_in(gd, graph))).collect();
    let title = |id: NodeId| graph.node(id).map_or("?".to_owned(), |n| n.kind.title(gd));

    // References into GameData and variables.
    for n in &graph.nodes {
        match &n.kind {
            NodeKind::DatasetRow { dataset, row } => match gd.dataset(dataset) {
                None => out.push(Diagnostic::error(
                    n.id,
                    None,
                    "This dataset was deleted. Pick another or remove the node.",
                )),
                Some(ds) => {
                    if let Some(r) = row {
                        if ds.row(r).is_none() && graph.link_into(&crate::graph::PinRef::new(n.id, "row")).is_none() {
                            out.push(Diagnostic::error(
                                n.id,
                                Some("row"),
                                format!("The row picked here was deleted from {}.", ds.name),
                            ));
                        }
                    } else if graph.link_into(&crate::graph::PinRef::new(n.id, "row")).is_none() {
                        out.push(Diagnostic::error(
                            n.id,
                            Some("row"),
                            format!("Pick a row of {} or connect a Row input.", ds.name),
                        ));
                    }
                }
            },
            NodeKind::Constant { constant } if gd.constant(constant).is_none() => {
                out.push(Diagnostic::error(n.id, None, "This constant was deleted."));
            }
            NodeKind::GetVariable { name } | NodeKind::SetVariable { name } if graph.variable(name).is_none() => {
                out.push(Diagnostic::error(
                    n.id,
                    None,
                    format!("No variable named \"{name}\". Add it in the graph's Variables."),
                ));
            }
            NodeKind::RunScript { function, .. } if function.trim().is_empty() => {
                out.push(Diagnostic::error(n.id, None, "Name the script function to run."));
            }
            NodeKind::OnCustomEvent { event, .. } | NodeKind::EmitEvent { event, .. } if event.trim().is_empty() => {
                out.push(Diagnostic::error(n.id, None, "Name the event."));
            }
            _ => {}
        }
        // Inline values must match their pin's type.
        if let Some(sig) = sigs.get(&n.id) {
            for (pin, value) in &n.inputs {
                if let Some(PinType::Data(t)) = sig.input(pin).map(|p| &p.ty) {
                    if value.value_type() != *t {
                        out.push(Diagnostic::warning(
                            n.id,
                            Some(pin),
                            format!(
                                "Value is {} but {} expects {}; using a default.",
                                value.value_type().label(),
                                pin,
                                t.label()
                            ),
                        ));
                    }
                }
            }
        }
    }

    // Links: both ends exist, and types match.
    let mut inputs_seen: HashSet<(NodeId, String)> = HashSet::new();
    for l in &graph.links {
        let (Some(fs), Some(ts)) = (sigs.get(&l.from.node), sigs.get(&l.to.node)) else {
            out.push(Diagnostic::error(
                l.from.node.min(l.to.node),
                None,
                "A wire points to a node that no longer exists.",
            ));
            continue;
        };
        let Some(from) = fs.output(&l.from.pin) else {
            let what = match graph.node(l.from.node).map(|n| &n.kind) {
                Some(NodeKind::DatasetRow { .. }) => "That field was deleted from the dataset",
                _ => "This output no longer exists",
            };
            out.push(Diagnostic::error(
                l.from.node,
                Some(&l.from.pin),
                format!("{what}; {} is no longer fed.", title(l.to.node)),
            ));
            continue;
        };
        let Some(to) = ts.input(&l.to.pin) else {
            out.push(Diagnostic::error(l.to.node, Some(&l.to.pin), "This input no longer exists."));
            continue;
        };
        match (&from.ty, &to.ty) {
            (PinType::Exec, PinType::Exec) => {}
            (PinType::Data(a), PinType::Data(b)) if a == b => {}
            (PinType::Data(a), PinType::Data(b)) => out.push(Diagnostic::error(
                l.to.node,
                Some(&l.to.pin),
                format!(
                    "{} gives {} but {} needs {}. Add a conversion node.",
                    from.name,
                    a.label(),
                    to.name,
                    b.label()
                ),
            )),
            _ => {
                out.push(Diagnostic::error(l.to.node, Some(&l.to.pin), "Execution and value pins can't be connected."))
            }
        }
        if matches!(to.ty, PinType::Data(_)) && !inputs_seen.insert((l.to.node, l.to.pin.clone())) {
            out.push(Diagnostic::error(l.to.node, Some(&l.to.pin), "An input can only take one wire."));
        }
    }

    // Cycles among value-only (pure) nodes can't be evaluated.
    let pure: HashSet<NodeId> = graph.nodes.iter().filter(|n| n.kind.is_pure(gd)).map(|n| n.id).collect();
    let deps: HashMap<NodeId, Vec<NodeId>> = graph
        .links
        .iter()
        .filter(|l| pure.contains(&l.to.node) && pure.contains(&l.from.node))
        .fold(HashMap::new(), |mut m, l| {
            m.entry(l.to.node).or_default().push(l.from.node);
            m
        });
    let mut state: HashMap<NodeId, u8> = HashMap::new(); // 1 visiting, 2 done
    fn visit(n: NodeId, deps: &HashMap<NodeId, Vec<NodeId>>, state: &mut HashMap<NodeId, u8>) -> Option<NodeId> {
        match state.get(&n) {
            Some(1) => return Some(n),
            Some(2) => return None,
            _ => {}
        }
        state.insert(n, 1);
        for d in deps.get(&n).into_iter().flatten() {
            if let Some(c) = visit(*d, deps, state) {
                return Some(c);
            }
        }
        state.insert(n, 2);
        None
    }
    let mut ids: Vec<NodeId> = pure.iter().copied().collect();
    ids.sort();
    for id in ids {
        if let Some(c) = visit(id, &deps, &mut state) {
            out.push(Diagnostic::error(c, None, "These value nodes feed into each other in a loop; break the cycle."));
            break;
        }
    }

    // Actions nobody triggers.
    for n in &graph.nodes {
        let sig = &sigs[&n.id];
        if sig.input("exec").is_some() && !graph.links.iter().any(|l| l.to.node == n.id && l.to.pin == "exec") {
            out.push(Diagnostic::warning(
                n.id,
                Some("exec"),
                "Nothing triggers this node: connect an event's white wire.",
            ));
        }
    }
    out
}
