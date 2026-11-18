//! Runtime commands (SVC-008, LOGIC_SPECS §16, RUNTIME_MANAGEMENT §6.3, §8): one file per verb.
//!
//! Provision runs through the same `launch::LocalTrustedSteps` as the launch pipeline: it
//! takes the store lock and waits while a Shared-mode game session is open, so a Provision
//! never starts during one. Results come back as `Internal::RuntimeJob`; tamper notices
//! (`ZN-RT-020`/`021`) are reported as failures.

mod adopt;
mod ended;
mod job_done;
mod provision;
mod remove;
mod scan;
mod umu_db;
mod verify;

pub(crate) use job_done::RuntimeJobDone;
pub use umu_db::CatalogUmuDb;
