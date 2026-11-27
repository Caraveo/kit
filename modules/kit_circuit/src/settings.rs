//! Per-project Circuit settings: `circuits.settings.toml` in the project root,
//! one table per Circuit id. It lives outside `circuits/` so uninstalling and
//! reinstalling a Circuit keeps its settings. Fields marked `secret` are never
//! written to the project (they'd ship inside the game); they live in a file
//! in the user's Kit settings folder instead.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde_json::{Map, Value};

use crate::manifest::Manifest;

pub const FILE: &str = "circuits.settings.toml";

fn read_table(path: &Path) -> Result<toml::Table> {
    if !path.exists() {
        return Ok(toml::Table::new());
    }
    let text = std::fs::read_to_string(path)?;
    toml::from_str(&text).with_context(|| format!("{} is damaged", path.display()))
}

fn write_table(path: &Path, table: &toml::Table, header: &str) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("toml.tmp");
    std::fs::write(&tmp, format!("{header}{}", toml::to_string_pretty(table)?))?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

/// Where a project's secret settings live on this machine.
pub fn secrets_path(config_dir: &Path, project: &Path) -> PathBuf {
    let key = crate::package::sha256_hex(project.to_string_lossy().as_bytes());
    config_dir.join("circuit-secrets").join(format!("{}.toml", &key[..16]))
}

/// A Circuit's settings: saved values over the manifest's defaults.
pub fn load(project: &Path, secrets: Option<&Path>, m: &Manifest) -> Result<Map<String, Value>> {
    let id = &m.circuit.id;
    let saved = read_table(&project.join(FILE))?;
    let secret = match secrets {
        Some(p) => read_table(p)?,
        None => toml::Table::new(),
    };
    let mut out = Map::new();
    for (key, field) in &m.config {
        let source = if field.secret { &secret } else { &saved };
        let v = source
            .get(id)
            .and_then(|t| t.get(key))
            .and_then(|v| serde_json::to_value(v).ok())
            .unwrap_or_else(|| field.default_json());
        out.insert(key.clone(), v);
    }
    Ok(out)
}

/// Save a Circuit's settings (only keys its manifest declares).
pub fn save(project: &Path, secrets: Option<&Path>, m: &Manifest, values: &Map<String, Value>) -> Result<()> {
    let id = &m.circuit.id;
    let mut public = toml::Table::new();
    let mut secret = toml::Table::new();
    for (key, field) in &m.config {
        let Some(v) = values.get(key) else { continue };
        let Ok(tv) = toml::Value::try_from(v.clone()) else { continue };
        if field.secret {
            secret.insert(key.clone(), tv)
        } else {
            public.insert(key.clone(), tv)
        };
    }
    let path = project.join(FILE);
    let mut all = read_table(&path)?;
    all.insert(id.clone(), toml::Value::Table(public));
    write_table(
        &path,
        &all,
        "# Circuit settings for this project (Project menu → each Circuit). Commit it with the project.\n",
    )?;
    if let Some(p) = secrets {
        if !secret.is_empty() || p.exists() {
            let mut all = read_table(p)?;
            all.insert(id.clone(), toml::Value::Table(secret));
            write_table(
                p,
                &all,
                "# Secret Circuit settings for one project on this machine. Never commit or ship this.\n",
            )?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secrets_stay_out_of_the_project() {
        let project = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let m = Manifest::parse(
            "schema = 1\n[circuit]\nid = \"acme.x\"\nname = \"X\"\npublisher = \"acme\"\ndescription = \"d\"\nversion = \"1.0.0\"\nlicense = \"MIT\"\nkit = \">=0.0.1\"\nplatforms = [\"linux\"]\n[config.world]\ntype = \"string\"\ndefault = \"main\"\n[config.key]\ntype = \"string\"\nsecret = true\n",
        )
        .unwrap();
        let secrets = secrets_path(home.path(), project.path());
        let mut v = load(project.path(), Some(&secrets), &m).unwrap();
        assert_eq!(v["world"], "main");
        v.insert("world".into(), "forest".into());
        v.insert("key".into(), "hunter2".into());
        save(project.path(), Some(&secrets), &m, &v).unwrap();
        let text = std::fs::read_to_string(project.path().join(FILE)).unwrap();
        assert!(text.contains("forest") && !text.contains("hunter2"), "{text}");
        assert_eq!(load(project.path(), Some(&secrets), &m).unwrap()["key"], "hunter2");
    }
}
