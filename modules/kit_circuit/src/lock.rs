//! `circuits.lock`: the exact Circuits a project uses. Checked into the
//! project so every machine (and every export) gets the same bytes.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

pub const FILE: &str = "circuits.lock";
/// Installed Circuits live in `<project>/circuits/<id>/`.
pub const DIR: &str = "circuits";

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Lock {
    /// Lockfile format version.
    pub version: u32,
    #[serde(default, rename = "circuit")]
    pub circuits: Vec<Locked>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Locked {
    pub id: String,
    pub version: String,
    /// SHA-256 of the package that was installed.
    pub sha256: String,
    /// Where it came from (the registry download URL).
    pub source: String,
    /// Disabled Circuits stay installed but the game doesn't load them.
    #[serde(default = "yes")]
    pub enabled: bool,
}

fn yes() -> bool {
    true
}

impl Lock {
    pub fn path(project: &Path) -> PathBuf {
        project.join(FILE)
    }

    /// Read the project's lockfile (empty when there is none).
    pub fn load(project: &Path) -> Result<Self> {
        let path = Self::path(project);
        if !path.exists() {
            return Ok(Self { version: 1, circuits: Vec::new() });
        }
        let text = std::fs::read_to_string(&path)?;
        let lock: Lock = toml::from_str(&text).with_context(|| format!("{} is damaged", path.display()))?;
        if lock.version != 1 {
            bail!("{} is format {}; this Kit reads format 1", path.display(), lock.version);
        }
        Ok(lock)
    }

    /// Write atomically (temp file, then rename).
    pub fn save(&self, project: &Path) -> Result<()> {
        let mut sorted = self.clone();
        sorted.version = 1;
        sorted.circuits.sort_by(|a, b| a.id.cmp(&b.id));
        let text = format!(
            "# Circuits this project uses. Kit writes this file; commit it with the project.\n{}",
            toml::to_string_pretty(&sorted)?
        );
        let path = Self::path(project);
        let tmp = path.with_extension("lock.tmp");
        std::fs::write(&tmp, text)?;
        std::fs::rename(&tmp, &path)?;
        Ok(())
    }

    pub fn get(&self, id: &str) -> Option<&Locked> {
        self.circuits.iter().find(|c| c.id == id)
    }

    pub fn upsert(&mut self, entry: Locked) {
        match self.circuits.iter_mut().find(|c| c.id == entry.id) {
            Some(c) => *c = entry,
            None => self.circuits.push(entry),
        }
    }

    pub fn remove(&mut self, id: &str) -> Option<Locked> {
        let i = self.circuits.iter().position(|c| c.id == id)?;
        Some(self.circuits.remove(i))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_sorted() {
        let dir = tempfile::tempdir().unwrap();
        let mut lock = Lock::load(dir.path()).unwrap();
        assert!(lock.circuits.is_empty());
        for id in ["kit.zeta", "kit.alpha"] {
            lock.upsert(Locked {
                id: id.into(),
                version: "1.0.0".into(),
                sha256: "ab".into(),
                source: "s".into(),
                enabled: true,
            });
        }
        lock.save(dir.path()).unwrap();
        let back = Lock::load(dir.path()).unwrap();
        assert_eq!(back.circuits[0].id, "kit.alpha");
        assert_eq!(back.circuits.len(), 2);
    }
}
