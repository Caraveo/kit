//! The Animation timeline for Rigg clips: clip tabs, transport, a ruler with
//! a scrubber, and key diamonds. Keys store every volume's pose at a time.

use crate::state::Editor;
use crate::theme::*;
use egui::{Align2, Color32, CornerRadius, FontId, Pos2, RichText, Sense, Stroke, Ui, vec2};
use kit::ecs::Entity;
use kit::gamesys::{Rig, RigClip};

/// Timeline state (per editor session).
#[derive(Default)]
pub struct Timeline {
    pub open: bool,
    pub clip: Option<String>,
    pub time: f32,
    pub playing: bool,
    /// Key the pose automatically when a volume is moved.
    pub auto_key: bool,
    /// A pose change waiting to be auto-keyed (fingerprint of volume transforms).
    last_pose: u64,
    rig: Option<Entity>,
    new_clip: String,
}

fn pose_fingerprint(ed: &Editor, rig: Entity) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for v in kit::rigg::volumes_of(ed.world(), rig) {
        if let Some(t) = ed.world().get::<kit::scene::Transform>(v) {
            for x in kit::gamesys::rig::pack_transform(&t) {
                x.to_bits().hash(&mut h);
            }
        }
    }
    h.finish()
}

fn clip<'a>(r: &'a Rig, name: &Option<String>) -> Option<&'a RigClip> {
    name.as_ref().and_then(|n| r.clips.iter().find(|c| &c.name == n))
}

/// Draw the timeline for the active rig.
pub fn show(ui: &mut Ui, ed: &mut Editor, tl: &mut Timeline, rig: Entity) {
    if tl.rig != Some(rig) {
        tl.rig = Some(rig);
        tl.clip = None;
        tl.playing = false;
        tl.last_pose = 0;
    }
    let Some(r) = ed.world().get_cloned::<Rig>(rig) else { return };
    if tl.clip.as_ref().is_none_or(|c| !r.clips.iter().any(|x| &x.name == c)) {
        tl.clip = r.clips.first().map(|c| c.name.clone());
        tl.time = 0.0;
    }
    // Playback (editor preview, not the game).
    if tl.playing {
        if let Some(c) = clip(&r, &tl.clip) {
            let dt = ui.input(|i| i.stable_dt).min(0.1);
            tl.time += dt;
            if tl.time > c.length {
                if c.looping {
                    tl.time %= c.length.max(1e-3)
                } else {
                    tl.time = c.length;
                    tl.playing = false
                }
            }
            kit::rigg::pose_clip(&mut ed.app.world, rig, &c.name, tl.time);
            tl.last_pose = pose_fingerprint(ed, rig);
            ui.ctx().request_repaint();
        }
    }
    // Auto-key: a moved volume (not while dragging) is keyed at the current time.
    let fp = pose_fingerprint(ed, rig);
    let dragging = ui.input(|i| i.pointer.any_down());
    if tl.auto_key && !tl.playing && !dragging && fp != tl.last_pose && tl.last_pose != 0 {
        if let Some(name) = tl.clip.clone() {
            kit::rigg::key_pose(&mut ed.app.world, rig, &name, tl.time);
        }
    }
    if !dragging {
        tl.last_pose = fp;
    }

    // Header: title, clip tabs, new clip.
    ui.horizontal(|ui| {
        ui.label(RichText::new("ANIMATION").size(11.0).color(MUTED()));
        ui.add_space(8.0);
        for c in &r.clips {
            let on = tl.clip.as_deref() == Some(&c.name);
            let text =
                RichText::new(format!("▶ {}", c.name)).size(12.5).color(if on { TEXT_STRONG() } else { LABEL() });
            let b = egui::Button::new(text)
                .fill(if on { ACCENT().gamma_multiply(0.28) } else { Color32::TRANSPARENT })
                .stroke(Stroke::new(1.0, if on { ACCENT() } else { CARD_BORDER() }))
                .corner_radius(CornerRadius::same(8));
            if ui.add(b).clicked() && !on {
                tl.clip = Some(c.name.clone());
                tl.time = 0.0;
                tl.playing = false;
                kit::rigg::pose_clip(&mut ed.app.world, rig, &c.name, 0.0);
                tl.last_pose = pose_fingerprint(ed, rig);
            }
        }
        ui.add(egui::TextEdit::singleline(&mut tl.new_clip).hint_text("New clip").desired_width(90.0));
        let name = tl.new_clip.trim().to_owned();
        let free = !name.is_empty() && !r.clips.iter().any(|c| c.name == name);
        if ui.add_enabled(free, egui::Button::new("+")).on_hover_text("Add a clip (e.g. Idle, Walk, Jump)").clicked() {
            if let Some(mut rr) = ed.app.world.get_mut::<Rig>(rig) {
                rr.clips.push(RigClip { name: name.clone(), ..Default::default() });
            }
            tl.clip = Some(name);
            tl.time = 0.0;
            tl.new_clip.clear();
            // Start it from the current pose.
            kit::rigg::key_pose(&mut ed.app.world, rig, tl.clip.as_deref().unwrap(), 0.0);
        }
    });
    let Some(c) = clip(&r, &tl.clip).cloned() else {
        ui.add_space(12.0);
        ui.label(
            RichText::new(
                "No clips yet. Type a name (Idle, Walk, Jump…) and press + to start one from the current pose.",
            )
            .color(MUTED()),
        );
        return;
    };

    // Transport.
    ui.horizontal(|ui| {
        let jump = |tl: &mut Timeline, ed: &mut Editor, t: f32| {
            tl.time = t.clamp(0.0, c.length);
            tl.playing = false;
            kit::rigg::pose_clip(&mut ed.app.world, rig, &c.name, tl.time);
            tl.last_pose = pose_fingerprint(ed, rig);
        };
        let keys = kit::rigg::clip::key_times(&c);
        if ui.button("⏮").on_hover_text("Start").clicked() {
            jump(tl, ed, 0.0);
        }
        if ui.button("◀").on_hover_text("Previous key").clicked() {
            let t = keys.iter().rev().find(|k| **k < tl.time - 1e-3).copied().unwrap_or(0.0);
            jump(tl, ed, t);
        }
        if ui.button(if tl.playing { "⏸" } else { "▶" }).on_hover_text("Play / pause (preview)").clicked() {
            tl.playing = !tl.playing;
            if tl.playing && tl.time >= c.length {
                tl.time = 0.0;
            }
        }
        if ui.button("▶|").on_hover_text("Next key").clicked() {
            let t = keys.iter().find(|k| **k > tl.time + 1e-3).copied().unwrap_or(c.length);
            jump(tl, ed, t);
        }
        let mut looping = c.looping;
        if ui.toggle_value(&mut looping, "Loop").changed() {
            if let Some(mut rr) = ed.app.world.get_mut::<Rig>(rig) {
                if let Some(x) = rr.clips.iter_mut().find(|x| x.name == c.name) {
                    x.looping = looping;
                }
            }
        }
        ui.label(RichText::new(format!("{:06.3} / {:06.3}", tl.time, c.length)).monospace().color(TEXT_STRONG()));
        let mut length = c.length;
        ui.label(RichText::new("Length").size(11.0).color(MUTED()));
        if ui.add(egui::DragValue::new(&mut length).speed(0.05).range(0.1..=600.0).suffix(" s")).changed() {
            if let Some(mut rr) = ed.app.world.get_mut::<Rig>(rig) {
                if let Some(x) = rr.clips.iter_mut().find(|x| x.name == c.name) {
                    x.length = length;
                }
            }
        }
        ui.separator();
        if ui
            .button(RichText::new("◆ Key").color(Color32::from_rgb(255, 196, 80)))
            .on_hover_text("Key every volume's pose at the current time")
            .clicked()
        {
            kit::rigg::key_pose(&mut ed.app.world, rig, &c.name, tl.time);
        }
        ui.toggle_value(&mut tl.auto_key, "Auto-key").on_hover_text("Key automatically whenever you move a volume");
        if ui.button("Delete key").on_hover_text("Remove the keys at the current time").clicked() {
            if let Some(mut rr) = ed.app.world.get_mut::<Rig>(rig) {
                if let Some(x) = rr.clips.iter_mut().find(|x| x.name == c.name) {
                    kit::rigg::clip::remove_keys_at(x, tl.time);
                }
            }
        }
    });

    // Ruler, keys and scrubber.
    let (rect, resp) = ui.allocate_exact_size(
        vec2(ui.available_width(), (ui.available_height() - 4.0).clamp(54.0, 120.0)),
        Sense::click_and_drag(),
    );
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 8.0, FIELD_BG());
    let x_of = |t: f32| rect.left() + 12.0 + (t / c.length.max(1e-3)) * (rect.width() - 24.0);
    let t_of = |x: f32| ((x - rect.left() - 12.0) / (rect.width() - 24.0) * c.length).clamp(0.0, c.length);
    // Ticks every 0.25 s (labels every 0.5 s or so).
    let step = if c.length > 8.0 { 1.0 } else { 0.25 };
    let mut t = 0.0;
    let mut i = 0;
    while t <= c.length + 1e-4 {
        let x = x_of(t);
        let major = i % 2 == 0;
        p.line_segment(
            [Pos2::new(x, rect.top() + 4.0), Pos2::new(x, rect.top() + if major { 14.0 } else { 9.0 })],
            Stroke::new(1.0, MUTED()),
        );
        if major {
            p.text(
                Pos2::new(x + 3.0, rect.top() + 14.0),
                Align2::LEFT_TOP,
                format!("{:.2}", t),
                FontId::proportional(10.0),
                MUTED(),
            );
        }
        t += step;
        i += 1;
    }
    // The key track.
    let track_y = rect.bottom() - 18.0;
    p.line_segment(
        [Pos2::new(x_of(0.0), track_y), Pos2::new(x_of(c.length), track_y)],
        Stroke::new(2.0, Color32::from_rgb(64, 235, 204).gamma_multiply(0.6)),
    );
    for k in kit::rigg::clip::key_times(&c) {
        let center = Pos2::new(x_of(k), track_y);
        let s = 6.0;
        let pts = vec![center + vec2(0.0, -s), center + vec2(s, 0.0), center + vec2(0.0, s), center + vec2(-s, 0.0)];
        p.add(egui::Shape::convex_polygon(
            pts,
            Color32::from_rgb(255, 196, 80),
            Stroke::new(1.0, Color32::from_rgb(120, 70, 10)),
        ));
    }
    // Scrubbing.
    if let Some(pos) = resp.interact_pointer_pos().filter(|_| resp.dragged() || resp.clicked()) {
        // Snap to a nearby key.
        let mut t = t_of(pos.x);
        if let Some(k) = kit::rigg::clip::key_times(&c).into_iter().find(|k| (x_of(*k) - pos.x).abs() < 6.0) {
            t = k;
        }
        tl.time = t;
        tl.playing = false;
        kit::rigg::pose_clip(&mut ed.app.world, rig, &c.name, t);
        tl.last_pose = pose_fingerprint(ed, rig);
    }
    let x = x_of(tl.time);
    p.line_segment([Pos2::new(x, rect.top()), Pos2::new(x, rect.bottom())], Stroke::new(2.0, ACCENT()));
    p.add(egui::Shape::convex_polygon(
        vec![Pos2::new(x - 6.0, rect.top()), Pos2::new(x + 6.0, rect.top()), Pos2::new(x, rect.top() + 8.0)],
        ACCENT(),
        Stroke::NONE,
    ));
}
