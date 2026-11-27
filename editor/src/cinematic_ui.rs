//! The **🎬 Cinematic** menu at the right end of the Game View / Scene View /
//! Preview row, shown when the project has the Cinematics Circuit installed
//! and enabled: play, skip and stop cinematics, and make new ones and new
//! shots from the current Scene View.

use egui::{Layout, RichText, Ui};
use kit::ecs::Entity;
use kit::gamesys::{Camera, CinematicControl, CinematicRequest, ExtraData};
use kit::math::Vec3;
use kit::scene::{GlobalTransform, Name, Transform};

use crate::state::{Editor, Preset};
use crate::theme::*;
use crate::ui::mark_rect as mark;

const CIRCUIT: &str = "kit.cinematics";
const DIRECTOR: &str = "cinematic";
const SHOT: &str = "cinematic_shot";

/// Is the Cinematics Circuit installed and enabled in the open project?
pub fn available() -> bool {
    crate::circuit_ui::active().iter().any(|m| m.circuit.id == CIRCUIT)
}

/// Defaults for one of the Circuit's components (from its circuit.toml).
fn defaults(component: &str) -> serde_json::Value {
    crate::circuit_ui::active()
        .iter()
        .filter(|m| m.circuit.id == CIRCUIT)
        .flat_map(|m| m.components.iter())
        .find(|c| c.id == component)
        .map(|c| c.defaults())
        .unwrap_or_else(|| serde_json::json!({}))
}

/// Cinematics in the scene: (director, name).
fn cinematics(ed: &Editor) -> Vec<(Entity, String)> {
    let w = ed.world();
    let mut out: Vec<(Entity, String)> = w
        .query::<(ExtraData,)>()
        .into_iter()
        .filter_map(|e| {
            let d = w.get::<ExtraData>(e)?.0.get(DIRECTOR)?.clone();
            let name = d.get("name").and_then(|n| n.as_str()).filter(|n| !n.trim().is_empty()).map(str::to_owned);
            Some((e, name.unwrap_or_else(|| w.get::<Name>(e).map(|n| n.0.clone()).unwrap_or_default())))
        })
        .collect();
    out.sort_by(|a, b| a.1.to_lowercase().cmp(&b.1.to_lowercase()));
    out
}

fn shot_count(ed: &Editor, director: Entity) -> usize {
    let w = ed.world();
    kit::scene::descendants_and_self(w, director)
        .into_iter()
        .filter(|&e| e != director && w.get::<ExtraData>(e).is_some_and(|x| x.0.contains_key(SHOT)))
        .count()
}

fn request(ed: &mut Editor, r: CinematicRequest) {
    if let Some(mut c) = ed.app.world.try_resource_mut::<CinematicControl>() {
        c.requests.push(r);
    }
}

/// Start the game (if needed) and play a cinematic.
pub(crate) fn play(ed: &mut Editor, name: &str) {
    if !ed.playing {
        ed.play();
    }
    ed.settings.game_camera = true;
    request(ed, CinematicRequest::Play(name.to_owned()));
}

/// A new Empty with a Cinematic, and a first shot from the current view.
pub(crate) fn new_cinematic(ed: &mut Editor) -> Entity {
    let n = cinematics(ed).len() + 1;
    let name = if n == 1 { "Cinematic".to_owned() } else { format!("Cinematic {n}") };
    let director = ed.spawn(Preset::Empty, None);
    let mut d = defaults(DIRECTOR);
    d["name"] = serde_json::Value::String(name.clone());
    let w = &mut ed.app.world;
    if let Some(mut nm) = w.get_mut::<Name>(director) {
        nm.0 = name;
    }
    if let Some(mut t) = w.get_mut::<Transform>(director) {
        t.translation = Vec3::ZERO;
    }
    let mut x = w.get_cloned::<ExtraData>(director).unwrap_or_default();
    x.0.insert(DIRECTOR.into(), d);
    w.insert(director, x);
    kit::scene::propagate_transforms(w);
    add_shot(ed, director);
    ed.selected = Some(director);
    director
}

/// A shot camera inside `director`, framed like the Scene View right now.
pub(crate) fn add_shot(ed: &mut Editor, director: Entity) -> Entity {
    let n = shot_count(ed, director) + 1;
    let eye = ed.camera.eye();
    let target = ed.camera.target;
    let cam = ed.spawn(Preset::Camera, Some(director));
    let w = &mut ed.app.world;
    let mut world_t = Transform::from_xyz(eye.x, eye.y, eye.z);
    world_t.look_at(target, Vec3::Y);
    // Into the director's space.
    let parent = w.get::<GlobalTransform>(director).map(|g| g.0).unwrap_or_default();
    let local = parent.inverse() * world_t.matrix();
    let (scale, rot, pos) = local.to_scale_rotation_translation();
    if let Some(mut t) = w.get_mut::<Transform>(cam) {
        *t = Transform { translation: pos, rotation: rot.normalize(), scale };
    }
    // A shot never takes over the game's view by itself.
    if let Some(mut c) = w.get_mut::<Camera>(cam) {
        c.active = false;
    }
    if let Some(mut nm) = w.get_mut::<Name>(cam) {
        nm.0 = format!("Shot {n}");
    }
    let mut shot = defaults(SHOT);
    if n > 1 {
        shot["transition"] = serde_json::Value::String("blend".into());
    }
    let mut x = w.get_cloned::<ExtraData>(cam).unwrap_or_default();
    x.0.insert(SHOT.into(), shot);
    w.insert(cam, x);
    kit::scene::propagate_transforms(w);
    ed.selected = Some(cam);
    cam
}

/// The menu, right-aligned at the end of the view row. Call inside that row.
pub fn view_tab(ui: &mut Ui, ed: &mut Editor) {
    if !available() {
        return;
    }
    let playing_now = ed.world().try_resource::<CinematicControl>().and_then(|c| c.playing.clone());
    ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
        let label = match &playing_now {
            Some(name) => format!("🎬 {name}"),
            None => "🎬 Cinematic".to_owned(),
        };
        let menu = ui.menu_button(RichText::new(label).color(if playing_now.is_some() { ACCENT() } else { TEXT() }), |ui| {
            ui.set_min_width(230.0);
            if let Some(name) = &playing_now {
                ui.label(RichText::new(format!("Playing {name}")).size(11.0).color(MUTED()));
                if ui.button("⏭  Skip").clicked() {
                    request(ed, CinematicRequest::Skip);
                    ui.close();
                }
                if ui.button("■  Stop cinematic").clicked() {
                    request(ed, CinematicRequest::Stop);
                    ui.close();
                }
                ui.separator();
            }
            let list = cinematics(ed);
            if list.is_empty() {
                ui.label(RichText::new("No cinematics in this scene yet.").size(11.0).color(MUTED()));
            }
            for (director, name) in &list {
                let shots = shot_count(ed, *director);
                ui.horizontal(|ui| {
                    let play_btn = ui
                        .add_enabled(shots > 0, egui::Button::new(format!("▶  {name}")))
                        .on_hover_text(if shots > 0 { "Play it (starts the game if it isn't running)" } else { "Add a shot first" });
                    if play_btn.clicked() {
                        play(ed, name);
                        ui.close();
                    }
                    ui.label(RichText::new(format!("{shots} shot{}", if shots == 1 { "" } else { "s" })).size(11.0).color(MUTED()));
                });
                let add = ui
                    .add_enabled(!ed.playing, egui::Button::new(RichText::new("     + Shot from this view").size(11.5)))
                    .on_hover_text("A camera framed like the Scene View right now, as the next shot");
                if add.clicked() {
                    add_shot(ed, *director);
                    ed.settings.game_camera = false;
                    ui.close();
                }
            }
            ui.separator();
            let new = ui
                .add_enabled(!ed.playing, egui::Button::new("+ New cinematic"))
                .on_hover_text("An Empty with a Cinematic, and a first shot framed like the Scene View");
            mark(ui, "cinematic_new", new.rect);
            if new.clicked() {
                new_cinematic(ed);
                ed.settings.game_camera = false;
                ui.close();
            }
            ui.label(
                RichText::new("Tip: select a shot and press 5 to draw its camera move.").size(10.5).color(MUTED()),
            );
        });
        mark(ui, "cinematic_menu", menu.response.rect);
    });
}
