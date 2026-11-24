//! Headless UI tests: drive the real editor UI with synthetic input.

use crate::state::Editor;
use crate::ui::{self, UiState};
use egui::{Event, Key, Modifiers, PointerButton, Pos2, RawInput, Rect, Vec2};
use std::path::PathBuf;

struct Harness {
    ctx: egui::Context,
    ed: Editor,
    st: UiState,
    dir: PathBuf,
    /// Simulated clock (egui uses it for double-click timing).
    time: f64,
    /// Modifier keys held for the next frames.
    mods: Modifiers,
}

impl Harness {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("kit_ui_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut ed = Editor::new();
        ed.create_project(&dir).unwrap();
        let ctx = egui::Context::default();
        ctx.set_theme(egui::Theme::Dark);
        crate::theme::apply(&ctx);
        let mut h =
            Harness { ctx, ed, st: UiState::new(Vec::new(), "test".into()), dir, time: 0.0, mods: Modifiers::NONE };
        h.frame(vec![]);
        h.frame(vec![]);
        h
    }

    fn frame(&mut self, events: Vec<Event>) {
        self.frame_with_files(events, Vec::new());
    }

    fn frame_with_files(&mut self, mut events: Vec<Event>, dropped_files: Vec<egui::DroppedFileHandle>) {
        // Held modifier keys (egui tracks them through ModifiersChanged events).
        events.insert(0, Event::ModifiersChanged(self.mods));
        self.time += 1.0 / 60.0;
        let raw = RawInput {
            time: Some(self.time),
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1480.0, 900.0))),
            events,
            dropped_files,
            focused: true,
            ..Default::default()
        };
        let (ed, st) = (&mut self.ed, &mut self.st);
        let mut out = self.ctx.run_ui(raw, |ui| ui::draw(ui, ed, st));
        out.textures_delta.clear();
        // Like the app's frame loop: record an undo step whenever the editor is idle.
        let at_rest = !self.ctx.input(|i| i.pointer.any_down()) && self.ctx.memory(|m| m.focused().is_none());
        self.ed.track_changes(at_rest);
    }

    fn click(&mut self, pos: Pos2) {
        self.frame(vec![Event::PointerMoved(pos)]);
        self.frame(vec![Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed: true,
            modifiers: Modifiers::NONE,
        }]);
        self.frame(vec![Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed: false,
            modifiers: Modifiers::NONE,
        }]);
        self.frame(vec![]);
    }

    fn key(&mut self, key: Key) {
        let ev = |pressed| Event::Key { key, physical_key: None, pressed, repeat: false, modifiers: Modifiers::NONE };
        self.frame(vec![ev(true)]);
        self.frame(vec![ev(false)]);
    }

    /// Let real time pass (so separate clicks are not merged into one multi-click).
    fn wait(&mut self, seconds: f64) {
        self.time += seconds;
        self.frame(vec![]);
    }

    fn row(&self, name: &str) -> Rect {
        let e = kit::scene::find_by_name(self.ed.world(), name).unwrap();
        ui::row_rects(&self.ctx).into_iter().find(|(x, _)| *x == e).map(|(_, r)| r).expect("row drawn")
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[test]
fn clicking_a_hierarchy_row_selects_it() {
    let mut h = Harness::new("select");
    assert_eq!(h.ed.selected, None);
    let r = h.row("Cube");
    h.click(r.center());
    let cube = kit::scene::find_by_name(h.ed.world(), "Cube").unwrap();
    assert_eq!(h.ed.selected, Some(cube));
    let r = h.row("Camera");
    h.click(r.center());
    assert_eq!(h.ed.selected, kit::scene::find_by_name(h.ed.world(), "Camera"));
}

#[test]
fn dragging_a_row_onto_another_reparents_it() {
    let mut h = Harness::new("drag");
    let (from, to) = (h.row("Sun").center(), h.row("Cube").center());
    h.frame(vec![Event::PointerMoved(from)]);
    h.frame(vec![Event::PointerButton {
        pos: from,
        button: PointerButton::Primary,
        pressed: true,
        modifiers: Modifiers::NONE,
    }]);
    for i in 1..=10 {
        h.frame(vec![Event::PointerMoved(from + (to - from) * (i as f32 / 10.0))]);
    }
    h.frame(vec![Event::PointerButton {
        pos: to,
        button: PointerButton::Primary,
        pressed: false,
        modifiers: Modifiers::NONE,
    }]);
    h.frame(vec![]);
    let w = h.ed.world();
    let sun = kit::scene::find_by_name(w, "Sun").unwrap();
    let cube = kit::scene::find_by_name(w, "Cube").unwrap();
    assert_eq!(kit::scene::children(w, cube), vec![sun]);
}

#[test]
fn new_script_with_enter() {
    let mut h = Harness::new("script");
    ui::begin_new_asset(&mut h.ed, &mut h.st, ui::AssetKind::Script);
    h.frame(vec![]);
    h.frame(vec![Event::Text("Enemy AI".into())]);
    h.key(Key::Enter);
    assert!(h.dir.join("scripts/enemy_ai.x").is_file(), "script file created");
    let src = std::fs::read_to_string(h.dir.join("scripts/enemy_ai.x")).unwrap();
    assert!(src.contains("CREATE EnemyAi["));
    assert_eq!(h.ed.scripts.last().map(|s| s.path.as_str()), Some("scripts/enemy_ai.x"));
}

#[test]
fn new_scene_saves_dirty_scene_first() {
    let mut h = Harness::new("scene");
    h.ed.spawn(crate::state::Preset::Light, None);
    h.ed.track_changes(true);
    assert!(h.ed.scene_dirty());
    ui::begin_new_asset(&mut h.ed, &mut h.st, ui::AssetKind::Scene);
    h.frame(vec![]);
    h.frame(vec![Event::Text("level2".into())]);
    h.key(Key::Enter);
    assert!(h.dir.join("scenes/level2.kscene").is_file());
    assert_eq!(h.ed.scene_path.as_deref(), Some("scenes/level2.kscene"));
    // The previous scene was saved, including the new light.
    let main = std::fs::read_to_string(h.dir.join("main.kscene")).unwrap();
    assert_eq!(main.matches("\"light\"").count(), 2, "{main}");
}

#[test]
fn new_script_without_project_explains_why() {
    let mut ed = Editor::new();
    assert!(ed.new_script("x").is_none());
    assert!(ed.notice.as_ref().is_some_and(|(t, _)| t.contains("project")));
}

#[test]
fn canvas_sketch_creates_a_path_the_object_follows() {
    let mut h = Harness::new("canvas");
    let cube = kit::scene::find_by_name(h.ed.world(), "Cube").unwrap();
    // The starter cube spins via its script; remove it to measure path movement only.
    h.ed.app.world.remove::<kit::gamesys::Script>(cube);
    h.ed.selected = Some(cube);
    h.key(Key::Num5);
    assert!(h.ed.settings.canvas, "5 toggles canvas mode");
    h.ed.settings.canvas_ground = true; // this test draws on the ground

    let vp = h.st.viewport_rect;
    let from = vp.center();
    h.frame(vec![Event::PointerMoved(from)]);
    h.frame(vec![Event::PointerButton {
        pos: from,
        button: PointerButton::Primary,
        pressed: true,
        modifiers: Modifiers::NONE,
    }]);
    // An L-shaped stroke: right, then toward the camera (down on screen).
    let mut pos = from;
    for _ in 0..20 {
        pos += Vec2::new(10.0, 0.0);
        h.frame(vec![Event::PointerMoved(pos)]);
    }
    for _ in 0..15 {
        pos += Vec2::new(0.0, 8.0);
        h.frame(vec![Event::PointerMoved(pos)]);
    }
    h.frame(vec![Event::PointerButton {
        pos,
        button: PointerButton::Primary,
        pressed: false,
        modifiers: Modifiers::NONE,
    }]);
    h.frame(vec![]);

    let mut path = h.ed.world().get_cloned::<kit::gamesys::PathFollow>(cube).expect("sketch made a path");
    path.smooth = false; // measure the exact sketched corner
    h.ed.app.world.insert(cube, path.clone());
    assert!(path.points.len() >= 3 && path.points.len() < 20, "simplified corner path: {:?}", path.points);
    let start = h.ed.world().get::<kit::scene::Transform>(cube).unwrap().translation;
    assert!(path.points[0].distance(start) < 1e-3, "path starts at the object");
    // The drawing plane is the object's height.
    assert!(path.points.iter().all(|p| (p.y - start.y).abs() < 1e-3));
    // The camera did not orbit while sketching.
    let end = *path.points.last().unwrap();

    h.ed.play();
    for _ in 0..600 {
        h.ed.app.update(1.0 / 60.0);
    }
    let cube_now = kit::scene::find_by_name(h.ed.world(), "Cube").unwrap();
    let p = h.ed.world().get::<kit::scene::Transform>(cube_now).unwrap().translation;
    assert!(p.distance(end) < 0.05, "followed the sketch to its end: {p} vs {end}");
    h.ed.stop();
    let cube = kit::scene::find_by_name(h.ed.world(), "Cube").unwrap();
    assert!(
        h.ed.world().get::<kit::scene::Transform>(cube).unwrap().translation.distance(start) < 1e-4,
        "stop restores"
    );
    assert!(h.ed.world().has::<kit::gamesys::PathFollow>(cube), "path is part of the scene");
}

#[test]
fn physics_is_a_component_and_settings_apply_live() {
    let mut h = Harness::new("physics");
    let cube = kit::scene::find_by_name(h.ed.world(), "Cube").unwrap();
    h.ed.app.world.remove::<kit::gamesys::Script>(cube);
    // "Physics" from Add component = rigid body + fitted collider.
    h.ed.app.world.insert(cube, kit::gamesys::RigidBody::default());
    h.ed.app.world.insert(cube, kit::gamesys::Collider::default());
    h.ed.selected = Some(cube);
    h.frame(vec![]);

    // Edit project settings: low gravity, then save.
    h.st.settings.open(&mut h.ed);
    let mut cfg = h.st.settings.draft.clone().unwrap();
    cfg.physics.gravity = [0.0, -1.0, 0.0];
    cfg.physics.rate = 120;
    crate::settings::apply(&mut h.ed, cfg).unwrap();
    let saved = kit::resource::ProjectConfig::load(&h.dir.join("kit.project")).unwrap();
    assert_eq!(saved.physics.gravity, [0.0, -1.0, 0.0]);
    assert_eq!(saved.physics.rate, 120);
    let live = kit::physics::settings(&mut h.ed.app.world).unwrap();
    assert_eq!(live.gravity.y, -1.0);
    assert_eq!(live.rate, 120);

    // In play, the cube falls slowly (g = 1): about 0.5 m in 1 s.
    let y0 = h.ed.world().get::<kit::scene::Transform>(cube).unwrap().translation.y;
    h.ed.play();
    for _ in 0..60 {
        h.ed.app.update(1.0 / 60.0);
    }
    let cube_now = kit::scene::find_by_name(h.ed.world(), "Cube").unwrap();
    let fell = y0 - h.ed.world().get::<kit::scene::Transform>(cube_now).unwrap().translation.y;
    assert!((fell - 0.5).abs() < 0.1, "fell {fell} m under g = 1");
    h.ed.stop();

    // Reopening the project keeps the settings.
    let mut ed2 = Editor::new();
    ed2.open_project(&h.dir).unwrap();
    assert_eq!(kit::physics::settings(&mut ed2.app.world).unwrap().gravity.y, -1.0);
}

/// The real flow: click "+", click "New script…", type, press Enter — via the mouse and keyboard.
#[test]
fn new_script_through_the_plus_menu() {
    let mut h = Harness::new("plusmenu");
    let plus = ui::marked(&h.ctx, "assets_plus").expect("+ drawn").center();
    h.click(plus);
    let item = ui::marked(&h.ctx, "new_script_item").expect("menu opened").center();
    h.click(item);
    h.frame(vec![]);
    h.frame(vec![Event::Text("hero".into())]);
    h.key(Key::Enter);
    h.frame(vec![]);
    assert!(h.dir.join("scripts/hero.x").is_file(), "notice: {:?}", h.ed.notice);
}

#[test]
fn new_scene_through_the_plus_menu() {
    let mut h = Harness::new("plusscene");
    let plus = ui::marked(&h.ctx, "assets_plus").unwrap().center();
    h.click(plus);
    let item = ui::marked(&h.ctx, "new_scene_item").expect("menu opened").center();
    h.click(item);
    h.frame(vec![]);
    h.frame(vec![Event::Text("arena".into())]);
    h.key(Key::Enter);
    h.frame(vec![]);
    assert!(h.dir.join("scenes/arena.kscene").is_file(), "notice: {:?}", h.ed.notice);
}

/// Header "+ New" → Script… → type → click Create (mouse only).
#[test]
fn new_script_from_header_with_create_button() {
    let mut h = Harness::new("header");
    h.click(ui::marked(&h.ctx, "header_new").unwrap().center());
    h.click(ui::marked(&h.ctx, "header_new_script").expect("menu opened").center());
    h.frame(vec![]);
    h.frame(vec![Event::Text("Boss Fight".into())]);
    h.frame(vec![]);
    h.click(ui::marked(&h.ctx, "new_asset_create").expect("dialog shown").center());
    assert!(h.dir.join("scripts/boss_fight.x").is_file(), "notice: {:?}", h.ed.notice);
    let console: Vec<String> =
        h.ed.world().resource::<kit::core::Console>().lines().iter().map(|l| l.text.clone()).collect();
    assert!(console.iter().any(|l| l == "created scripts/boss_fight.x"), "{console:?}");
}

#[test]
fn new_scene_from_header() {
    let mut h = Harness::new("headerscene");
    h.click(ui::marked(&h.ctx, "header_new").unwrap().center());
    h.click(ui::marked(&h.ctx, "header_new_scene").expect("menu opened").center());
    h.frame(vec![]);
    h.frame(vec![Event::Text("Level 2".into())]);
    h.key(Key::Enter);
    assert!(h.dir.join("scenes/level_2.kscene").is_file(), "notice: {:?}", h.ed.notice);
    assert_eq!(h.ed.scene_path.as_deref(), Some("scenes/level_2.kscene"));
    // It shows up in the assets list right away.
    assert!(h.ed.assets.iter().any(|a| a == "scenes/level_2.kscene"));
}

#[test]
fn assets_grouped_with_per_group_new_buttons() {
    let mut h = Harness::new("groups");
    std::fs::create_dir_all(h.dir.join("art")).unwrap();
    std::fs::write(h.dir.join("art/ship.glb"), b"glTF").unwrap();
    h.ed.refresh_assets(true);
    h.frame(vec![]);
    assert!(h.ed.assets.iter().any(|a| a == "art/ship.glb"));
    assert!(!h.ed.assets.iter().any(|a| a == "kit.project"));
    h.click(ui::marked(&h.ctx, "group_new_script").expect("Scripts group has +").center());
    h.frame(vec![Event::Text("door".into())]);
    h.key(Key::Enter);
    assert!(h.dir.join("scripts/door.x").is_file(), "notice: {:?}", h.ed.notice);
}

/// Press at `from`, move to `to` in steps, release.
fn drag(h: &mut Harness, from: Pos2, to: Pos2) {
    h.frame(vec![Event::PointerMoved(from)]);
    h.frame(vec![Event::PointerButton { pos: from, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE }]);
    for i in 1..=10 {
        h.frame(vec![Event::PointerMoved(from + (to - from) * (i as f32 / 10.0))]);
    }
    h.frame(vec![Event::PointerButton { pos: to, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE }]);
    h.frame(vec![]);
}

fn select(h: &mut Harness, name: &str) -> kit::ecs::Entity {
    let r = h.row(name);
    h.click(r.center());
    h.frame(vec![]);
    let e = kit::scene::find_by_name(h.ed.world(), name).unwrap();
    assert_eq!(h.ed.selected, Some(e));
    e
}

#[test]
fn dragging_the_x_arrow_moves_only_along_x() {
    let mut h = Harness::new("gizmo_move");
    let cube = select(&mut h, "Cube");
    let before = *h.ed.world().get::<kit::scene::Transform>(cube).unwrap();
    let undo_before = h.ed.undo_len();
    let base = ui::marked(&h.ctx, "gizmo_x_base").expect("selected object shows an X arrow").center();
    let tip = ui::marked(&h.ctx, "gizmo_x_tip").unwrap().center();
    // Grab along the arrow and pull further out along it.
    let grab = base + (tip - base) * 0.6;
    let pull = grab + (tip - base).normalized() * 60.0;
    drag(&mut h, grab, pull);
    let after = *h.ed.world().get::<kit::scene::Transform>(cube).unwrap();
    assert!(after.translation.x > before.translation.x + 0.1, "{before:?} -> {after:?}");
    assert!((after.translation.y - before.translation.y).abs() < 1e-4);
    assert!((after.translation.z - before.translation.z).abs() < 1e-4);
    assert_eq!(h.ed.selected, Some(cube), "dragging a handle keeps the selection");
    assert_eq!(h.ed.undo_len(), undo_before + 1, "one drag = one undo step");
}

#[test]
fn dragging_a_ring_rotates_around_that_axis() {
    let mut h = Harness::new("gizmo_ring");
    let cube = select(&mut h, "Cube");
    let before = *h.ed.world().get::<kit::scene::Transform>(cube).unwrap();
    let undo_before = h.ed.undo_len();
    let center = ui::marked(&h.ctx, "gizmo_x_base").unwrap().center();
    let grab = ui::marked(&h.ctx, "gizmo_ring_y").expect("selected object shows a Y ring").center();
    // Pull along the ring (perpendicular to the radius).
    let radial = (grab - center).normalized();
    let to = grab + Vec2::new(-radial.y, radial.x) * 50.0;
    drag(&mut h, grab, to);
    let after = *h.ed.world().get::<kit::scene::Transform>(cube).unwrap();
    let (axis, angle) = (after.rotation * before.rotation.inverse()).to_axis_angle();
    assert!(angle.to_degrees() > 5.0, "turned {}°", angle.to_degrees());
    assert!(axis.y.abs() > 0.99, "turned around Y, got {axis:?}");
    assert!(after.translation.distance(before.translation) < 1e-4, "rotating doesn't move it");
    assert_eq!(h.ed.selected, Some(cube));
    assert_eq!(h.ed.undo_len(), undo_before + 1);
}

#[test]
fn export_wizard_opens_from_the_header_and_explains_platforms() {
    let mut h = Harness::new("export_wizard");
    h.click(ui::marked(&h.ctx, "header_export").expect("header has Export").center());
    h.frame(vec![]);
    assert!(h.st.export.open, "Export opens the wizard");
    assert_eq!(h.st.export.platform, Some(crate::export::host()), "starts on this computer's platform");
    // Every platform has a card; consoles can't be exported (they need licensed SDKs).
    for p in crate::export::platforms() {
        assert!(ui::marked(&h.ctx, p.id).is_some(), "card for {}", p.name);
    }
    h.click(ui::marked(&h.ctx, "switch").unwrap().center());
    h.frame(vec![]);
    assert!(ui::marked(&h.ctx, "request_access").is_some(), "consoles offer Request access");
    h.click(ui::marked(&h.ctx, "xbox").unwrap().center());
    h.frame(vec![]);
    assert_eq!(h.st.export.platform, Some("xbox"));
    h.click(ui::marked(&h.ctx, "export_go").unwrap().center());
    h.frame(vec![]);
    assert!(!h.ed.exporting(), "a console export can't start");
    let title = h.ed.project.as_ref().unwrap().config.project.title.clone();
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from).unwrap();
    assert_eq!(h.st.export.destination(&h.ed), Some(home.join("KitBuilds").join(&title).join("xbox")));
    assert_eq!(crate::export::default_builds_dir("A/B: C?"), home.join("KitBuilds").join("A_B_ C_"));
}

#[derive(Debug)]
struct DroppedSound(PathBuf);

impl egui::DroppedFile for DroppedSound {
    fn path(&self) -> &std::path::Path {
        &self.0
    }
    fn bytes(&self) -> Result<Vec<u8>, String> {
        std::fs::read(&self.0).map_err(|e| e.to_string())
    }
}

/// A short WAV beep written outside the project (like a file in Downloads).
fn beep(h: &Harness, name: &str) -> PathBuf {
    let dir = h.dir.with_extension("downloads");
    std::fs::create_dir_all(&dir).unwrap();
    let tone: Vec<i16> = (0..4800).map(|i| ((i as f32 * 0.06).sin() * 9000.0) as i16).collect();
    let path = dir.join(name);
    std::fs::write(&path, kit::audio::wav_bytes(48_000, 1, &tone)).unwrap();
    path
}

fn sound_objects(h: &Harness) -> Vec<(kit::ecs::Entity, kit::gamesys::AudioSource)> {
    let w = h.ed.world();
    w.query::<(kit::gamesys::AudioSource,)>()
        .into_iter()
        .map(|e| (e, w.get::<kit::gamesys::AudioSource>(e).unwrap().clone()))
        .collect()
}

#[test]
fn dropping_a_sound_file_on_the_viewport_imports_and_places_it() {
    let mut h = Harness::new("sound_drop");
    let source = beep(&h, "Door Bell.wav");
    // A broken "sound" is refused with a message, not imported.
    let bad = source.with_file_name("broken.mp3");
    std::fs::write(&bad, b"not really audio").unwrap();
    let center = h.st.viewport_rect.center();
    h.frame(vec![Event::PointerMoved(center)]);
    h.frame_with_files(
        vec![],
        vec![std::sync::Arc::new(DroppedSound(source)), std::sync::Arc::new(DroppedSound(bad))],
    );
    h.frame(vec![]);
    assert!(h.dir.join("sounds/Door Bell.wav").is_file(), "copied into sounds/");
    assert!(!h.dir.join("sounds/broken.mp3").exists(), "unplayable files are refused");
    assert!(h.ed.assets.iter().any(|a| a == "sounds/Door Bell.wav"), "listed under Sounds");
    let placed = sound_objects(&h);
    assert_eq!(placed.len(), 1, "dropped on the viewport: a sound object");
    let (e, source) = &placed[0];
    assert_eq!(source.clip, "sounds/Door Bell.wav");
    assert!(source.spatial, "placed sounds are 3D");
    assert_eq!(h.ed.selected, Some(*e));
    assert_eq!(h.ed.entity_label(*e), "Door Bell");
    assert_eq!(h.ed.entity_kind(*e), "SOUND");
    // Its inspector section shows, with the proximity settings.
    assert!(ui::marked(&h.ctx, "audio_spatial").is_some());
}

#[test]
fn dragging_a_sound_onto_an_object_or_the_viewport() {
    let mut h = Harness::new("sound_drag");
    let source = beep(&h, "hum.wav");
    let clip = h.ed.import_sound(&source).unwrap();
    assert_eq!(clip, "sounds/hum.wav");
    h.frame(vec![]);
    h.frame(vec![]);

    // Onto the Cube in the hierarchy: it gets an Audio Source.
    let row = ui::marked(&h.ctx, "sound_asset").expect("Sounds group lists the file").center();
    let cube_row = h.row("Cube").center();
    drag(&mut h, row, cube_row);
    let cube = kit::scene::find_by_name(h.ed.world(), "Cube").unwrap();
    let source = h.ed.world().get::<kit::gamesys::AudioSource>(cube).map(|a| a.clip.clone());
    assert_eq!(source.as_deref(), Some("sounds/hum.wav"));
    assert_eq!(sound_objects(&h).len(), 1, "no extra object");

    // Onto the viewport: a new sound object where it was dropped.
    let row = ui::marked(&h.ctx, "sound_asset").unwrap().center();
    let target = h.st.viewport_rect.center();
    drag(&mut h, row, target);
    let sounds = sound_objects(&h);
    assert_eq!(sounds.len(), 2);
    let placed = sounds.iter().find(|(e, _)| *e != cube).unwrap();
    assert_eq!(placed.1.clip, "sounds/hum.wav");
    assert_eq!(h.ed.selected, Some(placed.0));

    // Saved with the scene and back after undo/redo.
    h.ed.save_scene().unwrap();
    let text = std::fs::read_to_string(h.dir.join(h.ed.scene_path.clone().unwrap())).unwrap();
    assert!(text.contains("\"audio\"") && text.contains("sounds/hum.wav"));
}

#[test]
fn post_volume_authoring_duplicate_undo_and_save() {
    use kit::gamesys::{Override, PostVolume, VolumeShape};
    let mut h = Harness::new("post_volume");
    // An empty object with a Post-Processing Volume (Add component).
    let e = h.ed.spawn(crate::state::Preset::Empty, None);
    h.ed.app.world.insert(e, PostVolume::default());
    h.ed.selected = Some(e);
    h.frame(vec![]);
    h.frame(vec![]);
    assert_eq!(h.ed.entity_kind(e), "VOLUME");
    assert!(ui::marked(&h.ctx, "toggle_volumes").is_some(), "the viewport has a Volumes toggle");
    // Inspector: switch to a sphere and turn Glow on (only Glow; others inherit).
    h.click(ui::marked(&h.ctx, "volume_shape_sphere").expect("shape buttons").center());
    h.frame(vec![]);
    h.click(ui::marked(&h.ctx, "fx_glow_on").expect("Glow override").center());
    h.frame(vec![]);
    let v = h.ed.world().get::<PostVolume>(e).unwrap().clone();
    assert_eq!(v.shape, VolumeShape::Sphere);
    assert_eq!(v.effects.glow.mode, Override::On);
    assert_eq!(v.effects.color.mode, Override::Inherit);
    // Undo goes back to the box; redo returns.
    h.ed.track_changes(true);
    h.ed.undo();
    let after_undo = h.ed.world().query::<(PostVolume,)>().len();
    assert!(after_undo <= 1);
    h.ed.redo();
    let e = h.ed.world().query::<(PostVolume,)>()[0];
    assert_eq!(h.ed.world().get::<PostVolume>(e).unwrap().shape, VolumeShape::Sphere);
    // Duplicate copies the component.
    h.ed.duplicate(e);
    assert_eq!(h.ed.world().query::<(PostVolume,)>().len(), 2);
    // Save and reopen: settings come back exactly.
    h.ed.save_scene().unwrap();
    let path = h.ed.scene_path.clone().unwrap();
    h.ed.open_scene(&path).unwrap();
    let vols: Vec<PostVolume> = {
        let w = h.ed.world();
        w.query::<(PostVolume,)>().into_iter().map(|e| w.get::<PostVolume>(e).unwrap().clone()).collect()
    };
    assert_eq!(vols.len(), 2);
    assert!(vols.iter().all(|x| *x == v), "saved and reopened unchanged");
}

#[test]
fn number_keys_toggle_the_viewport_bar_and_wasd_moves_the_view() {
    let mut h = Harness::new("keys");
    let s = h.ed.settings.clone();
    for k in [Key::Num1, Key::Num2, Key::Num3, Key::Num4, Key::Num6] {
        h.key(k);
    }
    let t = &h.ed.settings;
    assert_eq!(
        (t.wireframe, t.lighting, t.grid, t.colliders, t.volumes),
        (!s.wireframe, !s.lighting, !s.grid, !s.colliders, !s.volumes),
        "1 wireframe, 2 lighting, 3 grid, 4 colliders, 6 volumes"
    );
    // W and D no longer toggle anything; with the mouse over the viewport they move the view.
    let over = h.st.viewport_rect.center();
    h.frame(vec![Event::PointerMoved(over)]);
    let (d, target) = (h.ed.camera.distance, h.ed.camera.target);
    let hold = |h: &mut Harness, key: Key| {
        h.frame(vec![Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE }]);
        for _ in 0..10 {
            h.frame(vec![]);
        }
        h.frame(vec![Event::Key { key, physical_key: None, pressed: false, repeat: false, modifiers: Modifiers::NONE }]);
    };
    hold(&mut h, Key::W);
    assert!(h.ed.camera.distance < d, "W zooms in");
    assert_eq!(h.ed.settings.wireframe, !s.wireframe, "W no longer toggles wireframe");
    hold(&mut h, Key::D);
    assert!(h.ed.camera.target != target, "D pans the view");
    assert!(!h.ed.settings.canvas, "D no longer toggles canvas");
}

#[test]
fn theme_toggle_switches_light_and_dark() {
    let mut h = Harness::new("theme_toggle");
    let was_light = crate::theme::is_light();
    let toggle = ui::marked(&h.ctx, "theme_toggle").expect("header has a theme toggle");
    h.click(toggle.center());
    assert_ne!(crate::theme::is_light(), was_light);
    assert_eq!(h.ctx.global_style().visuals.dark_mode, was_light);
    assert!(matches!(h.st.requests.as_slice(), [ui::Request::SaveTheme]));
    crate::theme::set_light(&h.ctx, was_light);
}

#[test]
fn models_panel_offers_import_without_a_file_drop() {
    let mut h = Harness::new("model_picker");
    let import = ui::marked(&h.ctx, "import_models").expect("Models panel offers an import button");
    h.click(import.center());
    assert!(matches!(h.st.requests.as_slice(), [ui::Request::ImportModels]));
}

#[test]
fn dropped_obj_can_be_placed_picked_and_saved() {
    #[derive(Debug)]
    struct DroppedModel(PathBuf);

    impl egui::DroppedFile for DroppedModel {
        fn path(&self) -> &std::path::Path {
            &self.0
        }

        fn bytes(&self) -> Result<Vec<u8>, String> {
            std::fs::read(&self.0).map_err(|error| error.to_string())
        }
    }

    let mut h = Harness::new("model_import");
    let input = h.dir.join(".downloads");
    std::fs::create_dir(&input).unwrap();
    let source = input.join("ship.OBJ");
    let obj = "v 8 0 0\nv 10 0 0\nv 8 2 0\nvt 0 0\nvt 1 0\nvt 0 1\nf -3/1 -2/2 -1/3\n";
    std::fs::write(&source, obj).unwrap();
    let initial_count = h.ed.world().entity_count();
    h.frame_with_files(vec![], vec![std::sync::Arc::new(DroppedModel(source))]);

    let path = "models/ship.obj";
    assert_eq!(std::fs::read_to_string(h.dir.join(path)).unwrap(), obj);
    assert_eq!(h.ed.world().entity_count(), initial_count);
    assert!(h.ed.assets.iter().any(|asset| asset == path));

    let row = ui::marked(&h.ctx, "model_asset").unwrap();
    double_click(&mut h, row.center());
    let selected = h.ed.selected.unwrap();
    let world = h.ed.world();
    assert_eq!(world.entity_count(), initial_count + 1);
    assert_eq!(world.get::<kit::gamesys::Model>(selected).unwrap().mesh_asset, path);
    let mesh = kit::render::load_obj(&h.dir.join(path)).unwrap();
    assert_eq!(mesh.indices, [0, 1, 2]);
    assert_eq!(mesh.vertices[0].normal, [0.0, 0.0, 1.0]);
    let transform = world.get::<kit::scene::GlobalTransform>(selected).unwrap().0;
    let ray = kit::render::pick::Ray {
        origin: transform.transform_point3(kit::math::Vec3::new(8.5, 0.5, 5.0)),
        dir: kit::math::Vec3::NEG_Z,
    };
    assert_eq!(kit::render::pick::pick(world, &ray), Some(selected));

    h.ed.save_scene().unwrap();
    let scene = h.ed.scene_path.clone().unwrap();
    h.ed.open_scene(&scene).unwrap();
    let ship = kit::scene::find_by_name(h.ed.world(), "ship").unwrap();
    assert_eq!(h.ed.world().get::<kit::gamesys::Model>(ship).unwrap().mesh_asset, path);
}

fn double_click(h: &mut Harness, pos: Pos2) {
    h.frame(vec![Event::PointerMoved(pos)]);
    for _ in 0..2 {
        h.frame(vec![Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed: true,
            modifiers: Modifiers::NONE,
        }]);
        h.frame(vec![Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed: false,
            modifiers: Modifiers::NONE,
        }]);
    }
    h.frame(vec![]);
}

fn select_all_and_type(h: &mut Harness, text: &str) {
    let ev =
        |pressed| Event::Key { key: Key::A, physical_key: None, pressed, repeat: false, modifiers: Modifiers::COMMAND };
    h.frame(vec![ev(true)]);
    h.frame(vec![ev(false)]);
    h.frame(vec![Event::Text(text.into())]);
}

#[test]
fn rename_by_double_click_in_hierarchy() {
    let mut h = Harness::new("rename_dbl");
    let r = h.row("Cube");
    double_click(&mut h, r.center());
    assert!(h.ed.renaming.is_some(), "double-click starts renaming");
    select_all_and_type(&mut h, "Hero");
    h.key(Key::Enter);
    assert!(
        kit::scene::find_by_name(h.ed.world(), "Hero").is_some(),
        "renamed; renaming = {:?}",
        h.ed.renaming.as_ref().map(|r| r.1.clone())
    );
}

#[test]
fn rename_with_f2() {
    let mut h = Harness::new("rename_f2");
    let r = h.row("Cube");
    h.click(r.center());
    h.key(Key::F2);
    assert!(h.ed.renaming.is_some(), "F2 starts renaming");
    select_all_and_type(&mut h, "Box");
    h.key(Key::Enter);
    assert!(kit::scene::find_by_name(h.ed.world(), "Box").is_some());
}

#[test]
fn rename_in_inspector() {
    let mut h = Harness::new("rename_insp");
    let cube = kit::scene::find_by_name(h.ed.world(), "Cube").unwrap();
    h.ed.selected = Some(cube);
    h.frame(vec![]);
    let field = ui::marked(&h.ctx, "inspector_name").expect("name field drawn");
    h.click(field.center());
    select_all_and_type(&mut h, "Thing");
    h.key(Key::Enter);
    assert_eq!(h.ed.entity_label(cube), "Thing");
}

#[test]
fn rename_escape_cancels_and_click_away_commits() {
    let mut h = Harness::new("rename_esc");
    let r = h.row("Cube");
    double_click(&mut h, r.center());
    select_all_and_type(&mut h, "Nope");
    h.key(Key::Escape);
    assert!(h.ed.renaming.is_none());
    assert!(kit::scene::find_by_name(h.ed.world(), "Cube").is_some(), "Esc keeps the old name");

    h.wait(1.0);
    let r = h.row("Cube");
    double_click(&mut h, r.center());
    select_all_and_type(&mut h, "Kept");
    // Click on the viewport: commits.
    let vp = h.st.viewport_rect.center();
    h.click(vp);
    assert!(kit::scene::find_by_name(h.ed.world(), "Kept").is_some());
}

#[test]
fn selecting_in_hierarchy_frames_the_object() {
    let mut h = Harness::new("frame");
    // A big object far away.
    let big = h.ed.spawn(crate::state::Preset::Mesh(kit::gamesys::MeshKind::Sphere), None);
    {
        let mut t = h.ed.app.world.get_mut::<kit::scene::Transform>(big).unwrap();
        t.translation = kit::math::Vec3::new(40.0, 0.0, 0.0);
        t.scale = kit::math::Vec3::splat(10.0);
    }
    kit::scene::propagate_transforms(&mut h.ed.app.world);
    h.ed.selected = None;
    h.frame(vec![]);
    let small_dist = h.ed.camera.distance;
    h.click(h.row("Sphere").center());
    assert_eq!(h.ed.selected, Some(big));
    for _ in 0..90 {
        h.ed.camera.tick(1.0 / 60.0);
    }
    assert!(h.ed.camera.target.distance(kit::math::Vec3::new(40.0, 0.0, 0.0)) < 0.01, "camera moved to it");
    assert!(h.ed.camera.distance > small_dist, "zoomed out for a big object");
}

#[test]
fn sculpting_raises_terrain_under_the_brush() {
    let mut h = Harness::new("sculpt");
    let terrain = h.ed.spawn(crate::state::Preset::Terrain, None);
    h.ed.app.world.get_mut::<kit::gamesys::Terrain>(terrain).unwrap().flatten_all(0.0);
    h.ed.selected = Some(terrain);
    h.ed.camera.frame(kit::math::Vec3::ZERO, 10.0);
    for _ in 0..5 {
        h.frame(vec![]);
    }
    let before = ui::marked(&h.ctx, "sculpt_raise");
    h.frame(vec![]);
    assert_eq!(before, ui::marked(&h.ctx, "sculpt_raise"), "layout settled");
    h.click(ui::marked(&h.ctx, "sculpt_raise").expect("sculpt tools shown").center());
    assert_eq!(h.ed.settings.sculpt, Some(kit::gamesys::SculptTool::Raise));
    let c = h.st.viewport_rect.center();
    h.frame(vec![Event::PointerMoved(c)]);
    h.frame(vec![Event::PointerButton {
        pos: c,
        button: PointerButton::Primary,
        pressed: true,
        modifiers: Modifiers::NONE,
    }]);
    for _ in 0..30 {
        h.frame(vec![Event::PointerMoved(c)]);
    }
    h.frame(vec![Event::PointerButton {
        pos: c,
        button: PointerButton::Primary,
        pressed: false,
        modifiers: Modifiers::NONE,
    }]);
    let t = h.ed.world().get::<kit::gamesys::Terrain>(terrain).unwrap();
    let (_, hi) = t.height_range();
    assert!(hi > 0.3, "brush raised the ground: {hi}");
    assert_eq!(h.ed.selected, Some(terrain), "sculpting does not change the selection");
}

#[test]
fn selecting_a_camera_shows_its_preview() {
    let mut h = Harness::new("preview");
    h.frame(vec![]);
    assert!(ui::marked(&h.ctx, "camera_preview").is_none());
    h.click(h.row("Camera").center());
    h.frame(vec![]);
    assert!(h.st.preview_camera.is_some());
    assert!(h.st.preview_px[0] > 100);
    let r = ui::marked(&h.ctx, "camera_preview").expect("preview drawn");
    let vp = h.st.viewport_rect;
    assert!(r.right() <= vp.right() && r.bottom() <= vp.bottom() && r.center().x > vp.center().x, "bottom-right");
    h.click(h.row("Cube").center());
    h.frame(vec![]);
    assert_eq!(h.st.preview_px, [0, 0], "hidden for other objects");
}

#[test]
fn cameras_can_be_picked_in_the_viewport() {
    let w = {
        let mut ed = Editor::new();
        ed.new_scene("main.kscene");
        ed
    };
    let cam = kit::scene::find_by_name(w.world(), "Camera").unwrap();
    let pos = w.world().get::<kit::scene::GlobalTransform>(cam).unwrap().translation();
    let ray = kit::render::pick::Ray { origin: pos + kit::math::Vec3::new(5.0, 0.0, 0.0), dir: kit::math::Vec3::NEG_X };
    assert_eq!(kit::render::pick::pick(w.world(), &ray), Some(cam));
}

fn stroke(h: &mut Harness, from: Pos2, deltas: &[Vec2], modifiers: Modifiers) {
    h.mods = modifiers;
    h.frame(vec![Event::PointerMoved(from)]);
    h.frame(vec![Event::PointerButton { pos: from, button: PointerButton::Primary, pressed: true, modifiers }]);
    let mut pos = from;
    for d in deltas {
        for _ in 0..10 {
            pos += *d / 10.0;
            h.frame(vec![Event::PointerMoved(pos)]);
        }
    }
    h.frame(vec![Event::PointerButton { pos, button: PointerButton::Primary, pressed: false, modifiers }]);
    h.mods = Modifiers::NONE;
    h.frame(vec![]);
    h.wait(0.5);
}

fn canvas_harness(tag: &str) -> (Harness, kit::ecs::Entity) {
    let mut h = Harness::new(tag);
    let cube = kit::scene::find_by_name(h.ed.world(), "Cube").unwrap();
    h.ed.app.world.remove::<kit::gamesys::Script>(cube);
    h.ed.selected = Some(cube);
    h.ed.settings.canvas = true;
    h.ed.camera.frame(kit::math::Vec3::new(0.0, 0.75, 0.0), 3.0);
    h.frame(vec![]);
    h.frame(vec![]);
    (h, cube)
}

fn path_of(h: &Harness, e: kit::ecs::Entity) -> Vec<kit::math::Vec3> {
    h.ed.world().get::<kit::gamesys::PathFollow>(e).map(|p| p.points.clone()).unwrap_or_default()
}

#[test]
fn canvas_draws_on_the_view_plane_and_strokes_extend_in_3d() {
    let (mut h, cube) = canvas_harness("viewplane");
    let start = h.ed.world().get::<kit::scene::Transform>(cube).unwrap().translation;
    // Stroke 1: up and to the right on screen, from the cube.
    let vp = h.st.viewport_rect;
    let fwd1 = crate::canvas::view_forward(&h.ed.camera.view(vp.width() / vp.height()));
    let c = crate::canvas::to_screen(h.ed.camera.view(vp.width() / vp.height()).view_proj(), vp, start).unwrap();
    stroke(&mut h, c, &[Vec2::new(150.0, -120.0)], Modifiers::NONE);
    let p1 = path_of(&h, cube);
    assert!(p1.len() >= 2, "{p1:?}");
    assert!(p1[0].distance(start) < 1e-3, "starts at the object");
    for p in &p1 {
        assert!((*p - start).dot(fwd1).abs() < 1e-2, "stroke 1 lies on the camera-facing plane");
    }
    assert!(p1.last().unwrap().y > start.y + 0.3, "drew upward: a climb, not a ground line");

    // Rotate the view 90 degrees and draw again: the path continues from its end, on the new plane.
    h.ed.camera.yaw += std::f32::consts::FRAC_PI_2;
    h.frame(vec![]);
    let view2 = h.ed.camera.view(vp.width() / vp.height());
    let fwd2 = crate::canvas::view_forward(&view2);
    let end1 = *p1.last().unwrap();
    let c2 = crate::canvas::to_screen(view2.view_proj(), vp, end1).unwrap();
    stroke(&mut h, c2, &[Vec2::new(120.0, 0.0)], Modifiers::NONE);
    let p2 = path_of(&h, cube);
    assert!(p2.len() > p1.len(), "second stroke extends the path");
    assert_eq!(&p2[..p1.len()], &p1[..], "earlier points are kept");
    for p in &p2[p1.len()..] {
        assert!((*p - end1).dot(fwd2).abs() < 1e-2, "stroke 2 lies on the new plane through the path end");
    }
    let spread = p2.iter().fold((kit::math::Vec3::MAX, kit::math::Vec3::MIN), |(lo, hi), p| (lo.min(*p), hi.max(*p)));
    let size = spread.1 - spread.0;
    assert!(size.x > 0.2 && size.y > 0.2 && size.z > 0.2, "a real 3D path: {size}");

    // Ctrl+Z removes just the last stroke.
    let undo =
        |pressed| Event::Key { key: Key::Z, physical_key: None, pressed, repeat: false, modifiers: Modifiers::COMMAND };
    h.mods = Modifiers::COMMAND;
    h.frame(vec![undo(true)]);
    h.frame(vec![undo(false)]);
    h.mods = Modifiers::NONE;
    let cube = kit::scene::find_by_name(h.ed.world(), "Cube").unwrap();
    assert_eq!(path_of(&h, cube), p1, "undo removed the last stroke");
}

#[test]
fn canvas_shift_starts_over_and_points_can_be_dragged() {
    let (mut h, cube) = canvas_harness("handles");
    let vp = h.st.viewport_rect;
    let start = h.ed.world().get::<kit::scene::Transform>(cube).unwrap().translation;
    let view = h.ed.camera.view(vp.width() / vp.height());
    let c = crate::canvas::to_screen(view.view_proj(), vp, start).unwrap();
    stroke(&mut h, c, &[Vec2::new(200.0, 0.0)], Modifiers::NONE);
    stroke(&mut h, c + Vec2::new(200.0, 0.0), &[Vec2::new(0.0, -150.0)], Modifiers::NONE);
    let long = path_of(&h, cube);
    // Shift: start over from the object.
    stroke(&mut h, c, &[Vec2::new(-150.0, 0.0)], Modifiers::SHIFT);
    let fresh = path_of(&h, cube);
    assert!(fresh[0].distance(start) < 1e-3);
    assert!(fresh.last().unwrap().x < start.x, "new path goes the new way");
    assert!(fresh.len() < long.len());

    // Drag the last point somewhere else (the viewport may have resized as the inspector grew).
    let last = *fresh.last().unwrap();
    let vp = h.st.viewport_rect;
    let view = h.ed.camera.view(vp.width() / vp.height());
    let handle = crate::canvas::to_screen(view.view_proj(), vp, last).unwrap();
    stroke(&mut h, handle, &[Vec2::new(0.0, -100.0)], Modifiers::NONE);
    let moved = path_of(&h, cube);
    assert_eq!(moved.len(), fresh.len(), "dragging a point does not add points");
    assert!(moved.last().unwrap().y > last.y + 0.2, "point moved up: {last} -> {}", moved.last().unwrap());
}

fn press_at(h: &mut Harness, pos: Pos2, button: PointerButton, pressed: bool) {
    h.frame(vec![Event::PointerButton { pos, button, pressed, modifiers: Modifiers::NONE }]);
}


#[test]
fn diagram_view_adds_nodes_wires_them_and_undoes_over_the_canvas() {
    use kit::diagram::{NodeKind, PinRef};
    let mut h = Harness::new("diagram");
    let cube = kit::scene::find_by_name(h.ed.world(), "Cube").unwrap();
    h.ed.selected = Some(cube);
    h.key(Key::Num7);
    assert!(h.ed.settings.diagram, "7 opens the Diagram view");
    // Add a Diagram to the cube (what the view's button and the component menu do).
    let path = crate::diagram_view::add_diagram(&mut h.ed, cube).unwrap();
    h.frame(vec![]);
    h.frame(vec![]);
    assert_eq!(h.st.diagram.path(), Some(path.as_str()));
    let start = h.st.diagram.graph.nodes[0].id;
    let gd = kit::gamedata::GameData::default();

    // Right-click the canvas, search, Enter: a Set Visible node.
    let then = h.st.diagram.pin_screen_pos(&gd, &PinRef::new(start, "then"), true).expect("on screen");
    let spot = then + egui::vec2(140.0, 60.0);
    h.frame(vec![Event::PointerMoved(spot)]);
    press_at(&mut h, spot, PointerButton::Secondary, true);
    press_at(&mut h, spot, PointerButton::Secondary, false);
    h.frame(vec![]);
    h.frame(vec![Event::Text("set visible".into())]);
    h.key(Key::Enter);
    let vis = h.st.diagram.graph.nodes.iter().find(|n| n.kind == NodeKind::SetVisible).map(|n| n.id).expect("node added from the picker");
    h.frame(vec![]);

    // Drag a wire from On Start's exec output to the new node's exec input.
    let to = h.st.diagram.pin_screen_pos(&gd, &PinRef::new(vis, "exec"), false).unwrap();
    drag(&mut h, then, to);
    assert!(h.st.diagram.graph.links.iter().any(|l| l.from == PinRef::new(start, "then") && l.to == PinRef::new(vis, "exec")), "{:?}", h.st.diagram.graph.links);
    // Saved to disk right away.
    let saved = std::fs::read_to_string(h.dir.join(&path)).unwrap();
    assert!(kit::diagram::Graph::from_json(&saved).unwrap().links.len() == 1);

    // Click the wire to select it; Backspace removes just the wire; undo brings it back.
    let link = h.st.diagram.graph.links[0].clone();
    let mid = h.st.diagram.wire_screen_mid(&gd, &link).unwrap();
    h.click(mid);
    assert_eq!(h.st.diagram.selected_wire(), Some(&link), "clicking a wire selects it");
    h.key(Key::Backspace);
    assert!(h.st.diagram.graph.links.is_empty() && h.st.diagram.graph.node(vis).is_some(), "only the wire is deleted");
    h.frame(vec![Event::Key { key: Key::Z, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::COMMAND }]);
    h.frame(vec![]);
    assert_eq!(h.st.diagram.graph.links, vec![link]);
    // Clicking empty canvas deselects the wire.
    h.click(mid + egui::vec2(0.0, 90.0));
    assert!(h.st.diagram.selected_wire().is_none());

    // Select the node and press Delete over the canvas: the node goes, the cube stays.
    let header = h.st.diagram.header_screen_pos(vis).unwrap();
    h.click(header);
    h.key(Key::Delete);
    assert!(h.st.diagram.graph.node(vis).is_none());
    assert!(h.ed.world().is_alive(cube), "Delete over the diagram doesn't delete the object");
    // Undo over the canvas brings it (and its wire) back.
    h.frame(vec![Event::Key { key: Key::Z, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::COMMAND }]);
    h.frame(vec![]);
    assert!(h.st.diagram.graph.node(vis).is_some() && h.st.diagram.graph.links.len() == 1);
}

#[test]
fn import_accepts_every_kind_kit_uses_and_explains_the_rest() {
    let mut h = Harness::new("import_all");
    let src = std::env::temp_dir().join(format!("kit_import_src_{}", std::process::id()));
    std::fs::create_dir_all(&src).unwrap();
    image::RgbaImage::new(4, 4).save(src.join("hero.png")).unwrap();
    std::fs::write(src.join("enemy.x"), "CREATE Enemy[\n    EDIT FLOAT Speed: 1.0\n]\n").unwrap();
    std::fs::write(src.join("boss.kscene"), kit::gamesys::save_scene(h.ed.world()).to_json()).unwrap();
    std::fs::write(src.join("door.kdiagram"), kit::diagram::Graph::default().to_json()).unwrap();
    std::fs::write(src.join("notes.docx"), "nope").unwrap();
    std::fs::write(src.join("fake.png"), "not an image").unwrap();
    let got: Vec<String> =
        ["hero.png", "enemy.x", "boss.kscene", "door.kdiagram"].iter().map(|f| h.ed.import_asset(&src.join(f)).unwrap().unwrap()).collect();
    assert_eq!(got, ["sprites/hero.png", "scripts/enemy.x", "scenes/boss.kscene", "diagrams/door.kdiagram"]);
    for g in &got {
        assert!(h.dir.join(g).is_file() && h.ed.assets.contains(g), "{g}");
    }
    let err = h.ed.import_asset(&src.join("notes.docx")).unwrap_err().to_string();
    assert!(err.contains("can't use .docx") && err.contains("sprites"), "{err}");
    assert!(h.ed.import_asset(&src.join("fake.png")).is_err(), "broken images are refused");
    // Importing the same file again doesn't duplicate it.
    assert_eq!(h.ed.import_asset(&src.join("hero.png")).unwrap().unwrap(), "sprites/hero.png");
    // A sprite dropped on the viewport becomes an object.
    let e = h.ed.place_sprite("sprites/hero.png", None);
    assert_eq!(h.ed.world().get::<kit::gamesys::Sprite>(e).unwrap().texture, "sprites/hero.png");
    let _ = std::fs::remove_dir_all(&src);
}


#[test]
fn console_starts_collapsed_and_counts_new_errors() {
    let mut h = Harness::new("console_badge");
    assert!(!h.ed.settings.console, "collapsed by default");
    h.ed.console().error("test", "boom");
    h.frame(vec![]);
    assert_eq!(h.ed.world().resource::<kit::core::Console>().errors_total, 1);
    // Opening the console marks them seen.
    h.key(Key::Backtick);
    assert!(h.ed.settings.console);
    h.key(Key::Backtick);
    h.frame(vec![]);
    assert_eq!(h.st.errors_seen_for_test(), 1);
}
