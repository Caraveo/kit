//! Game packs: a whole project in one file, so the web page can fetch it in
//! one request.
//!
//! Layout (little-endian): `KITPACK1`, file count (u32), then per file: path
//! length (u32), UTF-8 project-relative path with `/` separators, data length
//! (u32), data.

use anyhow::{Context, Result, bail, ensure};
use std::collections::HashMap;
use std::path::Path;

const MAGIC: &[u8; 8] = b"KITPACK1";

pub fn read(bytes: &[u8]) -> Result<HashMap<String, Vec<u8>>> {
    ensure!(bytes.starts_with(MAGIC), "not a Kit game pack");
    let mut at = MAGIC.len();
    let mut take = |n: usize| -> Result<&[u8]> {
        let end = at.checked_add(n).filter(|&e| e <= bytes.len()).context("game pack is truncated")?;
        let s = &bytes[at..end];
        at = end;
        Ok(s)
    };
    let word = |b: &[u8]| u32::from_le_bytes(b.try_into().unwrap()) as usize;
    let count = word(take(4)?);
    let mut files = HashMap::with_capacity(count);
    for _ in 0..count {
        let name_len = word(take(4)?);
        let name = std::str::from_utf8(take(name_len)?).context("game pack has a bad file name")?.to_owned();
        let len = word(take(4)?);
        files.insert(name, take(len)?.to_vec());
    }
    Ok(files)
}

/// Pack a project folder. Skips hidden files and build/export folders, like
/// the desktop export.
pub fn write_dir(root: &Path) -> Result<Vec<u8>> {
    write_dir_skipping(root, &[])
}

/// [`write_dir`], also skipping these folders (e.g. an export folder that
/// sits inside the project).
pub fn write_dir_skipping(root: &Path, skip: &[std::path::PathBuf]) -> Result<Vec<u8>> {
    let skip: Vec<_> = skip.iter().filter_map(|p| p.canonicalize().ok()).collect();
    let mut files = Vec::new();
    collect(root, root, &skip, &mut files)?;
    files.sort();
    let mut out = MAGIC.to_vec();
    out.extend_from_slice(&u32::try_from(files.len())?.to_le_bytes());
    for (name, path) in files {
        let data = std::fs::read(&path).with_context(|| format!("reading {}", path.display()))?;
        out.extend_from_slice(&u32::try_from(name.len())?.to_le_bytes());
        out.extend_from_slice(name.as_bytes());
        out.extend_from_slice(&u32::try_from(data.len()).context("file too big for a game pack")?.to_le_bytes());
        out.extend_from_slice(&data);
    }
    Ok(out)
}

fn collect(
    root: &Path,
    dir: &Path,
    skip: &[std::path::PathBuf],
    files: &mut Vec<(String, std::path::PathBuf)>,
) -> Result<()> {
    for entry in std::fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))?.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') || name == "target" || name == "build" || name == "export" {
            continue;
        }
        let p = entry.path();
        if p.canonicalize().is_ok_and(|c| skip.iter().any(|s| s.starts_with(&c))) {
            continue;
        }
        if p.is_dir() {
            collect(root, &p, skip, files)?;
        } else {
            let rel = p.strip_prefix(root)?;
            let rel: Vec<String> = rel.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
            if rel.iter().any(|c| c.contains('/')) {
                bail!("bad file name {}", p.display());
            }
            files.push((rel.join("/"), p));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let dir = std::env::temp_dir().join(format!("kit_pack_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("scripts")).unwrap();
        std::fs::create_dir_all(dir.join("export/web")).unwrap();
        std::fs::write(dir.join("kit.project"), "[project]").unwrap();
        std::fs::write(dir.join("scripts/a.x"), "x").unwrap();
        std::fs::write(dir.join("export/web/old.pack"), "skip").unwrap();
        std::fs::write(dir.join(".hidden"), "skip").unwrap();
        let files = read(&write_dir(&dir).unwrap()).unwrap();
        let mut names: Vec<_> = files.keys().cloned().collect();
        names.sort();
        assert_eq!(names, ["kit.project", "scripts/a.x"]);
        assert_eq!(files["scripts/a.x"], b"x");
        assert!(read(b"KITPACK1\x05\0\0\0").is_err(), "truncated packs are rejected");
    }
}
