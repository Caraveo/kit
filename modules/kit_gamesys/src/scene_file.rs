//! `.kscene` files: a JSON list of entities with their components.

use crate::{
    AudioSource, Camera, Collider, Diagram, InterfaceAttachment, Light, Model, PathFollow, PostVolume, RigidBody, Script,
    Sprite, Terrain,
};
use anyhow::{Context, Result, bail};
use kit_ecs::{Entity, World};
use kit_scene::{Name, RootOrder, Transform};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub const SCENE_EXTENSION: &str = "kscene";
const VERSION: u32 = 1;

/// One entity in a scene file.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct EntityData {
    /// File-local id, used for `parent` links.
    pub id: u32,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent: Option<u32>,
    pub transform: Transform,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub camera: Option<Camera>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interface: Option<InterfaceAttachment>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<Model>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sprite: Option<Sprite>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub light: Option<Light>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub script: Option<Script>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub collider: Option<Collider>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rigid_body: Option<RigidBody>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<PathFollow>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub terrain: Option<Terrain>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audio: Option<AudioSource>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub post_volume: Option<PostVolume>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diagram: Option<Diagram>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rig: Option<crate::Rig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rig_volume: Option<crate::RigVolume>,
    /// Labels for `Scene.FindTagged` and `HasTag`.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    /// Fields this version of Kit does not know (from newer versions or plugins).
    /// Kept and written back so saving never drops them.
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// Unknown scene-file fields carried by an entity (see [`EntityData::extra`]).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ExtraData(pub serde_json::Map<String, serde_json::Value>);

/// A whole scene. Entities are stored parents-first, siblings in order.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SceneFile {
    pub version: u32,
    pub entities: Vec<EntityData>,
}

impl Default for SceneFile {
    fn default() -> Self {
        Self { version: VERSION, entities: Vec::new() }
    }
}

impl SceneFile {
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("scene serializes")
    }

    pub fn from_json(text: &str) -> Result<Self> {
        let s: SceneFile = serde_json::from_str(text).context("invalid scene file")?;
        if s.version > VERSION {
            bail!("scene file version {} is newer than this Kit ({VERSION})", s.version);
        }
        Ok(s)
    }
}

fn entity_data(world: &World, e: Entity, id: u32, parent: Option<u32>) -> EntityData {
    EntityData {
        id,
        name: world.get::<Name>(e).map(|n| n.0.clone()).unwrap_or_default(),
        parent,
        transform: world.get_cloned::<Transform>(e).unwrap_or_default(),
        camera: world.get_cloned::<Camera>(e),
        interface: world.get_cloned::<InterfaceAttachment>(e),
        model: world.get_cloned::<Model>(e),
        sprite: world.get_cloned::<Sprite>(e),
        light: world.get_cloned::<Light>(e),
        script: world.get_cloned::<Script>(e),
        collider: world.get_cloned::<Collider>(e),
        rigid_body: world.get_cloned::<RigidBody>(e),
        path: world.get_cloned::<PathFollow>(e),
        terrain: world.get_cloned::<Terrain>(e),
        audio: world.get_cloned::<AudioSource>(e),
        post_volume: world.get_cloned::<PostVolume>(e),
        diagram: world.get_cloned::<Diagram>(e),
        rig: world.get_cloned::<crate::Rig>(e),
        rig_volume: world.get_cloned::<crate::RigVolume>(e),
        tags: world.get::<crate::Tags>(e).map(|t| t.0.clone()).unwrap_or_default(),
        extra: world.get::<ExtraData>(e).map(|x| x.0.clone()).unwrap_or_default(),
    }
}

fn collect(world: &World, roots: &[Entity]) -> Vec<EntityData> {
    let mut out = Vec::new();
    let mut ids: HashMap<Entity, u32> = HashMap::new();
    for &root in roots {
        for e in kit_scene::descendants_and_self(world, root) {
            let id = out.len() as u32;
            ids.insert(e, id);
            let parent =
                if e == root { None } else { world.get::<kit_scene::Parent>(e).and_then(|p| ids.get(&p.0).copied()) };
            out.push(entity_data(world, e, id, parent));
        }
    }
    out
}

/// Serialize one object and everything inside it (a Fab), with its root at
/// its own place relative to nothing (top level).
pub fn save_subtree(world: &World, e: Entity) -> SceneFile {
    SceneFile { version: VERSION, entities: collect(world, &[e]) }
}

/// Serialize every entity in the scene graph.
pub fn save_scene(world: &World) -> SceneFile {
    let roots = kit_scene::roots(world);
    SceneFile { version: VERSION, entities: collect(world, &roots) }
}

/// Spawn the entities of a scene file into `world` (added to whatever is there).
/// Returns the spawned entities in file order.
pub fn load_scene(world: &mut World, scene: &SceneFile) -> Result<Vec<Entity>> {
    let mut map: HashMap<u32, Entity> = HashMap::new();
    let mut spawned = Vec::with_capacity(scene.entities.len());
    let mut next_root = kit_scene::roots(world).len() as u32;
    for d in &scene.entities {
        let e = kit_scene::spawn_node(world, &d.name, d.transform);
        if map.insert(d.id, e).is_some() {
            bail!("duplicate entity id {} in scene", d.id);
        }
        if let Some(c) = d.camera {
            world.insert(e, c);
        }
        if let Some(i) = &d.interface {
            world.insert(e, i.clone());
        }
        if let Some(m) = &d.model {
            world.insert(e, m.clone());
        }
        if let Some(s) = &d.sprite {
            world.insert(e, s.clone());
        }
        if let Some(l) = d.light {
            world.insert(e, l);
        }
        if let Some(s) = &d.script {
            world.insert(e, s.clone());
        }
        if let Some(c) = &d.collider {
            world.insert(e, c.clone());
        }
        if let Some(b) = &d.rigid_body {
            world.insert(e, b.clone());
        }
        if let Some(p) = &d.path {
            world.insert(e, p.clone());
        }
        if let Some(t) = &d.terrain {
            let mut t = t.clone();
            t.validate();
            world.insert(e, t);
        }
        if let Some(a) = &d.audio {
            world.insert(e, a.clone());
        }
        if let Some(v) = &d.post_volume {
            world.insert(e, v.clone());
        }
        if let Some(g) = &d.diagram {
            world.insert(e, g.clone());
        }
        if let Some(r) = &d.rig {
            world.insert(e, r.clone());
        }
        if let Some(v) = &d.rig_volume {
            world.insert(e, v.clone());
        }
        if !d.tags.is_empty() {
            world.insert(e, crate::Tags(d.tags.clone()));
        }
        if !d.extra.is_empty() {
            world.insert(e, ExtraData(d.extra.clone()));
        }
        spawned.push(e);
    }
    for (d, &e) in scene.entities.iter().zip(&spawned) {
        match d.parent {
            Some(pid) => {
                let p = *map.get(&pid).with_context(|| format!("entity {} has unknown parent {pid}", d.id))?;
                kit_scene::set_parent(world, e, p);
            }
            None => {
                world.insert(e, RootOrder(next_root));
                next_root += 1;
            }
        }
    }
    kit_scene::propagate_transforms(world);
    Ok(spawned)
}

/// Duplicate the subtree rooted at `e` next to it. Returns the new root.
pub fn clone_subtree(world: &mut World, e: Entity) -> Result<Entity> {
    let parent = world.get::<kit_scene::Parent>(e).map(|p| p.0);
    let data = collect(world, &[e]);
    let spawned = load_scene(world, &SceneFile { version: VERSION, entities: data })?;
    let new_root = spawned[0];
    if let Some(p) = parent {
        let idx = kit_scene::children(world, p).iter().position(|c| *c == e).map(|i| i + 1).unwrap_or(usize::MAX);
        world.remove::<RootOrder>(new_root);
        kit_scene::set_parent_at(world, new_root, p, idx);
    } else if let Some(order) = world.get::<RootOrder>(e).map(|o| o.0) {
        // Place right after the original among roots.
        for r in kit_scene::roots(world) {
            if r == new_root {
                continue;
            }
            let o = world.get::<RootOrder>(r).map(|o| o.0).unwrap_or(0);
            if o > order {
                world.get_mut::<RootOrder>(r).unwrap().0 = o + 1;
            }
        }
        world.get_mut::<RootOrder>(new_root).unwrap().0 = order + 1;
    }
    Ok(new_root)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MeshKind;
    use glam::Vec3;

    #[test]
    fn roundtrip() {
        let mut w = World::new();
        let cam = kit_scene::spawn_node(&mut w, "Camera", Transform::from_xyz(0.0, 2.0, 8.0));
        w.insert(cam, Camera::perspective());
        w.insert(cam, InterfaceAttachment { asset: "interfaces/pause-menu.xui".into() });
        let root = kit_scene::spawn_node(&mut w, "Root", Transform::IDENTITY);
        let child = kit_scene::spawn_node(&mut w, "Child", Transform::from_xyz(1.0, 0.0, 0.0));
        w.insert(child, Model { mesh: MeshKind::Sphere, ..Default::default() });
        w.insert(child, Script::new("scripts/spin.x"));
        w.insert(child, crate::Collider::default());
        w.insert(child, crate::RigidBody { kind: crate::BodyKind::Kinematic, ..Default::default() });
        kit_scene::set_parent(&mut w, child, root);
        w.insert(cam, RootOrder(0));
        w.insert(root, RootOrder(1));

        let file = save_scene(&w);
        let json = file.to_json();
        let back = SceneFile::from_json(&json).unwrap();
        assert_eq!(back, file);

        let mut w2 = World::new();
        let spawned = load_scene(&mut w2, &back).unwrap();
        assert_eq!(spawned.len(), 3);
        assert_eq!(save_scene(&w2), file);
        let c = kit_scene::find_by_name(&w2, "Child").unwrap();
        assert_eq!(w2.get::<Model>(c).unwrap().mesh, MeshKind::Sphere);
        let loaded_cam = kit_scene::find_by_name(&w2, "Camera").unwrap();
        assert_eq!(
            w2.get::<InterfaceAttachment>(loaded_cam).unwrap().asset,
            "interfaces/pause-menu.xui"
        );
        let g = w2.get::<kit_scene::GlobalTransform>(c).unwrap().translation();
        assert!((g - Vec3::new(1.0, 0.0, 0.0)).length() < 1e-5);
    }

    #[test]
    fn duplicate_subtree() {
        let mut w = World::new();
        let a = kit_scene::spawn_node(&mut w, "A", Transform::IDENTITY);
        w.insert(a, RootOrder(0));
        let b = kit_scene::spawn_node(&mut w, "B", Transform::IDENTITY);
        kit_scene::set_parent(&mut w, b, a);
        let z = kit_scene::spawn_node(&mut w, "Z", Transform::IDENTITY);
        w.insert(z, RootOrder(1));
        let copy = clone_subtree(&mut w, a).unwrap();
        assert_eq!(kit_scene::roots(&w), vec![a, copy, z]);
        assert_eq!(kit_scene::children(&w, copy).len(), 1);
        let inner = clone_subtree(&mut w, b).unwrap();
        assert_eq!(kit_scene::children(&w, a), vec![b, inner]);
    }

    #[test]
    fn unknown_fields_survive_a_round_trip() {
        let text = r#"{"version":1,"entities":[{"id":0,"name":"x","hologram":{"glow":3}}]}"#;
        let mut w = World::new();
        load_scene(&mut w, &SceneFile::from_json(text).unwrap()).unwrap();
        let json = save_scene(&w).to_json();
        assert!(json.contains("\"hologram\""), "{json}");
        assert!(json.contains("\"glow\": 3"), "{json}");
    }

    #[test]
    fn old_files_fill_defaults() {
        let s =
            SceneFile::from_json(r#"{"version":1,"entities":[{"id":0,"name":"x","model":{"mesh":"Torus"}}]}"#).unwrap();
        assert_eq!(s.entities[0].model.as_ref().unwrap().color, Model::default().color);
    }
}
