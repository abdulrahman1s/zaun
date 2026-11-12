//! Self-check and the shareable report.
//!
//! Layer 5. See MODULES §6 and ERROR_HANDLING §6–7. The host checks themselves stay in
//! `host::doctor` (the runtime check D-15 is built by `runtime`, which must not depend on
//! `diagnostics`); [`doctor`] assembles the ordered report from both. The redacted report
//! archive (`report.rs`) is not implemented yet.

pub mod doctor;

pub use doctor::{doctor_report, runtime_check};
