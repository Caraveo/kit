//! # kit_web
//!
//! Kit's web player. The game itself (ECS, scenes, x scripts, physics, paths,
//! terrain) runs here as WebAssembly, exactly as on desktop. Each frame it
//! writes a small binary *command buffer* for the page's WebGPU renderer
//! (`web/template/kit.js`): new meshes and textures to upload, then the draw
//! list from [`kit_render::draw`], the same list the desktop Vulkan renderer
//! draws.
//!
//! The project arrives as a *game pack* (see [`pack`]) fetched by the page.
//! No wasm-bindgen: the page calls the `kit_*` functions exported below and
//! reads results straight out of WebAssembly memory.

pub use kit_resource::pack;

use anyhow::{Context, Result};
use glam::{Mat4, Vec3};
use kit_core::App;
use kit_ecs::Entity;
use kit_gamesys::{MeshKind, SceneFile, Terrain};
use kit_render::View;
use kit_render::draw::{self, DrawItem, DrawUniforms, Geometry, ImportedGeometry, Pass};
use kit_resource::{ProjectConfig, Resources};
use std::collections::HashMap;

/// Command buffer opcodes (mirrored in kit.js).
pub mod op {
    /// id, layout (0 = mesh: 8 floats/vertex, 1 = terrain: 11), vertex count, index count, vertices, indices.
    pub const GEOMETRY: u32 = 1;
    pub const FREE_GEOMETRY: u32 = 2;
    /// id, width, height, mip count, then each mip's RGBA8 bytes (padded to 4).
    pub const TEXTURE: u32 = 3;
    /// clear color (4 floats, linear), frame uniforms (32 floats).
    pub const FRAME: u32 = 5;
    /// pass (0 opaque, 1 blended, 2 sprite, 3 terrain), geometry, first, count, texture, uniforms (32 floats).
    pub const DRAW: u32 = 6;
    pub const END: u32 = 7;
    /// Post-processing for this frame (right after FRAME; absent = draw
    /// straight to the canvas): uniform byte count, uniforms, temp count, each
    /// temp's size divisor, pass count, then per pass: entry name (length +
    /// bytes, padded to 4), inputs a and b, output (slot codes: 0 none,
    /// 1 scene, 2 output, 3 + n = temp n), and its 256-byte uniforms.
    pub const POST: u32 = 8;
}

/// Passes as numbered in the command buffer.
fn pass_id(pass: Pass) -> u32 {
    match pass {
        Pass::Opaque => 0,
        Pass::Blended => 1,
        Pass::Sprite => 2,
        // Colored meshes use the terrain pipeline (per-vertex color).
        Pass::Colored => PASS_TERRAIN,
    }
}
const PASS_TERRAIN: u32 = 3;

/// The built-in shaders adapted for WebGPU: per-draw data comes from a
/// uniform buffer (WebGPU has no push constants).
pub fn web_shader() -> String {
    kit_render::SHADER_SRC.replace("var<immediate> pc: Push;", "@group(2) @binding(0) var<uniform> pc: Push;")
}

/// The running game and what the page has already been sent.
pub struct WebPlayer {
    pub app: App,
    pub title: String,
    out: Vec<u8>,
    next_id: u32,
    builtin: HashMap<MeshKind, u32>,
    imported: HashMap<String, Option<(u32, ImportedGeometry)>>,
    terrains: HashMap<Entity, (u64, u32)>,
    /// CPU-deformed meshes (e.g. volume rigs): (revision, geometry id, colored).
    deformed: HashMap<Entity, (u64, u32, bool)>,
    /// Texture path → id (None = failed to load, don't retry).
    textures: HashMap<String, Option<u32>>,
}

impl WebPlayer {
    /// Start a game from a game pack.
    pub fn new(pack_bytes: &[u8]) -> Result<Self> {
        let files = pack::read(pack_bytes)?;
        let resources = Resources::in_memory(files);
        let config = ProjectConfig::parse(&resources.read_text(kit_resource::PROJECT_FILE)?)?;
        let mut app = App::new();
        let physics = &config.physics;
        app.add_module(kit_resource::ResourceModule { root: "/game".into() })
            .add_module(kit_scene::SceneModule)
            .add_module(kit_input::InputModule)
            .add_module(kit_gamesys::GameSysModule)
            .add_module(kit_script::ScriptModule)
            .add_module(kit_path::PathModule)
            .add_module(kit_gamedata::GameDataModule)
            .add_module(kit_diagram::DiagramModule)
            .add_module(kit_rigg::RiggModule)
            .add_module(kit_collider::ColliderModule)
            .add_module(kit_physics::PhysicsModule {
                gravity: Vec3::from_array(physics.gravity),
                rate: physics.rate,
                enabled: physics.enabled,
            })
            // The page pulls mixed audio (kit_audio_mix) for Web Audio.
            .add_module(kit_audio::AudioModule { device: false });
        // Read the project from memory instead of a folder.
        app.world.insert_resource(resources.clone());
        kit_gamedata::reload(&mut app.world);
        let main = &config.project.main_scene;
        let text = resources.read_text(main).with_context(|| format!("loading main scene `{main}`"))?;
        let scene = SceneFile::from_json(&text).with_context(|| format!("in {main}"))?;
        let spawned = kit_gamesys::load_scene(&mut app.world, &scene)?;
        kit_gamesys::scenes::mark_loaded(&mut app.world, main, spawned);
        kit_scene::propagate_transforms(&mut app.world);
        Ok(Self {
            app,
            title: config.project.title.clone(),
            out: Vec::new(),
            next_id: 1,
            builtin: HashMap::new(),
            imported: HashMap::new(),
            terrains: HashMap::new(),
            deformed: HashMap::new(),
            textures: HashMap::new(),
        })
    }

    fn id(&mut self) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    fn u32(&mut self, v: u32) {
        self.out.extend_from_slice(&v.to_le_bytes());
    }

    fn f32s(&mut self, v: &[f32]) {
        self.out.extend_from_slice(bytemuck::cast_slice(v));
    }

    fn geometry(&mut self, id: u32, layout: u32, vertices: &[f32], vertex_count: usize, indices: &[u32]) {
        self.u32(op::GEOMETRY);
        self.u32(id);
        self.u32(layout);
        self.u32(vertex_count as u32);
        self.u32(indices.len() as u32);
        self.f32s(vertices);
        self.out.extend_from_slice(bytemuck::cast_slice(indices));
    }

    /// Upload anything new: built-in meshes, imported models, changed terrain, textures.
    fn sync(&mut self) {
        if self.builtin.is_empty() {
            for kind in MeshKind::ALL {
                let data = kit_render::build_mesh(kind);
                let id = self.id();
                self.geometry(id, 0, bytemuck::cast_slice(&data.vertices), data.vertices.len(), &data.indices);
                self.builtin.insert(kind, id);
            }
        }
        let resources = self.app.world.resource::<Resources>().clone();
        for asset in draw::wanted_models(&self.app.world) {
            if self.imported.contains_key(&asset) {
                continue;
            }
            let loaded = match ImportedGeometry::load(&resources, &asset) {
                Ok(mut geometry) => {
                    let data = std::mem::take(&mut geometry.data);
                    let id = self.id();
                    self.geometry(id, 0, bytemuck::cast_slice(&data.vertices), data.vertices.len(), &data.indices);
                    Some((id, geometry))
                }
                Err(e) => {
                    log::error!("{asset}: {e:#}");
                    None
                }
            };
            self.imported.insert(asset, loaded);
        }
        // Terrain: rebuild when its shape or colors change (scripts can sculpt).
        let live: Vec<(Entity, u64)> = self
            .app
            .world
            .storage::<Terrain>()
            .map(|s| s.iter().map(|(e, t)| (e, t.fingerprint())).collect())
            .unwrap_or_default();
        let stale: Vec<Entity> = self
            .terrains
            .iter()
            .filter(|(e, (fp, _))| !live.iter().any(|(le, lfp)| le == *e && lfp == fp))
            .map(|(e, _)| *e)
            .collect();
        for e in stale {
            if let Some((_, id)) = self.terrains.remove(&e) {
                self.u32(op::FREE_GEOMETRY);
                self.u32(id);
            }
        }
        for (e, fp) in live {
            if self.terrains.contains_key(&e) {
                continue;
            }
            let (vertices, indices) = {
                let t = self.app.world.get::<Terrain>(e).unwrap();
                kit_render::terrain_mesh(&t)
            };
            let id = self.id();
            self.geometry(id, 1, bytemuck::cast_slice(&vertices), vertices.len(), &indices);
            self.terrains.insert(e, (fp, id));
        }
        // Deformed meshes: re-send when their revision changes.
        let live: Vec<(Entity, u64)> = self
            .app
            .world
            .try_resource::<draw::Deformed>()
            .map(|d| d.meshes.iter().map(|(e, m)| (*e, m.revision)).collect())
            .unwrap_or_default();
        let stale: Vec<Entity> = self
            .deformed
            .iter()
            .filter(|(e, (rev, _, _))| !live.iter().any(|(le, lr)| le == *e && lr == rev))
            .map(|(e, _)| *e)
            .collect();
        for e in stale {
            if let Some((_, id, _)) = self.deformed.remove(&e) {
                self.u32(op::FREE_GEOMETRY);
                self.u32(id);
            }
        }
        for (e, rev) in live {
            if self.deformed.contains_key(&e) {
                continue;
            }
            let (data, colored, count): (Vec<f32>, bool, usize) = {
                let d = self.app.world.resource::<draw::Deformed>();
                let mesh = &d.meshes[&e];
                match mesh.colored_vertices() {
                    Some(c) => (bytemuck::cast_slice(&c).to_vec(), true, c.len()),
                    None => (bytemuck::cast_slice(&mesh.vertices).to_vec(), false, mesh.vertices.len()),
                }
            };
            if count == 0 {
                continue;
            }
            let indices: Vec<u32> = (0..count as u32).collect();
            let id = self.id();
            self.geometry(id, if colored { 1 } else { 0 }, &data, count, &indices);
            self.deformed.insert(e, (rev, id, colored));
        }
        let wanted = draw::wanted_textures(&self.app.world, |asset| {
            self.imported.get(asset).and_then(|m| m.as_ref()).map(|(_, g)| g)
        });
        for path in wanted {
            if self.textures.contains_key(&path) {
                continue;
            }
            match resources.load_image(&path) {
                Ok(img) => {
                    let id = self.id();
                    let mips = mip_chain(img.width, img.height, img.rgba);
                    self.u32(op::TEXTURE);
                    self.u32(id);
                    self.u32(mips[0].0);
                    self.u32(mips[0].1);
                    self.u32(mips.len() as u32);
                    for (_, _, rgba) in &mips {
                        self.out.extend_from_slice(rgba);
                        while self.out.len() % 4 != 0 {
                            self.out.push(0);
                        }
                    }
                    self.textures.insert(path, Some(id));
                }
                Err(e) => {
                    log::error!("{e:#}");
                    self.textures.insert(path, None);
                }
            }
        }
    }

    fn geometry_id(&self, g: &Geometry) -> Option<u32> {
        match g {
            Geometry::Builtin(kind) => self.builtin.get(kind).copied(),
            Geometry::Imported(asset) => self.imported.get(asset)?.as_ref().map(|(id, _)| *id),
            Geometry::Deformed(e) => self.deformed.get(e).map(|(_, id, _)| *id),
        }
    }

    fn draw_cmd(&mut self, pass: u32, geometry: u32, first: u32, count: u32, texture: u32, u: &DrawUniforms) {
        self.u32(op::DRAW);
        self.u32(pass);
        self.u32(geometry);
        self.u32(first);
        self.u32(count);
        self.u32(texture);
        self.f32s(bytemuck::cast_slice(std::slice::from_ref(u)));
    }

    fn item(&mut self, item: &DrawItem) {
        let Some(geometry) = self.geometry_id(&item.geometry) else { return };
        let texture =
            if item.texture.is_empty() { 0 } else { self.textures.get(&item.texture).copied().flatten().unwrap_or(0) };
        // u32::MAX = the whole mesh (the page knows each mesh's index count).
        let count = item.count.unwrap_or(u32::MAX);
        self.draw_cmd(pass_id(item.pass), geometry, item.first, count, texture, &item.uniforms);
    }

    fn post(&mut self, post: &kit_post::PostFrame) {
        let slot = |s: Option<kit_post::Slot>| match s {
            None => 0,
            Some(kit_post::Slot::Scene) => 1,
            Some(kit_post::Slot::Out) => 2,
            Some(kit_post::Slot::Temp(i)) => 3 + i as u32,
        };
        self.u32(op::POST);
        let uniforms = bytemuck::bytes_of(&post.uniforms).to_vec();
        self.u32(uniforms.len() as u32);
        self.out.extend_from_slice(&uniforms);
        self.u32(kit_post::TEMPS.len() as u32);
        for (_, divisor) in kit_post::TEMPS {
            self.u32(divisor);
        }
        self.u32(post.passes.len() as u32);
        for p in &post.passes {
            self.u32(p.entry.len() as u32);
            self.out.extend_from_slice(p.entry.as_bytes());
            while self.out.len() % 4 != 0 {
                self.out.push(0);
            }
            self.u32(slot(p.a));
            self.u32(slot(p.b));
            self.u32(slot(Some(p.output)));
            self.out.extend_from_slice(bytemuck::bytes_of(&p.uniforms));
        }
    }

    /// Run one frame and build its command buffer.
    pub fn frame(&mut self, dt: f32, width: f32, height: f32) -> &[u8] {
        self.out.clear();
        self.app.update(dt);
        self.sync();

        let aspect = width / height.max(1.0);
        let world = &self.app.world;
        let mut view = View::from_camera(world, aspect).unwrap_or_else(|| View::fallback(aspect));
        view.time = world.resource::<kit_core::Time>().elapsed as f32;
        let mut uniforms = draw::frame_uniforms(world, &view);
        // Kit's projections target Vulkan (y down); WebGPU's clip space is y up.
        uniforms.view_proj = (Mat4::from_scale(Vec3::new(1.0, -1.0, 1.0)) * view.view_proj()).to_cols_array_2d();
        let clear = kit_render::srgb_to_linear(view.clear);
        let list =
            draw::draw_list(world, &view, |asset| self.imported.get(asset).and_then(|m| m.as_ref()).map(|(_, g)| g));

        let post = view.post.then(|| {
            kit_post::frame(world, &kit_render::camera_info(&view, true), width.max(1.0) as u32, height.max(1.0) as u32)
        });
        self.u32(op::FRAME);
        self.f32s(&clear.to_array());
        self.f32s(bytemuck::cast_slice(std::slice::from_ref(&uniforms)));
        if let Some(Some(post)) = post {
            self.post(&post);
        }
        for item in &list.opaque {
            self.item(item);
        }
        for t in &list.terrains {
            if let Some(&(_, id)) = self.terrains.get(&t.entity) {
                self.draw_cmd(PASS_TERRAIN, id, 0, u32::MAX, 0, &t.uniforms);
            }
        }
        for item in &list.blended {
            self.item(item);
        }
        self.u32(op::END);
        &self.out
    }
}

/// RGBA8 mip levels down to 1×1 (box filter), like the desktop renderer's mipmaps.
fn mip_chain(width: u32, height: u32, rgba: Vec<u8>) -> Vec<(u32, u32, Vec<u8>)> {
    let mut mips = vec![(width, height, rgba)];
    loop {
        let (w, h, src) = mips.last().unwrap();
        let (w, h) = (*w, *h);
        if w == 1 && h == 1 {
            break;
        }
        let (nw, nh) = ((w / 2).max(1), (h / 2).max(1));
        let mut dst = vec![0u8; (nw * nh * 4) as usize];
        for y in 0..nh {
            for x in 0..nw {
                for c in 0..4 {
                    let mut sum = 0u32;
                    for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                        let sx = (x * 2 + dx).min(w - 1);
                        let sy = (y * 2 + dy).min(h - 1);
                        sum += src[((sy * w + sx) * 4 + c) as usize] as u32;
                    }
                    dst[((y * nw + x) * 4 + c) as usize] = (sum / 4) as u8;
                }
            }
        }
        mips.push((nw, nh, dst));
    }
    mips
}

// ------------------------------------------------------------ wasm exports

#[cfg(target_arch = "wasm32")]
mod exports {
    use super::*;
    use std::cell::RefCell;

    thread_local! {
        static PLAYER: RefCell<Option<WebPlayer>> = const { RefCell::new(None) };
        static TEXT: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
        static SOUND: RefCell<Vec<f32>> = const { RefCell::new(Vec::new()) };
    }

    /// Mix the next `frames` of game audio at `rate` Hz (interleaved stereo
    /// f32, at the returned pointer) for the page to queue in Web Audio.
    #[unsafe(no_mangle)]
    pub extern "C" fn kit_audio_mix(frames: u32, rate: u32) -> *const f32 {
        SOUND.with(|buf| {
            let mut buf = buf.borrow_mut();
            buf.resize(frames as usize * 2, 0.0);
            PLAYER.with(|cell| {
                match cell
                    .borrow()
                    .as_ref()
                    .and_then(|p| p.app.world.try_resource::<kit_audio::Audio>().map(|a| a.mixer.clone()))
                {
                    Some(mixer) => {
                        let mut m = mixer.lock().unwrap();
                        m.rate = rate;
                        m.mix(&mut buf);
                    }
                    None => buf.fill(0.0),
                }
            });
            buf.as_ptr()
        })
    }

    /// Log lines waiting for the page: level byte (1 info, 2 warn, 3 error) + text + '\n'.
    static LOGS: std::sync::Mutex<Vec<u8>> = std::sync::Mutex::new(Vec::new());

    struct PageLogger;
    impl log::Log for PageLogger {
        fn enabled(&self, m: &log::Metadata) -> bool {
            m.level() <= log::Level::Info
        }
        fn log(&self, r: &log::Record) {
            if !self.enabled(r.metadata()) {
                return;
            }
            let level = match r.level() {
                log::Level::Error => b'3',
                log::Level::Warn => b'2',
                _ => b'1',
            };
            if let Ok(mut logs) = LOGS.lock() {
                logs.push(level);
                logs.extend_from_slice(format!("{}\n", r.args()).replace('\n', " ").as_bytes());
                logs.push(b'\n');
            }
        }
        fn flush(&self) {}
    }

    fn set_text(s: &str) {
        TEXT.with(|t| *t.borrow_mut() = s.as_bytes().to_vec());
    }

    #[unsafe(no_mangle)]
    pub extern "C" fn kit_alloc(len: usize) -> *mut u8 {
        let mut v = Vec::<u8>::with_capacity(len);
        let p = v.as_mut_ptr();
        std::mem::forget(v);
        p
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn kit_free(ptr: *mut u8, len: usize) {
        drop(unsafe { Vec::from_raw_parts(ptr, 0, len) });
    }

    /// The text result of the last call (error message, shader, build id, logs).
    #[unsafe(no_mangle)]
    pub extern "C" fn kit_text_ptr() -> *const u8 {
        TEXT.with(|t| t.borrow().as_ptr())
    }

    #[unsafe(no_mangle)]
    pub extern "C" fn kit_text_len() -> usize {
        TEXT.with(|t| t.borrow().len())
    }

    #[unsafe(no_mangle)]
    pub extern "C" fn kit_shader() {
        set_text(&web_shader());
    }

    /// The post-processing shader (for the page's 4× MSAA depth).
    #[unsafe(no_mangle)]
    pub extern "C" fn kit_post_shader() {
        set_text(&kit_post::shader_source(4));
    }

    #[unsafe(no_mangle)]
    pub extern "C" fn kit_build_id() {
        set_text(kit_core::BUILD_MARKER);
    }

    /// Move pending log lines into the text result.
    #[unsafe(no_mangle)]
    pub extern "C" fn kit_logs() {
        let lines = LOGS.lock().map(|mut l| std::mem::take(&mut *l)).unwrap_or_default();
        TEXT.with(|t| *t.borrow_mut() = lines);
    }

    /// Start the game from a pack in memory (the page frees it after).
    /// Returns 1 on success; on failure 0 and the error in the text result.
    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn kit_start(ptr: *const u8, len: usize, seed: f64) -> u32 {
        let _ = log::set_logger(&PageLogger).map(|()| log::set_max_level(log::LevelFilter::Info));
        std::panic::set_hook(Box::new(|info| {
            log::error!("Kit crashed: {info}");
        }));
        kit_x::set_random_seed(seed.to_bits() | 1);
        let bytes = unsafe { std::slice::from_raw_parts(ptr, len) };
        match WebPlayer::new(bytes) {
            Ok(p) => {
                set_text(&p.title);
                PLAYER.with(|cell| *cell.borrow_mut() = Some(p));
                1
            }
            Err(e) => {
                set_text(&format!("{e:#}"));
                0
            }
        }
    }

    /// Start a protected (signed) game: verify `ptr/len` (a sealed pack)
    /// against the publisher key the page pinned (`key_ptr`, 32 bytes) before
    /// reading any of it. Returns 1, or 0 with the reason in the text result.
    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn kit_start_sealed(
        ptr: *const u8,
        len: usize,
        key_ptr: *const u8,
        key_len: usize,
        seed: f64,
    ) -> u32 {
        let sealed = unsafe { std::slice::from_raw_parts(ptr, len) };
        let key = unsafe { std::slice::from_raw_parts(key_ptr, key_len) };
        let Ok(key) = <[u8; 32]>::try_from(key) else {
            set_text("the page's publisher key is invalid");
            return 0;
        };
        match kit_export_security::open(sealed, &key, kit_export_security::Mode::Signed, None) {
            Ok(pack) => unsafe { kit_start(pack.as_ptr(), pack.len(), seed) },
            Err(e) => {
                set_text(&format!("This game's files failed their security check: {e}."));
                0
            }
        }
    }

    /// Run one frame. The command buffer is at `kit_frame_ptr()` / `kit_frame_len()`.
    #[unsafe(no_mangle)]
    pub extern "C" fn kit_frame(dt: f32, width: f32, height: f32) -> u32 {
        PLAYER.with(|cell| match cell.borrow_mut().as_mut() {
            Some(p) => {
                p.frame(dt, width, height);
                1
            }
            None => 0,
        })
    }

    #[unsafe(no_mangle)]
    pub extern "C" fn kit_frame_ptr() -> *const u8 {
        PLAYER.with(|cell| cell.borrow().as_ref().map_or(std::ptr::null(), |p| p.out.as_ptr()))
    }

    #[unsafe(no_mangle)]
    pub extern "C" fn kit_frame_len() -> usize {
        PLAYER.with(|cell| cell.borrow().as_ref().map_or(0, |p| p.out.len()))
    }

    fn with_input(f: impl FnOnce(&mut kit_input::Input)) {
        PLAYER.with(|cell| {
            if let Some(p) = cell.borrow_mut().as_mut() {
                if let Some(mut input) = p.app.world.try_resource_mut::<kit_input::Input>() {
                    f(&mut input);
                }
            }
        });
    }

    /// A key went down (1) or up (0). `ptr/len` is the DOM `KeyboardEvent.code`,
    /// e.g. "KeyW", "ArrowLeft", "ShiftLeft"; Kit normalizes the names.
    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn kit_key(ptr: *const u8, len: usize, down: u32) {
        let name = String::from_utf8_lossy(unsafe { std::slice::from_raw_parts(ptr, len) }).into_owned();
        with_input(|i| if down != 0 { i.press(&name) } else { i.release(&name) });
    }

    /// Mouse button 0 left, 1 middle, 2 right (DOM numbering).
    #[unsafe(no_mangle)]
    pub extern "C" fn kit_mouse_button(button: u32, down: u32) {
        let name = match button {
            0 => "MouseLeft".to_owned(),
            1 => "MouseMiddle".to_owned(),
            2 => "MouseRight".to_owned(),
            3 => "MouseBack".to_owned(),
            4 => "MouseForward".to_owned(),
            n => format!("Mouse{n}"),
        };
        with_input(|i| if down != 0 { i.press(&name) } else { i.release(&name) });
    }

    #[unsafe(no_mangle)]
    pub extern "C" fn kit_mouse_move(x: f32, y: f32) {
        with_input(|i| i.set_mouse(glam::Vec2::new(x, y)));
    }

    /// Scroll in lines (positive = away from the user, like desktop).
    #[unsafe(no_mangle)]
    pub extern "C" fn kit_wheel(lines: f32) {
        with_input(|i| i.scroll += lines);
    }

    /// The page lost focus: release every key.
    #[unsafe(no_mangle)]
    pub extern "C" fn kit_blur() {
        with_input(|i| i.clear());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hello_pack() -> Vec<u8> {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/hello");
        pack::write_dir(&root).unwrap()
    }

    /// Walk a command buffer, returning (opcode, payload offset) pairs.
    fn commands(buf: &[u8]) -> Vec<u32> {
        let word = |i: usize| u32::from_le_bytes(buf[i..i + 4].try_into().unwrap());
        let mut ops = Vec::new();
        let mut i = 0;
        while i < buf.len() {
            let o = word(i);
            ops.push(o);
            i += 4;
            i += match o {
                op::GEOMETRY => {
                    let (layout, v, n) = (word(i + 4), word(i + 8) as usize, word(i + 12) as usize);
                    16 + v * if layout == 0 { 32 } else { 44 } + n * 4
                }
                op::FREE_GEOMETRY => 4,
                op::TEXTURE => {
                    let (mut w, mut h, mips) = (word(i + 4), word(i + 8), word(i + 12));
                    let mut size = 16;
                    for _ in 0..mips {
                        size += ((w * h * 4) as usize).div_ceil(4) * 4;
                        w = (w / 2).max(1);
                        h = (h / 2).max(1);
                    }
                    size
                }
                op::FRAME => 4 * (4 + 32),
                op::POST => {
                    let mut j = i;
                    let n = word(j) as usize;
                    j += 4 + n;
                    let temps = word(j) as usize;
                    j += 4 + temps * 4;
                    let passes = word(j) as usize;
                    j += 4;
                    for _ in 0..passes {
                        let len = word(j) as usize;
                        j += 4 + len.div_ceil(4) * 4 + 12 + 256;
                    }
                    j - i
                }
                op::DRAW => 4 * 5 + 4 * 32,
                op::END => 0,
                other => panic!("unknown opcode {other}"),
            };
        }
        assert_eq!(i, buf.len(), "command sizes add up");
        ops
    }

    #[test]
    fn hello_runs_and_draws_everything() {
        let mut p = WebPlayer::new(&hello_pack()).unwrap();
        let first = commands(p.frame(1.0 / 60.0, 1280.0, 720.0));
        assert!(first.contains(&op::GEOMETRY) && first.contains(&op::FRAME) && first.ends_with(&[op::END]));
        let world = &p.app.world;
        let models = world.storage::<kit_gamesys::Model>().map_or(0, |s| s.iter().filter(|(_, m)| m.visible).count());
        let terrains = world.storage::<Terrain>().map_or(0, |s| s.iter().count());
        // Later frames only draw (everything is uploaded once).
        let static_meshes = (p.builtin.len(), p.imported.len(), p.terrains.len(), p.textures.len());
        let deformed_before: Vec<(Entity, u64)> = p.deformed.iter().map(|(e, (r, _, _))| (*e, *r)).collect();
        let ops = commands(p.frame(1.0 / 60.0, 1280.0, 720.0));
        let draws = ops.iter().filter(|o| **o == op::DRAW).count();
        assert!(draws >= models + terrains, "{draws} draws for {models} models + {terrains} terrains");
        // Static meshes and textures upload once; only rigs whose pose changed
        // (e.g. volumes following Canvas paths) send their mesh again.
        assert!(!ops.contains(&op::TEXTURE));
        assert_eq!((p.builtin.len(), p.imported.len(), p.terrains.len(), p.textures.len()), static_meshes);
        let reposed = p.deformed.iter().filter(|(e, (r, _, _))| !deformed_before.contains(&(**e, *r))).count();
        let geometry_ops = ops.iter().filter(|o| **o == op::GEOMETRY).count();
        assert_eq!(geometry_ops, reposed, "only re-posed rigs re-upload");
        // Imported models and their textures made it in.
        assert!(p.imported.values().all(Option::is_some), "imported models load from the pack");
        assert!(p.textures.values().all(Option::is_some), "textures load from the pack");
        assert!(p.app.world.storage::<kit_gamesys::Script>().is_some());
    }

    #[test]
    fn post_volumes_send_a_post_command() {
        let hello = commands(WebPlayer::new(&hello_pack()).unwrap().frame(1.0 / 60.0, 640.0, 360.0));
        assert!(!hello.contains(&op::POST), "no volumes: direct path");
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/volumes");
        let mut p = WebPlayer::new(&pack::write_dir(&root).unwrap()).unwrap();
        let ops = commands(p.frame(1.0 / 60.0, 640.0, 360.0));
        let at = ops.iter().position(|o| *o == op::POST).expect("POST command");
        assert_eq!(ops[at - 1], op::FRAME, "POST follows FRAME");
    }

    #[test]
    fn web_shader_uses_a_uniform_for_draw_data() {
        let s = web_shader();
        assert!(!s.contains("var<immediate>") && s.contains("@group(2) @binding(0) var<uniform> pc: Push;"));
    }

    #[test]
    fn mips_go_down_to_one_pixel() {
        let mips = mip_chain(4, 2, vec![255; 4 * 2 * 4]);
        let sizes: Vec<_> = mips.iter().map(|(w, h, d)| (*w, *h, d.len())).collect();
        assert_eq!(sizes, [(4, 2, 32), (2, 1, 8), (1, 1, 4)]);
    }
}
