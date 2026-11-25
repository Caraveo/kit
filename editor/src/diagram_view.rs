//! The Diagram view (key 7): a node editor for the selected object's
//! Diagram graph. Edits save to the `.kdiagram` file right away; undo and
//! redo work while the mouse is over the view.

use crate::state::Editor;
use crate::theme::*;
use egui::{Align2, Color32, FontId, Pos2, Rect, RichText, Sense, Stroke, Ui, Vec2, pos2, vec2};
use kit::diagram::catalog::{BoolOp, CompareOp, Conversion, MathOp, NumberType, PinDef, PinType, Space};
use kit::diagram::{Graph, NodeId, NodeKind, PinRef, Scope, Severity, TargetRef, Variable, validate};
use kit::ecs::Entity;
use kit::gamedata::{GameData, Value, ValueType};
use std::collections::{BTreeSet, HashMap};

const NODE_W: f32 = 230.0;
const HEADER_H: f32 = 26.0;
const ROW_H: f32 = 24.0;
const PIN_R: f32 = 5.5;
/// Light edge around black execution wires and pins (dark mode).
const EDGE: Color32 = Color32::from_rgb(120, 120, 140);

/// A wire being dragged from a pin.
#[derive(Clone)]
struct Wire {
    pin: PinRef,
    /// Dragged from an output (true) or an input.
    from_output: bool,
    ty: PinType,
}

struct Picker {
    /// Where the node goes (scene coordinates).
    at: Pos2,
    /// Screen position of the menu.
    screen: Pos2,
    search: String,
    /// Connect the new node to this wire.
    wire: Option<Wire>,
    focus: bool,
}

#[derive(Default)]
pub struct DiagramView {
    path: Option<String>,
    pub graph: Graph,
    undo: Vec<Graph>,
    redo: Vec<Graph>,
    scene_rect: Option<Rect>,
    selected: BTreeSet<NodeId>,
    /// A selected wire (Del / Backspace removes it).
    selected_wire: Option<kit::diagram::Link>,
    wire: Option<Wire>,
    picker: Option<Picker>,
    error: Option<String>,
    /// The mouse is over the view (for undo / delete routing).
    pub hovered: bool,
    /// A node title is being dragged (one undo step per drag).
    dragging: bool,
    new_var: String,
    /// Scene → screen transform of the canvas (last frame).
    to_global: Option<egui::emath::TSTransform>,
    /// A Diagram opened from the asset list (shown until the selection changes).
    pinned: Option<String>,
    pinned_selection: Option<Entity>,
}

/// One row of a node: an input on the left and/or an output on the right.
struct Row {
    input: Option<PinDef>,
    output: Option<PinDef>,
}

fn rows(sig: &kit::diagram::Signature) -> Vec<Row> {
    let mut rows = Vec::new();
    let exec_in = sig.inputs.iter().find(|p| p.ty == PinType::Exec).cloned();
    let mut outs: Vec<PinDef> = sig.outputs.clone();
    let first_exec_out = outs.iter().position(|p| p.ty == PinType::Exec).map(|i| outs.remove(i));
    if exec_in.is_some() || first_exec_out.is_some() {
        rows.push(Row { input: exec_in, output: first_exec_out });
    }
    for p in sig.inputs.iter().filter(|p| p.ty != PinType::Exec) {
        rows.push(Row { input: Some(p.clone()), output: None });
    }
    for p in outs {
        rows.push(Row { input: None, output: Some(p) });
    }
    rows
}

fn node_height(n_rows: usize) -> f32 {
    HEADER_H + ROW_H * n_rows as f32 + 8.0
}

fn type_color(ty: &PinType) -> Color32 {
    match ty {
        // Execution wires: black in dark mode (with a light edge so they
        // read on the dark canvas), near-black in light mode.
        PinType::Exec => {
            if crate::theme::is_light() {
                Color32::from_rgb(40, 40, 52)
            } else {
                Color32::BLACK
            }
        }
        PinType::Data(t) => match t {
            ValueType::Bool => Color32::from_rgb(230, 90, 90),
            ValueType::Int => Color32::from_rgb(90, 210, 170),
            ValueType::Float => Color32::from_rgb(140, 220, 90),
            ValueType::Text => Color32::from_rgb(240, 120, 220),
            ValueType::Color => Color32::from_rgb(250, 190, 80),
            ValueType::Vector2 | ValueType::Vector3 => Color32::from_rgb(250, 210, 60),
            ValueType::Transform => Color32::from_rgb(240, 140, 60),
            ValueType::Entity => Color32::from_rgb(90, 160, 250),
            ValueType::Row(_) => Color32::from_rgb(170, 140, 250),
        },
    }
}

fn category_color(cat: &str) -> Color32 {
    match cat {
        "EVENTS" => Color32::from_rgb(170, 50, 60),
        "OBJECTS" => Color32::from_rgb(50, 100, 170),
        "APPEARANCE & PHYSICS" => Color32::from_rgb(40, 130, 120),
        "DATASETS" => Color32::from_rgb(120, 80, 190),
        "VARIABLES" => Color32::from_rgb(70, 130, 60),
        "SCRIPT FIELDS" => Color32::from_rgb(150, 110, 40),
        "INTEGRATION" => Color32::from_rgb(140, 70, 130),
        _ => Color32::from_rgb(80, 80, 100),
    }
}

/// Where a pin sits (scene coordinates).
fn pin_pos(node_pos: [f32; 2], row: usize, output: bool) -> Pos2 {
    pos2(
        node_pos[0] + if output { NODE_W } else { 0.0 },
        node_pos[1] + HEADER_H + ROW_H * row as f32 + ROW_H / 2.0 + 2.0,
    )
}

fn compatible(a: &PinType, b: &PinType) -> bool {
    a == b
}

impl DiagramView {
    /// Change the graph (one undo step), then save it.
    fn change(&mut self, ed: &mut Editor, f: impl FnOnce(&mut Graph)) {
        let before = self.graph.clone();
        f(&mut self.graph);
        if self.graph != before {
            self.undo.push(before);
            self.redo.clear();
            self.save(ed);
        }
    }

    fn save(&mut self, ed: &mut Editor) {
        let (Some(path), Some(project)) = (&self.path, &ed.project) else { return };
        if let Err(e) = project.resources.write_text(path, &self.graph.to_json()) {
            self.error = Some(format!("Couldn't save {path}: {e:#}"));
        }
    }

    pub fn undo(&mut self, ed: &mut Editor) {
        if let Some(g) = self.undo.pop() {
            self.redo.push(std::mem::replace(&mut self.graph, g));
            self.save(ed);
        }
    }

    pub fn redo(&mut self, ed: &mut Editor) {
        if let Some(g) = self.redo.pop() {
            self.undo.push(std::mem::replace(&mut self.graph, g));
            self.save(ed);
        }
    }

    /// Delete the selected nodes, or the selected wire.
    pub fn delete_selected(&mut self, ed: &mut Editor) {
        if let Some(link) = self.selected_wire.take() {
            self.change(ed, |g| g.links.retain(|l| *l != link));
            return;
        }
        let sel: Vec<NodeId> = std::mem::take(&mut self.selected).into_iter().collect();
        self.change(ed, |g| sel.iter().for_each(|n| g.remove_node(*n)));
    }

    pub fn duplicate_selected(&mut self, ed: &mut Editor) {
        let sel: Vec<NodeId> = self.selected.iter().copied().collect();
        let mut new = BTreeSet::new();
        self.change(ed, |g| {
            let mut map = HashMap::new();
            for id in &sel {
                let Some(n) = g.node(*id).cloned() else { continue };
                let copy = g.add(n.kind.clone(), [n.pos[0] + 30.0, n.pos[1] + 30.0]);
                let c = g.node_mut(copy).unwrap();
                c.inputs = n.inputs.clone();
                c.target = n.target.clone();
                map.insert(*id, copy);
                new.insert(copy);
            }
            // Keep wires between duplicated nodes.
            let links: Vec<_> = g
                .links
                .iter()
                .filter(|l| map.contains_key(&l.from.node) && map.contains_key(&l.to.node))
                .cloned()
                .collect();
            for l in links {
                g.links.push(kit::diagram::Link {
                    from: PinRef { node: map[&l.from.node], pin: l.from.pin },
                    to: PinRef { node: map[&l.to.node], pin: l.to.pin },
                });
            }
        });
        self.selected = new;
    }

    /// Load the graph at `path` (when it isn't the one open).
    fn load(&mut self, ed: &Editor, path: &str) {
        if self.path.as_deref() == Some(path) {
            return;
        }
        *self = DiagramView { path: Some(path.to_owned()), new_var: String::new(), ..Default::default() };
        let Some(project) = &ed.project else { return };
        match project.resources.read_text(path) {
            Ok(text) => match Graph::from_json(&text) {
                Ok(g) => self.graph = g,
                Err(e) => self.error = Some(format!("{path} can't be read: {e:#}")),
            },
            Err(_) => self.graph = Graph::default(),
        }
    }

    #[cfg(test)]
    pub fn path(&self) -> Option<&str> {
        self.path.as_deref()
    }

    /// Where a pin is on screen (last frame).
    #[cfg(test)]
    pub fn pin_screen_pos(&self, gd: &GameData, pin: &PinRef, output: bool) -> Option<Pos2> {
        let n = self.graph.node(pin.node)?;
        let rows = rows(&n.kind.signature_in(gd, &self.graph));
        let i = rows
            .iter()
            .position(|r| (if output { &r.output } else { &r.input }).as_ref().is_some_and(|p| p.id == pin.pin))?;
        Some(self.to_global? * pin_pos(n.pos, i, output))
    }

    /// The middle of a wire on screen (last frame).
    #[cfg(test)]
    pub fn wire_screen_mid(&self, gd: &GameData, link: &kit::diagram::Link) -> Option<Pos2> {
        let a = self.pin_screen_pos(gd, &link.from, true)?;
        let b = self.pin_screen_pos(gd, &link.to, false)?;
        let to_scene = self.to_global?.inverse();
        let [p0, p1, p2, p3] = wire_points(to_scene * a, to_scene * b);
        let mid = ((p0.to_vec2() + p1.to_vec2() * 3.0 + p2.to_vec2() * 3.0 + p3.to_vec2()) / 8.0).to_pos2();
        Some(self.to_global? * mid)
    }

    #[cfg(test)]
    pub fn selected_wire(&self) -> Option<&kit::diagram::Link> {
        self.selected_wire.as_ref()
    }

    /// A node's title bar on screen (last frame).
    #[cfg(test)]
    pub fn header_screen_pos(&self, id: NodeId) -> Option<Pos2> {
        let n = self.graph.node(id)?;
        Some(self.to_global? * pos2(n.pos[0] + NODE_W / 2.0, n.pos[1] + HEADER_H / 2.0))
    }
}

/// Create `diagrams/<name>.kdiagram` with an On Start node and attach it.
pub fn add_diagram(ed: &mut Editor, e: Entity) -> Option<String> {
    let project = ed.project.as_ref()?;
    let base: String =
        ed.entity_label(e).chars().map(|c| if c.is_alphanumeric() { c.to_ascii_lowercase() } else { '_' }).collect();
    let base = if base.trim_matches('_').is_empty() { "logic".to_owned() } else { base };
    let mut path = format!("diagrams/{base}.kdiagram");
    for n in 2.. {
        if !project.resources.exists(&path) {
            break;
        }
        path = format!("diagrams/{base}_{n}.kdiagram");
    }
    let mut g = Graph::default();
    g.add(NodeKind::OnStart, [0.0, 0.0]);
    if let Err(err) = project.resources.write_text(&path, &g.to_json()) {
        ed.report(Err(err));
        return None;
    }
    ed.app.world.insert(e, kit::gamesys::Diagram { graph: path.clone() });
    ed.refresh_assets(true);
    Some(path)
}

fn value_editor(ui: &mut Ui, v: &mut Value) -> bool {
    match v {
        Value::Bool(b) => ui.checkbox(b, "").changed(),
        Value::Int(i) => ui.add(egui::DragValue::new(i).speed(0.1)).changed(),
        Value::Float(f) => ui.add(egui::DragValue::new(f).speed(0.05).max_decimals(3)).changed(),
        Value::Text(t) => ui.add(egui::TextEdit::singleline(t).desired_width(90.0)).changed(),
        Value::Color(c) => ui.color_edit_button_rgba_unmultiplied(c).changed(),
        Value::Vector2(a) => {
            a.iter_mut().fold(false, |ch, x| ui.add(egui::DragValue::new(x).speed(0.05).max_decimals(2)).changed() | ch)
        }
        Value::Vector3(a) => {
            a.iter_mut().fold(false, |ch, x| ui.add(egui::DragValue::new(x).speed(0.05).max_decimals(2)).changed() | ch)
        }
        _ => false,
    }
}

/// Draw the Diagram view into `ui`.
pub fn show(ui: &mut Ui, ed: &mut Editor, view: &mut DiagramView) {
    let gd: GameData = ed.world().try_resource::<GameData>().map(|g| g.clone()).unwrap_or_default();
    let full = ui.max_rect();
    view.hovered = ui.ctx().pointer_hover_pos().is_some_and(|p| full.contains(p));

    // A Diagram opened from Assets stays until another object is selected.
    if view.pinned.is_some() && ed.selected != view.pinned_selection {
        view.pinned = None;
    }
    if let Some(path) = view.pinned.clone() {
        return show_pinned(ui, ed, view, &gd, &path);
    }
    // Which graph: the selected object's.
    let Some(e) = ed.selected.filter(|e| ed.world().is_alive(*e)) else {
        empty(ui, "Select an object to see its Diagram.", None);
        return;
    };
    let path = ed.world().get::<kit::gamesys::Diagram>(e).map(|d| d.graph.clone()).filter(|p| !p.is_empty());
    let Some(path) = path else {
        let name = ed.entity_label(e);
        if empty(ui, &format!("{name} has no Diagram yet."), Some(&format!("+ Add Diagram to {name}"))) && !ed.playing {
            if let Some(p) = add_diagram(ed, e) {
                view.load(ed, &p);
            }
        }
        return;
    };
    view.load(ed, &path);
    let diagnostics = validate(&view.graph, &gd);

    // Toolbar.
    ui.horizontal(|ui| {
        ui.label(RichText::new("DIAGRAM").size(11.0).color(MUTED()));
        ui.label(RichText::new(format!("{} › {path}", ed.entity_label(e))).size(12.0).color(TEXT_STRONG()));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("Frame all").clicked() {
                view.scene_rect = None; // re-fit on the next draw
            }
            if ui.button("+ Node").on_hover_text("Or right-click the canvas, or drag a wire into empty space").clicked()
            {
                let c = view.scene_rect.map_or(Pos2::ZERO, |r| r.center());
                view.picker = Some(Picker {
                    at: c,
                    screen: ui.ctx().content_rect().center(),
                    search: String::new(),
                    wire: None,
                    focus: true,
                });
            }
            let errors = diagnostics.iter().filter(|d| d.severity == Severity::Error).count();
            let warnings = diagnostics.len() - errors;
            if errors > 0 {
                ui.label(
                    RichText::new(format!("● {errors} error{}", if errors == 1 { "" } else { "s" })).color(ERROR()),
                )
                .on_hover_text(
                    diagnostics
                        .iter()
                        .filter(|d| d.severity == Severity::Error)
                        .map(|d| d.message.clone())
                        .collect::<Vec<_>>()
                        .join("\n"),
                );
            }
            if warnings > 0 {
                ui.label(RichText::new(format!("⚠ {warnings}")).color(WARN())).on_hover_text(
                    diagnostics
                        .iter()
                        .filter(|d| d.severity == Severity::Warning)
                        .map(|d| d.message.clone())
                        .collect::<Vec<_>>()
                        .join("\n"),
                );
            }
            if ed.playing {
                ui.label(RichText::new("● live").color(ACCENT()))
                    .on_hover_text("Lit nodes just ran. Edits apply the next time you press Play.");
            }
        });
    });
    if let Some(err) = &view.error {
        ui.label(RichText::new(err).color(ERROR()).size(11.0));
    }

    // Details strip for the selected node, or the graph's variables.
    egui::Panel::bottom("diagram_details")
        .default_size(96.0)
        .size_range(60.0..=260.0)
        .resizable(true)
        .frame(egui::Frame::NONE.inner_margin(egui::Margin::symmetric(4, 6)))
        .show_separator_line(true)
        .show(ui, |ui| {
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| details(ui, ed, view, &gd, e));
        });

    canvas(ui, ed, view, &gd, e, &diagnostics);
    picker(ui, ed, view, &gd, e);
}

impl DiagramView {
    #[cfg(test)]
    pub fn pinned(&self) -> Option<&str> {
        self.pinned.as_deref()
    }

    /// Show a Diagram file (clicked in Assets), whether or not an object uses it.
    pub fn open_path(&mut self, ed: &Editor, path: &str) {
        // (Loading a different file resets the view, so mark it afterwards.)
        self.load(ed, path);
        self.pinned = Some(path.to_owned());
        self.pinned_selection = ed.selected;
    }
}

/// The objects whose Diagram is `path`.
fn users_of(ed: &Editor, path: &str) -> Vec<Entity> {
    let world = ed.world();
    let mut out = Vec::new();
    for r in kit::scene::roots(world) {
        for e in kit::scene::descendants_and_self(world, r) {
            if world.get::<kit::gamesys::Diagram>(e).is_some_and(|d| d.graph == path) {
                out.push(e);
            }
        }
    }
    out
}

/// A Diagram opened from Assets: edit it; live debugging follows the first
/// object that uses it, and the selected object can start using it.
fn show_pinned(ui: &mut Ui, ed: &mut Editor, view: &mut DiagramView, gd: &GameData, path: &str) {
    view.load(ed, path);
    let users = users_of(ed, path);
    let selected = ed.selected.filter(|e| ed.world().is_alive(*e));
    // Any object works as the canvas's context (live view, script fields); prefer a user.
    let context = users.first().copied().or(selected).or_else(|| kit::scene::roots(ed.world()).first().copied());
    let diagnostics = validate(&view.graph, gd);
    ui.horizontal(|ui| {
        ui.label(RichText::new("DIAGRAM").size(11.0).color(MUTED()));
        ui.label(RichText::new(path).size(12.0).color(TEXT_STRONG()));
        let used = match users.len() {
            0 => "not used by any object".to_owned(),
            1 => format!("used by {}", ed.entity_label(users[0])),
            n => format!("used by {n} objects"),
        };
        ui.label(RichText::new(used).size(11.0).color(MUTED()));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("Frame all").clicked() {
                view.scene_rect = None;
            }
            if ui.button("+ Node").clicked() {
                let c = view.scene_rect.map_or(Pos2::ZERO, |r| r.center());
                view.picker = Some(Picker { at: c, screen: ui.ctx().content_rect().center(), search: String::new(), wire: None, focus: true });
            }
            if let Some(e) = selected.filter(|e| !users.contains(e)) {
                let name = ed.entity_label(e);
                if ui.add_enabled(!ed.playing, egui::Button::new(format!("Use on {name}"))).clicked() {
                    ed.app.world.insert(e, kit::gamesys::Diagram { graph: path.to_owned() });
                }
            }
            let errors = diagnostics.iter().filter(|d| d.severity == Severity::Error).count();
            if errors > 0 {
                ui.label(RichText::new(format!("● {errors} error{}", if errors == 1 { "" } else { "s" })).color(ERROR()));
            }
        });
    });
    let Some(e) = context else {
        empty(ui, "Add an object to the scene to edit this Diagram.", None);
        return;
    };
    egui::Panel::bottom("diagram_details")
        .default_size(96.0)
        .size_range(60.0..=260.0)
        .resizable(true)
        .frame(egui::Frame::NONE.inner_margin(egui::Margin::symmetric(4, 6)))
        .show_separator_line(true)
        .show(ui, |ui| {
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| details(ui, ed, view, gd, e));
        });
    canvas(ui, ed, view, gd, e, &diagnostics);
    picker(ui, ed, view, gd, e);
}

/// A centered message with an optional button; returns true when clicked.
fn empty(ui: &mut Ui, text: &str, button: Option<&str>) -> bool {
    let mut clicked = false;
    ui.centered_and_justified(|ui| {
        ui.vertical_centered(|ui| {
            ui.add_space(ui.available_height() * 0.35);
            ui.label(RichText::new(text).color(MUTED()));
            if let Some(b) = button {
                ui.add_space(8.0);
                clicked = ui.button(b).clicked();
            }
            ui.add_space(6.0);
            ui.label(
                RichText::new("Diagrams are visual logic: events → actions, no code. (7 toggles this view)")
                    .size(11.0)
                    .color(MUTED()),
            );
        });
    });
    clicked
}

fn canvas(
    ui: &mut Ui,
    ed: &mut Editor,
    view: &mut DiagramView,
    gd: &GameData,
    e: Entity,
    diags: &[kit::diagram::Diagnostic],
) {
    let painter_rect = ui.available_rect_before_wrap();
    ui.painter().rect_filled(painter_rect, 8.0, STAGE_BG());
    let live = ed.playing.then(|| {
        let now = ed.world().resource::<kit::core::Time>().elapsed;
        let rt = ed.world().resource::<kit::diagram::DiagramRuntime>();
        (now, rt.instance(e).map(|i| i.debug.clone()))
    });
    let mut scene_rect = view.scene_rect.filter(|r| r.is_finite() && r.area() > 0.0).unwrap_or_else(|| {
        // Start with every node in view (at most 1:1 zoom), top-left aligned.
        let mut r = Rect::NOTHING;
        for n in &view.graph.nodes {
            let h = node_height(rows(&n.kind.signature_in(gd, &view.graph)).len());
            r = r.union(Rect::from_min_size(pos2(n.pos[0], n.pos[1]), vec2(NODE_W, h)));
        }
        if !r.is_positive() {
            r = Rect::from_min_size(Pos2::ZERO, vec2(NODE_W, 80.0));
        }
        let r = r.expand(30.0);
        let size = painter_rect.size();
        if r.width() <= size.x && r.height() <= size.y { Rect::from_min_size(r.min, size) } else { r }
    });
    let sigs: HashMap<NodeId, Vec<Row>> =
        view.graph.nodes.iter().map(|n| (n.id, rows(&n.kind.signature_in(gd, &view.graph)))).collect();
    let pin_at = |pin: &PinRef, output: bool, graph: &Graph| -> Option<Pos2> {
        let n = graph.node(pin.node)?;
        let rows = sigs.get(&pin.node)?;
        let i = rows.iter().position(|r| {
            let p = if output { &r.output } else { &r.input };
            p.as_ref().is_some_and(|p| p.id == pin.pin)
        })?;
        Some(pin_pos(n.pos, i, output))
    };

    let mut actions: Vec<Box<dyn FnOnce(&mut Graph)>> = Vec::new();
    let mut start_wire: Option<Wire> = None;
    let mut open_picker: Option<(Pos2, Pos2)> = None;
    let mut select_click: Option<(NodeId, bool)> = None;
    let mut drag_moves: Vec<(NodeId, Vec2)> = Vec::new();
    let mut drag_started = false;
    let mut drag_stopped = false;
    let mut inline_changes: Vec<(NodeId, String, Value)> = Vec::new();
    let mut disconnect: Option<PinRef> = None;
    let mut clear_selection = false;

    let g = &ed.controls.gestures;
    let mut pan = egui::DragPanButtons::empty();
    pan.set(egui::DragPanButtons::PRIMARY, g.diagram_pan_left);
    pan.set(egui::DragPanButtons::MIDDLE, g.diagram_pan_middle);
    pan.set(egui::DragPanButtons::SECONDARY, g.diagram_pan_right);
    let picker_button = g.diagram_picker.egui();
    let resp = egui::Scene::new().zoom_range(0.2..=1.6).drag_pan_buttons(pan).show(ui, &mut scene_rect, |ui| {
        let to_scene = ui.ctx().layer_transform_from_global(ui.layer_id()).unwrap_or_default();
        view.to_global = ui.ctx().layer_transform_to_global(ui.layer_id());
        let pointer = ui.ctx().pointer_latest_pos().map(|p| to_scene * p);
        let painter = ui.painter().clone();
        // Dot grid.
        let clip = ui.clip_rect();
        let step = 32.0;
        let dot = MUTED().gamma_multiply(0.35);
        let mut x = (clip.min.x / step).floor() * step;
        while x < clip.max.x {
            let mut y = (clip.min.y / step).floor() * step;
            while y < clip.max.y {
                painter.circle_filled(pos2(x, y), 1.2, dot);
                y += step;
            }
            x += step;
        }

        // Wires (the one under the pointer is highlighted; click selects it).
        let mut hovered_wire: Option<(f32, kit::diagram::Link)> = None;
        if let Some(ptr) = pointer {
            for l in &view.graph.links {
                let (Some(a), Some(b)) = (pin_at(&l.from, true, &view.graph), pin_at(&l.to, false, &view.graph)) else {
                    continue;
                };
                let d = bezier_distance(a, b, ptr);
                if d < 7.0 && hovered_wire.as_ref().is_none_or(|(bd, _)| d < *bd) {
                    hovered_wire = Some((d, l.clone()));
                }
            }
        }
        let hovered_wire = hovered_wire.map(|(_, l)| l);
        for l in &view.graph.links {
            let (Some(a), Some(b)) = (pin_at(&l.from, true, &view.graph), pin_at(&l.to, false, &view.graph)) else {
                continue;
            };
            let ty = sigs[&l.from.node]
                .iter()
                .find_map(|r| r.output.as_ref().filter(|p| p.id == l.from.pin))
                .map(|p| p.ty.clone())
                .unwrap_or(PinType::Exec);
            let mut color = type_color(&ty);
            let mut width = if ty == PinType::Exec { 3.0 } else { 2.2 };
            if let Some((now, Some(dbg))) = &live {
                if dbg.link_time.get(&(l.from.clone(), l.to.clone())).is_some_and(|t| now - t < 0.35) {
                    color = ACCENT();
                    width += 2.0;
                }
            }
            if view.selected_wire.as_ref() == Some(l) {
                bezier(&painter, a, b, Stroke::new(width + 6.0, ACCENT().gamma_multiply(0.45)));
                color = ACCENT();
            } else if hovered_wire.as_ref() == Some(l) {
                width += 1.5;
            }
            if ty == PinType::Exec && color == Color32::BLACK {
                bezier(&painter, a, b, Stroke::new(width + 2.0, EDGE));
            }
            bezier(&painter, a, b, Stroke::new(width, color));
        }

        // Nodes.
        for node in &view.graph.nodes {
            let rows = &sigs[&node.id];
            let rect = Rect::from_min_size(pos2(node.pos[0], node.pos[1]), vec2(NODE_W, node_height(rows.len())));
            let selected = view.selected.contains(&node.id);
            let node_diags: Vec<&kit::diagram::Diagnostic> = diags.iter().filter(|d| d.node == Some(node.id)).collect();
            let has_error = node_diags.iter().any(|d| d.severity == Severity::Error);
            let runtime_error = live
                .as_ref()
                .and_then(|(_, d)| d.as_ref())
                .and_then(|d| d.error.clone())
                .filter(|(n, _)| *n == node.id);
            let lit = live.as_ref().is_some_and(|(now, d)| {
                d.as_ref().is_some_and(|d| d.node_time.get(&node.id).is_some_and(|t| now - t < 0.35))
            });
            if lit {
                painter.rect_filled(rect.expand(5.0), 10.0, ACCENT().gamma_multiply(0.35));
            }
            painter.rect_filled(rect, 7.0, CARD());
            let border = if has_error || runtime_error.is_some() {
                Stroke::new(2.0, ERROR())
            } else if selected {
                Stroke::new(2.0, ACCENT())
            } else {
                Stroke::new(1.0, CARD_BORDER())
            };
            painter.rect_stroke(rect, 7.0, border, egui::StrokeKind::Outside);
            let header = Rect::from_min_size(rect.min, vec2(NODE_W, HEADER_H));
            painter.rect_filled(
                header,
                egui::CornerRadius { nw: 7, ne: 7, sw: 0, se: 0 },
                category_color(node.kind.category()),
            );
            painter.text(
                header.left_center() + vec2(10.0, 0.0),
                Align2::LEFT_CENTER,
                node.kind.title(gd),
                FontId::proportional(13.0),
                Color32::WHITE,
            );
            if let TargetRef::Picked(p) = &node.target {
                if rows.iter().any(|r| r.input.as_ref().is_some_and(|i| i.id == "target")) {
                    painter.text(
                        header.right_center() - vec2(8.0, 0.0),
                        Align2::RIGHT_CENTER,
                        format!("→ {}", p.rsplit('/').next().unwrap_or(p)),
                        FontId::proportional(10.0),
                        Color32::from_white_alpha(200),
                    );
                }
            }
            let hr = ui.interact(header, ui.id().with(("node", node.id)), Sense::click_and_drag());
            if hr.clicked() || hr.drag_started() {
                select_click =
                    Some((node.id, ui.input(|i| ed.controls.gestures.diagram_multiselect.held(i.modifiers))));
            }
            if hr.drag_started() {
                drag_started = true;
            }
            if hr.dragged() {
                drag_moves.push((node.id, hr.drag_delta()));
            }
            if hr.drag_stopped() {
                drag_stopped = true;
            }
            let tip: Vec<String> = node_diags
                .iter()
                .map(|d| d.message.clone())
                .chain(runtime_error.iter().map(|(_, m)| format!("While playing: {m}")))
                .collect();
            if !tip.is_empty() {
                hr.on_hover_text(tip.join("\n"));
            }

            for (i, row) in rows.iter().enumerate() {
                let y = rect.min.y + HEADER_H + ROW_H * i as f32 + 2.0;
                for (pin, output) in [(&row.input, false), (&row.output, true)] {
                    let Some(pin) = pin else { continue };
                    let p = pin_pos(node.pos, i, output);
                    let pref = PinRef::new(node.id, &pin.id);
                    let connected = if output {
                        view.graph.links_from(&pref).next().is_some()
                    } else {
                        view.graph.link_into(&pref).is_some()
                    };
                    let c = type_color(&pin.ty);
                    if pin.ty == PinType::Exec {
                        let pts = vec![
                            p + vec2(-5.0, -6.0),
                            p + vec2(3.0, -6.0),
                            p + vec2(8.0, 0.0),
                            p + vec2(3.0, 6.0),
                            p + vec2(-5.0, 6.0),
                        ];
                        let fill = if connected { c } else { Color32::TRANSPARENT };
                        let edge = if c == Color32::BLACK { EDGE } else { c };
                        painter.add(egui::Shape::convex_polygon(pts, fill, Stroke::new(1.5, edge)));
                    } else if connected {
                        painter.circle_filled(p, PIN_R, c);
                    } else {
                        painter.circle_stroke(p, PIN_R, Stroke::new(1.6, c));
                    }
                    let pr = ui.interact(
                        Rect::from_center_size(p, Vec2::splat(18.0)),
                        ui.id().with(("pin", node.id, &pin.id, output)),
                        Sense::click_and_drag(),
                    );
                    if pr.drag_started() {
                        start_wire = Some(Wire { pin: pref.clone(), from_output: output, ty: pin.ty.clone() });
                    }
                    if pr.secondary_clicked() {
                        disconnect = Some(pref.clone());
                    }
                    let mut tip = format!(
                        "{} ({})",
                        if pin.name.is_empty() { if output { "Then" } else { "Run" } } else { &pin.name },
                        match &pin.ty {
                            PinType::Exec => "execution".to_owned(),
                            PinType::Data(t) => t.label(),
                        }
                    );
                    if let Some((_, Some(dbg))) = &live {
                        if let Some(v) = dbg.values.get(&pref) {
                            tip += &format!("\nLast value: {}", v.display());
                        }
                    }
                    pr.on_hover_text(format!("{tip}\nDrag to connect · right-click to disconnect"));
                    // Label.
                    if !pin.name.is_empty() {
                        let (anchor, at) = if output {
                            (Align2::RIGHT_CENTER, p - vec2(12.0, 0.0))
                        } else {
                            (Align2::LEFT_CENTER, p + vec2(12.0, 0.0))
                        };
                        painter.text(at, anchor, &pin.name, FontId::proportional(11.5), LABEL());
                    }
                    // Inline value for unconnected data inputs.
                    if !output && !connected {
                        if let PinType::Data(t) = &pin.ty {
                            let editable = !matches!(t, ValueType::Entity | ValueType::Row(_) | ValueType::Transform);
                            if editable {
                                let mut v = node
                                    .inputs
                                    .get(&pin.id)
                                    .filter(|v| v.value_type() == *t)
                                    .cloned()
                                    .or_else(|| pin.default.clone())
                                    .unwrap_or_else(|| t.default_value());
                                let label_w = ui.fonts_mut(|f| {
                                    f.layout_no_wrap(pin.name.clone(), FontId::proportional(11.5), LABEL()).size().x
                                });
                                let left = p.x + 18.0 + label_w;
                                let r =
                                    Rect::from_min_max(pos2(left, y + 1.0), pos2(rect.max.x - 8.0, y + ROW_H - 1.0));
                                let mut child = ui.new_child(
                                    egui::UiBuilder::new()
                                        .max_rect(r)
                                        .layout(egui::Layout::left_to_right(egui::Align::Center)),
                                );
                                child.spacing_mut().item_spacing.x = 3.0;
                                child.style_mut().spacing.interact_size.y = ROW_H - 6.0;
                                if value_editor(&mut child, &mut v) {
                                    inline_changes.push((node.id, pin.id.clone(), v));
                                }
                            } else if pin.id == "target" {
                                let what = match &node.target {
                                    TargetRef::SelfObject => "Self".to_owned(),
                                    TargetRef::Picked(p) => p.clone(),
                                };
                                painter.text(
                                    pos2(rect.max.x - 10.0, p.y),
                                    Align2::RIGHT_CENTER,
                                    what,
                                    FontId::proportional(10.5),
                                    MUTED(),
                                );
                            }
                        }
                    }
                }
            }
        }

        // The wire being dragged.
        if let (Some(w), Some(ptr)) = (view.wire.as_ref().or(start_wire.as_ref()), pointer) {
            if let Some(a) = pin_at(&w.pin, w.from_output, &view.graph) {
                let (from, to) = if w.from_output { (a, ptr) } else { (ptr, a) };
                let c = type_color(&w.ty);
                if c == Color32::BLACK {
                    bezier(&painter, from, to, Stroke::new(4.5, EDGE));
                }
                bezier(&painter, from, to, Stroke::new(2.5, c));
            }
        }
        let over_node = pointer.is_some_and(|p| {
            view.graph.nodes.iter().any(|n| {
                Rect::from_min_size(pos2(n.pos[0], n.pos[1]), vec2(NODE_W, node_height(sigs[&n.id].len())))
                    .expand(8.0)
                    .contains(p)
            })
        });
        (pointer, over_node, hovered_wire)
    });
    view.scene_rect = Some(scene_rect);
    let (pointer_scene, over_node, hovered_wire) = resp.inner;
    if hovered_wire.is_some() && !over_node && view.wire.is_none() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        egui::Tooltip::always_open(
            ui.ctx().clone(),
            ui.layer_id(),
            egui::Id::new("wire_tip"),
            egui::PopupAnchor::Pointer,
        )
        .gap(14.0)
        .show(|ui| ui.label(RichText::new("Click to select · Del removes the wire").size(11.0)));
    }

    // Background: click clears selection; right-click opens the picker.
    let on_canvas = ui.ctx().pointer_latest_pos().is_some_and(|p| painter_rect.contains(p)) && view.picker.is_none();
    let (left, picker_clicked) =
        ui.input(|i| (i.pointer.button_clicked(egui::PointerButton::Primary), i.pointer.button_clicked(picker_button)));
    if on_canvas && !over_node && left {
        clear_selection = true;
        view.selected_wire = hovered_wire.clone();
    }
    if on_canvas && !over_node && picker_clicked {
        if let (Some(ps), Some(screen)) = (pointer_scene, ui.ctx().pointer_latest_pos()) {
            open_picker = Some((ps, screen));
        }
    }

    // Selection and dragging nodes.
    if select_click.is_some() {
        view.selected_wire = None;
    }
    if let Some((id, add)) = select_click {
        if add {
            if !view.selected.remove(&id) {
                view.selected.insert(id);
            }
        } else if !view.selected.contains(&id) {
            view.selected = [id].into();
        }
    } else if clear_selection {
        view.selected.clear();
    }
    if drag_started && !view.dragging {
        view.dragging = true;
        view.undo.push(view.graph.clone());
        view.redo.clear();
    }
    for (id, d) in drag_moves {
        let ids: Vec<NodeId> =
            if view.selected.contains(&id) { view.selected.iter().copied().collect() } else { vec![id] };
        for n in ids {
            if let Some(n) = view.graph.node_mut(n) {
                n.pos[0] += d.x;
                n.pos[1] += d.y;
            }
        }
    }
    if drag_stopped {
        view.dragging = false;
        // Snap to the grid.
        for id in view.selected.clone() {
            if let Some(n) = view.graph.node_mut(id) {
                n.pos = [(n.pos[0] / 8.0).round() * 8.0, (n.pos[1] / 8.0).round() * 8.0];
            }
        }
        view.save(ed);
    }
    for (id, pin, v) in inline_changes {
        view.change(ed, |g| {
            if let Some(n) = g.node_mut(id) {
                n.inputs.insert(pin, v);
            }
        });
    }
    if let Some(p) = disconnect {
        view.change(ed, |g| g.disconnect_pin(&p));
    }
    for a in actions.drain(..) {
        view.change(ed, a);
    }

    // Wires: start, then drop on a pin (or empty space → picker).
    if let Some(w) = start_wire {
        view.wire = Some(w);
    }
    if view.wire.is_some() && ui.input(|i| i.pointer.any_released()) {
        let w = view.wire.take().unwrap();
        let target = pointer_scene.and_then(|ptr| {
            let mut best: Option<(f32, PinRef)> = None;
            for n in &view.graph.nodes {
                for (i, row) in sigs[&n.id].iter().enumerate() {
                    let pin = if w.from_output { &row.input } else { &row.output };
                    let Some(pin) = pin else { continue };
                    if n.id == w.pin.node || !compatible(&pin.ty, &w.ty) {
                        continue;
                    }
                    let d = pin_pos(n.pos, i, !w.from_output).distance(ptr);
                    if d < 16.0 && best.as_ref().is_none_or(|(bd, _)| d < *bd) {
                        best = Some((d, PinRef::new(n.id, &pin.id)));
                    }
                }
            }
            best.map(|(_, p)| p)
        });
        match target {
            Some(t) => {
                let exec = w.ty == PinType::Exec;
                let (from, to) = if w.from_output { (w.pin, t) } else { (t, w.pin) };
                view.change(ed, |g| g.connect(from, to, exec));
            }
            None => {
                let over_pin = pointer_scene
                    .is_some_and(|p| pin_at(&w.pin, w.from_output, &view.graph).is_some_and(|a| a.distance(p) < 12.0));
                if let (false, Some(ps), Some(screen)) = (over_pin, pointer_scene, ui.ctx().pointer_latest_pos()) {
                    view.picker = Some(Picker { at: ps, screen, search: String::new(), wire: Some(w), focus: true });
                }
            }
        }
    }
    if let Some((at, screen)) = open_picker {
        view.picker = Some(Picker { at, screen, search: String::new(), wire: None, focus: true });
    }
    let _ = e;
}

fn wire_points(a: Pos2, b: Pos2) -> [Pos2; 4] {
    let dx = ((b.x - a.x).abs() * 0.5).max(40.0);
    [a, a + vec2(dx, 0.0), b - vec2(dx, 0.0), b]
}

fn bezier(painter: &egui::Painter, a: Pos2, b: Pos2, stroke: Stroke) {
    let shape =
        egui::epaint::CubicBezierShape::from_points_stroke(wire_points(a, b), false, Color32::TRANSPARENT, stroke);
    painter.add(shape);
}

/// How far `p` is from the wire between `a` and `b`.
fn bezier_distance(a: Pos2, b: Pos2, p: Pos2) -> f32 {
    let [p0, p1, p2, p3] = wire_points(a, b);
    let at = |t: f32| {
        let u = 1.0 - t;
        (p0.to_vec2() * (u * u * u)
            + p1.to_vec2() * (3.0 * u * u * t)
            + p2.to_vec2() * (3.0 * u * t * t)
            + p3.to_vec2() * (t * t * t))
            .to_pos2()
    };
    let mut best = f32::INFINITY;
    let mut prev = p0;
    for i in 1..=32 {
        let next = at(i as f32 / 32.0);
        // Distance to the segment prev–next.
        let seg = next - prev;
        let t = if seg.length_sq() > 0.0 { ((p - prev).dot(seg) / seg.length_sq()).clamp(0.0, 1.0) } else { 0.0 };
        best = best.min((prev + seg * t).distance(p));
        prev = next;
    }
    best
}

/// Every node the picker offers for this object.
fn offered(ed: &mut Editor, view: &DiagramView, gd: &GameData, e: Entity) -> Vec<NodeKind> {
    let mut kinds = NodeKind::catalog(gd, &view.graph);
    let script = ed.world().get::<kit::gamesys::Script>(e).map(|s| s.path.clone()).filter(|p| !p.is_empty());
    if let (Some(path), Some(res)) = (script, ed.project.as_ref().map(|p| p.resources.clone())) {
        let declared = ed.app.world.resource_mut::<kit::script::ScriptRuntime>().declared(&res, &path);
        kinds.extend(NodeKind::script_field_nodes(&declared.fields));
    }
    kinds
}

fn picker(ui: &mut Ui, ed: &mut Editor, view: &mut DiagramView, gd: &GameData, e: Entity) {
    let Some(mut p) = view.picker.take() else { return };
    // The click that opened the menu mustn't close it.
    let just_opened = p.focus;
    let kinds = offered(ed, view, gd, e);
    let mut chosen: Option<NodeKind> = None;
    let mut close = false;
    let area = egui::Area::new(egui::Id::new("diagram_picker"))
        .order(egui::Order::Foreground)
        .fixed_pos(p.screen)
        .show(ui.ctx(), |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.set_width(260.0);
                let r = ui.add(egui::TextEdit::singleline(&mut p.search).hint_text("Search nodes…"));
                if std::mem::take(&mut p.focus) {
                    r.request_focus();
                }
                let q = p.search.to_lowercase();
                let fits = |k: &NodeKind| {
                    let Some(w) = &p.wire else { return true };
                    let sig = k.signature_in(gd, &view.graph);
                    let pins = if w.from_output { &sig.inputs } else { &sig.outputs };
                    pins.iter().any(|pin| compatible(&pin.ty, &w.ty))
                };
                let matches: Vec<&NodeKind> = kinds
                    .iter()
                    .filter(|k| {
                        fits(k)
                            && (q.is_empty()
                                || k.title(gd).to_lowercase().contains(&q)
                                || k.category().to_lowercase().contains(&q))
                    })
                    .collect();
                if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    chosen = matches.first().map(|k| (*k).clone());
                }
                egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                    let mut last = "";
                    for k in &matches {
                        if k.category() != last {
                            last = k.category();
                            ui.add_space(4.0);
                            ui.label(RichText::new(last).size(10.0).color(MUTED()));
                        }
                        if ui.selectable_label(false, k.title(gd)).clicked() {
                            chosen = Some((*k).clone());
                        }
                    }
                    if matches.is_empty() {
                        ui.label(RichText::new("No matching nodes").color(MUTED()));
                    }
                });
            });
        });
    if ui.input(|i| i.key_pressed(egui::Key::Escape)) || (area.response.clicked_elsewhere() && !just_opened) {
        close = true;
    }
    if let Some(kind) = chosen {
        let wire = p.wire.clone();
        let mut new_id = 0;
        view.change(ed, |g| {
            new_id = g.add(kind, [p.at.x, p.at.y]);
        });
        if let Some(w) = wire {
            let sig = view.graph.node(new_id).map(|n| n.kind.signature_in(gd, &view.graph));
            if let Some(sig) = sig {
                let pins = if w.from_output { &sig.inputs } else { &sig.outputs };
                if let Some(pin) = pins.iter().find(|pin| compatible(&pin.ty, &w.ty)) {
                    let t = PinRef::new(new_id, &pin.id);
                    let exec = w.ty == PinType::Exec;
                    let (from, to) = if w.from_output { (w.pin, t) } else { (t, w.pin) };
                    // Same undo step as adding the node.
                    view.graph.connect(from, to, exec);
                    view.save(ed);
                }
            }
        }
        view.selected = [new_id].into();
        return;
    }
    if !close {
        view.picker = Some(p);
    }
}

fn combo<T: PartialEq + Clone>(ui: &mut Ui, id: &str, value: &mut T, options: &[(T, String)]) -> bool {
    let current = options.iter().find(|(v, _)| v == value).map(|(_, l)| l.clone()).unwrap_or_default();
    let mut changed = false;
    egui::ComboBox::from_id_salt(id).selected_text(current).show_ui(ui, |ui| {
        for (v, label) in options {
            if ui.selectable_label(v == value, label).clicked() && v != value {
                *value = v.clone();
                changed = true;
            }
        }
    });
    changed
}

fn type_options(gd: &GameData) -> Vec<(ValueType, String)> {
    let mut v: Vec<(ValueType, String)> = [
        ValueType::Bool,
        ValueType::Int,
        ValueType::Float,
        ValueType::Text,
        ValueType::Color,
        ValueType::Vector3,
        ValueType::Entity,
    ]
    .into_iter()
    .map(|t| (t.clone(), t.label()))
    .collect();
    v.extend(gd.datasets.iter().map(|d| (ValueType::Row(d.id.clone()), format!("Row of {}", d.name))));
    v
}

/// Settings of the selected node, or the graph's variables.
fn details(ui: &mut Ui, ed: &mut Editor, view: &mut DiagramView, gd: &GameData, _e: Entity) {
    let single = (view.selected.len() == 1).then(|| *view.selected.iter().next().unwrap());
    let Some(id) = single.filter(|id| view.graph.node(*id).is_some()) else {
        variables(ui, ed, view, gd);
        return;
    };
    let node = view.graph.node(id).unwrap().clone();
    let mut kind = node.kind.clone();
    let mut target = node.target.clone();
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new(node.kind.title(gd)).strong());
        ui.add_space(8.0);
        use NodeKind::*;
        match &mut kind {
            OnCustomEvent { event, payload } | EmitEvent { event, payload } => {
                ui.label("Event");
                ui.add(egui::TextEdit::singleline(event).desired_width(120.0));
                ui.label("Payload");
                let mut opts = vec![(None, "None".to_owned())];
                opts.extend(
                    type_options(gd)
                        .into_iter()
                        .filter(|(t, _)| !matches!(t, ValueType::Row(_)))
                        .map(|(t, l)| (Some(t), l)),
                );
                combo(ui, "payload", payload, &opts);
            }
            DatasetRow { dataset, row } => {
                ui.label("Dataset");
                let opts: Vec<(String, String)> = gd.datasets.iter().map(|d| (d.id.clone(), d.name.clone())).collect();
                if combo(ui, "dataset", dataset, &opts) {
                    *row = gd.dataset(dataset).and_then(|d| d.rows.first().map(|r| r.id.clone()));
                }
                ui.label("Row");
                let mut opts: Vec<(Option<String>, String)> = vec![(None, "From the Row input".into())];
                if let Some(ds) = gd.dataset(dataset) {
                    opts.extend(ds.rows.iter().map(|r| (Some(r.id.clone()), r.name.clone())));
                }
                combo(ui, "row", row, &opts);
            }
            Constant { constant } => {
                ui.label("Constant");
                let opts: Vec<(String, String)> = gd.constants.iter().map(|c| (c.id.clone(), c.name.clone())).collect();
                combo(ui, "constant", constant, &opts);
            }
            GetVariable { name } | SetVariable { name } => {
                ui.label("Variable");
                let opts: Vec<(String, String)> =
                    view.graph.variables.iter().map(|v| (v.name.clone(), v.name.clone())).collect();
                combo(ui, "variable", name, &opts);
            }
            Compare { op, number } => {
                combo(ui, "op", op, &CompareOp::ALL.map(|o| (o, o.symbol().to_owned())));
                combo(
                    ui,
                    "num",
                    number,
                    &[
                        (NumberType::Int, "Int".into()),
                        (NumberType::Float, "Float".into()),
                        (NumberType::Vector3, "Vector3".into()),
                    ],
                );
            }
            Math { op, number } => {
                combo(ui, "op", op, &MathOp::ALL.map(|o| (o, o.symbol().to_owned())));
                combo(
                    ui,
                    "num",
                    number,
                    &[
                        (NumberType::Int, "Int".into()),
                        (NumberType::Float, "Float".into()),
                        (NumberType::Vector3, "Vector3".into()),
                    ],
                );
            }
            Logic { op } => {
                combo(
                    ui,
                    "op",
                    op,
                    &[(BoolOp::And, "And".into()), (BoolOp::Or, "Or".into()), (BoolOp::Not, "Not".into())],
                );
            }
            Convert { conversion } => {
                combo(
                    ui,
                    "conv",
                    conversion,
                    &[
                        (Conversion::IntToFloat, "Int → Float".into()),
                        (Conversion::FloatToInt, "Float → Int".into()),
                        (Conversion::FloatToText, "Float → Text".into()),
                        (Conversion::IntToText, "Int → Text".into()),
                    ],
                );
            }
            Sequence { outputs } => {
                ui.label("Outputs");
                ui.add(egui::DragValue::new(outputs).range(2..=8));
            }
            GetPosition { space } | SetPosition { space } | SetRotation { space } | SetTransform { space } => {
                combo(
                    ui,
                    "space",
                    space,
                    &[(Space::World, "World".into()), (Space::Local, "Local (to parent)".into())],
                );
            }
            RunScript { function, args } => {
                ui.label("Function");
                ui.add(egui::TextEdit::singleline(function).desired_width(120.0));
                ui.label(format!("Args: {}", args.len()));
                if ui.small_button("+").clicked() {
                    args.push(ValueType::Float);
                }
                if !args.is_empty() && ui.small_button("−").clicked() {
                    args.pop();
                }
                for (i, a) in args.iter_mut().enumerate() {
                    combo(
                        ui,
                        &format!("arg{i}"),
                        a,
                        &type_options(gd)
                            .into_iter()
                            .filter(|(t, _)| !matches!(t, ValueType::Row(_) | ValueType::Entity))
                            .collect::<Vec<_>>(),
                    );
                }
            }
            _ => {}
        }
        // Target for object nodes.
        let sig = node.kind.signature_in(gd, &view.graph);
        if sig.input("target").is_some() {
            ui.add_space(8.0);
            ui.label("Target");
            let world = ed.world();
            let mut opts = vec![(TargetRef::SelfObject, "Self".to_owned())];
            for r in kit::scene::roots(world) {
                for x in kit::scene::descendants_and_self(world, r) {
                    if world.get::<kit::scene::Name>(x).is_some() {
                        let p = kit::diagram::exec::path_of(world, x);
                        opts.push((TargetRef::Picked(p.clone()), p));
                    }
                }
            }
            if let TargetRef::Picked(p) = &target {
                if !opts.iter().any(|(t, _)| t == &target) {
                    opts.push((target.clone(), format!("{p} (missing)")));
                }
            }
            combo(ui, "target", &mut target, &opts);
        }
    });
    if kind != node.kind || target != node.target {
        view.change(ed, |g| {
            if let Some(n) = g.node_mut(id) {
                n.kind = kind;
                n.target = target;
            }
        });
    }
}

fn variables(ui: &mut Ui, ed: &mut Editor, view: &mut DiagramView, gd: &GameData) {
    ui.horizontal(|ui| {
        ui.label(RichText::new("VARIABLES").size(11.0).color(MUTED()));
        ui.add(egui::TextEdit::singleline(&mut view.new_var).hint_text("New variable name").desired_width(140.0));
        let name = view.new_var.trim().to_owned();
        if ui
            .add_enabled(!name.is_empty() && view.graph.variable(&name).is_none(), egui::Button::new("+ Add"))
            .clicked()
        {
            view.new_var.clear();
            view.change(ed, |g| g.variables.push(Variable { name, scope: Scope::Object, default: Value::Float(0.0) }));
        }
        ui.label(
            RichText::new("Select a node to edit its settings · right-click the canvas to add nodes")
                .size(11.0)
                .color(MUTED()),
        );
    });
    let mut vars = view.graph.variables.clone();
    let mut remove = None;
    egui::Grid::new("diagram_vars").num_columns(5).spacing([10.0, 4.0]).show(ui, |ui| {
        for (i, v) in vars.iter_mut().enumerate() {
            ui.label(&v.name);
            let mut ty = v.value_type();
            if combo(
                ui,
                &format!("vt{i}"),
                &mut ty,
                &type_options(gd).into_iter().filter(|(t, _)| !matches!(t, ValueType::Row(_))).collect::<Vec<_>>(),
            ) {
                v.default = ty.default_value();
            }
            combo(
                ui,
                &format!("vs{i}"),
                &mut v.scope,
                &[(Scope::Object, "Per object".into()), (Scope::Scene, "Shared in scene".into())],
            );
            ui.horizontal(|ui| value_editor(ui, &mut v.default));
            if ui.small_button("🗑").on_hover_text("Remove (nodes using it will be flagged)").clicked() {
                remove = Some(i);
            }
            ui.end_row();
        }
    });
    if let Some(i) = remove {
        vars.remove(i);
    }
    if vars != view.graph.variables {
        view.change(ed, |g| g.variables = vars);
    }
}
