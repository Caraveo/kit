//! CPU picking: which entity is under a screen point.

use glam::{Mat4, Vec2, Vec3, Vec4Swizzles};
use kit_ecs::{Entity, World};
use kit_gamesys::{AudioSource, Camera, PostVolume, Light, MeshKind, Model, Sprite, Terrain};
use kit_scene::GlobalTransform;

/// A world-space ray.
#[derive(Clone, Copy, Debug)]
pub struct Ray {
    pub origin: Vec3,
    pub dir: Vec3,
}

impl Ray {
    /// Ray through a point in normalized device coordinates (x, y in -1..1,
    /// y down as in Vulkan) for the given view-projection matrix.
    pub fn from_ndc(view_proj: Mat4, ndc: Vec2) -> Ray {
        let inv = view_proj.inverse();
        let near = inv * ndc.extend(0.0).extend(1.0);
        let far = inv * ndc.extend(1.0).extend(1.0);
        let near = near.xyz() / near.w;
        let far = far.xyz() / far.w;
        Ray { origin: near, dir: (far - near).normalize_or(Vec3::NEG_Z) }
    }

    /// Distance along the ray to a sphere, if it hits.
    pub fn hit_sphere(&self, center: Vec3, radius: f32) -> Option<f32> {
        let oc = self.origin - center;
        let b = oc.dot(self.dir);
        let c = oc.length_squared() - radius * radius;
        let disc = b * b - c;
        if disc < 0.0 {
            return None;
        }
        let t = -b - disc.sqrt();
        let t = if t < 0.0 { -b + disc.sqrt() } else { t };
        (t >= 0.0).then_some(t)
    }

    /// Distance to a unit box (-0.5..0.5) transformed by `m`.
    pub fn hit_box(&self, m: Mat4, half: Vec3) -> Option<f32> {
        let inv = m.inverse();
        let o = inv.transform_point3(self.origin);
        let d = inv.transform_vector3(self.dir);
        let mut tmin = f32::NEG_INFINITY;
        let mut tmax = f32::INFINITY;
        for i in 0..3 {
            if d[i].abs() < 1e-8 {
                if o[i] < -half[i] || o[i] > half[i] {
                    return None;
                }
            } else {
                let t1 = (-half[i] - o[i]) / d[i];
                let t2 = (half[i] - o[i]) / d[i];
                tmin = tmin.max(t1.min(t2));
                tmax = tmax.min(t1.max(t2));
            }
        }
        if tmax < tmin.max(0.0) {
            return None;
        }
        let t_local = if tmin >= 0.0 { tmin } else { tmax };
        // Convert local t back to world distance.
        let hit = m.transform_point3(o + d * t_local);
        Some((hit - self.origin).length())
    }
}

/// The closest model or sprite hit by `ray`.
pub fn pick(world: &World, ray: &Ray) -> Option<Entity> {
    let mut best: Option<(f32, Entity)> = None;
    let mut consider = |t: f32, e: Entity| {
        if best.is_none_or(|(bt, _)| t < bt) {
            best = Some((t, e));
        }
    };
    if let Some(models) = world.storage::<Model>() {
        for (e, m) in models.iter() {
            if !m.visible {
                continue;
            }
            let Some(g) = world.get::<GlobalTransform>(e).map(|g| g.0) else { continue };
            let t = if !m.mesh_asset.is_empty() {
                crate::model_bounds(world, &m.mesh_asset).and_then(|(lo, hi)| {
                    let center = lo * 0.5 + hi * 0.5;
                    let half = (hi * 0.5 - lo * 0.5).max(Vec3::splat(0.001));
                    ray.hit_box(g * Mat4::from_translation(center), half)
                })
            } else {
                match m.mesh {
                    MeshKind::Cube => ray.hit_box(g, Vec3::splat(0.5)),
                    MeshKind::Plane => ray.hit_box(g, Vec3::new(0.5, 0.01, 0.5)),
                    MeshKind::Quad => ray.hit_box(g, Vec3::new(0.5, 0.5, 0.01)),
                    MeshKind::Cylinder => ray.hit_box(g, Vec3::new(0.5, 0.5, 0.5)),
                    MeshKind::Torus => ray.hit_box(g, Vec3::new(0.5, 0.15, 0.5)),
                    MeshKind::Sphere => {
                        let (s, _, p) = g.to_scale_rotation_translation();
                        ray.hit_sphere(p, 0.5 * s.max_element())
                    }
                }
            };
            if let Some(t) = t {
                consider(t, e);
            }
        }
    }
    if let Some(sprites) = world.storage::<Sprite>() {
        for (e, s) in sprites.iter() {
            if !s.visible {
                continue;
            }
            let Some(g) = world.get::<GlobalTransform>(e).map(|g| g.0) else { continue };
            let m = g * Mat4::from_scale(s.size.extend(1.0));
            if let Some(t) = ray.hit_box(m, Vec3::new(0.5, 0.5, 0.01)) {
                consider(t, e);
            }
        }
    }
    // Cameras, lights and sounds are picked by their editor gizmo.
    let gizmos = world.query::<(Camera,)>().into_iter().chain(world.query::<(Light,)>());
    for e in gizmos.chain(world.query::<(AudioSource,)>()).chain(world.query::<(PostVolume,)>()) {
        if world.has::<Model>(e) || world.has::<Sprite>(e) {
            continue;
        }
        if let Some(g) = world.get::<GlobalTransform>(e) {
            if let Some(t) = ray.hit_sphere(g.translation(), 0.45) {
                consider(t, e);
            }
        }
    }
    if let Some(terrains) = world.storage::<Terrain>() {
        for (e, t) in terrains.iter() {
            if let Some((dist, _)) = terrain_hit(world, e, t, ray) {
                consider(dist, e);
            }
        }
    }
    best.map(|(_, e)| e)
}

/// Where a ray hits a terrain entity: (world distance, world point).
pub fn terrain_hit(world: &World, e: Entity, t: &Terrain, ray: &Ray) -> Option<(f32, Vec3)> {
    let g = world.get::<GlobalTransform>(e).map(|g| g.0)?;
    let inv = g.inverse();
    let o = inv.transform_point3(ray.origin);
    let d = inv.transform_vector3(ray.dir);
    let local_t = t.raycast(o, d, 10_000.0)?;
    let hit = g.transform_point3(o + d.normalize() * local_t);
    Some(((hit - ray.origin).length(), hit))
}

#[cfg(test)]
mod tests {
    use super::*;
    use kit_scene::Transform;

    #[test]
    fn picks_closest() {
        let mut w = World::new();
        let near = kit_scene::spawn_node(&mut w, "near", Transform::from_xyz(0.0, 0.0, 2.0));
        w.insert(near, Model::default());
        let far = kit_scene::spawn_node(&mut w, "far", Transform::from_xyz(0.0, 0.0, -2.0));
        w.insert(far, Model { mesh: MeshKind::Sphere, ..Default::default() });
        let off = kit_scene::spawn_node(&mut w, "sprite", Transform::from_xyz(5.0, 0.0, 0.0));
        w.insert(off, Sprite::default());
        kit_scene::propagate_transforms(&mut w);
        let ray = Ray { origin: Vec3::new(0.0, 0.0, 10.0), dir: Vec3::NEG_Z };
        assert_eq!(pick(&w, &ray), Some(near));
        let ray = Ray { origin: Vec3::new(5.0, 0.0, 10.0), dir: Vec3::NEG_Z };
        assert_eq!(pick(&w, &ray), Some(off));
        let ray = Ray { origin: Vec3::new(0.0, 5.0, 10.0), dir: Vec3::NEG_Z };
        assert_eq!(pick(&w, &ray), None);
    }

    #[test]
    fn ndc_ray_goes_forward() {
        let view = glam::camera::rh::view::look_at_mat4(Vec3::new(0.0, 0.0, 5.0), Vec3::ZERO, Vec3::Y);
        let proj = glam::camera::rh::proj::vulkan::perspective(1.0, 1.0, 0.1, 100.0);
        let r = Ray::from_ndc(proj * view, Vec2::ZERO);
        assert!((r.dir - Vec3::NEG_Z).length() < 1e-4);
        // Vulkan NDC y is down: top of the screen is -1 and should point up in world.
        let up = Ray::from_ndc(proj * view, Vec2::new(0.0, -0.9));
        assert!(up.dir.y > 0.0);
    }
}
