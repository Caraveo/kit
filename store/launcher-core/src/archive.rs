use anyhow::{Context, Result, bail};
use sha2::{Digest, Sha256};
use std::{
    io::Read,
    path::{Component, Path, PathBuf},
};

pub const MAX_ARCHIVE_BYTES: u64 = 2 * 1024 * 1024 * 1024;
pub const MAX_UNPACKED_BYTES: u64 = 8 * 1024 * 1024 * 1024;
pub const MAX_ENTRIES: usize = 100_000;

pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

pub fn verify_sha256(bytes: &[u8], expected: &str) -> Result<()> {
    if !bytes.len().try_into().is_ok_and(|n: u64| n <= MAX_ARCHIVE_BYTES) {
        bail!("archive is too large");
    }
    if sha256_hex(bytes).eq_ignore_ascii_case(expected.trim()) { Ok(()) } else { bail!("SHA-256 mismatch") }
}

fn safe_path(path: &Path) -> Result<PathBuf> {
    if path.as_os_str().is_empty()
        || path.is_absolute()
        || path.components().any(|c| !matches!(c, Component::Normal(_)))
    {
        bail!("archive contains an unsafe path: {}", path.display());
    }
    Ok(path.to_path_buf())
}

/// Extract only regular files and directories. The destination must not exist.
pub fn extract_tar_gz(bytes: &[u8], destination: &Path) -> Result<()> {
    if bytes.len() as u64 > MAX_ARCHIVE_BYTES {
        bail!("archive is too large");
    }
    if destination.exists() {
        bail!("staging destination already exists");
    }
    std::fs::create_dir_all(destination)?;
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(bytes));
    let mut total = 0u64;
    for (index, item) in archive.entries()?.enumerate() {
        if index >= MAX_ENTRIES {
            bail!("archive has too many entries");
        }
        let mut entry = item?;
        let name = safe_path(&entry.path()?)?;
        let target = destination.join(&name);
        match entry.header().entry_type() {
            tar::EntryType::Directory => std::fs::create_dir_all(target)?,
            tar::EntryType::Regular | tar::EntryType::Continuous => {
                total = total.checked_add(entry.size()).context("archive size overflow")?;
                if total > MAX_UNPACKED_BYTES {
                    bail!("archive expands beyond the size limit");
                }
                if let Some(parent) = target.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                let mut data = Vec::new();
                entry.read_to_end(&mut data)?;
                std::fs::write(&target, data)?;
            }
            _ => bail!("archive entry {} is a link or unsupported type", name.display()),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::{Compression, write::GzEncoder};

    fn archive(mut add: impl FnMut(&mut tar::Builder<GzEncoder<Vec<u8>>>)) -> Vec<u8> {
        let gz = GzEncoder::new(Vec::new(), Compression::fast());
        let mut tar = tar::Builder::new(gz);
        add(&mut tar);
        let gz = tar.into_inner().unwrap();
        gz.finish().unwrap()
    }

    #[test]
    fn rejects_traversal_and_links() {
        let traversal = archive(|tar| {
            let mut h = tar::Header::new_gnu();
            h.set_size(1);
            h.set_mode(0o644);
            h.as_gnu_mut().unwrap().name[..8].copy_from_slice(b"../evil\0");
            h.set_cksum();
            tar.append(&h, &b"x"[..]).unwrap();
        });
        let dir = tempfile::tempdir().unwrap();
        assert!(extract_tar_gz(&traversal, &dir.path().join("out")).is_err());
        assert!(!dir.path().join("evil").exists());

        let link = archive(|tar| {
            let mut h = tar::Header::new_gnu();
            h.set_entry_type(tar::EntryType::Symlink);
            h.set_link_name("/etc/passwd").unwrap();
            h.set_size(0);
            h.set_cksum();
            tar.append(&h, &[][..]).unwrap();
        });
        assert!(extract_tar_gz(&link, &dir.path().join("link")).is_err());
    }

    #[test]
    fn verifies_digest_case_insensitively() {
        let bytes = b"release";
        assert!(verify_sha256(bytes, &sha256_hex(bytes).to_uppercase()).is_ok());
        assert!(verify_sha256(bytes, "00").is_err());
    }
}
