//! # kit_resource
//!
//! Project and resource access (the role of Defold's `resource` module).
//!
//! A Kit project is a folder with a `kit.project` file (TOML, like Defold's
//! `game.project`). Every resource path in scenes and scripts is relative to that
//! folder and uses `/` separators, e.g. `scripts/player.x`.

pub mod model;
pub mod pack;

use anyhow::{Context, Result, bail};
use kit_core::{App, Module};
use serde::{Deserialize, Serialize};
use std::path::{Component, Path, PathBuf};
use std::time::SystemTime;

pub const PROJECT_FILE: &str = "kit.project";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct ProjectSection {
    pub title: String,
    pub version: String,
    /// Scene loaded when the game starts.
    pub main_scene: String,
}

impl Default for ProjectSection {
    fn default() -> Self {
        Self { title: "Untitled".into(), version: "0.0.1".into(), main_scene: "main.kscene".into() }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct DisplaySection {
    pub width: u32,
    pub height: u32,
    pub vsync: bool,
    pub fullscreen: bool,
}

impl Default for DisplaySection {
    fn default() -> Self {
        Self { width: 1280, height: 720, vsync: true, fullscreen: false }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct PhysicsSection {
    /// Simulate rigid bodies. When off, colliders still detect touches and triggers.
    pub enabled: bool,
    /// Gravity in m/s². Use `[0, -9.81, 0]` for 3D and 2D side views.
    pub gravity: [f32; 3],
    /// Simulation steps per second.
    pub rate: u32,
}

impl Default for PhysicsSection {
    fn default() -> Self {
        Self { enabled: true, gravity: [0.0, -9.81, 0.0], rate: 60 }
    }
}

/// Contents of `kit.project`.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct ProjectConfig {
    pub project: ProjectSection,
    pub display: DisplaySection,
    pub physics: PhysicsSection,
}

impl ProjectConfig {
    pub fn load(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))
    }

    /// Parse `kit.project` text.
    pub fn parse(text: &str) -> Result<Self> {
        toml::from_str(text).context("parsing kit.project")
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        std::fs::write(path, toml::to_string_pretty(self)?).with_context(|| format!("writing {}", path.display()))
    }
}

/// Decoded RGBA8 image.
pub struct ImageData {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// Resource access rooted at the project folder, or (on the web) at an
/// in-memory copy of the project's files.
#[derive(Clone, Debug)]
pub struct Resources {
    root: PathBuf,
    /// Project-relative path → contents, when the project lives in memory.
    memory: Option<std::sync::Arc<std::collections::HashMap<String, Vec<u8>>>>,
}

impl Resources {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into(), memory: None }
    }

    /// A project held in memory (the web player loads one from a game pack).
    pub fn in_memory(files: std::collections::HashMap<String, Vec<u8>>) -> Self {
        Self { root: PathBuf::from("/game"), memory: Some(std::sync::Arc::new(files)) }
    }

    /// The in-memory file for a project-relative path.
    fn memory_file(&self, rel: &str) -> Option<Result<&[u8]>> {
        let files = self.memory.as_ref()?;
        let key = rel.trim().trim_start_matches('/').replace('\\', "/");
        Some(files.get(&key).map(Vec::as_slice).with_context(|| format!("reading {rel}: not in the game files")))
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Turn a project-relative path into a filesystem path. Rejects paths that
    /// escape the project folder.
    pub fn resolve(&self, rel: &str) -> Result<PathBuf> {
        let rel = rel.trim().trim_start_matches('/');
        let p = Path::new(rel);
        if p.components().any(|c| matches!(c, Component::ParentDir | Component::RootDir | Component::Prefix(_))) {
            bail!("resource path `{rel}` must stay inside the project");
        }
        Ok(self.root.join(p))
    }

    /// Inverse of [`Resources::resolve`]: project-relative path with `/` separators.
    pub fn relative(&self, abs: &Path) -> Option<String> {
        let r = abs.strip_prefix(&self.root).ok()?;
        Some(r.components().map(|c| c.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/"))
    }

    pub fn exists(&self, rel: &str) -> bool {
        if let Some(file) = self.memory_file(rel) {
            return file.is_ok();
        }
        self.resolve(rel).map(|p| p.is_file()).unwrap_or(false)
    }

    pub fn read_text(&self, rel: &str) -> Result<String> {
        if let Some(file) = self.memory_file(rel) {
            return String::from_utf8(file?.to_vec()).with_context(|| format!("{rel} is not UTF-8 text"));
        }
        let p = self.resolve(rel)?;
        std::fs::read_to_string(&p).with_context(|| format!("reading {rel}"))
    }

    pub fn read_bytes(&self, rel: &str) -> Result<Vec<u8>> {
        if let Some(file) = self.memory_file(rel) {
            return Ok(file?.to_vec());
        }
        let p = self.resolve(rel)?;
        std::fs::read(&p).with_context(|| format!("reading {rel}"))
    }

    pub fn write_text(&self, rel: &str, text: &str) -> Result<()> {
        anyhow::ensure!(self.memory.is_none(), "can't write {rel}: the game's files are read-only");
        let p = self.resolve(rel)?;
        if let Some(dir) = p.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(&p, text).with_context(|| format!("writing {rel}"))
    }

    pub fn modified(&self, rel: &str) -> Option<SystemTime> {
        if self.memory.is_some() {
            return None;
        }
        self.resolve(rel).ok()?.metadata().ok()?.modified().ok()
    }

    pub fn load_image(&self, rel: &str) -> Result<ImageData> {
        let bytes = self.read_bytes(rel)?;
        let img = image::load_from_memory(&bytes).with_context(|| format!("decoding image {rel}"))?.to_rgba8();
        Ok(ImageData { width: img.width(), height: img.height(), rgba: img.into_raw() })
    }

    /// Load a 3D model (`.glb`, `.gltf`, `.obj`) from the project.
    pub fn load_model(&self, rel: &str) -> Result<model::ImportedModel> {
        let bytes = self.read_bytes(rel)?;
        model::load(rel, &bytes, &|other| self.read_bytes(other))
    }

    /// All files under the project with the given extension (project-relative, sorted).
    pub fn list(&self, ext: &str) -> Vec<String> {
        if let Some(files) = &self.memory {
            let mut out: Vec<String> = files
                .keys()
                .filter(|k| Path::new(k).extension().is_some_and(|x| x.eq_ignore_ascii_case(ext)))
                .cloned()
                .collect();
            out.sort();
            return out;
        }
        let mut out = Vec::new();
        let mut stack = vec![self.root.clone()];
        while let Some(dir) = stack.pop() {
            let Ok(entries) = std::fs::read_dir(&dir) else { continue };
            for e in entries.flatten() {
                let p = e.path();
                let name = e.file_name().to_string_lossy().to_string();
                if name.starts_with('.') || name == "target" || name == "build" {
                    continue;
                }
                if p.is_dir() {
                    // A folder with its own project file is another game (an
                    // export, or a nested project), not this project's assets.
                    if !p.join(PROJECT_FILE).is_file() {
                        stack.push(p);
                    }
                } else if p.extension().is_some_and(|x| x.eq_ignore_ascii_case(ext)) {
                    if let Some(r) = self.relative(&p) {
                        out.push(r);
                    }
                }
            }
        }
        out.sort();
        out
    }
}

/// An opened project: its folder and config.
#[derive(Clone, Debug)]
pub struct Project {
    pub resources: Resources,
    pub config: ProjectConfig,
}

impl Project {
    /// A project held in memory (from a verified game pack).
    pub fn from_files(files: std::collections::HashMap<String, Vec<u8>>) -> Result<Self> {
        let resources = Resources::in_memory(files);
        let config = ProjectConfig::parse(&resources.read_text(PROJECT_FILE)?)?;
        Ok(Self { resources, config })
    }

    /// Open a project from its folder or its `kit.project` file.
    pub fn open(path: &Path) -> Result<Self> {
        let file = if path.is_dir() { path.join(PROJECT_FILE) } else { path.to_path_buf() };
        let root = file.parent().context("project path has no folder")?.to_path_buf();
        let config = ProjectConfig::load(&file)?;
        Ok(Self { resources: Resources::new(root), config })
    }

    /// Create a new project folder with a `kit.project` file.
    pub fn create(dir: &Path, title: &str) -> Result<Self> {
        std::fs::create_dir_all(dir)?;
        let mut config = ProjectConfig::default();
        config.project.title = title.to_string();
        config.save(&dir.join(PROJECT_FILE))?;
        Ok(Self { resources: Resources::new(dir), config })
    }

    pub fn save_config(&self) -> Result<()> {
        self.config.save(&self.resources.root().join(PROJECT_FILE))
    }

    pub fn root(&self) -> &Path {
        self.resources.root()
    }
}

/// Inserts [`Resources`] for the given project root.
pub struct ResourceModule {
    pub root: PathBuf,
}

impl Module for ResourceModule {
    fn name(&self) -> &'static str {
        "resource"
    }
    fn build(&self, app: &mut App) {
        app.world.insert_resource(Resources::new(self.root.clone()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_rejects_escape() {
        let r = Resources::new("/proj");
        assert!(r.resolve("../etc/passwd").is_err());
        assert_eq!(r.resolve("/scripts/a.x").unwrap(), PathBuf::from("/proj/scripts/a.x"));
        assert_eq!(r.relative(Path::new("/proj/scripts/a.x")).unwrap(), "scripts/a.x");
    }

    #[test]
    fn config_roundtrip_and_defaults() {
        let c: ProjectConfig = toml::from_str("[project]\ntitle = \"Demo\"\n").unwrap();
        assert_eq!(c.project.title, "Demo");
        assert_eq!(c.project.main_scene, "main.kscene");
        assert_eq!(c.display.width, 1280);
        let back: ProjectConfig = toml::from_str(&toml::to_string(&c).unwrap()).unwrap();
        assert_eq!(back, c);
    }
}

#[cfg(test)]
mod memory_tests {
    use super::*;

    #[test]
    fn listing_skips_exported_games_inside_the_project() {
        let root = std::env::temp_dir().join(format!("kit-list-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for (path, text) in [
            ("kit.project", "[project]\ntitle = \"T\"\n"),
            ("main.kscene", "{}"),
            ("scenes/level2.kscene", "{}"),
            ("export/linux/T/game/kit.project", "[project]\ntitle = \"T\"\n"),
            ("export/linux/T/game/main.kscene", "{}"),
            ("export/linux/T/game/scenes/level2.kscene", "{}"),
        ] {
            let p = root.join(path);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, text).unwrap();
        }
        assert_eq!(Resources::new(&root).list("kscene"), ["main.kscene", "scenes/level2.kscene"]);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn in_memory_projects_read_without_a_disk() {
        let mut files = std::collections::HashMap::new();
        files.insert("scenes/a.kscene".to_owned(), b"{}".to_vec());
        files.insert("kit.project".to_owned(), b"[project]\ntitle = \"Web\"\n".to_vec());
        let r = Resources::in_memory(files);
        assert_eq!(r.read_text("/scenes/a.kscene").unwrap(), "{}");
        assert!(r.exists("kit.project") && !r.exists("missing.png"));
        assert!(r.read_bytes("missing.png").is_err());
        assert!(r.write_text("x.txt", "no").is_err());
        assert_eq!(r.list("kscene"), ["scenes/a.kscene"]);
        assert_eq!(ProjectConfig::parse(&r.read_text("kit.project").unwrap()).unwrap().project.title, "Web");
    }
}
