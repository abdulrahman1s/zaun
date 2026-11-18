//! Zaun core: everything that is not UI.
//!
//! Modules form a layered DAG (ARCHITECTURE §4). A module may only import modules listed in
//! its "Depends on" row in MODULES; lower layers never import higher ones.
//!
//! | Layer | Modules |
//! |---|---|
//! | 1 | [`consts`], [`ids`], [`clock`], [`paths`], [`error`], [`model`], [`ui_thread`] |
//! | 2 | [`jobs`], [`logging`], [`fsx`], [`procfs`] |
//! | 3 | [`storage`], [`config`], [`host`] |
//! | 4 | [`identity`], [`sandbox`], [`umu`] |
//! | 5 | [`runtime`], [`launch`], [`session`], [`library`], [`stats`], [`artwork`], [`desktop`], [`diagnostics`] |
//! | 6 | [`service`] |
//!
//! `unsafe` is denied crate-wide; only `session::sys` and `sandbox::fd` may allow it
//! (ARCHITECTURE §3 rule 5), each block with a `// SAFETY:` comment.

#[macro_use]
mod macros;

// Layer 1 — vocabulary
pub mod clock;
pub mod consts;
pub mod error;
pub mod ids;
pub mod model;
pub mod paths;
pub mod ui_thread;

// Layer 2 — mechanisms
pub mod fsx;
pub mod jobs;
pub mod logging;
pub mod procfs;

// Layer 3 — state and facts
pub mod config;
pub mod host;
pub mod steam;
pub mod storage;

// Layer 4 — pure planning and recognition
pub mod identity;
pub mod sandbox;
pub mod umu;

// Layer 5 — use cases
pub mod artwork;
pub mod chat;
pub mod desktop;
pub mod diagnostics;
pub mod gifs;
pub mod launch;
pub mod library;
pub mod linkcheck;
pub mod runtime;
pub mod session;
pub mod stats;

// Layer 6 — application façade
pub mod service;

pub use error::{Error, ErrorCode, Result};
