//! Update checks and self-update (ADR-0040, ADR-0041).
//!
//! Layer 5; reaches the network only through `artwork::http` ([`UPDATE_HOSTS`]).
//!
//! - [`source`]: the repository, the hosts, the trusted keys and the limits.
//! - [`manifest`]: the signed release manifest ([`verify`]).
//! - [`check`]: fetch and verify the newest release.
//! - [`install`]: how this copy was installed.
//! - [`store`]: the bundle store, its version folders and links.
//! - [`stage`] (with [`archive`]), [`apply`], [`trial`]: download, switch, start and
//!   confirm or roll back.
//! - [`state`]: `<state>/update.json`, the throttle and what was announced.

pub mod apply;
pub mod archive;
pub mod check;
mod deps;
mod error;
pub mod install;
pub mod manifest;
pub mod source;
pub mod stage;
pub mod state;
pub mod store;
pub mod trial;
pub mod version;

pub use deps::UpdateDeps;
pub use error::UpdateError;
pub use install::{Install, detect};
pub use manifest::{Artifact, Release, verify};
pub use source::{TRUSTED_KEYS, UPDATE_HOSTS};
pub use state::UpdateState;
pub use store::Store;
pub use version::Version;
