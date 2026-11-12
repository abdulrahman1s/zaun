//! Locate `umu-run` and resolve the interpreter closure that must be visible in the sandbox
//! (UMU_INTEGRATION §2).
//!
//! The algorithm is filesystem-only and runs inside the host probe (layer 3), so it lives in
//! `host::umu_probe`; this module is its public face for the umu layer and adds the helpers
//! the command builder needs. The version check (step 5) is the probe's job because this
//! module never executes `umu-run`.

use std::path::{Path, PathBuf};

pub use crate::host::umu_probe::{
    LocateError, LocateInput, Located, Shebang, check_closure_entry, exec_target, find_candidate,
    locate, parse_shebang, parse_umu_version,
};
use crate::host::{FsLayout, UmuFacts};

/// Closure entries not already covered by the layout's system binds, in order: what
/// BUBBLEWRAP_SANDBOX §4.14 binds read-only at the identical path.
pub fn binds_needed(umu: &UmuFacts, layout: &FsLayout) -> Vec<PathBuf> {
    let roots = layout.system_roots();
    umu.closure
        .iter()
        .filter(|p| !roots.iter().any(|r| p.starts_with(r)))
        .cloned()
        .collect()
}

/// The directory of `umu-run`, which table A appends to the sandbox `PATH` when missing.
pub fn program_dir(umu: &UmuFacts) -> Option<&Path> {
    umu.path.parent()
}
