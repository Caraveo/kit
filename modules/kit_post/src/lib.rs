//! # kit_post
//!
//! Post-processing volumes (Defold-style module): the shared half of Kit's
//! post-processing. It decides, per rendered camera, which effect settings
//! apply ([`resolve`]), packs them for the GPU ([`PostUniforms`]) and lists
//! the passes to run, in order ([`plan`]). The effects themselves are one
//! WGSL file ([`SHADER`]) used by both GPU backends:
//!
//! * the Vulkan renderer (`kit_render`) compiles it with naga and runs the
//!   passes into its own textures;
//! * the web player runs the same passes with WebGPU (`web/template/kit.js`).
//!
//! Effect order: ambient occlusion → depth of field → bloom (composited
//! before tone mapping) → exposure and tone mapping → color grading → FXAA and
//! sharpening. Colors stay linear HDR until the final pass, and the output
//! target's sRGB encoding converts them once.
//!
//! **Depth.** Focus, Ambient and "Objects inside" volumes read the scene's
//! depth buffer. With MSAA the depth buffer is multisampled, and a color
//! resolve doesn't resolve it, so both backends bind the multisampled depth
//! texture and read sample 0 (`texture_depth_multisampled_2d`); without MSAA
//! the shader is built with `texture_depth_2d` instead ([`shader_source`]).
//! Transparent geometry (blended models, sprites) doesn't write depth, so
//! depth-based effects see the opaque surface behind it.
//!
//! A world with no active volume renders exactly as before: [`frame`]
//! returns `None` and the renderers keep their direct path.

pub mod resolve;

pub use resolve::{Placed, Settings, influence, resolve};

use glam::{Mat4, Vec3};
use kit_ecs::World;
use kit_gamesys::{Override, PostEffects, PostVolume, Tonemap, VolumeAffect, VolumeShape};
use kit_scene::GlobalTransform;

/// The effect shader (WGSL), for MSAA depth.
pub const SHADER: &str = include_str!("post.wgsl");

/// The shader for a depth buffer with `samples` samples per pixel.
pub fn shader_source(samples: u32) -> String {
    if samples > 1 { SHADER.to_owned() } else { SHADER.replace("texture_depth_multisampled_2d", "texture_depth_2d") }
}

/// "Objects inside" volumes evaluated per pixel (more are ignored, lowest
/// priority first).
pub const MAX_OBJECT_VOLUMES: usize = 8;
/// Largest depth-of-field blur radius, in pixels at 1080p (scaled with height).
pub const MAX_BLUR_PX: f32 = 14.0;
/// Bloom levels (each half the size of the one before).
pub const BLOOM_LEVELS: usize = 6;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct ParamsGpu {
    pub color: [f32; 4],
    pub exposure: [f32; 4],
    pub glow: [f32; 4],
    pub focus: [f32; 4],
    pub ambient: [f32; 4],
    pub image: [f32; 4],
}

impl From<&Settings> for ParamsGpu {
    fn from(s: &Settings) -> Self {
        Self {
            color: [s.saturation, s.contrast, s.temperature, 0.0],
            exposure: [s.ev, if s.tonemap == Tonemap::Filmic { 1.0 } else { 0.0 }, 0.0, 0.0],
            glow: [s.glow_threshold, s.glow_intensity, s.glow_radius, 0.0],
            focus: [s.focus_distance, s.focus_range, s.focus_strength, 0.0],
            ambient: [s.ao_radius, s.ao_strength, s.ao_quality.samples() as f32, 0.0],
            image: [s.fxaa, s.sharpen, 0.0, 0.0],
        }
    }
}

impl ParamsGpu {
    /// A volume's own values (used where its override is On).
    fn of(fx: &PostEffects) -> Self {
        Self {
            color: [fx.color.saturation, fx.color.contrast, fx.color.temperature, 0.0],
            exposure: [fx.exposure.ev, if fx.exposure.tonemap == Tonemap::Filmic { 1.0 } else { 0.0 }, 0.0, 0.0],
            glow: [fx.glow.threshold, fx.glow.intensity, fx.glow.radius, 0.0],
            focus: [fx.focus.distance, fx.focus.range, fx.focus.strength, 0.0],
            ambient: [fx.ambient.radius, fx.ambient.strength, fx.ambient.quality.samples() as f32, 0.0],
            image: [if fx.image.fxaa { 1.0 } else { 0.0 }, fx.image.sharpen, 0.0, 0.0],
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct VolumeGpu {
    pub to_local: [[f32; 4]; 4],
    pub shape: [f32; 4],
    pub half: [f32; 4],
    pub modes_a: [f32; 4],
    pub modes_b: [f32; 4],
    pub params: ParamsGpu,
}

/// `Post` in post.wgsl.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct PostUniforms {
    pub view_proj: [[f32; 4]; 4],
    pub inv_view_proj: [[f32; 4]; 4],
    pub camera_pos: [f32; 4],
    pub camera_fwd: [f32; 4],
    pub screen: [f32; 4],
    pub info: [f32; 4],
    pub base: ParamsGpu,
    pub objects: [VolumeGpu; MAX_OBJECT_VOLUMES],
}

/// `Pass` in post.wgsl (one 256-byte slot per pass: dynamic-offset aligned).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct PassUniforms {
    pub dst: [f32; 4],
    pub a: [f32; 4],
    pub _pad: [[f32; 4]; 14],
}

/// The camera a frame is rendered from.
#[derive(Clone, Copy, Debug)]
pub struct CameraInfo {
    /// The clip matrix the scene was drawn with (projection × view).
    pub view_proj: Mat4,
    pub position: Vec3,
    /// Unit view direction.
    pub forward: Vec3,
    pub orthographic: bool,
    /// The clip matrix has NDC y up (WebGPU) instead of down (Vulkan).
    pub ndc_y_up: bool,
}

/// Where a pass reads or writes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Slot {
    /// The scene, rendered in linear HDR (single-sample, resolved).
    Scene,
    /// The final output (the window, editor viewport or preview).
    Out,
    /// An intermediate target (see [`TEMPS`]).
    Temp(usize),
}

/// Intermediate targets: (name, size divisor). All RGBA16F.
pub const TEMPS: [(&str, u32); 4 + 2 * BLOOM_LEVELS - 1] = [
    ("ao", 1),
    ("ao_blur", 1),
    ("color_a", 1),
    ("color_b", 1),
    ("bloom_down_1", 2),
    ("bloom_down_2", 4),
    ("bloom_down_3", 8),
    ("bloom_down_4", 16),
    ("bloom_down_5", 32),
    ("bloom_down_6", 64),
    ("bloom_up_1", 2),
    ("bloom_up_2", 4),
    ("bloom_up_3", 8),
    ("bloom_up_4", 16),
    ("bloom_up_5", 32),
];
const AO: usize = 0;
const AO_BLUR: usize = 1;
const COLOR_A: usize = 2;
const COLOR_B: usize = 3;
const fn down(level: usize) -> usize {
    4 + level - 1
}
const fn up(level: usize) -> usize {
    4 + BLOOM_LEVELS + level - 1
}

/// Size of a temp target for a `width`×`height` output.
pub fn temp_size(index: usize, width: u32, height: u32) -> (u32, u32) {
    let d = TEMPS[index].1;
    ((width / d).max(1), (height / d).max(1))
}

fn slot_size(slot: Slot, width: u32, height: u32) -> (u32, u32) {
    match slot {
        Slot::Temp(i) => temp_size(i, width, height),
        _ => (width, height),
    }
}

/// One fullscreen pass.
#[derive(Clone, Debug, PartialEq)]
pub struct Pass {
    /// Fragment entry point in [`SHADER`] (vertex: `vs_post`).
    pub entry: &'static str,
    /// Bound to `tex_a` and `tex_b` (unused slots get a 1×1 placeholder).
    pub a: Option<Slot>,
    pub b: Option<Slot>,
    pub output: Slot,
    pub uniforms: PassUniforms,
}

/// Everything a backend needs to post-process one view.
#[derive(Clone, Debug)]
pub struct PostFrame {
    pub settings: Settings,
    pub uniforms: PostUniforms,
    pub passes: Vec<Pass>,
}

impl PostFrame {
    /// Does any pass read the depth buffer? (Depth must then be stored.)
    pub fn needs_depth(&self) -> bool {
        self.uniforms.info[0] > 0.0
            || self.passes.iter().any(|p| matches!(p.entry, "fs_ao" | "fs_dof" | "fs_debug_depth"))
    }
}

/// Volumes in the world: (volume, world matrix, scene order).
pub fn gather(world: &World) -> Vec<(PostVolume, Mat4, usize)> {
    let Some(storage) = world.storage::<PostVolume>() else { return Vec::new() };
    if storage.iter().next().is_none() {
        return Vec::new();
    }
    let mut order = Vec::new();
    for root in kit_scene::roots(world) {
        order.extend(kit_scene::descendants_and_self(world, root));
    }
    let mut out: Vec<(PostVolume, Mat4, usize)> = storage
        .iter()
        .map(|(e, v)| {
            let m = world.get::<GlobalTransform>(e).map(|g| g.0).unwrap_or(Mat4::IDENTITY);
            let i = order.iter().position(|o| *o == e).unwrap_or(usize::MAX);
            (v.clone(), m, i)
        })
        .collect();
    out.sort_by_key(|v| v.2);
    out
}

fn mode(m: Override) -> f32 {
    match m {
        Override::Inherit => 0.0,
        Override::On => 1.0,
        Override::Off => 2.0,
    }
}

fn volume_gpu(v: &PostVolume, world: &Mat4) -> VolumeGpu {
    let (to_local, half) = resolve::shape_frame(world);
    let fx = &v.effects;
    VolumeGpu {
        to_local: to_local.to_cols_array_2d(),
        shape: [
            if v.shape == VolumeShape::Sphere { 1.0 } else { 0.0 },
            v.blend_distance.max(0.0),
            v.weight.clamp(0.0, 1.0),
            half.max_element(),
        ],
        half: [half.x, half.y, half.z, 0.0],
        modes_a: [mode(fx.color.mode), mode(fx.exposure.mode), mode(fx.glow.mode), mode(fx.focus.mode)],
        modes_b: [mode(fx.ambient.mode), mode(fx.image.mode), 0.0, 0.0],
        params: ParamsGpu::of(fx),
    }
}

/// Effects an "Objects inside" volume turns on (so their passes must run).
fn objects_turn_on(volumes: &[&(PostVolume, Mat4, usize)]) -> [bool; 6] {
    let mut on = [false; 6];
    for (v, _, _) in volumes {
        let fx = &v.effects;
        on[0] |= fx.color.mode == Override::On;
        on[1] |= fx.exposure.mode == Override::On;
        on[2] |= fx.glow.mode == Override::On && fx.glow.intensity > 0.0;
        on[3] |= fx.focus.mode == Override::On && fx.focus.strength > 0.0;
        on[4] |= fx.ambient.mode == Override::On && fx.ambient.strength > 0.0;
        on[5] |= fx.image.mode == Override::On;
    }
    on
}

/// Post-process a view of `world` from `camera` into a `width`×`height`
/// target, or `None` when no volume changes the picture (draw directly).
pub fn frame(world: &World, camera: &CameraInfo, width: u32, height: u32) -> Option<PostFrame> {
    let volumes = gather(world);
    if volumes.is_empty() {
        return None;
    }
    let (camera_volumes, object_volumes): (Vec<_>, Vec<_>) =
        volumes.iter().partition(|(v, _, _)| v.shape == VolumeShape::Global || v.affect == VolumeAffect::Camera);
    let placed: Vec<Placed> =
        camera_volumes.iter().map(|(v, m, o)| Placed { volume: v, world: *m, order: *o }).collect();
    let settings = resolve(&placed, camera.position);
    let mut objects: Vec<&(PostVolume, Mat4, usize)> = object_volumes;
    {
        let mut tmp: Vec<Placed> = objects.iter().map(|(v, m, o)| Placed { volume: v, world: *m, order: *o }).collect();
        resolve::order(&mut tmp);
        objects = tmp.iter().filter_map(|p| object_volumes_lookup(&volumes, p.order)).collect();
    }
    objects.truncate(MAX_OBJECT_VOLUMES);
    let on = objects_turn_on(&objects);
    if !settings.any_active() && !on.iter().any(|x| *x) {
        return None;
    }
    Some(build(settings, &objects, on, camera, width, height))
}

fn object_volumes_lookup(volumes: &[(PostVolume, Mat4, usize)], order: usize) -> Option<&(PostVolume, Mat4, usize)> {
    volumes.iter().find(|v| v.2 == order)
}

fn build(
    settings: Settings,
    objects: &[&(PostVolume, Mat4, usize)],
    on: [bool; 6],
    camera: &CameraInfo,
    width: u32,
    height: u32,
) -> PostFrame {
    let (w, h) = (width.max(1), height.max(1));
    let mut gpu_objects = [VolumeGpu::default(); MAX_OBJECT_VOLUMES];
    for (slot, (v, m, _)) in gpu_objects.iter_mut().zip(objects) {
        *slot = volume_gpu(v, m);
    }
    let uniforms = PostUniforms {
        view_proj: camera.view_proj.to_cols_array_2d(),
        inv_view_proj: camera.view_proj.inverse().to_cols_array_2d(),
        camera_pos: camera.position.extend(if camera.orthographic { 1.0 } else { 0.0 }).into(),
        camera_fwd: camera.forward.normalize_or(Vec3::NEG_Z).extend(0.0).into(),
        screen: [w as f32, h as f32, 1.0 / w as f32, 1.0 / h as f32],
        info: [
            objects.len() as f32,
            settings.ao_quality.samples() as f32,
            if camera.ndc_y_up { 1.0 } else { 0.0 },
            MAX_BLUR_PX * h as f32 / 1080.0,
        ],
        base: ParamsGpu::from(&settings),
        objects: gpu_objects,
    };
    PostFrame { settings, uniforms, passes: plan(&settings, on, w, h) }
}

fn pass(entry: &'static str, a: Option<Slot>, b: Option<Slot>, output: Slot, w: u32, h: u32, extra: [f32; 4]) -> Pass {
    let (dw, dh) = slot_size(output, w, h);
    let mut uniforms =
        PassUniforms { dst: [dw as f32, dh as f32, 1.0 / dw as f32, 1.0 / dh as f32], a: extra, ..Default::default() };
    if let Some(src) = a {
        let (sw, sh) = slot_size(src, w, h);
        uniforms.a[0] = 1.0 / sw as f32;
        uniforms.a[1] = 1.0 / sh as f32;
    }
    Pass { entry, a, b, output, uniforms }
}

/// The ordered passes for these settings (effects that contribute nothing
/// are skipped). `on`: effects some "Objects inside" volume switches on.
pub fn plan(s: &Settings, on: [bool; 6], w: u32, h: u32) -> Vec<Pass> {
    let mut passes = Vec::new();
    let mut current = Slot::Scene;
    let mut free = [COLOR_A, COLOR_B];
    let mut next_color = |current: Slot| {
        let pick = if current == Slot::Temp(free[0]) { free[1] } else { free[0] };
        free.swap(0, 1);
        Slot::Temp(pick)
    };
    if s.ambient_active() || on[4] {
        passes.push(pass("fs_ao", None, None, Slot::Temp(AO), w, h, [0.0; 4]));
        passes.push(pass("fs_ao_blur", Some(Slot::Temp(AO)), None, Slot::Temp(AO_BLUR), w, h, [0.0; 4]));
        let out = next_color(current);
        passes.push(pass("fs_ao_apply", Some(current), Some(Slot::Temp(AO_BLUR)), out, w, h, [0.0; 4]));
        current = out;
    }
    if s.focus_active() || on[3] {
        let out = next_color(current);
        passes.push(pass("fs_dof", Some(current), None, out, w, h, [0.0; 4]));
        current = out;
    }
    let mut bloom = None;
    if s.glow_active() || on[2] {
        passes.push(pass("fs_bloom_prefilter", Some(current), None, Slot::Temp(down(1)), w, h, [0.0; 4]));
        for level in 2..=BLOOM_LEVELS {
            passes.push(pass(
                "fs_bloom_down",
                Some(Slot::Temp(down(level - 1))),
                None,
                Slot::Temp(down(level)),
                w,
                h,
                [0.0; 4],
            ));
        }
        let mut lower = Slot::Temp(down(BLOOM_LEVELS));
        for level in (1..BLOOM_LEVELS).rev() {
            let out = Slot::Temp(up(level));
            passes.push(pass("fs_bloom_up", Some(lower), Some(Slot::Temp(down(level))), out, w, h, [0.0; 4]));
            lower = out;
        }
        bloom = Some(lower);
    }
    let image = s.image_active() || on[5];
    let final_out = if image { next_color(current) } else { Slot::Out };
    // Bloom levels add up; scale so intensity 1 ≈ a moderate glow.
    let bloom_scale = 1.0;
    passes.push(pass(
        "fs_final",
        Some(current),
        bloom,
        final_out,
        w,
        h,
        [0.0, 0.0, if bloom.is_some() { 1.0 } else { 0.0 }, bloom_scale],
    ));
    if image {
        passes.push(pass("fs_image", Some(final_out), None, Slot::Out, w, h, [0.0; 4]));
    }
    passes
}

/// A pass list that writes the scene's view depth (÷ `scale`) to the output:
/// used by tests to prove each backend can read MSAA depth.
pub fn debug_depth_frame(camera: &CameraInfo, width: u32, height: u32, scale: f32) -> PostFrame {
    let mut f = build(Settings::NEUTRAL, &[], [false; 6], camera, width, height);
    f.passes = vec![pass("fs_debug_depth", None, None, Slot::Out, width.max(1), height.max(1), [0.0, 0.0, 0.0, scale])];
    f
}

#[cfg(test)]
mod tests {
    use super::*;
    use kit_gamesys::{ExposureFx, GlowFx, ImageFx};

    fn validate(src: &str) {
        let module = naga::front::wgsl::parse_str(src).unwrap_or_else(|e| panic!("{}", e.emit_to_string(src)));
        naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::all())
            .validate(&module)
            .unwrap_or_else(|e| panic!("{}", e.emit_to_string(src)));
    }

    #[test]
    fn shader_compiles_for_msaa_and_single_sample_depth() {
        validate(&shader_source(4));
        validate(&shader_source(1));
        assert!(shader_source(1).contains("texture_depth_2d"));
        assert_eq!(std::mem::size_of::<PassUniforms>(), 256);
        assert_eq!(std::mem::size_of::<PostUniforms>() % 16, 0);
    }

    fn world_with(volumes: &[PostVolume]) -> World {
        let mut w = World::new();
        for v in volumes {
            let e = kit_scene::spawn_node(&mut w, "Volume", kit_scene::Transform::from_xyz(0.0, 0.0, 0.0));
            w.insert(e, v.clone());
        }
        kit_scene::propagate_transforms(&mut w);
        w
    }

    fn camera(at: Vec3) -> CameraInfo {
        CameraInfo {
            view_proj: Mat4::IDENTITY,
            position: at,
            forward: Vec3::NEG_Z,
            orthographic: false,
            ndc_y_up: false,
        }
    }

    #[test]
    fn no_active_volume_keeps_the_direct_path() {
        assert!(frame(&World::new(), &camera(Vec3::ZERO), 640, 360).is_none());
        // A volume whose groups all Inherit changes nothing.
        let w = world_with(&[PostVolume { shape: VolumeShape::Global, ..Default::default() }]);
        assert!(frame(&w, &camera(Vec3::ZERO), 640, 360).is_none());
    }

    #[test]
    fn passes_run_in_order_and_skip_unused_effects() {
        let fx = PostEffects {
            exposure: ExposureFx { mode: Override::On, ev: 1.0, tonemap: Tonemap::Filmic },
            glow: GlowFx { mode: Override::On, intensity: 1.0, ..Default::default() },
            image: ImageFx { mode: Override::On, fxaa: true, sharpen: 0.2 },
            ..Default::default()
        };
        let w = world_with(&[PostVolume { shape: VolumeShape::Global, effects: fx, ..Default::default() }]);
        let f = frame(&w, &camera(Vec3::ZERO), 1280, 720).unwrap();
        let entries: Vec<&str> = f.passes.iter().map(|p| p.entry).collect();
        assert!(!entries.contains(&"fs_ao") && !entries.contains(&"fs_dof"), "unused effects skipped: {entries:?}");
        let pos = |e: &str| entries.iter().position(|x| *x == e).unwrap();
        assert!(pos("fs_bloom_prefilter") < pos("fs_final") && pos("fs_final") < pos("fs_image"));
        assert_eq!(f.passes.last().unwrap().output, Slot::Out);
        assert!(!f.needs_depth(), "no depth effects: depth needn't be kept");
        // Every input was written by an earlier pass (or is the scene).
        let mut written = vec![Slot::Scene];
        for p in &f.passes {
            for input in [p.a, p.b].into_iter().flatten() {
                assert!(written.contains(&input), "{} reads {input:?} before it's written", p.entry);
            }
            assert!(p.a != Some(p.output) && p.b != Some(p.output), "{} reads its own output", p.entry);
            written.push(p.output);
        }
    }

    #[test]
    fn object_volumes_go_to_the_gpu_and_enable_their_passes() {
        let fx = PostEffects {
            glow: GlowFx { mode: Override::On, intensity: 2.0, ..Default::default() },
            ..Default::default()
        };
        let v = PostVolume { affect: VolumeAffect::Objects, effects: fx, ..Default::default() };
        let w = world_with(&[v]);
        // Camera far away: the camera's own settings are neutral, but the glow
        // still applies to what's inside the box.
        let f = frame(&w, &camera(Vec3::splat(100.0)), 640, 360).unwrap();
        assert_eq!(f.settings, Settings::NEUTRAL);
        assert_eq!(f.uniforms.info[0], 1.0);
        assert_eq!(f.uniforms.objects[0].modes_a[2], 1.0);
        assert!(f.passes.iter().any(|p| p.entry == "fs_bloom_prefilter"));
        assert!(f.needs_depth());
    }
}
