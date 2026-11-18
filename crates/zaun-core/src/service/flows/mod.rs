//! The flows of LOGIC_SPECS: one folder or file per user action.

pub mod artwork;
pub mod chat;
pub mod desktop;
pub mod diagnostics;
pub mod games;
pub mod launch;
mod pool;
pub mod runtime;
pub mod settings;
mod state;
pub mod stats;
pub mod steam;
pub mod updates;

pub use launch::not_installed;
pub(crate) use state::{FlowDone, FlowState};
