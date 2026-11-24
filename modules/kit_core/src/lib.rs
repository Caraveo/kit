//! # kit_core
//!
//! The backbone every Kit module plugs into, like Defold's engine core:
//!
//! * [`Module`] — a unit of engine functionality (rendering, scripting, input...).
//!   Each module registers components, resources and systems in [`Module::build`].
//! * [`App`] — owns the [`World`] and the [`Schedule`], and keeps track of loaded modules.
//! * [`Stage`] — fixed update order for systems (input → scripts → transforms ...).
//! * [`Time`] and [`Console`] — resources available to every module.

pub use kit_ecs;
/// Identifies the engine source this binary was built from. The editor only
/// exports with a game player whose build ID matches its own.
pub const BUILD_ID: &str = env!("KIT_BUILD_ID");
/// `BUILD_ID` as it appears inside a binary (players embed it, and the editor
/// finds it there without running them).
pub const BUILD_MARKER: &str = concat!("kit-build-id:", env!("KIT_BUILD_ID"));

use kit_ecs::World;

/// Ordered stages systems run in each frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Stage {
    /// Runs once, before the first frame.
    Startup,
    /// Engine bookkeeping at frame start (input edge detection...).
    First,
    /// Before gameplay logic.
    PreUpdate,
    /// Gameplay logic: scripts, game systems.
    Update,
    /// After gameplay: transform propagation, bookkeeping for rendering.
    PostUpdate,
    /// End of frame.
    Last,
}

impl Stage {
    pub const FRAME: [Stage; 5] = [Stage::First, Stage::PreUpdate, Stage::Update, Stage::PostUpdate, Stage::Last];
}

type SystemFn = Box<dyn FnMut(&mut World)>;

struct System {
    name: &'static str,
    run: SystemFn,
    /// Systems that only run in play mode are skipped while the editor is editing.
    play_only: bool,
}

/// Systems grouped by [`Stage`], run in insertion order within each stage.
#[derive(Default)]
pub struct Schedule {
    stages: Vec<(Stage, Vec<System>)>,
}

impl Schedule {
    fn stage_mut(&mut self, stage: Stage) -> &mut Vec<System> {
        if let Some(i) = self.stages.iter().position(|(s, _)| *s == stage) {
            return &mut self.stages[i].1;
        }
        self.stages.push((stage, Vec::new()));
        self.stages.sort_by_key(|(s, _)| *s);
        let i = self.stages.iter().position(|(s, _)| *s == stage).unwrap();
        &mut self.stages[i].1
    }

    pub fn run_stage(&mut self, stage: Stage, world: &mut World, playing: bool) {
        if let Some((_, systems)) = self.stages.iter_mut().find(|(s, _)| *s == stage) {
            for sys in systems.iter_mut() {
                if sys.play_only && !playing {
                    continue;
                }
                (sys.run)(world);
            }
        }
    }

    /// Names of all systems, in run order (for diagnostics / the editor).
    pub fn system_names(&self) -> Vec<(Stage, &'static str)> {
        self.stages.iter().flat_map(|(st, v)| v.iter().map(move |s| (*st, s.name))).collect()
    }
}

/// A pluggable piece of engine functionality.
///
/// ```
/// use kit_core::{App, Module, Stage};
///
/// struct Spin;
/// impl Module for Spin {
///     fn name(&self) -> &'static str { "spin" }
///     fn build(&self, app: &mut App) {
///         app.add_system(Stage::Update, "spin", |_world| { /* ... */ });
///     }
/// }
///
/// let mut app = App::new();
/// app.add_module(Spin);
/// assert!(app.has_module("spin"));
/// ```
pub trait Module: 'static {
    /// Unique module name, e.g. `"render"`.
    fn name(&self) -> &'static str;
    /// Modules that must be added before this one.
    fn dependencies(&self) -> &'static [&'static str] {
        &[]
    }
    /// Register resources and systems.
    fn build(&self, app: &mut App);
}

/// Whether the simulation is running (play mode) or paused for editing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunMode {
    /// Gameplay systems (scripts...) run.
    Play,
    /// Only engine systems run (transform propagation...), used by the editor.
    Edit,
}

/// The application: a world, a schedule, and the modules that configured them.
pub struct App {
    pub world: World,
    pub schedule: Schedule,
    modules: Vec<&'static str>,
    startup_done: bool,
    pub mode: RunMode,
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

impl App {
    pub fn new() -> Self {
        let mut world = World::new();
        world.insert_resource(Time::default());
        world.insert_resource(Console::default());
        Self { world, schedule: Schedule::default(), modules: Vec::new(), startup_done: false, mode: RunMode::Play }
    }

    /// Add a module. Adding the same module twice is a no-op.
    pub fn add_module<M: Module>(&mut self, module: M) -> &mut Self {
        let name = module.name();
        if self.has_module(name) {
            return self;
        }
        for dep in module.dependencies() {
            assert!(self.has_module(dep), "module `{name}` requires module `{dep}` to be added first");
        }
        log::debug!("module + {name}");
        self.modules.push(name);
        module.build(self);
        self
    }

    pub fn has_module(&self, name: &str) -> bool {
        self.modules.contains(&name)
    }

    pub fn modules(&self) -> &[&'static str] {
        &self.modules
    }

    /// Add a system that always runs (play and edit mode).
    pub fn add_system(&mut self, stage: Stage, name: &'static str, f: impl FnMut(&mut World) + 'static) -> &mut Self {
        self.schedule.stage_mut(stage).push(System { name, run: Box::new(f), play_only: false });
        self
    }

    /// Add a system that only runs in [`RunMode::Play`].
    pub fn add_play_system(
        &mut self,
        stage: Stage,
        name: &'static str,
        f: impl FnMut(&mut World) + 'static,
    ) -> &mut Self {
        self.schedule.stage_mut(stage).push(System { name, run: Box::new(f), play_only: true });
        self
    }

    /// Run startup systems again on the next update (used when play mode restarts).
    pub fn reset_startup(&mut self) {
        self.startup_done = false;
    }

    /// Advance one frame by `dt` seconds.
    pub fn update(&mut self, dt: f32) {
        let playing = self.mode == RunMode::Play;
        {
            let mut time = self.world.resource_mut::<Time>();
            time.advance(dt, playing);
        }
        if !self.startup_done {
            self.startup_done = true;
            self.schedule.run_stage(Stage::Startup, &mut self.world, playing);
        }
        for stage in Stage::FRAME {
            self.schedule.run_stage(stage, &mut self.world, playing);
        }
    }
}

/// Frame timing.
#[derive(Clone, Debug)]
pub struct Time {
    /// Seconds since last frame (scaled; zero while not playing).
    pub delta: f32,
    /// Seconds of play time since start.
    pub elapsed: f64,
    /// Real (unscaled) seconds since last frame.
    pub real_delta: f32,
    pub frame: u64,
    pub scale: f32,
}

impl Default for Time {
    fn default() -> Self {
        Self { delta: 0.0, elapsed: 0.0, real_delta: 0.0, frame: 0, scale: 1.0 }
    }
}

impl Time {
    fn advance(&mut self, dt: f32, playing: bool) {
        // Clamp huge hitches (debugger breaks, window drags) to keep simulation sane.
        let dt = dt.clamp(0.0, 0.25);
        self.real_delta = dt;
        self.delta = if playing { dt * self.scale } else { 0.0 };
        self.elapsed += self.delta as f64;
        self.frame += 1;
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LogLevel {
    Info,
    Warn,
    Error,
}

#[derive(Clone, Debug)]
pub struct ConsoleLine {
    pub level: LogLevel,
    /// Where the message came from: a module name or a script path.
    pub source: String,
    pub text: String,
}

/// In-engine log shown by the editor console (script `DISPLAY` output, errors...).
#[derive(Default)]
pub struct Console {
    lines: Vec<ConsoleLine>,
    /// Bumped on every push so UIs can auto-scroll.
    pub revision: u64,
    /// Errors pushed so far (never reset; for "new errors" badges).
    pub errors_total: u64,
}

impl Console {
    pub const MAX_LINES: usize = 1000;

    pub fn push(&mut self, level: LogLevel, source: impl Into<String>, text: impl Into<String>) {
        let line = ConsoleLine { level, source: source.into(), text: text.into() };
        match level {
            LogLevel::Info => log::info!("[{}] {}", line.source, line.text),
            LogLevel::Warn => log::warn!("[{}] {}", line.source, line.text),
            LogLevel::Error => log::error!("[{}] {}", line.source, line.text),
        }
        if level == LogLevel::Error {
            self.errors_total += 1;
        }
        self.lines.push(line);
        if self.lines.len() > Self::MAX_LINES {
            let extra = self.lines.len() - Self::MAX_LINES;
            self.lines.drain(..extra);
        }
        self.revision += 1;
    }

    pub fn info(&mut self, source: impl Into<String>, text: impl Into<String>) {
        self.push(LogLevel::Info, source, text);
    }
    pub fn warn(&mut self, source: impl Into<String>, text: impl Into<String>) {
        self.push(LogLevel::Warn, source, text);
    }
    pub fn error(&mut self, source: impl Into<String>, text: impl Into<String>) {
        self.push(LogLevel::Error, source, text);
    }

    pub fn lines(&self) -> &[ConsoleLine] {
        &self.lines
    }

    pub fn clear(&mut self) {
        self.lines.clear();
        self.revision += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    struct Recorder(Rc<RefCell<Vec<&'static str>>>);
    impl Module for Recorder {
        fn name(&self) -> &'static str {
            "recorder"
        }
        fn build(&self, app: &mut App) {
            for (stage, label) in [
                (Stage::Update, "update"),
                (Stage::Startup, "startup"),
                (Stage::First, "first"),
                (Stage::PostUpdate, "post"),
            ] {
                let log = self.0.clone();
                app.add_system(stage, label, move |_| log.borrow_mut().push(label));
            }
            let log = self.0.clone();
            app.add_play_system(Stage::Update, "play", move |_| log.borrow_mut().push("play"));
        }
    }

    #[test]
    fn stage_order_and_play_mode() {
        let log = Rc::new(RefCell::new(Vec::new()));
        let mut app = App::new();
        app.add_module(Recorder(log.clone()));
        app.add_module(Recorder(log.clone())); // duplicate ignored
        app.update(0.016);
        assert_eq!(*log.borrow(), ["startup", "first", "update", "play", "post"]);
        log.borrow_mut().clear();
        app.mode = RunMode::Edit;
        app.update(0.016);
        assert_eq!(*log.borrow(), ["first", "update", "post"]);
        assert_eq!(app.world.resource::<Time>().delta, 0.0);
    }

    #[test]
    #[should_panic(expected = "requires module")]
    fn missing_dependency_panics() {
        struct Needy;
        impl Module for Needy {
            fn name(&self) -> &'static str {
                "needy"
            }
            fn dependencies(&self) -> &'static [&'static str] {
                &["nope"]
            }
            fn build(&self, _: &mut App) {}
        }
        App::new().add_module(Needy);
    }
}
