//! The move gizmo: colored X / Y / Z arrows on the selected object.
//! Drag an arrow to slide the object along that world axis, or the center
//! dot to move it freely across the view plane. Hold Ctrl to snap to 0.5.
//! Colored rings rotate around each axis (Ctrl snaps to 15°).

use crate::canvas::{hit_plane, screen_ray, to_screen, view_forward};
use crate::state::Editor;
use crate::theme::viewport::*;
use egui::{Color32, Pos2, Rect, Stroke};
use kit::ecs::Entity;
use kit::math::{Quat, Vec3};
use kit::render::View;
use kit::render::pick::Ray;
use kit::scene::{GlobalTransform, Parent, Transform};

/// Arrow length on screen, in points.
const ARROW_PX: f32 = 90.0;
/// How close (in points) the pointer must be to grab a handle.
const GRAB_PX: f32 = 9.0;
const CENTER_PX: f32 = 7.0;
const SNAP: f32 = 0.5;
/// Rotation rings, as a fraction of the arrow length.
const RING_SCALE: f32 = 0.7;
const RING_SEGMENTS: usize = 64;
const ANGLE_SNAP_DEG: f32 = 15.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Handle {
    Axis(usize),
    Ring(usize),
    Center,
}

#[derive(Clone, Copy)]
struct Drag {
    handle: Handle,
    entity: Entity,
    /// World position of the object when the drag began.
    start: Vec3,
    /// Where the grab landed: the axis parameter, the point on the view plane,
    /// or (rings) the direction from the center on the ring's plane.
    grab: Vec3,
    start_rotation: Quat,
    /// Rings: the angle turned so far (radians) and the pointer where the drag began.
    angle: f32,
    press: Pos2,
}

#[derive(Default)]
pub struct MoveGizmo {
    drag: Option<Drag>,
    /// The handle under the pointer (highlighted).
    pub hovered: Option<Handle>,
}

const AXES: [Vec3; 3] = [Vec3::X, Vec3::Y, Vec3::Z];

fn axis_color(i: usize) -> Color32 {
    [AXIS_X(), AXIS_Y(), AXIS_Z()][i]
}

/// The selection the gizmo acts on, if it should be shown.
fn target(ed: &Editor) -> Option<(Entity, Vec3)> {
    if ed.playing || ed.settings.canvas || ed.settings.sculpt.is_some() {
        return None;
    }
    let e = ed.selected.filter(|e| ed.world().is_alive(*e))?;
    ed.world().get::<Transform>(e)?;
    let pos = ed.world().get::<GlobalTransform>(e).map(|g| g.0.w_axis.truncate())?;
    Some((e, pos))
}

/// World length that shows up as `ARROW_PX` on screen at `origin`.
fn arrow_length(view: &View, rect: Rect, origin: Vec3) -> Option<f32> {
    let right = view.view.row(0).truncate().normalize_or_zero();
    let a = to_screen(view.view_proj(), rect, origin)?;
    let b = to_screen(view.view_proj(), rect, origin + right)?;
    let px_per_unit = (b - a).length();
    (px_per_unit > 1e-3).then(|| ARROW_PX / px_per_unit)
}

/// Screen-space segments for each visible axis: (axis, origin, tip).
fn segments(ed: &Editor, view: &View, rect: Rect, origin: Vec3) -> Vec<(usize, Pos2, Pos2)> {
    let Some(len) = arrow_length(view, rect, origin) else { return Vec::new() };
    let vp = view.view_proj();
    let Some(o) = to_screen(vp, rect, origin) else { return Vec::new() };
    let count = if ed.camera.two_d { 2 } else { 3 };
    (0..count)
        .filter_map(|i| {
            let tip = to_screen(vp, rect, origin + AXES[i] * len)?;
            // An axis pointing straight at the camera can't be dragged meaningfully.
            ((tip - o).length() > 8.0).then_some((i, o, tip))
        })
        .collect()
}

/// Two directions spanning the plane a ring lies in.
fn ring_basis(i: usize) -> (Vec3, Vec3) {
    match i {
        0 => (Vec3::Y, Vec3::Z),
        1 => (Vec3::Z, Vec3::X),
        _ => (Vec3::X, Vec3::Y),
    }
}

fn ring_axes(ed: &Editor) -> &'static [usize] {
    if ed.camera.two_d { &[2] } else { &[0, 1, 2] }
}

/// Screen points around a ring, each flagged true when on the camera-facing half.
fn ring_points(view: &View, rect: Rect, origin: Vec3, i: usize) -> Vec<(Pos2, bool)> {
    let Some(len) = arrow_length(view, rect, origin) else { return Vec::new() };
    let r = len * RING_SCALE;
    let (u, v) = ring_basis(i);
    let to_cam = (view.camera_pos - origin).normalize_or_zero();
    (0..=RING_SEGMENTS)
        .filter_map(|k| {
            let a = k as f32 / RING_SEGMENTS as f32 * std::f32::consts::TAU;
            let d = u * a.cos() + v * a.sin();
            Some((to_screen(view.view_proj(), rect, origin + d * r)?, d.dot(to_cam) >= -0.05))
        })
        .collect()
}

fn distance_to_ring(p: Pos2, pts: &[(Pos2, bool)]) -> f32 {
    pts.windows(2).map(|w| distance_to_segment(p, w[0].0, w[1].0)).fold(f32::INFINITY, f32::min)
}

fn distance_to_segment(p: Pos2, a: Pos2, b: Pos2) -> f32 {
    let ab = b - a;
    let t = ((p - a).dot(ab) / ab.length_sq().max(1e-6)).clamp(0.0, 1.0);
    (a + ab * t - p).length()
}

/// The handle at a screen position, if any.
pub fn handle_at(ed: &Editor, view: &View, rect: Rect, pos: Pos2) -> Option<Handle> {
    let (_, origin) = target(ed)?;
    let segs = segments(ed, view, rect, origin);
    let o = to_screen(view.view_proj(), rect, origin)?;
    if (pos - o).length() <= CENTER_PX + 3.0 {
        return Some(Handle::Center);
    }
    let arrows = segs.iter().map(|&(i, a, b)| (Handle::Axis(i), distance_to_segment(pos, a, b)));
    let rings =
        ring_axes(ed).iter().map(|&i| (Handle::Ring(i), distance_to_ring(pos, &ring_points(view, rect, origin, i))));
    arrows
        .chain(rings)
        .filter(|&(_, d)| d <= GRAB_PX)
        // Arrows win ties where they cross a ring.
        .min_by(|x, y| {
            (x.1 - matches!(x.0, Handle::Axis(_)) as u8 as f32 * 2.0)
                .total_cmp(&(y.1 - matches!(y.0, Handle::Axis(_)) as u8 as f32 * 2.0))
        })
        .map(|(h, _)| h)
}

/// Parameter along the line (point, dir) closest to the ray.
fn closest_on_axis(point: Vec3, dir: Vec3, ray: &Ray) -> Option<f32> {
    let w0 = point - ray.origin;
    let b = dir.dot(ray.dir);
    let denom = 1.0 - b * b;
    if denom.abs() < 1e-4 {
        return None;
    }
    let d = dir.dot(w0);
    let e = ray.dir.dot(w0);
    Some((b * e - d) / denom)
}

/// Where the pointer grabs the handle: an axis parameter (in x) or a point on the view plane.
fn grab_point(handle: Handle, start: Vec3, view: &View, ray: &Ray) -> Option<Vec3> {
    match handle {
        Handle::Axis(i) => closest_on_axis(start, AXES[i], ray).map(|t| Vec3::new(t, 0.0, 0.0)),
        Handle::Center => hit_plane(ray, (start, view_forward(view))),
        // Edge-on rings fall back to horizontal mouse motion (grab = ZERO).
        Handle::Ring(i) => Some(ring_plane_dir(start, AXES[i], view, ray).unwrap_or(Vec3::ZERO)),
    }
}

/// Direction from the center to where the ray meets the ring's plane.
fn ring_plane_dir(center: Vec3, axis: Vec3, view: &View, ray: &Ray) -> Option<Vec3> {
    let facing = (view.camera_pos - center).normalize_or_zero().dot(axis).abs();
    if facing < 0.12 {
        return None;
    }
    let hit = hit_plane(ray, (center, axis))?;
    (hit - center).try_normalize()
}

/// Handle viewport input. Returns true while the gizmo owns the pointer
/// (so the camera doesn't orbit and clicks don't change the selection).
pub fn handle_input(
    gizmo: &mut MoveGizmo,
    resp: &egui::Response,
    ui: &egui::Ui,
    ed: &mut Editor,
    rect: Rect,
    view: &View,
    pose_rig: Option<Entity>,
) -> bool {
    let hover = resp.hover_pos().and_then(|p| handle_at(ed, view, rect, p));
    gizmo.hovered = gizmo.drag.map(|d| d.handle).or(hover);

    let button = ed.controls.gestures.gizmo.egui();
    if resp.drag_started_by(button) {
        let origin = ui.input(|i| i.pointer.press_origin()).or(resp.interact_pointer_pos());
        if let (Some(pos), Some((entity, start))) = (origin, target(ed)) {
            if let Some(handle) = handle_at(ed, view, rect, pos) {
                let ray = screen_ray(view, rect, pos);
                if let Some(grab) = grab_point(handle, start, view, &ray) {
                    let start_rotation = world_rotation(ed, entity);
                    gizmo.drag = Some(Drag { handle, entity, start, grab, start_rotation, angle: 0.0, press: pos });
                }
            }
        }
    }

    let Some(drag) = gizmo.drag else {
        // A click on a handle keeps the selection.
        return hover.is_some() && resp.clicked_by(button);
    };
    if !resp.dragged_by(button) || !ed.world().is_alive(drag.entity) {
        gizmo.drag = None;
        return resp.drag_stopped();
    }
    if let (Handle::Ring(i), Some(pos)) = (drag.handle, resp.interact_pointer_pos()) {
        let ray = screen_ray(view, rect, pos);
        let axis = AXES[i];
        let angle = if drag.grab == Vec3::ZERO {
            // Edge-on: drag sideways to turn.
            (pos.x - drag.press.x).to_radians() * 0.6
        } else if let Some(now) = ring_plane_dir(drag.start, axis, view, &ray) {
            // Accumulate small steps so turns past 180° keep going.
            let step = axis.dot(drag.grab.cross(now)).atan2(drag.grab.dot(now));
            let d = gizmo.drag.as_mut().unwrap();
            d.grab = now;
            d.angle + step
        } else {
            drag.angle
        };
        gizmo.drag.as_mut().unwrap().angle = angle;
        let mut shown = angle.to_degrees();
        if ui.input(|i| ed.controls.gestures.snap_gizmo.held(i.modifiers)) {
            shown = (shown / ANGLE_SNAP_DEG).round() * ANGLE_SNAP_DEG;
        }
        let world = Quat::from_axis_angle(axis, shown.to_radians()) * drag.start_rotation;
        set_world_rotation(ed, drag.entity, world);
        return true;
    }
    if let Some(pos) = resp.interact_pointer_pos() {
        let ray = screen_ray(view, rect, pos);
        if let Some(now) = grab_point(drag.handle, drag.start, view, &ray) {
            let mut delta = match drag.handle {
                Handle::Axis(i) => AXES[i] * (now.x - drag.grab.x),
                Handle::Center | Handle::Ring(_) => now - drag.grab,
            };
            if ed.camera.two_d {
                delta.z = 0.0;
            }
            let mut world = drag.start + delta;
            if ui.input(|i| ed.controls.gestures.snap_gizmo.held(i.modifiers)) {
                world = (world / SNAP).round() * SNAP;
                // Only snap the axis being dragged.
                if let Handle::Axis(i) = drag.handle {
                    for k in 0..3 {
                        if k != i {
                            world[k] = drag.start[k];
                        }
                    }
                }
            }
            if let Some(rig) = pose_rig.filter(|_| ed.world().has::<kit::gamesys::RigVolume>(drag.entity)) {
                kit::rigg::move_control(&mut ed.app.world, rig, drag.entity, world);
            } else {
                set_world_position(ed, drag.entity, world);
            }
        }
    }
    true
}

fn parent_rotation(w: &kit::ecs::World, e: Entity) -> Quat {
    w.get::<Parent>(e)
        .and_then(|p| w.get::<GlobalTransform>(p.0).map(|g| g.0.to_scale_rotation_translation().1))
        .unwrap_or(Quat::IDENTITY)
}

fn world_rotation(ed: &Editor, e: Entity) -> Quat {
    let w = ed.world();
    parent_rotation(w, e) * w.get::<Transform>(e).map_or(Quat::IDENTITY, |t| t.rotation)
}

/// Turn an entity so its world rotation is `world`, respecting its parent.
fn set_world_rotation(ed: &mut Editor, e: Entity, world: Quat) {
    let w = &mut ed.app.world;
    let local = (parent_rotation(w, e).inverse() * world).normalize();
    if let Some(mut t) = w.get_mut::<Transform>(e) {
        t.rotation = local;
    }
}

/// Move an entity so its world position is `world`, respecting its parent.
fn set_world_position(ed: &mut Editor, e: Entity, world: Vec3) {
    let w = &mut ed.app.world;
    let parent_inv = w
        .get::<Parent>(e)
        .and_then(|p| w.get::<GlobalTransform>(p.0).map(|g| g.0.inverse()))
        .unwrap_or(kit::math::Mat4::IDENTITY);
    let local = parent_inv.transform_point3(world);
    if let Some(mut t) = w.get_mut::<Transform>(e) {
        t.translation = local;
    }
    // Keep the gizmo glued to the object this frame, before propagation runs.
    if let Some(mut g) = w.get_mut::<GlobalTransform>(e) {
        g.0.w_axis = world.extend(1.0);
    }
}

pub fn draw(painter: &egui::Painter, gizmo: &MoveGizmo, ed: &Editor, rect: Rect, view: &View) {
    let Some((_, origin)) = target(ed) else { return };
    let Some(o) = to_screen(view.view_proj(), rect, origin) else { return };
    draw_rings(painter, gizmo, ed, rect, view, origin);
    let mut segs = segments(ed, view, rect, origin);
    // Draw the hovered / dragged axis last so it sits on top.
    segs.sort_by_key(|&(i, ..)| gizmo.hovered == Some(Handle::Axis(i)));
    let dragging = gizmo.drag.is_some();
    let turning = matches!(gizmo.drag, Some(Drag { handle: Handle::Ring(_), .. }));
    for (i, a, b) in segs {
        if turning {
            break;
        }
        let hot = gizmo.hovered == Some(Handle::Axis(i));
        let mut color = axis_color(i);
        if dragging && !hot {
            color = color.gamma_multiply(0.35);
        }
        let width = if hot { 4.0 } else { 2.5 };
        let dir = (b - a).normalized();
        let side = egui::vec2(-dir.y, dir.x);
        let head = if hot { 15.0 } else { 13.0 };
        let base = b - dir * head;
        painter.line_segment([a, base], Stroke::new(width, color));
        painter.add(egui::Shape::convex_polygon(
            vec![b, base + side * head * 0.45, base - side * head * 0.45],
            color,
            Stroke::NONE,
        ));
        let label = ["X", "Y", "Z"][i];
        painter.text(
            b + dir * 10.0,
            egui::Align2::CENTER_CENTER,
            label,
            egui::FontId::proportional(if hot { 13.0 } else { 11.5 }),
            color,
        );
    }
    let hot = gizmo.hovered == Some(Handle::Center);
    let fill = if hot { ACCENT_HOVER() } else { pick(Color32::WHITE, Color32::from_rgb(0xee, 0xeb, 0xfb)) };
    painter.circle(
        o,
        if hot { CENTER_PX + 1.5 } else { CENTER_PX },
        fill,
        Stroke::new(1.5, pick(Color32::from_rgb(0x4a, 0x40, 0x66), Color32::from_rgb(0x20, 0x16, 0x32))),
    );
}

/// Screen positions of the arrows (for UI tests).
pub fn screen_handles(ed: &Editor, view: &View, rect: Rect) -> Option<Vec<(usize, Pos2, Pos2)>> {
    let (_, origin) = target(ed)?;
    Some(segments(ed, view, rect, origin))
}

/// A grabbable point on ring `i` (front-facing, away from the arrows), for UI tests.
#[cfg(test)]
pub fn ring_grab_point(ed: &Editor, view: &View, rect: Rect, i: usize) -> Option<Pos2> {
    let (_, origin) = target(ed)?;
    ring_points(view, rect, origin, i)
        .into_iter()
        .filter(|&(p, front)| front && handle_at(ed, view, rect, p) == Some(Handle::Ring(i)))
        .map(|(p, _)| p)
        .next()
}

fn draw_rings(painter: &egui::Painter, gizmo: &MoveGizmo, ed: &Editor, rect: Rect, view: &View, origin: Vec3) {
    let dragging = gizmo.drag.map(|d| d.handle);
    let mut rings: Vec<usize> = ring_axes(ed).to_vec();
    rings.sort_by_key(|&i| gizmo.hovered == Some(Handle::Ring(i)));
    for i in rings {
        let hot = gizmo.hovered == Some(Handle::Ring(i));
        if dragging.is_some() && !hot {
            continue; // only the ring being turned stays visible
        }
        let color = axis_color(i);
        let pts = ring_points(view, rect, origin, i);
        for w in pts.windows(2) {
            let front = w[0].1 && w[1].1;
            let (width, c) = match (hot, front) {
                (true, _) => (3.5, color),
                (false, true) => (2.0, color.gamma_multiply(0.85)),
                (false, false) => (1.2, color.gamma_multiply(0.3)),
            };
            painter.line_segment([w[0].0, w[1].0], Stroke::new(width, c));
        }
    }
    // While turning: a wedge from the start angle and the live angle.
    if let Some(Drag { handle: Handle::Ring(i), angle, .. }) = gizmo.drag {
        if let Some(o) = to_screen(view.view_proj(), rect, origin) {
            let deg = angle.to_degrees();
            let text = format!("{} {:+.0}°", ["X", "Y", "Z"][i], deg);
            let galley = painter.layout_no_wrap(text, egui::FontId::proportional(12.5), TEXT());
            let pos = o + egui::vec2(-galley.size().x / 2.0, -ARROW_PX - 34.0);
            let bg = Rect::from_min_size(pos, galley.size()).expand2(egui::vec2(8.0, 4.0));
            painter.rect_filled(bg, 6.0, DIALOG_BG());
            painter.rect_stroke(bg, 6.0, Stroke::new(1.0, axis_color(i)), egui::StrokeKind::Inside);
            painter.galley(pos, galley, TEXT());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closest_point_on_axis_follows_the_ray() {
        // A ray looking down -Z at x = 3 meets the X axis at t = 3.
        let ray = Ray { origin: Vec3::new(3.0, 0.0, 10.0), dir: Vec3::NEG_Z };
        let t = closest_on_axis(Vec3::ZERO, Vec3::X, &ray).unwrap();
        assert!((t - 3.0).abs() < 1e-4);
        // Parallel to the axis: no answer.
        let along = Ray { origin: Vec3::new(0.0, 1.0, 0.0), dir: Vec3::X };
        assert!(closest_on_axis(Vec3::ZERO, Vec3::X, &along).is_none());
    }
}
