use anyhow::{Context, Result, bail};
use std::{
    path::{Path, PathBuf},
    process::{Child, Command},
};

use crate::{extract_tar_gz, verify_sha256};

#[derive(Clone, Debug)]
pub struct InstallLayout {
    pub root: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstalledGame {
    pub id: String,
    pub version: String,
    pub active: PathBuf,
    pub saves: PathBuf,
}

impl InstallLayout {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }
    pub fn game(&self, id: &str) -> GameLayout {
        GameLayout { root: self.root.join(id.to_owned()) }
    }
    pub fn for_current_user() -> Result<Self> {
        let home = std::env::var_os("HOME").context("HOME is not set")?;
        Ok(Self::new(PathBuf::from(home).join(".local/share/kit/store")))
    }
}

#[derive(Clone, Debug)]
pub struct GameLayout {
    root: PathBuf,
}
impl GameLayout {
    fn versions(&self) -> PathBuf {
        self.root.join("versions")
    }
    fn active(&self) -> PathBuf {
        self.root.join("active")
    }
    fn saves(&self) -> PathBuf {
        self.root.join("saves")
    }
    fn version(&self, version: &str) -> PathBuf {
        self.versions().join(version)
    }
}

pub fn install(layout: &InstallLayout, id: &str, version: &str, archive: &[u8], sha256: &str) -> Result<InstalledGame> {
    if id.is_empty()
        || version.is_empty()
        || id.contains('/')
        || id.contains('\\')
        || version.contains('/')
        || version.contains('\\')
    {
        bail!("game id and version must be single path components");
    }
    verify_sha256(archive, sha256)?;
    let game = layout.game(id);
    std::fs::create_dir_all(game.versions())?;
    std::fs::create_dir_all(game.saves())?;
    let staging = game.versions().join(format!(".{version}.staging-{}", std::process::id()));
    if staging.exists() {
        std::fs::remove_dir_all(&staging)?;
    }
    if let Err(error) = extract_tar_gz(archive, &staging) {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(error).context("extracting release");
    }
    let target = game.version(version);
    let backup = game.versions().join(format!(".{version}.previous-{}", std::process::id()));
    if backup.exists() {
        std::fs::remove_dir_all(&backup)?;
    }
    if target.exists() {
        std::fs::rename(&target, &backup).context("staging previous release")?;
    }
    if let Err(error) = std::fs::rename(&staging, &target) {
        let _ = std::fs::remove_dir_all(&staging);
        if backup.exists() {
            let _ = std::fs::rename(&backup, &target);
        }
        return Err(error).context("committing release");
    }

    // rename(2) of a symlink is atomic, so a failed switch leaves the old
    // release usable. The symlink target is relative and never user input.
    let active_tmp = game.root.join(format!(".active-{}", std::process::id()));
    let _ = std::fs::remove_file(&active_tmp);
    if let Err(error) = std::os::unix::fs::symlink(Path::new("versions").join(version), &active_tmp) {
        let _ = std::fs::remove_dir_all(&target);
        if backup.exists() {
            let _ = std::fs::rename(&backup, &target);
        }
        return Err(error).context("preparing active release switch");
    }
    if let Err(error) = std::fs::rename(&active_tmp, game.active()) {
        let _ = std::fs::remove_file(&active_tmp);
        let _ = std::fs::remove_dir_all(&target);
        if backup.exists() {
            let _ = std::fs::rename(&backup, &target);
        }
        return Err(error).context("switching active release");
    }
    if backup.exists() {
        std::fs::remove_dir_all(backup)?;
    }
    Ok(InstalledGame { id: id.into(), version: version.into(), active: game.active(), saves: game.saves() })
}

pub fn uninstall(layout: &InstallLayout, id: &str) -> Result<()> {
    let game = layout.game(id);
    if !game.root.exists() {
        return Ok(());
    }
    // Deliberately retain saves. Removing active and versions cannot affect
    // user progress and permits a later reinstall to continue where it left off.
    let _ = std::fs::remove_file(game.active());
    if game.versions().exists() {
        std::fs::remove_dir_all(game.versions())?;
    }
    if game.saves().exists() {
        return Ok(());
    }
    std::fs::remove_dir_all(game.root)?;
    Ok(())
}

pub fn launch(
    installed: &InstalledGame,
    executable: &str,
    args: impl IntoIterator<Item = impl AsRef<std::ffi::OsStr>>,
) -> Result<Child> {
    let relative = Path::new(executable);
    if relative.is_absolute() || relative.components().any(|c| !matches!(c, std::path::Component::Normal(_))) {
        bail!("executable must be a safe relative path");
    }
    let path = installed.active.join(relative);
    if !path.is_file() {
        bail!("executable does not exist: {}", path.display());
    }
    Command::new(path).args(args).current_dir(&installed.active).spawn().context("launching installed game")
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::{Compression, write::GzEncoder};
    use sha2::{Digest, Sha256};

    fn archive(name: &str, contents: &[u8]) -> Vec<u8> {
        let mut gz = GzEncoder::new(Vec::new(), Compression::fast());
        let mut tar = tar::Builder::new(&mut gz);
        let mut header = tar::Header::new_gnu();
        header.set_size(contents.len() as u64);
        header.set_mode(0o755);
        header.set_cksum();
        tar.append_data(&mut header, name, contents).unwrap();
        drop(tar);
        gz.finish().unwrap()
    }
    fn digest(bytes: &[u8]) -> String {
        Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
    }

    #[test]
    fn update_and_uninstall_preserve_saves() {
        let dir = tempfile::tempdir().unwrap();
        let layout = InstallLayout::new(dir.path());
        let a = archive("game", b"one");
        let installed = install(&layout, "demo", "1", &a, &digest(&a)).unwrap();
        std::fs::write(installed.saves.join("slot"), "save").unwrap();
        let b = archive("game", b"two");
        let installed = install(&layout, "demo", "2", &b, &digest(&b)).unwrap();
        assert_eq!(std::fs::read_to_string(installed.saves.join("slot")).unwrap(), "save");
        uninstall(&layout, "demo").unwrap();
        assert!(installed.saves.join("slot").is_file());
    }
}
