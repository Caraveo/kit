//! Editor gizmos: cameras and lights drawn as objects in the viewport.

use crate::canvas::to_screen;
use crate::state::Editor;
use crate::theme::viewport::*;
use egui::{Color32, Pos2, Rect, Stroke};
use kit::gamesys::{AudioSource, Camera, Light, Model, Projection, Sprite};
use kit::math::{Mat4, Vec3};
use kit::render::View;
use kit::scene::GlobalTransform;

fn line(p: &egui::Painter, vp: Mat4, rect: Rect, a: Vec3, b: Vec3, s: Stroke) {
    if let (Some(a), Some(b)) = (to_screen(vp, rect, a), to_screen(vp, rect, b)) {
        p.line_segment([a, b], s);
    }
}

pub fn draw(painter: &egui::Painter, ed: &Editor, rect: Rect, view: &View) {
    if ed.playing {
        return;
    }
    let w = ed.world();
    let vp = view.view_proj();

    // Cameras: a small body plus the view frustum.
    for e in w.query::<(Camera,)>() {
        let Some(g) = w.get::<GlobalTransform>(e).map(|g| g.0) else { continue };
        let cam = *w.get::<Camera>(e).unwrap();
        let (_, rot, pos) = g.to_scale_rotation_translation();
        let m = Mat4::from_rotation_translation(rot, pos);
        let pt = |v: Vec3| m.transform_point3(v);
        let selected = ed.selected == Some(e);
        let color = if selected {
            ACCENT_HOVER()
        } else if cam.active {
            pick(Color32::from_rgb(0x4a, 0x40, 0x66), Color32::from_rgb(0xcf, 0xc6, 0xe8))
        } else {
            MUTED()
        };
        let s = Stroke::new(if selected { 2.0 } else { 1.4 }, color);
        let aspect = 16.0 / 9.0;
        // Frustum out to a fixed preview distance.
        let (near_hw, near_hh, far_hw, far_hh, n, f) = match cam.projection {
            Projection::Perspective { fov_y, .. } => {
                let k = (fov_y.to_radians() * 0.5).tan();
                let (n, f) = (0.35, if selected { 3.0 } else { 1.4 });
                (k * n * aspect, k * n, k * f * aspect, k * f, n, f)
            }
            Projection::Orthographic { height, .. } => {
                let hh = (height * 0.5).min(if selected { 6.0 } else { 1.5 });
                (hh * aspect, hh, hh * aspect, hh, 0.35, if selected { 3.0 } else { 1.4 })
            }
        };
        let near = [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)]
            .map(|(x, y)| pt(Vec3::new(x * near_hw, y * near_hh, -n)));
        let far = [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)]
            .map(|(x, y)| pt(Vec3::new(x * far_hw, y * far_hh, -f)));
        let faint = Stroke::new(1.0, color.gamma_multiply(if selected { 0.7 } else { 0.45 }));
        for i in 0..4 {
            line(painter, vp, rect, near[i], near[(i + 1) % 4], s);
            line(painter, vp, rect, far[i], far[(i + 1) % 4], faint);
            line(painter, vp, rect, near[i], far[i], faint);
        }
        // Camera body (a little box behind the lens) and an "up" marker.
        let b = 0.18;
        let body: Vec<Vec3> = [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)]
            .iter()
            .map(|(x, y)| pt(Vec3::new(x * b * 1.3, y * b, 0.0)))
            .collect();
        for i in 0..4 {
            line(painter, vp, rect, body[i], body[(i + 1) % 4], s);
            line(painter, vp, rect, body[i], near[i], s);
        }
        let up0 = pt(Vec3::new(-near_hw * 0.5, near_hh * 1.15, -n));
        let up1 = pt(Vec3::new(near_hw * 0.5, near_hh * 1.15, -n));
        let up2 = pt(Vec3::new(0.0, near_hh * 1.7, -n));
        if let (Some(a), Some(b2), Some(c)) =
            (to_screen(vp, rect, up0), to_screen(vp, rect, up1), to_screen(vp, rect, up2))
        {
            painter.add(egui::Shape::convex_polygon(vec![a, b2, c], color.gamma_multiply(0.8), Stroke::NONE));
        }
        if let Some(c) = to_screen(vp, rect, pos) {
            painter.circle_filled(c, if selected { 4.0 } else { 3.0 }, color);
        }
    }

    // Lights: a sun with rays and a direction arrow.
    for e in w.query::<(Light,)>() {
        if w.has::<Model>(e) || w.has::<Sprite>(e) {
            continue;
        }
        let Some(g) = w.get::<GlobalTransform>(e).map(|g| g.0) else { continue };
        let pos = g.w_axis.truncate();
        let dir = g.transform_vector3(Vec3::NEG_Z).normalize_or(Vec3::NEG_Y);
        let selected = ed.selected == Some(e);
        let color = if selected {
            pick(Color32::from_rgb(0xc9, 0x86, 0x00), Color32::from_rgb(0xff, 0xe0, 0x8a))
        } else {
            pick(Color32::from_rgb(0xb0, 0x78, 0x10), Color32::from_rgb(0xff, 0xd0, 0x8a).gamma_multiply(0.7))
        };
        let Some(c) = to_screen(vp, rect, pos) else { continue };
        painter.circle_stroke(c, 7.0, Stroke::new(1.6, color));
        for i in 0..8 {
            let a = i as f32 / 8.0 * std::f32::consts::TAU;
            let d = egui::vec2(a.cos(), a.sin());
            painter.line_segment([c + d * 10.0, c + d * 14.0], Stroke::new(1.4, color));
        }
        let tip = pos + dir * if selected { 2.5 } else { 1.2 };
        if let Some(t) = to_screen(vp, rect, tip) {
            painter.line_segment([c, t], Stroke::new(1.4, color));
            let d = (t - c).normalized();
            let n = egui::vec2(-d.y, d.x);
            painter.add(egui::Shape::convex_polygon(
                vec![t, t - d * 8.0 + n * 4.0, t - d * 8.0 - n * 4.0],
                color,
                Stroke::NONE,
            ));
        }
        let _ = Pos2::ZERO;
    }

    // Sounds: a speaker marker; when selected, the proximity range.
    for e in w.query::<(AudioSource,)>() {
        let Some(g) = w.get::<GlobalTransform>(e).map(|g| g.0) else { continue };
        let source = w.get::<AudioSource>(e).unwrap().clone();
        let pos = g.w_axis.truncate();
        let selected = ed.selected == Some(e);
        let color = if selected { ACCENT_HOVER() } else { pick(Color32::from_rgb(0x4a, 0x40, 0x66), Color32::from_rgb(0xcf, 0xc6, 0xe8)) };
        let Some(c) = to_screen(vp, rect, pos) else { continue };
        if selected && source.spatial {
            // Screen-facing outlines of the min/max spheres plus their rings on the ground plane.
            let right = view.view.row(0).truncate().normalize_or_zero();
            for (radius, alpha, width) in [(source.min_distance, 0.9, 1.6), (source.max_distance, 0.45, 1.2)] {
                if let Some(edge) = to_screen(vp, rect, pos + right * radius) {
                    painter.circle_stroke(c, (edge - c).length(), Stroke::new(width, color.gamma_multiply(alpha)));
                }
                let ring: Vec<Pos2> = (0..=64)
                    .filter_map(|i| {
                        let a = i as f32 / 64.0 * std::f32::consts::TAU;
                        to_screen(vp, rect, pos + Vec3::new(a.cos(), 0.0, a.sin()) * radius)
                    })
                    .collect();
                if ring.len() > 2 {
                    painter.add(egui::Shape::dashed_line(&ring, Stroke::new(1.0, color.gamma_multiply(alpha * 0.8)), 6.0, 5.0));
                }
            }
        }
        if w.has::<Model>(e) || w.has::<Sprite>(e) || w.has::<Camera>(e) || w.has::<Light>(e) {
            // Other objects with a sound: a small badge beside them.
            painter.text(c + egui::vec2(14.0, -14.0), egui::Align2::CENTER_CENTER, "♪", egui::FontId::proportional(13.0), color);
            continue;
        }
        painter.circle_filled(c, 11.0, color.gamma_multiply(0.18));
        painter.circle_stroke(c, 11.0, Stroke::new(1.5, color));
        painter.text(c, egui::Align2::CENTER_CENTER, "♪", egui::FontId::proportional(15.0), color);
    }
}
