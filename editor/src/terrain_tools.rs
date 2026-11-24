//! Terrain sculpting in the viewport.

use crate::canvas::to_screen;
use crate::state::Editor;
use crate::theme::viewport::*;
use egui::{Pos2, Rect, Stroke};
use kit::ecs::Entity;
use kit::gamesys::{SculptTool, Terrain};
use kit::math::{Mat4, Vec2, Vec3};
use kit::render::View;
use kit::render::pick::{Ray, terrain_hit};
use kit::scene::GlobalTransform;

/// Sculpt stroke state.
#[derive(Default)]
pub struct Brush {
    /// Flatten target height (local), captured when the stroke starts.
    flatten_to: Option<f32>,
}

fn ray_at(view: &View, rect: Rect, pos: Pos2) -> Ray {
    let ndc =
        Vec2::new((pos.x - rect.left()) / rect.width() * 2.0 - 1.0, (pos.y - rect.top()) / rect.height() * 2.0 - 1.0);
    Ray::from_ndc(view.view_proj(), ndc)
}

/// The world point on the selected terrain under the pointer.
fn hover(ed: &Editor, rect: Rect, view: &View, pos: Pos2) -> Option<(Entity, Vec3, Mat4)> {
    let e = ed.selected?;
    let t = ed.world().get::<Terrain>(e)?;
    let g = ed.world().get::<GlobalTransform>(e)?.0;
    let (_, hit) = terrain_hit(ed.world(), e, &t, &ray_at(view, rect, pos))?;
    Some((e, hit, g))
}

/// Apply the brush while the left button is held. Returns true when sculpting.
pub fn handle_input(
    resp: &egui::Response,
    ui: &egui::Ui,
    ed: &mut Editor,
    brush: &mut Brush,
    rect: Rect,
    view: &View,
) -> bool {
    let Some(tool) = ed.settings.sculpt else { return false };
    let down = ui.input(|i| i.pointer.primary_down());
    if !down {
        brush.flatten_to = None;
        return false;
    }
    if !(resp.is_pointer_button_down_on() || resp.dragged_by(egui::PointerButton::Primary)) {
        return false;
    }
    let Some(pos) = ui.input(|i| i.pointer.interact_pos()) else { return false };
    let Some((e, hit, g)) = hover(ed, rect, view, pos) else { return true };
    let local = g.inverse().transform_point3(hit);
    let scale = g.to_scale_rotation_translation().0;
    let ctrl = ui.input(|i| i.modifiers.command);
    let tool = match (tool, ctrl) {
        (SculptTool::Raise, true) => SculptTool::Lower,
        (SculptTool::Lower, true) => SculptTool::Raise,
        (t, _) => t,
    };
    let dt = ui.input(|i| i.stable_dt).clamp(1.0 / 240.0, 0.1);
    let strength = ed.settings.brush_strength;
    let radius = ed.settings.brush_radius / scale.x.abs().max(1e-3);
    let mut t = ed.world().get_cloned::<Terrain>(e).unwrap();
    let target = *brush.flatten_to.get_or_insert(local.y);
    let amount = match tool {
        SculptTool::Raise | SculptTool::Lower => strength * radius.max(1.0) * 1.5 * dt,
        SculptTool::Smooth | SculptTool::Flatten => strength * 10.0 * dt,
    };
    t.sculpt(tool, Vec2::new(local.x, local.z), radius, amount, target);
    ed.app.world.insert(e, t);
    ui.ctx().request_repaint();
    true
}

/// Draw the brush circle on the terrain under the pointer.
pub fn draw_brush(painter: &egui::Painter, ui: &egui::Ui, ed: &Editor, _brush: &Brush, rect: Rect, view: &View) {
    let Some(pos) = ui.input(|i| i.pointer.hover_pos()).filter(|p| rect.contains(*p)) else { return };
    let Some((e, hit, g)) = hover(ed, rect, view, pos) else { return };
    let Some(t) = ed.world().get::<Terrain>(e) else { return };
    let inv = g.inverse();
    let center = inv.transform_point3(hit);
    let scale = g.to_scale_rotation_translation().0;
    let radius = ed.settings.brush_radius / scale.x.abs().max(1e-3);
    let vp = view.view_proj();
    for (r, alpha) in [(radius, 1.0), (radius * 0.5, 0.45)] {
        let pts: Vec<Pos2> = (0..=48)
            .filter_map(|i| {
                let a = i as f32 / 48.0 * std::f32::consts::TAU;
                let p = Vec2::new(center.x + a.cos() * r, center.z + a.sin() * r);
                let h = t.height_at(p).unwrap_or(center.y);
                to_screen(vp, rect, g.transform_point3(Vec3::new(p.x, h + 0.05, p.y)))
            })
            .collect();
        painter.add(egui::Shape::line(pts, Stroke::new(1.8, ACCENT().gamma_multiply(alpha))));
    }
    if let Some(c) = to_screen(vp, rect, hit) {
        painter.circle_filled(c, 2.5, ACCENT_HOVER());
    }
    ui.ctx().request_repaint();
}
