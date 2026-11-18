//! Updates (ADR-0040): the daily check, "Check now", the download of an owned install,
//! "Restart now" and the release notes. Installing happens outside the service, when the
//! process starts or restarts (`zaun`'s `update_start`).

mod check;
mod download;
mod notes;
mod publish;
mod restart;
mod route;
mod schedule;
mod state;

pub(crate) use state::{UpdatesDone, UpdatesState};
