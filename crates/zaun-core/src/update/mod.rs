//! Update checks and self-update (ADR-0040).
//!
//! Layer 5; reaches the network only through `artwork::http` ([`UPDATE_HOSTS`]).
//!
//! - [`source`]: the repository, the hosts, the trusted keys and the limits.
//! - [`manifest`]: the signed release manifest ([`verify`]).
//! - [`check`]: fetch and verify the newest release.
//! - [`install`]: how this copy was installed; an owned install's folders.
//! - [`stage`], [`apply`], [`trial`]: download, put in place, confirm or roll back.
//! - [`state`]: `<state>/update.json`, the throttle and what was announced.

pub mod apply;
pub mod check;
mod deps;
mod error;
pub mod install;
pub mod manifest;
pub mod source;
pub mod stage;
pub mod state;
pub mod trial;
pub mod version;

pub use deps::UpdateDeps;
pub use error::UpdateError;
pub use install::{Install, Layout, detect};
pub use manifest::{Artifact, Release, verify};
pub use source::{TRUSTED_KEYS, UPDATE_HOSTS};
pub use state::UpdateState;
pub use version::Version;
