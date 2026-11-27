//! Authored fog-cloud emitters. Live cloud state is owned by `kit_fog`.

use glam::{Vec3, Vec4};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum FogKind {
    #[default]
    Mist,
    Dust,
    Smog,
    Ghost,
    Radiance,
}

impl FogKind {
    pub const ALL: [Self; 5] = [Self::Mist, Self::Dust, Self::Smog, Self::Ghost, Self::Radiance];

    pub fn label(self) -> &'static str {
        match self {
            Self::Mist => "Mist",
            Self::Dust => "Dust",
            Self::Smog => "Smog",
            Self::Ghost => "Ghost",
            Self::Radiance => "Radiance",
        }
    }
}

/// Emits soft, drifting volumetric-looking clouds from an object.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FogEmitter {
    pub enabled: bool,
    pub kind: FogKind,
    pub rate: f32,
    pub burst: u32,
    pub lifetime: f32,
    pub size: f32,
    pub size_variation: f32,
    pub drift: Vec3,
    pub turbulence: f32,
    pub density: f32,
    pub color: Vec4,
    pub texture: String,
}

impl Default for FogEmitter {
    fn default() -> Self {
        Self {
            enabled: true,
            kind: FogKind::Mist,
            rate: 10.0,
            burst: 8,
            lifetime: 8.0,
            size: 2.8,
            size_variation: 0.35,
            drift: Vec3::new(0.15, 0.05, 0.0),
            turbulence: 0.25,
            density: 0.35,
            color: Vec4::new(0.68, 0.75, 0.9, 1.0),
            texture: String::new(),
        }
    }
}
