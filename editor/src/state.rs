//! Editor state and operations (no UI code here).

use crate::camera::EditorCamera;
use crate::templates;
use anyhow::{Context, Result, bail};
use kit::core::{App, Console, RunMode};
use kit::ecs::{Entity, World};
use kit::gamesys::{Camera, Light, MeshKind, Model, SceneFile, Script, Sprite};
use kit::math::{Vec2, Vec3, Vec4};
use kit::resource::Project;
use kit::scene::{GlobalTransform, Name, RootOrder, Transform};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// Asset groups shown in the editor, by file extension.
pub struct AssetGroup {
    pub name: &'static str,
    pub icon: &'static str,
    pub extensions: &'static [&'static str],
    /// Kit can use these files today (others are listed for upcoming importers).
    pub supported: bool,
}

pub const ASSET_GROUPS: [AssetGroup; 8] = [
    AssetGroup { name: "Scenes", icon: "◈", extensions: &["kscene"], supported: true },
    AssetGroup { name: "Scripts", icon: "✏", extensions: &["x"], supported: true },
    AssetGroup { name: "Diagrams", icon: "◫", extensions: &["kdiagram"], supported: true },
    AssetGroup { name: "Sprites", icon: "🖼", extensions: &["png", "jpg", "jpeg"], supported: true },
    AssetGroup { name: "Models", icon: "◆", extensions: crate::model_import::EXTENSIONS, supported: false },
    AssetGroup { name: "Sounds", icon: "♪", extensions: kit::gamesys::SOUND_EXTENSIONS, supported: true },
    AssetGroup { name: "Fonts", icon: "🔤", extensions: &["ttf", "otf"], supported: false },
    AssetGroup { name: "Data", icon: "📄", extensions: &["json", "toml", "csv", "txt"], supported: false },
];

/// Which group a file belongs to.
pub fn asset_group(path: &str) -> Option<&'static AssetGroup> {
    let ext = path.rsplit('.').next()?.to_ascii_lowercase();
    ASSET_GROUPS.iter().find(|g| g.extensions.contains(&ext.as_str()))
}

/// What the "add entity" menus can create.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Preset {
    Empty,
    Mesh(MeshKind),
    Sprite,
    Camera,
    Camera2D,
    Light,
    Terrain,
    Sound,
}

impl Preset {
    pub const MENU: [Preset; 13] = [
        Preset::Empty,
        Preset::Mesh(MeshKind::Cube),
        Preset::Mesh(MeshKind::Sphere),
        Preset::Mesh(MeshKind::Plane),
        Preset::Mesh(MeshKind::Cylinder),
        Preset::Mesh(MeshKind::Torus),
        Preset::Mesh(MeshKind::Quad),
        Preset::Sprite,
        Preset::Camera,
        Preset::Camera2D,
        Preset::Light,
        Preset::Terrain,
        Preset::Sound,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Preset::Empty => "Empty",
            Preset::Mesh(k) => k.label(),
            Preset::Sprite => "Sprite",
            Preset::Camera => "Camera",
            Preset::Camera2D => "Camera (2D)",
            Preset::Light => "Light",
            Preset::Terrain => "Terrain",
            Preset::Sound => "Sound",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ViewSettings {
    pub wireframe: bool,
    pub lighting: bool,
    pub grid: bool,
    pub console: bool,
    /// While playing, look through the game camera (otherwise the editor camera).
    pub game_camera: bool,
    /// Canvas mode: drag in the viewport to sketch a path for the selection.
    pub canvas: bool,
    /// Canvas draws on the ground/terrain instead of the camera-facing plane.
    pub canvas_ground: bool,
    /// Draw collider outlines.
    pub colliders: bool,
    /// Preview post-processing volumes in the main viewport (from the editor
    /// camera). Camera previews and Play always apply them.
    pub volumes: bool,
    /// Show the Diagram (visual logic) view under the viewport.
    pub diagram: bool,
    /// Terrain sculpting tool (when a terrain is selected).
    pub sculpt: Option<kit::gamesys::SculptTool>,
    pub brush_radius: f32,
    pub brush_strength: f32,
}

impl Default for ViewSettings {
    fn default() -> Self {
        Self {
            wireframe: false,
            lighting: true,
            grid: true,
            console: false,
            game_camera: true,
            canvas: false,
            canvas_ground: false,
            colliders: true,
            volumes: true,
            diagram: false,
            sculpt: None,
            brush_radius: 4.0,
            brush_strength: 0.5,
        }
    }
}

/// An open script in the code editor.
pub struct ScriptDoc {
    pub path: String,
    pub text: String,
    pub saved: String,
    pub error: Option<String>,
}

impl ScriptDoc {
    pub fn dirty(&self) -> bool {
        self.text != self.saved
    }
}

pub struct Editor {
    pub project: Option<Project>,
    pub app: App,
    pub scene_path: Option<String>,
    pub selected: Option<Entity>,
    pub camera: EditorCamera,
    pub settings: ViewSettings,
    pub playing: bool,
    play_snapshot: Option<(SceneFile, Option<usize>)>,
    undo: Vec<SceneFile>,
    redo: Vec<SceneFile>,
    baseline: SceneFile,
    saved: SceneFile,
    pub scripts: Vec<ScriptDoc>,
    pub active_script: Option<usize>,
    pub notice: Option<(String, Instant)>,
    pub assets: Vec<String>,
    assets_refreshed: Option<Instant>,
    /// A background export and its platform.
    export_job: Option<(std::sync::mpsc::Receiver<Result<PathBuf>>, crate::export::Platform)>,
    /// How the last export ended: the folder written, or the error.
    pub last_export: Option<std::result::Result<(PathBuf, crate::export::Platform), String>>,
    model_imports: Option<crate::model_import::ImportQueue>,
    /// The sound file being previewed in the editor.
    previewing: Option<String>,
    pub renaming: Option<(Entity, String)>,
    /// Ask the UI to open Project Settings.
    pub request_settings: bool,
}

fn new_app(root: PathBuf, config: &kit::resource::ProjectConfig) -> App {
    let mut app = App::new();
    kit::engine::add_default_modules(&mut app, root, config);
    app.mode = RunMode::Edit;
    app
}

/// Depth-first scene order, matching `save_scene` / `load_scene`.
fn scene_order(world: &World) -> Vec<Entity> {
    let mut out = Vec::new();
    for r in kit::scene::roots(world) {
        out.extend(kit::scene::descendants_and_self(world, r));
    }
    out
}

impl Editor {
    /// The scene camera's background, if the developer changed it from the default.
    pub fn custom_background(&self) -> Option<Vec4> {
        let w = &self.app.world;
        let cam = kit::gamesys::active_camera(w)?;
        let clear = w.get::<kit::gamesys::Camera>(cam)?.clear_color;
        let default = kit::gamesys::Camera::default().clear_color;
        (clear.distance(default) > 1e-3).then_some(clear)
    }

    /// What the editor's scene view clears to in light mode: white unless the scene sets its own.
    pub fn light_background(&self) -> Vec4 {
        self.custom_background().unwrap_or(Vec4::ONE)
    }

    pub fn new() -> Self {
        let root = std::env::current_dir().unwrap_or_default();
        Self {
            project: None,
            app: new_app(root, &Default::default()),
            scene_path: None,
            selected: None,
            camera: EditorCamera::default(),
            settings: ViewSettings::default(),
            playing: false,
            play_snapshot: None,
            undo: Vec::new(),
            redo: Vec::new(),
            baseline: SceneFile::default(),
            saved: SceneFile::default(),
            scripts: Vec::new(),
            active_script: None,
            notice: None,
            assets: Vec::new(),
            assets_refreshed: None,
            model_imports: None,
            previewing: None,
            export_job: None,
            last_export: None,
            renaming: None,
            request_settings: false,
        }
    }

    pub fn world(&self) -> &World {
        &self.app.world
    }

    pub fn notify(&mut self, text: impl Into<String>) {
        self.notice = Some((text.into(), Instant::now()));
    }

    pub fn console(&self) -> std::cell::RefMut<'_, Console> {
        self.app.world.resource_mut::<Console>()
    }

    pub fn report(&mut self, result: Result<()>) {
        if let Err(e) = result {
            let msg = format!("{e:#}");
            log::error!("{msg}");
            self.console().error("editor", msg.clone());
            self.notify(msg);
        }
    }

    // ----------------------------------------------------------------- project

    pub fn open_project(&mut self, path: &Path) -> Result<()> {
        if self.playing {
            self.stop();
        }
        let project = Project::open(path)?;
        self.app = new_app(project.root().to_path_buf(), &project.config);
        self.scripts.clear();
        self.active_script = None;
        self.selected = None;
        let main = project.config.project.main_scene.clone();
        let title = project.config.project.title.clone();
        self.project = Some(project);
        self.refresh_assets(true);
        if self.project.as_ref().unwrap().resources.exists(&main) {
            self.open_scene(&main)?;
        } else {
            self.new_scene(&main);
        }
        self.console().info("editor", format!("opened project {title}"));
        Ok(())
    }

    pub fn create_project(&mut self, dir: &Path) -> Result<()> {
        if dir.join(kit::resource::PROJECT_FILE).exists() {
            bail!("{} already contains a Kit project", dir.display());
        }
        templates::create_project(dir)?;
        self.open_project(dir)
    }

    pub fn project_title(&self) -> &str {
        self.project.as_ref().map(|p| p.config.project.title.as_str()).unwrap_or("No project")
    }

    pub fn refresh_assets(&mut self, force: bool) {
        if !force && self.assets_refreshed.is_some_and(|t| t.elapsed() < Duration::from_secs(2)) {
            return;
        }
        self.assets_refreshed = Some(Instant::now());
        self.assets.clear();
        if let Some(p) = &self.project {
            for group in &ASSET_GROUPS {
                for ext in group.extensions {
                    self.assets.extend(p.resources.list(ext));
                }
            }
            // The project file itself is edited through Project Settings.
            self.assets.retain(|a| a != kit::resource::PROJECT_FILE);
            self.assets.sort();
            self.assets.dedup();
        }
    }

    /// Copy a sound file into the project's `sounds/` folder (checking Kit can
    /// play it). Returns its project path.
    pub fn import_sound(&mut self, source: &Path) -> Result<String> {
        let project = self.project.as_ref().context("Open a project before adding sounds")?;
        let name = source.file_name().and_then(|n| n.to_str()).context("The sound's file name must be valid UTF-8")?;
        anyhow::ensure!(
            kit::gamesys::is_sound(name),
            "Kit plays WAV, MP3, Ogg Vorbis, FLAC and AAC (.m4a) sounds"
        );
        let bytes = std::fs::read(source).with_context(|| format!("reading {}", source.display()))?;
        let ext = Path::new(name).extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
        kit::audio::decode(bytes.clone(), &ext).with_context(|| format!("{name} can't be played"))?;
        let root = project.root().to_path_buf();
        // Already in the project: use it where it is.
        if let Ok(rel) = source.canonicalize()?.strip_prefix(root.canonicalize()?) {
            let rel = rel.to_string_lossy().replace('\\', "/");
            self.refresh_assets(true);
            return Ok(rel);
        }
        let rel = copy_into_project(&root, "sounds", name, &bytes)?;
        self.refresh_assets(true);
        self.notify(format!("{rel} added under Assets → Sounds"));
        Ok(rel)
    }

    /// Import any file Kit can use: sounds, sprites, 3D models, scripts,
    /// scenes and diagrams. Each kind is checked, then copied into its folder
    /// (`sprites/`, `scripts/`, …); files already in the project stay where
    /// they are. Returns the project path (None for models, which import in
    /// the background).
    pub fn import_asset(&mut self, source: &Path) -> Result<Option<String>> {
        let name = source.file_name().and_then(|n| n.to_str()).context("The file name must be valid UTF-8")?.to_owned();
        let ext = Path::new(&name).extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
        if kit::gamesys::is_sound(&name) {
            return self.import_sound(source).map(Some);
        }
        if crate::model_import::SOURCE_EXTENSIONS.contains(&ext.as_str()) {
            self.import_model(source)?;
            return Ok(None);
        }
        let project = self.project.as_ref().context("Open a project before importing files")?;
        let root = project.root().to_path_buf();
        let bytes = std::fs::read(source).with_context(|| format!("reading {}", source.display()))?;
        let text = || String::from_utf8(bytes.clone()).with_context(|| format!("{name} isn't a text file"));
        let (folder, group) = match ext.as_str() {
            "png" | "jpg" | "jpeg" => {
                image::load_from_memory(&bytes).with_context(|| format!("{name} isn't an image Kit can read"))?;
                ("sprites", "Sprites")
            }
            "x" => {
                if let Err(e) = kit::x::parse(&text()?) {
                    self.console().warn("import", format!("{name} has a script error (imported anyway): {e}"));
                }
                ("scripts", "Scripts")
            }
            "kscene" => {
                kit::gamesys::SceneFile::from_json(&text()?).with_context(|| format!("{name} isn't a Kit scene"))?;
                ("scenes", "Scenes")
            }
            "kdiagram" => {
                kit::diagram::Graph::from_json(&text()?).with_context(|| format!("{name} isn't a Kit diagram"))?;
                ("diagrams", "Diagrams")
            }
            _ => bail!(
                "Kit can't use .{ext} files. It imports sounds (WAV, MP3, Ogg, FLAC, AAC), sprites (PNG, JPEG), 3D models ({}), scripts (.x), scenes (.kscene) and diagrams (.kdiagram).",
                crate::model_import::SOURCE_EXTENSIONS.iter().map(|e| e.to_uppercase()).collect::<Vec<_>>().join(", ")
            ),
        };
        let rel = match source.canonicalize().ok().and_then(|s| s.strip_prefix(root.canonicalize().ok()?).ok().map(|r| r.to_path_buf())) {
            Some(inside) => inside.to_string_lossy().replace('\\', "/"),
            None => copy_into_project(&root, folder, &name, &bytes)?,
        };
        self.refresh_assets(true);
        self.notify(format!("{rel} added under Assets → {group}"));
        Ok(Some(rel))
    }

    /// Add a sprite object showing `texture` (at `at`, or near the view).
    pub fn place_sprite(&mut self, texture: &str, at: Option<Vec3>) -> Entity {
        let e = self.spawn(Preset::Empty, None);
        let name = Path::new(texture).file_stem().and_then(|s| s.to_str()).unwrap_or("Sprite").to_owned();
        self.app.world.insert(e, Name(name));
        self.app.world.insert(e, kit::gamesys::Sprite { texture: texture.to_owned(), ..Default::default() });
        if let (Some(p), Some(mut t)) = (at, self.app.world.get_mut::<kit::scene::Transform>(e)) {
            t.translation = p;
        }
        self.selected = Some(e);
        e
    }

    /// Add a sound object playing `clip` (at `at`, or near the view).
    pub fn place_sound(&mut self, clip: &str, at: Option<Vec3>) -> Entity {
        let e = self.spawn(Preset::Sound, None);
        let w = &mut self.app.world;
        let name = Path::new(clip).file_stem().and_then(|s| s.to_str()).unwrap_or("Sound").to_owned();
        w.insert(e, Name(name));
        if let Some(mut a) = w.get_mut::<kit::gamesys::AudioSource>(e) {
            a.clip = clip.to_owned();
        }
        if let (Some(p), Some(mut t)) = (at, w.get_mut::<Transform>(e)) {
            t.translation = p;
        }
        self.selected = Some(e);
        e
    }

    /// Give an object a sound (adds an Audio Source, or changes its clip).
    pub fn attach_sound(&mut self, e: Entity, clip: &str) {
        let w = &mut self.app.world;
        if let Some(mut a) = w.get_mut::<kit::gamesys::AudioSource>(e) {
            a.clip = clip.to_owned();
        } else {
            let spatial = !w.has::<Camera>(e);
            w.insert(e, kit::gamesys::AudioSource { clip: clip.to_owned(), spatial, ..Default::default() });
        }
        self.selected = Some(e);
        let label = self.entity_label(e);
        self.notify(format!("{label} now plays {clip}"));
    }

    /// Editor preview of a sound file: plays it, or stops it if it's playing.
    pub fn toggle_preview(&mut self, clip: &str) {
        let project = self.project.as_ref().map(|p| p.resources.clone());
        let Some(resources) = project else { return };
        let Some(mut audio) = self.app.world.try_resource_mut::<kit::audio::Audio>() else { return };
        if self.previewing.as_deref() == Some(clip) && audio.previewing() {
            audio.stop_preview();
            self.previewing = None;
            return;
        }
        let result = audio.preview(&resources, clip, 1.0, 1.0);
        let has_output = audio.has_output();
        drop(audio);
        match result {
            Ok(_) if !has_output => {
                self.previewing = Some(clip.to_owned());
                self.notify("No audio output device found, so previews are silent");
            }
            Ok(_) => self.previewing = Some(clip.to_owned()),
            Err(e) => self.report(Err(e)),
        }
    }

    /// The sound being previewed, if it's still playing.
    pub fn preview_playing(&self) -> Option<&str> {
        let playing = self.app.world.try_resource::<kit::audio::Audio>().is_some_and(|a| a.previewing());
        self.previewing.as_deref().filter(|_| playing)
    }

    pub fn import_model(&mut self, source: &Path) -> Result<()> {
        let project = self.project.as_ref().context("Open a project before importing a model")?;
        let root = project.root().canonicalize()?;
        crate::model_import::validate_extension(source)?;
        if !crate::model_import::needs_conversion(source)? {
            let imported = crate::model_import::import(source, &root)?;
            self.model_imported(&imported);
        } else {
            if self.model_imports.is_none() {
                self.model_imports = Some(crate::model_import::ImportQueue::new()?);
            }
            self.model_imports.as_mut().unwrap().submit(source, root)?;
            self.notify(format!("Importing {}…", source.file_name().unwrap_or_default().to_string_lossy()));
        }
        Ok(())
    }

    pub fn poll_model_imports(&mut self) -> bool {
        let Some(queue) = self.model_imports.as_mut() else { return false };
        let (finished, pending) = queue.poll();
        for result in finished {
            match result {
                Ok(imported) => self.model_imported(&imported),
                Err(error) => self.report(Err(error)),
            }
        }
        pending
    }

    fn model_imported(&mut self, imported: &str) {
        self.refresh_assets(true);
        self.notify(format!("{imported} is available under Assets → Models"));
    }

    pub fn place_model(&mut self, path: &str) -> Result<()> {
        anyhow::ensure!(!self.playing, "Stop play mode before placing a model");
        let project = self.project.as_ref().context("No project open")?;
        if kit::render::is_model_bundle(Path::new(path)) {
            let model = kit::render::model_asset::ModelAsset::load(&project.resources, path)?;
            for warning in model.warnings {
                self.console().warn("models", warning);
            }
        } else {
            kit::render::load_obj(&project.resources.resolve(path)?)?;
        }
        let name = Path::new(path).file_stem().and_then(|stem| stem.to_str()).unwrap_or("Model");
        let entity = self.spawn(Preset::Empty, None);
        self.app.world.insert(entity, Name(name.to_owned()));
        self.app.world.insert(entity, Model { mesh_asset: path.to_owned(), color: Vec4::ONE, ..Default::default() });
        self.frame_selected();
        Ok(())
    }

    pub fn assets_with(&self, exts: &[&str]) -> Vec<String> {
        self.assets
            .iter()
            .filter(|a| exts.iter().any(|e| a.to_lowercase().ends_with(&format!(".{e}"))))
            .cloned()
            .collect()
    }

    // ------------------------------------------------------------------- scene

    fn replace_scene(&mut self, scene: &SceneFile) -> Result<Vec<Entity>> {
        kit::script::stop_scripts(&mut self.app.world);
        kit::collider::reset(&mut self.app.world);
        kit::path::reset(&mut self.app.world);
        self.app.world.clear_entities();
        let spawned = kit::gamesys::load_scene(&mut self.app.world, scene)?;
        Ok(spawned)
    }

    pub fn open_scene(&mut self, rel: &str) -> Result<()> {
        if self.playing {
            self.stop();
        }
        let project = self.project.as_ref().context("no project open")?;
        let text = project.resources.read_text(rel)?;
        let scene = SceneFile::from_json(&text).with_context(|| format!("in {rel}"))?;
        self.replace_scene(&scene)?;
        self.scene_path = Some(rel.to_string());
        self.selected = None;
        self.undo.clear();
        self.redo.clear();
        self.baseline = kit::gamesys::save_scene(&self.app.world);
        self.saved = self.baseline.clone();
        self.frame_all();
        Ok(())
    }

    pub fn new_scene(&mut self, rel: &str) {
        let scene = templates::starter_scene();
        if let Err(e) = self.replace_scene(&scene) {
            self.report(Err(e));
        }
        self.scene_path = Some(rel.to_string());
        self.selected = None;
        self.undo.clear();
        self.redo.clear();
        self.baseline = kit::gamesys::save_scene(&self.app.world);
        self.saved = SceneFile::default();
    }

    pub fn save_scene(&mut self) -> Result<()> {
        if self.playing {
            bail!("stop the game before saving");
        }
        let project = self.project.as_ref().context("open or create a project first")?;
        let rel = self.scene_path.clone().unwrap_or_else(|| project.config.project.main_scene.clone());
        let scene = kit::gamesys::save_scene(&self.app.world);
        project.resources.write_text(&rel, &scene.to_json())?;
        self.saved = scene;
        self.scene_path = Some(rel.clone());
        self.notify(format!("Saved {rel}"));
        self.refresh_assets(true);
        Ok(())
    }

    /// Save the scene if it has unsaved changes.
    pub fn save_if_dirty(&mut self) -> Result<()> {
        if self.scene_dirty() { self.save_scene() } else { Ok(()) }
    }

    pub fn scene_dirty(&self) -> bool {
        !self.playing && self.baseline != self.saved
    }

    pub fn scene_name(&self) -> String {
        self.scene_path.clone().unwrap_or_else(|| "untitled.kscene".into())
    }

    // -------------------------------------------------------------- undo/redo

    /// Called once per frame. When nothing is being edited (`at_rest`), compare
    /// the scene to the last known state and record an undo step if it changed.
    #[cfg(test)]
    pub fn undo_len(&self) -> usize {
        self.undo.len()
    }

    pub fn track_changes(&mut self, at_rest: bool) {
        if self.playing || !at_rest {
            return;
        }
        let now = kit::gamesys::save_scene(&self.app.world);
        if now != self.baseline {
            let prev = std::mem::replace(&mut self.baseline, now);
            self.undo.push(prev);
            if self.undo.len() > 200 {
                self.undo.remove(0);
            }
            self.redo.clear();
        }
    }

    fn selected_index(&self) -> Option<usize> {
        let sel = self.selected?;
        scene_order(&self.app.world).iter().position(|e| *e == sel)
    }

    fn restore(&mut self, scene: SceneFile) {
        let idx = self.selected_index();
        match self.replace_scene(&scene) {
            Ok(spawned) => {
                self.selected = idx.and_then(|i| spawned.get(i).copied());
                self.baseline = scene;
            }
            Err(e) => self.report(Err(e)),
        }
    }

    pub fn can_undo(&self) -> bool {
        !self.playing && !self.undo.is_empty()
    }
    pub fn can_redo(&self) -> bool {
        !self.playing && !self.redo.is_empty()
    }

    pub fn undo(&mut self) {
        if !self.can_undo() {
            return;
        }
        let prev = self.undo.pop().unwrap();
        self.redo.push(self.baseline.clone());
        self.restore(prev);
    }

    pub fn redo(&mut self) {
        if !self.can_redo() {
            return;
        }
        let next = self.redo.pop().unwrap();
        self.undo.push(self.baseline.clone());
        self.restore(next);
    }

    // ----------------------------------------------------------------- editing

    pub fn spawn(&mut self, preset: Preset, parent: Option<Entity>) -> Entity {
        let w = &mut self.app.world;
        let mut t = Transform::IDENTITY;
        if parent.is_none() && !self.camera.two_d {
            // Drop new objects near the view target.
            t.translation = Vec3::new(self.camera.target.x.round(), 0.5, self.camera.target.z.round());
        }
        if self.camera.two_d && parent.is_none() {
            t.translation = Vec3::new(self.camera.target.x.round(), self.camera.target.y.round(), 0.0);
        }
        let name = match preset {
            Preset::Empty => "Entity",
            other => other.label(),
        };
        let e = kit::scene::spawn_node(w, name, t);
        match preset {
            Preset::Empty => {}
            Preset::Mesh(mesh) => {
                w.insert(e, Model { mesh, ..Default::default() });
                if mesh == MeshKind::Plane {
                    w.get_mut::<Transform>(e).unwrap().scale = Vec3::new(10.0, 1.0, 10.0);
                    w.get_mut::<Transform>(e).unwrap().translation.y = 0.0;
                    w.get_mut::<Model>(e).unwrap().color = Vec4::new(0.25, 0.22, 0.33, 1.0);
                }
            }
            Preset::Sprite => {
                w.insert(e, Sprite { size: Vec2::ONE, ..Default::default() });
            }
            Preset::Camera => {
                let mut ct = Transform::from_xyz(0.0, 2.0, 8.0);
                ct.look_at(Vec3::ZERO, Vec3::Y);
                *w.get_mut::<Transform>(e).unwrap() = ct;
                w.insert(e, Camera::perspective());
            }
            Preset::Camera2D => {
                *w.get_mut::<Transform>(e).unwrap() = Transform::from_xyz(0.0, 0.0, 10.0);
                w.insert(e, Camera::orthographic(10.0));
            }
            Preset::Terrain => {
                let mut t = kit::gamesys::Terrain::default();
                t.generate(
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.subsec_nanos())
                        .unwrap_or(1),
                    6.0,
                    0.5,
                );
                w.insert(e, t);
                w.get_mut::<Transform>(e).unwrap().translation = Vec3::ZERO;
            }
            Preset::Sound => {
                // Placed in the world, so proximity (3D) by default.
                w.insert(e, kit::gamesys::AudioSource { spatial: true, ..Default::default() });
            }
            Preset::Light => {
                w.get_mut::<Transform>(e).unwrap().set_euler_degrees(Vec3::new(-50.0, 35.0, 0.0));
                w.get_mut::<Transform>(e).unwrap().translation = Vec3::new(0.0, 4.0, 0.0);
                w.insert(e, Light::default());
            }
        }
        match parent {
            Some(p) if w.is_alive(p) => {
                w.get_mut::<Transform>(e).unwrap().translation = Vec3::ZERO;
                kit::scene::set_parent(w, e, p);
            }
            _ => {
                let n = kit::scene::roots(w).len() as u32;
                w.insert(e, RootOrder(n + 1000));
                self.normalize_root_order();
            }
        }
        kit::scene::propagate_transforms(&mut self.app.world);
        self.selected = Some(e);
        e
    }

    /// Renumber root order keys 0..n in their current order.
    pub fn normalize_root_order(&mut self) {
        let roots = kit::scene::roots(&self.app.world);
        for (i, r) in roots.into_iter().enumerate() {
            self.app.world.insert(r, RootOrder(i as u32));
        }
    }

    pub fn delete(&mut self, e: Entity) {
        kit::scene::despawn_recursive(&mut self.app.world, e);
        if self.selected.is_some_and(|s| !self.app.world.is_alive(s)) {
            self.selected = None;
        }
        self.normalize_root_order();
    }

    pub fn duplicate(&mut self, e: Entity) {
        match kit::gamesys::clone_subtree(&mut self.app.world, e) {
            Ok(copy) => {
                if let Some(mut n) = self.app.world.get_mut::<Name>(copy) {
                    n.0 = format!("{} copy", n.0);
                }
                self.selected = Some(copy);
                self.normalize_root_order();
            }
            Err(e) => self.report(Err(e)),
        }
    }

    /// Move `e` under `new_parent` (or to the root level) at `index`.
    pub fn reparent(&mut self, e: Entity, new_parent: Option<Entity>, index: usize) {
        let w = &mut self.app.world;
        if !w.is_alive(e) {
            return;
        }
        kit::scene::propagate_transforms(w);
        let world_m = w.get::<GlobalTransform>(e).map(|g| g.0);
        match new_parent {
            Some(p) => {
                if !kit::scene::set_parent_at(w, e, p, index) {
                    return;
                }
                w.remove::<RootOrder>(e);
            }
            None => {
                kit::scene::remove_parent(w, e);
                let mut roots: Vec<Entity> = kit::scene::roots(w).into_iter().filter(|r| *r != e).collect();
                roots.insert(index.min(roots.len()), e);
                for (i, r) in roots.into_iter().enumerate() {
                    w.insert(r, RootOrder(i as u32));
                }
            }
        }
        // Keep the object where it is in the world.
        if let Some(m) = world_m {
            kit::scene::propagate_transforms(w);
            kit::scene::set_world_matrix(w, e, m);
        }
        kit::scene::propagate_transforms(w);
    }

    pub fn entity_label(&self, e: Entity) -> String {
        self.app.world.get::<Name>(e).map(|n| n.0.clone()).filter(|n| !n.is_empty()).unwrap_or_else(|| "Entity".into())
    }

    /// Short badge for the footer: the main component type.
    pub fn entity_kind(&self, e: Entity) -> &'static str {
        let w = &self.app.world;
        if w.has::<Camera>(e) {
            "CAMERA"
        } else if w.has::<Light>(e) {
            "LIGHT"
        } else if w.has::<Sprite>(e) {
            "SPRITE"
        } else if w.has::<Model>(e) {
            "MODEL"
        } else if w.has::<kit::gamesys::AudioSource>(e) {
            "SOUND"
        } else if w.has::<kit::gamesys::PostVolume>(e) {
            "VOLUME"
        } else if w.has::<Script>(e) {
            "SCRIPT"
        } else {
            "EMPTY"
        }
    }

    // ------------------------------------------------------------------ camera

    fn bounds_of(&self, entities: &[Entity]) -> Option<(Vec3, f32)> {
        let w = &self.app.world;
        let mut min = Vec3::splat(f32::MAX);
        let mut max = Vec3::splat(f32::MIN);
        let mut any = false;
        for &e in entities {
            let Some(g) = w.get::<GlobalTransform>(e).map(|g| g.0) else { continue };
            if let Some(model) = w.get::<Model>(e) {
                if let Some((lo, hi)) = kit::render::model_bounds(w, &model.mesh_asset) {
                    let center = g.transform_point3(lo * 0.5 + hi * 0.5);
                    let half = hi * 0.5 - lo * 0.5;
                    let extent = g.x_axis.truncate().abs() * half.x
                        + g.y_axis.truncate().abs() * half.y
                        + g.z_axis.truncate().abs() * half.z;
                    min = min.min(center - extent);
                    max = max.max(center + extent);
                    any = true;
                    continue;
                }
            }
            let (s, _, p) = g.to_scale_rotation_translation();
            let r = if let Some(t) = w.get::<kit::gamesys::Terrain>(e) {
                let (lo, hi) = t.height_range();
                (t.size * 0.5 * s.x.max(s.z)).max((hi - lo) * s.y)
            } else if let Some(m) = w.get::<Model>(e) {
                m.mesh.bounding_radius() * s.abs().max_element()
            } else if let Some(sp) = w.get::<Sprite>(e) {
                (sp.size * s.truncate()).length() * 0.5
            } else if let Some(v) = w.get::<kit::gamesys::PostVolume>(e) {
                // The volume's own size (globals: a marker-sized spot).
                match v.shape {
                    kit::gamesys::VolumeShape::Box => (s.abs() * 0.5).length(),
                    kit::gamesys::VolumeShape::Sphere => s.abs().max_element() * 0.5,
                    kit::gamesys::VolumeShape::Global => 0.5,
                }
            } else {
                0.5
            };
            min = min.min(p - Vec3::splat(r));
            max = max.max(p + Vec3::splat(r));
            any = true;
        }
        any.then(|| ((min + max) * 0.5, ((max - min) * 0.5).length()))
    }

    pub fn frame_selected(&mut self) {
        match self.selected {
            Some(e) => {
                if let Some((c, r)) = self.bounds_of(&kit::scene::descendants_and_self(&self.app.world, e)) {
                    self.camera.frame_animated(c, r);
                }
            }
            None => self.frame_all(),
        }
    }

    pub fn frame_all(&mut self) {
        let w = &self.app.world;
        let ents: Vec<Entity> = scene_order(w)
            .into_iter()
            .filter(|e| w.has::<Model>(*e) || w.has::<Sprite>(*e))
            .filter(|e| w.get::<Model>(*e).is_none_or(|m| m.mesh != MeshKind::Plane))
            .collect();
        if let Some((c, r)) = self.bounds_of(&ents) {
            self.camera.frame(c, r.max(1.5));
        }
    }

    // -------------------------------------------------------------------- play

    pub fn play(&mut self) {
        if self.playing {
            return;
        }
        // Graphs and game data are read fresh for each Play.
        kit::gamedata::reload(&mut self.app.world);
        kit::diagram::reset(&mut self.app.world);
        let snapshot = kit::gamesys::save_scene(&self.app.world);
        let idx = self.selected_index();
        self.play_snapshot = Some((snapshot, idx));
        self.app.mode = RunMode::Play;
        self.app.world.resource_mut::<kit::core::Time>().elapsed = 0.0;
        self.app.reset_startup();
        self.playing = true;
        self.console().info("editor", "▶ play");
    }

    pub fn stop(&mut self) {
        if !self.playing {
            return;
        }
        kit::script::stop_scripts(&mut self.app.world);
        kit::collider::reset(&mut self.app.world);
        kit::path::reset(&mut self.app.world);
        kit::audio::reset(&mut self.app.world);
        kit::diagram::reset(&mut self.app.world);
        self.app.mode = RunMode::Edit;
        self.app.world.resource_mut::<kit::core::Time>().elapsed = 0.0;
        self.playing = false;
        if let Some((scene, idx)) = self.play_snapshot.take() {
            match self.replace_scene(&scene) {
                Ok(spawned) => self.selected = idx.and_then(|i| spawned.get(i).copied()),
                Err(e) => self.report(Err(e)),
            }
        }
        if let Some(mut input) = self.app.world.try_resource_mut::<kit::input::Input>() {
            input.clear();
            input.end_frame();
        }
        self.console().info("editor", "■ stop");
    }

    // ----------------------------------------------------------------- scripts

    pub fn open_script(&mut self, rel: &str) {
        if let Some(i) = self.scripts.iter().position(|s| s.path == rel) {
            self.active_script = Some(i);
            return;
        }
        let Some(project) = &self.project else { return };
        match project.resources.read_text(rel) {
            Ok(text) => {
                let error = kit::script::check_source(rel, &text).err().map(|e| e.to_string());
                self.scripts.push(ScriptDoc { path: rel.to_string(), saved: text.clone(), text, error });
                self.active_script = Some(self.scripts.len() - 1);
            }
            Err(e) => self.report(Err(e)),
        }
    }

    pub fn save_script(&mut self, i: usize) {
        let Some(project) = &self.project else { return };
        let doc = &mut self.scripts[i];
        let result = project.resources.write_text(&doc.path, &doc.text);
        match result {
            Ok(()) => {
                doc.saved = doc.text.clone();
                doc.error = kit::script::check_source(&doc.path, &doc.text).err().map(|e| e.to_string());
                let msg = match &doc.error {
                    Some(e) => format!("Saved with errors: {e}"),
                    None => format!("Saved {}", doc.path),
                };
                self.notify(msg);
            }
            Err(e) => self.report(Err(e)),
        }
    }

    /// Create `scripts/<name>.x` from the template and open it.
    pub fn new_script(&mut self, name: &str) -> Option<String> {
        let Some(project) = self.project.as_ref() else {
            self.notify("Open or create a project first (Project menu)");
            return None;
        };
        // Keep names file-system friendly: "Enemy AI" -> "enemy_ai".
        let cleaned: String = name
            .trim()
            .trim_end_matches(".x")
            .chars()
            .map(|c| if c.is_alphanumeric() || c == '/' || c == '_' || c == '-' { c.to_ascii_lowercase() } else { '_' })
            .collect();
        let stem = cleaned.trim_matches(|c| c == '_' || c == '/');
        if stem.is_empty() {
            self.notify("Type a script name first");
            return None;
        }
        let rel = format!("scripts/{stem}.x");
        if !project.resources.exists(&rel) {
            let class = templates::class_name_for(stem.rsplit('/').next().unwrap_or(stem));
            if let Err(e) = project.resources.write_text(&rel, &templates::script_template(&class)) {
                self.report(Err(e));
                return None;
            }
        }
        self.refresh_assets(true);
        self.open_script(&rel);
        self.notify(format!("Created {rel}"));
        Some(rel)
    }

    // ------------------------------------------------------------------ export

    /// Export a runnable copy of the game: the player executable (renamed to the
    /// game title), its runtime libraries, and the project in `game/`.
    /// Describe an export of the open project for `platform` into `dest`.
    pub fn export_job(&self, platform: &crate::export::Platform, dest: &Path) -> Result<crate::export::ExportJob> {
        let project = self.project.as_ref().context("no project open")?;
        let bin_dir = std::env::current_exe()?.parent().context("editor has no folder")?.to_path_buf();
        Ok(crate::export::ExportJob {
            platform: *platform,
            availability: crate::export::availability(platform, &bin_dir),
            bin_dir,
            root: project.root().to_path_buf(),
            title: project.config.project.title.clone(),
            dest: dest.to_path_buf(),
            protection: crate::export::Protection::Standard,
            key_file: None,
        })
    }

    /// Export in the background (saving the scene first). `poll_export` reports the result.
    pub fn start_export(
        &mut self,
        platform: &crate::export::Platform,
        dest: &Path,
        protection: crate::export::Protection,
    ) -> Result<()> {
        anyhow::ensure!(self.export_job.is_none(), "An export is already in progress");
        if let Err(why) = crate::export::protection_supported(platform, protection) {
            bail!("{why}");
        }
        if protection != crate::export::Protection::Standard {
            // A missing or unreadable key is reported before anything is written.
            crate::export::load_key(None)?;
        }
        if self.scene_dirty() {
            self.save_scene()?;
        }
        let mut job = self.export_job(platform, dest)?;
        job.protection = protection;
        if let crate::export::Availability::NeedsPlayer(why) | crate::export::Availability::Unavailable(why) =
            &job.availability
        {
            bail!("{why}");
        }
        let (tx, rx) = std::sync::mpsc::channel();
        let platform = *platform;
        std::thread::Builder::new().name("kit-export".into()).spawn(move || {
            let _ = tx.send(job.run());
        })?;
        self.export_job = Some((rx, platform));
        self.last_export = None;
        Ok(())
    }

    /// Is an export running?
    pub fn exporting(&self) -> bool {
        self.export_job.is_some()
    }

    /// Finish a background export. Returns true while one is still running.
    pub fn poll_export(&mut self) -> bool {
        let Some((rx, platform)) = &self.export_job else { return false };
        let platform = *platform;
        match rx.try_recv() {
            Ok(result) => {
                self.export_job = None;
                match result {
                    Ok(out) => {
                        self.notify(format!("Exported for {} to {}", platform.name, out.display()));
                        self.last_export = Some(Ok((out, platform)));
                    }
                    Err(e) => {
                        self.console().error("export", format!("{e:#}"));
                        self.last_export = Some(Err(format!("{e:#}")));
                    }
                }
                false
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => true,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.export_job = None;
                self.last_export = Some(Err("The export stopped unexpectedly".into()));
                false
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assets_are_grouped_by_extension() {
        assert_eq!(asset_group("scripts/a.x").unwrap().name, "Scripts");
        assert_eq!(asset_group("art/Hero.PNG").unwrap().name, "Sprites");
        assert_eq!(asset_group("models/ship.glb").unwrap().name, "Models");
        assert_eq!(asset_group("sfx/jump.ogg").unwrap().name, "Sounds");
        assert_eq!(asset_group("music/theme.M4A").unwrap().name, "Sounds");
        assert_eq!(asset_group("main.kscene").unwrap().name, "Scenes");
        assert!(asset_group("README").is_none());
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "kit_editor_{tag}_{}_{}",
            std::process::id(),
            Instant::now().elapsed().as_nanos()
        ));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn project_edit_undo_play_cycle() {
        let dir = temp_dir("cycle");
        let mut ed = Editor::new();
        ed.create_project(&dir).unwrap();
        let n0 = ed.world().entity_count();
        assert_eq!(n0, 3);
        assert!(!ed.scene_dirty());

        let e = ed.spawn(Preset::Mesh(MeshKind::Sphere), None);
        ed.track_changes(true);
        assert_eq!(ed.world().entity_count(), 4);
        assert!(ed.scene_dirty());
        let child = ed.spawn(Preset::Light, Some(e));
        ed.track_changes(true);
        assert_eq!(kit::scene::children(ed.world(), e), vec![child]);

        ed.undo();
        assert_eq!(ed.world().entity_count(), 4);
        ed.undo();
        assert_eq!(ed.world().entity_count(), 3);
        ed.redo();
        assert_eq!(ed.world().entity_count(), 4);

        ed.save_scene().unwrap();
        assert!(!ed.scene_dirty());

        // Play runs scripts (the spinner moves the cube), stop restores the scene.
        let cube = kit::scene::find_by_name(ed.world(), "Cube").unwrap();
        let before = *ed.world().get::<Transform>(cube).unwrap();
        ed.play();
        for _ in 0..5 {
            ed.app.update(0.1);
        }
        let cube_now = kit::scene::find_by_name(ed.world(), "Cube").unwrap();
        assert_ne!(*ed.world().get::<Transform>(cube_now).unwrap(), before);
        ed.stop();
        let cube = kit::scene::find_by_name(ed.world(), "Cube").unwrap();
        assert_eq!(*ed.world().get::<Transform>(cube).unwrap(), before);
        let errors: Vec<_> = ed
            .app
            .world
            .resource::<Console>()
            .lines()
            .iter()
            .filter(|l| l.level == kit::core::LogLevel::Error)
            .map(|l| l.text.clone())
            .collect();
        assert!(errors.is_empty(), "{errors:?}");

        // Reopen from disk.
        let mut ed2 = Editor::new();
        ed2.open_project(&dir).unwrap();
        assert_eq!(ed2.world().entity_count(), 4);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn reparent_keeps_world_position() {
        let mut ed = Editor::new();
        ed.new_scene("main.kscene");
        let a = ed.spawn(Preset::Empty, None);
        ed.app.world.get_mut::<Transform>(a).unwrap().translation = Vec3::new(5.0, 0.0, 0.0);
        let b = ed.spawn(Preset::Empty, None);
        ed.app.world.get_mut::<Transform>(b).unwrap().translation = Vec3::new(1.0, 2.0, 3.0);
        kit::scene::propagate_transforms(&mut ed.app.world);
        ed.reparent(b, Some(a), 0);
        let g = ed.world().get::<GlobalTransform>(b).unwrap().translation();
        assert!((g - Vec3::new(1.0, 2.0, 3.0)).length() < 1e-4);
        assert!((ed.world().get::<Transform>(b).unwrap().translation - Vec3::new(-4.0, 2.0, 3.0)).length() < 1e-4);
        ed.reparent(b, None, 0);
        assert_eq!(kit::scene::roots(ed.world())[0], b);
    }
}

/// Copy `bytes` to `<folder>/<name>` in the project, picking `name-2.ext`, …
/// when a different file already has that name. Returns the project path.
fn copy_into_project(root: &Path, folder: &str, name: &str, bytes: &[u8]) -> Result<String> {
    std::fs::create_dir_all(root.join(folder))?;
    let (stem, ext) = name.rsplit_once('.').unwrap_or((name, ""));
    let mut rel = format!("{folder}/{name}");
    for n in 2.. {
        let target = root.join(&rel);
        if !target.exists() {
            std::fs::write(&target, bytes)?;
            break;
        }
        if std::fs::read(&target).is_ok_and(|b| b == bytes) {
            break; // the same file is already there
        }
        rel = format!("{folder}/{stem}-{n}.{ext}");
    }
    Ok(rel)
}
