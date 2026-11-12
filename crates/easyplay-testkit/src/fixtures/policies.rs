//! Sample policies and `proptest` generators for sandbox properties (SANDBOX-031).

use std::path::PathBuf;

use easyplay_core::clock::Timestamp;
use easyplay_core::model::{
    AudioAccess, ControllerAccess, FolderAccess, FolderGrant, GameDirAccess, GrantOrigin,
    NetworkAccess, RemovableAccess, RuntimeIsolation, SandboxPolicy, SaveDataLocation, SeccompMode,
    SessionKind, SocketAccess, UserDir,
};
use easyplay_core::sandbox::{classify, offline};
use proptest::prelude::*;

use super::hosts::{self, SandboxWorld};

/// A folder grant at `path` (canonical = path).
pub fn grant(path: &str, access: FolderAccess, letter: Option<char>) -> FolderGrant {
    FolderGrant {
        path: path.into(),
        canonical: path.into(),
        access,
        origin: GrantOrigin::UserChosen,
        drive_letter: letter,
        granted_at: Timestamp::from_millis(1_794_480_000_000),
    }
}

/// Grant pool of [`SandboxWorld`] (all exist and pass the deny-list).
pub const GRANT_POOL: &[(&str, Option<UserDir>)] = &[
    ("/home/sam/Documents", Some(UserDir::Documents)),
    ("/home/sam/Music", Some(UserDir::Music)),
    ("/home/sam/Games/Mods", None),
    ("/mnt/data/shared", None),
    ("/mnt/data/shared/inner", None),
];

/// One generated planning case.
#[derive(Clone, Debug)]
pub struct Case {
    /// Host preset name.
    pub host: &'static str,
    /// Session kind.
    pub kind: SessionKind,
    /// Network override.
    pub network_override: Option<NetworkAccess>,
    /// Loose executable instead of a game folder.
    pub loose: bool,
    /// The policy.
    pub policy: SandboxPolicy,
}

impl Case {
    /// Build the world for this case.
    pub fn world(&self) -> SandboxWorld {
        let mut w = SandboxWorld::new(hosts::by_name(self.host));
        if self.loose {
            w.loose_exe();
        }
        w.set_kind(self.kind, self.network_override);
        w
    }
}

fn arb_kind() -> impl Strategy<Value = (SessionKind, Option<NetworkAccess>)> {
    prop_oneof![
        4 => Just((SessionKind::Game, None)),
        1 => Just((SessionKind::Installer, None)),
        1 => Just((SessionKind::Tool, None)),
        1 => Just((SessionKind::Prepare, None)),
        1 => Just((SessionKind::Prepare, Some(NetworkAccess::Full))),
        1 => Just((SessionKind::Provision, None)),
    ]
}

const HOSTS: &[&str] = &[
    "arch_mesa",
    "fedora_nvidia",
    "ubuntu_lts_apparmor",
    "debian_stable_old_bwrap",
    "nixos",
    "no_userns",
    "x11_only",
    "no_pipewire",
    "input_group_user",
];

/// A valid policy (structured fields only; no exceptions) for the given host.
pub fn arb_policy() -> impl Strategy<Value = SandboxPolicy> {
    let grants = proptest::collection::vec(
        (any::<bool>(), any::<bool>(), any::<bool>()),
        GRANT_POOL.len(),
    );
    (
        (any::<bool>(), 0u8..3, grants, any::<bool>(), 0u8..3),
        (any::<bool>(), any::<bool>(), 0u8..3, any::<bool>(), 0u8..3),
        (
            any::<bool>(),
            any::<bool>(),
            any::<bool>(),
            any::<bool>(),
            0u8..3,
        ),
    )
        .prop_map(
            |(
                (net, game_dir, grants, external_save, removable),
                (audio, alsa, controllers, camera, display),
                (seccomp, mh, mm, mhw, iso),
            )| {
                let mut p = offline();
                p.network = if net {
                    NetworkAccess::Full
                } else {
                    NetworkAccess::None
                };
                p.game_dir = [
                    GameDirAccess::ReadOnly,
                    GameDirAccess::ReadWrite,
                    GameDirAccess::Protected,
                ][usize::from(game_dir)];
                let mut letters = ['d', 'e', 'f', 'g', 'h'].into_iter();
                for ((path, dir), (on, rw, letter)) in GRANT_POOL.iter().zip(grants) {
                    if on {
                        let mut g = grant(
                            path,
                            if rw {
                                FolderAccess::ReadWrite
                            } else {
                                FolderAccess::ReadOnly
                            },
                            if letter { letters.next() } else { None },
                        );
                        if let Some(d) = dir {
                            g.origin = GrantOrigin::WellKnown(*d);
                        }
                        p.folders.push(g);
                    }
                }
                if external_save {
                    p.save_data = SaveDataLocation::External {
                        dir: PathBuf::from("/mnt/saves/hk"),
                    };
                }
                p.removable = [
                    RemovableAccess::None,
                    RemovableAccess::ReadOnly,
                    RemovableAccess::ReadWrite,
                ][usize::from(removable)];
                p.devices.audio = if audio {
                    AudioAccess::Full
                } else {
                    AudioAccess::None
                };
                p.devices.alsa_devices = alsa;
                p.devices.controllers = [
                    ControllerAccess::None,
                    ControllerAccess::Detected,
                    ControllerAccess::AllInput,
                ][usize::from(controllers)];
                p.devices.camera = camera;
                let (w, x) = [
                    (SocketAccess::Auto, SocketAccess::Auto),
                    (SocketAccess::Auto, SocketAccess::Deny),
                    (SocketAccess::Deny, SocketAccess::Auto),
                ][usize::from(display)];
                p.display.wayland = w;
                p.display.x11 = x;
                p.hardening.seccomp = if seccomp {
                    SeccompMode::Default
                } else {
                    SeccompMode::Off
                };
                p.hardening.mask_hostname = mh;
                p.hardening.mask_machine_id = mm;
                p.hardening.mask_hardware_ids = mhw;
                p.hardening.runtime_isolation = [
                    RuntimeIsolation::Auto,
                    RuntimeIsolation::Isolated,
                    RuntimeIsolation::Shared,
                ][usize::from(iso)];
                p.preset = classify(&p);
                p
            },
        )
}

/// A valid planning case: the policy is adjusted so that it is valid on the chosen host
/// (Protected only with overlay support, a display that exists, `FileOnly` for loose files).
pub fn arb_case() -> impl Strategy<Value = Case> {
    (
        proptest::sample::select(HOSTS),
        arb_kind(),
        any::<bool>(),
        arb_policy(),
    )
        .prop_map(|(host, (kind, network_override), loose, mut policy)| {
            let facts = hosts::by_name(host);
            if policy.game_dir == GameDirAccess::Protected && !facts.overlay_ok() {
                policy.game_dir = GameDirAccess::ReadWrite;
            }
            if facts.display.wayland.is_none() {
                policy.display.x11 = SocketAccess::Auto;
            }
            if loose {
                policy.game_dir = GameDirAccess::FileOnly;
            }
            policy.preset = classify(&policy);
            Case {
                host,
                kind,
                network_override,
                loose,
                policy,
            }
        })
}
