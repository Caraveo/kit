//! Rigg in the editor: Rig mode (key 9) for the selected model. The model
//! turns see-through, its volumes show as teal shapes (selected: amber), and
//! you pick and move them with the usual gizmos. Fit mode moves volumes
//! without bending the mesh (it becomes the new rest pose); Pose mode bends it.

use crate::state::Editor;
use crate::theme::*;
use egui::{Align2, Color32, CornerRadius, Frame, Margin, Pos2, RichText, Stroke, Ui, vec2};
use kit::ecs::Entity;
use kit::gamesys::{Model, Rig, RigShape, RigVolume};
use kit::math::{Mat4, Vec3, Vec4};
use kit::render::draw::Overlay;
use kit::scene::{GlobalTransform, Name, Transform};

/// Teal for volumes, amber for the selected one and joints.
pub const TEAL: Vec4 = Vec4::new(0.25, 0.92, 0.80, 0.30);
pub const AMBER: Vec4 = Vec4::new(1.0, 0.62, 0.18, 0.55);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RigTool {
    /// Move volumes to fit the model; the mesh doesn't bend (rest pose follows).
    #[default]
    Fit,
    /// Move volumes to pose: the mesh bends.
    Pose,
}

/// Rig mode state (in `UiState`).
#[derive(Default)]
pub struct RiggState {
    pub tool: RigTool,
    pub weights: bool,
    pub timeline: crate::rigg_timeline::Timeline,
    fit_edit: Option<Entity>,
}

/// The rig being edited: the selection's rig (the selection itself, or the
/// model a selected volume belongs to).
pub fn target(ed: &Editor) -> Option<Entity> {
    let world = ed.world();
    let mut e = ed.selected.filter(|e| world.is_alive(*e))?;
    if !world.has::<Model>(e) && !world.has::<RigVolume>(e) {
        return None;
    }
    loop {
        if world.has::<Model>(e) {
            return world.has::<Rig>(e).then_some(e);
        }
        e = world.get::<kit::scene::Parent>(e)?.0;
    }
}

pub fn available(ed: &Editor) -> bool {
    !ed.playing && (target(ed).is_some() || rigging_candidate(ed).is_some())
}

/// The selected model that could get a rig (has a Model, no Rig yet).
pub fn rigging_candidate(ed: &Editor) -> Option<Entity> {
    let e = ed.selected.filter(|e| ed.world().is_alive(*e))?;
    (ed.world().has::<Model>(e) && !ed.world().has::<Rig>(e)).then_some(e)
}

/// Add a rig to a model (Fit tool; no volumes yet).
pub fn add_rig(ed: &mut Editor, e: Entity) {
    ed.app.world.insert(e, Rig::default());
}

/// Shapes for one volume in world space (capsules: a cylinder and two spheres).
fn shapes(world_m: Mat4, shape: &RigShape, color: Vec4) -> Vec<Overlay> {
    use kit::gamesys::MeshKind;
    let o = |mesh, local: Mat4| Overlay { mesh, transform: world_m * local, color };
    match *shape {
        RigShape::Sphere { radius } => vec![o(MeshKind::Sphere, Mat4::from_scale(Vec3::splat(radius * 2.0)))],
        RigShape::Box { size } => vec![o(MeshKind::Cube, Mat4::from_scale(size))],
        RigShape::Capsule { radius, length } => {
            let h = length * 0.5;
            vec![
                o(MeshKind::Cylinder, Mat4::from_scale(Vec3::new(radius * 2.0, length, radius * 2.0))),
                o(MeshKind::Sphere, Mat4::from_translation(Vec3::Y * h) * Mat4::from_scale(Vec3::splat(radius * 2.0))),
                o(MeshKind::Sphere, Mat4::from_translation(-Vec3::Y * h) * Mat4::from_scale(Vec3::splat(radius * 2.0))),
            ]
        }
    }
}

/// Add Rig mode's ghost model and volume shapes to the editor view.
pub fn decorate_view(ed: &Editor, view: &mut kit::render::View) {
    if ed.playing {
        return;
    }
    let world = ed.world();
    let rig = target(ed);
    // Outside Rig mode, a selected volume still shows (so it can be found).
    let active_rig = rig.filter(|_| ed.settings.rig);
    if let Some(rig) = active_rig {
        view.ghosts.push(rig);
        view.selected.clear();
    }
    let volumes: Vec<Entity> = match (active_rig, ed.selected) {
        (Some(rig), _) => kit::rigg::volumes_of(world, rig),
        (None, Some(s)) if rig.is_some() && world.has::<RigVolume>(s) => vec![s],
        _ => Vec::new(),
    };
    for v in volumes {
        let (Some(vol), Some(g)) = (world.get::<RigVolume>(v), world.get::<GlobalTransform>(v)) else { continue };
        let color = if ed.selected == Some(v) { AMBER } else { TEAL };
        view.overlay.extend(shapes(g.0, &vol.shape, color));
    }
}

/// Ray-pick a volume of the active rig: the nearest one hit.
pub fn pick(ed: &Editor, ray: &kit::render::pick::Ray) -> Option<Entity> {
    if !ed.settings.rig {
        return None;
    }
    let world = ed.world();
    let rig = target(ed)?;
    let mut best: Option<(f32, Entity)> = None;
    for v in kit::rigg::volumes_of(world, rig) {
        let (Some(vol), Some(g)) = (world.get::<RigVolume>(v), world.get::<GlobalTransform>(v)) else { continue };
        let inv = g.0.inverse();
        let (o, d) = (inv.transform_point3(ray.origin), inv.transform_vector3(ray.dir));
        let len = d.length();
        if len <= 0.0 {
            continue;
        }
        let d = d / len;
        // March along the ray against the shape's distance field.
        let mut t = 0.0;
        for _ in 0..96 {
            let dist = vol.shape.distance(o + d * t);
            if dist < 1e-3 {
                let world_t = t / len;
                if best.is_none_or(|(bt, _)| world_t < bt) {
                    best = Some((world_t, v));
                }
                break;
            }
            t += dist.max(1e-3);
            if t > 1e4 {
                break;
            }
        }
    }
    best.map(|(_, e)| e)
}

pub fn fit_snapshot(ed: &Editor, st: &RiggState) -> Option<(Entity, Vec<(Entity, Mat4)>)> {
    if st.tool != RigTool::Fit || !ed.settings.rig || ed.playing {
        return None;
    }
    let rig = target(ed)?;
    let transforms = kit::rigg::volumes_of(ed.world(), rig)
        .into_iter()
        .filter_map(|e| ed.world().get::<Transform>(e).map(|t| (e, t.matrix())))
        .collect();
    Some((rig, transforms))
}

/// Update the influence preview and commit a Fit edit after the pointer is released.
pub fn sync(ed: &mut Editor, st: &mut RiggState, before: Option<(Entity, Vec<(Entity, Mat4)>)>, pointer_down: bool) {
    let rig = target(ed).filter(|_| ed.settings.rig && !ed.playing);
    let preview = kit::rigg::RigPreview {
        rig,
        weights: st.weights && rig.is_some(),
        focus: rig.and(ed.selected.filter(|s| ed.world().has::<RigVolume>(*s))),
        fitting: st.tool == RigTool::Fit && rig.is_some(),
    };
    if *ed.app.world.resource::<kit::rigg::RigPreview>() != preview {
        *ed.app.world.resource_mut::<kit::rigg::RigPreview>() = preview;
    }
    let after = fit_snapshot(ed, st);
    if let (Some(before), Some(after)) = (before, after.as_ref()) {
        if before.0 == after.0 && before.1 != after.1 {
            st.fit_edit = Some(after.0);
        }
    }
    if !pointer_down {
        if let Some(rig) = st.fit_edit.take() {
            if ed.world().has::<Rig>(rig) {
                kit::rigg::rebind(&mut ed.app.world, rig);
            }
        }
    }
}

/// Where a new volume goes: in front of the selected volume's end, or the
/// model's center.
fn new_volume(ed: &mut Editor, rig: Entity, shape: RigShape, base: &str) -> Entity {
    let world = ed.world();
    let parent = ed.selected.filter(|s| world.has::<RigVolume>(*s)).unwrap_or(rig);
    let mut name = base.to_owned();
    let mut n = 2;
    while kit::rigg::volumes_of(world, rig).iter().any(|v| world.get::<Name>(*v).is_some_and(|x| x.0 == name)) {
        name = format!("{base}_{n}");
        n += 1;
    }
    let offset = if parent == rig {
        // Center of the model's rest mesh.
        let mesh = world
            .get::<Model>(rig)
            .map(|m| if m.mesh_asset.is_empty() { format!("builtin:{:?}", m.mesh) } else { m.mesh_asset.clone() });
        let bounds = mesh.and_then(|m| world.resource::<kit::rigg::RigRuntime>().rest_bounds(&m));
        bounds.map_or(Vec3::ZERO, |(a, b)| (a + b) * 0.5)
    } else {
        Vec3::new(0.0, 0.15, 0.0)
    };
    let e = kit::rigg::preset::add_volume(
        &mut ed.app.world,
        parent,
        &name,
        shape,
        Transform::from_xyz(offset.x, offset.y, offset.z),
    );
    ed.selected = Some(e);
    e
}

fn segmented(ui: &mut Ui, on: bool, label: &str, tip: &str) -> bool {
    let fill = if on { ACCENT().gamma_multiply(0.28) } else { Color32::TRANSPARENT };
    let text = RichText::new(label).size(12.5).color(if on { TEXT_STRONG() } else { LABEL() });
    ui.add(egui::Button::new(text).fill(fill).corner_radius(CornerRadius::same(8)).min_size(vec2(0.0, 26.0)))
        .on_hover_text(tip)
        .clicked()
}

/// The floating Rig toolbar at the top of the viewport.
pub fn toolbar(ui: &mut Ui, ed: &mut Editor, st: &mut RiggState, rect: egui::Rect) {
    if !ed.settings.rig || !available(ed) {
        return;
    }
    let rig = target(ed);
    egui::Area::new(egui::Id::new("rig_toolbar"))
        .fixed_pos(rect.center_top() + vec2(0.0, 48.0))
        .pivot(Align2::CENTER_TOP)
        .order(egui::Order::Middle)
        .show(ui.ctx(), |ui| {
            Frame::new()
                .fill(DOCK_BG())
                .stroke(Stroke::new(1.0, DOCK_BORDER()))
                .corner_radius(CornerRadius::same(14))
                .inner_margin(Margin::symmetric(10, 6))
                .shadow(egui::Shadow { offset: [0, 4], blur: 16, spread: 0, color: Color32::from_black_alpha(90) })
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("RIGG").size(11.0).strong().color(Color32::from_rgb(64, 235, 204)));
                        let Some(rig) = rig else {
                            if let Some(e) = rigging_candidate(ed) {
                                let name = ed.entity_label(e);
                                if ui.button(format!("Rig {name}")).clicked() {
                                    add_rig(ed, e);
                                }
                            }
                            return;
                        };
                        ui.label(RichText::new(ed.entity_label(rig)).size(12.5).color(TEXT_STRONG()));
                        if ed.selected != Some(rig) && ui.button("Select mesh").clicked() {
                            ed.selected = Some(rig);
                        }
                        ui.separator();
                        if segmented(ui, st.tool == RigTool::Fit, "Fit", "Place volumes inside the model. The mesh doesn't bend; where you leave them becomes the rest pose.") {
                            st.tool = RigTool::Fit;
                        }
                        if segmented(ui, st.tool == RigTool::Pose, "Pose", "Move volumes to bend the mesh.") {
                            st.tool = RigTool::Pose;
                        }
                        ui.separator();
                        if segmented(ui, st.weights, "Weights", "Color the mesh by influence (the selected volume, or all volumes)") {
                            st.weights = !st.weights;
                        }
                        ui.separator();
                        let volumes = kit::rigg::volumes_of(ed.world(), rig);
                        if volumes.is_empty() {
                            if ui.button("Humanoid preset").on_hover_text("Add ~30 volumes fitted to this model").clicked() {
                                add_humanoid(ed, rig);
                            }
                        }
                        ui.menu_button("+ Volume", |ui| {
                            for (label, shape) in [
                                ("Sphere (joint)", RigShape::Sphere { radius: 0.08 }),
                                ("Capsule (limb)", RigShape::Capsule { radius: 0.06, length: 0.3 }),
                                ("Box (torso)", RigShape::Box { size: Vec3::new(0.3, 0.25, 0.2) }),
                            ] {
                                if ui.button(label).clicked() {
                                    new_volume(ed, rig, shape, shape.label());
                                    ui.close();
                                }
                            }
                        });
                        let sel_volume = ed.selected.filter(|s| ed.world().has::<RigVolume>(*s));
                        if ui.add_enabled(sel_volume.is_some(), egui::Button::new("Mirror L↔R")).on_hover_text("Copy this L_/R_ volume's shape and pose to the other side").clicked() {
                            if kit::rigg::mirror(&mut ed.app.world, rig, sel_volume.unwrap()).is_none() {
                                ed.notify("Mirroring needs a matching L_/R_ name on the other side");
                            }
                        }
                        if st.tool == RigTool::Pose && ui.button("Reset pose").on_hover_text("Put every volume back at rest").clicked() {
                            kit::rigg::reset_pose(&mut ed.app.world, rig);
                        }
                        if segmented(ui, st.timeline.open, "Animation", "Show the Animation timeline") {
                            st.timeline.open = !st.timeline.open;
                            if st.timeline.open {
                                st.tool = RigTool::Pose;
                            }
                        }
                        if ui.button("Done").on_hover_text("Leave Rig mode (9)").clicked() {
                            ed.settings.rig = false;
                        }
                    });
                });
        });
}

/// Add the humanoid preset, fitted to the model's rest mesh.
pub fn add_humanoid(ed: &mut Editor, rig: Entity) {
    let mesh = ed
        .world()
        .get::<Model>(rig)
        .map(|m| if m.mesh_asset.is_empty() { format!("builtin:{:?}", m.mesh) } else { m.mesh_asset.clone() });
    let bounds = mesh.and_then(|m| ed.world().resource::<kit::rigg::RigRuntime>().rest_bounds(&m));
    let Some(bounds) = bounds else {
        ed.notify("The model is still loading; try again in a moment");
        return;
    };
    let folder = kit::rigg::preset::add_humanoid(&mut ed.app.world, rig, bounds);
    kit::scene::propagate_transforms(&mut ed.app.world);
    kit::rigg::rebind(&mut ed.app.world, rig);
    ed.selected = kit::scene::children(ed.world(), folder).first().copied();
    ed.notify("Humanoid rig added: adjust volumes in Fit, then switch to Pose");
}

/// Lines between connected volumes and amber joint dots (screen overlay).
pub fn paint_connections(ui: &Ui, ed: &Editor, view_proj: Mat4, rect: egui::Rect) {
    if !ed.settings.rig || ed.playing {
        return;
    }
    let Some(rig) = target(ed) else { return };
    let world = ed.world();
    let to_screen = |p: Vec3| -> Option<Pos2> {
        let c = view_proj * p.extend(1.0);
        if c.w <= 1e-4 {
            return None;
        }
        let n = c.truncate() / c.w;
        Some(Pos2::new(rect.left() + (n.x * 0.5 + 0.5) * rect.width(), rect.top() + (n.y * 0.5 + 0.5) * rect.height()))
    };
    let painter = ui.painter_at(rect);
    let volumes = kit::rigg::volumes_of(world, rig);
    let center = |e: Entity| world.get::<GlobalTransform>(e).map(|g| g.0.w_axis.truncate());
    for v in &volumes {
        let Some(c) = center(*v) else { continue };
        // Parent links (volume → parent volume) and named extra links.
        let mut others: Vec<Entity> = Vec::new();
        if let Some(p) = world.get::<kit::scene::Parent>(*v).map(|p| p.0).filter(|p| volumes.contains(p)) {
            others.push(p);
        }
        if let Some(vol) = world.get::<RigVolume>(*v) {
            for name in &vol.links {
                if let Some(o) = volumes.iter().find(|o| world.get::<Name>(**o).is_some_and(|n| &n.0 == name)) {
                    others.push(*o);
                }
            }
        }
        for o in others {
            if let (Some(a), Some(b)) = (to_screen(c), center(o).and_then(to_screen)) {
                painter.line_segment([a, b], Stroke::new(1.5, Color32::from_rgba_unmultiplied(220, 235, 245, 170)));
            }
        }
    }
    for v in &volumes {
        let is_joint = world.get::<RigVolume>(*v).is_some_and(|r| matches!(r.shape, RigShape::Sphere { .. }));
        if let (true, Some(p)) = (is_joint, center(*v).and_then(to_screen)) {
            let sel = ed.selected == Some(*v);
            painter.circle_filled(p, if sel { 5.5 } else { 4.0 }, Color32::from_rgb(255, 170, 60));
            painter.circle_stroke(p, if sel { 5.5 } else { 4.0 }, Stroke::new(1.2, Color32::from_rgb(120, 60, 10)));
        }
    }
}

// ------------------------------------------------------------ Inspector

fn row(ui: &mut Ui, label: &str, add: impl FnOnce(&mut Ui)) {
    ui.horizontal(|ui| {
        ui.add_sized([86.0, 22.0], egui::Label::new(RichText::new(label).size(12.0).color(LABEL())));
        add(ui);
    });
}

/// A small diagram of the shape with its dimensions (like the Rig Volume card).
fn shape_diagram(ui: &mut Ui, shape: &RigShape) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 96.0), egui::Sense::hover());
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 10.0, FIELD_BG());
    let teal = Color32::from_rgb(64, 235, 204);
    let fill = Color32::from_rgba_unmultiplied(64, 235, 204, 40);
    let c = rect.center() - vec2(24.0, 0.0);
    let label = |pos: Pos2, text: &str| {
        p.text(pos, Align2::LEFT_CENTER, text, egui::FontId::proportional(11.0), LABEL());
    };
    match shape {
        RigShape::Sphere { .. } => {
            p.circle(c, 30.0, fill, Stroke::new(1.5, teal));
            p.line_segment([c, c + vec2(30.0, 0.0)], Stroke::new(1.0, LABEL()));
            label(c + vec2(40.0, 0.0), "Radius");
        }
        RigShape::Capsule { .. } => {
            let r = egui::Rect::from_center_size(c, vec2(36.0, 78.0));
            p.rect(r, CornerRadius::same(18), fill, Stroke::new(1.5, teal), egui::StrokeKind::Middle);
            p.line_segment(
                [r.left_bottom() + vec2(0.0, 6.0), r.right_bottom() + vec2(0.0, 6.0)],
                Stroke::new(1.0, LABEL()),
            );
            p.line_segment(
                [r.right_top() + vec2(12.0, 0.0), r.right_bottom() + vec2(12.0, 0.0)],
                Stroke::new(1.0, LABEL()),
            );
            label(r.right_center() + vec2(18.0, 0.0), "Length");
            label(r.center_bottom() + vec2(24.0, 4.0), "Radius");
        }
        RigShape::Box { .. } => {
            let r = egui::Rect::from_center_size(c, vec2(60.0, 50.0));
            p.rect(r, CornerRadius::same(4), fill, Stroke::new(1.5, teal), egui::StrokeKind::Middle);
            label(r.right_center() + vec2(10.0, 0.0), "Size X · Y · Z");
        }
    }
}

/// Inspector section for a volume.
pub fn volume_section(
    ui: &mut Ui,
    ed: &mut Editor,
    e: Entity,
    tool: RigTool,
    section: impl FnOnce(&mut Ui, &mut dyn FnMut(&mut Ui)) -> bool,
) {
    let Some(mut v) = ed.world().get_cloned::<RigVolume>(e) else { return };
    let rig = target(ed);
    let mut mirror = false;
    let mut new_parent = None;
    let keep = section(ui, &mut |ui: &mut Ui| {
        row(ui, "Shape", |ui| {
            let current = v.shape.label();
            egui::ComboBox::from_id_salt("rig_shape").selected_text(current).show_ui(ui, |ui| {
                let size = v.shape.extent();
                for s in [
                    RigShape::Sphere { radius: size },
                    RigShape::Capsule { radius: size, length: size * 3.0 },
                    RigShape::Box { size: Vec3::splat(size * 2.0) },
                ] {
                    if ui.selectable_label(s.label() == current, s.label()).clicked() && s.label() != current {
                        v.shape = s;
                    }
                }
            });
        });
        shape_diagram(ui, &v.shape);
        match &mut v.shape {
            RigShape::Sphere { radius } => row(ui, "Radius", |ui| {
                ui.add(egui::DragValue::new(radius).speed(0.005).range(0.005..=100.0).suffix(" m"));
            }),
            RigShape::Capsule { radius, length } => {
                row(ui, "Radius", |ui| {
                    ui.add(egui::DragValue::new(radius).speed(0.005).range(0.005..=100.0).suffix(" m"));
                });
                row(ui, "Length", |ui| {
                    ui.add(egui::DragValue::new(length).speed(0.01).range(0.0..=100.0).suffix(" m"));
                });
            }
            RigShape::Box { size } => row(ui, "Size", |ui| {
                for (i, a) in ["X ", "Y ", "Z "].iter().enumerate() {
                    ui.add(egui::DragValue::new(&mut size[i]).speed(0.01).range(0.01..=100.0).prefix(*a));
                }
            }),
        }
        row(ui, "Influence", |ui| {
            ui.add(egui::Slider::new(&mut v.influence, 0.0..=1.0).fixed_decimals(2));
        });
        row(ui, "Falloff", |ui| {
            ui.add(egui::Slider::new(&mut v.falloff, 0.0..=1.0).fixed_decimals(2).suffix(" m"))
                .on_hover_text("How far past its surface the volume still pulls the mesh");
        });
        ui.add_space(4.0);
        ui.label(RichText::new("CONNECTED TO").size(10.5).color(MUTED()));
        let world = ed.world();
        let parent = world.get::<kit::scene::Parent>(e).map(|p| p.0).filter(|p| world.has::<RigVolume>(*p));
        if let Some(rig) = rig.filter(|_| ed.settings.rig && tool == RigTool::Fit) {
            let descendants = kit::scene::descendants_and_self(world, e);
            let current = parent.map_or_else(|| "None".into(), |p| ed.entity_label(p));
            row(ui, "Follow", |ui| {
                egui::ComboBox::from_id_salt("rig_follow").selected_text(current).show_ui(ui, |ui| {
                    if ui.selectable_label(parent.is_none(), "None").clicked() && parent.is_some() {
                        new_parent = Some(rig);
                    }
                    for p in kit::rigg::volumes_of(world, rig).into_iter().filter(|p| !descendants.contains(p)) {
                        if ui.selectable_label(parent == Some(p), ed.entity_label(p)).clicked() && parent != Some(p) {
                            new_parent = Some(p);
                        }
                    }
                });
            });
        } else if let Some(p) = parent {
            ui.label(RichText::new(format!("↑ {} (fixed length)", ed.entity_label(p))).size(12.0).color(LABEL()));
        }
        for c in kit::scene::children(world, e).into_iter().filter(|c| world.has::<RigVolume>(*c)) {
            ui.label(RichText::new(format!("↓ {}", ed.entity_label(c))).size(12.0).color(LABEL()));
        }
        for l in &v.links {
            ui.label(RichText::new(format!("↔ {l}")).size(12.0).color(LABEL()));
        }
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            if ui.button("Mirror L↔R").clicked() {
                mirror = true;
            }
            if !ed.settings.rig && ui.button("Rig mode (9)").clicked() {
                crate::ui::enter_rig(ed);
            }
        });
    });
    if !keep {
        kit::scene::despawn_recursive(&mut ed.app.world, e);
        ed.selected = rig;
        return;
    }
    if ed.world().get::<RigVolume>(e).is_some_and(|cur| *cur != v) {
        *ed.app.world.get_mut::<RigVolume>(e).unwrap() = v;
    }
    if let (Some(parent), Some(rig)) = (new_parent, rig) {
        let index = kit::scene::children(ed.world(), parent).len();
        ed.reparent(e, Some(parent), index);
        kit::rigg::rebind(&mut ed.app.world, rig);
    }
    if let (true, Some(rig)) = (mirror, rig) {
        if kit::rigg::mirror(&mut ed.app.world, rig, e).is_none() {
            ed.notify("Mirroring needs a matching L_/R_ name on the other side");
        }
    }
}

/// Inspector section for a rigged model.
pub fn rig_section(
    ui: &mut Ui,
    ed: &mut Editor,
    e: Entity,
    section: impl FnOnce(&mut Ui, &mut dyn FnMut(&mut Ui)) -> bool,
) {
    let Some(mut r) = ed.world().get_cloned::<Rig>(e) else { return };
    let volumes = kit::rigg::volumes_of(ed.world(), e).len();
    let mut open_mode = false;
    let mut preset = false;
    let mut pose_action: Option<(bool, String)> = None;
    let mut pose_name = ui.ctx().data(|d| d.get_temp::<String>(egui::Id::new("rig_pose_name"))).unwrap_or_default();
    let keep = section(ui, &mut |ui: &mut Ui| {
        row(ui, "Volumes", |ui| {
            ui.label(RichText::new(volumes.to_string()).color(TEXT_STRONG()));
            if !ed.settings.rig && ui.button("Edit in Rig mode (9)").clicked() {
                open_mode = true;
            }
            if volumes == 0 && ui.button("Humanoid preset").clicked() {
                preset = true;
            }
        });
        row(ui, "", |ui| {
            ui.checkbox(&mut r.enabled, "Deform mesh");
            ui.checkbox(&mut r.preserve_volume, "Preserve volume")
                .on_hover_text("Smoother bends and twists (dual-quaternion blending)");
        });
        row(ui, "Play on start", |ui| {
            let shown = if r.autoplay.is_empty() { "None".to_owned() } else { r.autoplay.clone() };
            egui::ComboBox::from_id_salt("rig_autoplay").selected_text(shown).show_ui(ui, |ui| {
                ui.selectable_value(&mut r.autoplay, String::new(), "None");
                for c in &r.clips {
                    ui.selectable_value(&mut r.autoplay, c.name.clone(), &c.name);
                }
            });
        });
        ui.add_space(4.0);
        ui.label(RichText::new("POSES").size(10.5).color(MUTED()));
        for p in &r.poses {
            ui.horizontal(|ui| {
                ui.label(RichText::new(&p.name).size(12.0).color(LABEL()));
                if ui.small_button("Apply").clicked() {
                    pose_action = Some((false, p.name.clone()));
                }
            });
        }
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut pose_name).hint_text("Pose name").desired_width(120.0));
            if ui.add_enabled(!pose_name.trim().is_empty(), egui::Button::new("Save pose")).clicked() {
                pose_action = Some((true, pose_name.trim().to_owned()));
            }
        });
    });
    ui.ctx().data_mut(|d| d.insert_temp(egui::Id::new("rig_pose_name"), pose_name));
    if !keep {
        ed.app.world.remove::<Rig>(e);
        return;
    }
    if ed.world().get::<Rig>(e).is_some_and(|cur| *cur != r) {
        *ed.app.world.get_mut::<Rig>(e).unwrap() = r;
    }
    if open_mode {
        crate::ui::enter_rig(ed);
    }
    if preset {
        add_humanoid(ed, e);
    }
    match pose_action {
        Some((true, name)) => {
            kit::rigg::save_pose(&mut ed.app.world, e, &name);
            ed.notify(format!("Saved pose {name}"));
        }
        Some((false, name)) => {
            kit::rigg::apply_pose(&mut ed.app.world, e, &name);
        }
        None => {}
    }
}
