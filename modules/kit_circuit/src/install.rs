//! Installing Circuits into a project: verify, check compatibility and
//! dependencies, unpack beside the old copy, swap, record in the lockfile.
//! Any failure leaves the project as it was.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use semver::{Version, VersionReq};

use crate::lock::{self, Lock, Locked};
use crate::manifest::Manifest;
use crate::package::{self, PublisherKey, ReleaseManifest};

/// An installed Circuit as the project has it.
#[derive(Clone, Debug)]
pub struct Installed {
    pub lock: Locked,
    /// `None` when the folder is missing or its manifest is damaged.
    pub manifest: Option<Manifest>,
    pub problem: Option<String>,
}

pub fn circuit_dir(project: &Path, id: &str) -> PathBuf {
    project.join(lock::DIR).join(id)
}

fn staging_dir(project: &Path) -> PathBuf {
    project.join(lock::DIR).join(".staging")
}

/// Every Circuit in the lockfile, with its manifest and any problem found.
pub fn installed(project: &Path) -> Result<Vec<Installed>> {
    let lock = Lock::load(project)?;
    Ok(lock
        .circuits
        .into_iter()
        .map(|l| {
            let (manifest, problem) = match Manifest::load(&circuit_dir(project, &l.id)) {
                Ok(m) if m.circuit.version != l.version => {
                    let v = m.circuit.version.clone();
                    (Some(m), Some(format!("folder has {v} but circuits.lock says {}", l.version)))
                }
                Ok(m) => (Some(m), None),
                Err(e) => (None, Some(format!("{e:#}"))),
            };
            Installed { lock: l, manifest, problem }
        })
        .collect())
}

/// The enabled Circuits the game should load, in dependency order.
pub fn enabled(project: &Path) -> Result<Vec<Manifest>> {
    let mut list: Vec<Manifest> =
        installed(project)?.into_iter().filter(|i| i.lock.enabled && i.problem.is_none()).filter_map(|i| i.manifest).collect();
    // Dependencies first (simple stable topological sort; cycles keep file order).
    let mut ordered: Vec<Manifest> = Vec::new();
    while !list.is_empty() {
        let pos = list
            .iter()
            .position(|m| m.dependencies.keys().all(|d| ordered.iter().any(|o| &o.circuit.id == d) || !list.iter().any(|l| &l.circuit.id == d)))
            .unwrap_or(0);
        ordered.push(list.remove(pos));
    }
    Ok(ordered)
}

/// Why installing `next` would break the project, if it would.
pub fn check_install(project: &Path, next: &Manifest, kit: &Version, platform: &str) -> Result<()> {
    if let Some(why) = next.incompatibility(kit, platform) {
        bail!("{} {} {why}", next.circuit.name, next.circuit.version);
    }
    let lock = Lock::load(project)?;
    for (dep, req) in &next.dependencies {
        let req = VersionReq::parse(req)?;
        match lock.get(dep) {
            None => bail!("{} needs the Circuit {dep} ({req}). Install it first.", next.circuit.name),
            Some(have) if !req.matches(&Version::parse(&have.version)?) => {
                bail!("{} needs {dep} {req}, but the project has {}", next.circuit.name, have.version)
            }
            _ => {}
        }
    }
    let new_version = next.version();
    for other in installed(project)? {
        let Some(m) = other.manifest else { continue };
        if let Some(req) = m.dependencies.get(&next.circuit.id) {
            if !VersionReq::parse(req)?.matches(&new_version) {
                bail!("{} needs {} {req}; {new_version} would break it", m.circuit.name, next.circuit.id);
            }
        }
    }
    Ok(())
}

/// Install (or update/downgrade to) a verified release.
pub fn install(project: &Path, release: &ReleaseManifest, bytes: &[u8], keys: &[PublisherKey], source: &str) -> Result<()> {
    install_with(project, release, bytes, keys, source, &crate::manifest::kit_version(), crate::manifest::host_platform())
}

pub fn install_with(
    project: &Path,
    release: &ReleaseManifest,
    bytes: &[u8],
    keys: &[PublisherKey],
    source: &str,
    kit: &Version,
    platform: &str,
) -> Result<()> {
    release.manifest.validate()?;
    package::verify(bytes, release, keys)?;
    check_install(project, &release.manifest, kit, platform)?;
    let id = &release.manifest.circuit.id;

    let staging = staging_dir(project);
    std::fs::create_dir_all(&staging)?;
    let fresh = staging.join(format!("{id}.new"));
    let previous = staging.join(format!("{id}.previous"));
    for stale in [&fresh, &previous] {
        if stale.exists() {
            std::fs::remove_dir_all(stale)?;
        }
    }
    let unpacked = match package::extract(bytes, &fresh) {
        Ok(m) => m,
        Err(e) => {
            let _ = std::fs::remove_dir_all(&fresh);
            return Err(e);
        }
    };
    if unpacked != release.manifest {
        let _ = std::fs::remove_dir_all(&fresh);
        bail!("the package's circuit.toml doesn't match the published release");
    }

    // Swap the folders, then the lockfile; undo the swap if the lockfile fails.
    let target = circuit_dir(project, id);
    let had_previous = target.exists();
    if had_previous && Lock::load(project)?.get(id).is_none() {
        let _ = std::fs::remove_dir_all(&fresh);
        bail!("{} exists but Kit didn't install it, so Kit won't replace it. Move that folder first.", target.display());
    }
    if had_previous {
        std::fs::rename(&target, &previous).context("moving the installed version aside")?;
    }
    if let Err(e) = std::fs::rename(&fresh, &target) {
        if had_previous {
            let _ = std::fs::rename(&previous, &target);
        }
        return Err(e).context("moving the new version into place");
    }
    let mut lock = Lock::load(project)?;
    let enabled = lock.get(id).is_none_or(|l| l.enabled);
    lock.upsert(Locked {
        id: id.clone(),
        version: release.manifest.circuit.version.clone(),
        sha256: release.package.sha256.clone(),
        source: source.to_owned(),
        enabled,
    });
    if let Err(e) = lock.save(project) {
        let _ = std::fs::remove_dir_all(&target);
        if had_previous {
            let _ = std::fs::rename(&previous, &target);
        }
        return Err(e).context("writing circuits.lock");
    }
    if had_previous {
        let _ = std::fs::remove_dir_all(&previous);
    }
    let _ = std::fs::remove_dir(&staging);
    Ok(())
}

/// Remove a Circuit, unless another installed Circuit needs it.
pub fn uninstall(project: &Path, id: &str) -> Result<()> {
    let mut lock = Lock::load(project)?;
    if lock.get(id).is_none() {
        bail!("{id} isn't installed");
    }
    for other in installed(project)? {
        if let Some(m) = other.manifest {
            if m.dependencies.contains_key(id) {
                bail!("{} needs {id}. Uninstall {} first.", m.circuit.name, m.circuit.name);
            }
        }
    }
    let target = circuit_dir(project, id);
    let staging = staging_dir(project);
    let doomed = staging.join(format!("{id}.removed"));
    if target.exists() {
        std::fs::create_dir_all(&staging)?;
        if doomed.exists() {
            std::fs::remove_dir_all(&doomed)?;
        }
        std::fs::rename(&target, &doomed)?;
    }
    lock.remove(id);
    if let Err(e) = lock.save(project) {
        if doomed.exists() {
            let _ = std::fs::rename(&doomed, &target);
        }
        return Err(e);
    }
    let _ = std::fs::remove_dir_all(&doomed);
    let _ = std::fs::remove_dir(&staging);
    Ok(())
}

pub fn set_enabled(project: &Path, id: &str, enabled: bool) -> Result<()> {
    let mut lock = Lock::load(project)?;
    let Some(entry) = lock.circuits.iter_mut().find(|c| c.id == id) else { bail!("{id} isn't installed") };
    entry.enabled = enabled;
    lock.save(project)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn release(dir: &Path, id: &str, version: &str, deps: &str) -> (ReleaseManifest, Vec<u8>) {
        let publisher = id.split('.').next().unwrap();
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(
            dir.join("circuit.toml"),
            format!(
                "schema = 1\n[circuit]\nid = \"{id}\"\nname = \"{id}\"\npublisher = \"{publisher}\"\ndescription = \"d\"\nversion = \"{version}\"\nlicense = \"MIT\"\nkit = \">=0.0.1\"\nplatforms = [\"linux\"]\n[dependencies]\n{deps}\n"
            ),
        )
        .unwrap();
        std::fs::write(dir.join("README.md"), version).unwrap();
        let (m, bytes) = package::pack(dir).unwrap();
        (package::release_manifest(&m, &bytes, None, None), bytes)
    }

    fn put(project: &Path, src: &Path, id: &str, v: &str, deps: &str) -> Result<()> {
        let (r, b) = release(&src.join(format!("{id}-{v}")), id, v, deps);
        install_with(project, &r, &b, &[], "test", &Version::new(0, 0, 4), "linux")
    }

    #[test]
    fn install_update_downgrade_uninstall() {
        let project = tempfile::tempdir().unwrap();
        let src = tempfile::tempdir().unwrap();
        let p = project.path();
        put(p, src.path(), "kit.net", "1.0.0", "").unwrap();
        put(p, src.path(), "kit.net", "1.2.0", "").unwrap();
        assert_eq!(std::fs::read_to_string(circuit_dir(p, "kit.net").join("README.md")).unwrap(), "1.2.0");
        put(p, src.path(), "kit.net", "1.0.0", "").unwrap();
        assert_eq!(Lock::load(p).unwrap().get("kit.net").unwrap().version, "1.0.0");
        set_enabled(p, "kit.net", false).unwrap();
        put(p, src.path(), "kit.net", "1.1.0", "").unwrap();
        assert!(!Lock::load(p).unwrap().get("kit.net").unwrap().enabled, "updates keep the enabled switch");
        uninstall(p, "kit.net").unwrap();
        assert!(!circuit_dir(p, "kit.net").exists());
        assert!(Lock::load(p).unwrap().circuits.is_empty());
        assert!(!p.join("circuits/.staging").exists());
    }

    #[test]
    fn dependencies_are_enforced() {
        let project = tempfile::tempdir().unwrap();
        let src = tempfile::tempdir().unwrap();
        let p = project.path();
        let err = put(p, src.path(), "acme.game", "1.0.0", "\"kit.net\" = \"^1.1\"").unwrap_err();
        assert!(err.to_string().contains("Install it first"), "{err}");
        put(p, src.path(), "kit.net", "1.1.0", "").unwrap();
        put(p, src.path(), "acme.game", "1.0.0", "\"kit.net\" = \"^1.1\"").unwrap();
        let err = put(p, src.path(), "kit.net", "1.0.0", "").unwrap_err();
        assert!(err.to_string().contains("would break"), "{err}");
        assert!(uninstall(p, "kit.net").unwrap_err().to_string().contains("needs kit.net"));
        let order: Vec<_> = enabled(p).unwrap().into_iter().map(|m| m.circuit.id).collect();
        assert_eq!(order, vec!["kit.net", "acme.game"]);
    }

    #[test]
    fn bad_download_changes_nothing() {
        let project = tempfile::tempdir().unwrap();
        let src = tempfile::tempdir().unwrap();
        let p = project.path();
        put(p, src.path(), "kit.net", "1.0.0", "").unwrap();
        let (r, mut b) = release(&src.path().join("bad"), "kit.net", "2.0.0", "");
        b[10] ^= 0xff;
        assert!(install_with(p, &r, &b, &[], "t", &Version::new(0, 0, 4), "linux").is_err());
        assert_eq!(Lock::load(p).unwrap().get("kit.net").unwrap().version, "1.0.0");
        assert_eq!(std::fs::read_to_string(circuit_dir(p, "kit.net").join("README.md")).unwrap(), "1.0.0");
    }

    #[test]
    fn never_replaces_folders_it_did_not_install() {
        let project = tempfile::tempdir().unwrap();
        let src = tempfile::tempdir().unwrap();
        let mine = circuit_dir(project.path(), "kit.net");
        std::fs::create_dir_all(&mine).unwrap();
        std::fs::write(mine.join("notes.txt"), "my work").unwrap();
        let err = put(project.path(), src.path(), "kit.net", "1.0.0", "").unwrap_err();
        assert!(err.to_string().contains("won't replace"), "{err}");
        assert_eq!(std::fs::read_to_string(mine.join("notes.txt")).unwrap(), "my work");
    }

    #[test]
    fn incompatible_kit_is_refused() {
        let project = tempfile::tempdir().unwrap();
        let src = tempfile::tempdir().unwrap();
        let (r, b) = release(&src.path().join("a"), "kit.net", "1.0.0", "");
        let err = install_with(project.path(), &r, &b, &[], "t", &Version::new(0, 0, 4), "web").unwrap_err();
        assert!(err.to_string().contains("doesn't support web"));
    }
}
