//! Wayland security-context listener (`wp-security-context-v1`, feature
//! `wayland-security-context`).
//!
//! Not part of 1.0: ROADMAP R-01 and TECHNICAL_RESEARCH V-015 place it after 1.0 (it needs a
//! Wayland client in the supervisor and per-compositor validation). 1.0 binds the
//! compositor socket directly (BUBBLEWRAP_SANDBOX §4.7) and the planner notes that no
//! security context is used; THREAT_MODEL T-15 lists the residual risk. This typed stub keeps
//! the call site and the error stable until then.

use crate::error::Error;

/// Create a security-context socket for one session.
pub fn create_context_socket() -> Result<std::path::PathBuf, Error> {
    Err(Error::not_available("SANDBOX-wayland-security-context"))
}
