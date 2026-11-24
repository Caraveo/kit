//! `kit-player [project]` — run a Kit project.
//!
//! Without an argument the player looks for a `game/` folder (or a
//! `kit.project` file) next to its executable, which is how exported games are
//! laid out, and then in the current directory.

#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use kit::resource::{PROJECT_FILE, Project};
use std::path::{Path, PathBuf};

fn find_project(arg: Option<String>) -> Option<PathBuf> {
    if let Some(a) = arg {
        return Some(PathBuf::from(a));
    }
    let mut candidates = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join("game"));
            candidates.push(dir.to_path_buf());
            // macOS app bundle: Contents/MacOS/kit-player -> Contents/Resources/game
            candidates.push(dir.join("../Resources/game"));
        }
    }
    if let Ok(cwd) = std::env::current_dir() {
        candidates.push(cwd);
    }
    candidates.into_iter().find(|d| d.join(PROJECT_FILE).is_file())
}

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let arg = std::env::args().nth(1);
    if matches!(arg.as_deref(), Some("-h" | "--help")) {
        println!("kit-player {}\nusage: kit-player [project-folder]", kit::VERSION);
        return;
    }
    if arg.as_deref() == Some("--build-id") {
        // Print the marker (keeps it in the binary for the editor's export check).
        println!("{}", kit::BUILD_MARKER.trim_start_matches("kit-build-id:"));
        return;
    }
    if matches!(arg.as_deref(), Some("-V" | "--version")) {
        println!("kit-player {}", kit::VERSION);
        return;
    }
    let Some(path) = find_project(arg) else {
        eprintln!("kit-player: no project found. Pass a folder containing {PROJECT_FILE}.");
        std::process::exit(2);
    };
    let result = Project::open(Path::new(&path)).and_then(kit::engine::run_project);
    if let Err(e) = result {
        log::error!("{e:#}");
        eprintln!("kit-player: {e:#}");
        std::process::exit(1);
    }
}
