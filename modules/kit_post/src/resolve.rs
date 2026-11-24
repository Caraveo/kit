//! How volumes combine into the settings one camera sees.
//!
//! Rules (the GPU's per-pixel "Objects inside" volumes follow the same ones):
//!
//! 1. Start from [`Settings::NEUTRAL`] (no visible change).
//! 2. Apply Global volumes, then local ones, each in [`order`]: lower priority
//!    first, so higher priority is applied last and wins in the interior.
//!    Equal priority: the bigger volume first (the more specific, smaller one
//!    wins), then scene order (later wins). Globals count as infinitely big.
//! 3. A volume's influence ([`influence`]) is 1 inside its shape, fading to 0
//!    over Blend Distance (world units) outside it, times its Weight.
//! 4. Per effect group: Inherit leaves the settings alone; On blends toward
//!    the volume's values; Off blends toward neutral. Numbers blend linearly by
//!    the influence; discrete choices (tone mapping, AO quality, FXAA) switch
//!    to the volume's value once the influence reaches one half.

use glam::{Mat4, Quat, Vec3};
use kit_gamesys::{AoQuality, Override, PostEffects, PostVolume, Tonemap, VolumeShape};

/// Fully resolved effect settings (what the renderer applies).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    pub saturation: f32,
    pub contrast: f32,
    pub temperature: f32,
    pub ev: f32,
    pub tonemap: Tonemap,
    pub glow_threshold: f32,
    pub glow_intensity: f32,
    pub glow_radius: f32,
    pub focus_distance: f32,
    pub focus_range: f32,
    pub focus_strength: f32,
    pub ao_radius: f32,
    pub ao_strength: f32,
    pub ao_quality: AoQuality,
    /// 0 … 1 (blends like a number; the editor shows it as a checkbox).
    pub fxaa: f32,
    pub sharpen: f32,
}

const EPS: f32 = 1e-3;

impl Settings {
    /// No change to the picture (what Kit draws without post-processing).
    pub const NEUTRAL: Settings = Settings {
        saturation: 1.0,
        contrast: 1.0,
        temperature: 0.0,
        ev: 0.0,
        tonemap: Tonemap::None,
        glow_threshold: 1.0,
        glow_intensity: 0.0,
        glow_radius: 0.6,
        focus_distance: 10.0,
        focus_range: 4.0,
        focus_strength: 0.0,
        ao_radius: 0.6,
        ao_strength: 0.0,
        ao_quality: AoQuality::Medium,
        fxaa: 0.0,
        sharpen: 0.0,
    };

    pub fn color_active(&self) -> bool {
        (self.saturation - 1.0).abs() > EPS || (self.contrast - 1.0).abs() > EPS || self.temperature.abs() > EPS
    }
    pub fn exposure_active(&self) -> bool {
        self.ev.abs() > EPS || self.tonemap != Tonemap::None
    }
    pub fn glow_active(&self) -> bool {
        self.glow_intensity > EPS
    }
    pub fn focus_active(&self) -> bool {
        self.focus_strength > EPS
    }
    pub fn ambient_active(&self) -> bool {
        self.ao_strength > EPS
    }
    pub fn image_active(&self) -> bool {
        self.fxaa > EPS || self.sharpen > EPS
    }
    /// Does anything differ from the direct render?
    pub fn any_active(&self) -> bool {
        self.color_active()
            || self.exposure_active()
            || self.glow_active()
            || self.focus_active()
            || self.ambient_active()
            || self.image_active()
    }

    /// Apply one volume's effect groups with influence `t` (0 … 1).
    pub fn apply(&mut self, fx: &PostEffects, t: f32) {
        let t = t.clamp(0.0, 1.0);
        if t <= 0.0 {
            return;
        }
        let lerp = |a: &mut f32, b: f32| *a += (b - *a) * t;
        let switch = t >= 0.5;
        let n = Settings::NEUTRAL;
        match fx.color.mode {
            Override::Inherit => {}
            Override::On => {
                lerp(&mut self.saturation, fx.color.saturation);
                lerp(&mut self.contrast, fx.color.contrast);
                lerp(&mut self.temperature, fx.color.temperature);
            }
            Override::Off => {
                lerp(&mut self.saturation, n.saturation);
                lerp(&mut self.contrast, n.contrast);
                lerp(&mut self.temperature, n.temperature);
            }
        }
        match fx.exposure.mode {
            Override::Inherit => {}
            Override::On => {
                lerp(&mut self.ev, fx.exposure.ev);
                if switch {
                    self.tonemap = fx.exposure.tonemap;
                }
            }
            Override::Off => {
                lerp(&mut self.ev, n.ev);
                if switch {
                    self.tonemap = n.tonemap;
                }
            }
        }
        match fx.glow.mode {
            Override::Inherit => {}
            Override::On => {
                lerp(&mut self.glow_threshold, fx.glow.threshold);
                lerp(&mut self.glow_intensity, fx.glow.intensity);
                lerp(&mut self.glow_radius, fx.glow.radius);
            }
            Override::Off => lerp(&mut self.glow_intensity, 0.0),
        }
        match fx.focus.mode {
            Override::Inherit => {}
            Override::On => {
                lerp(&mut self.focus_distance, fx.focus.distance);
                lerp(&mut self.focus_range, fx.focus.range);
                lerp(&mut self.focus_strength, fx.focus.strength);
            }
            Override::Off => lerp(&mut self.focus_strength, 0.0),
        }
        match fx.ambient.mode {
            Override::Inherit => {}
            Override::On => {
                lerp(&mut self.ao_radius, fx.ambient.radius);
                lerp(&mut self.ao_strength, fx.ambient.strength);
                if switch {
                    self.ao_quality = fx.ambient.quality;
                }
            }
            Override::Off => lerp(&mut self.ao_strength, 0.0),
        }
        match fx.image.mode {
            Override::Inherit => {}
            Override::On => {
                lerp(&mut self.fxaa, if fx.image.fxaa { 1.0 } else { 0.0 });
                lerp(&mut self.sharpen, fx.image.sharpen);
            }
            Override::Off => {
                lerp(&mut self.fxaa, 0.0);
                lerp(&mut self.sharpen, 0.0);
            }
        }
    }
}

/// A volume placed in the world (its object's world matrix, with parenting).
#[derive(Clone, Copy, Debug)]
pub struct Placed<'a> {
    pub volume: &'a PostVolume,
    pub world: Mat4,
    /// Position in scene (hierarchy) order: the last tie-break.
    pub order: usize,
}

/// The box/sphere frame: world-to-volume transform without scale, and the
/// half extents (box) or radius (sphere) in world units.
pub fn shape_frame(world: &Mat4) -> (Mat4, Vec3) {
    let (scale, rot, pos) = world.to_scale_rotation_translation();
    let to_local = Mat4::from_rotation_translation(rot, pos).inverse();
    let _ = Quat::IDENTITY;
    (to_local, scale.abs() * 0.5)
}

/// World units from `p` to the volume (0 inside). Globals contain everything.
pub fn distance(shape: VolumeShape, world: &Mat4, p: Vec3) -> f32 {
    let (to_local, half) = shape_frame(world);
    let q = to_local.transform_point3(p);
    match shape {
        VolumeShape::Global => 0.0,
        VolumeShape::Box => (q.abs() - half).max(Vec3::ZERO).length(),
        VolumeShape::Sphere => (q.length() - half.max_element()).max(0.0),
    }
}

/// Smooth 0 … 1 falloff (C1-continuous at both ends).
fn smooth(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

/// How strongly a volume applies at world position `p` (0 … 1).
pub fn influence(volume: &PostVolume, world: &Mat4, p: Vec3) -> f32 {
    let weight = volume.weight.clamp(0.0, 1.0);
    let d = distance(volume.shape, world, p);
    if d <= 0.0 {
        return weight;
    }
    if volume.blend_distance <= 0.0 {
        return 0.0;
    }
    weight * smooth(1.0 - d / volume.blend_distance)
}

/// Size used to order equal-priority volumes (bigger first).
fn size(p: &Placed) -> f32 {
    let (_, half) = shape_frame(&p.world);
    match p.volume.shape {
        VolumeShape::Global => f32::INFINITY,
        VolumeShape::Box => 8.0 * half.x * half.y * half.z,
        VolumeShape::Sphere => 4.18879 * half.max_element().powi(3),
    }
}

/// Application order: globals first, then by priority ascending, bigger
/// volumes before smaller ones, then scene order. The last applied wins.
pub fn order(volumes: &mut [Placed]) {
    volumes.sort_by(|a, b| {
        let global = |p: &Placed| p.volume.shape != VolumeShape::Global;
        global(a)
            .cmp(&global(b))
            .then(a.volume.priority.cmp(&b.volume.priority))
            .then(size(b).total_cmp(&size(a)))
            .then(a.order.cmp(&b.order))
    });
}

/// The settings a camera at `camera` sees from "Camera inside" volumes.
pub fn resolve(volumes: &[Placed], camera: Vec3) -> Settings {
    let mut sorted: Vec<Placed> = volumes.to_vec();
    order(&mut sorted);
    let mut s = Settings::NEUTRAL;
    for v in &sorted {
        s.apply(&v.volume.effects, influence(v.volume, &v.world, camera));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use kit_gamesys::{ColorFx, ExposureFx, GlowFx, ImageFx};

    fn vol(shape: VolumeShape, priority: i32, fx: PostEffects) -> PostVolume {
        PostVolume { shape, priority, blend_distance: 2.0, effects: fx, ..Default::default() }
    }

    fn glow(intensity: f32) -> PostEffects {
        PostEffects { glow: GlowFx { mode: Override::On, intensity, ..Default::default() }, ..Default::default() }
    }

    fn exposure(ev: f32) -> PostEffects {
        PostEffects { exposure: ExposureFx { mode: Override::On, ev, tonemap: Tonemap::Filmic }, ..Default::default() }
    }

    fn at(v: &PostVolume, world: Mat4, order: usize) -> Placed<'_> {
        Placed { volume: v, world, order }
    }

    #[test]
    fn no_volumes_is_neutral() {
        assert_eq!(resolve(&[], Vec3::ZERO), Settings::NEUTRAL);
        assert!(!Settings::NEUTRAL.any_active());
    }

    #[test]
    fn box_influence_is_full_inside_and_fades_over_blend_distance() {
        let v = vol(VolumeShape::Box, 0, glow(1.0));
        let m = Mat4::from_scale(Vec3::splat(4.0)); // half extent 2
        assert_eq!(influence(&v, &m, Vec3::new(1.9, 0.0, 0.0)), 1.0);
        let mid = influence(&v, &m, Vec3::new(3.0, 0.0, 0.0)); // 1 unit outside of 2
        assert!((mid - 0.5).abs() < 1e-5, "{mid}");
        assert_eq!(influence(&v, &m, Vec3::new(4.0, 0.0, 0.0)), 0.0);
        // Monotonic fade.
        let samples: Vec<f32> =
            (0..=20).map(|i| influence(&v, &m, Vec3::new(2.0 + i as f32 * 0.1, 0.0, 0.0))).collect();
        assert!(samples.windows(2).all(|w| w[1] <= w[0]));
    }

    #[test]
    fn rotated_boxes_spheres_and_parenting_use_world_transforms() {
        let v = vol(VolumeShape::Box, 0, glow(1.0));
        // A 10×2×2 box rotated 90° about Y: long along Z.
        let m = Mat4::from_rotation_y(std::f32::consts::FRAC_PI_2) * Mat4::from_scale(Vec3::new(10.0, 2.0, 2.0));
        assert_eq!(distance(VolumeShape::Box, &m, Vec3::new(0.0, 0.0, 4.5)), 0.0, "inside along the rotated axis");
        assert!((distance(VolumeShape::Box, &m, Vec3::new(4.5, 0.0, 0.0)) - 3.5).abs() < 1e-4);
        // Parent moved by 100 on X: the child's world matrix moves the volume.
        let parent = Mat4::from_translation(Vec3::new(100.0, 0.0, 0.0));
        let world = parent * Mat4::from_scale(Vec3::splat(2.0));
        assert_eq!(influence(&v, &world, Vec3::new(100.5, 0.0, 0.0)), 1.0);
        assert_eq!(influence(&v, &world, Vec3::ZERO), 0.0);
        // Spheres: diameter = largest scale.
        let s = Mat4::from_scale(Vec3::new(6.0, 2.0, 2.0));
        assert_eq!(distance(VolumeShape::Sphere, &s, Vec3::new(0.0, 2.9, 0.0)), 0.0);
        assert!((distance(VolumeShape::Sphere, &s, Vec3::new(0.0, 5.0, 0.0)) - 2.0).abs() < 1e-5);
        assert_eq!(distance(VolumeShape::Global, &s, Vec3::splat(1e6)), 0.0);
    }

    #[test]
    fn globals_set_the_baseline_and_higher_priority_wins_inside() {
        let global = vol(VolumeShape::Global, 10, exposure(1.0)); // high priority, but global
        let low = vol(VolumeShape::Box, 0, exposure(-2.0));
        let high = vol(VolumeShape::Box, 5, exposure(3.0));
        let big = Mat4::from_scale(Vec3::splat(20.0));
        let small = Mat4::from_scale(Vec3::splat(4.0));
        let vols = [at(&global, Mat4::IDENTITY, 0), at(&high, small, 2), at(&low, big, 1)];
        // Outside both boxes: the global baseline.
        assert_eq!(resolve(&vols, Vec3::splat(50.0)).ev, 1.0);
        // Inside only the big low-priority box.
        assert_eq!(resolve(&vols, Vec3::new(8.0, 0.0, 0.0)).ev, -2.0);
        // Inside both: the high-priority one wins, whatever the list order.
        assert_eq!(resolve(&vols, Vec3::ZERO).ev, 3.0);
        // In the high one's blend region: a smooth mix between the two.
        let edge = resolve(&vols, Vec3::new(3.0, 0.0, 0.0)).ev;
        assert!(edge > -2.0 && edge < 3.0, "{edge}");
        // Walking out of the overlap and back to the baseline is continuous.
        let path: Vec<f32> = (0..=300).map(|i| resolve(&vols, Vec3::new(i as f32 * 0.1, 0.0, 0.0)).ev).collect();
        assert!(path.windows(2).all(|w| (w[1] - w[0]).abs() < 0.6), "no jumps");
        assert_eq!(*path.last().unwrap(), 1.0);
    }

    #[test]
    fn equal_priority_ties_are_stable() {
        let a = vol(VolumeShape::Box, 0, exposure(1.0));
        let b = vol(VolumeShape::Box, 0, exposure(2.0));
        let big = Mat4::from_scale(Vec3::splat(10.0));
        let small = Mat4::from_scale(Vec3::splat(2.0));
        // Same priority: the smaller volume wins, regardless of order given.
        assert_eq!(resolve(&[at(&b, small, 0), at(&a, big, 1)], Vec3::ZERO).ev, 2.0);
        assert_eq!(resolve(&[at(&a, big, 1), at(&b, small, 0)], Vec3::ZERO).ev, 2.0);
        // Same priority and size: the later one in the scene wins.
        assert_eq!(resolve(&[at(&a, big, 3), at(&b, big, 4)], Vec3::ZERO).ev, 2.0);
        assert_eq!(resolve(&[at(&b, big, 4), at(&a, big, 3)], Vec3::ZERO).ev, 2.0);
    }

    #[test]
    fn inherit_keeps_other_groups_and_off_differs_from_inherit() {
        let global = vol(
            VolumeShape::Global,
            0,
            PostEffects {
                exposure: ExposureFx { mode: Override::On, ev: 0.5, tonemap: Tonemap::Filmic },
                color: ColorFx { mode: Override::On, saturation: 0.5, ..Default::default() },
                glow: GlowFx { mode: Override::On, intensity: 0.3, ..Default::default() },
                ..Default::default()
            },
        );
        // Only changes Glow; Color and Exposure inherit.
        let glow_only = vol(VolumeShape::Global, 1, glow(2.0));
        let s = resolve(&[at(&global, Mat4::IDENTITY, 0), at(&glow_only, Mat4::IDENTITY, 1)], Vec3::ZERO);
        assert_eq!((s.glow_intensity, s.saturation, s.ev), (2.0, 0.5, 0.5));
        // Off switches Glow off (unlike Inherit, which would keep 0.3).
        let off = vol(
            VolumeShape::Global,
            1,
            PostEffects { glow: GlowFx { mode: Override::Off, ..Default::default() }, ..Default::default() },
        );
        let s = resolve(&[at(&global, Mat4::IDENTITY, 0), at(&off, Mat4::IDENTITY, 1)], Vec3::ZERO);
        assert_eq!(s.glow_intensity, 0.0);
        assert_eq!(s.saturation, 0.5, "other groups still inherit");
    }

    #[test]
    fn numbers_blend_and_discrete_choices_switch_at_half() {
        let mut s = Settings::NEUTRAL;
        let fx = PostEffects {
            exposure: ExposureFx { mode: Override::On, ev: 2.0, tonemap: Tonemap::Filmic },
            image: ImageFx { mode: Override::On, fxaa: true, sharpen: 1.0 },
            ..Default::default()
        };
        s.apply(&fx, 0.25);
        assert_eq!((s.ev, s.tonemap), (0.5, Tonemap::None), "below half: discrete value not taken yet");
        assert_eq!((s.fxaa, s.sharpen), (0.25, 0.25));
        let mut s = Settings::NEUTRAL;
        s.apply(&fx, 0.5);
        assert_eq!((s.ev, s.tonemap), (1.0, Tonemap::Filmic), "at half: switched");
        // Weight scales influence.
        let mut v = vol(VolumeShape::Global, 0, fx);
        v.weight = 0.5;
        assert_eq!(resolve(&[at(&v, Mat4::IDENTITY, 0)], Vec3::ZERO).ev, 1.0);
    }
}
