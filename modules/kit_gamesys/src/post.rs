//! Post-processing volumes: the component and its authored effect settings.
//! How volumes combine and how effects render live in the `kit_post` module.
//!
//! A volume is an (invisible) object whose transform places a box or sphere in
//! the world; or a Global volume that sets scene-wide defaults. Each effect
//! group can Inherit (leave it to lower-priority volumes), turn On (with this
//! volume's values), or turn Off (back to neutral).

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum VolumeShape {
    /// A unit cube scaled, rotated and moved by the object's transform.
    #[default]
    Box,
    /// A sphere of diameter = the object's largest scale.
    Sphere,
    /// Everywhere: scene-wide defaults.
    Global,
}

impl VolumeShape {
    pub const ALL: [VolumeShape; 3] = [VolumeShape::Box, VolumeShape::Sphere, VolumeShape::Global];
    pub fn label(self) -> &'static str {
        match self {
            VolumeShape::Box => "Box",
            VolumeShape::Sphere => "Sphere",
            VolumeShape::Global => "Global",
        }
    }
}

/// What a local volume changes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum VolumeAffect {
    /// The whole picture, while the camera is inside or near the volume.
    #[default]
    Camera,
    /// Only what's inside the volume (per pixel), seen from anywhere.
    Objects,
}

impl VolumeAffect {
    pub const ALL: [VolumeAffect; 2] = [VolumeAffect::Camera, VolumeAffect::Objects];
    pub fn label(self) -> &'static str {
        match self {
            VolumeAffect::Camera => "Camera inside",
            VolumeAffect::Objects => "Objects inside",
        }
    }
}

/// Per-effect override: leave it alone, set it, or switch it off.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Override {
    #[default]
    Inherit,
    On,
    Off,
}

impl Override {
    pub const ALL: [Override; 3] = [Override::Inherit, Override::On, Override::Off];
    pub fn label(self) -> &'static str {
        match self {
            Override::Inherit => "Inherit",
            Override::On => "On",
            Override::Off => "Off",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Tonemap {
    /// Clamp (what Kit draws without post-processing).
    None,
    /// Filmic curve (ACES approximation): soft highlights.
    #[default]
    Filmic,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum AoQuality {
    Low,
    #[default]
    Medium,
    High,
}

impl AoQuality {
    pub const ALL: [AoQuality; 3] = [AoQuality::Low, AoQuality::Medium, AoQuality::High];
    pub fn label(self) -> &'static str {
        match self {
            AoQuality::Low => "Low",
            AoQuality::Medium => "Medium",
            AoQuality::High => "High",
        }
    }
    /// Samples per pixel.
    pub fn samples(self) -> u32 {
        match self {
            AoQuality::Low => 8,
            AoQuality::Medium => 16,
            AoQuality::High => 32,
        }
    }
}

/// Color grading.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ColorFx {
    pub mode: Override,
    /// 0 = grey, 1 = unchanged, 2 = vivid.
    pub saturation: f32,
    /// 1 = unchanged.
    pub contrast: f32,
    /// -1 cool (blue) … 1 warm (orange).
    pub temperature: f32,
}

impl Default for ColorFx {
    fn default() -> Self {
        Self { mode: Override::Inherit, saturation: 1.0, contrast: 1.0, temperature: 0.0 }
    }
}

/// Manual exposure and tone mapping.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ExposureFx {
    pub mode: Override,
    /// Stops: +1 doubles the brightness.
    pub ev: f32,
    pub tonemap: Tonemap,
}

impl Default for ExposureFx {
    fn default() -> Self {
        Self { mode: Override::Inherit, ev: 0.0, tonemap: Tonemap::Filmic }
    }
}

/// HDR bloom.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GlowFx {
    pub mode: Override,
    /// Brightness (linear) where glow starts.
    pub threshold: f32,
    pub intensity: f32,
    /// 0 tight … 1 wide.
    pub radius: f32,
}

impl Default for GlowFx {
    fn default() -> Self {
        Self { mode: Override::Inherit, threshold: 1.0, intensity: 0.8, radius: 0.6 }
    }
}

/// Depth of field.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FocusFx {
    pub mode: Override,
    /// World units from the camera (along its view direction) that are sharp.
    pub distance: f32,
    /// Depth around the focus distance that stays sharp.
    pub range: f32,
    /// 0 = sharp everywhere, 1 = full blur.
    pub strength: f32,
}

impl Default for FocusFx {
    fn default() -> Self {
        Self { mode: Override::Inherit, distance: 10.0, range: 4.0, strength: 1.0 }
    }
}

/// Screen-space ambient occlusion.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AmbientFx {
    pub mode: Override,
    /// World units searched for occluders.
    pub radius: f32,
    pub strength: f32,
    pub quality: AoQuality,
}

impl Default for AmbientFx {
    fn default() -> Self {
        Self { mode: Override::Inherit, radius: 0.6, strength: 1.0, quality: AoQuality::Medium }
    }
}

/// Anti-aliasing and sharpening (MSAA stays as it is).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ImageFx {
    pub mode: Override,
    pub fxaa: bool,
    /// 0 = none, 1 = strong.
    pub sharpen: f32,
}

/// Single directional-light volumetric fog.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FogFx {
    pub mode: Override,
    /// Exponential density in world units.
    pub density: f32,
    pub tint: glam::Vec3,
    /// Directional-light scattering contribution.
    pub scattering: f32,
    /// Maximum ray distance in world units.
    pub max_distance: f32,
}

impl Default for FogFx {
    fn default() -> Self {
        Self { mode: Override::Inherit, density: 0.0, tint: glam::Vec3::ONE, scattering: 0.0, max_distance: 100.0 }
    }
}

impl Default for ImageFx {
    fn default() -> Self {
        Self { mode: Override::Inherit, fxaa: true, sharpen: 0.3 }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PostEffects {
    pub color: ColorFx,
    pub exposure: ExposureFx,
    pub glow: GlowFx,
    pub focus: FocusFx,
    pub ambient: AmbientFx,
    pub image: ImageFx,
    pub fog: FogFx,
}

/// A post-processing volume on an (otherwise empty, invisible) object.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PostVolume {
    pub shape: VolumeShape,
    pub affect: VolumeAffect,
    /// Higher wins where volumes overlap.
    pub priority: i32,
    /// World units outside the shape over which the effect fades in.
    pub blend_distance: f32,
    /// 0 … 1: how strongly the volume applies at full influence.
    pub weight: f32,
    pub effects: PostEffects,
}

impl Default for PostVolume {
    fn default() -> Self {
        Self {
            shape: VolumeShape::Box,
            affect: VolumeAffect::Camera,
            priority: 0,
            blend_distance: 2.0,
            weight: 1.0,
            effects: PostEffects::default(),
        }
    }
}
