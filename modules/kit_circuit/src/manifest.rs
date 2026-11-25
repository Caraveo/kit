//! `circuit.toml`: what a Circuit is, who publishes it, what it needs and
//! what it adds to a project. See `circuit/SPEC.md` for the full format.

use std::collections::BTreeMap;

use anyhow::{Context, Result, bail};
use semver::{Version, VersionReq};
use serde::{Deserialize, Serialize};

/// The manifest schema this Kit understands.
pub const SCHEMA: u32 = 1;

/// The file name of the manifest at the root of every Circuit package.
pub const FILE: &str = "circuit.toml";

/// Operating systems a Circuit can declare.
pub const PLATFORMS: &[&str] = &["linux", "macos", "windows", "web", "android", "ios"];

/// Architectures a Circuit can declare (`any` means no native code).
pub const ARCHITECTURES: &[&str] = &["any", "x86_64", "aarch64", "wasm32"];

/// Permissions a Circuit can request. Kit shows them before installing.
pub const PERMISSIONS: &[(&str, &str)] = &[
    ("network.client", "Open network connections from the game"),
    ("network.server", "Accept network connections (dedicated servers)"),
    ("project.read", "Read files in the project"),
    ("project.write", "Write files in the project"),
    ("editor.ui", "Add interfaces to the Kit editor"),
    ("native", "Run native code (desktop only, must be signed)"),
];

/// Release channels, most to least stable.
pub const CHANNELS: &[&str] = &["stable", "beta", "alpha"];

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    /// Manifest schema version (currently 1).
    pub schema: u32,
    pub circuit: Info,
    /// Other Circuits this one needs: id → semver requirement.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub dependencies: BTreeMap<String, String>,
    #[serde(default)]
    pub entry: Entry,
    /// Optional settings a project can set for this Circuit.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub config: BTreeMap<String, ConfigField>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Info {
    /// `<publisher>.<name>`, lowercase, e.g. `kit.multiplayer`.
    pub id: String,
    /// Display name.
    pub name: String,
    /// Publisher identifier (the part of `id` before the dot).
    pub publisher: String,
    pub description: String,
    /// Semantic version of this release.
    pub version: String,
    #[serde(default = "stable")]
    pub channel: String,
    /// SPDX license expression.
    pub license: String,
    /// Kit versions this release works with (semver requirement).
    pub kit: String,
    pub platforms: Vec<String>,
    #[serde(default = "any_arch")]
    pub architectures: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub permissions: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub documentation: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repository: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub homepage: Option<String>,
    /// Path of an icon inside the package (PNG or SVG).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    /// Path of the release notes inside the package (Markdown).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub release_notes: Option<String>,
}

fn stable() -> String {
    "stable".into()
}

fn any_arch() -> Vec<String> {
    vec!["any".into()]
}

/// What a Circuit adds. Paths are relative to the package root.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    /// x scripts the game runtime loads.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub runtime: Vec<String>,
    /// x scripts that only run in dedicated server mode.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub server: Vec<String>,
    /// `.xui` interfaces the editor offers (never infrastructure dashboards).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub editor: Vec<String>,
    /// Native libraries by `<os>-<arch>` exposing the C ABI in SPEC.md.
    /// Reserved: this Kit verifies but never loads them.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub native: BTreeMap<String, String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfigField {
    /// `string`, `integer`, `number`, `boolean` or `url`.
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<toml::Value>,
    #[serde(default)]
    pub description: String,
    /// Never written to the project file (e.g. keys); set per machine.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub secret: bool,
}

impl Manifest {
    pub fn parse(text: &str) -> Result<Self> {
        let m: Manifest = toml::from_str(text).context("circuit.toml is not valid")?;
        m.validate()?;
        Ok(m)
    }

    pub fn load(dir: &std::path::Path) -> Result<Self> {
        let path = dir.join(FILE);
        let text = std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
        Self::parse(&text)
    }

    pub fn to_toml(&self) -> String {
        toml::to_string_pretty(self).expect("manifest serializes")
    }

    pub fn version(&self) -> Version {
        Version::parse(&self.circuit.version).expect("validated")
    }

    pub fn kit_requirement(&self) -> VersionReq {
        VersionReq::parse(&self.circuit.kit).expect("validated")
    }

    /// Check everything that can be checked without other files.
    pub fn validate(&self) -> Result<()> {
        if self.schema != SCHEMA {
            bail!("circuit.toml schema {} isn't supported by this Kit (it reads schema {SCHEMA})", self.schema);
        }
        let c = &self.circuit;
        validate_id(&c.id)?;
        let publisher = c.id.split('.').next().unwrap_or_default();
        if c.publisher != publisher {
            bail!("publisher \"{}\" must match the start of the id \"{}\"", c.publisher, c.id);
        }
        if c.name.trim().is_empty() || c.name.len() > 60 {
            bail!("name must be 1–60 characters");
        }
        if c.description.trim().is_empty() || c.description.len() > 400 {
            bail!("description must be 1–400 characters");
        }
        Version::parse(&c.version).with_context(|| format!("version \"{}\" is not semantic (e.g. 1.2.0)", c.version))?;
        VersionReq::parse(&c.kit).with_context(|| format!("kit \"{}\" is not a version requirement (e.g. \">=0.0.4\")", c.kit))?;
        if !CHANNELS.contains(&c.channel.as_str()) {
            bail!("channel must be one of {}", CHANNELS.join(", "));
        }
        if c.license.trim().is_empty() {
            bail!("license is required (an SPDX id such as MIT)");
        }
        if c.platforms.is_empty() {
            bail!("platforms must list at least one of {}", PLATFORMS.join(", "));
        }
        for p in &c.platforms {
            if !PLATFORMS.contains(&p.as_str()) {
                bail!("unknown platform \"{p}\" (use {})", PLATFORMS.join(", "));
            }
        }
        for a in &c.architectures {
            if !ARCHITECTURES.contains(&a.as_str()) {
                bail!("unknown architecture \"{a}\" (use {})", ARCHITECTURES.join(", "));
            }
        }
        for p in &c.permissions {
            if !PERMISSIONS.iter().any(|(id, _)| id == p) {
                bail!("unknown permission \"{p}\"");
            }
        }
        if !self.entry.native.is_empty() && !c.permissions.iter().any(|p| p == "native") {
            bail!("native entry points need the \"native\" permission");
        }
        for url in [&c.documentation, &c.repository, &c.homepage].into_iter().flatten() {
            if !url.starts_with("https://") {
                bail!("links must use https:// ({url})");
            }
        }
        for (id, req) in &self.dependencies {
            validate_id(id)?;
            if id == &c.id {
                bail!("a Circuit can't depend on itself");
            }
            VersionReq::parse(req).with_context(|| format!("dependency {id} = \"{req}\" is not a version requirement"))?;
        }
        let paths = self.entry.runtime.iter().chain(&self.entry.server).chain(&self.entry.editor).chain(self.entry.native.values());
        for p in paths.chain(c.icon.iter()).chain(c.release_notes.iter()) {
            crate::package::check_relative(p).with_context(|| format!("entry path \"{p}\""))?;
        }
        for (key, field) in &self.config {
            if !key.chars().all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_') {
                bail!("config key \"{key}\" must be lowercase letters, digits and _");
            }
            if !["string", "integer", "number", "boolean", "url"].contains(&field.kind.as_str()) {
                bail!("config {key}: unknown type \"{}\"", field.kind);
            }
        }
        Ok(())
    }

    /// Why this release can't be used with this Kit on this platform, if it can't.
    pub fn incompatibility(&self, kit: &Version, platform: &str) -> Option<String> {
        let req = self.kit_requirement();
        if !req.matches(kit) {
            return Some(format!("needs Kit {} (this is {kit})", self.circuit.kit));
        }
        if !self.circuit.platforms.iter().any(|p| p == platform) {
            return Some(format!("doesn't support {platform} (supports {})", self.circuit.platforms.join(", ")));
        }
        None
    }
}

pub fn validate_id(id: &str) -> Result<()> {
    let ok = id.len() <= 64
        && id.split('.').count() == 2
        && id.split('.').all(|part| {
            !part.is_empty()
                && part.chars().next().is_some_and(|c| c.is_ascii_lowercase())
                && part.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        });
    if !ok {
        bail!("id \"{id}\" must look like publisher.name (lowercase letters, digits and -)");
    }
    Ok(())
}

/// The OS name Kit uses in manifests for the machine it runs on.
pub fn host_platform() -> &'static str {
    if cfg!(target_arch = "wasm32") {
        "web"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(windows) {
        "windows"
    } else if cfg!(target_os = "android") {
        "android"
    } else if cfg!(target_os = "ios") {
        "ios"
    } else {
        "linux"
    }
}

/// This Kit's version.
pub fn kit_version() -> Version {
    Version::parse(env!("CARGO_PKG_VERSION")).expect("workspace version is semver")
}

#[cfg(test)]
mod tests {
    use super::*;

    pub const SAMPLE: &str = r#"
schema = 1

[circuit]
id = "kit.multiplayer"
name = "Multiplayer"
publisher = "kit"
description = "Sessions, connection tokens and dedicated servers."
version = "0.1.0"
license = "MIT"
kit = ">=0.0.4"
platforms = ["linux", "macos", "windows"]
permissions = ["network.client", "network.server"]
repository = "https://github.com/caraveo/kit"

[entry]
runtime = ["scripts/Multiplayer.x"]
"#;

    #[test]
    fn parses_and_checks_compatibility() {
        let m = Manifest::parse(SAMPLE).unwrap();
        assert_eq!(m.circuit.channel, "stable");
        assert_eq!(m.circuit.architectures, vec!["any"]);
        assert!(m.incompatibility(&Version::new(0, 0, 4), "linux").is_none());
        assert!(m.incompatibility(&Version::new(0, 0, 3), "linux").unwrap().contains("needs Kit"));
        assert!(m.incompatibility(&Version::new(0, 0, 4), "web").unwrap().contains("doesn't support web"));
        assert_eq!(Manifest::parse(&m.to_toml()).unwrap(), m);
    }

    #[test]
    fn rejects_bad_manifests() {
        let bad = |from: &str, to: &str| Manifest::parse(&SAMPLE.replace(from, to)).unwrap_err().to_string();
        assert!(format!("{:#}", Manifest::parse(&SAMPLE.replace("kit.multiplayer", "Multiplayer")).unwrap_err()).contains("publisher.name"));
        assert!(bad("publisher = \"kit\"", "publisher = \"acme\"").contains("publisher"));
        assert!(bad("0.1.0", "1.0").contains("semantic"));
        assert!(bad("\"linux\", ", "\"beos\", ").contains("platform"));
        assert!(bad("network.server", "root").contains("permission"));
        assert!(bad("scripts/Multiplayer.x", "../evil.x").contains("entry path"));
        assert!(bad("schema = 1", "schema = 2").contains("schema 2"));
        assert!(bad("https://github", "http://github").contains("https"));
    }
}
