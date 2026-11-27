//! Shared directional-light selection for Kit renderers.

use glam::Vec3;
use kit_ecs::{Entity, World};
use kit_gamesys::Light;
use kit_scene::GlobalTransform;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DirectionalLight {
    pub direction: Vec3,
    pub color: Vec3,
}

impl Default for DirectionalLight {
    fn default() -> Self {
        Self { direction: Vec3::new(-0.45, -0.8, -0.4).normalize(), color: Vec3::splat(1.05) }
    }
}

/// Extract the first active, enabled directional [`Light`].
pub fn extract(world: &World) -> DirectionalLight {
    let mut result = DirectionalLight::default();
    let Some(lights) = world.storage::<Light>() else { return result };
    let active = |e: Entity| kit_scene::is_active(world, e) && !kit_gamesys::feature_disabled(world, e, "Light");
    if let Some((entity, light)) = lights.iter().find(|(e, _)| active(*e)) {
        if let Some(transform) = world.get::<GlobalTransform>(entity) {
            result.direction = transform.0.transform_vector3(Vec3::NEG_Z).normalize_or(result.direction);
        }
        result.color = light.color * light.intensity;
    }
    result
}

pub const SHADOW_MAP_SIZE: u32 = 2048;

/// Renderer-owned directional shadow-map configuration.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ShadowMapSettings {
    pub size: u32,
}

impl Default for ShadowMapSettings {
    fn default() -> Self {
        Self { size: SHADOW_MAP_SIZE }
    }
}
