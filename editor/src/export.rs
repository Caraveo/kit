//! Exporting games: the target platforms, where each one's `kit-player` comes
//! from, and writing the game folder (or macOS app bundle).
//!
//! An exported game is a `kit-player` binary for the target platform plus the
//! project files. The player must be built from the same engine code as this
//! editor, which is checked through the build ID stamped into both binaries
//! (`kit::BUILD_MARKER`); reading it from the file works for any platform.

use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};

pub use crate::hooks::{Access, Group, Platform};

/// Every export platform: one per hook (`hooks/<id>`).
pub fn platforms() -> &'static [Platform] {
    crate::hooks::all()
}

pub fn platform(id: &str) -> Option<Platform> {
    // Older names still work on the command line.
    let id = match id {
        "macos" => "mac",
        "html5" => "web",
        other => other,
    };
    platforms().iter().copied().find(|p| p.id == id)
}

/// The desktop platform this editor runs on.
pub fn host() -> &'static str {
    if cfg!(windows) {
        "windows"
    } else if cfg!(target_os = "macos") {
        "mac"
    } else {
        "linux"
    }
}

/// Can this editor export to a platform, and how?
#[derive(Clone, Debug)]
pub enum Availability {
    /// A matching player is ready.
    Ready { player: PathBuf },
    /// The player is missing or stale, but cargo can rebuild it (running from source).
    Rebuild { player: PathBuf },
    /// Needs a player binary for that platform (text says how to get one).
    NeedsPlayer(String),
    /// Not supported yet / needs something outside Kit (text explains).
    Unavailable(String),
    /// A licensed platform: apply to the platform holder, then install its hook.
    RequestAccess,
    /// An installed hook exports it with its own program.
    Hook(&'static crate::hooks::External),
}

impl Availability {
    pub fn can_export(&self) -> bool {
        matches!(self, Availability::Ready { .. } | Availability::Rebuild { .. } | Availability::Hook(_))
    }
}

fn exe_name(os: &str, stem: &str) -> String {
    if os == "windows" { format!("{stem}.exe") } else { stem.to_owned() }
}

/// Does this binary carry this editor's build ID?
pub fn player_matches(player: &Path) -> bool {
    let Ok(bytes) = std::fs::read(player) else { return false };
    let marker = kit::BUILD_MARKER.as_bytes();
    bytes.windows(marker.len()).any(|w| w == marker)
}

/// Folders to look for a player for `os`, in order.
fn player_candidates(bin_dir: &Path, os: &str) -> Vec<PathBuf> {
    let name = exe_name(os, "kit-player");
    let mut found = Vec::new();
    if os == host() {
        found.push(bin_dir.join(&name));
    }
    // Players for other platforms ship in `players/<os>/` next to the editor
    // (or in the macOS bundle's Resources).
    found.push(bin_dir.join("players").join(os).join(&name));
    found.push(bin_dir.join("../Resources/players").join(os).join(&name));
    found.push(bin_dir.join("../share/kit/players").join(os).join(&name));
    found
}

pub fn availability(platform: &Platform, bin_dir: &Path) -> Availability {
    if let Some(external) = platform.export {
        return Availability::Hook(external);
    }
    match platform.access {
        Access::Request => return Availability::RequestAccess,
        Access::Planned => return Availability::Unavailable(platform.summary.to_owned()),
        Access::Open => {}
    }
    match platform.id {
        "linux" | "windows" | "mac" => {
            let candidates = player_candidates(bin_dir, platform.id);
            if let Some(player) = candidates.iter().find(|p| player_matches(p)) {
                return Availability::Ready { player: player.clone() };
            }
            if platform.id == host() && cargo_build(bin_dir, false).is_some() {
                return Availability::Rebuild { player: candidates[0].clone() };
            }
            let stale = candidates.iter().any(|p| p.is_file());
            let folder = bin_dir.join("players").join(platform.id);
            Availability::NeedsPlayer(if stale {
                format!(
                    "The {} player found next to Kit was built from a different Kit version. Replace it with the \
                     kit-player from this Kit release (in {}).",
                    platform.name,
                    folder.display()
                )
            } else if platform.id == host() {
                "kit-player is missing next to the editor. Reinstall Kit.".to_owned()
            } else {
                format!(
                    "Kit needs its {} player to build this. Copy kit-player{} from the Kit {} release into {}, or \
                     export from Kit running on {}.",
                    platform.name,
                    if platform.id == "windows" { ".exe" } else { "" },
                    platform.name,
                    folder.display(),
                    platform.name
                )
            })
        }
        "steam" => Availability::Unavailable(
            "Steam games are regular desktop builds: export for Windows, Linux or macOS, then upload the folder \
             with Steamworks (SteamPipe)."
                .into(),
        ),
        "web" => {
            let candidates = web_player_candidates(bin_dir);
            if let Some(player) = candidates.iter().find(|p| player_matches(p)) {
                return Availability::Ready { player: player.clone() };
            }
            if cargo_build(bin_dir, true).is_some() && wasm_target_installed() {
                if let Some(player) = candidates.last() {
                    return Availability::Rebuild { player: player.clone() };
                }
            }
            let folder = bin_dir.join("players").join("web");
            Availability::NeedsPlayer(if candidates.iter().any(|p| p.is_file()) {
                format!(
                    "The web player (kit_web.wasm) next to Kit was built from a different Kit version. Replace it with \
                     the one from this Kit release (in {}).",
                    folder.display()
                )
            } else if cargo_build(bin_dir, true).is_some() {
                "To build the web player from source, add the WebAssembly target once: \
                 rustup target add wasm32-unknown-unknown"
                    .to_owned()
            } else {
                format!("Kit's web player (kit_web.wasm) is missing from {}. Reinstall Kit.", folder.display())
            })
        }
        "android" | "ios" => {
            Availability::Unavailable("Coming later: mobile builds need touch input and an app package step.".into())
        }
        _ => Availability::Unavailable(format!(
            "{} builds need the platform holder's licensed SDK and developer program, which Kit can't include.",
            platform.name
        )),
    }
}

/// Where the web player (`kit_web.wasm`) may be: shipped next to the editor,
/// or (running from source) cargo's wasm output, which must be last.
fn web_player_candidates(bin_dir: &Path) -> Vec<PathBuf> {
    let mut found = vec![
        bin_dir.join("players/web/kit_web.wasm"),
        bin_dir.join("../Resources/players/web/kit_web.wasm"),
        // Linux packages: /usr/bin/kit-editor + /usr/share/kit/players.
        bin_dir.join("../share/kit/players/web/kit_web.wasm"),
    ];
    if let Some(target) = bin_dir.parent().filter(|t| t.file_name().is_some_and(|n| n == "target")) {
        found.push(target.join("wasm32-unknown-unknown/release/kit_web.wasm"));
    }
    found
}

/// Can cargo build for the web (is Rust's WebAssembly target installed)?
fn wasm_target_installed() -> bool {
    let rustc = std::env::var_os("HOME")
        .map(|h| PathBuf::from(h).join(".cargo/bin/rustc"))
        .filter(|p| p.is_file())
        .unwrap_or_else(|| PathBuf::from("rustc"));
    std::process::Command::new(rustc)
        .args(["--print", "target-libdir", "--target", "wasm32-unknown-unknown"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .is_some_and(|o| Path::new(String::from_utf8_lossy(&o.stdout).trim()).is_dir())
}

/// When the editor runs from a cargo `target/<profile>` folder, the command
/// that rebuilds the host player with the same profile (or the web player).
pub fn cargo_build(bin_dir: &Path, web: bool) -> Option<std::process::Command> {
    let profile = bin_dir.file_name()?.to_str()?.to_owned();
    let target = bin_dir.parent()?;
    let workspace = target.parent()?;
    if target.file_name()? != "target" || !workspace.join("Cargo.toml").is_file() || !workspace.join("player").is_dir()
    {
        return None;
    }
    let home_cargo = std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cargo/bin/cargo"));
    let cargo = std::env::var_os("CARGO")
        .map(PathBuf::from)
        .or_else(|| home_cargo.filter(|p| p.is_file()))
        .unwrap_or_else(|| PathBuf::from("cargo"));
    let mut cmd = std::process::Command::new(cargo);
    if web {
        cmd.current_dir(workspace).args(["build", "-p", "kit_web", "--release", "--target", "wasm32-unknown-unknown"]);
        return Some(cmd);
    }
    cmd.current_dir(workspace).args(["build", "-p", "kit-player"]);
    match profile.as_str() {
        "debug" => {}
        "release" => {
            cmd.arg("--release");
        }
        other => {
            cmd.args(["--profile", other]);
        }
    }
    Some(cmd)
}

/// Rebuild the host player (or the web player) with cargo.
pub fn rebuild_player(bin_dir: &Path, web: bool) -> Result<()> {
    let what = if web { "the web player" } else { "kit-player" };
    let mut cmd = cargo_build(bin_dir, web).context("not running from a Kit source build")?;
    let output = cmd.output().with_context(|| format!("running cargo to build {what}"))?;
    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        let last = err.lines().filter(|l| !l.trim().is_empty()).last().unwrap_or("").to_owned();
        bail!("Building {what} failed: {last}");
    }
    Ok(())
}

/// Everything needed to write one export (runs on a background thread).
pub struct ExportJob {
    pub platform: Platform,
    pub availability: Availability,
    pub bin_dir: PathBuf,
    pub root: PathBuf,
    pub title: String,
    pub dest: PathBuf,
    /// Standard, Signed, or Signed + Encrypted.
    pub protection: Protection,
    /// Publisher key file for protected exports (default: [`default_key_path`]).
    pub key_file: Option<PathBuf>,
}

pub use kit_export_security::Mode as Protection;

/// Kit's settings folder (per user).
pub fn config_dir() -> Option<PathBuf> {
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

/// Where exports go by default: `~/KitBuilds/<project name>` (each platform
/// in its own folder inside it).
pub fn default_builds_dir(project_title: &str) -> PathBuf {
    let home =
        std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from).unwrap_or_default();
    // Keep the title readable, minus characters file systems reject.
    let name: String =
        project_title.chars().map(|c| if c.is_control() || r#"/\:*?"<>|"#.contains(c) { '_' } else { c }).collect();
    let name = name.trim().trim_matches('.');
    home.join("KitBuilds").join(if name.is_empty() { "Game" } else { name })
}

/// Where Kit keeps the publisher signing key unless told otherwise: in the
/// user's settings, never in a project or export.
pub fn default_key_path() -> Option<PathBuf> {
    config_dir().map(|d| d.join("keys").join("publisher.key"))
}

/// Can `platform` be exported with `mode`? (Err: why not.)
pub fn protection_supported(platform: &Platform, mode: Protection) -> std::result::Result<(), String> {
    match (mode, platform.id) {
        (Protection::Standard, _) => Ok(()),
        (_, "linux" | "windows" | "mac") => Ok(()),
        (Protection::Signed, "web") => Ok(()),
        (Protection::Encrypted, "web") => Err(
            "Web builds can't be encrypted: every visitor's browser would need the decryption key, so it would only \
             hide files from people who don't look. Use Signed: it still guarantees players get exactly your build."
                .into(),
        ),
        _ => Err(format!("{} builds don't support protected exports yet", platform.name)),
    }
}

/// Load the publisher key for a protected export.
pub fn load_key(path: Option<&Path>) -> Result<kit_export_security::PublisherKey> {
    let path =
        path.map(Path::to_path_buf).or_else(default_key_path).context("no settings folder for the publisher key")?;
    anyhow::ensure!(
        path.is_file(),
        "No publisher key yet. Create one in Export → Protection (Generate key), or pass --key <file>. Expected it at {}",
        path.display()
    );
    kit_export_security::PublisherKey::load(&path)
}

/// Text in the web bootstrap that a protected export replaces with its trust.
const WEB_TRUST_PLACEHOLDER: &str = "const KIT_TRUST = null;";

impl ExportJob {
    pub fn needs_build(&self) -> bool {
        matches!(self.availability, Availability::Rebuild { .. })
    }

    /// File-system safe name for the game.
    pub fn safe_name(&self) -> String {
        let s: String =
            self.title.chars().map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect();
        let s = s.trim_matches('_');
        if s.is_empty() { "Game".into() } else { s.to_owned() }
    }

    pub fn run(&self) -> Result<PathBuf> {
        // Check what can fail before touching any output.
        if let Err(why) = protection_supported(&self.platform, self.protection) {
            bail!("{why}");
        }
        let key = if self.protection == Protection::Standard {
            None
        } else {
            anyhow::ensure!(self.platform.export.is_none(), "Installed hooks only support Standard exports");
            Some(load_key(self.key_file.as_deref())?)
        };
        let player = match &self.availability {
            Availability::Ready { player } => player.clone(),
            Availability::Rebuild { player } => {
                rebuild_player(&self.bin_dir, self.platform.id == "web")?;
                anyhow::ensure!(
                    player_matches(player),
                    "the player still doesn't match the editor after rebuilding it"
                );
                player.clone()
            }
            Availability::Hook(external) => return self.run_hook(external),
            Availability::RequestAccess => bail!(
                "{} needs access from the platform holder. Apply at {}, then install the {} hook into hooks/{}/ next to Kit.",
                self.platform.name,
                self.platform.request_url.unwrap_or("their developer portal"),
                self.platform.name,
                self.platform.id
            ),
            Availability::NeedsPlayer(why) | Availability::Unavailable(why) => bail!("{why}"),
        };
        // Build next to the destination, then swap it in: a failed export
        // leaves the previous one untouched.
        let name = self.safe_name();
        let final_name = if self.platform.id == "mac" { format!("{name}.app") } else { name.clone() };
        let out = self.dest.join(&final_name);
        let staging = self.dest.join(format!(".{final_name}.kit-partial"));
        if staging.exists() {
            std::fs::remove_dir_all(&staging)?;
        }
        std::fs::create_dir_all(&staging)?;
        let skip = [out.clone(), staging.clone()];
        let result = match self.platform.id {
            "mac" => self.write_app(&player, &staging, &skip, key.as_ref()),
            "web" => self.write_web(&player, &staging, &skip, key.as_ref()),
            _ => self.write_folder(&player, &staging, &skip, key.as_ref()),
        };
        if let Err(e) = result {
            let _ = std::fs::remove_dir_all(&staging);
            return Err(e);
        }
        let old = self.dest.join(format!(".{final_name}.kit-old"));
        if old.exists() {
            std::fs::remove_dir_all(&old)?;
        }
        if out.exists() {
            std::fs::rename(&out, &old).with_context(|| format!("replacing {}", out.display()))?;
        }
        if let Err(e) = std::fs::rename(&staging, &out) {
            if old.exists() {
                let _ = std::fs::rename(&old, &out);
            }
            return Err(e).with_context(|| format!("writing {}", out.display()));
        }
        if old.exists() {
            let _ = std::fs::remove_dir_all(&old);
        }
        if self.platform.id == "mac" && key.is_some() {
            resign_mac_app(&out);
        }
        Ok(out)
    }

    /// The project's game pack, sealed for a protected export.
    fn sealed_pack(
        &self,
        skip: &[PathBuf],
        key: &kit_export_security::PublisherKey,
    ) -> Result<(Vec<u8>, Option<[u8; 32]>)> {
        let pack = kit_web::pack::write_dir_skipping(&self.root, skip)?;
        let content_key =
            if self.protection == Protection::Encrypted { Some(kit_export_security::new_content_key()?) } else { None };
        let metadata = format!(
            "title={}\nplatform={}\nkit={}\nbuild={}\n",
            self.title.replace('\n', " "),
            self.platform.id,
            kit::VERSION,
            kit::BUILD_ID
        );
        let sealed = kit_export_security::seal(&pack, key, self.protection, content_key.as_ref(), &metadata)?;
        Ok((sealed, content_key))
    }

    /// The launcher: the player with this export's trust pinned in it.
    fn launcher(
        &self,
        player: &Path,
        key: Option<&kit_export_security::PublisherKey>,
        content_key: Option<[u8; 32]>,
    ) -> Result<Vec<u8>> {
        let mut bytes = std::fs::read(player).with_context(|| format!("reading {}", player.display()))?;
        if let Some(key) = key {
            let trust = kit_export_security::Trust { mode: self.protection, publisher: key.public(), content_key };
            kit_export_security::write_trust_slot(&mut bytes, &trust)?;
        }
        Ok(bytes)
    }

    fn write_exe(path: &Path, bytes: &[u8]) -> Result<()> {
        std::fs::write(path, bytes).with_context(|| format!("writing {}", path.display()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))?;
        }
        Ok(())
    }

    /// Export through an installed hook's program.
    fn run_hook(&self, hook: &crate::hooks::External) -> Result<PathBuf> {
        let out = self.dest.join(self.safe_name());
        std::fs::create_dir_all(&out)?;
        let fill = |arg: &str| {
            arg.replace("{project}", &self.root.to_string_lossy())
                .replace("{out}", &out.to_string_lossy())
                .replace("{title}", &self.title)
                .replace("{build_id}", kit::BUILD_ID)
                .replace("{kit_dir}", &self.bin_dir.to_string_lossy())
        };
        let program = hook.dir.join(&hook.command);
        let output = std::process::Command::new(&program)
            .args(hook.args.iter().map(|a| fill(a)))
            .current_dir(&hook.dir)
            .output()
            .with_context(|| format!("starting the {} hook ({})", self.platform.name, program.display()))?;
        let last =
            |b: &[u8]| String::from_utf8_lossy(b).lines().filter(|l| !l.trim().is_empty()).last().map(str::to_owned);
        if !output.status.success() {
            let why =
                last(&output.stderr).or_else(|| last(&output.stdout)).unwrap_or_else(|| output.status.to_string());
            bail!("The {} hook failed: {why}", self.platform.name);
        }
        if let Some(line) = last(&output.stdout) {
            log::info!("{} hook: {line}", self.platform.name);
        }
        Ok(out)
    }

    /// Web: a static site (`index.html`, `kit.js`, `kit_web.wasm`, `game.pack`)
    /// for any web host, itch.io, or Kit's local preview server.
    fn write_web(
        &self,
        wasm: &Path,
        out: &Path,
        skip: &[PathBuf],
        key: Option<&kit_export_security::PublisherKey>,
    ) -> Result<()> {
        let html = WEB_INDEX.replace("<title>Kit game</title>", &format!("<title>{}</title>", xml_escape(&self.title)));
        std::fs::write(out.join("index.html"), html)?;
        std::fs::write(out.join("kit-logo.png"), include_bytes!("../../packaging/icons/kit-128.png"))?;
        std::fs::write(out.join("KIT-LICENSE.txt"), KIT_LICENSE)?;
        std::fs::copy(wasm, out.join("kit_web.wasm")).with_context(|| format!("copying {}", wasm.display()))?;
        match key {
            None => {
                std::fs::write(out.join("kit.js"), WEB_SCRIPT)?;
                std::fs::write(out.join("game.pack"), kit_web::pack::write_dir_skipping(&self.root, skip)?)?;
            }
            Some(key) => {
                // The bootstrap pins the publisher key and mode; the page only
                // loads the signed pack.
                anyhow::ensure!(WEB_SCRIPT.contains(WEB_TRUST_PLACEHOLDER), "kit.js has no trust placeholder");
                let trust = format!(
                    "const KIT_TRUST = {{ mode: \"{}\", publisher: [{}] }};",
                    self.protection.id(),
                    key.public().iter().map(|b| b.to_string()).collect::<Vec<_>>().join(",")
                );
                std::fs::write(out.join("kit.js"), WEB_SCRIPT.replacen(WEB_TRUST_PLACEHOLDER, &trust, 1))?;
                let (sealed, _) = self.sealed_pack(skip, key)?;
                std::fs::write(out.join(kit_export_security::SEALED_PACK), sealed)?;
            }
        }
        std::fs::write(
            out.join("README.txt"),
            format!(
                "{title}: web build (made with Kit)\n\n\
                 Upload this whole folder to any web host, or zip it and upload it to itch.io as an HTML game.\n\
                 Browsers only run it from a web server (not by double-clicking index.html).\n\
                 To try it locally: python3 -m http.server, then open http://localhost:8000\n\
                 Needs a browser with WebGPU (recent Chrome, Edge, Firefox or Safari).\n",
                title = self.title
            ),
        )?;
        Ok(())
    }

    /// Runtime libraries for the target shipped next to the player.
    fn libraries(&self, player: &Path) -> Vec<PathBuf> {
        let ext: &[&str] = match self.platform.id {
            "windows" => &[".dll"],
            "mac" => &[".dylib", "_icd.json"],
            _ => &[".so"],
        };
        let dirs = [player.parent().map(Path::to_path_buf), player.parent().map(|d| d.join("../Frameworks"))];
        dirs.into_iter()
            .flatten()
            .filter_map(|d| std::fs::read_dir(d).ok())
            .flat_map(|d| d.flatten())
            .map(|e| e.path())
            .filter(|p| p.is_file() && ext.iter().any(|x| p.to_string_lossy().ends_with(x)))
            .collect()
    }

    /// Windows / Linux: `<Name>/<Name>[.exe]` + libraries + `game/` (Standard)
    /// or `game.kitseal` (protected).
    fn write_folder(
        &self,
        player: &Path,
        out: &Path,
        skip: &[PathBuf],
        key: Option<&kit_export_security::PublisherKey>,
    ) -> Result<()> {
        let name = self.safe_name();
        std::fs::write(out.join("KIT-LICENSE.txt"), KIT_LICENSE)?;
        for lib in self.libraries(player) {
            std::fs::copy(&lib, out.join(lib.file_name().unwrap()))?;
        }
        let content_key = match key {
            None => {
                copy_dir(&self.root, &out.join("game"), skip)?;
                None
            }
            Some(key) => {
                let (sealed, content_key) = self.sealed_pack(skip, key)?;
                std::fs::write(out.join(kit_export_security::SEALED_PACK), sealed)?;
                content_key
            }
        };
        Self::write_exe(&out.join(exe_name(self.platform.id, &name)), &self.launcher(player, key, content_key)?)
    }

    /// macOS: `<Name>.app` with the game in Contents/Resources (a `game/`
    /// folder, or `game.kitseal` when protected).
    fn write_app(
        &self,
        player: &Path,
        app: &Path,
        skip: &[PathBuf],
        key: Option<&kit_export_security::PublisherKey>,
    ) -> Result<()> {
        let name = self.safe_name();
        let contents = app.join("Contents");
        for d in ["MacOS", "Resources", "Frameworks"] {
            std::fs::create_dir_all(contents.join(d))?;
        }
        std::fs::write(contents.join("Resources/KIT-LICENSE.txt"), KIT_LICENSE)?;
        let content_key = match key {
            None => {
                copy_dir(&self.root, &contents.join("Resources/game"), skip)?;
                None
            }
            Some(key) => {
                let (sealed, content_key) = self.sealed_pack(skip, key)?;
                std::fs::write(contents.join("Resources").join(kit_export_security::SEALED_PACK), sealed)?;
                content_key
            }
        };
        Self::write_exe(&contents.join("MacOS").join(&name), &self.launcher(player, key, content_key)?)?;
        for lib in self.libraries(player) {
            std::fs::copy(&lib, contents.join("Frameworks").join(lib.file_name().unwrap()))?;
        }
        let id: String = name.chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>().to_lowercase();
        let plist = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key><string>{title}</string>
    <key>CFBundleDisplayName</key><string>{title}</string>
    <key>CFBundleExecutable</key><string>{name}</string>
    <key>CFBundleIdentifier</key><string>com.kitgame.{id}</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleShortVersionString</key><string>1.0</string>
    <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
"#,
            title = xml_escape(&self.title),
        );
        std::fs::write(contents.join("Info.plist"), plist)?;
        Ok(())
    }
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// A protected Mac app's launcher was changed, so its signature must be
/// renewed. Done here when exporting on a Mac; elsewhere it's reported.
fn resign_mac_app(app: &Path) {
    if cfg!(target_os = "macos") {
        match std::process::Command::new("codesign").args(["--force", "--deep", "--sign", "-"]).arg(app).output() {
            Ok(o) if o.status.success() => {}
            Ok(o) => log::warn!("codesign failed: {}", String::from_utf8_lossy(&o.stderr).trim()),
            Err(e) => log::warn!("codesign: {e}"),
        }
    } else {
        log::warn!(
            "{} must be re-signed on a Mac before it runs on Apple Silicon: codesign --force --deep --sign - <app>",
            app.display()
        );
    }
}

/// Copy a project into `to` (skipping `skip`: export folders inside the project).
fn copy_dir(from: &Path, to: &Path, skip: &[PathBuf]) -> Result<()> {
    let skip: Vec<PathBuf> = skip.iter().filter_map(|p| p.canonicalize().ok()).collect();
    copy_dir_inner(from, to, &skip)
}

fn copy_dir_inner(from: &Path, to: &Path, skip: &[PathBuf]) -> Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)?.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') || name == "target" || name == "build" || name == "export" {
            continue;
        }
        let p = entry.path();
        if p.canonicalize().is_ok_and(|c| skip.iter().any(|s| s.starts_with(&c))) {
            continue;
        }
        if p.is_dir() {
            copy_dir_inner(&p, &to.join(&name), skip)?;
        } else {
            std::fs::copy(&p, to.join(&name))?;
        }
    }
    Ok(())
}

/// The web player's page and renderer, built into the editor so they always
/// match it.
const KIT_LICENSE: &str = include_str!("../../LICENSE");
const WEB_INDEX: &str = include_str!("../../web/template/index.html");
const WEB_SCRIPT: &str = include_str!("../../web/template/kit.js");

/// Serve a web export on localhost (once per folder) and return its URL.
pub fn serve(dir: &Path) -> Result<String> {
    use std::collections::HashMap;
    use std::sync::Mutex;
    static SERVERS: Mutex<Option<HashMap<PathBuf, u16>>> = Mutex::new(None);
    let dir = dir.canonicalize()?;
    let mut servers = SERVERS.lock().unwrap();
    let servers = servers.get_or_insert_with(HashMap::new);
    if let Some(port) = servers.get(&dir) {
        return Ok(format!("http://127.0.0.1:{port}/"));
    }
    let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
    let port = listener.local_addr()?.port();
    let root = dir.clone();
    std::thread::Builder::new().name("kit-web-preview".into()).spawn(move || {
        for stream in listener.incoming().flatten() {
            let root = root.clone();
            std::thread::spawn(move || {
                let _ = serve_one(stream, &root);
            });
        }
    })?;
    servers.insert(dir, port);
    Ok(format!("http://127.0.0.1:{port}/"))
}

fn serve_one(mut stream: std::net::TcpStream, root: &Path) -> Result<()> {
    use std::io::{BufRead, BufReader, Write};
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut request = String::new();
    reader.read_line(&mut request)?;
    loop {
        let mut header = String::new();
        if reader.read_line(&mut header)? == 0 || header.trim().is_empty() {
            break;
        }
    }
    let path = request.split_whitespace().nth(1).unwrap_or("/");
    let path = path.split(['?', '#']).next().unwrap_or("/").trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };
    // Only plain names inside the export folder.
    let file = if path.split('/').all(|c| !c.is_empty() && c != ".." && !c.starts_with('.')) {
        std::fs::read(root.join(path)).ok()
    } else {
        None
    };
    let Some(body) = file else {
        stream.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")?;
        return Ok(());
    };
    let kind = match path.rsplit('.').next() {
        Some("html") => "text/html; charset=utf-8",
        Some("js") => "text/javascript; charset=utf-8",
        Some("wasm") => "application/wasm",
        Some("png") => "image/png",
        Some("txt") => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    };
    write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        body.len()
    )?;
    stream.write_all(&body)?;
    Ok(())
}

/// Open a URL in the default browser.
pub fn open_url(url: &str) {
    let _ = if cfg!(windows) {
        std::process::Command::new("cmd").args(["/C", "start", "", url]).spawn()
    } else if cfg!(target_os = "macos") {
        std::process::Command::new("open").arg(url).spawn()
    } else {
        std::process::Command::new("xdg-open").arg(url).spawn()
    };
}

/// Open a folder in the system file manager.
pub fn reveal(path: &Path) {
    let program = if cfg!(windows) {
        "explorer"
    } else if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    let _ = std::process::Command::new(program).arg(path).spawn();
}

/// Launch an exported game built for this computer.
pub fn launch(out: &Path, platform: &Platform) -> Result<()> {
    if platform.id == "web" {
        open_url(&serve(out)?);
        return Ok(());
    }
    anyhow::ensure!(platform.id == host(), "This build is for {}, not this computer", platform.name);
    if platform.id == "mac" {
        std::process::Command::new("open").arg(out).spawn()?;
    } else {
        let name = out.file_name().context("bad export folder")?.to_string_lossy().to_string();
        std::process::Command::new(out.join(exe_name(platform.id, &name))).current_dir(out).spawn()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("kit_export_test_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("models/thing")).unwrap();
        std::fs::write(dir.join("kit.project"), "[project]\ntitle = \"My Game!\"\n").unwrap();
        std::fs::write(dir.join("models/thing/a.mesh"), "mesh").unwrap();
        std::fs::write(dir.join(".hidden"), "x").unwrap();
        dir
    }

    fn fake_player(dir: &Path, os: &str, matching: bool) -> PathBuf {
        let p = dir.join("players").join(os).join(exe_name(os, "kit-player"));
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        let id = if matching { kit::BUILD_MARKER.to_owned() } else { "kit-build-id:0000".to_owned() };
        std::fs::write(&p, format!("\x7fELF...{id}...")).unwrap();
        p
    }

    #[test]
    fn platforms_report_honest_availability() {
        let bin = std::env::temp_dir().join(format!("kit_export_bin_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&bin);
        let other = if host() == "windows" { "linux" } else { "windows" };
        let other = platform(other).unwrap();
        assert!(matches!(availability(&other, &bin), Availability::NeedsPlayer(_)));
        fake_player(&bin, other.id, false);
        assert!(
            matches!(availability(&other, &bin), Availability::NeedsPlayer(ref s) if s.contains("different Kit version"))
        );
        fake_player(&bin, other.id, true);
        assert!(matches!(availability(&other, &bin), Availability::Ready { .. }));
        for id in ["android", "ios", "xbox", "ps5", "ps4", "switch", "steam"] {
            assert!(!availability(&platform(id).unwrap(), &bin).can_export(), "{id}");
        }
        for id in ["xbox", "ps5", "ps4", "switch", "steam"] {
            assert!(matches!(availability(&platform(id).unwrap(), &bin), Availability::RequestAccess), "{id}");
        }
        assert_eq!(platform("html5").unwrap().id, "web", "old names still work");
    }

    #[test]
    fn writes_windows_folder_and_mac_app() {
        let root = project("desktop");
        let bin = root.join("bin");
        let dest = root.join("out");
        for (id, check) in [("windows", "My_Game/My_Game.exe"), ("mac", "My_Game.app/Contents/MacOS/My_Game")] {
            let player = fake_player(&bin, id, true);
            let job = ExportJob {
                platform: platform(id).unwrap(),
                availability: Availability::Ready { player },
                bin_dir: bin.clone(),
                root: root.clone(),
                title: "My Game!".into(),
                dest: dest.clone(),
                protection: Protection::Standard,
                key_file: None,
            };
            let out = job.run().unwrap();
            assert!(dest.join(check).is_file(), "{id}: {check}");
            let resources = if id == "mac" { out.join("Contents/Resources") } else { out };
            assert_eq!(std::fs::read_to_string(resources.join("KIT-LICENSE.txt")).unwrap(), KIT_LICENSE);
            let game = resources.join("game");
            assert!(game.join("models/thing/a.mesh").is_file());
            assert!(!game.join(".hidden").exists());
            // The export folder sits inside the project but isn't copied into itself.
            assert!(!game.join("out").exists());
        }
        assert!(dest.join("My_Game.app/Contents/Info.plist").is_file());
    }

    #[test]
    fn writes_a_web_site_and_serves_it() {
        let root = project("web");
        let bin = std::env::temp_dir().join(format!("kit_export_test_webbin_{}", std::process::id()));
        let wasm = bin.join("players/web/kit_web.wasm");
        std::fs::create_dir_all(wasm.parent().unwrap()).unwrap();
        std::fs::write(&wasm, format!("\0asm...{}...", kit::BUILD_MARKER)).unwrap();
        let web = platform("web").unwrap();
        assert!(matches!(availability(&web, &bin), Availability::Ready { .. }));
        let job = ExportJob {
            platform: web,
            availability: availability(&web, &bin),
            bin_dir: bin.clone(),
            root: root.clone(),
            title: "My <Game>".into(),
            dest: root.join("out_web"),
            protection: Protection::Standard,
            key_file: None,
        };
        let out = job.run().unwrap();
        for f in ["index.html", "kit-logo.png", "kit.js", "kit_web.wasm", "game.pack", "README.txt"] {
            assert!(out.join(f).is_file(), "{f}");
        }
        assert_eq!(std::fs::read_to_string(out.join("KIT-LICENSE.txt")).unwrap(), KIT_LICENSE);
        let html = std::fs::read_to_string(out.join("index.html")).unwrap();
        assert!(html.contains("<title>My &lt;Game&gt;</title>"));
        assert!(html.contains("src=\"kit-logo.png\""));
        let files = kit_web::pack::read(&std::fs::read(out.join("game.pack")).unwrap()).unwrap();
        assert!(files.contains_key("kit.project") && files.contains_key("models/thing/a.mesh"));
        assert!(!files.keys().any(|k| k.starts_with("out_web") || k.starts_with("bin_web")), "{:?}", files.keys());

        // The preview server serves the files (and nothing outside the folder).
        let url = serve(&out).unwrap();
        let get = |path: &str| {
            use std::io::{Read, Write};
            let mut s = std::net::TcpStream::connect(url.trim_start_matches("http://").trim_end_matches('/')).unwrap();
            write!(s, "GET {path} HTTP/1.1\r\nHost: x\r\n\r\n").unwrap();
            let mut response = Vec::new();
            s.read_to_end(&mut response).unwrap();
            String::from_utf8_lossy(&response).into_owned()
        };
        assert!(get("/kit-logo.png").contains("image/png"));
        assert!(get("/").contains("200 OK") && get("/kit_web.wasm").contains("application/wasm"));
        assert!(get("/../kit.project").starts_with("HTTP/1.1 404"));
    }
}
