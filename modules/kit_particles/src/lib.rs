//! # kit_particles
//!
//! Runs [`ParticleEmitter`]s: CPU-simulated particles drawn as camera-facing
//! quads through the renderer's shared sprite list (so they work with
//! perspective and orthographic cameras, on desktop and the web).
//!
//! * **Play:** particles advance with game time, so they stop while the game
//!   is paused (`Time.Scale = 0`). Every Play session starts from nothing,
//!   with each emitter's burst.
//! * **Editor:** emitters preview live (on real time) while you edit.
//! * Live particles ([`ParticleState`]) are never saved: scenes and Fabs only
//!   hold the emitter's settings.
//! * Counts are bounded: [`MAX_PER_EMITTER`] per emitter and [`MAX_TOTAL`]
//!   in the whole world.

use glam::Vec3;
use kit_core::{App, Module, Stage, Time};
use kit_ecs::{Entity, World};
pub use kit_gamesys::ParticleEmitter;
use kit_scene::GlobalTransform;

/// Most live particles one emitter can have.
pub const MAX_PER_EMITTER: usize = 1024;
/// Most live particles in the whole world.
pub const MAX_TOTAL: usize = 16_384;

/// One live particle (world space).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Particle {
    pub position: Vec3,
    pub velocity: Vec3,
    /// Seconds since it was emitted.
    pub age: f32,
    pub lifetime: f32,
}

impl Particle {
    /// 1 when new, fading to 0 at the end of its life.
    pub fn opacity(&self) -> f32 {
        (1.0 - self.age / self.lifetime.max(1e-4)).clamp(0.0, 1.0)
    }

    fn advance(&mut self, gravity: Vec3, dt: f32) {
        self.position += self.velocity * dt + gravity * (0.5 * dt * dt);
        self.velocity += gravity * dt;
        self.age += dt;
    }
}

/// An emitter's live particles (a component; never saved).
#[derive(Clone, Debug, Default)]
pub struct ParticleState {
    pub particles: Vec<Particle>,
    /// Emission left over from last frame (fractions of a particle).
    fraction: f32,
    seed: u32,
}

impl ParticleState {
    fn new(entity: Entity) -> Self {
        Self { particles: Vec::new(), fraction: 0.0, seed: entity.index().wrapping_mul(2_654_435_761).wrapping_add(1) }
    }

    fn random(&mut self) -> f32 {
        self.seed = self.seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (self.seed >> 8) as f32 / 16_777_216.0
    }
}

/// Forget every live particle (the next frame starts fresh, with bursts).
pub fn reset(world: &mut World) {
    for e in world.query::<(ParticleState,)>() {
        world.remove::<ParticleState>(e);
    }
}

/// Live particles in the whole world.
pub fn total(world: &World) -> usize {
    world.storage::<ParticleState>().map_or(0, |s| s.iter().map(|(_, st)| st.particles.len()).sum())
}

/// Advance every emitter by `dt` seconds: age and move particles, drop dead
/// ones, emit new ones (a burst when an emitter starts).
pub fn simulate(world: &mut World, dt: f32) {
    if !dt.is_finite() || dt <= 0.0 {
        return;
    }
    let emitters: Vec<Entity> = world.query::<(ParticleEmitter,)>();
    // States of removed emitters go too.
    for e in world.query::<(ParticleState,)>() {
        if !world.has::<ParticleEmitter>(e) {
            world.remove::<ParticleState>(e);
        }
    }
    let mut budget = MAX_TOTAL.saturating_sub(total(world));
    for e in emitters {
        let Some(emitter) = world.get_cloned::<ParticleEmitter>(e) else { continue };
        let on =
            emitter.enabled && kit_scene::is_active(world, e) && !kit_gamesys::feature_disabled(world, e, "Particles");
        if !on {
            // Switched off: particles vanish; switching back on bursts again.
            if let Some(st) = world.remove::<ParticleState>(e) {
                budget += st.particles.len();
            }
            continue;
        }
        let Some(m) = world.get::<GlobalTransform>(e).map(|g| g.0) else { continue };
        let fresh = !world.has::<ParticleState>(e);
        let mut st = world.get_cloned::<ParticleState>(e).unwrap_or_else(|| ParticleState::new(e));
        let before = st.particles.len();

        let lifetime = emitter.lifetime.clamp(0.01, 60.0);
        for p in &mut st.particles {
            p.advance(emitter.gravity, dt);
        }
        st.particles.retain(|p| p.age < p.lifetime);
        budget += before - st.particles.len();

        let rate = emitter.rate.clamp(0.0, MAX_PER_EMITTER as f32 * 10.0);
        let emission = st.fraction + rate * dt;
        let continuous = emission.floor() as usize;
        st.fraction = emission.fract();
        let burst = if fresh { emitter.burst as usize } else { 0 };
        let room = MAX_PER_EMITTER.saturating_sub(st.particles.len()).min(budget);
        let count = (burst + continuous).min(room);

        let origin = m.w_axis.truncate();
        let axis = m.transform_vector3(emitter.direction).try_normalize().unwrap_or(Vec3::Y);
        let (tangent, bitangent) = axis.any_orthonormal_pair();
        let cos_spread = emitter.spread_degrees.clamp(0.0, 180.0).to_radians().cos();
        let speed = emitter.speed.clamp(0.0, 1000.0);
        for i in 0..count {
            // A random direction inside the cone (uniform over its cap).
            let c = 1.0 - st.random() * (1.0 - cos_spread);
            let s = (1.0 - c * c).max(0.0).sqrt();
            let (sin_a, cos_a) = (st.random() * std::f32::consts::TAU).sin_cos();
            let dir = axis * c + (tangent * cos_a + bitangent * sin_a) * s;
            let mut p = Particle { position: origin, velocity: dir * speed, age: 0.0, lifetime };
            // Continuous particles are spread over the frame instead of in clumps.
            if i >= burst && rate > 0.0 {
                let back = ((i - burst) as f32 + st.random()) / rate;
                p.advance(emitter.gravity, back.min(dt));
            }
            st.particles.push(p);
        }
        budget -= count;
        world.insert(e, st);
    }
}

/// What ran last frame, so switching between editing and playing starts fresh.
#[derive(Default)]
struct ParticleClock {
    /// The play system ran this frame.
    ran: bool,
    /// The last frame was a Play frame.
    playing: bool,
}

fn play_step(world: &mut World) {
    let starting = world.try_resource::<ParticleClock>().is_some_and(|c| !c.playing);
    if starting {
        // A new Play session: nothing left over from the editor preview.
        reset(world);
    }
    if let Some(mut c) = world.try_resource_mut::<ParticleClock>() {
        c.ran = true;
        c.playing = true;
    }
    // Game time: zero while paused.
    let dt = world.resource::<Time>().delta;
    simulate(world, dt);
}

fn preview_step(world: &mut World) {
    let Some(mut c) = world.try_resource_mut::<ParticleClock>() else { return };
    if std::mem::take(&mut c.ran) {
        return;
    }
    let stopped = std::mem::take(&mut c.playing);
    drop(c);
    if stopped {
        reset(world);
    }
    // Editing: preview on real time.
    let dt = world.resource::<Time>().real_delta;
    simulate(world, dt);
}

/// Registers the particle systems (after scripts, so they see this frame's moves).
pub struct ParticlesModule;

impl Module for ParticlesModule {
    fn name(&self) -> &'static str {
        "particles"
    }
    fn dependencies(&self) -> &'static [&'static str] {
        &["scene", "gamesys"]
    }
    fn build(&self, app: &mut App) {
        app.world.insert_resource(ParticleClock::default());
        app.add_play_system(Stage::PostUpdate, "particles", play_step);
        app.add_system(Stage::PostUpdate, "particles_preview", preview_step);
    }
}

#[cfg(test)]
mod tests;
