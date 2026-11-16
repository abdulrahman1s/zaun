//! The three presets (SECURITY_MODEL §6), classification and widening detection
//! (LOGIC_SPECS §5).

use serde::{Deserialize, Serialize};

use crate::consts::POLICY_SCHEMA_VERSION;
use crate::host::HostFacts;
use crate::model::{
    AudioAccess, BasePreset, ControllerAccess, DeviceAccess, DisplayAccess, DisplayIsolation,
    FolderAccess, GameDirAccess, Hardening, IpcAccess, NetworkAccess, Preset, RemovableAccess,
    RuntimeIsolation, SandboxPolicy, SaveDataLocation, SeccompMode, SessionBusAccess, SocketAccess,
};

fn base(
    preset: Preset,
    network: NetworkAccess,
    controllers: ControllerAccess,
    mask: bool,
) -> SandboxPolicy {
    SandboxPolicy {
        schema_version: POLICY_SCHEMA_VERSION,
        preset,
        network,
        game_dir: GameDirAccess::ReadWrite,
        folders: Vec::new(),
        save_data: SaveDataLocation::Internal,
        removable: RemovableAccess::None,
        devices: DeviceAccess {
            audio: AudioAccess::Playback,
            alsa_devices: false,
            controllers,
            camera: false,
        },
        display: DisplayAccess {
            wayland: SocketAccess::Auto,
            x11: SocketAccess::Auto,
            isolation: DisplayIsolation::None,
        },
        ipc: IpcAccess {
            session_bus: SessionBusAccess::None,
        },
        hardening: Hardening {
            seccomp: SeccompMode::Default,
            mask_hostname: true,
            mask_machine_id: mask,
            mask_hardware_ids: mask,
            runtime_isolation: RuntimeIsolation::Auto,
        },
        exceptions: Vec::new(),
    }
}

/// *Offline*: no network, own folder only.
pub fn offline() -> SandboxPolicy {
    base(
        Preset::Offline,
        NetworkAccess::None,
        ControllerAccess::Detected,
        true,
    )
}

/// *Online*: network, own folder only.
pub fn online() -> SandboxPolicy {
    base(
        Preset::Online,
        NetworkAccess::Full,
        ControllerAccess::Detected,
        true,
    )
}

/// *Standard*: network, all input devices when safe on this host, no identity masking.
pub fn standard(host: &HostFacts) -> SandboxPolicy {
    let controllers = if host.input.keyboards_readable {
        ControllerAccess::Detected
    } else {
        ControllerAccess::AllInput
    };
    base(Preset::Standard, NetworkAccess::Full, controllers, false)
}

/// The policy of `preset` on `host`.
pub fn preset_policy(preset: BasePreset, host: &HostFacts) -> SandboxPolicy {
    match preset {
        BasePreset::Offline => offline(),
        BasePreset::Online => online(),
        BasePreset::Standard => standard(host),
    }
}

/// Whether `policy` equals preset `p` in every field except `preset`. `game_dir` may be
/// `ReadWrite` or `FileOnly` (forced for loose executables); Standard's controllers may be
/// `AllInput` or `Detected` (the host-dependent downgrade); the microphone may be on or off
/// (no preset decides it, ADR-0026).
fn matches(policy: &SandboxPolicy, p: BasePreset) -> bool {
    let reference = match p {
        BasePreset::Offline => offline(),
        BasePreset::Online => online(),
        BasePreset::Standard => base(
            Preset::Standard,
            NetworkAccess::Full,
            ControllerAccess::AllInput,
            false,
        ),
    };
    let mut candidate = policy.clone();
    candidate.preset = reference.preset;
    candidate.schema_version = reference.schema_version;
    if candidate.game_dir == GameDirAccess::FileOnly {
        candidate.game_dir = GameDirAccess::ReadWrite;
    }
    if candidate.devices.audio == AudioAccess::Full {
        candidate.devices.audio = AudioAccess::Playback;
    }
    if p == BasePreset::Standard && candidate.devices.controllers == ControllerAccess::Detected {
        candidate.devices.controllers = ControllerAccess::AllInput;
    }
    candidate == reference
}

/// The preset `policy` matches, or `Custom` based on the preset it was derived from.
pub fn classify(policy: &SandboxPolicy) -> Preset {
    let own = policy.preset.base();
    if matches(policy, own) {
        return own.into();
    }
    BasePreset::ALL
        .into_iter()
        .find(|p| matches(policy, *p))
        .map_or(Preset::Custom { base: own }, Preset::from)
}

/// Switch `policy` to `preset`, keeping the game-folder kind, folder grants, the save
/// location and the microphone.
pub fn apply_preset(policy: &SandboxPolicy, preset: BasePreset, host: &HostFacts) -> SandboxPolicy {
    let mut out = preset_policy(preset, host);
    if policy.devices.audio == AudioAccess::Full && out.devices.audio != AudioAccess::None {
        out.devices.audio = AudioAccess::Full;
    }
    out.game_dir = policy.game_dir;
    out.folders = policy.folders.clone();
    out.save_data = policy.save_data.clone();
    out.preset = classify(&out);
    if out.preset.is_custom() {
        out.preset = Preset::Custom { base: preset };
    }
    out
}

/// One way a change widens access (LOGIC_SPECS §5).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Widening {
    /// Network None → Full.
    Network,
    /// A new folder grant (canonical path).
    NewGrant(std::path::PathBuf),
    /// A grant became read-write.
    GrantWritable(std::path::PathBuf),
    /// The game folder became writable (`ReadOnly`/`FileOnly` → `ReadWrite`/`Protected`).
    GameDirWritable,
    /// External save folder newly chosen or changed.
    SaveData,
    /// Removable media access increased.
    Removable,
    /// Sound turned on.
    Audio,
    /// The microphone turned on.
    Microphone,
    /// Raw ALSA devices turned on.
    AlsaDevices,
    /// Controllers widened (to `Detected` or `AllInput`).
    Controllers,
    /// Camera turned on.
    Camera,
    /// A display socket allowed.
    Display,
    /// Seccomp turned off.
    SeccompOff,
    /// A masking switch turned off.
    MaskingOff,
    /// Runtime isolation weakened to Shared.
    RuntimeShared,
    /// A new exception.
    Exception,
}

/// Every widening from `old` to `new`.
pub fn widening(old: &SandboxPolicy, new: &SandboxPolicy) -> Vec<Widening> {
    let mut w = Vec::new();
    if old.network == NetworkAccess::None && new.network == NetworkAccess::Full {
        w.push(Widening::Network);
    }
    for g in &new.folders {
        match old.folders.iter().find(|o| o.canonical == g.canonical) {
            None => w.push(Widening::NewGrant(g.canonical.clone())),
            Some(o)
                if o.access == FolderAccess::ReadOnly && g.access == FolderAccess::ReadWrite =>
            {
                w.push(Widening::GrantWritable(g.canonical.clone()));
            }
            Some(_) => {}
        }
    }
    let writable =
        |d: GameDirAccess| matches!(d, GameDirAccess::ReadWrite | GameDirAccess::Protected);
    if !writable(old.game_dir) && writable(new.game_dir) {
        w.push(Widening::GameDirWritable);
    }
    if let SaveDataLocation::External { .. } = &new.save_data
        && new.save_data != old.save_data
    {
        w.push(Widening::SaveData);
    }
    let rank_removable = |r: RemovableAccess| match r {
        RemovableAccess::None => 0,
        RemovableAccess::ReadOnly => 1,
        RemovableAccess::ReadWrite => 2,
    };
    if rank_removable(new.removable) > rank_removable(old.removable) {
        w.push(Widening::Removable);
    }
    if old.devices.audio == AudioAccess::None && new.devices.audio != AudioAccess::None {
        w.push(Widening::Audio);
    }
    if old.devices.audio != AudioAccess::Full && new.devices.audio == AudioAccess::Full {
        w.push(Widening::Microphone);
    }
    if !old.devices.alsa_devices && new.devices.alsa_devices {
        w.push(Widening::AlsaDevices);
    }
    let rank_ctl = |c: ControllerAccess| match c {
        ControllerAccess::None => 0,
        ControllerAccess::Detected => 1,
        ControllerAccess::AllInput => 2,
    };
    if rank_ctl(new.devices.controllers) > rank_ctl(old.devices.controllers) {
        w.push(Widening::Controllers);
    }
    if !old.devices.camera && new.devices.camera {
        w.push(Widening::Camera);
    }
    if (old.display.wayland == SocketAccess::Deny && new.display.wayland == SocketAccess::Auto)
        || (old.display.x11 == SocketAccess::Deny && new.display.x11 == SocketAccess::Auto)
    {
        w.push(Widening::Display);
    }
    if old.hardening.seccomp == SeccompMode::Default && new.hardening.seccomp == SeccompMode::Off {
        w.push(Widening::SeccompOff);
    }
    let (o, n) = (&old.hardening, &new.hardening);
    if (o.mask_hostname && !n.mask_hostname)
        || (o.mask_machine_id && !n.mask_machine_id)
        || (o.mask_hardware_ids && !n.mask_hardware_ids)
    {
        w.push(Widening::MaskingOff);
    }
    if o.runtime_isolation != RuntimeIsolation::Shared
        && n.runtime_isolation == RuntimeIsolation::Shared
    {
        w.push(Widening::RuntimeShared);
    }
    if new.exceptions.iter().any(|e| !old.exceptions.contains(e)) {
        w.push(Widening::Exception);
    }
    w
}
