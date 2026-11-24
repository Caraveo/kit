//! # kit_export_security
//!
//! Protected exports: a **sealed game pack** (`game.kitseal`) and the trust
//! pinned into the game's launcher.
//!
//! The payload is Kit's existing game pack (`KITPACK1`). Around it goes a
//! versioned envelope:
//!
//! ```text
//! "KITSEAL1"               8 bytes   magic
//! version                  u16 LE    = 1
//! mode                     u8        1 = signed, 2 = signed + encrypted
//! reserved                 u8        = 0
//! key id                   8 bytes   first 8 bytes of SHA-256(publisher public key)
//! nonce                    24 bytes  XChaCha20 nonce (encrypted), zero (signed)
//! metadata length          u32 LE    then UTF-8 metadata (`key=value` lines)
//! payload length           u64 LE    then the payload (pack, or ciphertext + tag)
//! signature                64 bytes  Ed25519 over every byte before it
//! ```
//!
//! The signature covers the version, mode, key id, nonce, metadata and payload,
//! so none of them can change without detection. Encrypted packs also bind
//! everything before the payload as AEAD associated data. Players **verify
//! the signature first**, then decrypt, then parse the pack.
//!
//! The verifying key and the required mode never come from the pack itself.
//! They're pinned in the launcher: the desktop player's [`TrustSlot`], which
//! the exporter writes into the copy it ships, or the web bootstrap (`kit.js`).
//! Replacing the launcher or bootstrap is outside this guarantee.

#[cfg(feature = "publish")]
use anyhow::Context;
use anyhow::{Result, ensure};

pub const SEAL_MAGIC: &[u8; 8] = b"KITSEAL1";
pub const SEAL_VERSION: u16 = 1;
/// File name of a sealed pack next to a protected launcher.
pub const SEALED_PACK: &str = "game.kitseal";
const HEADER_FIXED: usize = 8 + 2 + 1 + 1 + 8 + 24;
const SIGNATURE_LEN: usize = 64;

/// How an export is protected.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Mode {
    /// Plain game files (the existing export).
    #[default]
    Standard,
    /// The pack is signed; players reject any change.
    Signed,
    /// Signed and encrypted (desktop only).
    Encrypted,
}

impl Mode {
    pub const ALL: [Mode; 3] = [Mode::Standard, Mode::Signed, Mode::Encrypted];

    pub fn id(self) -> &'static str {
        match self {
            Mode::Standard => "standard",
            Mode::Signed => "signed",
            Mode::Encrypted => "encrypted",
        }
    }

    pub fn parse(s: &str) -> Option<Mode> {
        match s.trim().to_ascii_lowercase().as_str() {
            "standard" | "none" | "plain" => Some(Mode::Standard),
            "signed" => Some(Mode::Signed),
            "encrypted" | "signed+encrypted" | "signed-encrypted" => Some(Mode::Encrypted),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Mode::Standard => "Standard",
            Mode::Signed => "Signed",
            Mode::Encrypted => "Signed + Encrypted",
        }
    }

    fn byte(self) -> u8 {
        match self {
            Mode::Standard => 0,
            Mode::Signed => 1,
            Mode::Encrypted => 2,
        }
    }

    fn from_byte(b: u8) -> Option<Mode> {
        match b {
            0 => Some(Mode::Standard),
            1 => Some(Mode::Signed),
            2 => Some(Mode::Encrypted),
            _ => None,
        }
    }
}

/// Why a sealed pack was rejected (shown to players and in tests).
#[derive(Debug, PartialEq, Eq)]
pub enum Rejected {
    Missing,
    NotSealed,
    UnsupportedVersion(u16),
    WrongMode { expected: Mode, found: Mode },
    WrongPublisher,
    BadSignature,
    Damaged,
    CannotDecrypt,
}

impl std::fmt::Display for Rejected {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Rejected::Missing => write!(f, "the game data ({SEALED_PACK}) is missing"),
            Rejected::NotSealed => write!(f, "the game data isn't a signed Kit pack"),
            Rejected::UnsupportedVersion(v) => {
                write!(f, "the game data uses protection version {v}, which this player doesn't know")
            }
            Rejected::WrongMode { expected, found } => {
                write!(f, "the game data is {} but this game requires {}", found.label(), expected.label())
            }
            Rejected::WrongPublisher => write!(f, "the game data was signed by a different publisher"),
            Rejected::BadSignature => write!(f, "the game data's signature is invalid (the files were changed)"),
            Rejected::Damaged => write!(f, "the game data is damaged"),
            Rejected::CannotDecrypt => write!(f, "the game data can't be decrypted"),
        }
    }
}

impl std::error::Error for Rejected {}

/// A parsed (not yet trusted) envelope. (The nonce and header are only read
/// when decrypting.)
#[cfg_attr(not(feature = "decrypt"), allow(dead_code))]
struct Envelope<'a> {
    version: u16,
    mode: Mode,
    key_id: [u8; 8],
    nonce: [u8; 24],
    metadata: &'a [u8],
    /// Everything the signature covers.
    signed: &'a [u8],
    /// Everything before the payload (AEAD associated data).
    header: &'a [u8],
    payload: &'a [u8],
    signature: [u8; 64],
}

fn parse(bytes: &[u8]) -> std::result::Result<Envelope<'_>, Rejected> {
    if !bytes.starts_with(SEAL_MAGIC) {
        return Err(Rejected::NotSealed);
    }
    if bytes.len() < HEADER_FIXED + 4 + 8 + SIGNATURE_LEN {
        return Err(Rejected::Damaged);
    }
    let version = u16::from_le_bytes([bytes[8], bytes[9]]);
    if version != SEAL_VERSION {
        return Err(Rejected::UnsupportedVersion(version));
    }
    let mode = Mode::from_byte(bytes[10]).filter(|m| *m != Mode::Standard).ok_or(Rejected::Damaged)?;
    let key_id: [u8; 8] = bytes[12..20].try_into().unwrap();
    let nonce: [u8; 24] = bytes[20..44].try_into().unwrap();
    let mut at = HEADER_FIXED;
    let meta_len = u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()) as usize;
    at += 4;
    let metadata = bytes.get(at..at.checked_add(meta_len).ok_or(Rejected::Damaged)?).ok_or(Rejected::Damaged)?;
    at += meta_len;
    let payload_len = u64::from_le_bytes(bytes.get(at..at + 8).ok_or(Rejected::Damaged)?.try_into().unwrap());
    at += 8;
    let header_end = at;
    let payload_len = usize::try_from(payload_len).map_err(|_| Rejected::Damaged)?;
    let end = at.checked_add(payload_len).ok_or(Rejected::Damaged)?;
    if end.checked_add(SIGNATURE_LEN) != Some(bytes.len()) {
        return Err(Rejected::Damaged);
    }
    Ok(Envelope {
        version,
        mode,
        key_id,
        nonce,
        metadata,
        signed: &bytes[..end],
        header: &bytes[..header_end],
        payload: &bytes[at..end],
        signature: bytes[end..].try_into().unwrap(),
    })
}

#[cfg(feature = "verify")]
pub fn key_id(public_key: &[u8; 32]) -> [u8; 8] {
    use sha2::Digest;
    sha2::Sha256::digest(public_key)[..8].try_into().unwrap()
}

/// Short, readable fingerprint of a publisher key (for the editor and logs).
#[cfg(feature = "verify")]
pub fn fingerprint(public_key: &[u8; 32]) -> String {
    key_id(public_key)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<Vec<_>>()
        .chunks(2)
        .map(|c| c.concat())
        .collect::<Vec<_>>()
        .join("-")
}

/// Verify a sealed pack against the pinned publisher key and required mode,
/// and return the game pack bytes. The signature is checked before anything
/// is decrypted or parsed. `content_key` is required for encrypted packs.
#[cfg(feature = "verify")]
pub fn open(
    sealed: &[u8],
    publisher: &[u8; 32],
    required: Mode,
    content_key: Option<&[u8; 32]>,
) -> std::result::Result<Vec<u8>, Rejected> {
    use ed25519_dalek::{Signature, VerifyingKey};
    let env = parse(sealed)?;
    let _ = (env.version, env.metadata);
    if env.mode != required {
        return Err(Rejected::WrongMode { expected: required, found: env.mode });
    }
    if env.key_id != key_id(publisher) {
        return Err(Rejected::WrongPublisher);
    }
    let key = VerifyingKey::from_bytes(publisher).map_err(|_| Rejected::WrongPublisher)?;
    key.verify_strict(env.signed, &Signature::from_bytes(&env.signature)).map_err(|_| Rejected::BadSignature)?;
    match env.mode {
        Mode::Signed => Ok(env.payload.to_vec()),
        Mode::Encrypted => decrypt(&env, content_key.ok_or(Rejected::CannotDecrypt)?),
        Mode::Standard => Err(Rejected::NotSealed),
    }
}

#[cfg(feature = "decrypt")]
fn decrypt(env: &Envelope, content_key: &[u8; 32]) -> std::result::Result<Vec<u8>, Rejected> {
    use chacha20poly1305::aead::{Aead, KeyInit, Payload};
    use chacha20poly1305::{XChaCha20Poly1305, XNonce};
    let cipher = XChaCha20Poly1305::new(content_key.into());
    cipher
        .decrypt(XNonce::from_slice(&env.nonce), Payload { msg: env.payload, aad: env.header })
        .map_err(|_| Rejected::CannotDecrypt)
}

#[cfg(all(feature = "verify", not(feature = "decrypt")))]
fn decrypt(_env: &Envelope, _content_key: &[u8; 32]) -> std::result::Result<Vec<u8>, Rejected> {
    Err(Rejected::CannotDecrypt)
}

// ------------------------------------------------------------- trust slot

/// The launcher's pinned trust: the required mode, the publisher's public
/// key, and (encrypted builds) the content key.
///
/// It lives in a reserved block of the player binary that starts with
/// [`trust_slot_magic`]; an unprotected player has zeros there. The exporter
/// writes the slot in the launcher it ships ([`write_trust_slot`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Trust {
    pub mode: Mode,
    pub publisher: [u8; 32],
    pub content_key: Option<[u8; 32]>,
}

/// Start of the reserved trust block in a player binary. Kept in two halves
/// so the only contiguous copy in a player is the slot itself.
const MAGIC_A: &[u8; 10] = b"KIT-TRUST-";
const MAGIC_B: &[u8; 6] = b"SLOT:1";

/// The 16-byte marker at the start of a trust block.
pub fn trust_slot_magic() -> [u8; 16] {
    let mut m = [0u8; 16];
    m[..10].copy_from_slice(MAGIC_A);
    m[10..].copy_from_slice(MAGIC_B);
    m
}
/// Size of the whole block (magic included).
pub const TRUST_SLOT_LEN: usize = 128;

/// The block a player embeds (all zeros after the magic = unprotected).
pub const fn empty_trust_slot() -> [u8; TRUST_SLOT_LEN] {
    let mut b = [0u8; TRUST_SLOT_LEN];
    let mut i = 0;
    while i < 10 {
        b[i] = MAGIC_A[i];
        i += 1;
    }
    while i < 16 {
        b[i] = MAGIC_B[i - 10];
        i += 1;
    }
    b
}

/// Read a trust block (as embedded in a launcher). `None` = unprotected.
pub fn read_trust_slot(slot: &[u8; TRUST_SLOT_LEN]) -> Option<Trust> {
    if &slot[..10] != MAGIC_A || &slot[10..16] != MAGIC_B {
        return None;
    }
    let mode = Mode::from_byte(slot[16])?;
    if mode == Mode::Standard {
        return None;
    }
    let publisher: [u8; 32] = slot[17..49].try_into().unwrap();
    let content_key: [u8; 32] = slot[49..81].try_into().unwrap();
    Some(Trust { mode, publisher, content_key: (mode == Mode::Encrypted).then_some(content_key) })
}

/// Write `trust` into every trust block of a player binary (a macOS universal
/// binary has one per architecture). Fails if the binary has none.
pub fn write_trust_slot(binary: &mut [u8], trust: &Trust) -> Result<usize> {
    let magic = trust_slot_magic();
    let mut found = 0;
    let mut at = 0;
    while let Some(i) = find(&binary[at..], &magic) {
        let start = at + i;
        at = start + 16;
        // Only a fresh slot (zeros after the marker) is written.
        if start + TRUST_SLOT_LEN > binary.len() || binary[start + 16..start + TRUST_SLOT_LEN].iter().any(|b| *b != 0) {
            continue;
        }
        let slot = &mut binary[start..start + TRUST_SLOT_LEN];
        slot[16] = trust.mode.byte();
        slot[17..49].copy_from_slice(&trust.publisher);
        slot[49..81].copy_from_slice(&trust.content_key.unwrap_or([0; 32]));
        found += 1;
        at = start + TRUST_SLOT_LEN;
    }
    ensure!(
        found > 0,
        "this game player has no empty trust slot (it's from an older Kit, or already protected), so it can't run protected exports"
    );
    Ok(found)
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

// ---------------------------------------------------------------- publish

/// A publisher's signing key. The private half never leaves this struct
/// except through [`PublisherKey::to_file_text`].
#[cfg(feature = "publish")]
pub struct PublisherKey {
    signing: ed25519_dalek::SigningKey,
}

#[cfg(feature = "publish")]
const KEY_FILE_HEADER: &str = "kit-publisher-key-v1";

#[cfg(feature = "publish")]
impl PublisherKey {
    /// A new random key.
    pub fn generate() -> Result<Self> {
        let mut seed = [0u8; 32];
        getrandom::getrandom(&mut seed).map_err(|e| anyhow::anyhow!("no secure randomness: {e}"))?;
        Ok(Self { signing: ed25519_dalek::SigningKey::from_bytes(&seed) })
    }

    pub fn public(&self) -> [u8; 32] {
        self.signing.verifying_key().to_bytes()
    }

    pub fn fingerprint(&self) -> String {
        fingerprint(&self.public())
    }

    /// Key file contents (private!).
    pub fn to_file_text(&self) -> String {
        use base64::Engine;
        let seed = base64::engine::general_purpose::STANDARD.encode(self.signing.to_bytes());
        format!(
            "{KEY_FILE_HEADER}\n# Kit publisher signing key. Keep it private and backed up.\n# public: {}\n{seed}\n",
            fingerprint(&self.public())
        )
    }

    pub fn from_file_text(text: &str) -> Result<Self> {
        use base64::Engine;
        let mut lines = text.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#'));
        ensure!(lines.next() == Some(KEY_FILE_HEADER), "not a Kit publisher key file");
        let seed = lines.next().context("the key file is empty")?;
        let bytes = base64::engine::general_purpose::STANDARD.decode(seed).context("the key file is damaged")?;
        let seed: [u8; 32] = bytes.try_into().map_err(|_| anyhow::anyhow!("the key file is damaged"))?;
        Ok(Self { signing: ed25519_dalek::SigningKey::from_bytes(&seed) })
    }

    /// Load from a key file. Refuses files other users can read (Unix).
    pub fn load(path: &std::path::Path) -> Result<Self> {
        let text =
            std::fs::read_to_string(path).with_context(|| format!("reading the publisher key {}", path.display()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(path)?.permissions().mode();
            ensure!(
                mode & 0o077 == 0,
                "the publisher key {} can be read by other users; run: chmod 600 {}",
                path.display(),
                path.display()
            );
        }
        Self::from_file_text(&text)
    }

    /// Save to a key file readable only by its owner (never overwrites).
    pub fn save(&self, path: &std::path::Path) -> Result<()> {
        use std::io::Write;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(path).with_context(|| format!("creating {}", path.display()))?;
        file.write_all(self.to_file_text().as_bytes())?;
        Ok(())
    }

    pub(crate) fn sign(&self, message: &[u8]) -> [u8; 64] {
        use ed25519_dalek::Signer;
        self.signing.sign(message).to_bytes()
    }
}

/// A fresh random key for one encrypted export.
#[cfg(feature = "publish")]
pub fn new_content_key() -> Result<[u8; 32]> {
    let mut key = [0u8; 32];
    getrandom::getrandom(&mut key).map_err(|e| anyhow::anyhow!("no secure randomness: {e}"))?;
    Ok(key)
}

/// Seal a game pack. Encrypted mode needs a content key and uses a fresh
/// random nonce on every call.
#[cfg(feature = "publish")]
pub fn seal(
    pack: &[u8],
    key: &PublisherKey,
    mode: Mode,
    content_key: Option<&[u8; 32]>,
    metadata: &str,
) -> Result<Vec<u8>> {
    ensure!(mode != Mode::Standard, "Standard exports aren't sealed");
    let mut nonce = [0u8; 24];
    if mode == Mode::Encrypted {
        getrandom::getrandom(&mut nonce).map_err(|e| anyhow::anyhow!("no secure randomness: {e}"))?;
    }
    let mut out = Vec::with_capacity(pack.len() + 256);
    out.extend_from_slice(SEAL_MAGIC);
    out.extend_from_slice(&SEAL_VERSION.to_le_bytes());
    out.push(mode.byte());
    out.push(0);
    out.extend_from_slice(&key_id(&key.public()));
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&u32::try_from(metadata.len())?.to_le_bytes());
    out.extend_from_slice(metadata.as_bytes());
    let payload = match mode {
        Mode::Encrypted => {
            use chacha20poly1305::aead::{Aead, KeyInit, Payload};
            use chacha20poly1305::{XChaCha20Poly1305, XNonce};
            let content_key = content_key.context("encrypted exports need a content key")?;
            // Associated data: everything before the payload, including its length.
            let ciphertext_len = pack.len() + 16;
            let mut aad = out.clone();
            aad.extend_from_slice(&(ciphertext_len as u64).to_le_bytes());
            XChaCha20Poly1305::new(content_key.into())
                .encrypt(XNonce::from_slice(&nonce), Payload { msg: pack, aad: &aad })
                .map_err(|_| anyhow::anyhow!("encryption failed"))?
        }
        _ => pack.to_vec(),
    };
    out.extend_from_slice(&(payload.len() as u64).to_le_bytes());
    out.extend_from_slice(&payload);
    let signature = key.sign(&out);
    out.extend_from_slice(&signature);
    Ok(out)
}

#[cfg(all(test, feature = "publish"))]
mod tests {
    use super::*;

    const PACK: &[u8] = b"KITPACK1\x01\0\0\0\x0b\0\0\0kit.project\x04\0\0\0demo";

    #[test]
    fn signed_packs_open_only_unchanged_and_with_the_pinned_key() {
        let key = PublisherKey::generate().unwrap();
        let sealed = seal(PACK, &key, Mode::Signed, None, "title=Demo").unwrap();
        assert_eq!(open(&sealed, &key.public(), Mode::Signed, None).unwrap(), PACK);
        // Every single-byte change is rejected (header, metadata, payload, signature).
        for i in 0..sealed.len() {
            let mut bad = sealed.clone();
            bad[i] ^= 0x01;
            assert!(open(&bad, &key.public(), Mode::Signed, None).is_err(), "byte {i} changed but accepted");
        }
        // Another publisher's key.
        let other = PublisherKey::generate().unwrap();
        assert_eq!(open(&sealed, &other.public(), Mode::Signed, None), Err(Rejected::WrongPublisher));
        // A pack re-signed by someone else, with the right key id forged, still fails.
        let mut forged = seal(PACK, &other, Mode::Signed, None, "").unwrap();
        forged[12..20].copy_from_slice(&key_id(&key.public()));
        assert_eq!(open(&forged, &key.public(), Mode::Signed, None), Err(Rejected::BadSignature));
        // Unsigned replacements and missing data.
        assert_eq!(open(PACK, &key.public(), Mode::Signed, None), Err(Rejected::NotSealed));
        assert_eq!(open(b"", &key.public(), Mode::Signed, None), Err(Rejected::NotSealed));
        // The launcher pins the mode: a signed pack can't stand in for an encrypted one.
        assert!(matches!(open(&sealed, &key.public(), Mode::Encrypted, None), Err(Rejected::WrongMode { .. })));
        // Truncation.
        assert!(open(&sealed[..sealed.len() - 1], &key.public(), Mode::Signed, None).is_err());
    }

    #[test]
    fn encrypted_packs_hide_the_payload_and_use_fresh_nonces() {
        let key = PublisherKey::generate().unwrap();
        let content = new_content_key().unwrap();
        let a = seal(PACK, &key, Mode::Encrypted, Some(&content), "").unwrap();
        let b = seal(PACK, &key, Mode::Encrypted, Some(&content), "").unwrap();
        assert_ne!(a[20..44], b[20..44], "fresh nonce per export");
        assert_ne!(a, b);
        for sealed in [&a, &b] {
            assert!(sealed.windows(11).all(|w| w != b"kit.project"), "no plaintext file names");
            assert_eq!(open(sealed, &key.public(), Mode::Encrypted, Some(&content)).unwrap(), PACK);
        }
        // Signature checked before decrypting: a changed byte is a signature error.
        let mut bad = a.clone();
        let last_payload = a.len() - 65;
        bad[last_payload] ^= 1;
        assert_eq!(open(&bad, &key.public(), Mode::Encrypted, Some(&content)), Err(Rejected::BadSignature));
        // The wrong content key can't decrypt (after a valid signature).
        let wrong = new_content_key().unwrap();
        assert_eq!(open(&a, &key.public(), Mode::Encrypted, Some(&wrong)), Err(Rejected::CannotDecrypt));
        assert_eq!(open(&a, &key.public(), Mode::Encrypted, None), Err(Rejected::CannotDecrypt));
    }

    #[test]
    fn trust_slots_round_trip() {
        let mut binary = b"....".to_vec();
        binary.extend_from_slice(&empty_trust_slot());
        binary.extend_from_slice(b"....");
        binary.extend_from_slice(&empty_trust_slot());
        let slot = |b: &[u8], i: usize| -> [u8; TRUST_SLOT_LEN] { b[i..i + TRUST_SLOT_LEN].try_into().unwrap() };
        assert_eq!(read_trust_slot(&slot(&binary, 4)), None, "unprotected player");
        let trust = Trust { mode: Mode::Encrypted, publisher: [7; 32], content_key: Some([9; 32]) };
        assert_eq!(write_trust_slot(&mut binary, &trust).unwrap(), 2, "every architecture's slot");
        assert_eq!(read_trust_slot(&slot(&binary, 4)), Some(trust));
        assert_eq!(read_trust_slot(&slot(&binary, 4 + TRUST_SLOT_LEN + 4)), Some(trust));
        assert!(write_trust_slot(&mut b"no slot here".to_vec(), &trust).is_err());
        assert!(write_trust_slot(&mut binary.clone(), &trust).is_err(), "already-protected slots aren't rewritten");
    }

    #[test]
    fn key_files_round_trip_and_stay_private() {
        let dir = std::env::temp_dir().join(format!("kit_keys_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("publisher.key");
        let key = PublisherKey::generate().unwrap();
        key.save(&path).unwrap();
        assert!(key.save(&path).is_err(), "never overwrites a key");
        let loaded = PublisherKey::load(&path).unwrap();
        assert_eq!(loaded.public(), key.public());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
            assert!(PublisherKey::load(&path).is_err(), "refuses a key others can read");
        }
        assert!(PublisherKey::from_file_text("hello").is_err());
    }
}
