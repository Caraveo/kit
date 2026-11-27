//! The Particle Emitter component: what an emitter looks like and how it
//! emits. Saved with scenes and Fabs. The simulation (live particles, never
//! saved) and its systems live in the `kit_particles` module.

use glam::{Vec3, Vec4};
use serde::{Deserialize, Serialize};

/// Sprays camera-facing particles from the object's position.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ParticleEmitter {
    pub enabled: bool,
    /// Particles per second, continuously (0 = none).
    pub rate: f32,
    /// Particles at once when the emitter starts (play begins, it's switched
    /// on, or its object becomes active).
    pub burst: u32,
    /// Seconds each particle lives; it fades out over that time.
    pub lifetime: f32,
    /// Launch speed, units per second.
    pub speed: f32,
    /// Launch direction, in the object's own space (it turns with the object).
    pub direction: Vec3,
    /// Cone around the direction, in degrees (0 = a straight line, 180 = all around).
    pub spread_degrees: f32,
    /// World-space acceleration (e.g. (0, -9.81, 0) to fall).
    pub gravity: Vec3,
    /// Width of a particle, in units.
    pub size: f32,
    pub color: Vec4,
    /// Project-relative image; empty = a soft round dot.
    pub texture: String,
}

impl Default for ParticleEmitter {
    fn default() -> Self {
        Self {
            enabled: true,
            rate: 24.0,
            burst: 0,
            lifetime: 1.25,
            speed: 2.0,
            direction: Vec3::Y,
            spread_degrees: 25.0,
            gravity: Vec3::new(0.0, -2.0, 0.0),
            size: 0.15,
            color: Vec4::new(1.0, 0.65, 0.25, 1.0),
            texture: String::new(),
        }
    }
}
