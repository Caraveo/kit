//! Kit editor: a native Rust scene editor with a Vulkan viewport.
//!
//! The viewport renders the scene with `kit_render` into an offscreen image
//! that egui shows as a rounded panel, styled after Dimension.

#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod camera;
mod canvas;
mod code;
mod controls;
mod diagram_view;
mod markdown;
mod rex_panel;
mod rigg_mode;
mod rigg_timeline;
mod export;
mod export_wizard;
mod gizmos;
mod hooks;
mod model_import;
mod move_gizmo;
mod new_project;
mod settings;
mod state;
mod templates;
mod terrain_tools;
mod theme;
mod ui;
#[cfg(test)]
mod ui_tests;
mod widgets;

use anyhow::{Context, Result};
use egui_ash_renderer::{DynamicRendering, Options, RenderMode};
use kit::graphics::{Gfx, Image, Presenter, vk};
use kit::render::{Renderer, Target, View};
use raw_window_handle::{HasDisplayHandle, HasWindowHandle};
use state::Editor;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use ui::{Request, UiState};
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowId};

const VIEWPORT_FORMAT: vk::Format = vk::Format::R8G8B8A8_SRGB;

/// The offscreen image the scene is rendered into, shown by egui.
struct ViewportTarget {
    image: Option<Image>,
    layout: vk::DescriptorSetLayout,
    pool: vk::DescriptorPool,
    sampler: vk::Sampler,
    set: vk::DescriptorSet,
    tex: Option<egui::TextureId>,
    size: [u32; 2],
}

impl ViewportTarget {
    fn new(gfx: &Gfx) -> Result<Self> {
        let layout = egui_ash_renderer::vulkan::create_vulkan_descriptor_set_layout(&gfx.device)?;
        unsafe {
            let pool = gfx.device.create_descriptor_pool(
                &vk::DescriptorPoolCreateInfo::default().max_sets(1).pool_sizes(&[vk::DescriptorPoolSize {
                    ty: vk::DescriptorType::COMBINED_IMAGE_SAMPLER,
                    descriptor_count: 1,
                }]),
                None,
            )?;
            let set = gfx.device.allocate_descriptor_sets(
                &vk::DescriptorSetAllocateInfo::default().descriptor_pool(pool).set_layouts(&[layout]),
            )?[0];
            let sampler = gfx.device.create_sampler(
                &vk::SamplerCreateInfo::default()
                    .mag_filter(vk::Filter::LINEAR)
                    .min_filter(vk::Filter::LINEAR)
                    .address_mode_u(vk::SamplerAddressMode::CLAMP_TO_EDGE)
                    .address_mode_v(vk::SamplerAddressMode::CLAMP_TO_EDGE)
                    .address_mode_w(vk::SamplerAddressMode::CLAMP_TO_EDGE),
                None,
            )?;
            Ok(Self { image: None, layout, pool, sampler, set, tex: None, size: [0, 0] })
        }
    }

    /// (Re)create the image at `size`. Returns true when the image is new and
    /// must be transitioned from UNDEFINED.
    fn ensure(
        &mut self,
        gfx: &Gfx,
        egui: &mut egui_ash_renderer::Renderer<egui_ash_renderer::allocator::GpuAllocator>,
        size: [u32; 2],
    ) -> Result<bool> {
        let size = [size[0].clamp(1, 8192), size[1].clamp(1, 8192)];
        if self.image.is_some() && self.size == size {
            return Ok(false);
        }
        gfx.wait_idle();
        if let Some(mut old) = self.image.take() {
            old.destroy(gfx);
        }
        let image = Image::new(
            gfx,
            vk::Extent2D { width: size[0], height: size[1] },
            VIEWPORT_FORMAT,
            vk::ImageUsageFlags::COLOR_ATTACHMENT | vk::ImageUsageFlags::SAMPLED,
            vk::SampleCountFlags::TYPE_1,
            "viewport",
        )?;
        let info = [vk::DescriptorImageInfo::default()
            .sampler(self.sampler)
            .image_view(image.view)
            .image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)];
        unsafe {
            gfx.device.update_descriptor_sets(
                &[vk::WriteDescriptorSet::default()
                    .dst_set(self.set)
                    .dst_binding(0)
                    .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
                    .image_info(&info)],
                &[],
            );
        }
        if self.tex.is_none() {
            self.tex = Some(egui.add_user_texture(self.set));
        }
        self.image = Some(image);
        self.size = size;
        Ok(true)
    }

    fn destroy(&mut self, gfx: &Gfx) {
        if let Some(mut i) = self.image.take() {
            i.destroy(gfx);
        }
        unsafe {
            gfx.device.destroy_sampler(self.sampler, None);
            gfx.device.destroy_descriptor_pool(self.pool, None);
            gfx.device.destroy_descriptor_set_layout(self.layout, None);
        }
    }
}

/// Copy the swapchain image into a host-visible buffer (image stays in COLOR_ATTACHMENT_OPTIMAL).
fn record_capture(
    gfx: &Gfx,
    cmd: vk::CommandBuffer,
    image: vk::Image,
    extent: vk::Extent2D,
) -> Result<kit::graphics::Buffer> {
    let size = (extent.width * extent.height * 4) as u64;
    let buf = kit::graphics::Buffer::new(
        gfx,
        size,
        vk::BufferUsageFlags::TRANSFER_DST,
        kit::graphics::MemoryLocation::GpuToCpu,
        "capture",
    )?;
    gfx.image_barrier(
        cmd,
        image,
        vk::ImageAspectFlags::COLOR,
        vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
        vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
        (vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT, vk::AccessFlags::COLOR_ATTACHMENT_WRITE),
        (vk::PipelineStageFlags::TRANSFER, vk::AccessFlags::TRANSFER_READ),
    );
    let region = vk::BufferImageCopy::default()
        .image_subresource(
            vk::ImageSubresourceLayers::default().aspect_mask(vk::ImageAspectFlags::COLOR).layer_count(1),
        )
        .image_extent(vk::Extent3D { width: extent.width, height: extent.height, depth: 1 });
    unsafe {
        gfx.device.cmd_copy_image_to_buffer(cmd, image, vk::ImageLayout::TRANSFER_SRC_OPTIMAL, buf.buffer, &[region]);
    }
    gfx.image_barrier(
        cmd,
        image,
        vk::ImageAspectFlags::COLOR,
        vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
        vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
        (vk::PipelineStageFlags::TRANSFER, vk::AccessFlags::TRANSFER_READ),
        (vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT, vk::AccessFlags::COLOR_ATTACHMENT_WRITE),
    );
    Ok(buf)
}

struct Gpu {
    window: Arc<Window>,
    egui_state: egui_winit::State,
    egui_renderer: egui_ash_renderer::Renderer<egui_ash_renderer::allocator::GpuAllocator>,
    renderer: Renderer,
    viewport: ViewportTarget,
    viewport_fresh: bool,
    /// Picture-in-picture view through the selected camera.
    preview: ViewportTarget,
    preview_fresh: bool,
    presenter: Presenter,
    gfx: Gfx,
}

/// Debug/automation options (used for docs screenshots and smoke tests).
#[derive(Default)]
struct Automation {
    screenshot: Option<PathBuf>,
    frames: u32,
    select: Option<String>,
    script: Option<String>,
    play: bool,
    canvas: bool,
    /// Open the Diagram view.
    diagram: bool,
    /// Open Rex.
    rex: bool,
    /// Rig mode for the selection ("fit", "pose" or "weights").
    rig: Option<String>,
    /// Open the Rename dialog for this asset (screenshots).
    rename: Option<String>,
    /// Ask Rex this (in `rex_mode`) once connected.
    rex_ask: Option<String>,
    rex_mode: Option<String>,
    settings: bool,
    new_script: bool,
    terrain: bool,
    /// Export the project into this folder without opening a window, then exit.
    export: Option<PathBuf>,
    /// Open the Export wizard (for screenshots).
    wizard: bool,
    /// Platform for `--export` (default: this computer's).
    platform: Option<String>,
    /// `--protect standard|signed|encrypted` for `--export`.
    protect: Option<String>,
    /// `--key <file>`: the publisher key for protected exports.
    key: Option<PathBuf>,
    /// `--new-key <file>`: create a publisher key there and exit.
    new_key: Option<PathBuf>,
    /// Force the light (Some(true)) or dark (Some(false)) theme without saving it.
    light: Option<bool>,
    frame_count: u32,
}

struct EditorApp {
    auto: Automation,
    editor: Editor,
    ui: UiState,
    egui_ctx: egui::Context,
    gpu: Option<Gpu>,
    last: Instant,
    fps_acc: (f32, u32),
    startup_project: Option<PathBuf>,
    error: Option<anyhow::Error>,
    title: String,
}

// ------------------------------------------------------------- recent projects

fn config_dir() -> Option<PathBuf> {
    export::config_dir()
}

#[allow(dead_code)]
fn config_dir_old() -> Option<PathBuf> {
    if cfg!(windows) {
        std::env::var_os("APPDATA").map(|p| PathBuf::from(p).join("Kit"))
    } else if cfg!(target_os = "macos") {
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library/Application Support/Kit"))
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
            .map(|p| p.join("kit"))
    }
}

fn load_recent() -> Vec<PathBuf> {
    let Some(f) = config_dir().map(|d| d.join("recent.txt")) else { return Vec::new() };
    std::fs::read_to_string(f)
        .unwrap_or_default()
        .lines()
        .map(PathBuf::from)
        .filter(|p| p.join(kit::resource::PROJECT_FILE).is_file())
        .collect()
}

fn remember(recent: &mut Vec<PathBuf>, path: &Path) {
    let p = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    recent.retain(|r| *r != p);
    recent.insert(0, p);
    recent.truncate(10);
    if let Some(dir) = config_dir() {
        let _ = std::fs::create_dir_all(&dir);
        let text: Vec<String> = recent.iter().map(|p| p.display().to_string()).collect();
        let _ = std::fs::write(dir.join("recent.txt"), text.join("\n"));
    }
}

fn user_projects_dir() -> PathBuf {
    let home =
        std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from).unwrap_or_default();
    let docs = home.join("Documents");
    if docs.is_dir() { docs.join("Kit Projects") } else { home.join("Kit Projects") }
}

/// The bundled example project, wherever this build keeps it.
fn bundled_example() -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join("examples/hello"));
            candidates.push(dir.join("../share/kit/examples/hello"));
            candidates.push(dir.join("../Resources/examples/hello"));
        }
    }
    candidates.push(Path::new(env!("CARGO_MANIFEST_DIR")).join("../examples/hello"));
    candidates.into_iter().find(|p| p.join(kit::resource::PROJECT_FILE).is_file())
}

fn copy_dir(from: &Path, to: &Path) -> Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let p = entry.path();
        let dest = to.join(entry.file_name());
        if p.is_dir() {
            copy_dir(&p, &dest)?;
        } else {
            std::fs::copy(&p, &dest)?;
        }
    }
    Ok(())
}

/// The Kit logo for the window, taskbar and dock.
fn app_icon() -> Option<winit::window::Icon> {
    let img = image::load_from_memory(include_bytes!("../../packaging/icons/kit-256.png")).ok()?.into_rgba8();
    let (w, h) = img.dimensions();
    winit::window::Icon::from_rgba(img.into_raw(), w, h).ok()
}

impl EditorApp {
    fn init(&mut self, event_loop: &ActiveEventLoop) -> Result<()> {
        let attributes = Window::default_attributes()
            .with_title("Kit")
            .with_inner_size(LogicalSize::new(1480, 900))
            .with_min_inner_size(LogicalSize::new(900, 560))
            .with_window_icon(app_icon());
        #[cfg(target_os = "linux")]
        let attributes = winit::platform::wayland::WindowAttributesExtWayland::with_name(
            attributes,
            "io.github.caraveo.Kit",
            "kit-editor",
        );
        let window = Arc::new(event_loop.create_window(attributes)?);
        let display = window.display_handle()?.as_raw();
        let gfx = Gfx::new(Some(display), "Kit Editor")?;
        let surface = gfx.create_surface(display, window.window_handle()?.as_raw())?;
        let size = window.inner_size();
        let presenter = Presenter::new(&gfx, surface, size.width, size.height, true)?;
        let renderer = Renderer::new(&gfx)?;
        let egui_renderer = egui_ash_renderer::Renderer::with_gpu_allocator(
            gfx.allocator().clone(),
            gfx.device.clone(),
            RenderMode::DynamicRendering(DynamicRendering {
                color_attachment_format: presenter.format,
                depth_attachment_format: None,
                stencil_attachment_format: None,
            }),
            Options {
                in_flight_frames: kit::graphics::FRAMES_IN_FLIGHT,
                enable_depth_test: false,
                enable_depth_write: false,
                srgb_framebuffer: true,
            },
        )?;
        let egui_state = egui_winit::State::new(
            self.egui_ctx.clone(),
            egui::ViewportId::ROOT,
            &*window,
            Some(window.scale_factor() as f32),
            None,
            Some(4096),
        );
        let viewport = ViewportTarget::new(&gfx)?;
        let preview = ViewportTarget::new(&gfx)?;
        self.ui.gpu_name = gfx.device_name.clone();
        self.gpu = Some(Gpu {
            window,
            egui_state,
            egui_renderer,
            renderer,
            viewport,
            viewport_fresh: true,
            preview,
            preview_fresh: true,
            presenter,
            gfx,
        });
        if let Some(p) = self.startup_project.take() {
            self.open(&p);
        }
        if self.auto.terrain {
            let t = self.editor.spawn(state::Preset::Terrain, None);
            if let Some(mut terrain) = self.editor.app.world.get_mut::<kit::gamesys::Terrain>(t) {
                terrain.generate(4, 7.0, 0.55);
            }
            self.editor.app.world.get_mut::<kit::scene::Transform>(t).unwrap().translation.y = -0.5;
            kit::scene::propagate_transforms(&mut self.editor.app.world);
            self.editor.frame_all();
        }
        if let Some(name) = self.auto.select.clone() {
            self.editor.selected = kit::scene::find_by_name(&self.editor.app.world, &name);
            // Like clicking it in the hierarchy: bring it into view.
            self.editor.frame_selected();
        }
        if let Some(script) = self.auto.script.clone() {
            self.editor.open_script(&script);
        }
        if self.auto.play {
            self.editor.play();
        }
        self.editor.settings.canvas |= self.auto.canvas;
        self.editor.settings.diagram |= self.auto.diagram;
        self.editor.settings.rex |= self.auto.rex || self.auto.rex_ask.is_some();
        if let Some(how) = self.auto.rig.clone() {
            self.editor.settings.rig = true;
            self.ui.rigg.tool = if how == "fit" { rigg_mode::RigTool::Fit } else { rigg_mode::RigTool::Pose };
            self.ui.rigg.weights = how == "weights";
            self.ui.rigg.timeline.open = how == "animate";
        }
        if let Some(p) = self.auto.rename.clone() {
            self.ui.start_rename_for_test(&p);
        }
        if let Some(m) = self.auto.rex_mode.clone() {
            self.ui.rex.mode = m;
            self.editor.settings.rex = true;
        }
        if let Some(q) = self.auto.rex_ask.clone() {
            self.ui.rex.ask(self.auto.rex_mode.as_deref().unwrap_or("buddy"), q);
            self.ui.rex.auto_send = true;
        }
        self.editor.request_settings |= self.auto.settings;
        if self.auto.wizard {
            self.ui.export.show(&mut self.editor);
        }
        if self.auto.new_script {
            ui::begin_new_asset(&mut self.editor, &mut self.ui, ui::AssetKind::Script);
        }
        Ok(())
    }

    fn open(&mut self, path: &Path) {
        match self.editor.open_project(path) {
            Ok(()) => remember(&mut self.ui.recent, path),
            Err(e) => self.editor.report(Err(e)),
        }
    }

    fn handle_requests(&mut self) {
        for req in std::mem::take(&mut self.ui.requests) {
            match req {
                Request::OpenPath(p) => self.open(&p),
                Request::OpenProject => {
                    if let Some(dir) = rfd::FileDialog::new().set_title("Open a Kit project folder").pick_folder() {
                        self.open(&dir);
                    }
                }
                Request::BrowseNewProject => {
                    if let Some(wizard) = self.ui.new_project.as_mut() {
                        if let Some(path) = rfd::FileDialog::new()
                            .set_title("Choose a parent folder for the Kit project")
                            .set_directory(&wizard.location)
                            .pick_folder()
                        {
                            wizard.select_location(&path);
                        }
                    }
                }
                Request::CreateProject(path, options) => {
                    let result = (|| -> Result<()> {
                        self.editor.save_if_dirty()?;
                        for index in 0..self.editor.scripts.len() {
                            if self.editor.scripts[index].dirty() {
                                self.editor.save_script(index);
                                anyhow::ensure!(
                                    !self.editor.scripts[index].dirty(),
                                    "Could not save {}",
                                    self.editor.scripts[index].path
                                );
                            }
                        }
                        self.editor.create_project_from(&path, &options)
                    })();
                    match result {
                        Ok(()) => {
                            remember(&mut self.ui.recent, &path);
                            self.ui.new_project = None;
                            self.ui.ensure_visible(&mut self.editor);
                        }
                        Err(error) => {
                            if let Some(wizard) = self.ui.new_project.as_mut() {
                                wizard.error = Some(format!("{error:#}"));
                            }
                            self.editor.report(Err(error));
                        }
                    }
                }
                Request::OpenExample => match bundled_example() {
                    Some(src) => {
                        // Work on a copy so the installed example stays pristine.
                        let dest = user_projects_dir().join("Hello Kit");
                        let result = if dest.join(kit::resource::PROJECT_FILE).is_file() {
                            Ok(())
                        } else {
                            copy_dir(&src, &dest)
                        };
                        match result {
                            Ok(()) => self.open(&dest),
                            Err(e) => self.editor.report(Err(e.context("copying the example project"))),
                        }
                    }
                    None => self.editor.notify("The example project was not found next to the editor."),
                },
                Request::ImportModels => {
                    let Some(project) = &self.editor.project else { continue };
                    let mut all: Vec<&str> = vec!["png", "jpg", "jpeg", "x", "kscene", "kdiagram"];
                    all.extend(kit::gamesys::SOUND_EXTENSIONS);
                    all.extend(model_import::SOURCE_EXTENSIONS);
                    if let Some(files) = rfd::FileDialog::new()
                        .set_title("Import files")
                        .set_directory(project.root())
                        .add_filter("Everything Kit can use", &all)
                        .add_filter("Sounds", kit::gamesys::SOUND_EXTENSIONS)
                        .add_filter("Sprites", &["png", "jpg", "jpeg"])
                        .add_filter("3D models", model_import::SOURCE_EXTENSIONS)
                        .add_filter("Scripts, scenes and diagrams", &["x", "kscene", "kdiagram"])
                        .pick_files()
                    {
                        for path in files {
                            let result = self.editor.import_asset(&path).map(|_| ());
                            self.editor.report(result);
                        }
                    }
                }
                Request::SaveTheme => {
                    if let Some(dir) = config_dir() {
                        let _ = std::fs::create_dir_all(&dir);
                        let mode = if theme::is_light() { "light" } else { "dark" };
                        let _ = std::fs::write(dir.join("theme.txt"), mode);
                    }
                }
                Request::ImportPublisherKey => {
                    if let Some(file) = rfd::FileDialog::new().set_title("Import a Kit publisher key").pick_file() {
                        let result = (|| -> Result<String> {
                            let text = std::fs::read_to_string(&file)?;
                            let key = kit_export_security::PublisherKey::from_file_text(&text)?;
                            let path = export::default_key_path().context("no settings folder")?;
                            anyhow::ensure!(
                                !path.exists(),
                                "A publisher key already exists at {}. Move it away (keep a backup) to import another.",
                                path.display()
                            );
                            key.save(&path)?;
                            Ok(key.fingerprint())
                        })();
                        match result {
                            Ok(fp) => self.editor.notify(format!("Imported publisher key {fp}")),
                            Err(e) => self.editor.report(Err(e)),
                        }
                        self.ui.export.refresh_key();
                    }
                }
                Request::PickExportFolder => {
                    let start = self.ui.export.destination(&self.editor);
                    let mut dialog = rfd::FileDialog::new().set_title("Export the game into…");
                    if let Some(dir) = start.as_ref().and_then(|d| d.ancestors().find(|a| a.is_dir())) {
                        dialog = dialog.set_directory(dir);
                    }
                    if let Some(dir) = dialog.pick_folder() {
                        self.ui.export.folder = Some(dir);
                    }
                }
            }
        }
    }

    fn frame(&mut self, event_loop: &ActiveEventLoop) -> Result<()> {
        let now = Instant::now();
        let dt = (now - self.last).as_secs_f32().min(0.25);
        self.last = now;
        self.fps_acc.0 += dt;
        self.fps_acc.1 += 1;
        if self.fps_acc.0 >= 0.5 {
            self.ui.fps = self.fps_acc.1 as f32 / self.fps_acc.0;
            self.fps_acc = (0.0, 0);
        }

        let Some(gpu) = self.gpu.as_mut() else { return Ok(()) };
        let raw = gpu.egui_state.take_egui_input(&gpu.window);
        let editor = &mut self.editor;
        let ui_state = &mut self.ui;
        let mut output = self.egui_ctx.run_ui(raw, |ui| ui::draw(ui, editor, ui_state));
        gpu.egui_state.handle_platform_output(&gpu.window, std::mem::take(&mut output.platform_output));
        // Take texture changes out right away; egui asserts they are all applied.
        let textures_set: Vec<_> = output.textures_delta.set.drain().collect();
        let textures_free: Vec<_> = output.textures_delta.free.drain().collect();

        // Simulation.
        self.editor.camera.tick(dt);
        self.editor.app.update(dt);
        let at_rest = !self.egui_ctx.input(|i| i.pointer.any_down()) && self.egui_ctx.memory(|m| m.focused().is_none());
        self.editor.track_changes(at_rest);

        let title = format!(
            "Kit — {} — {}{}",
            self.editor.project_title(),
            self.editor.scene_name(),
            if self.editor.scene_dirty() { " •" } else { "" }
        );
        let gpu = self.gpu.as_mut().unwrap();
        if title != self.title {
            gpu.window.set_title(&title);
            self.title = title;
        }

        // GPU resources.
        let size = self.ui.viewport_px;
        if size[0] > 0 && size[1] > 0 && gpu.viewport.ensure(&gpu.gfx, &mut gpu.egui_renderer, size)? {
            gpu.viewport_fresh = true;
        }
        self.ui.viewport_tex = gpu.viewport.tex;
        let preview_size = self.ui.preview_px;
        let preview_on = preview_size[0] > 0 && self.ui.preview_camera.is_some();
        if preview_on && gpu.preview.ensure(&gpu.gfx, &mut gpu.egui_renderer, preview_size)? {
            gpu.preview_fresh = true;
        }
        self.ui.preview_tex = gpu.preview.tex.filter(|_| preview_on);
        for (id, deltas) in &textures_set {
            for delta in deltas {
                gpu.egui_renderer.set_texture(gpu.gfx.queue, gpu.gfx.upload_pool, *id, delta)?;
            }
        }
        gpu.renderer.prepare(&gpu.gfx, &self.editor.app.world);
        let primitives = self.egui_ctx.tessellate(std::mem::take(&mut output.shapes), output.pixels_per_point);

        if let Some(frame) = gpu.presenter.begin_frame(&gpu.gfx)? {
            let cmd = frame.cmd;
            let gfx = &gpu.gfx;
            // 1. Scene into the viewport image.
            let ed = &self.editor;
            if let Some(image) = gpu.viewport.image.as_ref().filter(|_| size[0] > 0 && size[1] > 0) {
                let old_layout = if gpu.viewport_fresh {
                    vk::ImageLayout::UNDEFINED
                } else {
                    vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL
                };
                gfx.image_barrier(
                    cmd,
                    image.image,
                    vk::ImageAspectFlags::COLOR,
                    old_layout,
                    vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
                    (vk::PipelineStageFlags::FRAGMENT_SHADER, vk::AccessFlags::SHADER_READ),
                    (vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT, vk::AccessFlags::COLOR_ATTACHMENT_WRITE),
                );
                gpu.viewport_fresh = false;
                let extent = image.extent;
                let aspect = extent.width as f32 / extent.height.max(1) as f32;
                let mut view = if ed.playing && ed.settings.game_camera {
                    View::from_camera(&ed.app.world, aspect).unwrap_or_else(|| ed.camera.view(aspect))
                } else {
                    let mut v = ed.camera.view(aspect);
                    if !ed.settings.grid {
                        v.grid = None;
                    }
                    if let Some(sel) = ed.selected.filter(|_| !ed.playing) {
                        v.selected = kit::scene::descendants_and_self(&ed.app.world, sel);
                    }
                    if theme::is_light() {
                        v.backdrop = false;
                        v.clear = ed.light_background();
                    }
                    v.post = ed.settings.volumes;
                    v
                };
                rigg_mode::decorate_view(ed, &mut view);
                view.lighting = ed.settings.lighting;
                view.wireframe = ed.settings.wireframe;
                view.time = ed.app.world.resource::<kit::core::Time>().elapsed as f32;
                let target = Target { image: image.image, view: image.view, extent, format: VIEWPORT_FORMAT };
                gpu.renderer.render(gfx, cmd, frame.frame_index, &target, &view, &ed.app.world)?;
                gfx.image_barrier(
                    cmd,
                    image.image,
                    vk::ImageAspectFlags::COLOR,
                    vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
                    vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
                    (vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT, vk::AccessFlags::COLOR_ATTACHMENT_WRITE),
                    (vk::PipelineStageFlags::FRAGMENT_SHADER, vk::AccessFlags::SHADER_READ),
                );
            }
            // 1b. Camera preview (picture-in-picture).
            if let (true, Some(cam), Some(img)) = (preview_on, self.ui.preview_camera, gpu.preview.image.as_ref()) {
                let old_layout = if gpu.preview_fresh {
                    vk::ImageLayout::UNDEFINED
                } else {
                    vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL
                };
                gfx.image_barrier(
                    cmd,
                    img.image,
                    vk::ImageAspectFlags::COLOR,
                    old_layout,
                    vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
                    (vk::PipelineStageFlags::FRAGMENT_SHADER, vk::AccessFlags::SHADER_READ),
                    (vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT, vk::AccessFlags::COLOR_ATTACHMENT_WRITE),
                );
                gpu.preview_fresh = false;
                let aspect = img.extent.width as f32 / img.extent.height.max(1) as f32;
                if let Some(mut pv) = View::from_camera_entity(&ed.app.world, cam, aspect) {
                    pv.lighting = ed.settings.lighting;
                    let target =
                        Target { image: img.image, view: img.view, extent: img.extent, format: VIEWPORT_FORMAT };
                    gpu.renderer.render(gfx, cmd, frame.frame_index, &target, &pv, &ed.app.world)?;
                }
                gfx.image_barrier(
                    cmd,
                    img.image,
                    vk::ImageAspectFlags::COLOR,
                    vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
                    vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
                    (vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT, vk::AccessFlags::COLOR_ATTACHMENT_WRITE),
                    (vk::PipelineStageFlags::FRAGMENT_SHADER, vk::AccessFlags::SHADER_READ),
                );
            }

            // 2. The UI into the swapchain.
            let [r, g, b, _] = theme::BG().to_array();
            let bg = kit::render::srgb_to_linear(kit::math::Vec4::new(
                r as f32 / 255.0,
                g as f32 / 255.0,
                b as f32 / 255.0,
                1.0,
            ));
            let color = [vk::RenderingAttachmentInfo::default()
                .image_view(frame.view)
                .image_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
                .load_op(vk::AttachmentLoadOp::CLEAR)
                .store_op(vk::AttachmentStoreOp::STORE)
                .clear_value(vk::ClearValue { color: vk::ClearColorValue { float32: bg.to_array() } })];
            let rendering = vk::RenderingInfo::default()
                .render_area(vk::Rect2D { offset: vk::Offset2D::default(), extent: frame.extent })
                .layer_count(1)
                .color_attachments(&color);
            unsafe { gfx.device.cmd_begin_rendering(cmd, &rendering) };
            gpu.egui_renderer.cmd_draw(cmd, frame.extent, output.pixels_per_point, &primitives)?;
            unsafe { gfx.device.cmd_end_rendering(cmd) };

            self.auto.frame_count += 1;
            let capture = self.auto.screenshot.is_some() && self.auto.frame_count >= self.auto.frames.max(3);
            let readback = if capture { Some(record_capture(gfx, cmd, frame.image, frame.extent)?) } else { None };
            let (format, extent) = (frame.format, frame.extent);
            gpu.presenter.end_frame(gfx, frame)?;
            if let Some(mut buf) = readback {
                gfx.wait_idle();
                let mut pixels = buf.mapped().map(|m| m.to_vec()).unwrap_or_default();
                buf.destroy(gfx);
                if matches!(format, vk::Format::B8G8R8A8_SRGB | vk::Format::B8G8R8A8_UNORM) {
                    for px in pixels.chunks_exact_mut(4) {
                        px.swap(0, 2);
                    }
                }
                let path = self.auto.screenshot.take().unwrap();
                image::save_buffer(&path, &pixels, extent.width, extent.height, image::ColorType::Rgba8)?;
                log::info!("screenshot written to {}", path.display());
                event_loop.exit();
            }
        }

        if !textures_free.is_empty() {
            gpu.gfx.wait_idle();
            for id in &textures_free {
                gpu.egui_renderer.free_texture(*id)?;
            }
        }

        self.handle_requests();

        // Keep animating while something moves; otherwise sleep until egui needs us.
        let repaint =
            output.viewport_output.get(&egui::ViewportId::ROOT).map(|v| v.repaint_delay).unwrap_or(Duration::MAX);
        let gpu = self.gpu.as_ref().unwrap();
        if self.editor.playing
            || self.editor.camera.auto_rotate
            || self.editor.camera.animating()
            || repaint.is_zero()
            || self.auto.screenshot.is_some()
        {
            gpu.window.request_redraw();
            event_loop.set_control_flow(ControlFlow::Poll);
        } else if repaint < Duration::from_secs(10) {
            event_loop.set_control_flow(ControlFlow::WaitUntil(Instant::now() + repaint));
        } else {
            // Poll slowly so script/texture hot reload is noticed.
            event_loop.set_control_flow(ControlFlow::WaitUntil(Instant::now() + Duration::from_millis(500)));
        }
        Ok(())
    }

    fn shutdown(&mut self) {
        self.editor.stop();
        if let Some(mut gpu) = self.gpu.take() {
            gpu.gfx.wait_idle();
            gpu.renderer.destroy(&gpu.gfx);
            gpu.viewport.destroy(&gpu.gfx);
            gpu.preview.destroy(&gpu.gfx);
            drop(gpu.egui_renderer);
            gpu.presenter.destroy(&gpu.gfx);
            drop(gpu.gfx);
        }
    }
}

impl ApplicationHandler for EditorApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.gpu.is_none() {
            if let Err(e) = self.init(event_loop) {
                self.error = Some(e);
                event_loop.exit();
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let Some(gpu) = self.gpu.as_mut() else { return };
        // The running game gets input only while the viewport has focus.
        if self.editor.playing && self.ui.viewport_focused {
            kit::engine::winit_input::handle_event(&self.editor.app.world, &event);
        }
        let response = gpu.egui_state.on_window_event(&gpu.window, &event);
        if response.repaint {
            gpu.window.request_redraw();
        }
        match event {
            WindowEvent::CloseRequested => {
                if self.editor.scene_dirty() {
                    let r = self.editor.save_scene();
                    self.editor.report(r);
                }
                for i in 0..self.editor.scripts.len() {
                    if self.editor.scripts[i].dirty() {
                        self.editor.save_script(i);
                    }
                }
                self.shutdown();
                event_loop.exit();
            }
            WindowEvent::Resized(size) => {
                gpu.presenter.resize(size.width, size.height);
                gpu.window.request_redraw();
            }
            WindowEvent::RedrawRequested => {
                if let Err(e) = self.frame(event_loop) {
                    self.error = Some(e);
                    self.shutdown();
                    event_loop.exit();
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(gpu) = &self.gpu {
            if self.editor.playing || self.editor.camera.auto_rotate || self.editor.camera.animating() {
                gpu.window.request_redraw();
            }
        }
    }

    fn new_events(&mut self, _event_loop: &ActiveEventLoop, cause: winit::event::StartCause) {
        if matches!(cause, winit::event::StartCause::ResumeTimeReached { .. }) {
            if let Some(gpu) = &self.gpu {
                gpu.window.request_redraw();
            }
        }
    }

    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        self.shutdown();
    }
}

fn main() {
    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("info,naga=warn,egui_ash_renderer=warn"),
    )
    .init();
    let mut project = None;
    let mut auto = Automation::default();
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "-h" | "--help" => {
                println!("kit-editor {}\nusage: kit-editor [project-folder]", kit::VERSION);
                return;
            }
            "-V" | "--version" => {
                println!("kit-editor {}", kit::VERSION);
                return;
            }
            "--screenshot" => auto.screenshot = args.next().map(PathBuf::from),
            "--frames" => auto.frames = args.next().and_then(|f| f.parse().ok()).unwrap_or(0),
            "--select" => auto.select = args.next(),
            "--script" => auto.script = args.next(),
            "--play" => auto.play = true,
            "--canvas" => auto.canvas = true,
            "--diagram" => auto.diagram = true,
            "--rex" => auto.rex = true,
            "--rig" => auto.rig = args.next(),
            "--rename" => auto.rename = args.next(),
            "--rex-ask" => auto.rex_ask = args.next(),
            "--rex-mode" => auto.rex_mode = args.next(),
            "--settings" => auto.settings = true,
            "--wizard" => auto.wizard = true,
            "--new-script" => auto.new_script = true,
            "--terrain" => auto.terrain = true,
            "--light" => auto.light = Some(true),
            "--dark" => auto.light = Some(false),
            "--export" => auto.export = args.next().map(PathBuf::from),
            "--platform" => auto.platform = args.next(),
            "--protect" => auto.protect = args.next(),
            "--key" => auto.key = args.next().map(PathBuf::from),
            "--new-key" => auto.new_key = args.next().map(PathBuf::from),
            other => project = Some(PathBuf::from(other)),
        }
    }
    if let Some(path) = auto.new_key.clone() {
        // Creates a publisher signing key; prints only its public fingerprint.
        let result = kit_export_security::PublisherKey::generate().and_then(|k| k.save(&path).map(|()| k));
        match result {
            Ok(k) => println!("Created publisher key {} ({})", path.display(), k.fingerprint()),
            Err(e) => {
                eprintln!("kit-editor: {e:#}");
                std::process::exit(1);
            }
        }
        return;
    }
    if let Some(dest) = auto.export.clone() {
        let result =
            export_headless(project, &dest, auto.platform.as_deref(), auto.protect.as_deref(), auto.key.clone());
        match result {
            Ok(out) => println!("Exported to {}", out.display()),
            Err(e) => {
                eprintln!("kit-editor: {e:#}");
                std::process::exit(1);
            }
        }
        return;
    }
    if let Err(e) = run(project, auto) {
        eprintln!("kit-editor: {e:#}");
        log::error!("{e:#}");
        std::process::exit(1);
    }
}

/// `kit-editor <project> --export <folder> [--platform linux|windows|macos]`: export without a window.
fn export_headless(
    project: Option<PathBuf>,
    dest: &Path,
    platform: Option<&str>,
    protect: Option<&str>,
    key: Option<PathBuf>,
) -> Result<PathBuf> {
    let project = project.context("pass the project folder to export")?;
    let id = platform.unwrap_or(export::host());
    let platform = export::platform(id).with_context(|| format!("unknown platform `{id}`"))?;
    let protection = match protect {
        None => export::Protection::Standard,
        Some(p) => export::Protection::parse(p)
            .with_context(|| format!("unknown protection `{p}` (use standard, signed or encrypted)"))?,
    };
    if let Err(why) = export::protection_supported(&platform, protection) {
        anyhow::bail!("{why}");
    }
    let mut editor = Editor::new();
    editor.open_project(&project)?;
    let mut job = editor.export_job(&platform, dest)?;
    job.protection = protection;
    job.key_file = key;
    if job.needs_build() {
        eprintln!("The {} player doesn't match this editor; rebuilding it first…", platform.name);
    }
    job.run()
}

fn run(project: Option<PathBuf>, auto: Automation) -> Result<()> {
    let egui_ctx = egui::Context::default();
    theme::apply(&egui_ctx);
    let saved = config_dir().and_then(|d| std::fs::read_to_string(d.join("theme.txt")).ok());
    let light = auto.light.unwrap_or(saved.as_deref().map(str::trim) == Some("light"));
    theme::set_light(&egui_ctx, light);
    let mut editor = Editor::new();
    editor.controls = controls::Controls::load();
    let mut app = EditorApp {
        auto,
        editor,
        ui: UiState::new(load_recent(), String::new()),
        egui_ctx,
        gpu: None,
        last: Instant::now(),
        fps_acc: (0.0, 0),
        startup_project: project,
        error: None,
        title: String::new(),
    };
    let mut builder = EventLoop::builder();
    #[cfg(target_os = "linux")]
    {
        use winit::platform::x11::EventLoopBuilderExtX11 as _;
        // Winit's X11 backend delivers file drops. Its Wayland backend uses the file picker.
        if x11rb::connect(None).is_ok() {
            builder.with_x11();
        }
    }
    let event_loop = builder.build().context("creating event loop")?;
    event_loop.run_app(&mut app)?;
    match app.error {
        Some(e) => Err(e),
        None => Ok(()),
    }
}
