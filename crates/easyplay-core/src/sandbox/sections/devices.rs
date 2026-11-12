//! §4.4 `/proc` and §4.5 `/dev`.
//!
//! Order: `--proc`, `--dev`, GPU, NVIDIA nodes, ntsync, controller nodes, hidraw, ALSA,
//! cameras, `ExtraDevice` exceptions; within a group sorted by path. `/dev/input` as a
//! directory is bound only for `AllInput` when no keyboard or mouse is readable.

use std::path::PathBuf;

use super::{Effective, mk};
use crate::host::{HostFacts, InputNodeKind};
use crate::model::{ControllerAccess, PolicyException, SandboxPolicy};
use crate::sandbox::plan::{MountKind, PlanBuilder, PolicyField};
use crate::sandbox::validate::{IssueCode, PolicyIssue};

/// §4.4–4.5.
pub fn emit(
    policy: &SandboxPolicy,
    host: &HostFacts,
    eff: &Effective,
    developer_mode: bool,
    b: &mut PlanBuilder,
) {
    b.mount(
        MountKind::Proc {
            dst: "/proc".into(),
        },
        "fresh /proc for the sandbox's processes",
        None,
    );
    b.mount(MountKind::Dev { dst: "/dev".into() }, "minimal /dev", None);
    if host.gpu.dri_present {
        b.mount(mk::dev("/dev/dri", false), "GPU", None);
    }
    let mut nvidia = host.gpu.nvidia_nodes.clone();
    nvidia.sort();
    for node in nvidia {
        b.mount(mk::dev(&node, false), "NVIDIA GPU", None);
    }
    if host.gpu.ntsync {
        b.mount(
            mk::dev("/dev/ntsync", true),
            "Wine synchronisation driver",
            None,
        );
    }

    if eff.all_input_downgraded {
        b.issue(PolicyIssue::warning(
            IssueCode::AllInputUnsafe,
            None,
            "sec.input.unsafe|Your account can read keyboards directly; all-device mode would expose them",
        ));
        b.note(
            "note.input.downgraded",
            "All-device controller mode downgraded to detected controllers: keyboards are readable",
        );
    }
    let field = Some(PolicyField::Controllers);
    let joystick = |kind: InputNodeKind| {
        let mut v: Vec<PathBuf> = host
            .input
            .nodes
            .iter()
            .filter(|n| n.kind == kind && n.is_joystick() && n.readable)
            .map(|n| n.path.clone())
            .collect();
        v.sort();
        v
    };
    match eff.controllers {
        ControllerAccess::None => {}
        ControllerAccess::Detected => {
            let mut nodes = joystick(InputNodeKind::Event);
            nodes.extend(joystick(InputNodeKind::Js));
            nodes.sort();
            for n in nodes {
                b.mount(mk::dev(&n, false), "game controller", field.clone());
            }
        }
        ControllerAccess::AllInput => {
            b.mount(
                mk::dev("/dev/input", false),
                "all input devices (no keyboard is readable)",
                field.clone(),
            );
        }
    }
    if eff.controllers != ControllerAccess::None {
        for n in joystick(InputNodeKind::Hidraw) {
            b.mount(
                mk::dev(&n, false),
                "game controller (raw HID)",
                field.clone(),
            );
        }
    }
    if eff.alsa && host.audio.alsa_devices {
        b.mount(
            mk::dev("/dev/snd", false),
            "raw sound devices",
            Some(PolicyField::AlsaDevices),
        );
    }
    if eff.camera {
        let mut video = host.video_nodes.clone();
        video.sort();
        for v in video {
            b.mount(mk::dev(&v, false), "camera", Some(PolicyField::Camera));
        }
    }
    if eff.untrusted && developer_mode {
        let mut extra: Vec<&PathBuf> = policy
            .exceptions
            .iter()
            .filter_map(|e| match e {
                PolicyException::ExtraDevice { path, .. } => Some(path),
                _ => None,
            })
            .collect();
        extra.sort();
        extra.dedup();
        for p in extra {
            b.mount(
                mk::dev(p, false),
                "device exception",
                Some(PolicyField::ExtraDevice(p.clone())),
            );
        }
    }
}
