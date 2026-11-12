//! Wayland security-context listener (V-015, feature `wayland-security-context`). Not part
//! of milestone M4: the planner binds the plain Wayland socket and notes that no security
//! context is used.

use crate::error::Error;

/// Create a security-context socket for one session.
pub fn create_context_socket() -> Result<std::path::PathBuf, Error> {
    Err(Error::not_available("SANDBOX-wayland-security-context"))
}
