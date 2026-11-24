//! # kit_scene
//!
//! Kit's scene graph, built on the ECS (the role of Defold's `gameobject` transforms):
//!
//! * [`Transform`] — local translation / rotation / scale.
//! * [`GlobalTransform`] — world matrix, computed every frame by [`propagate_transforms`].
//! * [`Parent`] / [`Children`] — the hierarchy. Always edit it through
//!   [`set_parent`], [`remove_parent`] and [`despawn_recursive`] so both sides stay in sync.
//! * [`Name`] — a display name for the editor and for lookups from scripts.

use glam::{EulerRot, Mat4, Quat, Vec3};
use kit_core::{App, Module, Stage};
use kit_ecs::{Entity, World};
use serde::{Deserialize, Serialize};

/// Display name of an entity.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Name(pub String);

/// Local transform relative to the parent (or the world, for roots).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Transform {
    pub translation: Vec3,
    pub rotation: Quat,
    pub scale: Vec3,
}

impl Default for Transform {
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl Transform {
    pub const IDENTITY: Self = Self { translation: Vec3::ZERO, rotation: Quat::IDENTITY, scale: Vec3::ONE };

    pub fn from_xyz(x: f32, y: f32, z: f32) -> Self {
        Self { translation: Vec3::new(x, y, z), ..Self::IDENTITY }
    }

    pub fn with_scale(mut self, s: Vec3) -> Self {
        self.scale = s;
        self
    }

    pub fn with_rotation(mut self, r: Quat) -> Self {
        self.rotation = r;
        self
    }

    pub fn matrix(&self) -> Mat4 {
        Mat4::from_scale_rotation_translation(self.scale, self.rotation, self.translation)
    }

    pub fn from_matrix(m: Mat4) -> Self {
        let (scale, rotation, translation) = m.to_scale_rotation_translation();
        Self { translation, rotation, scale }
    }

    /// Rotation as XYZ Euler angles in degrees (what the editor and scripts show).
    pub fn euler_degrees(&self) -> Vec3 {
        let (x, y, z) = self.rotation.to_euler(EulerRot::XYZ);
        Vec3::new(x.to_degrees(), y.to_degrees(), z.to_degrees())
    }

    pub fn set_euler_degrees(&mut self, deg: Vec3) {
        self.rotation = Quat::from_euler(EulerRot::XYZ, deg.x.to_radians(), deg.y.to_radians(), deg.z.to_radians());
    }

    /// Rotate so that -Z points at `target` (camera convention).
    pub fn look_at(&mut self, target: Vec3, up: Vec3) {
        let forward = (target - self.translation).normalize_or_zero();
        if forward == Vec3::ZERO {
            return;
        }
        self.rotation = glam::camera::rh::view::look_to_quat(forward, up.normalize()).inverse();
    }

    pub fn forward(&self) -> Vec3 {
        self.rotation * Vec3::NEG_Z
    }
    pub fn right(&self) -> Vec3 {
        self.rotation * Vec3::X
    }
    pub fn up(&self) -> Vec3 {
        self.rotation * Vec3::Y
    }
}

/// World-space matrix. Written by [`propagate_transforms`]; read by rendering.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GlobalTransform(pub Mat4);

impl Default for GlobalTransform {
    fn default() -> Self {
        Self(Mat4::IDENTITY)
    }
}

impl GlobalTransform {
    pub fn translation(&self) -> Vec3 {
        self.0.w_axis.truncate()
    }
}

/// The parent of an entity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Parent(pub Entity);

/// Ordered children of an entity.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Children(pub Vec<Entity>);

/// Spawn an entity with a name and transform (and a global transform slot).
pub fn spawn_node(world: &mut World, name: &str, transform: Transform) -> Entity {
    let e = world.spawn();
    world.insert(e, Name(name.to_string()));
    world.insert(e, transform);
    world.insert(e, GlobalTransform(transform.matrix()));
    e
}

/// Is `ancestor` equal to `e` or one of its ancestors?
pub fn is_ancestor_or_self(world: &World, ancestor: Entity, e: Entity) -> bool {
    let mut cur = Some(e);
    while let Some(c) = cur {
        if c == ancestor {
            return true;
        }
        cur = world.get::<Parent>(c).map(|p| p.0);
    }
    false
}

/// Make `child` a child of `parent` (appended last). Detaches it from any previous parent.
/// Returns `false` (and does nothing) if this would create a cycle.
pub fn set_parent(world: &mut World, child: Entity, parent: Entity) -> bool {
    set_parent_at(world, child, parent, usize::MAX)
}

/// Like [`set_parent`] but inserts the child at `index` among its siblings.
pub fn set_parent_at(world: &mut World, child: Entity, parent: Entity, index: usize) -> bool {
    if is_ancestor_or_self(world, child, parent) {
        return false;
    }
    remove_parent(world, child);
    world.insert(child, Parent(parent));
    if !world.has::<Children>(parent) {
        world.insert(parent, Children::default());
    }
    let mut kids = world.get_mut::<Children>(parent).unwrap();
    let i = index.min(kids.0.len());
    kids.0.insert(i, child);
    true
}

/// Detach `child` from its parent, making it a root.
pub fn remove_parent(world: &mut World, child: Entity) {
    if let Some(Parent(p)) = world.remove::<Parent>(child) {
        let empty = if let Some(mut kids) = world.get_mut::<Children>(p) {
            kids.0.retain(|c| *c != child);
            kids.0.is_empty()
        } else {
            false
        };
        if empty {
            world.remove::<Children>(p);
        }
    }
}

/// Children of `e` (empty if none).
pub fn children(world: &World, e: Entity) -> Vec<Entity> {
    world.get::<Children>(e).map(|c| c.0.clone()).unwrap_or_default()
}

/// `e` and all of its descendants, depth-first (parents before children).
pub fn descendants_and_self(world: &World, e: Entity) -> Vec<Entity> {
    let mut out = Vec::new();
    let mut stack = vec![e];
    while let Some(n) = stack.pop() {
        out.push(n);
        let kids = children(world, n);
        stack.extend(kids.into_iter().rev());
    }
    out
}

/// Despawn `e` and its whole subtree.
pub fn despawn_recursive(world: &mut World, e: Entity) {
    if !world.is_alive(e) {
        return;
    }
    remove_parent(world, e);
    for n in descendants_and_self(world, e) {
        world.despawn(n);
    }
}

/// Scene roots: entities with a [`Transform`] and no [`Parent`], in a stable order.
pub fn roots(world: &World) -> Vec<Entity> {
    let mut r: Vec<Entity> = world.query::<(Transform,)>().into_iter().filter(|e| !world.has::<Parent>(*e)).collect();
    r.sort_by_key(|e| world.get::<RootOrder>(*e).map(|o| o.0).unwrap_or(u32::MAX));
    r
}

/// Optional sort key for root entities (the editor's hierarchy order).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RootOrder(pub u32);

/// Find the first entity with the given name.
pub fn find_by_name(world: &World, name: &str) -> Option<Entity> {
    let names = world.storage::<Name>()?;
    names.iter().find(|(_, n)| n.0 == name).map(|(e, _)| e)
}

/// Recompute every [`GlobalTransform`] from the hierarchy.
pub fn propagate_transforms(world: &mut World) {
    let roots = roots(world);
    let mut stack: Vec<(Entity, Mat4)> = roots.into_iter().map(|e| (e, Mat4::IDENTITY)).collect();
    while let Some((e, parent_m)) = stack.pop() {
        let Some(local) = world.get::<Transform>(e).map(|t| t.matrix()) else { continue };
        let m = parent_m * local;
        let updated = match world.get_mut::<GlobalTransform>(e) {
            Some(mut g) => {
                g.0 = m;
                true
            }
            None => false,
        };
        if !updated {
            world.insert(e, GlobalTransform(m));
        }
        for c in children(world, e) {
            stack.push((c, m));
        }
    }
}

/// Set an entity's local transform so that its world matrix becomes `world_m`.
pub fn set_world_matrix(world: &mut World, e: Entity, world_m: Mat4) {
    let parent_m =
        world.get::<Parent>(e).and_then(|p| world.get::<GlobalTransform>(p.0).map(|g| g.0)).unwrap_or(Mat4::IDENTITY);
    let local = parent_m.inverse() * world_m;
    if let Some(mut t) = world.get_mut::<Transform>(e) {
        *t = Transform::from_matrix(local);
    }
}

/// Registers transform propagation in [`Stage::PostUpdate`].
pub struct SceneModule;

impl Module for SceneModule {
    fn name(&self) -> &'static str {
        "scene"
    }
    fn build(&self, app: &mut App) {
        app.add_system(Stage::PostUpdate, "propagate_transforms", propagate_transforms);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: Vec3, b: Vec3) -> bool {
        (a - b).length() < 1e-4
    }

    #[test]
    fn hierarchy_propagates() {
        let mut w = World::new();
        let a = spawn_node(&mut w, "a", Transform::from_xyz(1.0, 0.0, 0.0));
        let b = spawn_node(&mut w, "b", Transform::from_xyz(0.0, 2.0, 0.0));
        let c = spawn_node(&mut w, "c", Transform::from_xyz(0.0, 0.0, 3.0));
        assert!(set_parent(&mut w, b, a));
        assert!(set_parent(&mut w, c, b));
        propagate_transforms(&mut w);
        assert!(approx(w.get::<GlobalTransform>(c).unwrap().translation(), Vec3::new(1.0, 2.0, 3.0)));
        assert_eq!(roots(&w), vec![a]);
        assert_eq!(descendants_and_self(&w, a), vec![a, b, c]);
    }

    #[test]
    fn rotation_and_scale_compose() {
        let mut w = World::new();
        let a = spawn_node(
            &mut w,
            "a",
            Transform::IDENTITY
                .with_rotation(Quat::from_rotation_y(std::f32::consts::FRAC_PI_2))
                .with_scale(Vec3::splat(2.0)),
        );
        let b = spawn_node(&mut w, "b", Transform::from_xyz(1.0, 0.0, 0.0));
        set_parent(&mut w, b, a);
        propagate_transforms(&mut w);
        // +X rotated 90° about Y is -Z, then scaled by 2.
        assert!(approx(w.get::<GlobalTransform>(b).unwrap().translation(), Vec3::new(0.0, 0.0, -2.0)));
    }

    #[test]
    fn cycles_rejected_and_reparent() {
        let mut w = World::new();
        let a = spawn_node(&mut w, "a", Transform::IDENTITY);
        let b = spawn_node(&mut w, "b", Transform::IDENTITY);
        let c = spawn_node(&mut w, "c", Transform::IDENTITY);
        set_parent(&mut w, b, a);
        assert!(!set_parent(&mut w, a, b));
        assert!(!set_parent(&mut w, a, a));
        set_parent(&mut w, b, c);
        assert!(!w.has::<Children>(a));
        assert_eq!(children(&w, c), vec![b]);
    }

    #[test]
    fn despawn_subtree() {
        let mut w = World::new();
        let a = spawn_node(&mut w, "a", Transform::IDENTITY);
        let b = spawn_node(&mut w, "b", Transform::IDENTITY);
        let c = spawn_node(&mut w, "c", Transform::IDENTITY);
        let keep = spawn_node(&mut w, "keep", Transform::IDENTITY);
        set_parent(&mut w, b, a);
        set_parent(&mut w, c, b);
        set_parent(&mut w, a, keep);
        despawn_recursive(&mut w, a);
        assert_eq!(w.entity_count(), 1);
        assert!(!w.has::<Children>(keep));
        assert_eq!(find_by_name(&w, "keep"), Some(keep));
    }

    #[test]
    fn world_matrix_roundtrip() {
        let mut w = World::new();
        let p = spawn_node(&mut w, "p", Transform::from_xyz(5.0, 0.0, 0.0));
        let c = spawn_node(&mut w, "c", Transform::IDENTITY);
        set_parent(&mut w, c, p);
        propagate_transforms(&mut w);
        set_world_matrix(&mut w, c, Mat4::from_translation(Vec3::new(1.0, 1.0, 1.0)));
        propagate_transforms(&mut w);
        assert!(approx(w.get::<Transform>(c).unwrap().translation, Vec3::new(-4.0, 1.0, 1.0)));
        assert!(approx(w.get::<GlobalTransform>(c).unwrap().translation(), Vec3::new(1.0, 1.0, 1.0)));
    }

    #[test]
    fn look_at_points_forward() {
        let mut t = Transform::from_xyz(0.0, 0.0, 5.0);
        t.look_at(Vec3::new(5.0, 0.0, 5.0), Vec3::Y);
        assert!(approx(t.forward(), Vec3::X));
    }

    #[test]
    fn euler_roundtrip() {
        let mut t = Transform::IDENTITY;
        t.set_euler_degrees(Vec3::new(10.0, 20.0, 30.0));
        assert!(approx(t.euler_degrees(), Vec3::new(10.0, 20.0, 30.0)));
    }
}
