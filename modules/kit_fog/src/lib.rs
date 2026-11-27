//! Drifting fog-cloud emitters.

use glam::{Vec3, Vec3Swizzles};
use kit_core::{App, Module, Stage, Time};
use kit_ecs::{Entity, World};
pub use kit_gamesys::{FogEmitter, FogKind};
use kit_gamesys::{PhysicsApi, Terrain};
use kit_scene::GlobalTransform;

pub const MAX_PER_EMITTER: usize = 256;
pub const MAX_TOTAL: usize = 4096;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FogCloud {
    pub position: Vec3,
    pub velocity: Vec3,
    pub age: f32,
    pub lifetime: f32,
    pub size: f32,
    pub phase: f32,
    pub surface_fade: f32,
    pub surface_scale: f32,
}

impl FogCloud {
    pub fn opacity(&self) -> f32 {
        let t = (self.age / self.lifetime.max(1e-4)).clamp(0.0, 1.0);
        let fade_in = (t * 8.0).min(1.0);
        let fade_out = ((1.0 - t) * 4.0).min(1.0);
        fade_in * fade_out * self.surface_fade
    }
}

#[derive(Clone, Debug, Default)]
pub struct FogState {
    pub clouds: Vec<FogCloud>,
    fraction: f32,
    seed: u32,
}

impl FogState {
    fn new(entity: Entity) -> Self {
        Self { seed: entity.index().wrapping_mul(2_654_435_761).wrapping_add(17), ..Default::default() }
    }

    fn random(&mut self) -> f32 {
        self.seed = self.seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (self.seed >> 8) as f32 / 16_777_216.0
    }
}

pub fn reset(world: &mut World) {
    for e in world.query::<(FogState,)>() {
        world.remove::<FogState>(e);
    }
}

pub fn total(world: &World) -> usize {
    world.storage::<FogState>().map_or(0, |s| s.iter().map(|(_, state)| state.clouds.len()).sum())
}

fn terrain_response(world: &World, position: Vec3, radius: f32) -> (Vec3, f32) {
    let mut push = Vec3::ZERO;
    let mut fade: f32 = 1.0;
    let Some(terrains) = world.storage::<Terrain>() else { return (push, fade) };
    for (entity, terrain) in terrains.iter() {
        if !terrain.collide || !kit_scene::is_active(world, entity) {
            continue;
        }
        let Some(transform) = world.get::<GlobalTransform>(entity).map(|g| g.0) else { continue };
        let inverse = transform.inverse();
        let local = inverse.transform_point3(position);
        let Some(height) = terrain.height_at(local.xz()) else { continue };
        let clearance = local.y - height;
        fade = fade.min((clearance / (radius * 1.5).max(0.05)).clamp(0.0, 1.0));
        if clearance < radius {
            let normal = transform
                .transform_vector3(terrain.normal(
                    ((local.x + terrain.size * 0.5) / terrain.cell()).round() as u32,
                    ((local.z + terrain.size * 0.5) / terrain.cell()).round() as u32,
                ))
                .normalize_or_zero();
            push += if normal == Vec3::ZERO { Vec3::Y } else { normal } * (radius - clearance).min(radius);
        }
    }
    (push, fade)
}

fn obstacle_response(world: &mut World, position: Vec3, velocity: Vec3, radius: f32) -> (Vec3, f32) {
    let mut push = Vec3::ZERO;
    let mut fade: f32 = 1.0;
    let direction = velocity.normalize_or_zero();
    let look_ahead = (velocity.length() * 0.75 + radius).clamp(radius, radius * 4.0);
    let _ = kit_collider::with_collision_world(world, |collision, world| {
        for entity in collision.overlap_sphere(position, radius, None, u32::MAX) {
            if world.has::<Terrain>(entity) {
                continue;
            }
            if let Some(transform) = world.get::<GlobalTransform>(entity).map(|g| g.0) {
                push += (position - transform.w_axis.truncate()).normalize_or_zero();
                fade = 0.0;
            }
        }
        if direction != Vec3::ZERO {
            if let Some(hit) = collision.sphere_cast(position, direction, radius * 0.7, look_ahead, None, u32::MAX) {
                push += hit.normal;
                fade = fade.min((hit.distance / look_ahead).clamp(0.0, 1.0));
            }
        }
    });
    (push.normalize_or_zero(), fade)
}

pub fn simulate(world: &mut World, dt: f32) {
    if !dt.is_finite() || dt <= 0.0 {
        return;
    }
    let emitters: Vec<Entity> = world.query::<(FogEmitter,)>();
    for e in world.query::<(FogState,)>() {
        if !world.has::<FogEmitter>(e) {
            world.remove::<FogState>(e);
        }
    }
    let mut budget = MAX_TOTAL.saturating_sub(total(world));
    for e in emitters {
        let Some(emitter) = world.get_cloned::<FogEmitter>(e) else { continue };
        let active = emitter.enabled && kit_scene::is_active(world, e);
        if !active {
            if let Some(state) = world.remove::<FogState>(e) {
                budget += state.clouds.len();
            }
            continue;
        }
        let Some(matrix) = world.get::<GlobalTransform>(e).map(|g| g.0) else { continue };
        let fresh = !world.has::<FogState>(e);
        let mut state = world.get_cloned::<FogState>(e).unwrap_or_else(|| FogState::new(e));
        let before = state.clouds.len();
        for cloud in &mut state.clouds {
            let wobble = Vec3::new(
                (cloud.phase + cloud.age * 0.71).sin(),
                (cloud.phase * 1.7 + cloud.age * 0.43).cos(),
                (cloud.phase * 0.37 + cloud.age * 0.89).sin(),
            );
            cloud.velocity += wobble * emitter.turbulence.clamp(0.0, 20.0) * dt;
            let radius = cloud.size.max(0.01) * 0.5;
            let (terrain_push, terrain_fade) = terrain_response(world, cloud.position, radius);
            let (obstacle_push, obstacle_fade) = obstacle_response(world, cloud.position, cloud.velocity, radius);
            let surface_fade = terrain_fade.min(obstacle_fade);
            cloud.surface_fade = cloud.surface_fade + (surface_fade - cloud.surface_fade) * (dt * 4.0).min(1.0);
            cloud.surface_scale = 0.55 + cloud.surface_fade * 0.45;
            let push = terrain_push + obstacle_push;
            if push != Vec3::ZERO {
                let push = push.normalize_or_zero();
                // Ease away before contact; avoid abrupt particle-like motion.
                cloud.velocity += push * (1.25 * dt);
                cloud.position += push * (radius * 0.04 * dt);
            }
            cloud.position += cloud.velocity * dt;
            cloud.age += dt;
        }
        state.clouds.retain(|cloud| cloud.age < cloud.lifetime);
        budget += before - state.clouds.len();

        let rate = emitter.rate.clamp(0.0, MAX_PER_EMITTER as f32 * 4.0);
        let emission = state.fraction + rate * dt;
        let continuous = emission.floor() as usize;
        state.fraction = emission.fract();
        let burst = if fresh { emitter.burst as usize } else { 0 };
        let room = MAX_PER_EMITTER.saturating_sub(state.clouds.len()).min(budget);
        let count = (burst + continuous).min(room);
        let origin = matrix.w_axis.truncate();
        let drift = matrix.transform_vector3(emitter.drift);
        for i in 0..count {
            let jitter = Vec3::new(state.random() - 0.5, state.random() - 0.5, state.random() - 0.5);
            let lifetime = emitter.lifetime.clamp(0.05, 120.0) * (0.75 + state.random() * 0.5);
            let size = emitter.size.max(0.01) * (1.0 + (state.random() - 0.5) * emitter.size_variation.clamp(0.0, 1.0));
            let mut cloud = FogCloud {
                position: origin + jitter * size * 0.25,
                velocity: drift + jitter * emitter.turbulence.clamp(0.0, 20.0) * 0.15,
                age: 0.0,
                lifetime,
                size,
                phase: state.random() * std::f32::consts::TAU,
                surface_fade: 1.0,
                surface_scale: 1.0,
            };
            if i >= burst && rate > 0.0 {
                cloud.age = (((i - burst) as f32 + state.random()) / rate).min(dt);
                cloud.position += cloud.velocity * cloud.age;
            }
            state.clouds.push(cloud);
        }
        budget -= count;
        world.insert(e, state);
    }
}

#[derive(Default)]
struct FogClock {
    ran: bool,
    playing: bool,
}

fn play_step(world: &mut World) {
    let starting = world.try_resource::<FogClock>().is_some_and(|clock| !clock.playing);
    if starting {
        reset(world);
    }
    if let Some(mut clock) = world.try_resource_mut::<FogClock>() {
        clock.ran = true;
        clock.playing = true;
    }
    let dt = world.resource::<Time>().delta;
    simulate(world, dt);
}

fn preview_step(world: &mut World) {
    let Some(mut clock) = world.try_resource_mut::<FogClock>() else { return };
    if std::mem::take(&mut clock.ran) {
        return;
    }
    let stopped = std::mem::take(&mut clock.playing);
    drop(clock);
    if stopped {
        reset(world);
    }
    let dt = world.resource::<Time>().real_delta;
    simulate(world, dt);
}

pub struct FogModule;

impl Module for FogModule {
    fn name(&self) -> &'static str {
        "fog"
    }

    fn dependencies(&self) -> &'static [&'static str] {
        &["scene", "gamesys"]
    }

    fn build(&self, app: &mut App) {
        app.world.insert_resource(FogClock::default());
        app.add_play_system(Stage::PostUpdate, "fog", play_step);
        app.add_system(Stage::PostUpdate, "fog_preview", preview_step);
    }
}
