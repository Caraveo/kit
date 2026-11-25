//! Circuit packages: a gzip'd tar (`<id>-<version>.kitcircuit`) with
//! `circuit.toml` at the root. Packing is deterministic so the same source
//! always gives the same SHA-256. Extraction only writes plain files and
//! folders inside the target and never runs anything.

use std::io::Read;
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result, bail};
use base64::Engine;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::manifest::{self, Manifest};

/// Package file extension.
pub const EXTENSION: &str = "kitcircuit";

/// Limits that keep a bad package from filling the disk.
pub const MAX_PACKAGE_BYTES: u64 = 256 * 1024 * 1024;
pub const MAX_UNPACKED_BYTES: u64 = 1024 * 1024 * 1024;
pub const MAX_ENTRIES: usize = 20_000;

/// What the registry publishes for each release (`circuit.json` next to the
/// package): the manifest plus how to check the package.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ReleaseManifest {
    pub manifest: Manifest,
    pub package: PackageInfo,
    /// Release notes (Markdown), copied from the package when it has them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PackageInfo {
    pub file: String,
    /// Lowercase hex SHA-256 of the package file.
    pub sha256: String,
    pub size: u64,
    /// Base64 Ed25519 signature of [`signed_message`], when the publisher signs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
    /// Which of the publisher's keys made the signature.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key_id: Option<String>,
}

pub fn file_name(id: &str, version: &str) -> String {
    format!("{id}-{version}.{EXTENSION}")
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// The exact bytes a publisher signs: binds the package hash to its id and version.
pub fn signed_message(id: &str, version: &str, sha256: &str) -> Vec<u8> {
    format!("kit-circuit/v1\n{id}\n{version}\n{sha256}\n").into_bytes()
}

/// Is `path` a plain relative path that stays inside the package?
pub fn check_relative(path: &str) -> Result<()> {
    let p = Path::new(path);
    if path.is_empty() || path.contains('\\') || p.is_absolute() {
        bail!("must be a relative path with / separators");
    }
    if path.split('/').any(|seg| seg.is_empty() || seg == "." || seg == ".." || seg.contains(':')) {
        bail!("must not use . or .. or a drive");
    }
    if !p.components().all(|c| matches!(c, Component::Normal(_))) {
        bail!("must not use . or .. or a drive");
    }
    Ok(())
}

/// Files that go into a package: everything under `dir` except hidden files
/// and build output, sorted so packing is reproducible.
fn package_files(dir: &Path) -> Result<Vec<PathBuf>> {
    fn walk(root: &Path, dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
        let mut entries: Vec<_> = std::fs::read_dir(dir)?.collect::<std::io::Result<_>>()?;
        entries.sort_by_key(|e| e.file_name());
        for e in entries {
            let name = e.file_name();
            let name = name.to_string_lossy();
            if name.starts_with('.') || name == "target" || name == "dist" {
                continue;
            }
            let ty = e.file_type()?;
            if ty.is_symlink() {
                bail!("{} is a symlink; packages can't contain links", e.path().display());
            }
            if ty.is_dir() {
                walk(root, &e.path(), out)?;
            } else {
                out.push(e.path().strip_prefix(root)?.to_path_buf());
            }
        }
        Ok(())
    }
    let mut out = Vec::new();
    walk(dir, dir, &mut out)?;
    Ok(out)
}

/// Pack a Circuit source folder. Returns the manifest and the package bytes.
pub fn pack(dir: &Path) -> Result<(Manifest, Vec<u8>)> {
    let manifest = Manifest::load(dir)?;
    for p in manifest.entry.runtime.iter().chain(&manifest.entry.server).chain(&manifest.entry.editor) {
        if !dir.join(p).is_file() {
            bail!("entry {p} is listed in circuit.toml but missing");
        }
    }
    let gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::best());
    let mut tar = tar::Builder::new(gz);
    tar.mode(tar::HeaderMode::Deterministic);
    for rel in package_files(dir)? {
        let name = rel.to_string_lossy().replace('\\', "/");
        check_relative(&name)?;
        let data = std::fs::read(dir.join(&rel))?;
        let mut header = tar::Header::new_gnu();
        header.set_size(data.len() as u64);
        header.set_mode(0o644);
        header.set_mtime(0);
        header.set_cksum();
        tar.append_data(&mut header, &name, data.as_slice())?;
    }
    let bytes = tar.into_inner()?.finish()?;
    Ok((manifest, bytes))
}

/// Build the release manifest for packed bytes, optionally signing them.
pub fn release_manifest(manifest: &Manifest, bytes: &[u8], notes: Option<String>, signer: Option<(&str, &ed25519_dalek::SigningKey)>) -> ReleaseManifest {
    let sha256 = sha256_hex(bytes);
    let c = &manifest.circuit;
    let (signature, key_id) = match signer {
        Some((key_id, key)) => {
            use ed25519_dalek::Signer;
            let sig = key.sign(&signed_message(&c.id, &c.version, &sha256));
            (Some(base64::engine::general_purpose::STANDARD.encode(sig.to_bytes())), Some(key_id.to_owned()))
        }
        None => (None, None),
    };
    ReleaseManifest {
        manifest: manifest.clone(),
        package: PackageInfo { file: file_name(&c.id, &c.version), sha256, size: bytes.len() as u64, signature, key_id },
        notes,
    }
}

/// A publisher key the registry vouches for.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PublisherKey {
    pub key_id: String,
    /// Base64 Ed25519 public key (32 bytes).
    pub public_key: String,
}

/// Check downloaded bytes against the release: size, SHA-256 and, when the
/// publisher signs releases, the signature.
pub fn verify(bytes: &[u8], release: &ReleaseManifest, keys: &[PublisherKey]) -> Result<()> {
    let p = &release.package;
    if bytes.len() as u64 != p.size {
        bail!("the download is {} bytes but the release says {}", bytes.len(), p.size);
    }
    let actual = sha256_hex(bytes);
    if actual != p.sha256.to_ascii_lowercase() {
        bail!("checksum mismatch: expected {}, got {actual}. The download is damaged or was changed.", p.sha256);
    }
    match (&p.signature, keys.is_empty()) {
        (None, true) => Ok(()),
        (None, false) => bail!("this publisher signs its releases but this one has no signature"),
        (Some(_), true) => bail!("the release is signed but the registry has no key for this publisher"),
        (Some(sig), false) => {
            let key = match &p.key_id {
                Some(id) => keys.iter().find(|k| &k.key_id == id).with_context(|| format!("unknown signing key {id}"))?,
                None => &keys[0],
            };
            let b64 = base64::engine::general_purpose::STANDARD;
            let pk: [u8; 32] = b64.decode(&key.public_key)?.try_into().map_err(|_| anyhow::anyhow!("bad public key"))?;
            let sig: [u8; 64] = b64.decode(sig)?.try_into().map_err(|_| anyhow::anyhow!("bad signature"))?;
            let vk = ed25519_dalek::VerifyingKey::from_bytes(&pk)?;
            let c = &release.manifest.circuit;
            vk.verify_strict(&signed_message(&c.id, &c.version, &actual), &ed25519_dalek::Signature::from_bytes(&sig))
                .context("signature check failed: this package was not signed by its publisher")
        }
    }
}

/// Unpack into `dest` (which must not exist yet) and return the manifest found
/// inside. Only regular files and folders are written.
pub fn extract(bytes: &[u8], dest: &Path) -> Result<Manifest> {
    if bytes.len() as u64 > MAX_PACKAGE_BYTES {
        bail!("package is larger than {} MB", MAX_PACKAGE_BYTES / 1024 / 1024);
    }
    if dest.exists() {
        bail!("{} already exists", dest.display());
    }
    std::fs::create_dir_all(dest)?;
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(bytes));
    let mut total = 0u64;
    let mut count = 0usize;
    for entry in archive.entries()? {
        let mut entry = entry?;
        count += 1;
        if count > MAX_ENTRIES {
            bail!("package has more than {MAX_ENTRIES} files");
        }
        let name = entry.path()?.to_string_lossy().into_owned();
        check_relative(name.trim_end_matches('/')).with_context(|| format!("package entry {name}"))?;
        let target = dest.join(name.trim_end_matches('/'));
        match entry.header().entry_type() {
            tar::EntryType::Directory => std::fs::create_dir_all(&target)?,
            tar::EntryType::Regular | tar::EntryType::Continuous => {
                total += entry.size();
                if total > MAX_UNPACKED_BYTES {
                    bail!("package unpacks to more than {} MB", MAX_UNPACKED_BYTES / 1024 / 1024);
                }
                if let Some(parent) = target.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                let mut data = Vec::with_capacity(entry.size() as usize);
                entry.read_to_end(&mut data)?;
                std::fs::write(&target, data)?;
            }
            other => bail!("package entry {name} is a {other:?}; only files and folders are allowed"),
        }
    }
    Manifest::load(dest).context("the package has no valid circuit.toml at its root")
}

/// Read the notes file named by the manifest from a source folder.
pub fn read_notes(dir: &Path, manifest: &Manifest) -> Option<String> {
    manifest.circuit.release_notes.as_ref().and_then(|p| std::fs::read_to_string(dir.join(p)).ok())
}

pub use manifest::FILE as MANIFEST_FILE;

#[cfg(test)]
mod tests {
    use super::*;

    pub fn sample_source(dir: &Path) {
        std::fs::create_dir_all(dir.join("scripts")).unwrap();
        std::fs::write(
            dir.join("circuit.toml"),
            "schema = 1\n[circuit]\nid = \"acme.hello\"\nname = \"Hello\"\npublisher = \"acme\"\ndescription = \"Says hi.\"\nversion = \"1.0.0\"\nlicense = \"MIT\"\nkit = \">=0.0.1\"\nplatforms = [\"linux\", \"macos\", \"windows\"]\nrelease_notes = \"NOTES.md\"\n[entry]\nruntime = [\"scripts/Hello.x\"]\n",
        )
        .unwrap();
        std::fs::write(dir.join("scripts/Hello.x"), "class Hello {}\n").unwrap();
        std::fs::write(dir.join("NOTES.md"), "First release.\n").unwrap();
        std::fs::write(dir.join(".DS_Store"), "junk").unwrap();
    }

    #[test]
    fn pack_is_reproducible_and_round_trips() {
        let src = tempfile::tempdir().unwrap();
        sample_source(src.path());
        let (m, a) = pack(src.path()).unwrap();
        let (_, b) = pack(src.path()).unwrap();
        assert_eq!(a, b, "same source, same bytes");
        let out = tempfile::tempdir().unwrap();
        let dest = out.path().join("x");
        let got = extract(&a, &dest).unwrap();
        assert_eq!(got, m);
        assert!(dest.join("scripts/Hello.x").is_file());
        assert!(!dest.join(".DS_Store").exists());
        let rel = release_manifest(&m, &a, read_notes(src.path(), &m), None);
        assert_eq!(rel.notes.as_deref(), Some("First release.\n"));
        verify(&a, &rel, &[]).unwrap();
        let mut bad = a.clone();
        *bad.last_mut().unwrap() ^= 1;
        assert!(verify(&bad, &rel, &[]).unwrap_err().to_string().contains("checksum"));
    }

    #[test]
    fn signatures_are_checked() {
        let src = tempfile::tempdir().unwrap();
        sample_source(src.path());
        let (m, bytes) = pack(src.path()).unwrap();
        let key = ed25519_dalek::SigningKey::from_bytes(&[7; 32]);
        let pk = base64::engine::general_purpose::STANDARD.encode(key.verifying_key().to_bytes());
        let keys = vec![PublisherKey { key_id: "acme-1".into(), public_key: pk }];
        let signed = release_manifest(&m, &bytes, None, Some(("acme-1", &key)));
        verify(&bytes, &signed, &keys).unwrap();
        let unsigned = release_manifest(&m, &bytes, None, None);
        assert!(verify(&bytes, &unsigned, &keys).is_err(), "signing publishers must sign");
        let other = ed25519_dalek::SigningKey::from_bytes(&[9; 32]);
        let forged = release_manifest(&m, &bytes, None, Some(("acme-1", &other)));
        assert!(verify(&bytes, &forged, &keys).unwrap_err().to_string().contains("signature"));
    }

    #[test]
    fn refuses_escaping_paths() {
        assert!(check_relative("../x").is_err());
        assert!(check_relative("/etc/passwd").is_err());
        assert!(check_relative("a/./b").is_err());
        assert!(check_relative("a\\b").is_err());
        check_relative("scripts/A.x").unwrap();

        // A tar whose entry climbs out of the target.
        let gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        let mut tar = tar::Builder::new(gz);
        let mut h = tar::Header::new_gnu();
        let data = b"pwned";
        h.set_size(data.len() as u64);
        h.set_mode(0o644);
        // set_path refuses "..", so write the raw name.
        h.as_gnu_mut().unwrap().name[..8].copy_from_slice(b"../evil\0");
        h.set_cksum();
        tar.append(&h, &data[..]).unwrap();
        let bytes = tar.into_inner().unwrap().finish().unwrap();
        let out = tempfile::tempdir().unwrap();
        let err = extract(&bytes, &out.path().join("x")).unwrap_err();
        assert!(format!("{err:#}").contains("package entry"), "{err:#}");
        assert!(!out.path().join("evil").exists());
    }
}
