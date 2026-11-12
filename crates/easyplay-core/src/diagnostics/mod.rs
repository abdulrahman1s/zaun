//! Self-check and the shareable report.
//!
//! Layer 5. See MODULES §6 and ERROR_HANDLING §6–7. The host checks themselves stay in
//! `host::doctor` (the runtime check D-15 is built by `runtime`, which must not depend on
//! `diagnostics`); [`doctor`] assembles the ordered report from both. [`report`] builds the
//! redacted, previewed report archive, [`crash`] the crash records of ERROR_HANDLING §4.11.

pub mod crash;
pub mod doctor;
pub mod evidence;
pub mod report;

pub use doctor::{
    DoctorProbes, complete, database_check, doctor_report, init_check, runtime_check,
};
