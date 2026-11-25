//! # kit_xui
//!
//! Reusable Kit interfaces. An interface is stored as a small HTML-like
//! `.xui` document and attached to a scene camera. The parser is deliberately
//! independent of egui or a renderer so the editor preview and game runtime
//! can share the same document and layout data.

pub mod doc;
pub mod edit;
pub mod layout;
pub mod paint;
pub mod runtime;
pub mod style;

pub use doc::{Document, Node, ParseError};
pub use epaint;

use anyhow::{Result, bail};
use kit_core::{App, Module, Stage};
use kit_ecs::Entity;
use kit_gamesys::InterfaceAttachment;
use kit_resource::Resources;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub const XUI_EXTENSION: &str = "xui";
pub const XUI_VERSION: u32 = 1;

pub const STARTER_TEMPLATE: &str = r##"<interface width="1280" height="720">
  <panel id="menu" anchor="center" width="420" padding="32" gap="16" align="center" background="#17152bee" border-radius="16">
    <label id="title" text="Paused" font-size="2em" color="#f4efff" />
    <button id="resume" text="Resume" script="Resume.x" width="fill" height="56" background="#7c5cff" hover-background="#9478ff" />
  </panel>
</interface>
"##;

/// A parsed `.xui` asset.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct InterfaceFile {
    #[serde(default = "default_version")]
    pub version: u32,
    pub source: String,
}

fn default_version() -> u32 {
    XUI_VERSION
}

impl InterfaceFile {
    pub fn starter() -> Self {
        Self { version: XUI_VERSION, source: STARTER_TEMPLATE.to_owned() }
    }

    pub fn parse(source: &str) -> Result<Self> {
        let document = Document::parse(source)?;
        if document.root.tag != "interface" {
            bail!("the root element must be <interface>");
        }
        Ok(Self { version: XUI_VERSION, source: source.to_owned() })
    }

    pub fn document(&self) -> Result<Document> {
        Document::parse(&self.source)
    }
}

/// Per-camera runtime state. Every attachment gets an independent class/state
/// set even when several cameras use the same asset.
#[derive(Clone, Debug, Default)]
pub struct InterfaceState {
    pub classes: HashMap<String, Vec<String>>,
    pub visible: HashMap<String, bool>,
}

#[derive(Clone, Debug, Default)]
pub struct InterfaceRuntime {
    pub instances: HashMap<Entity, InterfaceState>,
}

pub fn interface_runtime(world: &kit_ecs::World) -> Option<std::cell::Ref<'_, InterfaceRuntime>> {
    world.try_resource::<InterfaceRuntime>()
}

pub fn activate_class(world: &mut kit_ecs::World, camera: Entity, id: &str, class: &str) {
    let mut runtime = world.resource_mut::<InterfaceRuntime>();
    let classes = runtime.instances.entry(camera).or_default().classes.entry(id.to_owned()).or_default();
    if !classes.iter().any(|existing| existing == class) {
        classes.push(class.to_owned());
    }
}

pub fn set_visible(world: &mut kit_ecs::World, camera: Entity, id: &str, visible: bool) {
    world.resource_mut::<InterfaceRuntime>().instances.entry(camera).or_default().visible.insert(id.to_owned(), visible);
}

pub struct XuiModule;

impl Module for XuiModule {
    fn name(&self) -> &'static str {
        "xui"
    }

    fn dependencies(&self) -> &'static [&'static str] {
        &["resource", "scene", "gamesys", "script"]
    }

    fn build(&self, app: &mut App) {
        app.world.insert_resource(InterfaceRuntime::default());
        app.add_play_system(Stage::PreUpdate, "xui_instances", sync_instances);
        app.add_play_system(Stage::PostUpdate, "xui_update", runtime::update_ui);
    }
}

fn sync_instances(world: &mut kit_ecs::World) {
    let cameras = world.query::<(kit_gamesys::Camera, InterfaceAttachment)>();
    let mut runtime = world.resource_mut::<InterfaceRuntime>();
    runtime.instances.retain(|camera, _| cameras.contains(camera));
    for camera in cameras {
        runtime.instances.entry(camera).or_default();
    }
}

/// Load and validate an interface from project resources.
pub fn load(resources: &Resources, path: &str) -> Result<InterfaceFile> {
    let text = resources.read_text(path)?;
    InterfaceFile::parse(&text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_starter_and_round_trips() {
        let file = InterfaceFile::starter();
        let mut doc = file.document().unwrap();
        assert_eq!(doc.root.tag, "interface");
        assert!(doc.root.find_mut("resume").is_some());
        assert!(doc.to_source().contains("Paused"));
    }

    #[test]
    fn text_and_attributes_round_trip_with_markup_characters() {
        let doc = Document::parse(r#"<interface><!-- use > here --><label text='x > y &amp; "quoted"'>a &lt; b</label></interface>"#).unwrap();
        let label = &doc.root.children[0];
        assert_eq!(label.attr("text"), Some(r#"x > y & "quoted""#));
        assert_eq!(label.attr("_text"), Some("a < b"));
        assert_eq!(Document::parse(&doc.to_source()).unwrap(), doc);
    }

    #[test]
    fn reports_mismatched_tags() {
        let error = InterfaceFile::parse("<interface><panel></interface>").unwrap_err().to_string();
        assert!(error.contains("does not match"));
    }
}
