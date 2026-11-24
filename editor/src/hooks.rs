//! **Hooks**: Kit's export platform modules, one folder per platform
//! (`hooks/<id>/hook.toml`; see hooks/README.md in the repository).
//!
//! The manifests in the repository's `hooks/` folder are built into the
//! editor. A hook installed next to the editor (`hooks/<id>/`) replaces the
//! built-in one with the same id, which is how a licensed platform (a
//! "Request access" console) gains an exporter.

use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Group {
    Desktop,
    Web,
    Mobile,
    Console,
}

/// Can Kit build this platform?
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Access {
    /// Kit exports it itself.
    Open,
    /// The platform holder licenses the tools: developers apply for access.
    Request,
    /// Not supported yet.
    Planned,
}

/// An installed hook's exporter: a program in the hook's folder.
#[derive(Debug, PartialEq, Eq)]
pub struct External {
    pub dir: PathBuf,
    pub command: String,
    pub args: Vec<String>,
}

/// One export platform (from a hook manifest). `Copy`, with `'static` text:
/// hooks are loaded once per editor run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Platform {
    pub id: &'static str,
    pub name: &'static str,
    /// A glyph for the platform card (no brand logos).
    pub icon: &'static str,
    pub group: Group,
    pub access: Access,
    pub summary: &'static str,
    pub request_url: Option<&'static str>,
    pub request_label: Option<&'static str>,
    pub requirements: &'static [&'static str],
    /// Set by an installed (licensed) hook.
    pub export: Option<&'static External>,
}

#[derive(Deserialize)]
struct Manifest {
    id: String,
    name: String,
    #[serde(default)]
    icon: String,
    group: String,
    access: String,
    #[serde(default)]
    summary: String,
    request_url: Option<String>,
    request_label: Option<String>,
    #[serde(default)]
    requirements: Vec<String>,
    export: Option<ExportSection>,
}

#[derive(Deserialize)]
struct ExportSection {
    command: String,
    #[serde(default)]
    args: Vec<String>,
}

/// The repository's hook manifests, built in.
const BUILTIN: &[&str] = &[
    include_str!("../../hooks/linux/hook.toml"),
    include_str!("../../hooks/windows/hook.toml"),
    include_str!("../../hooks/mac/hook.toml"),
    include_str!("../../hooks/steam/hook.toml"),
    include_str!("../../hooks/web/hook.toml"),
    include_str!("../../hooks/android/hook.toml"),
    include_str!("../../hooks/ios/hook.toml"),
    include_str!("../../hooks/xbox/hook.toml"),
    include_str!("../../hooks/ps5/hook.toml"),
    include_str!("../../hooks/ps4/hook.toml"),
    include_str!("../../hooks/switch/hook.toml"),
];

fn leak(s: String) -> &'static str {
    Box::leak(s.into_boxed_str())
}

/// Parse a manifest; `dir` is set for installed hooks (needed for `[export]`).
pub fn parse(text: &str, dir: Option<&Path>) -> Result<Platform> {
    let m: Manifest = toml::from_str(text).context("reading hook.toml")?;
    anyhow::ensure!(
        !m.id.is_empty() && m.id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'),
        "a hook's id must be a simple folder name"
    );
    let group = match m.group.as_str() {
        "desktop" => Group::Desktop,
        "web" => Group::Web,
        "mobile" => Group::Mobile,
        "console" => Group::Console,
        other => anyhow::bail!("unknown hook group `{other}`"),
    };
    let access = match m.access.as_str() {
        "open" => Access::Open,
        "request" => Access::Request,
        "planned" => Access::Planned,
        other => anyhow::bail!("unknown hook access `{other}`"),
    };
    let export = match (m.export, dir) {
        (Some(e), Some(dir)) => {
            anyhow::ensure!(
                !e.command.contains("..") && !Path::new(&e.command).is_absolute(),
                "a hook's export command must be a program inside its folder"
            );
            Some(&*Box::leak(Box::new(External { dir: dir.to_path_buf(), command: e.command, args: e.args })))
        }
        _ => None,
    };
    let requirements: Vec<&'static str> = m.requirements.into_iter().map(leak).collect();
    Ok(Platform {
        id: leak(m.id),
        name: leak(m.name),
        icon: if m.icon.is_empty() { "◇" } else { leak(m.icon) },
        group,
        access,
        summary: leak(m.summary),
        request_url: m.request_url.map(leak),
        request_label: m.request_label.map(leak),
        requirements: Box::leak(requirements.into_boxed_slice()),
        export,
    })
}

/// Where installed hooks live, next to the editor.
pub fn install_dirs(bin_dir: &Path) -> Vec<PathBuf> {
    vec![bin_dir.join("hooks"), bin_dir.join("../Resources/hooks"), bin_dir.join("../share/kit/hooks")]
}

/// Built-in hooks, overridden or extended by hooks installed in `dirs`.
pub fn load(dirs: &[PathBuf]) -> Vec<Platform> {
    let mut hooks: Vec<Platform> = BUILTIN.iter().map(|t| parse(t, None).expect("built-in hook.toml")).collect();
    for dir in dirs {
        let Ok(entries) = std::fs::read_dir(dir) else { continue };
        let mut found: Vec<PathBuf> = entries.flatten().map(|e| e.path()).filter(|p| p.join("hook.toml").is_file()).collect();
        found.sort();
        for hook_dir in found {
            let parsed = std::fs::read_to_string(hook_dir.join("hook.toml"))
                .map_err(anyhow::Error::from)
                .and_then(|t| parse(&t, Some(&hook_dir)));
            match parsed {
                Ok(h) => match hooks.iter_mut().find(|x| x.id == h.id) {
                    Some(existing) => *existing = h,
                    None => hooks.push(h),
                },
                Err(e) => log::warn!("hook {}: {e:#}", hook_dir.display()),
            }
        }
    }
    hooks
}

/// Every export platform (loaded once).
pub fn all() -> &'static [Platform] {
    static HOOKS: OnceLock<Vec<Platform>> = OnceLock::new();
    HOOKS.get_or_init(|| {
        let bin_dir = std::env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf)).unwrap_or_default();
        load(&install_dirs(&bin_dir))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_hooks_cover_every_platform() {
        let ids: Vec<&str> = load(&[]).iter().map(|h| h.id).collect();
        assert_eq!(ids, ["linux", "windows", "mac", "steam", "web", "android", "ios", "xbox", "ps5", "ps4", "switch"]);
        for h in load(&[]) {
            if h.access == Access::Request {
                assert!(h.request_url.is_some_and(|u| u.starts_with("https://")), "{} has a Request access link", h.id);
                assert!(!h.requirements.is_empty());
            }
            assert!(h.export.is_none(), "built-in hooks have no external exporter");
        }
    }

    #[test]
    fn installed_hooks_replace_builtins() {
        let dir = std::env::temp_dir().join(format!("kit_hooks_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("switch")).unwrap();
        std::fs::create_dir_all(dir.join("broken")).unwrap();
        std::fs::write(
            dir.join("switch/hook.toml"),
            "id = \"switch\"\nname = \"Nintendo Switch\"\ngroup = \"console\"\naccess = \"request\"\n\
             [export]\ncommand = \"export.sh\"\nargs = [\"{project}\", \"{out}\"]\n",
        )
        .unwrap();
        std::fs::write(dir.join("broken/hook.toml"), "id = \"../evil\"\nname = \"x\"\ngroup = \"console\"\naccess = \"open\"\n").unwrap();
        let hooks = load(&[dir.clone()]);
        let switch = hooks.iter().find(|h| h.id == "switch").unwrap();
        let export = switch.export.expect("installed hook brings its exporter");
        assert_eq!(export.dir, dir.join("switch"));
        assert_eq!(export.command, "export.sh");
        assert_eq!(hooks.len(), 11, "the broken hook is skipped, not added");
        assert!(parse("id = \"x\"\nname = \"x\"\ngroup = \"desktop\"\naccess = \"open\"\n[export]\ncommand = \"../../bin/sh\"\n", Some(&dir)).is_err());
    }
}
