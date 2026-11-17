//! Developer Mode and diagnostics flows (DEV-001, DEV-003, DEV-004, ERR-002, ERR-003, ERR-005) and the maintenance job (SVC-010).
//!
//! Everything that touches the disk or `/proc` runs on the job pool and answers with a
//! `QueryResult` (or, for the Doctor, the diagnostics snapshot).

mod crash;
mod doctor;
mod dry_run;
mod inspect;
pub mod maintenance;
mod processes;
mod read_log;
mod report;
mod sessions;
