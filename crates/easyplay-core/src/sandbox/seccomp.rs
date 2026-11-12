//! The seccomp deny-list program (BUBBLEWRAP_SANDBOX §9). Not part of milestone M4: the
//! planner already reserves descriptor 7 and emits `--seccomp 7` when the filter applies;
//! building the BPF program lands with V-009 (feature `seccomp`).

use crate::error::Error;

/// Build the classic-BPF program for `--seccomp`.
pub fn build() -> Result<Vec<u8>, Error> {
    Err(Error::not_available("SANDBOX-seccomp"))
}
