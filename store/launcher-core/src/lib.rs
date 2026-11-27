//! Kit Store's platform-neutral Linux launcher core.
//!
//! The crate deliberately contains no GUI. It owns the on-disk transaction:
//! archives are verified and unpacked into a private staging directory, then
//! an `active` symlink is replaced atomically. Save data lives outside version
//! directories and therefore survives updates and uninstall.

mod api;
mod archive;
mod install;

pub use api::{ApiClient, ApiError, CatalogEntry, CatalogResponse, DEFAULT_API, Release, ReleaseFile};
pub use archive::{extract_tar_gz, sha256_hex, verify_sha256};
pub use install::{InstallLayout, InstalledGame, install, launch, uninstall};
