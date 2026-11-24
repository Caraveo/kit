//! Canvas mode: sketch movement paths in the viewport, plus overlays for paths
//! and collider outlines (drawn with egui on top of the 3D view).

use crate::state::Editor;
use crate::theme::viewport::*;
use egui::{Color32, Pos2, Rect, Stroke};
use kit::ecs::Entity;
use kit::gamesys::{Collider, ColliderShape, PathFollow};
use kit::math::{Mat4, Quat, Vec2, Vec3, Vec4Swizzles};
use kit::render::View;
use kit::render::pick::Ray;
use kit::scene::GlobalTransform;

/// An in-progress sketch or point drag.
#[derive(Default)]
pub struct Sketch {
    pub entity: Option<Entity>,
    /// Points of the stroke being drawn (the first is the anchor it continues from).
    pub points: Vec<Vec3>,
    /// Existing path points the stroke extends (empty when starting over).
    base: Vec<Vec3>,
    /// The drawing plane: a point on it and its normal.
    plane: (Vec3, Vec3),
    /// Ground mode on a terrain: the object's height above the ground.
    above_ground: Option<f32>,
    /// Index of a path point being dragged.
    dragging: Option<usize>,
}

/// Project a world point into the viewport rect (None when behind the camera).
pub fn to_screen(view_proj: Mat4, rect: Rect, p: Vec3) -> Option<Pos2> {
    let clip = view_proj * p.extend(1.0);
    if clip.w <= 1e-4 {
        return None;
    }
    let ndc = clip.xyz() / clip.w;
    if ndc.z < 0.0 || ndc.z > 1.0 {
        return None;
    }
    Some(Pos2::new(rect.left() + (ndc.x + 1.0) * 0.5 * rect.width(), rect.top() + (ndc.y + 1.0) * 0.5 * rect.height()))
}

pub fn screen_ray(view: &View, rect: Rect, pos: Pos2) -> Ray {
    let ndc =
        Vec2::new((pos.x - rect.left()) / rect.width() * 2.0 - 1.0, (pos.y - rect.top()) / rect.height() * 2.0 - 1.0);
    Ray::from_ndc(view.view_proj(), ndc)
}

/// Where a ray crosses a plane (point, normal).
pub fn hit_plane(ray: &Ray, plane: (Vec3, Vec3)) -> Option<Vec3> {
    let (p, n) = plane;
    let d = ray.dir.dot(n);
    if d.abs() < 1e-5 {
        return None;
    }
    let t = (p - ray.origin).dot(n) / d;
    (t > 0.0 && t < 10_000.0).then(|| ray.origin + ray.dir * t)
}

/// The camera's viewing direction (into the screen).
pub fn view_forward(view: &View) -> Vec3 {
    -view.view.row(2).truncate().normalize_or(Vec3::NEG_Z)
}

/// Camera right and up axes in world space (for drawing the guide grid).
fn view_axes(view: &View) -> (Vec3, Vec3) {
    (view.view.row(0).truncate().normalize_or(Vec3::X), view.view.row(1).truncate().normalize_or(Vec3::Y))
}

/// Height of the ground (any terrain) under a world (x, z).
fn ground_height(ed: &Editor, x: f32, z: f32) -> Option<f32> {
    let ray = Ray { origin: Vec3::new(x, 1.0e4, z), dir: Vec3::NEG_Y };
    terrain_point(ed, &ray).map(|p| p.y)
}

/// The first terrain point hit by a ray.
fn terrain_point(ed: &Editor, ray: &Ray) -> Option<Vec3> {
    let w = ed.world();
    let terrains = w.storage::<kit::gamesys::Terrain>()?;
    terrains
        .iter()
        .filter_map(|(e, t)| kit::render::pick::terrain_hit(w, e, t, ray))
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, p)| p)
}

/// Ramer–Douglas–Peucker simplification.
pub fn simplify(points: &[Vec3], epsilon: f32) -> Vec<Vec3> {
    if points.len() < 3 {
        return points.to_vec();
    }
    let (first, last) = (points[0], *points.last().unwrap());
    let line = last - first;
    let len = line.length();
    let (mut index, mut max) = (0, 0.0);
    for (i, p) in points.iter().enumerate().take(points.len() - 1).skip(1) {
        let d = if len < 1e-6 { p.distance(first) } else { line.cross(*p - first).length() / len };
        if d > max {
            index = i;
            max = d;
        }
    }
    if max > epsilon {
        let mut left = simplify(&points[..=index], epsilon);
        let right = simplify(&points[index..], epsilon);
        left.pop();
        left.extend(right);
        left
    } else {
        vec![first, last]
    }
}

fn entity_position(ed: &Editor, e: Entity) -> Option<Vec3> {
    ed.world().get::<GlobalTransform>(e).map(|g| g.translation())
}

/// Path point index under the pointer (within a few pixels).
fn point_under(ed: &Editor, e: Entity, rect: Rect, view: &View, pos: Pos2) -> Option<usize> {
    let path = ed.world().get::<PathFollow>(e)?;
    let vp = view.view_proj();
    path.points
        .iter()
        .enumerate()
        .filter_map(|(i, p)| to_screen(vp, rect, *p).map(|s| (i, s.distance(pos))))
        .filter(|(_, d)| *d < 9.0)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(i, _)| i)
}

/// Handle Canvas input: drag a point handle to move it, or drag elsewhere to draw.
/// Strokes are drawn on a flat sheet facing the camera through the end of the
/// path, and extend it; Shift starts over. Returns true when the input was used.
pub fn handle_input(
    resp: &egui::Response,
    ui: &egui::Ui,
    ed: &mut Editor,
    sketch: &mut Sketch,
    rect: Rect,
    view: &View,
) -> bool {
    let Some(sel) = ed.selected.filter(|e| ed.world().is_alive(*e)) else { return false };
    let ground_mode = ed.settings.canvas_ground && !ed.camera.two_d;
    let forward = view_forward(view);

    if resp.drag_started_by(egui::PointerButton::Primary) {
        let Some(start) = entity_position(ed, sel) else { return false };
        let press = ui.input(|i| i.pointer.press_origin()).or_else(|| resp.interact_pointer_pos());
        // Grab an existing point?
        if let Some(i) = press.and_then(|p| point_under(ed, sel, rect, view, p)) {
            let p = ed.world().get::<PathFollow>(sel).unwrap().points[i];
            *sketch = Sketch { entity: Some(sel), dragging: Some(i), plane: (p, forward), ..Default::default() };
            return true;
        }
        let start_over = ui.input(|i| i.modifiers.shift);
        let mut base = if start_over {
            Vec::new()
        } else {
            ed.world().get::<PathFollow>(sel).map(|p| p.points.clone()).unwrap_or_default()
        };
        if base.is_empty() {
            base.push(start);
        }
        let anchor = *base.last().unwrap();
        let (plane, above_ground) = if ground_mode {
            let above = ground_height(ed, anchor.x, anchor.z).map(|g| anchor.y - g);
            ((anchor, Vec3::Y), above)
        } else {
            ((anchor, forward), None)
        };
        *sketch = Sketch { entity: Some(sel), points: vec![anchor], base, plane, above_ground, dragging: None };
        return true;
    }
    if sketch.entity != Some(sel) {
        return false;
    }
    if resp.dragged_by(egui::PointerButton::Primary) {
        let Some(pos) = resp.interact_pointer_pos() else { return true };
        let ray = screen_ray(view, rect, pos);
        if let Some(i) = sketch.dragging {
            if let Some(p) = hit_plane(&ray, sketch.plane) {
                if let Some(mut path) = ed.app.world.get_mut::<PathFollow>(sel) {
                    if let Some(pt) = path.points.get_mut(i) {
                        *pt = p;
                    }
                }
            }
            return true;
        }
        let on_ground = sketch.above_ground.and_then(|off| terrain_point(ed, &ray).map(|p| p + Vec3::Y * off));
        if let Some(p) = on_ground.or_else(|| hit_plane(&ray, sketch.plane)) {
            let min_step =
                if ed.camera.two_d { ed.camera.ortho_height * 0.004 } else { ed.camera.distance * 0.004 }.max(0.02);
            if sketch.points.last().is_none_or(|l| l.distance(p) > min_step) {
                sketch.points.push(p);
            }
        }
        return true;
    }
    if resp.drag_stopped() {
        sketch.entity = None;
        if sketch.dragging.take().is_some() {
            return true;
        }
        let raw = std::mem::take(&mut sketch.points);
        let mut points = std::mem::take(&mut sketch.base);
        let eps =
            if ed.camera.two_d { ed.camera.ortho_height * 0.003 } else { ed.camera.distance * 0.003 }.clamp(0.01, 0.2);
        let stroke = simplify(&raw, eps);
        // The stroke's first point is the anchor (already the last base point).
        points.extend(stroke.into_iter().skip(1));
        if points.len() >= 2 {
            let w = &mut ed.app.world;
            let mut path = w.get_cloned::<PathFollow>(sel).unwrap_or_default();
            path.points = points;
            path.playing = true;
            let n = path.points.len();
            w.insert(sel, path);
            ed.notify(format!(
                "Path for {}: {n} points. Rotate the view and drag again to keep drawing. F5 to play.",
                ed.entity_label(sel)
            ));
        }
        return true;
    }
    false
}

fn polyline(painter: &egui::Painter, view_proj: Mat4, rect: Rect, pts: &[Vec3], stroke: Stroke) {
    let screen: Vec<Option<Pos2>> = pts.iter().map(|p| to_screen(view_proj, rect, *p)).collect();
    for w in screen.windows(2) {
        if let (Some(a), Some(b)) = (w[0], w[1]) {
            painter.line_segment([a, b], stroke);
        }
    }
}

fn arrow(painter: &egui::Painter, a: Pos2, b: Pos2, color: Color32) {
    let d = b - a;
    let len = d.length();
    if len < 12.0 {
        return;
    }
    let dir = d / len;
    let mid = a + d * 0.5;
    let n = egui::vec2(-dir.y, dir.x);
    let tip = mid + dir * 5.0;
    painter.add(egui::Shape::convex_polygon(
        vec![tip, mid - dir * 4.0 + n * 4.0, mid - dir * 4.0 - n * 4.0],
        color,
        Stroke::NONE,
    ));
}

/// Paths, collider outlines and the in-progress sketch.
pub fn draw_overlays(painter: &egui::Painter, ed: &Editor, sketch: &Sketch, rect: Rect, view: &View) {
    let vp = view.view_proj();
    let w = ed.world();
    let selected = ed.selected;

    // Paths: the followed curve, plus point handles on the selected path.
    let hover = painter.ctx().pointer_hover_pos();
    if let Some(paths) = w.storage::<PathFollow>() {
        for (e, path) in paths.iter() {
            if path.points.is_empty() || (sketch.entity == Some(e) && sketch.dragging.is_none()) {
                continue;
            }
            let is_sel = selected == Some(e);
            let color = if is_sel { ACCENT() } else { ACCENT().gamma_multiply(0.45) };
            let curve = path.curve();
            polyline(painter, vp, rect, &curve, Stroke::new(if is_sel { 2.5 } else { 1.5 }, color));
            let screen: Vec<Option<Pos2>> = curve.iter().map(|p| to_screen(vp, rect, *p)).collect();
            if is_sel {
                let step = (screen.len() / 12).max(1);
                for w2 in screen.windows(2).step_by(step) {
                    if let (Some(a), Some(b)) = (w2[0], w2[1]) {
                        arrow(painter, a, b, ACCENT());
                    }
                }
                if ed.settings.canvas {
                    for p in path.points.iter().filter_map(|p| to_screen(vp, rect, *p)) {
                        let hot = hover.is_some_and(|h| h.distance(p) < 9.0);
                        painter.circle_filled(
                            p,
                            if hot { 6.0 } else { 4.0 },
                            if hot { ACCENT_HOVER() } else { DIALOG_BG() },
                        );
                        painter.circle_stroke(p, if hot { 6.0 } else { 4.0 }, Stroke::new(1.5, ACCENT_HOVER()));
                    }
                }
            }
            if let Some(Some(end)) = screen.last() {
                painter.circle_stroke(*end, if is_sel { 6.0 } else { 4.0 }, Stroke::new(1.5, color));
            }
        }
    }

    // Live sketch, on a faint grid showing the drawing sheet.
    if !sketch.points.is_empty() {
        if sketch.above_ground.is_none() && sketch.dragging.is_none() {
            draw_sheet(painter, vp, rect, view, sketch.plane.0, ed);
        }
        let mut line = sketch.base.clone();
        line.extend(sketch.points.iter().skip(1));
        polyline(painter, vp, rect, &line, Stroke::new(3.0, ACCENT_HOVER()));
        if let Some(p) = sketch.points.first().and_then(|p| to_screen(vp, rect, *p)) {
            painter.circle_filled(p, 4.0, ACCENT_HOVER());
        }
    }

    // Collider outlines: the selection always, everything else when enabled.
    if let Some(colliders) = w.storage::<Collider>() {
        for (e, c) in colliders.iter() {
            let is_sel = selected == Some(e);
            if !is_sel && !ed.settings.colliders {
                continue;
            }
            let Some(g) = w.get::<GlobalTransform>(e).map(|g| g.0) else { continue };
            let color =
                if c.trigger {
                    pick(Color32::from_rgb(0xc0, 0x80, 0x00), Color32::from_rgb(0xff, 0xd0, 0x8a))
                } else {
                    pick(Color32::from_rgb(0x12, 0x9a, 0x62), Color32::from_rgb(0x8a, 0xf0, 0xc0))
                };
            let color = if is_sel { color } else { color.gamma_multiply(0.4) };
            collider_outline(painter, vp, rect, g, c, Stroke::new(if is_sel { 1.5 } else { 1.0 }, color));
        }
    }
}

/// A faint grid on the camera-facing drawing sheet around `center`.
fn draw_sheet(painter: &egui::Painter, vp: Mat4, rect: Rect, view: &View, center: Vec3, ed: &Editor) {
    let (right, up) = view_axes(view);
    let size = if ed.camera.two_d { ed.camera.ortho_height } else { center.distance(view.camera_pos).max(1.0) };
    let cell = size / 8.0;
    let n = 8;
    let stroke = Stroke::new(1.0, ACCENT().gamma_multiply(0.18));
    for i in -n..=n {
        let o = i as f32 * cell;
        let ext = n as f32 * cell;
        polyline(painter, vp, rect, &[center + right * o - up * ext, center + right * o + up * ext], stroke);
        polyline(painter, vp, rect, &[center + up * o - right * ext, center + up * o + right * ext], stroke);
    }
}

fn collider_outline(painter: &egui::Painter, vp: Mat4, rect: Rect, world: Mat4, c: &Collider, stroke: Stroke) {
    let (scale, rot, pos) = world.to_scale_rotation_translation();
    let s = scale.abs();
    let center = pos + rot * (c.offset * s);
    let pt = |local: Vec3| center + rot * local;
    let circle = |r: f32, y: f32, axis: usize| -> Vec<Vec3> {
        (0..=32)
            .map(|i| {
                let a = i as f32 / 32.0 * std::f32::consts::TAU;
                let (x, z) = (a.cos() * r, a.sin() * r);
                match axis {
                    0 => pt(Vec3::new(y, x, z)),
                    1 => pt(Vec3::new(x, y, z)),
                    _ => pt(Vec3::new(x, z, y)),
                }
            })
            .collect()
    };
    match c.shape {
        ColliderShape::Box { size } => {
            let h = size * s * 0.5;
            let corners: Vec<Vec3> = (0..8)
                .map(|i| {
                    pt(Vec3::new(
                        if i & 1 == 0 { -h.x } else { h.x },
                        if i & 2 == 0 { -h.y } else { h.y },
                        if i & 4 == 0 { -h.z } else { h.z },
                    ))
                })
                .collect();
            for (a, b) in
                [(0, 1), (2, 3), (4, 5), (6, 7), (0, 2), (1, 3), (4, 6), (5, 7), (0, 4), (1, 5), (2, 6), (3, 7)]
            {
                polyline(painter, vp, rect, &[corners[a], corners[b]], stroke);
            }
        }
        ColliderShape::Sphere { radius } => {
            let r = radius * s.max_element();
            for axis in 0..3 {
                polyline(painter, vp, rect, &circle(r, 0.0, axis), stroke);
            }
        }
        ColliderShape::Capsule { height, radius } | ColliderShape::Cylinder { height, radius } => {
            let r = radius * s.x.max(s.z);
            let half = height * s.y * 0.5;
            let is_capsule = matches!(c.shape, ColliderShape::Capsule { .. });
            let body = if is_capsule { (half - r).max(0.0) } else { half };
            for y in [-body, body] {
                polyline(painter, vp, rect, &circle(r, y, 1), stroke);
            }
            for (x, z) in [(r, 0.0), (-r, 0.0), (0.0, r), (0.0, -r)] {
                polyline(painter, vp, rect, &[pt(Vec3::new(x, -body, z)), pt(Vec3::new(x, body, z))], stroke);
            }
            if is_capsule {
                for sign in [-1.0f32, 1.0] {
                    for yaw in [0.0f32, std::f32::consts::FRAC_PI_2] {
                        let q = Quat::from_rotation_y(yaw);
                        let arc: Vec<Vec3> = (0..=16)
                            .map(|i| {
                                let a = i as f32 / 16.0 * std::f32::consts::PI;
                                pt(q * Vec3::new(a.cos() * r, sign * (body + a.sin() * r), 0.0))
                            })
                            .collect();
                        polyline(painter, vp, rect, &arc, stroke);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simplify_keeps_corners() {
        let mut pts = Vec::new();
        for i in 0..=20 {
            pts.push(Vec3::new(i as f32 * 0.1, 0.0, 0.0));
        }
        for i in 1..=20 {
            pts.push(Vec3::new(2.0, 0.0, i as f32 * 0.1));
        }
        let s = simplify(&pts, 0.01);
        assert_eq!(s.len(), 3);
        assert!(s[1].distance(Vec3::new(2.0, 0.0, 0.0)) < 1e-5);
    }
}
