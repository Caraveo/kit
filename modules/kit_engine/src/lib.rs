//! # kit_engine
//!
//! The windowed runtime (Defold's `engine`): opens a window, runs the [`App`]
//! every frame, feeds input, and renders the active camera with `kit_render`.
//!
//! ```no_run
//! let project = kit_resource::Project::open(std::path::Path::new("my_game"))?;
//! kit_engine::run_project(project)?;
//! # Ok::<(), anyhow::Error>(())
//! ```

pub mod winit_input;

use anyhow::{Context, Result};
use kit_core::{App, Stage};
use kit_gamesys::SceneFile;
use kit_graphics::{Gfx, Presenter};
use kit_render::{Renderer, Target, View};
use kit_resource::{Project, Resources};
use raw_window_handle::{HasDisplayHandle, HasWindowHandle};
use std::sync::Arc;
use std::time::Instant;
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Fullscreen, Window, WindowId};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Window settings for [`run`].
#[derive(Clone, Debug)]
pub struct EngineConfig {
    pub title: String,
    pub width: u32,
    pub height: u32,
    pub vsync: bool,
    pub fullscreen: bool,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self { title: "Kit".into(), width: 1280, height: 720, vsync: true, fullscreen: false }
    }
}

/// Add every standard Kit module to `app`.
pub fn add_default_modules(app: &mut App, project_root: std::path::PathBuf, config: &kit_resource::ProjectConfig) {
    let physics = &config.physics;
    app.add_module(kit_resource::ResourceModule { root: project_root })
        .add_module(kit_scene::SceneModule)
        .add_module(kit_input::InputModule)
        .add_module(kit_gamesys::GameSysModule)
        .add_module(kit_script::ScriptModule)
        .add_module(kit_xui::XuiModule)
        .add_module(kit_path::PathModule)
        // Game data, then visual graphs (after scripts, before physics).
        .add_module(kit_gamedata::GameDataModule)
        .add_module(kit_diagram::DiagramModule)
        // Volume rigs: after scripts/diagrams pose them, before drawing.
        .add_module(kit_rigg::RiggModule)
        // After scripts and paths, so physics sees this frame's changes.
        .add_module(kit_collider::ColliderModule)
        .add_module(kit_physics::PhysicsModule {
            gravity: glam::Vec3::from_array(physics.gravity),
            rate: physics.rate,
            enabled: physics.enabled,
        })
        .add_module(kit_audio::AudioModule { device: true })
        // After scripts and physics, so emitters follow this frame's moves.
        .add_module(kit_particles::ParticlesModule)
        // Circuits with engine-side runtimes. Each does nothing unless the
        // project has that Circuit installed and enabled (circuits.lock).
        .add_module(kit_multiplayer::MultiplayerModule)
        .add_module(kit_cinematics::CinematicsModule);
}

/// Build an app for a project and load its main scene.
pub fn project_app(project: &Project) -> Result<App> {
    let mut app = App::new();
    add_default_modules(&mut app, project.root().to_path_buf(), &project.config);
    // The project's own resources: a folder, or (protected exports) memory.
    app.world.insert_resource(project.resources.clone());
    kit_gamedata::reload(&mut app.world);
    let main = &project.config.project.main_scene;
    let text = project.resources.read_text(main).with_context(|| format!("loading main scene `{main}`"))?;
    let scene = SceneFile::from_json(&text).with_context(|| format!("in {main}"))?;
    let spawned = kit_gamesys::load_scene(&mut app.world, &scene)?;
    kit_gamesys::scenes::mark_loaded(&mut app.world, main, spawned);
    Ok(app)
}

/// Open a project and run it in a window until closed.
pub fn run_project(project: Project) -> Result<()> {
    let app = project_app(&project)?;
    let d = &project.config.display;
    run(
        app,
        EngineConfig {
            title: project.config.project.title.clone(),
            width: d.width,
            height: d.height,
            vsync: d.vsync,
            fullscreen: d.fullscreen,
        },
    )
}

/// Run a project for `frames` fixed steps (1/60 s) without a window and save
/// what its active camera sees as a PNG (`kit-player --screenshot`). Used to
/// compare the player with the editor and the web build.
pub fn screenshot_project(project: Project, out: &std::path::Path, frames: u32, width: u32, height: u32) -> Result<()> {
    use kit_graphics::{Image, vk};
    let mut app = project_app(&project)?;
    kit_scene::propagate_transforms(&mut app.world);
    app.world.insert_resource(kit_xui::runtime::UiScreen::window([width, height], 1.0));
    for _ in 0..frames {
        app.update(1.0 / 60.0);
    }
    let gfx = Gfx::new(None, &project.config.project.title)?;
    let mut renderer = Renderer::new(&gfx)?;
    renderer.prepare(&gfx, &app.world);
    let extent = vk::Extent2D { width, height };
    let format = vk::Format::R8G8B8A8_SRGB;
    let mut image = Image::new(
        &gfx,
        extent,
        format,
        vk::ImageUsageFlags::COLOR_ATTACHMENT | vk::ImageUsageFlags::TRANSFER_SRC,
        vk::SampleCountFlags::TYPE_1,
        "screenshot",
    )?;
    let aspect = width as f32 / height.max(1) as f32;
    let mut view = View::from_camera(&app.world, aspect).unwrap_or_else(|| View::fallback(aspect));
    view.time = app.world.resource::<kit_core::Time>().elapsed as f32;
    let target = Target { image: image.image, view: image.view, extent, format };
    let mut result = Ok(());
    gfx.one_shot(|cmd| {
        gfx.image_barrier(
            cmd,
            image.image,
            vk::ImageAspectFlags::COLOR,
            vk::ImageLayout::UNDEFINED,
            vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
            (vk::PipelineStageFlags::TOP_OF_PIPE, vk::AccessFlags::empty()),
            (vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT, vk::AccessFlags::COLOR_ATTACHMENT_WRITE),
        );
        result = renderer
            .render(&gfx, cmd, 0, &target, &view, &app.world)
            .and_then(|_| renderer.draw_ui(&gfx, cmd, 0, &target, &app.world));
    })?;
    result?;
    let pixels = image.read_back(&gfx, vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)?;
    image.destroy(&gfx);
    renderer.destroy(&gfx);
    kit_script::stop_scripts(&mut app.world);
    kit_audio::reset(&mut app.world);
    image::save_buffer(out, &pixels, width, height, image::ExtendedColorType::Rgba8)
        .context("saving the screenshot")?;
    Ok(())
}

struct Gpu {
    window: Arc<Window>,
    renderer: Renderer,
    presenter: Presenter,
    gfx: Gfx,
}

struct Runner {
    app: App,
    config: EngineConfig,
    gpu: Option<Gpu>,
    last: Instant,
    error: Option<anyhow::Error>,
    pointer: winit_input::Pointer,
}

impl Runner {
    fn init(&mut self, event_loop: &ActiveEventLoop) -> Result<()> {
        let mut attrs = Window::default_attributes()
            .with_title(&self.config.title)
            .with_inner_size(LogicalSize::new(self.config.width, self.config.height));
        if self.config.fullscreen {
            attrs = attrs.with_fullscreen(Some(Fullscreen::Borderless(None)));
        }
        let window = Arc::new(event_loop.create_window(attrs)?);
        let display = window.display_handle()?.as_raw();
        let gfx = Gfx::new(Some(display), &self.config.title)?;
        let surface = gfx.create_surface(display, window.window_handle()?.as_raw())?;
        let size = window.inner_size();
        let presenter = Presenter::new(&gfx, surface, size.width, size.height, self.config.vsync)?;
        let renderer = Renderer::new(&gfx)?;
        self.gpu = Some(Gpu { window, renderer, presenter, gfx });
        self.last = Instant::now();
        Ok(())
    }

    fn frame(&mut self) -> Result<()> {
        let now = Instant::now();
        let dt = (now - self.last).as_secs_f32();
        self.last = now;
        if let Some(gpu) = &self.gpu {
            let size = gpu.window.inner_size();
            let screen = kit_xui::runtime::UiScreen::window([size.width, size.height], gpu.window.scale_factor() as f32);
            self.app.world.insert_resource(screen);
        }
        self.app.update(dt);

        let Some(gpu) = self.gpu.as_mut() else { return Ok(()) };
        gpu.renderer.prepare(&gpu.gfx, &self.app.world);
        let Some(frame) = gpu.presenter.begin_frame(&gpu.gfx)? else { return Ok(()) };
        let aspect = frame.extent.width as f32 / frame.extent.height.max(1) as f32;
        let mut view = View::from_camera(&self.app.world, aspect).unwrap_or_else(|| View::fallback(aspect));
        view.time = self.app.world.resource::<kit_core::Time>().elapsed as f32;
        let target = Target { image: frame.image, view: frame.view, extent: frame.extent, format: frame.format };
        gpu.renderer.render(&gpu.gfx, frame.cmd, frame.frame_index, &target, &view, &self.app.world)?;
        gpu.renderer.draw_ui(&gpu.gfx, frame.cmd, frame.frame_index, &target, &self.app.world)?;
        gpu.presenter.end_frame(&gpu.gfx, frame)?;
        Ok(())
    }

    fn shutdown(&mut self) {
        kit_script::stop_scripts(&mut self.app.world);
        kit_collider::reset(&mut self.app.world);
        kit_path::reset(&mut self.app.world);
        kit_audio::reset(&mut self.app.world);
        if let Some(mut gpu) = self.gpu.take() {
            gpu.gfx.wait_idle();
            gpu.renderer.destroy(&gpu.gfx);
            gpu.presenter.destroy(&gpu.gfx);
            drop(gpu.gfx);
        }
    }
}

impl ApplicationHandler for Runner {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.gpu.is_none() {
            if let Err(e) = self.init(event_loop) {
                self.error = Some(e);
                event_loop.exit();
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        winit_input::handle_event(&self.app.world, &event);
        match event {
            WindowEvent::CloseRequested => {
                self.shutdown();
                event_loop.exit();
            }
            WindowEvent::Resized(size) => {
                if let Some(gpu) = self.gpu.as_mut() {
                    gpu.presenter.resize(size.width, size.height);
                }
            }
            WindowEvent::KeyboardInput { event: ref k, .. }
                if k.state.is_pressed()
                    && k.physical_key == winit::keyboard::PhysicalKey::Code(winit::keyboard::KeyCode::F11) =>
            {
                if let Some(gpu) = &self.gpu {
                    let fs = gpu.window.fullscreen().is_some();
                    gpu.window.set_fullscreen(if fs { None } else { Some(Fullscreen::Borderless(None)) });
                }
            }
            WindowEvent::RedrawRequested => {
                if let Err(e) = self.frame() {
                    self.error = Some(e);
                    self.shutdown();
                    event_loop.exit();
                }
            }
            _ => {}
        }
    }

    fn device_event(&mut self, _event_loop: &ActiveEventLoop, _id: winit::event::DeviceId, event: winit::event::DeviceEvent) {
        winit_input::handle_device_event(&self.app.world, &event);
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(gpu) = &self.gpu {
            let size = gpu.window.inner_size();
            winit_input::set_view(&self.app.world, glam::Vec2::ZERO, glam::Vec2::new(size.width as f32, size.height as f32));
            let focused = gpu.window.has_focus();
            winit_input::apply_pointer(&gpu.window, &self.app.world, focused, &mut self.pointer);
            gpu.window.request_redraw();
        }
    }

    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        self.shutdown();
    }
}

/// Run `app` in a window until it is closed.
pub fn run(mut app: App, config: EngineConfig) -> Result<()> {
    // Make sure transforms are current before the first frame.
    app.add_system(Stage::Startup, "initial_transforms", kit_scene::propagate_transforms);
    if !app.world.has_resource::<Resources>() {
        app.world.insert_resource(Resources::new(std::env::current_dir()?));
    }
    let event_loop = EventLoop::new().context("creating event loop")?;
    event_loop.set_control_flow(ControlFlow::Poll);
    let mut runner = Runner { app, config, gpu: None, last: Instant::now(), error: None, pointer: Default::default() };
    event_loop.run_app(&mut runner)?;
    match runner.error {
        Some(e) => Err(e),
        None => Ok(()),
    }
}
