//! §4.1 Namespaces and lifecycle.
//!
//! Every namespace flag is spelled out (no `--unshare-all`). The IPC namespace is shared with
//! the host exactly when the X11 socket is bound: Wine and DXVK use MIT-SHM through
//! (X)wayland, which passes SysV shared-memory IDs to the X server; with `--unshare-ipc` the
//! server cannot attach them and games fail with `BadValue … X_ShmPutImage` (field-tested on
//! the development host, TECHNICAL_RESEARCH F-220). The residual risk is listed in
//! SECURITY_MODEL §5.7.

use super::Effective;
use crate::consts::SANDBOX_HOSTNAME;
use crate::model::NetworkAccess;
use crate::sandbox::plan::{FD_JSON_STATUS, FD_SECCOMP, FD_SYNC, Namespaces, PlanBuilder};

/// Options that are never emitted (BUBBLEWRAP_SANDBOX §4.1, §13).
pub const FORBIDDEN_OPTIONS: &[&str] = &[
    "--disable-userns",
    "--share-net",
    "--unshare-all",
    "--uid",
    "--gid",
    "--cap-add",
    "--as-pid-1",
    "--not-a-security-boundary",
    "--userns",
    "--userns2",
    "--pidns",
    "--assert-userns-disabled",
];

/// Build the namespace set and hostname.
pub fn build(eff: &Effective, b: &mut PlanBuilder) -> (Namespaces, Option<String>) {
    if eff.seccomp_requested && !eff.seccomp {
        b.note(
            "note.seccomp.unavailable",
            "The system-call filter is not available in this build",
        );
    }
    if eff.share_ipc {
        b.note(
            "note.ipc.shared",
            "Shared memory is shared with the display server so X11 rendering works",
        );
    }
    let ns = Namespaces {
        user: true,
        ipc: !eff.share_ipc,
        pid: true,
        uts: true,
        cgroup_try: true,
        net: eff.network == NetworkAccess::None,
        die_with_parent: true,
        new_session: true,
        json_status_fd: Some(FD_JSON_STATUS),
        sync_fd: Some(FD_SYNC),
        seccomp_fd: eff.seccomp.then_some(FD_SECCOMP),
    };
    let hostname = eff.mask_hostname.then(|| SANDBOX_HOSTNAME.to_owned());
    (ns, hostname)
}
