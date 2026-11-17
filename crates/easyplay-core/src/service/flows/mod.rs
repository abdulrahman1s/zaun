//! The flows of LOGIC_SPECS: one folder or file per user action.

pub mod accent;
pub mod artwork;
pub mod availability;
pub mod chat;
pub mod chat_services;
pub mod debug;
pub mod desktop;
pub mod games;
pub mod launch;
pub mod maintenance;
pub mod open;
pub mod programs;
pub mod queries;
pub mod remove;
pub mod runtime;
pub mod settings;
pub mod setup;
mod state;
pub mod stats;
pub mod steam;
pub mod tools;

pub use launch::not_installed;
pub(crate) use state::{FlowDone, FlowState};
