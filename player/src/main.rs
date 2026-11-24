//! `kit-player [project]` — run a Kit project.
//!
//! Without an argument the player looks for a `game/` folder (or a
//! `kit.project` file) next to its executable, which is how exported games are
//! laid out, and then in the current directory.

#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use kit::resource::{PROJECT_FILE, Project};
use kit_export_security::{SEALED_PACK, TRUST_SLOT_LEN, Trust};
use std::path::{Path, PathBuf};

/// The launcher's pinned trust (publisher key, required mode, content key).
/// Zeros in an unprotected player; the editor's export writes it into the
/// copy it ships as a protected game. See `kit_export_security`.
#[used]
static TRUST_SLOT: [u8; TRUST_SLOT_LEN] = kit_export_security::empty_trust_slot();

fn pinned_trust() -> Option<Trust> {
    // Volatile reads: the compiler must not assume the shipped zeros.
    let mut slot = [0u8; TRUST_SLOT_LEN];
    for (i, b) in slot.iter_mut().enumerate() {
        *b = unsafe { std::ptr::read_volatile(TRUST_SLOT.as_ptr().add(i)) };
    }
    kit_export_security::read_trust_slot(&slot)
}

/// A protected game: verify the sealed pack next to the launcher (signature
/// first, then decrypt), and load it into memory. Never falls back to loose
/// files, a plain pack, or a folder given on the command line.
fn open_protected(trust: &Trust) -> anyhow::Result<Project> {
    let exe = std::env::current_exe()?;
    let dir = exe.parent().ok_or_else(|| anyhow::anyhow!("the game has no folder"))?;
    let path = [dir.join(SEALED_PACK), dir.join("../Resources").join(SEALED_PACK)].into_iter().find(|p| p.is_file());
    let sealed = match path {
        Some(p) => std::fs::read(p)?,
        None => return Err(kit_export_security::Rejected::Missing.into()),
    };
    let pack = kit_export_security::open(&sealed, &trust.publisher, trust.mode, trust.content_key.as_ref())?;
    Project::from_files(kit_resource::pack::read(&pack)?)
}

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
    let trust = pinned_trust();
    if arg.as_deref() == Some("--protection") {
        println!("{}", trust.map_or("standard", |t| t.mode.id()));
        return;
    }
    // kit-player <project> --screenshot out.png [--frames N] [--size WxH]
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Some(i) = args.iter().position(|a| a == "--screenshot") {
        let out = args.get(i + 1).cloned().unwrap_or_else(|| "screenshot.png".into());
        let value = |flag: &str| args.iter().position(|a| a == flag).and_then(|j| args.get(j + 1)).cloned();
        let frames = value("--frames").and_then(|f| f.parse().ok()).unwrap_or(30);
        let (w, h) = value("--size")
            .and_then(|s| s.split_once('x').and_then(|(w, h)| Some((w.parse().ok()?, h.parse().ok()?))))
            .unwrap_or((1280, 720));
        let project = args.first().filter(|a| !a.starts_with("--")).cloned();
        let opened = match &trust {
            Some(t) => open_protected(t),
            None => find_project(project)
                .ok_or_else(|| anyhow::anyhow!("no project found"))
                .and_then(|p| Project::open(Path::new(&p))),
        };
        let result = opened.and_then(|p| kit::engine::screenshot_project(p, Path::new(&out), frames, w, h));
        match result {
            Ok(()) => println!("screenshot written to {out}"),
            Err(e) => {
                eprintln!("kit-player: {e:#}");
                std::process::exit(1);
            }
        }
        return;
    }
    if let Some(trust) = trust {
        let result = open_protected(&trust).and_then(kit::engine::run_project);
        if let Err(e) = result {
            log::error!("{e:#}");
            eprintln!("This game can't start: {e:#}");
            std::process::exit(3);
        }
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
