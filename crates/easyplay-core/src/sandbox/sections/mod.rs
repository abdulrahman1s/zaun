//! Planner sections, one file per block of BUBBLEWRAP_SANDBOX §4, each a pure function
//! `fn(&Policy, &Context, &HostFacts, &Effective, &mut PlanBuilder)`.
//!
//! [`Effective`] holds the decisions derived once from policy, session kind and host facts
//! (effective network, which sockets are bound, …) so that every section and the plan
//! invariants agree.

pub mod devices;
pub mod env;
pub mod etc_files;
pub mod game;
pub mod grants;
pub mod home;
pub mod namespaces;
pub mod prefix;
pub mod runtime_store;
pub mod sysfs;
pub mod system;
pub mod tmp_run;

use std::path::{Path, PathBuf};

use super::context::SandboxContext;
use crate::host::HostFacts;
use crate::model::{
    AudioAccess, ControllerAccess, NetworkAccess, RuntimeIsolationMode, SandboxPolicy, SeccompMode,
    SessionKind, SocketAccess,
};

/// Decisions derived from policy, kind and host, shared by all sections.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Effective {
    /// Network actually granted.
    pub network: NetworkAccess,
    /// Trusted session with network (runtime store read-write).
    pub trusted_network: bool,
    /// Untrusted kind (policy device and folder fields apply).
    pub untrusted: bool,
    /// Sound sockets.
    pub audio: AudioAccess,
    /// Controller access after the `AllInput` safety downgrade.
    pub controllers: ControllerAccess,
    /// `AllInput` was requested but downgraded to `Detected`.
    pub all_input_downgraded: bool,
    /// `/dev/snd`.
    pub alsa: bool,
    /// `/dev/video*`.
    pub camera: bool,
    /// The Wayland socket is bound.
    pub wayland: bool,
    /// The X11 socket is bound.
    pub x11: bool,
    /// The IPC namespace is shared with the host (X11 MIT-SHM, see namespaces.rs).
    pub share_ipc: bool,
    /// The seccomp filter was requested.
    pub seccomp_requested: bool,
    /// The seccomp filter is applied (requested and available).
    pub seccomp: bool,
    /// `--hostname easyplay`.
    pub mask_hostname: bool,
    /// Synthesized `machine-id`.
    pub mask_machine_id: bool,
    /// DMI and network device masks.
    pub mask_hardware_ids: bool,
    /// Runtime isolation in effect for untrusted or offline sessions.
    pub isolation: Option<RuntimeIsolationMode>,
}

impl Effective {
    /// Compute the decisions.
    pub fn compute(policy: &SandboxPolicy, ctx: &SandboxContext<'_>, host: &HostFacts) -> Self {
        let untrusted = ctx.untrusted();
        let provision = ctx.kind == SessionKind::Provision;
        let network = ctx.network_override.unwrap_or(if provision {
            NetworkAccess::Full
        } else {
            policy.network
        });
        let trusted_network = ctx.trusted_network();
        let (controllers, downgraded) = if !untrusted {
            (ControllerAccess::None, false)
        } else {
            match policy.devices.controllers {
                ControllerAccess::AllInput if host.input.keyboards_readable => {
                    (ControllerAccess::Detected, true)
                }
                c => (c, false),
            }
        };
        let wayland =
            policy.display.wayland == SocketAccess::Auto && host.display.wayland.is_some();
        let x11 = policy.display.x11 == SocketAccess::Auto && host.display.x11_bindable().is_some();
        let seccomp_requested =
            provision || trusted_network || policy.hardening.seccomp == SeccompMode::Default;
        Self {
            network,
            trusted_network,
            untrusted,
            audio: if untrusted {
                policy.devices.audio
            } else {
                AudioAccess::None
            },
            controllers,
            all_input_downgraded: downgraded,
            alsa: untrusted && policy.devices.alsa_devices,
            camera: untrusted && policy.devices.camera,
            wayland,
            x11,
            share_ipc: x11,
            seccomp_requested,
            seccomp: seccomp_requested && ctx.seccomp_available,
            mask_hostname: provision || policy.hardening.mask_hostname,
            mask_machine_id: provision || policy.hardening.mask_machine_id,
            mask_hardware_ids: provision || policy.hardening.mask_hardware_ids,
            isolation: if trusted_network {
                None
            } else {
                ctx.runtime.map(|r| r.isolation)
            },
        }
    }

    /// Whether the sysfs network masks apply.
    pub fn mask_net(&self) -> bool {
        self.network == NetworkAccess::None || self.mask_hardware_ids
    }
}

/// In-sandbox locations derived from the home path (BUBBLEWRAP_SANDBOX §4.9).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SandboxDirs {
    /// `<home>/.local/share/umu`.
    pub umu: PathBuf,
    /// `<home>/.local/share/Steam/compatibilitytools.d`.
    pub compat: PathBuf,
    /// `<home>/.cache/umu`.
    pub cache: PathBuf,
    /// `/run/user/<uid>`.
    pub xrd: PathBuf,
}

impl SandboxDirs {
    /// The locations for `home` and `uid`.
    pub fn new(home: &Path, uid: u32) -> Self {
        Self {
            umu: home.join(".local/share/umu"),
            compat: home.join(".local/share/Steam/compatibilitytools.d"),
            cache: home.join(".cache/umu"),
            xrd: PathBuf::from(format!("/run/user/{uid}")),
        }
    }
}

/// Shorthands for building mounts.
pub mod mk {
    use std::path::{Path, PathBuf};

    use crate::sandbox::plan::{MountKind, MountMode};

    /// `--ro-bind src dst`.
    pub fn ro(src: impl AsRef<Path>, dst: impl AsRef<Path>) -> MountKind {
        bind(src, dst, MountMode::Ro, false)
    }
    /// `--ro-bind-try src dst`.
    pub fn ro_try(src: impl AsRef<Path>, dst: impl AsRef<Path>) -> MountKind {
        bind(src, dst, MountMode::Ro, true)
    }
    /// `--bind src dst`.
    pub fn rw(src: impl AsRef<Path>, dst: impl AsRef<Path>) -> MountKind {
        bind(src, dst, MountMode::Rw, false)
    }
    /// A bind.
    pub fn bind(
        src: impl AsRef<Path>,
        dst: impl AsRef<Path>,
        mode: MountMode,
        try_: bool,
    ) -> MountKind {
        MountKind::Bind {
            src: src.as_ref().to_path_buf(),
            dst: dst.as_ref().to_path_buf(),
            mode,
            try_,
        }
    }
    /// `--ro-bind p p`.
    pub fn ro_same(p: impl AsRef<Path>) -> MountKind {
        ro(&p, &p)
    }
    /// `--dev-bind p p` (or `-try`).
    pub fn dev(p: impl AsRef<Path>, try_: bool) -> MountKind {
        MountKind::DevBind {
            src: p.as_ref().to_path_buf(),
            dst: p.as_ref().to_path_buf(),
            try_,
        }
    }
    /// `--tmpfs dst`.
    pub fn tmpfs(dst: impl AsRef<Path>, perms: Option<u32>, size: Option<u64>) -> MountKind {
        MountKind::Tmpfs {
            dst: dst.as_ref().to_path_buf(),
            size,
            perms,
        }
    }
    /// `--dir dst`.
    pub fn dir(dst: impl AsRef<Path>) -> MountKind {
        MountKind::Dir {
            dst: dst.as_ref().to_path_buf(),
            perms: None,
        }
    }
    /// `--symlink target dst`.
    pub fn symlink(target: impl Into<PathBuf>, dst: impl AsRef<Path>) -> MountKind {
        MountKind::Symlink {
            target: target.into(),
            dst: dst.as_ref().to_path_buf(),
        }
    }
}
