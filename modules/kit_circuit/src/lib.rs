//! Kit Circuits: installable extensions that connect Kit to services and
//! technologies (multiplayer, storage, analytics…). Circuits aren't engine
//! modules: a project opts in per Circuit, pins exact versions in
//! `circuits.lock`, and can update, downgrade, disable or remove them without
//! touching anything else in the project.
//!
//! - [`manifest`]: the `circuit.toml` format.
//! - [`package`]: deterministic packing, checksum/signature checks, safe unpacking.
//! - [`install`]: atomic project installs with rollback.
//! - [`lock`]: the project lockfile.
//! - [`registry`]: the Circuit API client (discovery, releases, downloads).

pub mod install;
pub mod lock;
pub mod manifest;
pub mod package;
pub mod registry;
pub mod settings;

pub use manifest::Manifest;
pub use package::ReleaseManifest;
