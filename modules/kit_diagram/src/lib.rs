//! # kit_diagram
//!
//! Diagram: visual gameplay graphs (Blueprint-style) that run directly on
//! Kit's ECS and engine APIs: no x code is generated and none is required.
//!
//! * [`graph`]: graph definitions (`.kdiagram` files) with typed nodes,
//!   links, inline values and variables.
//! * [`catalog`]: every node kind and its typed pins.
//! * [`validate`]: checks before running, with diagnostics per node and pin.
//! * [`exec`]: the runtime: events, scheduling (Delay, Move Over Time,
//!   timers), an execution budget, and debug state for the editor.
//!
//! Graphs talk to x scripts only through `kit_gamesys::bridge`.

pub mod catalog;
pub mod exec;
pub mod graph;
#[cfg(test)]
mod tests;
pub mod validate;

pub use catalog::{NodeKind, PinDef, PinType, Signature, Space};
pub use exec::{BUDGET, DebugInfo, DiagramRuntime, reset, run_diagrams};
pub use graph::{Graph, Link, Node, NodeId, PinRef, Scope, TargetRef, Variable};
pub use validate::{Diagnostic, Severity, validate};

use kit_core::{App, Module, Stage};

/// Runs every object's Diagram graph during Play.
pub struct DiagramModule;

impl Module for DiagramModule {
    fn name(&self) -> &'static str {
        "diagram"
    }
    fn dependencies(&self) -> &'static [&'static str] {
        &["gamesys", "gamedata"]
    }
    fn build(&self, app: &mut App) {
        app.world.insert_resource(DiagramRuntime::default());
        app.add_play_system(Stage::Update, "run_diagrams", run_diagrams);
    }
}
