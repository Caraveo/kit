//! Stamp a build ID from the engine's source so the editor can tell whether a
//! game player (desktop `kit-player` or the web player) was built from the same
//! code as itself (see the editor's export).

use std::hash::{Hash, Hasher};
use std::path::Path;

fn hash_dir(dir: &Path, h: &mut impl Hasher) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let mut paths: Vec<_> = entries.flatten().map(|e| e.path()).collect();
    paths.sort();
    for p in paths {
        if p.is_dir() {
            hash_dir(&p, h);
        } else if p.extension().is_some_and(|e| e == "rs" || e == "wgsl" || e == "toml") {
            p.file_name().hash(h);
            std::fs::read(&p).unwrap_or_default().hash(h);
        }
    }
}

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for dir in ["modules", "kit/src", "player/src", "web/player/src"] {
        let dir = root.join(dir);
        hash_dir(&dir, &mut h);
        println!("cargo:rerun-if-changed={}", dir.display());
    }
    println!("cargo:rustc-env=KIT_BUILD_ID={:016x}", h.finish());
    println!("cargo:rerun-if-changed=build.rs");
}
