//! Editor layout: header, hierarchy, viewport, inspector, console, footer.

use crate::code;
use crate::controls::Shortcut;
use crate::state::{Editor, Preset};
use crate::theme::*;
use crate::widgets::{self, badge, dock_toggle, ghost, ghost_kbd, primary};
use egui::{Align, Align2, Color32, CornerRadius, Frame, Id, Key, Layout, Margin, RichText, Sense, Stroke, Ui, Vec2};
use kit::ecs::Entity;
use kit::gamesys::{Camera, Light, MeshKind, Model, Projection, Script, Sprite};
use kit::math::{Vec3, Vec4};
use kit::scene::{Name, Transform};
use std::collections::HashSet;
use std::path::PathBuf;
use std::time::Duration;

/// Things the UI asks the host loop to do (OS dialogs, quitting).
#[derive(Clone, Debug)]
pub enum Request {
    BrowseNewProject,
    CreateProject(PathBuf, crate::templates::NewProject),
    OpenProject,
    OpenExample,
    ImportModels,
    OpenPath(PathBuf),
    /// Choose the Export wizard's output folder.
    PickExportFolder,
    /// Import a publisher key file (Export → Protection).
    ImportPublisherKey,
    /// The light/dark theme was switched; remember it.
    SaveTheme,
}

#[derive(Clone, Copy)]
enum ViewPane {
    Hierarchy,
    Assets,
    Inspector,
    Diagram,
    Game,
    Console,
    Rex,
}

impl ViewPane {
    const ALL: [(Self, &'static str); 7] = [
        (Self::Hierarchy, "Hierarchy"),
        (Self::Assets, "Assets"),
        (Self::Inspector, "Inspector"),
        (Self::Diagram, "Diagram"),
        (Self::Game, "Game View"),
        (Self::Console, "Console"),
        (Self::Rex, "Rex"),
    ];
}

struct Views {
    /// Whether the combined Hierarchy / Assets dock is expanded.
    left_dock: bool,
    hierarchy: bool,
    assets: bool,
    inspector: bool,
    game: bool,
}

impl Default for Views {
    fn default() -> Self {
        Self { left_dock: true, hierarchy: true, assets: true, inspector: true, game: true }
    }
}

pub struct UiState {
    pub new_project: Option<crate::new_project::Wizard>,
    views: Views,
    pub viewport_tex: Option<egui::TextureId>,
    /// Desired viewport render size in physical pixels.
    pub viewport_px: [u32; 2],
    pub viewport_rect: egui::Rect,
    /// The game receives keyboard input only while the viewport has focus.
    pub viewport_focused: bool,
    collapsed: HashSet<Entity>,
    new_asset: Option<(AssetKind, String)>,
    new_asset_focus: bool,
    pub sketch: crate::canvas::Sketch,
    pub settings: crate::settings::SettingsWindow,
    pub brush: crate::terrain_tools::Brush,
    pub gizmo: crate::move_gizmo::MoveGizmo,
    pub export: crate::export_wizard::ExportWizard,
    pub diagram: crate::diagram_view::DiagramView,
    pub rex: crate::rex_panel::RexPanel,
    /// Camera preview (picture-in-picture): texture and desired pixel size; [0, 0] = hidden.
    pub preview_tex: Option<egui::TextureId>,
    pub preview_px: [u32; 2],
    pub preview_camera: Option<Entity>,
    pub requests: Vec<Request>,
    pub recent: Vec<PathBuf>,
    pub help: bool,
    pub fps: f32,
    pub gpu_name: String,
    console_rev: u64,
    /// Errors already seen (the console was open when they arrived).
    errors_seen: u64,
    pointer_moved: f32,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum AssetKind {
    Script,
    Scene,
}

impl UiState {
    fn pane_visible(&self, ed: &Editor, pane: ViewPane) -> bool {
        match pane {
            ViewPane::Hierarchy => self.views.left_dock && self.views.hierarchy,
            ViewPane::Assets => self.views.left_dock && self.views.assets,
            ViewPane::Inspector => self.views.inspector,
            ViewPane::Game => self.views.game,
            ViewPane::Diagram => ed.settings.diagram,
            ViewPane::Console => ed.settings.console,
            ViewPane::Rex => ed.settings.rex,
        }
    }

    fn can_hide(&self, ed: &Editor) -> bool {
        ViewPane::ALL.iter().filter(|(pane, _)| self.pane_visible(ed, *pane)).count() > 1
    }

    fn has_visible(&self, ed: &Editor) -> bool {
        ViewPane::ALL.iter().any(|(pane, _)| self.pane_visible(ed, *pane))
    }

    fn toggle_pane(&mut self, ed: &mut Editor, pane: ViewPane) {
        let was_visible = self.pane_visible(ed, pane);
        if was_visible && !self.can_hide(ed) {
            ed.notify("Keep at least one view open.");
            return;
        }
        let visible = match pane {
            ViewPane::Hierarchy => &mut self.views.hierarchy,
            ViewPane::Assets => &mut self.views.assets,
            ViewPane::Inspector => &mut self.views.inspector,
            ViewPane::Game => &mut self.views.game,
            ViewPane::Diagram => &mut ed.settings.diagram,
            ViewPane::Console => &mut ed.settings.console,
            ViewPane::Rex => &mut ed.settings.rex,
        };
        *visible = !was_visible;
        if !was_visible && matches!(pane, ViewPane::Rex) {
            self.rex.ask_focus();
        }
        if !was_visible && matches!(pane, ViewPane::Hierarchy | ViewPane::Assets) {
            self.views.left_dock = true;
        }
        if !self.views.game {
            self.hide_viewport();
        }
    }

    pub(crate) fn ensure_visible(&mut self, ed: &mut Editor) {
        if !ViewPane::ALL.iter().any(|(pane, _)| self.pane_visible(ed, *pane)) {
            self.views.game = true;
        }
    }

    fn hide_viewport(&mut self) {
        self.viewport_rect = egui::Rect::NOTHING;
        self.viewport_px = [0, 0];
        self.viewport_focused = false;
        self.preview_px = [0, 0];
        self.preview_camera = None;
    }

    fn restore_views(&mut self, ctx: &egui::Context, ed: &mut Editor) {
        self.views = Views::default();
        ed.settings.diagram = false;
        ed.settings.console = false;
        ed.settings.rex = false;
        for name in ["hierarchy", "inspector", "diagram", "console", "game_view", "rex"] {
            ctx.data_mut(|data| data.remove::<egui::panel::PanelState>(Id::new(name)));
        }
    }

    #[cfg(test)]
    pub fn errors_seen_for_test(&self) -> u64 {
        self.errors_seen
    }

    pub fn new(recent: Vec<PathBuf>, gpu_name: String) -> Self {
        Self {
            new_project: None,
            views: Views::default(),
            viewport_tex: None,
            viewport_px: [0, 0],
            viewport_rect: egui::Rect::NOTHING,
            viewport_focused: false,
            collapsed: HashSet::new(),
            new_asset: None,
            new_asset_focus: false,
            sketch: Default::default(),
            settings: Default::default(),
            brush: Default::default(),
            gizmo: Default::default(),
            export: Default::default(),
            diagram: Default::default(),
            rex: Default::default(),
            preview_tex: None,
            preview_px: [0, 0],
            preview_camera: None,
            requests: Vec::new(),
            recent,
            help: false,
            fps: 0.0,
            gpu_name,
            console_rev: 0,
            errors_seen: 0,
            pointer_moved: 0.0,
        }
    }
}

enum Action {
    Select(Option<Entity>),
    Spawn(Preset, Option<Entity>),
    Delete(Entity),
    Duplicate(Entity),
    Reparent(Entity, Option<Entity>, usize),
    StartRename(Entity),
    Frame(Entity),
    ToggleCollapse(Entity),
    AttachSound(Entity, String),
}

/// A sound asset being dragged from the Assets panel.
#[derive(Clone, Debug)]
pub struct SoundDrag(pub String);

/// Where something dropped at `pos` in the viewport lands: on the ground
/// (y = 0) in 3D, on the z = 0 plane in 2D, else in front of the camera.
fn viewport_point(ed: &Editor, st: &UiState, pos: egui::Pos2) -> Vec3 {
    let rect = st.viewport_rect;
    let view = ed.camera.view(rect.width() / rect.height().max(1.0));
    let ndc = kit::math::Vec2::new(
        (pos.x - rect.left()) / rect.width().max(1.0) * 2.0 - 1.0,
        (pos.y - rect.top()) / rect.height().max(1.0) * 2.0 - 1.0,
    );
    let ray = kit::render::pick::Ray::from_ndc(view.view_proj(), ndc);
    let (normal, height) = if ed.camera.two_d { (Vec3::Z, 0.0) } else { (Vec3::Y, 0.0) };
    let d = ray.dir.dot(normal);
    if d.abs() > 1e-4 {
        let t = (height - ray.origin.dot(normal)) / d;
        if t > 0.0 && t < 500.0 {
            let p = ray.origin + ray.dir * t;
            return if ed.camera.two_d { p } else { p + Vec3::Y * 0.5 };
        }
    }
    ray.origin + ray.dir * 6.0
}

fn apply(ed: &mut Editor, st: &mut UiState, actions: Vec<Action>) {
    for a in actions {
        match a {
            Action::Select(e) => {
                ed.selected = e;
                // Selecting in the hierarchy brings the object into view, sized to fit.
                if e.is_some() && !ed.playing {
                    ed.frame_selected();
                }
            }
            Action::Spawn(p, parent) => {
                ed.spawn(p, parent);
                if let Some(par) = parent {
                    st.collapsed.remove(&par);
                }
            }
            Action::Delete(e) => ed.delete(e),
            Action::Duplicate(e) => ed.duplicate(e),
            Action::Reparent(e, p, i) => ed.reparent(e, p, i),
            Action::StartRename(e) => ed.renaming = Some((e, ed.entity_label(e))),
            Action::Frame(e) => {
                ed.selected = Some(e);
                ed.frame_selected();
            }
            Action::AttachSound(e, clip) => ed.attach_sound(e, &clip),
            Action::ToggleCollapse(e) => {
                if !st.collapsed.remove(&e) {
                    st.collapsed.insert(e);
                }
            }
        }
    }
}

/// Build the whole editor UI for one frame.
pub fn draw(root: &mut Ui, ed: &mut Editor, st: &mut UiState) {
    let ctx = root.ctx().clone();
    st.ensure_visible(ed);
    if st.new_project.is_none() && !st.settings.capturing() {
        shortcuts(&ctx, ed, st);
    }
    // Files dropped from the system: sounds go to Assets → Sounds (and, dropped
    // on the viewport, into the scene); 3D models are imported.
    let drop_pos = ctx.input(|i| i.pointer.latest_pos());
    for file in ctx.input_mut(|input| std::mem::take(&mut input.raw.dropped_files)) {
        let path = file.path().to_path_buf();
        let on_viewport = drop_pos.filter(|p| st.viewport_rect.contains(*p) && !ed.playing);
        match ed.import_asset(&path) {
            Ok(Some(rel)) => {
                // Dropped on the 3D view: sounds and sprites become objects there.
                if let Some(pos) = on_viewport {
                    let at = viewport_point(ed, st, pos);
                    if kit::gamesys::is_sound(&rel) {
                        ed.place_sound(&rel, Some(at));
                    } else if matches!(crate::state::asset_group(&rel).map(|g| g.name), Some("Sprites")) {
                        ed.place_sprite(&rel, Some(at));
                    }
                }
            }
            Ok(None) => {}
            Err(e) => ed.report(Err(e)),
        }
    }
    // Overlays on the 3D view follow its background (white in light mode unless the scene sets one).
    let game_view = ed.playing && ed.settings.game_camera;
    let background = if crate::theme::is_light() && !game_view { Some(ed.light_background()) } else { None };
    let luminance = background.map_or(0.0, |c| 0.2126 * c.x + 0.7152 * c.y + 0.0722 * c.z);
    crate::theme::viewport::set_light(luminance > 0.5);
    let was_exporting = ed.exporting();
    if ed.poll_export() {
        ctx.request_repaint_after(std::time::Duration::from_millis(250));
    } else if was_exporting {
        // A rebuilt player changes what's ready.
        st.export.refresh();
    }
    if ed.poll_model_imports() {
        ctx.request_repaint_after(std::time::Duration::from_millis(100));
    }
    ed.refresh_assets(false);

    egui::Panel::top("header")
        .exact_size(64.0)
        .frame(Frame::NONE.inner_margin(Margin::symmetric(28, 0)).fill(BG()))
        .show_separator_line(false)
        .show(root, |ui| header(ui, ed, st));

    egui::Panel::bottom("footer")
        .exact_size(40.0)
        .frame(Frame::NONE.inner_margin(Margin::symmetric(28, 0)).fill(BG()))
        .show_separator_line(false)
        .show(root, |ui| footer(ui, ed, st));

    {
        let left_open_before = st.views.left_dock;
        let mut left_open = left_open_before;
        egui::Panel::left("hierarchy")
            .default_size(250.0)
            .size_range(180.0..=420.0)
            .resizable(true)
            .frame(Frame::NONE.inner_margin(Margin { left: 20, right: 6, top: 0, bottom: 8 }).fill(BG()))
            .show_separator_line(false)
            .show_collapsible(root, &mut left_open, |ui| {
                theme_card().show(ui, |ui| {
                    ui.set_min_size(ui.available_size());
                    if st.views.hierarchy {
                        hierarchy(ui, ed, st);
                    }
                    if st.views.assets {
                        if st.views.hierarchy {
                            ui.add_space(6.0);
                            ui.separator();
                        }
                        assets(ui, ed, st);
                    }
                });
            });
        st.views.left_dock = left_open;
        if left_open_before && !st.views.left_dock && !st.has_visible(ed) {
            st.views.left_dock = true;
            ed.notify("Keep at least one view open.");
        }
    }

    {
        let inspector_open_before = st.views.inspector;
        let mut inspector_open = inspector_open_before;
        egui::Panel::right("inspector")
            .default_size(300.0)
            .size_range(240.0..=520.0)
            .resizable(true)
            .frame(Frame::NONE.inner_margin(Margin { left: 6, right: 20, top: 0, bottom: 8 }).fill(BG()))
            .show_separator_line(false)
            .show_collapsible(root, &mut inspector_open, |ui| {
                theme_card().show(ui, |ui| {
                    ui.set_min_size(ui.available_size());
                    inspector(ui, ed);
                });
            });
        st.views.inspector = inspector_open;
        if inspector_open_before && !st.views.inspector && !st.has_visible(ed) {
            st.views.inspector = true;
            ed.notify("Keep at least one view open.");
        }
    }

    egui::CentralPanel::default()
        .frame(Frame::NONE.inner_margin(Margin { left: 6, right: 6, top: 0, bottom: 8 }).fill(BG()))
        .show(root, |ui| {
            {
                // Bottom panels are laid out first (Rex at the very bottom, then the
                // Console and the Diagram view); the game view fills what's left.
                let rex_open_before = ed.settings.rex;
                let mut rex_open = rex_open_before;
                egui::Panel::bottom("rex")
                    .default_size(380.0)
                    // Leave room for the game view above.
                    .size_range(230.0..=(ui.available_height() * 0.8).max(240.0))
                    .resizable(true)
                    .frame(Frame::NONE.inner_margin(Margin { left: 0, right: 0, top: 8, bottom: 0 }))
                    .show_separator_line(false)
                    .show_collapsible(ui, &mut rex_open, |ui| {
                        theme_card().inner_margin(Margin::symmetric(14, 10)).show(ui, |ui| {
                            ui.set_min_size(ui.available_size());
                            crate::rex_panel::show(ui, ed, &mut st.rex);
                        });
                    });
                ed.settings.rex = rex_open;
                if rex_open_before && !ed.settings.rex && !st.has_visible(ed) {
                    ed.settings.rex = true;
                    ed.notify("Keep at least one view open.");
                }
            }
            {
                let console_open_before = ed.settings.console;
                let mut console_open = console_open_before;
                egui::Panel::bottom("console")
                    .default_size(150.0)
                    .size_range(80.0..=500.0)
                    .resizable(true)
                    .frame(Frame::NONE.inner_margin(Margin { left: 0, right: 0, top: 8, bottom: 0 }))
                    .show_separator_line(false)
                    .show_collapsible(ui, &mut console_open, |ui| {
                        theme_card().inner_margin(Margin::symmetric(14, 10)).show(ui, |ui| {
                            ui.set_min_size(ui.available_size());
                            console(ui, ed, st);
                        });
                    });
                ed.settings.console = console_open;
                if console_open_before && !ed.settings.console && !st.has_visible(ed) {
                    ed.settings.console = true;
                    ed.notify("Keep at least one view open.");
                }
            }
            {
                let diagram_open_before = ed.settings.diagram;
                let mut diagram_open = diagram_open_before;
                let diagram_min = if st.views.game { 180.0 } else { 100.0 };
                egui::Panel::bottom("diagram")
                    .default_size(if st.views.game { ui.available_height() * 0.6 } else { ui.available_height() }.max(diagram_min))
                    .size_range(diagram_min..=if st.views.game { ui.available_height() * 0.85 } else { ui.available_height() }.max(diagram_min))
                    .resizable(true)
                    .frame(Frame::NONE.inner_margin(Margin { left: 0, right: 0, top: 8, bottom: 0 }))
                    .show_separator_line(false)
                    .show_collapsible(ui, &mut diagram_open, |ui| {
                        theme_card().inner_margin(Margin::symmetric(12, 10)).show(ui, |ui| {
                            ui.set_min_size(ui.available_size());
                            diagram_panel(ui, ed, st);
                        });
                    });
                ed.settings.diagram = diagram_open;
                if diagram_open_before && !ed.settings.diagram && !st.has_visible(ed) {
                    ed.settings.diagram = true;
                    ed.notify("Keep at least one view open.");
                }
            }
            let game_open_before = st.views.game;
            let mut game_open = game_open_before;
            egui::Panel::top("game_view")
                // Laid out last: the game view always fills the room the panels
                // below leave (resize those panels to change the split).
                .exact_size(ui.available_height().max(140.0))
                .resizable(false)
                .frame(Frame::NONE)
                .show_separator_line(false)
                .show_collapsible(ui, &mut game_open, |ui| viewport(ui, ed, st));
            st.views.game = game_open;
            if game_open_before && !st.views.game && !st.has_visible(ed) {
                st.views.game = true;
                ed.notify("Keep at least one view open.");
            }

            if !st.views.game {
                st.hide_viewport();
            }
        });

    if std::mem::take(&mut ed.request_settings) {
        st.settings.open(ed);
    }
    code::script_window(&ctx, ed);
    new_asset_dialog(&ctx, ed, st);
    crate::settings::window(&ctx, ed, &mut st.settings);
    let wizard = crate::export_wizard::window(&ctx, ed, &mut st.export);
    if wizard.pick_folder {
        st.requests.push(Request::PickExportFolder);
    }
    if wizard.import_key {
        st.requests.push(Request::ImportPublisherKey);
    }
    welcome(&ctx, ed, st);
    if let Some(wizard) = st.new_project.as_mut() {
        match crate::new_project::window(&ctx, wizard) {
            Some(crate::new_project::Action::Browse) => st.requests.push(Request::BrowseNewProject),
            Some(crate::new_project::Action::Create(path, options)) => {
                st.requests.push(Request::CreateProject(path, options))
            }
            Some(crate::new_project::Action::Close) => st.new_project = None,
            None => {}
        }
    }
    help(&ctx, st, &ed.controls);
    notice(&ctx, ed);
}

fn diagram_panel(ui: &mut Ui, ed: &mut Editor, st: &mut UiState) {
    crate::diagram_view::show(ui, ed, &mut st.diagram);
}

fn theme_card() -> Frame {
    crate::theme::card()
}

// --------------------------------------------------------------------- input

fn shortcuts(ctx: &egui::Context, ed: &mut Editor, st: &mut UiState) {
    let typing = ctx.egui_wants_keyboard_input();
    let script_focused = code::script_editor_focused(ctx);
    let (save, play, help, release) = ctx.input_mut(|input| {
        let controls = &ed.controls;
        (
            controls.take(input, Shortcut::Save),
            controls.take(input, Shortcut::Play),
            controls.take(input, Shortcut::Help),
            (st.viewport_focused || ed.settings.canvas && !typing && !st.diagram.hovered)
                && controls.take(input, Shortcut::Release),
        )
    });
    if save {
        if script_focused {
            if let Some(i) = ed.active_script {
                ed.save_script(i);
            }
        } else {
            let result = ed.save_scene();
            ed.report(result);
        }
    }
    if play {
        if ed.playing { ed.stop() } else { ed.play() }
    }
    if help {
        st.help = !st.help;
    }
    if release && st.viewport_focused {
        st.viewport_focused = false;
    }

    if ed.playing && st.viewport_focused || typing {
        return;
    }
    let in_diagram = ed.settings.diagram && st.diagram.hovered;
    let pressed = |action| ctx.input_mut(|input| ed.controls.take(input, action));
    let (undo, redo, delete, duplicate, diagram, console, rex) = (
        pressed(Shortcut::Undo),
        pressed(Shortcut::Redo),
        pressed(Shortcut::Delete),
        pressed(Shortcut::Duplicate),
        pressed(Shortcut::Diagram),
        pressed(Shortcut::Console),
        pressed(Shortcut::Rex),
    );
    if rex {
        st.toggle_pane(ed, ViewPane::Rex);
    }
    if undo && !redo {
        if in_diagram { st.diagram.undo(ed) } else { ed.undo() }
    }
    if redo {
        if in_diagram { st.diagram.redo(ed) } else { ed.redo() }
    }
    if diagram {
        st.toggle_pane(ed, ViewPane::Diagram);
    }
    if console {
        st.toggle_pane(ed, ViewPane::Console);
    }
    if in_diagram {
        if !ed.playing {
            if delete {
                st.diagram.delete_selected(ed);
            }
            if duplicate {
                st.diagram.duplicate_selected(ed);
            }
        }
        return;
    }

    for action in [
        Shortcut::Frame,
        Shortcut::Wireframe,
        Shortcut::Lighting,
        Shortcut::Grid,
        Shortcut::Colliders,
        Shortcut::Canvas,
        Shortcut::Volumes,
        Shortcut::Rotate,
        Shortcut::Dimension,
        Shortcut::Rename,
    ] {
        if !ctx.input_mut(|input| ed.controls.take(input, action)) {
            continue;
        }
        match action {
            Shortcut::Frame => ed.frame_selected(),
            Shortcut::Wireframe => ed.settings.wireframe = !ed.settings.wireframe,
            Shortcut::Lighting => ed.settings.lighting = !ed.settings.lighting,
            Shortcut::Grid => ed.settings.grid = !ed.settings.grid,
            Shortcut::Colliders => ed.settings.colliders = !ed.settings.colliders,
            Shortcut::Canvas if !ed.playing => ed.settings.canvas = !ed.settings.canvas,
            Shortcut::Volumes => ed.settings.volumes = !ed.settings.volumes,
            Shortcut::Rotate => ed.camera.auto_rotate = !ed.camera.auto_rotate,
            Shortcut::Dimension => ed.camera.set_two_d(!ed.camera.two_d),
            Shortcut::Rename if !ed.playing => {
                if let Some(entity) = ed.selected {
                    ed.renaming = Some((entity, ed.entity_label(entity)));
                }
            }
            _ => {}
        }
    }

    let over_viewport =
        st.views.game && ctx.input(|i| i.pointer.hover_pos()).is_some_and(|p| st.viewport_rect.contains(p));
    if over_viewport && !(ed.playing && ed.settings.game_camera) {
        let (dir, fast, dt) = ctx.input(|input| {
            let held = |action| ed.controls.held(input, action);
            let dir = Vec3::new(
                held(Shortcut::Right) - held(Shortcut::Left),
                held(Shortcut::Up) - held(Shortcut::Down),
                held(Shortcut::Forward) - held(Shortcut::Back),
            );
            (dir, ed.controls.gestures.fast_camera.held(input.modifiers), input.stable_dt.min(0.1))
        });
        if dir != Vec3::ZERO {
            ed.camera.fly(dir, dt, fast);
            ctx.request_repaint();
        }
    }
    if release {
        ed.settings.canvas = false;
    }
    if !ed.playing {
        if let Some(entity) = ed.selected {
            if delete {
                ed.delete(entity);
            } else if duplicate {
                ed.duplicate(entity);
            }
        }
    }
}

// -------------------------------------------------------------------- header

fn header(ui: &mut Ui, ed: &mut Editor, st: &mut UiState) {
    ui.horizontal_centered(|ui| {
        widgets::logo(ui, 30.0);
        ui.add_space(4.0);
        ui.label(RichText::new("Kit").size(21.0).strong().color(TEXT()));
        ui.add_space(14.0);
        if ed.project.is_some() {
            ui.label(RichText::new(ed.project_title()).color(MUTED()));
            ui.label(RichText::new("›").color(MUTED()));
            ui.label(RichText::new(ed.scene_name()).color(TEXT_STRONG()));
            if ed.scene_dirty() {
                ui.label(RichText::new("●").size(9.0).color(ACCENT())).on_hover_text("Unsaved changes");
            }
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let play_label = if ed.playing { "■  Stop" } else { "▶  Play" };
            if primary(ui, play_label, Some(&ed.controls.label(Shortcut::Play))).clicked() {
                if ed.playing { ed.stop() } else { ed.play() }
            }
            ui.add_space(6.0);
            let light = crate::theme::is_light();
            let toggle = ghost(ui, if light { "🌙" } else { "☀" }).on_hover_text(if light {
                "Switch to dark mode"
            } else {
                "Switch to light mode"
            });
            mark(ui, "theme_toggle", toggle.rect);
            if toggle.clicked() {
                crate::theme::set_light(ui.ctx(), !light);
                st.requests.push(Request::SaveTheme);
            }
            if ed.project.is_some() {
                let export = ghost(ui, "📤 Export").on_hover_text("Export your game for Windows, macOS, Linux…");
                mark(ui, "header_export", export.rect);
                if export.clicked() {
                    st.export.show(ed);
                }
                if ghost_kbd(ui, "Save", &ed.controls.label(Shortcut::Save)).clicked() {
                    let r = ed.save_scene();
                    ed.report(r);
                }
            }
            let new_menu = ui.menu_button(RichText::new("+  New").color(TEXT()), |ui| {
                let r = ui.button("Scene…");
                mark(ui, "header_new_scene", r.rect);
                if r.clicked() {
                    begin_new_asset(ed, st, AssetKind::Scene);
                    ui.close();
                }
                let r = ui.button("Script…");
                mark(ui, "header_new_script", r.rect);
                if r.clicked() {
                    begin_new_asset(ed, st, AssetKind::Script);
                    ui.close();
                }
                ui.separator();
                ui.add_enabled_ui(!ed.playing, |ui| {
                    ui.menu_button("Object", |ui| {
                        for p in Preset::MENU {
                            if ui.button(p.label()).clicked() {
                                ed.spawn(p, None);
                                ui.close();
                            }
                        }
                    });
                });
            });
            mark(ui, "header_new", new_menu.response.rect);
            let project_menu = ui.menu_button(RichText::new("Project").color(TEXT()), |ui| {
                let new_project = ui.button("New project…");
                mark(ui, "new_project_menu", new_project.rect);
                if new_project.clicked() {
                    st.new_project = Some(crate::new_project::Wizard::new(&crate::user_projects_dir()));
                    ui.close();
                }
                if ui.button("Open project…").clicked() {
                    st.requests.push(Request::OpenProject);
                    ui.close();
                }
                if ui.button("Open example").clicked() {
                    st.requests.push(Request::OpenExample);
                    ui.close();
                }
                ui.separator();
                ui.menu_button("View", |ui| {
                    for (pane, label) in ViewPane::ALL {
                        let mut visible = st.pane_visible(ed, pane);
                        if ui
                            .add_enabled(!visible || st.can_hide(ed), egui::Checkbox::new(&mut visible, label))
                            .changed()
                        {
                            st.toggle_pane(ed, pane);
                        }
                    }
                    ui.separator();
                    if ui.button("Default").on_hover_text("Restore the default panel layout").clicked() {
                        st.restore_views(ui.ctx(), ed);
                        ui.close();
                    }
                });
                if ui.add_enabled(ed.project.is_some(), egui::Button::new("Project settings…")).clicked() {
                    st.settings.open(ed);
                    ui.close();
                }
                if !st.recent.is_empty() {
                    ui.separator();
                    for p in st.recent.clone().iter().take(6) {
                        if ui.button(p.display().to_string()).clicked() {
                            st.requests.push(Request::OpenPath(p.clone()));
                            ui.close();
                        }
                    }
                }
            });
            mark(ui, "project_menu", project_menu.response.rect);
            ui.add_enabled_ui(ed.can_redo(), |ui| {
                if ghost(ui, "↷").on_hover_text(format!("Redo ({})", ed.controls.label(Shortcut::Redo))).clicked() {
                    ed.redo();
                }
            });
            ui.add_enabled_ui(ed.can_undo(), |ui| {
                if ghost(ui, "↶").on_hover_text(format!("Undo ({})", ed.controls.label(Shortcut::Undo))).clicked() {
                    ed.undo();
                }
            });
        });
    });
}

// -------------------------------------------------------------------- footer

fn footer(ui: &mut Ui, ed: &mut Editor, st: &mut UiState) {
    ui.horizontal_centered(|ui| {
        match ed.selected.filter(|e| ed.world().is_alive(*e)) {
            Some(e) => {
                ui.label(RichText::new(ed.entity_label(e)).size(12.0).strong().color(TEXT_STRONG()));
                badge(ui, ed.entity_kind(e));
            }
            None => {
                ui.label(RichText::new(ed.scene_name()).size(12.0).strong().color(TEXT_STRONG()));
                badge(ui, if ed.playing { "PLAYING" } else { "SCENE" });
            }
        }
        ui.label(RichText::new(format!("{} entities", ed.world().entity_count())).size(11.0).color(MUTED()));
        ui.label(RichText::new(format!("{:.0} fps", st.fps)).size(11.0).color(MUTED()));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let tip = if ed.playing && st.viewport_focused {
                format!(
                    "{} release input · {} stop",
                    ed.controls.label(Shortcut::Release),
                    ed.controls.label(Shortcut::Play)
                )
            } else {
                format!("{} shortcuts", ed.controls.label(Shortcut::Help))
            };
            ui.label(RichText::new(tip).size(11.0).color(MUTED()));
            ui.add_space(6.0);
            // Collapsed by default; new errors show a count here.
            let total = ed.world().try_resource::<kit::core::Console>().map_or(0, |c| c.errors_total);
            if ed.settings.console {
                st.errors_seen = total;
            }
            let unseen = total.saturating_sub(st.errors_seen);
            if unseen > 0 {
                let badge = ui
                    .add(
                        egui::Label::new(RichText::new(format!("● {unseen}")).size(11.0).color(ERROR()))
                            .sense(egui::Sense::click()),
                    )
                    .on_hover_text(format!("{unseen} new error{} in the console", if unseen == 1 { "" } else { "s" }));
                if badge.clicked() {
                    ed.settings.console = true;
                }
            }
            if ghost(ui, RichText::new(if ed.settings.console { "Console ▾" } else { "Console ▸" }).size(11.0).text())
                .clicked()
            {
                st.toggle_pane(ed, ViewPane::Console);
            }
        });
    });
}

// ----------------------------------------------------------------- hierarchy

/// `mark` for other editor modules.
pub(crate) fn mark_rect(ui: &Ui, name: &'static str, rect: egui::Rect) {
    mark(ui, name, rect);
}

/// Remember where a named widget was drawn (used by UI tests to click it).
fn mark(ui: &Ui, name: &'static str, rect: egui::Rect) {
    ui.ctx().data_mut(|d| d.insert_temp(Id::new(("kit_mark", name)), rect));
}

#[cfg(test)]
pub fn marked(ctx: &egui::Context, name: &'static str) -> Option<egui::Rect> {
    ctx.data(|d| d.get_temp(Id::new(("kit_mark", name))))
}

/// Remember where each row was drawn (used by UI tests to click rows).
fn st_row_rect(ui: &Ui, e: Entity, rect: egui::Rect) {
    ui.ctx().data_mut(|d| {
        let rows: &mut Vec<(Entity, egui::Rect)> = d.get_temp_mut_or_default(Id::new("kit_row_rects"));
        rows.retain(|(x, _)| *x != e);
        rows.push((e, rect));
    });
}

/// Row rectangles from the last frame (for tests).
#[cfg(test)]
pub fn row_rects(ctx: &egui::Context) -> Vec<(Entity, egui::Rect)> {
    ctx.data(|d| d.get_temp(Id::new("kit_row_rects")).unwrap_or_default())
}

struct Row {
    e: Entity,
    depth: usize,
    name: String,
    has_children: bool,
    icon: &'static str,
    parent: Option<Entity>,
    index: usize,
}

fn entity_icon(ed: &Editor, e: Entity) -> &'static str {
    match ed.entity_kind(e) {
        "CAMERA" => "🎥",
        "LIGHT" => "☀",
        "SPRITE" => "▣",
        "MODEL" => "◆",
        "SCRIPT" => "✏",
        "SOUND" => "♪",
        "VOLUME" => "✨",
        _ => "○",
    }
}

fn hierarchy(ui: &mut Ui, ed: &mut Editor, st: &mut UiState) {
    let mut actions = Vec::new();
    ui.horizontal(|ui| {
        widgets::section_label(ui, "OBJECTS");
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.add_enabled_ui(!ed.playing, |ui| {
                ui.menu_button(RichText::new("+").color(ACCENT()), |ui| {
                    for p in Preset::MENU {
                        if ui.button(p.label()).clicked() {
                            actions.push(Action::Spawn(p, None));
                            ui.close();
                        }
                    }
                })
                .response
                .on_hover_text("Add an object to this scene");
            });
        });
    });
    ui.add_space(4.0);

    // Flatten the tree.
    let mut rows = Vec::new();
    let w = ed.world();
    let mut stack: Vec<(Entity, usize, Option<Entity>, usize)> =
        kit::scene::roots(w).into_iter().enumerate().rev().map(|(i, e)| (e, 0, None, i)).collect();
    while let Some((e, depth, parent, index)) = stack.pop() {
        let kids = kit::scene::children(w, e);
        rows.push(Row {
            e,
            depth,
            name: ed.entity_label(e),
            has_children: !kids.is_empty(),
            icon: entity_icon(ed, e),
            parent,
            index,
        });
        if !st.collapsed.contains(&e) {
            for (i, k) in kids.into_iter().enumerate().rev() {
                stack.push((k, depth + 1, Some(e), i));
            }
        }
    }

    let avail = ui.available_height();
    // Only as tall as the rows need (up to half the panel), leaving room for assets.
    let tree_height = if st.views.assets {
        (rows.len() as f32 * 25.0 + 48.0).clamp(90.0, (avail * 0.5).max(90.0))
    } else {
        avail.max(0.0)
    };
    egui::ScrollArea::vertical().id_salt("tree").max_height(tree_height).auto_shrink([false, false]).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 1.0;
        for row in &rows {
            tree_row(ui, ed, st, row, &mut actions);
        }
        // Empty space: click to deselect, drop to move to the root level.
        let rest = ui.available_size().max(Vec2::new(0.0, 40.0));
        let (rect, resp) = ui.allocate_exact_size(rest, Sense::click());
        if resp.clicked() {
            actions.push(Action::Select(None));
        }
        if let Some(payload) = resp.dnd_release_payload::<Entity>() {
            actions.push(Action::Reparent(*payload, None, usize::MAX));
        }
        if resp.dnd_hover_payload::<Entity>().is_some() {
            ui.painter().rect_stroke(rect.shrink(2.0), 6.0, Stroke::new(1.0, ACCENT()), egui::StrokeKind::Inside);
        }
        resp.context_menu(|ui| {
            for p in Preset::MENU {
                if ui.button(format!("Add {}", p.label())).clicked() {
                    actions.push(Action::Spawn(p, None));
                    ui.close();
                }
            }
        });
    });
    apply(ed, st, actions);
}

fn tree_row(ui: &mut Ui, ed: &mut Editor, st: &UiState, row: &Row, actions: &mut Vec<Action>) {
    let e = row.e;
    let selected = ed.selected == Some(e);
    if ed.renaming.as_ref().is_some_and(|(re, _)| *re == e) {
        // Enter or clicking elsewhere commits; Esc cancels.
        let id = Id::new(("rename", e));
        let given_key = Id::new(("rename_focus_given", e));
        let focused = ui.memory(|m| m.has_focus(id));
        let given: bool = ui.ctx().data(|d| d.get_temp(given_key).unwrap_or(false));
        let enter = focused && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Enter));
        // egui drops focus on Escape before widgets run, so check the key itself.
        let esc = given && ui.input(|i| i.key_pressed(Key::Escape));
        ui.horizontal(|ui| {
            ui.add_space(row.depth as f32 * 14.0 + 18.0);
            let buf = &mut ed.renaming.as_mut().unwrap().1;
            let r = ui.add(egui::TextEdit::singleline(buf).id(id).desired_width(f32::INFINITY));
            if !given {
                // Focus once, with the whole name selected so typing replaces it.
                r.request_focus();
                let mut state = egui::text_edit::TextEditState::load(ui.ctx(), id).unwrap_or_default();
                let len = buf.chars().count();
                state.cursor.set_char_range(Some(egui::text::CCursorRange::two(
                    egui::text::CCursor::new(0),
                    egui::text::CCursor::new(len),
                )));
                state.store(ui.ctx(), id);
                ui.ctx().data_mut(|d| d.insert_temp(given_key, true));
            }
        });
        let clicked_away = given && !focused && !enter && !esc;
        if esc {
            ed.renaming = None;
        } else if enter || clicked_away {
            let (_, name) = ed.renaming.take().unwrap();
            let name = name.trim().to_string();
            if !name.is_empty() {
                if let Some(mut n) = ed.app.world.get_mut::<Name>(e) {
                    n.0 = name;
                }
            }
        }
        if ed.renaming.is_none() {
            ui.ctx().data_mut(|d| d.remove::<bool>(given_key));
        }
        return;
    }

    // One widget handles click (select), double-click (rename) and drag (reparent).
    // (egui's `dnd_drag_source` puts a drag-only area on top that swallows clicks.)
    let inner = {
        let width = ui.available_width();
        let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, 24.0), Sense::click_and_drag());
        let painter = ui.painter();
        if selected {
            painter.rect_filled(rect, 7.0, Color32::from_rgba_unmultiplied(0xb6, 0x9a, 0xf3, 0x2b));
            painter.rect_stroke(
                rect,
                7.0,
                Stroke::new(1.0, Color32::from_rgba_unmultiplied(0xbe, 0x9b, 0xff, 0x30)),
                egui::StrokeKind::Inside,
            );
        } else if resp.hovered() {
            painter.rect_filled(rect, 7.0, Color32::from_white_alpha(10));
        }
        let x0 = rect.left() + 6.0 + row.depth as f32 * 14.0;
        let cy = rect.center().y;
        if row.has_children {
            let arrow = if st.collapsed.contains(&e) { "▸" } else { "▾" };
            painter.text(
                egui::pos2(x0 + 5.0, cy),
                Align2::CENTER_CENTER,
                arrow,
                egui::FontId::proportional(11.0),
                MUTED(),
            );
        }
        painter.text(
            egui::pos2(x0 + 20.0, cy),
            Align2::CENTER_CENTER,
            row.icon,
            egui::FontId::proportional(12.0),
            if selected { ACCENT() } else { LABEL() },
        );
        // Long names (imported models) end in "…" instead of spilling past the card.
        let color = if selected { PRESSED_TEXT() } else { TEXT() };
        let mut job =
            egui::text::LayoutJob::simple_singleline(row.name.clone(), egui::FontId::proportional(13.0), color);
        job.wrap = egui::text::TextWrapping::truncate_at_width((rect.right() - 8.0 - (x0 + 32.0)).max(10.0));
        let galley = painter.layout_job(job);
        painter.galley(egui::pos2(x0 + 32.0, cy - galley.size().y / 2.0), galley, color);
        (resp, rect, x0)
    };
    let (resp, rect, x0) = inner;
    if resp.drag_started() && !ed.playing {
        egui::DragAndDrop::set_payload(ui.ctx(), e);
    }
    if resp.dragged() && egui::DragAndDrop::has_payload_of_type::<Entity>(ui.ctx()) {
        if let Some(pos) = ui.ctx().pointer_interact_pos() {
            let painter = ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Tooltip, Id::new("dnd_label")));
            let galley = painter.layout_no_wrap(row.name.clone(), egui::FontId::proportional(13.0), PRESSED_TEXT());
            let r = egui::Rect::from_min_size(pos + Vec2::new(12.0, 4.0), galley.size() + Vec2::new(16.0, 8.0));
            painter.rect_filled(r, 7.0, DIALOG_BG());
            painter.rect_stroke(r, 7.0, Stroke::new(1.0, ACCENT()), egui::StrokeKind::Inside);
            painter.galley(r.min + Vec2::new(8.0, 4.0), galley, PRESSED_TEXT());
        }
    }
    st_row_rect(ui, e, rect);

    if resp.clicked() {
        let on_arrow = row.has_children && resp.interact_pointer_pos().is_some_and(|p| p.x < x0 + 12.0);
        if on_arrow {
            actions.push(Action::ToggleCollapse(e));
        } else {
            actions.push(Action::Select(Some(e)));
        }
    }
    if resp.double_clicked() && !ed.playing {
        actions.push(Action::StartRename(e));
    }
    resp.context_menu(|ui| {
        if ui.button("Frame").clicked() {
            actions.push(Action::Frame(e));
            ui.close();
        }
        ui.add_enabled_ui(!ed.playing, |ui| {
            ui.menu_button("Add child", |ui| {
                for p in Preset::MENU {
                    if ui.button(p.label()).clicked() {
                        actions.push(Action::Spawn(p, Some(e)));
                        ui.close();
                    }
                }
            });
            if ui.button(format!("Rename    {}", ed.controls.label(Shortcut::Rename))).clicked() {
                actions.push(Action::StartRename(e));
                ui.close();
            }
            if ui.button(format!("Duplicate    {}", ed.controls.label(Shortcut::Duplicate))).clicked() {
                actions.push(Action::Duplicate(e));
                ui.close();
            }
            if ui.button(format!("Delete    {}", ed.controls.label(Shortcut::Delete))).clicked() {
                actions.push(Action::Delete(e));
                ui.close();
            }
        });
    });

    // Drop a sound from Assets onto an object to give it an Audio Source.
    if resp.dnd_hover_payload::<SoundDrag>().is_some() {
        ui.painter().rect_stroke(rect, 7.0, Stroke::new(1.5, ACCENT()), egui::StrokeKind::Inside);
    }
    // (Only take the payload when it's a sound: taking consumes any payload.)
    let carrying_sound = egui::DragAndDrop::has_payload_of_type::<SoundDrag>(ui.ctx());
    if let Some(sound) = carrying_sound.then(|| resp.dnd_release_payload::<SoundDrag>()).flatten() {
        actions.push(Action::AttachSound(e, sound.0.clone()));
    }

    // Drop target: top quarter = before, bottom quarter = after, middle = as child.
    let drop_resp = resp.clone();
    if let Some(dragged) = drop_resp.dnd_hover_payload::<Entity>() {
        if *dragged != e {
            let pos = ui.ctx().pointer_hover_pos().unwrap_or(rect.center());
            let t = (pos.y - rect.top()) / rect.height().max(1.0);
            let painter = ui.painter();
            if t < 0.25 {
                painter.hline(rect.x_range(), rect.top(), Stroke::new(2.0, ACCENT()));
            } else if t > 0.75 {
                painter.hline(rect.x_range(), rect.bottom(), Stroke::new(2.0, ACCENT()));
            } else {
                painter.rect_stroke(rect, 7.0, Stroke::new(1.5, ACCENT()), egui::StrokeKind::Inside);
            }
        }
    }
    if let Some(dragged) = drop_resp.dnd_release_payload::<Entity>() {
        let dragged = *dragged;
        if dragged != e && !ed.playing {
            let pos = ui.ctx().pointer_interact_pos().unwrap_or(rect.center());
            let t = (pos.y - rect.top()) / rect.height().max(1.0);
            if t < 0.25 || t > 0.75 {
                let mut index = row.index + (t > 0.75) as usize;
                // Moving down within the same parent shifts indices by one.
                let same_parent = ed.world().get::<kit::scene::Parent>(dragged).map(|p| p.0) == row.parent;
                if same_parent {
                    let siblings = match row.parent {
                        Some(p) => kit::scene::children(ed.world(), p),
                        None => kit::scene::roots(ed.world()),
                    };
                    if siblings.iter().position(|s| *s == dragged).is_some_and(|i| i < index) {
                        index -= 1;
                    }
                }
                actions.push(Action::Reparent(dragged, row.parent, index));
            } else {
                actions.push(Action::Reparent(dragged, Some(e), usize::MAX));
            }
        }
    }
}

fn assets(ui: &mut Ui, ed: &mut Editor, st: &mut UiState) {
    ui.horizontal(|ui| {
        widgets::section_label(ui, "ASSETS");
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let menu = ui.menu_button(RichText::new("+").color(ACCENT()), |ui| {
                let r = ui.button("New script…");
                mark(ui, "new_script_item", r.rect);
                if r.clicked() {
                    begin_new_asset(ed, st, AssetKind::Script);
                    ui.close();
                }
                let r = ui.button("New scene…");
                mark(ui, "new_scene_item", r.rect);
                if r.clicked() {
                    begin_new_asset(ed, st, AssetKind::Scene);
                    ui.close();
                }
                if ui
                    .add_enabled(ed.project.is_some(), egui::Button::new("Import files…"))
                    .on_hover_text(
                        "Sounds, sprites, 3D models, scripts, scenes and diagrams. You can also drag files onto Kit.",
                    )
                    .clicked()
                {
                    st.requests.push(Request::ImportModels);
                    ui.close();
                }
            });
            mark(ui, "assets_plus", menu.response.rect);
            menu.response.on_hover_text("Add an asset");
        });
    });
    ui.add_space(2.0);
    let current_scene = ed.scene_path.clone();
    egui::ScrollArea::vertical().id_salt("assets").auto_shrink([false, false]).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 1.0;
        if ed.project.is_none() {
            ui.label(RichText::new("No project open").color(MUTED()).size(12.0));
            return;
        }
        for group in &crate::state::ASSET_GROUPS {
            let files: Vec<String> = ed
                .assets
                .iter()
                .filter(|a| crate::state::asset_group(a).is_some_and(|g| g.name == group.name))
                .cloned()
                .collect();
            // Scenes and scripts always show (with their + button); other groups when they have files.
            let creatable = match group.name {
                "Scenes" => Some(AssetKind::Scene),
                "Scripts" => Some(AssetKind::Script),
                _ => None,
            };
            if files.is_empty() && creatable.is_none() && group.name != "Models" {
                continue;
            }
            let id = Id::new(("asset_group", group.name));
            let state = egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, true);
            let header = state.show_header(ui, |ui| {
                ui.label(RichText::new(group.icon).size(12.0).color(ACCENT()));
                ui.label(RichText::new(group.name).size(12.0).strong().color(TEXT_STRONG()));
                ui.label(RichText::new(files.len().to_string()).size(11.0).color(MUTED()));
                if let Some(kind) = creatable {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let r = ui
                            .add(egui::Button::new(RichText::new("+").size(12.0).color(ACCENT())).frame(false))
                            .on_hover_text(format!(
                                "New {}",
                                if kind == AssetKind::Scene { "scene" } else { "script" }
                            ));
                        mark(ui, if kind == AssetKind::Scene { "group_new_scene" } else { "group_new_script" }, r.rect);
                        if r.clicked() {
                            begin_new_asset(ed, st, kind);
                        }
                    });
                }
            });
            header.body(|ui| {
                if files.is_empty() {
                    if group.name == "Models" {
                        let import = ui.button("Import 3D models…");
                        mark(ui, "import_models", import.rect);
                        if import.clicked() {
                            st.requests.push(Request::ImportModels);
                        }
                    } else {
                        ui.label(RichText::new("None yet").size(11.0).color(MUTED()));
                    }
                }
                for path in files {
                    let is_current = current_scene.as_deref() == Some(path.as_str());
                    // File name first, folder dimmed after it.
                    let (dir, file) = match path.rsplit_once('/') {
                        Some((d, f)) => (format!("  {d}/"), f.to_string()),
                        None => (String::new(), path.clone()),
                    };
                    let mut job = egui::text::LayoutJob::default();
                    let importable = group.name == "Models"
                        && (crate::model_import::is_obj(std::path::Path::new(&path))
                            || kit::render::is_model_bundle(std::path::Path::new(&path)));
                    let color = if is_current {
                        PRESSED_TEXT()
                    } else if group.supported || importable {
                        LABEL()
                    } else {
                        MUTED()
                    };
                    job.append(
                        &file,
                        0.0,
                        egui::TextFormat { font_id: egui::FontId::proportional(12.0), color, ..Default::default() },
                    );
                    job.append(
                        &dir,
                        0.0,
                        egui::TextFormat {
                            font_id: egui::FontId::proportional(10.5),
                            color: MUTED(),
                            ..Default::default()
                        },
                    );
                    let sound = group.name == "Sounds";
                    let previewing = sound && ed.preview_playing() == Some(path.as_str());
                    let mut r = ui.add(egui::Button::selectable(is_current || previewing, job).frame_when_inactive(false));
                    if sound {
                        // Sounds can be dragged onto the viewport or onto an object.
                        r = r.interact(Sense::drag());
                        mark(ui, "sound_asset", r.rect);
                        if r.drag_started() && !ed.playing {
                            egui::DragAndDrop::set_payload(ui.ctx(), SoundDrag(path.clone()));
                        }
                        if r.dragged() && egui::DragAndDrop::has_payload_of_type::<SoundDrag>(ui.ctx()) {
                            if let Some(pos) = ui.ctx().pointer_interact_pos() {
                                let layer = egui::LayerId::new(egui::Order::Tooltip, Id::new("sound_drag"));
                                let painter = ui.ctx().layer_painter(layer);
                                let galley = painter.layout_no_wrap(
                                    format!("♪ {file}"),
                                    egui::FontId::proportional(13.0),
                                    PRESSED_TEXT(),
                                );
                                let r = egui::Rect::from_min_size(pos + Vec2::new(12.0, 4.0), galley.size() + Vec2::new(16.0, 8.0));
                                painter.rect_filled(r, 7.0, DIALOG_BG());
                                painter.rect_stroke(r, 7.0, Stroke::new(1.0, ACCENT()), egui::StrokeKind::Inside);
                                painter.galley(r.min + Vec2::new(8.0, 4.0), galley, PRESSED_TEXT());
                            }
                        }
                    }
                    r = if sound {
                        r.on_hover_text(format!(
                            "{path}\nClick to listen · double-click to add to the scene\nDrag onto the viewport or onto an object"
                        ))
                    } else if importable {
                        mark(ui, "model_asset", r.rect);
                        r.on_hover_text(format!("{path}\nDouble-click to add to the scene"))
                    } else if group.name == "Models" {
                        r.on_hover_text(format!("{path}\nDrop this file into Kit to convert it for the scene"))
                    } else if group.supported {
                        r.on_hover_text(&path)
                    } else {
                        r.on_hover_text(format!("{path}\n{} import is coming in a later version", group.name))
                    };
                    if r.clicked() || r.double_clicked() {
                        match group.name {
                            "Scripts" => ed.open_script(&path),
                            "Scenes" if !is_current => {
                                let res = ed.save_if_dirty().and_then(|_| ed.open_scene(&path));
                                ed.report(res);
                            }
                            "Sprites" => apply_texture(ed, &path),
                            "Models" if importable && r.double_clicked() => {
                                let result = ed.place_model(&path);
                                ed.report(result);
                            }
                            "Sounds" if r.double_clicked() => {
                                if ed.preview_playing().is_some() {
                                    ed.toggle_preview(&path);
                                }
                                if !ed.playing {
                                    ed.place_sound(&path, None);
                                }
                            }
                            "Sounds" => ed.toggle_preview(&path),
                            _ => {}
                        }
                    }
                }
                ui.add_space(4.0);
            });
        }
    });
}

/// Clicking an image while a sprite or model is selected assigns it as the texture.
fn apply_texture(ed: &mut Editor, path: &str) {
    let Some(e) = ed.selected.filter(|e| ed.world().is_alive(*e)) else {
        ed.notify("Select a sprite or model, then click an image to use it as its texture");
        return;
    };
    let w = &mut ed.app.world;
    if let Some(mut s) = w.get_mut::<Sprite>(e) {
        s.texture = path.to_string();
    } else if let Some(mut m) = w.get_mut::<Model>(e) {
        m.texture = path.to_string();
    } else {
        ed.notify("The selected object has no Sprite or Model to put a texture on");
        return;
    }
    ed.notify(format!("Texture set to {path}"));
}

pub(crate) fn begin_new_asset(ed: &mut Editor, st: &mut UiState, kind: AssetKind) {
    if ed.project.is_none() {
        ed.notify("Open or create a project first (Project menu)");
        return;
    }
    st.new_asset = Some((kind, String::new()));
    st.new_asset_focus = true;
}

/// Name dialog for a new script or scene: Enter or Create to confirm, Esc or Cancel to close.
fn new_asset_dialog(ctx: &egui::Context, ed: &mut Editor, st: &mut UiState) {
    let Some((kind, _)) = &st.new_asset else { return };
    let kind = *kind;
    let id = Id::new("new_asset_name");
    let focused = ctx.memory(|m| m.has_focus(id));
    // Read Enter before the text field consumes it.
    let enter = focused && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Enter));
    let mut create = enter;
    let mut cancel = false;
    let (title, hint, folder) = match kind {
        AssetKind::Script => ("New script", "e.g. enemy", "scripts/<name>.x"),
        AssetKind::Scene => ("New scene", "e.g. level2", "scenes/<name>.kscene"),
    };
    let resp = egui::Modal::new(Id::new("new_asset_modal"))
        .frame(
            Frame::new()
                .fill(DIALOG_BG())
                .stroke(Stroke::new(1.0, DIALOG_BORDER()))
                .corner_radius(CornerRadius::same(15))
                .inner_margin(Margin::same(22)),
        )
        .show(ctx, |ui| {
            ui.set_width(360.0);
            ui.label(RichText::new(title).size(19.0).strong());
            ui.add_space(4.0);
            ui.label(RichText::new(format!("Saved as {folder} in the project.")).size(12.0).color(MUTED()));
            ui.add_space(10.0);
            let name = &mut st.new_asset.as_mut().unwrap().1;
            let r = ui.add(
                egui::TextEdit::singleline(name)
                    .id(id)
                    .hint_text(hint)
                    .font(egui::FontId::proportional(15.0))
                    .desired_width(f32::INFINITY),
            );
            if st.new_asset_focus {
                r.request_focus();
                st.new_asset_focus = false;
            }
            ui.add_space(12.0);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let c = primary(ui, "Create", None);
                mark(ui, "new_asset_create", c.rect);
                if c.clicked() {
                    create = true;
                }
                if ghost(ui, "Cancel").clicked() {
                    cancel = true;
                }
            });
        });
    if resp.should_close() {
        cancel = true;
    }
    if cancel {
        st.new_asset = None;
        return;
    }
    if !create {
        return;
    }
    let name = st.new_asset.as_ref().unwrap().1.trim().to_string();
    if name.is_empty() {
        ed.notify("Type a name first");
        st.new_asset_focus = true;
        return;
    }
    st.new_asset = None;
    match kind {
        AssetKind::Script => {
            if let Some(rel) = ed.new_script(&name) {
                ed.console().info("editor", format!("created {rel}"));
            }
        }
        AssetKind::Scene => {
            let stem: String = name
                .trim_end_matches(".kscene")
                .chars()
                .map(|c| if c.is_alphanumeric() || c == '_' || c == '-' { c.to_ascii_lowercase() } else { '_' })
                .collect();
            let rel = format!("scenes/{stem}.kscene");
            let r = ed.save_if_dirty().and_then(|_| {
                ed.new_scene(&rel);
                ed.save_scene()
            });
            if r.is_ok() {
                ed.console().info("editor", format!("created {rel}"));
            }
            ed.report(r);
        }
    }
}

// ----------------------------------------------------------------- inspector

fn vec3_row(ui: &mut Ui, label: &str, v: &mut Vec3, speed: f32) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.add_sized([62.0, 20.0], egui::Label::new(RichText::new(label).size(12.0).color(LABEL())));
        let w = ((ui.available_width() - 16.0) / 3.0).max(30.0);
        for (i, color) in [AXIS_X(), AXIS_Y(), AXIS_Z()].into_iter().enumerate() {
            let axis = ["X", "Y", "Z"][i];
            let r = ui.add_sized(
                [w, 22.0],
                egui::DragValue::new(&mut v[i])
                    .speed(speed)
                    .max_decimals(3)
                    .prefix(RichText::new(format!("{axis} ")).color(color).text()),
            );
            changed |= r.changed();
        }
    });
    changed
}

fn color_row(ui: &mut Ui, label: &str, c: &mut Vec4) -> bool {
    let mut rgba = [
        (c.x.clamp(0.0, 1.0) * 255.0).round() as u8,
        (c.y.clamp(0.0, 1.0) * 255.0).round() as u8,
        (c.z.clamp(0.0, 1.0) * 255.0).round() as u8,
        (c.w.clamp(0.0, 1.0) * 255.0).round() as u8,
    ];
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.add_sized([62.0, 20.0], egui::Label::new(RichText::new(label).size(12.0).color(LABEL())));
        if ui.color_edit_button_srgba_unmultiplied(&mut rgba).changed() {
            *c = Vec4::new(rgba[0] as f32, rgba[1] as f32, rgba[2] as f32, rgba[3] as f32) / 255.0;
            changed = true;
        }
        ui.label(
            RichText::new(format!("#{:02x}{:02x}{:02x}", rgba[0], rgba[1], rgba[2]))
                .monospace()
                .size(11.0)
                .color(MUTED()),
        );
    });
    changed
}

fn texture_row(ui: &mut Ui, images: &[String], id: &str, tex: &mut String) {
    ui.horizontal(|ui| {
        ui.add_sized([62.0, 20.0], egui::Label::new(RichText::new("Texture").size(12.0).color(LABEL())));
        let shown = if tex.is_empty() { "None".to_string() } else { tex.clone() };
        egui::ComboBox::from_id_salt(id).selected_text(shown).width(ui.available_width() - 4.0).show_ui(ui, |ui| {
            ui.selectable_value(tex, String::new(), "None");
            for img in images {
                ui.selectable_value(tex, img.clone(), img.as_str());
            }
        });
    });
}

/// A collapsible component section with a remove button. Returns `false` when removed.
fn section(ui: &mut Ui, id: &str, title: &str, icon: &str, removable: bool, body: impl FnOnce(&mut Ui)) -> bool {
    let mut keep = true;
    let state =
        egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), Id::new(("section", id)), true);
    let header = state.show_header(ui, |ui| {
        ui.label(RichText::new(icon).color(ACCENT()));
        ui.label(RichText::new(title).strong().color(TEXT_STRONG()));
        if removable {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui
                    .add(egui::Button::new(RichText::new("✖").size(10.0).color(MUTED())).frame(false))
                    .on_hover_text("Remove component")
                    .clicked()
                {
                    keep = false;
                }
            });
        }
    });
    header.body(|ui| {
        ui.add_space(2.0);
        body(ui);
        ui.add_space(4.0);
    });
    ui.add_space(2.0);
    ui.separator();
    keep
}

/// Space kept free on the right of the inspector for its scroll bar.
const SCROLL_BAR_ROOM: f32 = 12.0;

/// Show a panel's scroll bar only after the mouse has rested over the panel
/// for a moment, or right away while scrolling, so it doesn't flicker in
/// over the controls when passing by.
fn delayed_scroll_bar(ui: &Ui, id: &str) -> egui::scroll_area::ScrollBarVisibility {
    const DELAY: f64 = 0.6;
    const LINGER: f64 = 1.2;
    let rect = ui.available_rect_before_wrap();
    let ctx = ui.ctx();
    let (now, hovered, scrolling) = ctx.input(|i| {
        let hovered = i.pointer.hover_pos().is_some_and(|p| rect.contains(p));
        (i.time, hovered, hovered && i.smooth_scroll_delta.y != 0.0)
    });
    let key = egui::Id::new(("scroll_bar_delay", id));
    let (mut since, mut last_scroll): (Option<f64>, f64) =
        ctx.data(|d| d.get_temp(key)).unwrap_or((None, f64::NEG_INFINITY));
    since = if hovered { Some(since.unwrap_or(now)) } else { None };
    if scrolling {
        last_scroll = now;
    }
    ctx.data_mut(|d| d.insert_temp(key, (since, last_scroll)));
    let rested = since.is_some_and(|t| now - t >= DELAY);
    let recent = now - last_scroll < LINGER;
    if let Some(t) = since.filter(|_| !rested) {
        ctx.request_repaint_after(std::time::Duration::from_secs_f64((t + DELAY - now).max(0.0)));
    }
    if recent && !rested {
        ctx.request_repaint_after(std::time::Duration::from_secs_f64(LINGER));
    }
    if rested || recent {
        egui::scroll_area::ScrollBarVisibility::VisibleWhenNeeded
    } else {
        egui::scroll_area::ScrollBarVisibility::AlwaysHidden
    }
}

fn inspector(ui: &mut Ui, ed: &mut Editor) {
    widgets::section_label(ui, "INSPECTOR");
    ui.add_space(6.0);
    let Some(e) = ed.selected.filter(|e| ed.world().is_alive(*e)) else {
        ui.add_space(20.0);
        ui.vertical_centered(|ui| {
            ui.label(RichText::new("Nothing selected").color(MUTED()));
            ui.label(RichText::new("Click an object in the viewport or the scene list.").size(11.0).color(MUTED()));
        });
        return;
    };
    let images = ed.assets_with(&["png", "jpg", "jpeg"]);
    let models = ed.assets_with(crate::model_import::EDITABLE_EXTENSIONS);
    let bar = delayed_scroll_bar(ui, "inspector");
    egui::ScrollArea::vertical().id_salt("inspector").auto_shrink([false, false]).scroll_bar_visibility(bar).show(
        ui,
        |ui| {
            // Keep content clear of the scroll bar so it never covers buttons.
            ui.set_max_width(ui.available_width() - SCROLL_BAR_ROOM);
            components(ui, ed, e, &images, &models);
            physics_sections(ui, ed, e);
            // Script section needs &mut Editor (opening files), so it is drawn separately.
            audio_section(ui, ed, e);
            volume_section(ui, ed, e);
            diagram_section(ui, ed, e);
            script_section(ui, ed, e);
            add_component_menu(ui, ed, e);
        },
    );
}

fn components(ui: &mut Ui, ed: &mut Editor, e: Entity, images: &[String], models: &[String]) {
    {
        let w = &mut ed.app.world;
        // Name
        if let Some(mut name) = w.get_mut::<Name>(e) {
            let r = ui.add(
                egui::TextEdit::singleline(&mut name.0)
                    .font(egui::FontId::proportional(15.0))
                    .desired_width(f32::INFINITY),
            );
            mark(ui, "inspector_name", r.rect);
        }
        ui.add_space(6.0);

        // Transform
        if w.has::<Transform>(e) {
            section(ui, "transform", "Transform", "⛶", false, |ui| {
                let mut t = *w.get::<Transform>(e).unwrap();
                let mut changed = false;
                changed |= vec3_row(ui, "Position", &mut t.translation, 0.02);
                let mut euler = t.euler_degrees();
                let euler_before = euler;
                if vec3_row(ui, "Rotation", &mut euler, 0.5) && euler != euler_before {
                    t.set_euler_degrees(euler);
                    changed = true;
                }
                changed |= vec3_row(ui, "Scale", &mut t.scale, 0.01);
                if changed {
                    *w.get_mut::<Transform>(e).unwrap() = t;
                }
            });
        }

        // Model
        if w.has::<Model>(e) {
            let keep = section(ui, "model", "Model", "◆", true, |ui| {
                let mut m = w.get_cloned::<Model>(e).unwrap();
                ui.horizontal(|ui| {
                    ui.add_sized([62.0, 20.0], egui::Label::new(RichText::new("Mesh").size(12.0).color(LABEL())));
                    // Imported models live at long paths: show just the file name, full path on hover.
                    let file_name = |path: &str| path.rsplit('/').next().unwrap_or(path).to_owned();
                    let label =
                        if m.mesh_asset.is_empty() { m.mesh.label().to_owned() } else { file_name(&m.mesh_asset) };
                    let combo = egui::ComboBox::from_id_salt("mesh")
                        .selected_text(label)
                        .width((ui.available_width() - 4.0).max(60.0))
                        .truncate()
                        .show_ui(ui, |ui| {
                            for kind in MeshKind::ALL {
                                if ui
                                    .selectable_label(m.mesh_asset.is_empty() && m.mesh == kind, kind.label())
                                    .clicked()
                                {
                                    m.mesh = kind;
                                    m.mesh_asset.clear();
                                    m.animation.clear();
                                }
                            }
                            for path in models {
                                let item =
                                    ui.selectable_label(m.mesh_asset == *path, file_name(path)).on_hover_text(path);
                                if item.clicked() {
                                    if m.mesh_asset != *path {
                                        m.animation.clear();
                                    }
                                    m.mesh_asset.clone_from(path);
                                }
                            }
                        });
                    if !m.mesh_asset.is_empty() {
                        combo.response.on_hover_text(&m.mesh_asset);
                    }
                });
                if kit::render::is_model_bundle(std::path::Path::new(&m.mesh_asset)) {
                    let asset = w.try_resource::<kit::resource::Resources>().and_then(|resources| {
                        kit::render::model_asset::ModelAsset::load(&resources, &m.mesh_asset).ok()
                    });
                    if let Some(asset) = asset.filter(|asset| !asset.clips.is_empty()) {
                        ui.horizontal(|ui| {
                            ui.checkbox(&mut m.animation_enabled, "Animate in play mode");
                            ui.checkbox(&mut m.animation_loop, "Loop");
                        });
                        if m.animation_enabled {
                            ui.horizontal(|ui| {
                                ui.label("Clip");
                                let name = if m.animation.is_empty() { &asset.clips[0].name } else { &m.animation };
                                egui::ComboBox::from_id_salt("animation_clip").selected_text(name).show_ui(ui, |ui| {
                                    for clip in &asset.clips {
                                        ui.selectable_value(&mut m.animation, clip.name.clone(), &clip.name);
                                    }
                                });
                            });
                            ui.horizontal(|ui| {
                                ui.label("Speed");
                                ui.add(
                                    egui::DragValue::new(&mut m.animation_speed)
                                        .range(0.0..=10.0)
                                        .speed(0.05)
                                        .suffix("×"),
                                );
                            });
                        }
                    }
                }
                color_row(ui, "Color", &mut m.color);
                texture_row(ui, images, "model_tex", &mut m.texture);
                ui.horizontal(|ui| {
                    ui.add_space(66.0);
                    ui.checkbox(&mut m.unlit, "Unlit");
                    ui.checkbox(&mut m.visible, "Visible");
                });
                if *w.get::<Model>(e).unwrap() != m {
                    *w.get_mut::<Model>(e).unwrap() = m;
                }
            });
            if !keep {
                w.remove::<Model>(e);
            }
        }

        // Sprite
        if w.has::<Sprite>(e) {
            let keep = section(ui, "sprite", "Sprite", "▣", true, |ui| {
                let mut s = w.get_cloned::<Sprite>(e).unwrap();
                let mut size = s.size.extend(0.0);
                ui.horizontal(|ui| {
                    ui.add_sized([62.0, 20.0], egui::Label::new(RichText::new("Size").size(12.0).color(LABEL())));
                    let half = ((ui.available_width() - 8.0) / 2.0).max(30.0);
                    ui.add_sized([half, 22.0], egui::DragValue::new(&mut size.x).speed(0.01).prefix("W "));
                    ui.add_sized([half, 22.0], egui::DragValue::new(&mut size.y).speed(0.01).prefix("H "));
                });
                s.size = size.truncate();
                color_row(ui, "Color", &mut s.color);
                texture_row(ui, images, "sprite_tex", &mut s.texture);
                ui.horizontal(|ui| {
                    ui.add_space(66.0);
                    ui.checkbox(&mut s.flip_x, "Flip X");
                    ui.checkbox(&mut s.flip_y, "Flip Y");
                    ui.checkbox(&mut s.visible, "Visible");
                });
                if *w.get::<Sprite>(e).unwrap() != s {
                    *w.get_mut::<Sprite>(e).unwrap() = s;
                }
            });
            if !keep {
                w.remove::<Sprite>(e);
            }
        }

        // Camera
        if w.has::<Camera>(e) {
            let keep = section(ui, "camera", "Camera", "🎥", true, |ui| {
                let mut c = *w.get::<Camera>(e).unwrap();
                let mut ortho = c.projection.is_orthographic();
                ui.horizontal(|ui| {
                    ui.add_sized([62.0, 20.0], egui::Label::new(RichText::new("Type").size(12.0).color(LABEL())));
                    ui.selectable_value(&mut ortho, false, "Perspective");
                    ui.selectable_value(&mut ortho, true, "2D / Ortho");
                });
                if ortho != c.projection.is_orthographic() {
                    c.projection =
                        if ortho { Camera::orthographic(10.0).projection } else { Camera::perspective().projection };
                }
                match &mut c.projection {
                    Projection::Perspective { fov_y, near, far } => {
                        ui.horizontal(|ui| {
                            ui.add_sized(
                                [62.0, 20.0],
                                egui::Label::new(RichText::new("FOV").size(12.0).color(LABEL())),
                            );
                            ui.add(egui::Slider::new(fov_y, 10.0..=120.0).suffix("°"));
                        });
                        ui.horizontal(|ui| {
                            ui.add_sized(
                                [62.0, 20.0],
                                egui::Label::new(RichText::new("Clip").size(12.0).color(LABEL())),
                            );
                            ui.add(egui::DragValue::new(near).speed(0.01).range(0.001..=10.0).prefix("near "));
                            ui.add(egui::DragValue::new(far).speed(1.0).range(1.0..=100000.0).prefix("far "));
                        });
                    }
                    Projection::Orthographic { height, .. } => {
                        ui.horizontal(|ui| {
                            ui.add_sized(
                                [62.0, 20.0],
                                egui::Label::new(RichText::new("Height").size(12.0).color(LABEL())),
                            );
                            ui.add(egui::DragValue::new(height).speed(0.05).range(0.1..=10000.0).suffix(" units"));
                        });
                    }
                }
                color_row(ui, "Clear", &mut c.clear_color);
                ui.horizontal(|ui| {
                    ui.add_space(66.0);
                    ui.checkbox(&mut c.active, "Active");
                });
                if *w.get::<Camera>(e).unwrap() != c {
                    *w.get_mut::<Camera>(e).unwrap() = c;
                }
            });
            if !keep {
                w.remove::<Camera>(e);
            }
        }

        // Light
        if w.has::<Light>(e) {
            let keep = section(ui, "light", "Light", "☀", true, |ui| {
                let mut l = *w.get::<Light>(e).unwrap();
                let mut c = l.color.extend(1.0);
                color_row(ui, "Color", &mut c);
                l.color = c.truncate();
                ui.horizontal(|ui| {
                    ui.add_sized([62.0, 20.0], egui::Label::new(RichText::new("Intensity").size(12.0).color(LABEL())));
                    ui.add(egui::Slider::new(&mut l.intensity, 0.0..=4.0));
                });
                ui.label(RichText::new("Shines along the entity's forward (−Z) axis.").size(11.0).color(MUTED()));
                if *w.get::<Light>(e).unwrap() != l {
                    *w.get_mut::<Light>(e).unwrap() = l;
                }
            });
            if !keep {
                w.remove::<Light>(e);
            }
        }
    }
}

fn label(ui: &mut Ui, text: &str) {
    ui.add_sized([62.0, 20.0], egui::Label::new(RichText::new(text).size(12.0).color(LABEL())));
}

/// Inherit / On / Off for one effect group; true when the group is On.
fn override_row(ui: &mut Ui, id: &str, title: &str, mode: &mut kit::gamesys::Override) -> bool {
    use kit::gamesys::Override;
    ui.horizontal(|ui| {
        ui.add_sized([62.0, 20.0], egui::Label::new(RichText::new(title).size(12.0).strong().color(TEXT_STRONG())));
        for m in Override::ALL {
            let r = ui.selectable_label(*mode == m, RichText::new(m.label()).size(11.5));
            if m == Override::On {
                mark(ui, leak_mark(format!("fx_{id}_on")), r.rect);
            }
            if r.on_hover_text(match m {
                Override::Inherit => "Keep what lower-priority volumes set",
                Override::On => "Use this volume's settings",
                Override::Off => "Switch this effect off here",
            })
            .clicked()
            {
                *mode = m;
            }
        }
    });
    *mode == Override::On
}

/// Marks need `'static` names; effect ids are a fixed small set.
fn leak_mark(name: String) -> &'static str {
    use std::collections::HashMap;
    use std::sync::Mutex;
    static NAMES: Mutex<Option<HashMap<String, &'static str>>> = Mutex::new(None);
    let mut names = NAMES.lock().unwrap();
    let names = names.get_or_insert_with(HashMap::new);
    names.entry(name.clone()).or_insert_with(|| Box::leak(name.into_boxed_str()))
}

fn slider(ui: &mut Ui, text: &str, value: &mut f32, range: std::ops::RangeInclusive<f32>) {
    ui.horizontal(|ui| {
        ui.add_space(14.0);
        ui.add_sized([62.0, 20.0], egui::Label::new(RichText::new(text).size(11.5).color(LABEL())));
        ui.add(egui::Slider::new(value, range).fixed_decimals(2));
    });
}

/// The Post-Processing Volume component: shape, blending, and per-effect overrides.
fn volume_section(ui: &mut Ui, ed: &mut Editor, e: Entity) {
    use kit::gamesys::{AoQuality, PostVolume, Tonemap, VolumeAffect, VolumeShape};
    let Some(mut v) = ed.world().get_cloned::<PostVolume>(e) else { return };
    let keep = section(ui, "post_volume", "Post-Processing Volume", "✨", true, |ui| {
        ui.horizontal(|ui| {
            label(ui, "Shape");
            for s in VolumeShape::ALL {
                let r = ui.selectable_label(v.shape == s, s.label());
                mark(ui, leak_mark(format!("volume_shape_{}", s.label().to_lowercase())), r.rect);
                if r.clicked() {
                    v.shape = s;
                }
            }
        });
        let local = v.shape != VolumeShape::Global;
        if local {
            ui.horizontal(|ui| {
                label(ui, "Affect");
                for a in VolumeAffect::ALL {
                    ui.selectable_value(&mut v.affect, a, a.label()).on_hover_text(match a {
                        VolumeAffect::Camera => "The whole picture changes while the camera is inside or near",
                        VolumeAffect::Objects => "Only what's inside the volume changes, seen from anywhere",
                    });
                }
            });
        }
        ui.horizontal(|ui| {
            label(ui, "Priority");
            ui.add(egui::DragValue::new(&mut v.priority).range(-1000..=1000))
                .on_hover_text("Higher wins where volumes overlap");
            if local {
                ui.label(RichText::new("Blend").size(12.0).color(LABEL()));
                ui.add(egui::DragValue::new(&mut v.blend_distance).range(0.0..=1000.0).speed(0.05).suffix(" m"))
                    .on_hover_text("World units outside the shape over which it fades in");
            }
        });
        ui.horizontal(|ui| {
            label(ui, "Weight");
            ui.add(egui::Slider::new(&mut v.weight, 0.0..=1.0).fixed_decimals(2));
        });
        ui.label(
            RichText::new(if local {
                "Size, position and rotation come from the Transform. Invisible in the game."
            } else {
                "Applies everywhere: scene-wide defaults that local volumes build on."
            })
            .size(11.0)
            .color(MUTED()),
        );
        ui.add_space(4.0);
        let fx = &mut v.effects;
        if override_row(ui, "color", "Color", &mut fx.color.mode) {
            slider(ui, "Saturation", &mut fx.color.saturation, 0.0..=2.0);
            slider(ui, "Contrast", &mut fx.color.contrast, 0.5..=1.5);
            slider(ui, "Temperature", &mut fx.color.temperature, -1.0..=1.0);
        }
        if override_row(ui, "exposure", "Exposure", &mut fx.exposure.mode) {
            slider(ui, "EV", &mut fx.exposure.ev, -5.0..=5.0);
            ui.horizontal(|ui| {
                ui.add_space(14.0);
                ui.add_sized([62.0, 20.0], egui::Label::new(RichText::new("Tone map").size(11.5).color(LABEL())));
                ui.selectable_value(&mut fx.exposure.tonemap, Tonemap::Filmic, "Filmic");
                ui.selectable_value(&mut fx.exposure.tonemap, Tonemap::None, "None");
            });
        }
        if override_row(ui, "glow", "Glow", &mut fx.glow.mode) {
            slider(ui, "Threshold", &mut fx.glow.threshold, 0.0..=4.0);
            slider(ui, "Intensity", &mut fx.glow.intensity, 0.0..=4.0);
            slider(ui, "Radius", &mut fx.glow.radius, 0.0..=1.0);
        }
        if override_row(ui, "focus", "Focus", &mut fx.focus.mode) {
            slider(ui, "Distance", &mut fx.focus.distance, 0.1..=200.0);
            slider(ui, "Range", &mut fx.focus.range, 0.1..=50.0);
            slider(ui, "Strength", &mut fx.focus.strength, 0.0..=1.0);
        }
        if override_row(ui, "ambient", "Ambient", &mut fx.ambient.mode) {
            slider(ui, "Radius", &mut fx.ambient.radius, 0.05..=5.0);
            slider(ui, "Strength", &mut fx.ambient.strength, 0.0..=4.0);
            ui.horizontal(|ui| {
                ui.add_space(14.0);
                ui.add_sized([62.0, 20.0], egui::Label::new(RichText::new("Quality").size(11.5).color(LABEL())));
                for q in AoQuality::ALL {
                    ui.selectable_value(&mut fx.ambient.quality, q, q.label());
                }
            });
        }
        if override_row(ui, "image", "Image", &mut fx.image.mode) {
            ui.horizontal(|ui| {
                ui.add_space(80.0);
                ui.checkbox(&mut fx.image.fxaa, "FXAA").on_hover_text("Smooths jagged edges (MSAA stays on)");
            });
            slider(ui, "Sharpen", &mut fx.image.sharpen, 0.0..=1.0);
        }
    });
    let w = &mut ed.app.world;
    if !keep {
        w.remove::<PostVolume>(e);
    } else if w.get::<PostVolume>(e).is_some_and(|old| *old != v) {
        *w.get_mut::<PostVolume>(e).unwrap() = v;
    }
}

/// The Audio Source component: clip with preview, volume/pitch, and proximity.
fn audio_section(ui: &mut Ui, ed: &mut Editor, e: Entity) {
    use kit::gamesys::{AudioCommand, AudioCommands, AudioSource, AudioState, Rolloff};
    let Some(mut a) = ed.world().get_cloned::<AudioSource>(e) else { return };
    let sounds = ed.assets_with(kit::gamesys::SOUND_EXTENSIONS);
    let previewing = ed.preview_playing().map(str::to_owned);
    let playing = ed.playing && ed.world().try_resource::<AudioState>().is_some_and(|s| s.playing.contains(&e));
    let mut preview = None;
    let mut command = None;
    let keep = section(ui, "audio", "Audio Source", "♪", true, |ui| {
        ui.horizontal(|ui| {
            label(ui, "Sound");
            let name = |p: &str| p.rsplit('/').next().unwrap_or(p).to_owned();
            let shown = if a.clip.is_empty() { "None".to_owned() } else { name(&a.clip) };
            let combo = egui::ComboBox::from_id_salt("audio_clip")
                .selected_text(shown)
                .width((ui.available_width() - 40.0).max(60.0))
                .truncate()
                .show_ui(ui, |ui| {
                    if sounds.is_empty() {
                        ui.label(RichText::new("Drop .wav / .mp3 / .ogg files into Kit").size(11.0).color(MUTED()));
                    }
                    for path in &sounds {
                        ui.selectable_value(&mut a.clip, path.clone(), name(path)).on_hover_text(path);
                    }
                });
            mark(ui, "audio_clip", combo.response.rect);
            let can_preview = !a.clip.is_empty();
            let is_previewing = previewing.as_deref() == Some(a.clip.as_str());
            let button = ui
                .add_enabled(
                    can_preview,
                    egui::Button::new(if is_previewing { "■" } else { "▶" }).min_size(Vec2::new(26.0, 22.0)),
                )
                .on_hover_text(if is_previewing { "Stop" } else { "Listen" });
            mark(ui, "audio_preview", button.rect);
            if button.clicked() {
                preview = Some(a.clip.clone());
            }
        });
        ui.horizontal(|ui| {
            label(ui, "Volume");
            ui.add(egui::Slider::new(&mut a.volume, 0.0..=2.0).fixed_decimals(2));
        });
        ui.horizontal(|ui| {
            label(ui, "Pitch");
            ui.add(egui::Slider::new(&mut a.pitch, 0.25..=3.0).logarithmic(true).fixed_decimals(2).suffix("×"));
        });
        ui.horizontal(|ui| {
            ui.add_space(66.0);
            ui.checkbox(&mut a.looping, "Loop");
            ui.checkbox(&mut a.play_on_start, "Play on start");
        });
        ui.add_space(2.0);
        ui.horizontal(|ui| {
            ui.add_space(66.0);
            let r = ui.checkbox(&mut a.spatial, "3D sound (proximity)");
            mark(ui, "audio_spatial", r.rect);
            r.on_hover_text("Louder near the listener (the active camera) and panned left/right");
        });
        if a.spatial {
            ui.horizontal(|ui| {
                label(ui, "Min dist");
                ui.add(egui::DragValue::new(&mut a.min_distance).range(0.0..=1000.0).speed(0.05).suffix(" m"))
                    .on_hover_text("Full volume within this distance");
                ui.label(RichText::new("Max").size(12.0).color(LABEL()));
                ui.add(egui::DragValue::new(&mut a.max_distance).range(0.1..=5000.0).speed(0.2).suffix(" m"))
                    .on_hover_text("Silent beyond this distance");
            });
            if a.max_distance <= a.min_distance {
                a.max_distance = a.min_distance + 0.1;
            }
            ui.horizontal(|ui| {
                label(ui, "Falloff");
                egui::ComboBox::from_id_salt("audio_rolloff").selected_text(a.rolloff.label()).show_ui(ui, |ui| {
                    for r in Rolloff::ALL {
                        ui.selectable_value(&mut a.rolloff, r, r.label());
                    }
                });
            });
            // The volume curve over distance.
            ui.horizontal(|ui| {
                ui.add_space(66.0);
                let (rect, _) =
                    ui.allocate_exact_size(Vec2::new(ui.available_width().min(220.0), 46.0), Sense::hover());
                let painter = ui.painter_at(rect.expand(1.0));
                painter.rect_filled(rect, 6.0, FIELD_BG());
                let span = a.max_distance * 1.15;
                let pts: Vec<egui::Pos2> = (0..=48)
                    .map(|i| {
                        let d = span * i as f32 / 48.0;
                        egui::pos2(
                            rect.left() + rect.width() * i as f32 / 48.0,
                            rect.bottom() - 4.0 - (rect.height() - 8.0) * a.attenuation(d),
                        )
                    })
                    .collect();
                painter.add(egui::Shape::line(pts, Stroke::new(1.8, ACCENT())));
                for (d, text) in [(a.min_distance, "min"), (a.max_distance, "max")] {
                    let x = rect.left() + rect.width() * d / span;
                    painter.line_segment(
                        [egui::pos2(x, rect.top() + 3.0), egui::pos2(x, rect.bottom() - 3.0)],
                        Stroke::new(1.0, MUTED()),
                    );
                    painter.text(
                        egui::pos2(x + 3.0, rect.top() + 3.0),
                        Align2::LEFT_TOP,
                        text,
                        egui::FontId::proportional(9.5),
                        MUTED(),
                    );
                }
            });
        } else {
            ui.horizontal(|ui| {
                ui.add_space(66.0);
                ui.label(RichText::new("Same volume everywhere (music, interface sounds).").size(11.0).color(MUTED()));
            });
        }
        if ed.playing {
            ui.horizontal(|ui| {
                ui.add_space(66.0);
                if ui.button(if playing { "■ Stop" } else { "▶ Play" }).clicked() {
                    command = Some(if playing { AudioCommand::Stop(e) } else { AudioCommand::Play(e) });
                }
                ui.label(RichText::new(if playing { "playing" } else { "stopped" }).size(11.0).color(MUTED()));
            });
        }
    });
    let w = &mut ed.app.world;
    if !keep {
        w.remove::<AudioSource>(e);
        return;
    }
    if w.get::<AudioSource>(e).is_some_and(|old| *old != a) {
        *w.get_mut::<AudioSource>(e).unwrap() = a;
    }
    if let Some(cmd) = command {
        if let Some(mut c) = w.try_resource_mut::<AudioCommands>() {
            c.0.push(cmd);
        }
    }
    if let Some(clip) = preview {
        ed.toggle_preview(&clip);
    }
}

fn terrain_section(ui: &mut Ui, ed: &mut Editor, e: Entity) {
    use kit::gamesys::{SculptTool, TERRAIN_RESOLUTIONS, Terrain};
    let Some(mut t) = ed.world().get_cloned::<Terrain>(e) else { return };
    let gen_id = Id::new(("terrain_gen", e));
    let (mut seed, mut height, mut rough): (u32, f32, f32) =
        ui.ctx().data(|d| d.get_temp(gen_id).unwrap_or((1, 6.0, 0.5)));
    let mut sculpt = ed.settings.sculpt;
    let mut radius = ed.settings.brush_radius;
    let mut strength = ed.settings.brush_strength;
    let keep = section(ui, "terrain", "Terrain", "⛰", true, |ui| {
        ui.label(RichText::new("SCULPT").size(10.0).color(MUTED()));
        ui.horizontal_wrapped(|ui| {
            if ui.selectable_label(sculpt.is_none(), "Off").clicked() {
                sculpt = None;
            }
            for tool in SculptTool::ALL {
                let r = ui.selectable_label(sculpt == Some(tool), tool.label());
                mark(
                    ui,
                    match tool {
                        SculptTool::Raise => "sculpt_raise",
                        SculptTool::Lower => "sculpt_lower",
                        SculptTool::Smooth => "sculpt_smooth",
                        SculptTool::Flatten => "sculpt_flatten",
                    },
                    r.rect,
                );
                if r.clicked() {
                    sculpt = Some(tool);
                }
            }
        });
        ui.horizontal(|ui| {
            label(ui, "Brush");
            ui.add(egui::DragValue::new(&mut radius).speed(0.05).range(0.25..=200.0).prefix("size "));
            ui.add(egui::DragValue::new(&mut strength).speed(0.01).range(0.01..=1.0).prefix("strength "));
        });
        if sculpt.is_some() {
            ui.label(
                RichText::new(format!(
                    "{}-drag to sculpt · {} reverses the brush",
                    ed.controls.gestures.sculpt.label(),
                    ed.controls.gestures.reverse_sculpt.label()
                ))
                .size(11.0)
                .color(ACCENT()),
            );
        }
        ui.add_space(4.0);
        ui.label(RichText::new("SHAPE").size(10.0).color(MUTED()));
        ui.horizontal(|ui| {
            label(ui, "Size");
            ui.add(egui::DragValue::new(&mut t.size).speed(0.25).range(1.0..=4096.0).suffix(" u"));
            let mut res = t.resolution;
            egui::ComboBox::from_id_salt("terrain_res").selected_text(format!("{res} × {res}")).show_ui(ui, |ui| {
                for r in TERRAIN_RESOLUTIONS {
                    ui.selectable_value(&mut res, r, format!("{r} × {r}"));
                }
            });
            if res != t.resolution {
                t.resample(res);
            }
        });
        ui.horizontal(|ui| {
            label(ui, "Generate");
            ui.add(egui::DragValue::new(&mut seed).prefix("seed "));
            ui.add(egui::DragValue::new(&mut height).speed(0.1).range(0.0..=500.0).prefix("height "));
        });
        ui.horizontal(|ui| {
            ui.add_space(66.0);
            ui.add(egui::Slider::new(&mut rough, 0.0..=1.0).text("rough"));
        });
        ui.horizontal(|ui| {
            ui.add_space(66.0);
            if ui.button("⛰ Generate hills").clicked() {
                t.generate(seed, height, rough);
            }
            if ui.button("🎲").on_hover_text("New random seed").clicked() {
                seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12_345) % 100_000;
                t.generate(seed, height, rough);
            }
            if ui.button("Flatten").clicked() {
                t.flatten_all(0.0);
            }
        });
        ui.add_space(4.0);
        ui.label(RichText::new("COLORS").size(10.0).color(MUTED()));
        color_row(ui, "Sand", &mut t.sand);
        color_row(ui, "Grass", &mut t.grass);
        color_row(ui, "Rock", &mut t.rock);
        color_row(ui, "Snow", &mut t.snow);
        ui.horizontal(|ui| {
            label(ui, "Levels");
            ui.add(egui::DragValue::new(&mut t.sand_height).speed(0.05).prefix("sand < "));
            ui.add(egui::DragValue::new(&mut t.snow_height).speed(0.05).prefix("snow > "));
        });
        ui.horizontal(|ui| {
            ui.add_space(66.0);
            ui.checkbox(&mut t.collide, "Collider").on_hover_text("Physics objects stand on it");
        });
    });
    ui.ctx().data_mut(|d| d.insert_temp(gen_id, (seed, height, rough)));
    ed.settings.sculpt = sculpt;
    ed.settings.brush_radius = radius;
    ed.settings.brush_strength = strength;
    if !keep {
        ed.app.world.remove::<Terrain>(e);
        ed.settings.sculpt = None;
    } else if ed.world().get::<Terrain>(e).is_some_and(|cur| *cur != t) {
        ed.app.world.insert(e, t);
    }
}

fn physics_sections(ui: &mut Ui, ed: &mut Editor, e: Entity) {
    terrain_section(ui, ed, e);
    use kit::gamesys::{BodyKind, Collider, ColliderShape, PathFollow, PathLoop, RigidBody};
    let mesh = ed.world().get::<Model>(e).map(|m| m.mesh);

    if let Some(mut c) = ed.world().get_cloned::<Collider>(e) {
        let keep = section(ui, "collider", "Collider", "⬚", true, |ui| {
            ui.horizontal(|ui| {
                label(ui, "Shape");
                egui::ComboBox::from_id_salt("collider_shape").selected_text(c.shape.label()).show_ui(ui, |ui| {
                    for shape in [
                        ColliderShape::Box { size: kit::math::Vec3::ONE },
                        ColliderShape::Sphere { radius: 0.5 },
                        ColliderShape::Capsule { height: 2.0, radius: 0.5 },
                        ColliderShape::Cylinder { height: 1.0, radius: 0.5 },
                    ] {
                        let label = shape.label();
                        if ui.selectable_label(c.shape.label() == label, label).clicked() {
                            c.shape = shape;
                        }
                    }
                });
                if let Some(mesh) = mesh {
                    if ui.small_button("Fit to mesh").clicked() {
                        c.shape = ColliderShape::fit(mesh);
                        c.offset = kit::math::Vec3::ZERO;
                    }
                }
            });
            match &mut c.shape {
                ColliderShape::Box { size } => {
                    vec3_row(ui, "Size", size, 0.01);
                }
                ColliderShape::Sphere { radius } => {
                    ui.horizontal(|ui| {
                        label(ui, "Radius");
                        ui.add(egui::DragValue::new(radius).speed(0.01).range(0.001..=1000.0));
                    });
                }
                ColliderShape::Capsule { height, radius } | ColliderShape::Cylinder { height, radius } => {
                    ui.horizontal(|ui| {
                        label(ui, "Size");
                        ui.add(egui::DragValue::new(height).speed(0.01).range(0.001..=1000.0).prefix("height "));
                        ui.add(egui::DragValue::new(radius).speed(0.01).range(0.001..=1000.0).prefix("radius "));
                    });
                }
            }
            vec3_row(ui, "Offset", &mut c.offset, 0.01);
            ui.horizontal(|ui| {
                label(ui, "Surface");
                ui.add(egui::DragValue::new(&mut c.friction).speed(0.01).range(0.0..=2.0).prefix("friction "));
                ui.add(egui::DragValue::new(&mut c.restitution).speed(0.01).range(0.0..=1.0).prefix("bounce "));
            });
            ui.horizontal(|ui| {
                ui.add_space(66.0);
                ui.checkbox(&mut c.trigger, "Trigger").on_hover_text("Reports overlaps (OnTrigger) but does not block");
            });
        });
        if keep && !ed.world().has::<RigidBody>(e) {
            ui.horizontal(|ui| {
                ui.add_space(66.0);
                if ui.button("⚛ Add Physics").on_hover_text("Make it fall, bounce and get pushed").clicked() {
                    ed.app.world.insert(e, RigidBody::default());
                }
            });
            ui.separator();
        }
        if !keep {
            ed.app.world.remove::<Collider>(e);
        } else if ed.world().get::<Collider>(e).is_some_and(|cur| *cur != c) {
            ed.app.world.insert(e, c);
        }
    }

    if let Some(mut b) = ed.world().get_cloned::<RigidBody>(e) {
        let has_collider = ed.world().has::<Collider>(e);
        let live = if ed.playing {
            ed.world().try_resource::<kit::gamesys::Physics>().and_then(|p| p.0.linear_velocity(e))
        } else {
            None
        };
        let mut open_settings = false;
        let keep = section(ui, "rigid_body", "Physics", "⚛", true, |ui| {
            ui.horizontal(|ui| {
                label(ui, "Type");
                for k in BodyKind::ALL {
                    ui.selectable_value(&mut b.kind, k, k.label());
                }
            });
            if b.kind == BodyKind::Dynamic {
                ui.horizontal(|ui| {
                    label(ui, "Mass");
                    ui.add(egui::DragValue::new(&mut b.mass).speed(0.05).range(0.001..=100000.0).suffix(" kg"));
                    ui.add(
                        egui::DragValue::new(&mut b.gravity_scale).speed(0.01).range(-10.0..=10.0).prefix("gravity ×"),
                    );
                });
                ui.horizontal(|ui| {
                    label(ui, "Damping");
                    ui.add(egui::DragValue::new(&mut b.linear_damping).speed(0.01).range(0.0..=100.0).prefix("move "));
                    ui.add(egui::DragValue::new(&mut b.angular_damping).speed(0.01).range(0.0..=100.0).prefix("spin "));
                });
            }
            ui.horizontal_wrapped(|ui| {
                ui.add_space(66.0);
                ui.checkbox(&mut b.lock_rotation, "Lock rotation");
                ui.checkbox(&mut b.lock_2d, "2D (XY plane)");
                ui.checkbox(&mut b.ccd, "Fast (CCD)");
            });
            if !has_collider {
                ui.label(RichText::new("Needs a Collider to take part in physics.").size(11.0).color(WARN()));
            }
            if let Some(v) = live {
                ui.horizontal(|ui| {
                    label(ui, "Velocity");
                    ui.label(
                        RichText::new(format!("{:.2}, {:.2}, {:.2}  ({:.2} u/s)", v.x, v.y, v.z, v.length()))
                            .monospace()
                            .size(11.0)
                            .color(TEXT_STRONG()),
                    );
                });
            }
            ui.horizontal(|ui| {
                ui.add_space(66.0);
                if ui.small_button("World settings…").on_hover_text("Gravity, step rate (Project settings)").clicked()
                {
                    open_settings = true;
                }
            });
        });
        if open_settings {
            ed.request_settings = true;
        }
        if !keep {
            ed.app.world.remove::<RigidBody>(e);
        } else if ed.world().get::<RigidBody>(e).is_some_and(|cur| *cur != b) {
            ed.app.world.insert(e, b);
        }
    }

    if let Some(mut p) = ed.world().get_cloned::<PathFollow>(e) {
        let mut sketch = false;
        let keep = section(ui, "path", "Path", "✏", true, |ui| {
            ui.horizontal(|ui| {
                label(ui, "Points");
                ui.label(RichText::new(format!("{}", p.points.len())).color(TEXT_STRONG()));
                if ui.button("✏ Sketch").on_hover_text("Canvas mode: drag in the viewport (Shift extends)").clicked()
                {
                    sketch = true;
                }
                if ui.button("Clear").clicked() {
                    p.points.clear();
                }
            });
            ui.horizontal(|ui| {
                label(ui, "Speed");
                ui.add(egui::DragValue::new(&mut p.speed).speed(0.05).range(0.01..=1000.0).suffix(" u/s"));
            });
            ui.horizontal(|ui| {
                label(ui, "At end");
                for m in PathLoop::ALL {
                    ui.selectable_value(&mut p.mode, m, m.label());
                }
            });
            ui.horizontal(|ui| {
                ui.add_space(66.0);
                ui.checkbox(&mut p.playing, "Play on start");
                ui.checkbox(&mut p.face_direction, "Face direction");
                ui.checkbox(&mut p.smooth, "Smooth").on_hover_text("Follow a smooth curve through the points");
            });
            ui.label(
                RichText::new("Follows the line as well as it can: colliders block it; physics objects fly it in 3D and land when it ends.")
                    .size(11.0)
                    .color(MUTED()),
            );
        });
        if sketch {
            ed.settings.canvas = true;
        }
        if !keep {
            ed.app.world.remove::<PathFollow>(e);
        } else if ed.world().get::<PathFollow>(e).is_some_and(|cur| *cur != p) {
            ed.app.world.insert(e, p);
        }
    }
}

fn script_section(ui: &mut Ui, ed: &mut Editor, e: Entity) {
    let Some(mut s) = ed.world().get_cloned::<Script>(e) else { return };
    let scripts = ed.assets_with(&["x"]);
    let mut open = None;
    let mut create = false;
    let keep = section(ui, "script", "Script", "✏", true, |ui| {
        ui.horizontal(|ui| {
            ui.add_sized([62.0, 20.0], egui::Label::new(RichText::new("File").size(12.0).color(LABEL())));
            let shown = if s.path.is_empty() { "None".to_string() } else { s.path.clone() };
            egui::ComboBox::from_id_salt("script_file").selected_text(shown).width(ui.available_width() - 4.0).show_ui(
                ui,
                |ui| {
                    ui.selectable_value(&mut s.path, String::new(), "None");
                    for p in &scripts {
                        ui.selectable_value(&mut s.path, p.clone(), p);
                    }
                },
            );
        });
        ui.horizontal(|ui| {
            ui.add_space(66.0);
            if !s.path.is_empty() && ui.button("Edit").clicked() {
                open = Some(s.path.clone());
            }
            if ui.button("New script").clicked() {
                create = true;
            }
        });
        if ed.playing {
            let mut rt = ed.world().resource_mut::<kit::script::ScriptRuntime>();
            if rt.has_error(e) {
                ui.label(RichText::new("Stopped after an error (see console)").size(11.0).color(ERROR()));
            }
            let fields = rt.fields(e);
            if !fields.is_empty() {
                ui.add_space(4.0);
                egui::Grid::new("script_fields").num_columns(3).spacing([10.0, 3.0]).show(ui, |ui| {
                    for f in fields {
                        field_label(ui, &f);
                        match (&f.access, f.value.clone()) {
                            // Live changes last until Play stops.
                            (kit::script::Access::Edit, Some(mut v)) => {
                                if field_editor(ui, &mut v) {
                                    rt.set_field(e, &f.name, &v);
                                }
                            }
                            _ => {
                                ui.label(RichText::new(&f.text).size(12.0).monospace().color(TEXT_STRONG()));
                            }
                        }
                        ui.end_row();
                    }
                });
                ui.label(RichText::new("Changes while playing are undone when you stop.").size(10.5).color(MUTED()));
            }
        } else if !s.path.is_empty() {
            let declared = {
                let res = ed.world().try_resource::<kit::resource::Resources>().map(|r| r.clone());
                res.map(|res| ed.world().resource_mut::<kit::script::ScriptRuntime>().declared(&res, &s.path))
            };
            if let Some(d) = declared {
                if let Some(err) = &d.error {
                    ui.label(RichText::new(format!("Can't read fields: {err}")).size(11.0).color(ERROR()));
                }
                if !d.fields.is_empty() {
                    ui.add_space(4.0);
                    egui::Grid::new("script_fields").num_columns(4).spacing([10.0, 3.0]).show(ui, |ui| {
                        for f in &d.fields {
                            field_label(ui, f);
                            match (&f.access, &f.value) {
                                (kit::script::Access::Edit, Some(default)) => {
                                    let mut v = s.values.get(&f.name).cloned().unwrap_or_else(|| default.clone());
                                    if field_editor(ui, &mut v) {
                                        if v == *default {
                                            s.values.remove(&f.name);
                                        } else {
                                            s.values.insert(f.name.clone(), v);
                                        }
                                    }
                                    if s.values.contains_key(&f.name) {
                                        if ui
                                            .small_button("↺")
                                            .on_hover_text(format!("Back to the script's default ({})", f.text))
                                            .clicked()
                                        {
                                            s.values.remove(&f.name);
                                        }
                                    } else {
                                        ui.label("");
                                    }
                                }
                                _ => {
                                    ui.label(RichText::new(&f.text).size(12.0).monospace().color(MUTED()))
                                        .on_hover_text(
                                            "PUBLIC: shown, not editable. Write EDIT in front to edit it here.",
                                        );
                                    ui.label("");
                                }
                            }
                            ui.end_row();
                        }
                    });
                } else if d.error.is_none() {
                    ui.label(
                        RichText::new("No editable fields. Put EDIT in front of a field to set it here (PUBLIC shows it read-only).")
                            .size(11.0)
                            .color(MUTED()),
                    );
                }
                // Saved values for fields that are gone or no longer EDIT.
                let stale: Vec<String> = s
                    .values
                    .keys()
                    .filter(|k| !d.fields.iter().any(|f| &f.name == *k && f.access == kit::script::Access::Edit))
                    .cloned()
                    .collect();
                if !stale.is_empty() && d.error.is_none() {
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(format!("Unused saved values: {}", stale.join(", ")))
                                .size(11.0)
                                .color(WARN()),
                        );
                        if ui.small_button("Remove").clicked() {
                            for k in &stale {
                                s.values.remove(k);
                            }
                        }
                    });
                }
            }
        }
    });
    if !keep {
        ed.app.world.remove::<Script>(e);
        return;
    }
    if create {
        let base = ed.entity_label(e).to_lowercase().replace(' ', "_");
        if let Some(rel) = ed.new_script(&base) {
            s.path = rel;
        }
    }
    if ed.world().get::<Script>(e).is_some_and(|cur| *cur != s) {
        *ed.app.world.get_mut::<Script>(e).unwrap() = s;
    }
    if let Some(p) = open {
        ed.open_script(&p);
    }
}

fn diagram_section(ui: &mut Ui, ed: &mut Editor, e: Entity) {
    let Some(mut d) = ed.world().get_cloned::<kit::gamesys::Diagram>(e) else { return };
    let graphs = ed.assets_with(&["kdiagram"]);
    let keep = section(ui, "diagram", "Diagram", "◫", true, |ui| {
        ui.horizontal(|ui| {
            ui.add_sized([62.0, 20.0], egui::Label::new(RichText::new("Graph").size(12.0).color(LABEL())));
            let shown = if d.graph.is_empty() { "None".to_string() } else { d.graph.clone() };
            egui::ComboBox::from_id_salt("diagram_file")
                .selected_text(shown)
                .width(ui.available_width() - 4.0)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut d.graph, String::new(), "None");
                    for g in &graphs {
                        ui.selectable_value(&mut d.graph, g.clone(), g);
                    }
                });
        });
        ui.horizontal(|ui| {
            ui.add_space(66.0);
            let label = if ed.settings.diagram { "Hide Diagram view" } else { "Open Diagram view" };
            if ui.button(format!("{label} ({})", ed.controls.label(Shortcut::Diagram))).clicked() {
                ed.settings.diagram = !ed.settings.diagram;
            }
        });
    });
    if !keep {
        ed.app.world.remove::<kit::gamesys::Diagram>(e);
    } else if ed.world().get::<kit::gamesys::Diagram>(e).is_some_and(|cur| *cur != d) {
        *ed.app.world.get_mut::<kit::gamesys::Diagram>(e).unwrap() = d;
    }
}

/// Type and name of a script field.
fn field_label(ui: &mut Ui, f: &kit::script::FieldView) {
    ui.label(RichText::new(&f.ty).size(10.0).color(CODE_TYPE()).monospace());
    ui.label(RichText::new(&f.name).size(12.0).color(LABEL()));
}

/// An input for an EDIT field's value. Returns true when it changed.
fn field_editor(ui: &mut Ui, v: &mut kit::gamesys::FieldValue) -> bool {
    use kit::gamesys::FieldValue as F;
    match v {
        F::Bool(b) => ui.checkbox(b, "").changed(),
        F::Int(i) => ui.add(egui::DragValue::new(i).speed(0.1)).changed(),
        F::Float(f) => ui.add(egui::DragValue::new(f).speed(0.05).max_decimals(3)).changed(),
        F::Text(t) => ui.add(egui::TextEdit::singleline(t).desired_width(140.0)).changed(),
        F::Vector(a) => {
            ui.horizontal(|ui| {
                let mut changed = false;
                for x in a.iter_mut() {
                    changed |= ui.add(egui::DragValue::new(x).speed(0.05).max_decimals(3)).changed();
                }
                changed
            })
            .inner
        }
    }
}

fn add_component_menu(ui: &mut Ui, ed: &mut Editor, e: Entity) {
    let mut sketch_now = false;
    add_component_menu_inner(ui, ed, e, &mut sketch_now);
    if sketch_now {
        ed.settings.canvas = true;
    }
}

fn add_component_menu_inner(ui: &mut Ui, ed: &mut Editor, e: Entity, sketch_now: &mut bool) {
    let mut add_diagram = false;
    add_component_menu_body(ui, ed, e, sketch_now, &mut add_diagram);
    if add_diagram && crate::diagram_view::add_diagram(ed, e).is_some() {
        ed.settings.diagram = true;
    }
}

fn add_component_menu_body(ui: &mut Ui, ed: &mut Editor, e: Entity, sketch_now: &mut bool, add_diagram: &mut bool) {
    ui.add_space(8.0);
    ui.vertical_centered(|ui| {
        ui.menu_button(RichText::new("+  Add component").color(ACCENT()), |ui| {
            let w = &mut ed.app.world;
            let add = |ui: &mut Ui, label: &str, has: bool, f: &mut dyn FnMut()| {
                if ui.add_enabled(!has, egui::Button::new(label)).clicked() {
                    f();
                    ui.close();
                }
            };
            let has_model = w.has::<Model>(e);
            add(ui, "Model", has_model, &mut || {
                w.insert(e, Model::default());
            });
            let has_sprite = w.has::<Sprite>(e);
            add(ui, "Sprite", has_sprite, &mut || {
                w.insert(e, Sprite::default());
            });
            let has_cam = w.has::<Camera>(e);
            add(ui, "Camera", has_cam, &mut || {
                w.insert(e, Camera::perspective());
            });
            let has_light = w.has::<Light>(e);
            add(ui, "Light", has_light, &mut || {
                w.insert(e, Light::default());
            });
            let has_diagram = w.has::<kit::gamesys::Diagram>(e);
            if ui.add_enabled(!has_diagram, egui::Button::new("Diagram (visual logic)")).clicked() {
                *add_diagram = true;
                ui.close();
            }
            let has_script = w.has::<Script>(e);
            add(ui, "Script", has_script, &mut || {
                w.insert(e, Script::default());
            });
            ui.separator();
            let mesh = w.get::<Model>(e).map(|m| m.mesh);
            let fit = || kit::gamesys::Collider {
                shape: mesh
                    .map(kit::gamesys::ColliderShape::fit)
                    .unwrap_or(kit::gamesys::ColliderShape::Box { size: Vec3::ONE }),
                ..Default::default()
            };
            let has_collider = w.has::<kit::gamesys::Collider>(e);
            add(ui, "⬚ Collider only (touch / trigger)", has_collider, &mut || {
                w.insert(e, fit());
            });
            let has_body = w.has::<kit::gamesys::RigidBody>(e);
            add(ui, "⚛ Physics (rigid body + collider)", has_body, &mut || {
                w.insert(e, kit::gamesys::RigidBody::default());
                if !has_collider {
                    w.insert(e, fit());
                }
            });
            let has_terrain = w.has::<kit::gamesys::Terrain>(e);
            add(ui, "⛰ Terrain", has_terrain, &mut || {
                let mut t = kit::gamesys::Terrain::default();
                t.generate(1, 6.0, 0.5);
                w.insert(e, t);
            });
            let has_volume = w.has::<kit::gamesys::PostVolume>(e);
            add(ui, "✨ Post-Processing Volume", has_volume, &mut || {
                w.insert(e, kit::gamesys::PostVolume::default());
            });
            let has_audio = w.has::<kit::gamesys::AudioSource>(e);
            add(ui, "♪ Audio Source", has_audio, &mut || {
                let spatial = !w.has::<Camera>(e);
                w.insert(e, kit::gamesys::AudioSource { spatial, ..Default::default() });
            });
            let has_path = w.has::<kit::gamesys::PathFollow>(e);
            add(ui, "✏ Path (sketch in Canvas)", has_path, &mut || {
                w.insert(e, kit::gamesys::PathFollow::default());
                *sketch_now = true;
            });
        });
    });
}

// ------------------------------------------------------------------ viewport

fn viewport(ui: &mut Ui, ed: &mut Editor, st: &mut UiState) {
    let rect = ui.available_rect_before_wrap();
    let resp = ui.allocate_rect(rect, Sense::click_and_drag());
    st.viewport_rect = rect;
    let ppp = ui.ctx().pixels_per_point();
    st.viewport_px = [(rect.width() * ppp).round().max(1.0) as u32, (rect.height() * ppp).round().max(1.0) as u32];

    let painter = ui.painter_at(rect);
    let radius = CornerRadius::same(16);
    match st.viewport_tex {
        Some(tex) => {
            egui::Image::new(egui::load::SizedTexture::new(tex, rect.size())).corner_radius(radius).paint_at(ui, rect);
        }
        None => {
            painter.rect_filled(rect, radius, STAGE_BG());
        }
    }
    let border = if ed.playing && st.viewport_focused { ACCENT() } else { STAGE_BORDER() };
    painter.rect_stroke(rect, radius, Stroke::new(1.0, border), egui::StrokeKind::Inside);

    // "● VIEWPORT" label.
    let label_pos = rect.left_top() + Vec2::new(22.0, 24.0);
    painter.circle_filled(label_pos, 6.0, Color32::from_rgba_unmultiplied(0xbb, 0xa2, 0xff, 0x30));
    painter.circle_filled(label_pos, 3.0, ACCENT());
    let label = if ed.playing {
        "PLAYING"
    } else if ed.camera.two_d {
        "VIEWPORT · 2D"
    } else {
        "VIEWPORT"
    };
    let galley = ui.painter().layout_job(widgets::spaced(label, 10.0, crate::theme::viewport::LABEL()));
    painter.galley(label_pos + Vec2::new(12.0, -galley.size().y / 2.0), galley, crate::theme::viewport::LABEL());

    // Camera controls.
    let gestures = &ed.controls.gestures;
    let select_button = gestures.select.egui();
    let pan_button = gestures.pan.egui();
    let alternate_pan = gestures.alternate_pan.egui();
    let orbit_button = gestures.orbit.egui();
    let alternate_orbit = gestures.alternate_orbit;
    let canvas_restart = gestures.canvas_restart.label();
    let using_game_cam = ed.playing && ed.settings.game_camera;
    let aspect = rect.width() / rect.height().max(1.0);
    let editor_view = ed.camera.view(aspect);
    if resp.hovered() && !using_game_cam && gestures.scroll_zoom {
        let direction = if gestures.invert_scroll { -1.0 } else { 1.0 };
        let scroll = ui.ctx().input(|i| i.smooth_scroll_delta.y) * direction;
        if scroll != 0.0 {
            ed.camera.zoom(scroll / 50.0);
        }
    }
    if resp.drag_started() {
        st.pointer_moved = 0.0;
    }
    // Move gizmo: drag the colored arrows to move the selection along X / Y / Z.
    let moving = !using_game_cam && crate::move_gizmo::handle_input(&mut st.gizmo, &resp, ui, ed, rect, &editor_view);
    // Terrain sculpting: left-drag on a selected terrain; right-drag orbits.
    let sculpting_mode = ed.settings.sculpt.is_some()
        && !ed.playing
        && ed.selected.is_some_and(|e| ed.world().has::<kit::gamesys::Terrain>(e));
    let sculpted =
        sculpting_mode && crate::terrain_tools::handle_input(&resp, ui, ed, &mut st.brush, rect, &editor_view);
    // Canvas mode: left-drag sketches a path for the selection; right-drag orbits.
    let canvas = (ed.settings.canvas || sculpting_mode) && !ed.playing && ed.selected.is_some();
    let sketching = sculpted
        || (ed.settings.canvas
            && canvas
            && crate::canvas::handle_input(&resp, ui, ed, &mut st.sketch, rect, &editor_view));
    if resp.dragged() && !using_game_cam && !sketching && !moving {
        let d = resp.drag_delta();
        st.pointer_moved += d.length();
        let d = kit::math::Vec2::new(d.x, d.y);
        let alternate = ui.input(|i| alternate_orbit.held(i.modifiers));
        let orbit = if canvas {
            resp.dragged_by(orbit_button) && !alternate
        } else {
            resp.dragged_by(orbit_button) || alternate && resp.dragged_by(pan_button)
        };
        let pan = resp.dragged_by(pan_button)
            || resp.dragged_by(alternate_pan)
            || canvas && alternate && resp.dragged_by(orbit_button);
        if orbit && !ed.camera.two_d {
            ed.camera.orbit(d);
        } else if pan || orbit {
            ed.camera.pan(d, rect.height());
        }
    }
    if resp.clicked_by(select_button) && !sculpting_mode && !moving {
        if ed.playing {
            st.viewport_focused = true;
        } else if let Some(pos) = resp.interact_pointer_pos() {
            let ndc = kit::math::Vec2::new(
                (pos.x - rect.left()) / rect.width() * 2.0 - 1.0,
                (pos.y - rect.top()) / rect.height() * 2.0 - 1.0,
            );
            let ray = kit::render::pick::Ray::from_ndc(editor_view.view_proj(), ndc);
            ed.selected = kit::render::pick::pick(ed.world(), &ray);
        }
    }
    if resp.double_clicked_by(select_button) && !ed.playing && ed.selected.is_some() {
        ed.frame_selected();
    }
    // A sound dragged from Assets: show where it lands, place it on release.
    if resp.dnd_hover_payload::<SoundDrag>().is_some() {
        painter.rect_stroke(rect.shrink(2.0), radius, Stroke::new(2.0, ACCENT()), egui::StrokeKind::Inside);
    }
    let carrying_sound = egui::DragAndDrop::has_payload_of_type::<SoundDrag>(ui.ctx());
    if let Some(sound) = carrying_sound.then(|| resp.dnd_release_payload::<SoundDrag>()).flatten() {
        if let Some(pos) = ui.ctx().pointer_interact_pos().or(resp.hover_pos()) {
            let at = viewport_point(ed, st, pos);
            ed.place_sound(&sound.0, Some(at));
        }
    }
    if ui.ctx().input(|i| i.pointer.any_pressed()) && !resp.hovered() {
        st.viewport_focused = false;
    }

    // Paths, colliders and the live sketch, drawn over the 3D view.
    if !using_game_cam {
        let painter = ui.painter_at(rect);
        crate::canvas::draw_overlays(&painter, ed, &st.sketch, rect, &editor_view);
        crate::gizmos::draw(&painter, ed, rect, &editor_view);
        crate::move_gizmo::draw(&painter, &st.gizmo, ed, rect, &editor_view);
        if let Some(segs) = crate::move_gizmo::screen_handles(ed, &editor_view, rect) {
            let dot = |p| egui::Rect::from_center_size(p, Vec2::splat(4.0));
            for (i, a, b) in segs {
                mark(ui, ["gizmo_x_base", "gizmo_y_base", "gizmo_z_base"][i], dot(a));
                mark(ui, ["gizmo_x_tip", "gizmo_y_tip", "gizmo_z_tip"][i], dot(b));
            }
            #[cfg(test)]
            for (i, name) in ["gizmo_ring_x", "gizmo_ring_y", "gizmo_ring_z"].into_iter().enumerate() {
                if let Some(p) = crate::move_gizmo::ring_grab_point(ed, &editor_view, rect, i) {
                    mark(ui, name, dot(p));
                }
            }
        }
        if sculpting_mode {
            crate::terrain_tools::draw_brush(&painter, ui, ed, &st.brush, rect, &editor_view);
        }
    }
    camera_preview(ui, ed, st, rect);
    if ed.settings.canvas && !ed.playing {
        let text = match ed.selected.filter(|e| ed.world().is_alive(*e)) {
            Some(e) => format!(
                "Drag to draw {}'s path · rotate the view and drag again to continue · {canvas_restart} starts over · drag a dot to move it",
                ed.entity_label(e)
            ),
            None => "Select an object, then drag to draw its path".to_string(),
        };
        egui::Area::new(Id::new("canvas_hint"))
            .fixed_pos(rect.center_top() + Vec2::new(0.0, 16.0))
            .pivot(Align2::CENTER_TOP)
            .order(egui::Order::Middle)
            .show(ui.ctx(), |ui| {
                Frame::new()
                    .fill(NOTICE_BG())
                    .stroke(Stroke::new(1.0, ACCENT()))
                    .corner_radius(CornerRadius::same(9))
                    .inner_margin(Margin::symmetric(14, 8))
                    .show(ui, |ui| {
                        ui.set_max_width((rect.width() - 260.0).max(320.0));
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("✏ Canvas").size(12.0).strong().color(ACCENT()));
                            ui.label(RichText::new("Draw on").size(11.5).color(MUTED()));
                            let view_r =
                                ui.selectable_label(!ed.settings.canvas_ground, RichText::new("View plane").size(11.5));
                            mark(ui, "canvas_view_plane", view_r.rect);
                            if view_r
                                .on_hover_text("A flat sheet facing the camera, through the end of the path")
                                .clicked()
                            {
                                ed.settings.canvas_ground = false;
                            }
                            let ground_r =
                                ui.selectable_label(ed.settings.canvas_ground, RichText::new("Ground").size(11.5));
                            mark(ui, "canvas_ground", ground_r.rect);
                            if ground_r.on_hover_text("On the ground / terrain at the object's height").clicked() {
                                ed.settings.canvas_ground = true;
                            }
                        });
                        ui.label(RichText::new(text).size(11.5).color(PRESSED_TEXT()));
                    });
            });
    }

    // Top-right actions.
    egui::Area::new(Id::new("viewport_actions"))
        .fixed_pos(rect.right_top() + Vec2::new(-16.0, 14.0))
        .pivot(Align2::RIGHT_TOP)
        .order(egui::Order::Middle)
        .show(ui.ctx(), |ui| {
            ui.horizontal(|ui| {
                let two_d = ed.camera.two_d;
                if viewport_ghost(ui, RichText::new(if two_d { "2D" } else { "3D" }).size(13.0).strong().text())
                    .on_hover_text(format!("Toggle 2D / 3D ({})", ed.controls.label(Shortcut::Dimension)))
                    .clicked()
                {
                    ed.camera.set_two_d(!two_d);
                }
                if viewport_ghost(ui, RichText::new("⛶").size(18.0).text())
                    .on_hover_text(format!("Frame selection ({})", ed.controls.label(Shortcut::Frame)))
                    .clicked()
                {
                    ed.frame_selected();
                }
                let rot = if ed.camera.auto_rotate { "⟳" } else { "↻" };
                if viewport_ghost(ui, RichText::new(rot).size(18.0).text())
                    .on_hover_text(format!("Auto-rotate ({})", ed.controls.label(Shortcut::Rotate)))
                    .clicked()
                {
                    ed.camera.auto_rotate = !ed.camera.auto_rotate;
                }
            });
        });

    // Bottom dock.
    egui::Area::new(Id::new("viewport_dock"))
        .fixed_pos(rect.center_bottom() - Vec2::new(0.0, 20.0))
        .pivot(Align2::CENTER_BOTTOM)
        .order(egui::Order::Middle)
        .show(ui.ctx(), |ui| {
            Frame::new()
                .fill(DOCK_BG())
                .stroke(Stroke::new(1.0, DOCK_BORDER()))
                .corner_radius(CornerRadius::same(14))
                .inner_margin(Margin::same(5))
                .shadow(egui::Shadow { offset: [0, 12], blur: 35, spread: 0, color: Color32::from_black_alpha(110) })
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        widgets::logo(ui, 26.0).on_hover_text("Kit");
                        let s = &mut ed.settings;
                        if dock_toggle(ui, "◇", "Wireframe", s.wireframe)
                            .on_hover_text(format!("Wireframe ({})", ed.controls.label(Shortcut::Wireframe)))
                            .clicked()
                        {
                            s.wireframe = !s.wireframe;
                        }
                        if dock_toggle(ui, "☀", "Lighting", s.lighting)
                            .on_hover_text(format!("Lighting ({})", ed.controls.label(Shortcut::Lighting)))
                            .clicked()
                        {
                            s.lighting = !s.lighting;
                        }
                        if dock_toggle(ui, "#", "Grid", s.grid)
                            .on_hover_text(format!("Grid ({})", ed.controls.label(Shortcut::Grid)))
                            .clicked()
                        {
                            s.grid = !s.grid;
                        }
                        if dock_toggle(ui, "⬚", "Colliders", s.colliders)
                            .on_hover_text(format!(
                                "Show collider outlines ({})",
                                ed.controls.label(Shortcut::Colliders)
                            ))
                            .clicked()
                        {
                            s.colliders = !s.colliders;
                        }
                        if !ed.playing
                            && dock_toggle(ui, "✏", "Canvas", s.canvas)
                                .on_hover_text(format!(
                                    "Sketch movement paths ({})",
                                    ed.controls.label(Shortcut::Canvas)
                                ))
                                .clicked()
                        {
                            s.canvas = !s.canvas;
                        }
                        let r = dock_toggle(ui, "✨", "Volumes", s.volumes).on_hover_text(format!(
                            "Preview post-processing volumes ({})",
                            ed.controls.label(Shortcut::Volumes)
                        ));
                        mark(ui, "toggle_volumes", r.rect);
                        if r.clicked() {
                            s.volumes = !s.volumes;
                        }
                        let r = dock_toggle(ui, "◫", "Diagram", s.diagram).on_hover_text(format!(
                            "Visual logic for the selected object ({})",
                            ed.controls.label(Shortcut::Diagram)
                        ));
                        mark(ui, "toggle_diagram", r.rect);
                        if r.clicked() {
                            st.toggle_pane(ed, ViewPane::Diagram);
                        }
                        let r = dock_toggle(ui, "●", "Rex", ed.settings.rex).on_hover_text(format!(
                            "Rex, your AI game-making puppy ({})",
                            ed.controls.label(Shortcut::Rex)
                        ));
                        mark(ui, "toggle_rex", r.rect);
                        if r.clicked() {
                            st.toggle_pane(ed, ViewPane::Rex);
                        }
                        if ed.playing && dock_toggle(ui, "🎥", "Game camera", ed.settings.game_camera).clicked() {
                            ed.settings.game_camera = !ed.settings.game_camera;
                        }
                    });
                });
        });

    if ed.playing && !st.viewport_focused {
        egui::Area::new(Id::new("click_to_play"))
            .fixed_pos(rect.center_top() + Vec2::new(0.0, 18.0))
            .pivot(Align2::CENTER_TOP)
            .order(egui::Order::Middle)
            .interactable(false)
            .show(ui.ctx(), |ui| {
                Frame::new()
                    .fill(NOTICE_BG())
                    .stroke(Stroke::new(1.0, NOTICE_BORDER()))
                    .corner_radius(CornerRadius::same(9))
                    .inner_margin(Margin::symmetric(14, 8))
                    .show(ui, |ui| {
                        ui.label(RichText::new("Click the viewport to give the game keyboard input").size(12.0));
                    });
            });
    }
}

/// Bottom-right preview of what the selected camera sees.
fn camera_preview(ui: &mut Ui, ed: &Editor, st: &mut UiState, rect: egui::Rect) {
    let cam = ed.selected.filter(|e| !ed.playing && ed.world().is_alive(*e) && ed.world().has::<Camera>(*e));
    st.preview_camera = cam;
    let Some(cam) = cam else {
        st.preview_px = [0, 0];
        return;
    };
    let w = (rect.width() * 0.3).clamp(180.0, 420.0);
    let h = w * 9.0 / 16.0;
    let ppp = ui.ctx().pixels_per_point();
    st.preview_px = [(w * ppp).round() as u32, (h * ppp).round() as u32];
    let frame_rect = egui::Rect::from_min_size(
        // Above the dock (bottom center) so the two never overlap.
        egui::pos2(rect.right() - w - 18.0, rect.bottom() - h - 22.0 - 78.0),
        Vec2::new(w, h + 22.0),
    );
    let painter = ui.painter_at(rect);
    painter.rect_filled(frame_rect.expand(1.0), 12.0, DOCK_BG());
    painter.rect_stroke(frame_rect.expand(1.0), 12.0, Stroke::new(1.0, DOCK_BORDER()), egui::StrokeKind::Outside);
    let title = format!("🎥  {}", ed.entity_label(cam));
    painter.text(
        frame_rect.left_top() + Vec2::new(10.0, 11.0),
        Align2::LEFT_CENTER,
        title,
        egui::FontId::proportional(11.5),
        LABEL(),
    );
    let active = ed.world().get::<Camera>(cam).is_some_and(|c| c.active);
    painter.text(
        frame_rect.right_top() + Vec2::new(-10.0, 11.0),
        Align2::RIGHT_CENTER,
        if active { "game view" } else { "inactive" },
        egui::FontId::proportional(10.5),
        MUTED(),
    );
    let img_rect = egui::Rect::from_min_size(frame_rect.left_top() + Vec2::new(0.0, 22.0), Vec2::new(w, h));
    mark(ui, "camera_preview", img_rect);
    match st.preview_tex {
        Some(tex) => {
            egui::Image::new(egui::load::SizedTexture::new(tex, img_rect.size()))
                .corner_radius(CornerRadius { nw: 0, ne: 0, sw: 11, se: 11 })
                .paint_at(ui, img_rect);
        }
        None => {
            painter.rect_filled(img_rect, 0.0, STAGE_BG());
        }
    }
}

// ------------------------------------------------------------------- console

fn console(ui: &mut Ui, ed: &mut Editor, st: &mut UiState) {
    let mut clear = false;
    ui.horizontal(|ui| {
        widgets::section_label(ui, "CONSOLE");
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if ui.add(egui::Button::new(RichText::new("Clear").size(11.0).color(MUTED())).frame(false)).clicked() {
                clear = true;
            }
        });
    });
    if clear {
        ed.console().clear();
    }
    let console = ed.world().resource::<kit::core::Console>();
    let rev = console.revision;
    let mut ask_rex: Option<String> = None;
    egui::ScrollArea::vertical().id_salt("console").auto_shrink([false, false]).stick_to_bottom(true).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 2.0;
        if console.lines().is_empty() {
            ui.label(RichText::new("Script output (DISPLAY) and errors appear here.").size(12.0).color(MUTED()));
        }
        for line in console.lines() {
            let color = match line.level {
                kit::core::LogLevel::Info => TEXT(),
                kit::core::LogLevel::Warn => WARN(),
                kit::core::LogLevel::Error => ERROR(),
            };
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(&line.source).size(11.0).monospace().color(MUTED()));
                ui.label(RichText::new(&line.text).size(12.0).monospace().color(color));
                if matches!(line.level, kit::core::LogLevel::Error)
                    && ui
                        .add(egui::Button::new(RichText::new("Ask Rex").size(10.5).color(ACCENT())).small())
                        .on_hover_text("Rex (Sniff mode) explains the error and how to fix it")
                        .clicked()
                {
                    ask_rex = Some(format!("Help me fix this error from the console:\n[{}] {}", line.source, line.text));
                }
            });
        }
    });
    st.console_rev = rev;
    drop(console);
    if let Some(question) = ask_rex {
        st.rex.ask("sniff", question);
        if !ed.settings.rex {
            st.toggle_pane(ed, ViewPane::Rex);
        }
    }
}

// ------------------------------------------------------------------- dialogs

fn welcome(ctx: &egui::Context, ed: &mut Editor, st: &mut UiState) {
    if ed.project.is_some() {
        return;
    }
    egui::Modal::new(Id::new("welcome"))
        .frame(
            Frame::new()
                .fill(DIALOG_BG())
                .stroke(Stroke::new(1.0, DIALOG_BORDER()))
                .corner_radius(CornerRadius::same(15))
                .inner_margin(Margin::same(26)),
        )
        .backdrop_color(Color32::from_rgba_unmultiplied(0x10, 0x0c, 0x18, 0xb3))
        .show(ctx, |ui| {
            ui.set_width(440.0);
            ui.horizontal(|ui| {
                widgets::logo(ui, 36.0);
                ui.vertical(|ui| {
                    ui.label(RichText::new("Kit").size(22.0).strong());
                    ui.label(
                        RichText::new(format!("v{}  ·  modular game engine", kit::VERSION)).size(11.0).color(MUTED()),
                    );
                });
            });
            ui.add_space(10.0);
            ui.label(
                RichText::new(
                    "Build 2D and 3D games with Rust, Vulkan and the x scripting language. \
                     Start a new project or open an existing one.",
                )
                .color(LABEL()),
            );
            ui.add_space(14.0);
            ui.horizontal(|ui| {
                if primary(ui, "New project…", None).clicked() {
                    st.new_project = Some(crate::new_project::Wizard::new(&crate::user_projects_dir()));
                }
                if ghost(ui, "Open project…").clicked() {
                    st.requests.push(Request::OpenProject);
                }
                if ghost(ui, "Open example").clicked() {
                    st.requests.push(Request::OpenExample);
                }
            });
            if !st.recent.is_empty() {
                ui.add_space(14.0);
                widgets::section_label(ui, "RECENT");
                for p in st.recent.clone().iter().take(6) {
                    let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                    let r = ui.add(
                        egui::Button::new(RichText::new(format!("{name}   {}", p.display())).size(12.0).color(LABEL()))
                            .frame(false),
                    );
                    if r.clicked() {
                        st.requests.push(Request::OpenPath(p.clone()));
                    }
                }
            }
            ui.add_space(8.0);
            ui.label(RichText::new(format!("GPU: {}", st.gpu_name)).size(10.5).color(MUTED()));
        });
}

fn help(ctx: &egui::Context, st: &mut UiState, controls: &crate::controls::Controls) {
    if !st.help {
        return;
    }
    let resp = egui::Modal::new(Id::new("help"))
        .frame(
            Frame::new()
                .fill(DIALOG_BG())
                .stroke(Stroke::new(1.0, DIALOG_BORDER()))
                .corner_radius(CornerRadius::same(15))
                .inner_margin(Margin::same(24)),
        )
        .show(ctx, |ui| {
            ui.set_width(380.0);
            ui.label(RichText::new("Shortcuts").size(20.0).strong());
            ui.add_space(8.0);
            egui::ScrollArea::vertical().max_height((ctx.content_rect().height() - 170.0).max(100.0)).show(ui, |ui| {
                egui::Grid::new("shortcuts").num_columns(2).spacing([30.0, 6.0]).show(ui, |ui| {
                    for &action in Shortcut::ALL {
                        ui.label(RichText::new(action.label()).color(LABEL()));
                        ui.label(RichText::new(controls.label(action)).color(MUTED()));
                        ui.end_row();
                    }
                });
            });
            ui.label("Rebind keys and pointer gestures in Project Settings → Editor Controls.");
            ui.add_space(10.0);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if primary(ui, "Got it", None).clicked() {
                    st.help = false;
                }
            });
        });
    if resp.should_close() {
        st.help = false;
    }
}

fn notice(ctx: &egui::Context, ed: &mut Editor) {
    let Some((text, at)) = ed.notice.clone() else { return };
    if at.elapsed() > Duration::from_secs(4) {
        ed.notice = None;
        return;
    }
    let screen = ctx.content_rect();
    egui::Area::new(Id::new("notice"))
        .fixed_pos(egui::pos2(screen.center().x, 76.0))
        .pivot(Align2::CENTER_TOP)
        .order(egui::Order::Foreground)
        .interactable(false)
        .show(ctx, |ui| {
            Frame::new()
                .fill(NOTICE_BG())
                .stroke(Stroke::new(1.0, NOTICE_BORDER()))
                .corner_radius(CornerRadius::same(9))
                .inner_margin(Margin::symmetric(14, 9))
                .show(ui, |ui| {
                    ui.set_max_width(600.0);
                    ui.label(RichText::new(text).size(13.0));
                });
        });
    ctx.request_repaint_after(Duration::from_millis(250));
}

/// A ghost button drawn over the (always dark) 3D viewport.
fn viewport_ghost(ui: &mut Ui, text: impl Into<String>) -> egui::Response {
    ui.add(
        egui::Button::new(RichText::new(text.into()).color(crate::theme::viewport::TEXT()))
            .fill(Color32::TRANSPARENT)
            .stroke(Stroke::NONE)
            .corner_radius(egui::CornerRadius::same(9)),
    )
}

#[cfg(test)]
mod view_tests {
    use super::*;

    #[test]
    fn last_view_stays_open_and_default_restores_regular_panes() {
        let mut editor = Editor::new();
        let mut ui = UiState::new(Vec::new(), String::new());
        ui.toggle_pane(&mut editor, ViewPane::Diagram);
        for pane in [ViewPane::Game, ViewPane::Assets, ViewPane::Hierarchy, ViewPane::Inspector, ViewPane::Diagram] {
            ui.toggle_pane(&mut editor, pane);
        }
        assert_eq!(ui.viewport_px, [0, 0]);
        assert!(editor.settings.diagram);
        assert!(!ui.can_hide(&editor));
        ui.restore_views(&egui::Context::default(), &mut editor);
        assert!(ui.views.game && ui.views.assets && ui.views.hierarchy && ui.views.inspector);
        assert!(!editor.settings.diagram && !editor.settings.console);
    }
}
