//! `kit-circuit`: pack and check Circuit packages (used by release CI and by
//! community publishers).
//!
//!   kit-circuit pack <source-dir> [--out <dir>]   → <id>-<version>.kitcircuit + circuit.json
//!   kit-circuit check <source-dir>                → validate circuit.toml
//!   kit-circuit verify <package> <circuit.json>   → checksum (and signature) check
//!
//! Set `KIT_CIRCUIT_SIGNING_KEY` (base64 Ed25519 secret, from a CI secret, never
//! a file in the repo) and `KIT_CIRCUIT_KEY_ID` to sign while packing.

use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use base64::Engine;
use kit_circuit::package;

fn main() {
    if let Err(e) = run() {
        eprintln!("kit-circuit: {e:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("pack") => {
            let src = PathBuf::from(args.get(1).context("pack <source-dir>")?);
            let out = match args.iter().position(|a| a == "--out") {
                Some(i) => PathBuf::from(args.get(i + 1).context("--out <dir>")?),
                None => src.join("dist"),
            };
            let (manifest, bytes) = package::pack(&src)?;
            let signer = match std::env::var("KIT_CIRCUIT_SIGNING_KEY") {
                Ok(secret) if !secret.trim().is_empty() => {
                    let raw: [u8; 32] = base64::engine::general_purpose::STANDARD
                        .decode(secret.trim())?
                        .try_into()
                        .map_err(|_| anyhow::anyhow!("KIT_CIRCUIT_SIGNING_KEY must be 32 bytes"))?;
                    let key_id = std::env::var("KIT_CIRCUIT_KEY_ID").context("set KIT_CIRCUIT_KEY_ID with the signing key")?;
                    Some((key_id, ed25519_dalek::SigningKey::from_bytes(&raw)))
                }
                _ => None,
            };
            let release = package::release_manifest(&manifest, &bytes, package::read_notes(&src, &manifest), signer.as_ref().map(|(id, k)| (id.as_str(), k)));
            std::fs::create_dir_all(&out)?;
            std::fs::write(out.join(&release.package.file), &bytes)?;
            std::fs::write(out.join("circuit.json"), serde_json::to_string_pretty(&release)?)?;
            println!("{} {} → {} ({} bytes, sha256 {}{})", manifest.circuit.id, manifest.circuit.version, out.join(&release.package.file).display(), bytes.len(), release.package.sha256, if signer.is_some() { ", signed" } else { "" });
        }
        Some("check") => {
            let m = kit_circuit::Manifest::load(&PathBuf::from(args.get(1).context("check <source-dir>")?))?;
            println!("{} {} is valid", m.circuit.id, m.circuit.version);
        }
        Some("verify") => {
            let bytes = std::fs::read(args.get(1).context("verify <package> <circuit.json>")?)?;
            let release: package::ReleaseManifest = serde_json::from_str(&std::fs::read_to_string(args.get(2).context("verify <package> <circuit.json>")?)?)?;
            package::verify(&bytes, &release, &[])?;
            println!("ok: sha256 {}", release.package.sha256);
        }
        _ => bail!("usage: kit-circuit pack <dir> [--out <dir>] | check <dir> | verify <package> <circuit.json>"),
    }
    Ok(())
}
