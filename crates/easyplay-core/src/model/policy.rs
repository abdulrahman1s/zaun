//! The per-game sandbox policy (DATA_MODEL §3, SECURITY_MODEL §5–6).
//!
//! The policy is what the user edits; `sandbox::planner` turns it into a plan. JSON shape:
//! `snake_case` fields, externally tagged `snake_case` enums. Field order is part of the stored
//! format (the example document of DATA_MODEL §3 round-trips byte-identically).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::{Problem, ValidationError};
use crate::clock::Timestamp;
use crate::consts::{MAX_EXCEPTION_REASON_CHARS, POLICY_SCHEMA_VERSION};

/// A complete sandbox policy.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SandboxPolicy {
    /// Document version, [`POLICY_SCHEMA_VERSION`].
    pub schema_version: u32,
    /// The preset this policy matches or is based on. Derived state kept for display:
    /// recomputed by `sandbox::presets::classify` on every save.
    pub preset: Preset,
    /// Network access.
    pub network: NetworkAccess,
    /// Access to the game's own folder.
    pub game_dir: GameDirAccess,
    /// Additional folders.
    pub folders: Vec<FolderGrant>,
    /// Where the Windows user profile (save data) lives.
    pub save_data: SaveDataLocation,
    /// Removable media.
    pub removable: RemovableAccess,
    /// Devices.
    pub devices: DeviceAccess,
    /// Display servers.
    pub display: DisplayAccess,
    /// Inter-process communication.
    pub ipc: IpcAccess,
    /// Hardening switches.
    pub hardening: Hardening,
    /// Narrow explicit exceptions.
    pub exceptions: Vec<PolicyException>,
}

impl SandboxPolicy {
    /// Whether this document's version can be read by this build.
    pub fn is_supported_version(&self) -> bool {
        self.schema_version >= 1 && self.schema_version <= POLICY_SCHEMA_VERSION
    }

    /// Structural validation that needs no host access: versions, drive letters, absolute
    /// paths, exception reasons, reserved values. Deny-list and existence checks are
    /// `sandbox::validate`.
    pub fn validate(&self) -> Result<(), ValidationError> {
        if !self.is_supported_version() {
            return Err(ValidationError::new(
                "policy.schema_version",
                Problem::UnsupportedVersion {
                    found: self.schema_version,
                    max: POLICY_SCHEMA_VERSION,
                },
            ));
        }
        let mut letters = Vec::new();
        for grant in &self.folders {
            require_absolute("policy.folders.path", &grant.path)?;
            require_absolute("policy.folders.canonical", &grant.canonical)?;
            if let Some(letter) = grant.drive_letter {
                if !is_valid_drive_letter(letter) {
                    return Err(ValidationError::new(
                        "policy.folders.drive_letter",
                        Problem::OutOfRange,
                    ));
                }
                if letters.contains(&letter) {
                    return Err(ValidationError::new(
                        "policy.folders.drive_letter",
                        Problem::Duplicate,
                    ));
                }
                letters.push(letter);
            }
        }
        if let SaveDataLocation::External { dir } = &self.save_data {
            require_absolute("policy.save_data.dir", dir)?;
        }
        for exception in &self.exceptions {
            let reason = exception.reason();
            if reason.trim().is_empty() {
                return Err(ValidationError::new(
                    "policy.exceptions.reason",
                    Problem::Empty,
                ));
            }
            if reason.chars().count() > MAX_EXCEPTION_REASON_CHARS {
                return Err(ValidationError::new(
                    "policy.exceptions.reason",
                    Problem::TooLong {
                        max: MAX_EXCEPTION_REASON_CHARS,
                    },
                ));
            }
            match exception {
                PolicyException::ExtraReadOnlyPath {
                    path, canonical, ..
                } => {
                    require_absolute("policy.exceptions.path", path)?;
                    require_absolute("policy.exceptions.canonical", canonical)?;
                }
                PolicyException::ExtraDevice { path, .. } => {
                    require_absolute("policy.exceptions.path", path)?;
                    if !path.starts_with("/dev") {
                        return Err(ValidationError::new(
                            "policy.exceptions.path",
                            Problem::InvalidFormat,
                        ));
                    }
                }
                PolicyException::PassEnv { name, .. } => {
                    super::launch_config::validate_env_name("policy.exceptions.name", name)?;
                }
            }
        }
        if self.display.isolation == DisplayIsolation::Gamescope {
            return Err(ValidationError::new(
                "policy.display.isolation",
                Problem::Reserved,
            ));
        }
        Ok(())
    }
}

bitflags::bitflags! {
    /// The permissions of a policy that are either granted or not, as a bit field: one bit
    /// for each permission the Protection tab shows as a switch, and one for the wider of
    /// the two degrees of a permission that has degrees (ADR-0023).
    ///
    /// It is a view of [`SandboxPolicy`], read with [`SandboxPolicy::permissions`] and
    /// written back with [`SandboxPolicy::set_permissions`]; the stored document keeps its
    /// fields. What carries more than yes or no is not in it: a folder grant (a path, a
    /// drive letter, a date), the game folder's access, the save data location, exceptions.
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
    pub struct Permissions: u32 {
        /// The internet and the local network (`network`).
        const INTERNET = 1 << 0;
        /// Sound through the speakers (`devices.audio` not `None`).
        const SOUND = 1 << 1;
        /// Game controllers (`devices.controllers`).
        const CONTROLLERS = 1 << 2;
        /// Degree of [`Self::CONTROLLERS`]: every input device, not only the controllers
        /// connected when the game starts.
        const ALL_INPUT = 1 << 3;
        /// Cameras (`devices.camera`).
        const CAMERA = 1 << 4;
        /// Removable drives (`removable`).
        const REMOVABLE = 1 << 5;
        /// Degree of [`Self::REMOVABLE`]: changing what is on them.
        const REMOVABLE_WRITE = 1 << 6;
        /// Sound devices directly (`devices.alsa_devices`).
        const DIRECT_SOUND = 1 << 7;
        /// This computer's name (`hardening.mask_hostname` off).
        const COMPUTER_NAME = 1 << 8;
        /// This computer's machine ID (`hardening.mask_machine_id` off).
        const MACHINE_ID = 1 << 9;
        /// Hardware identifiers (`hardening.mask_hardware_ids` off).
        const HARDWARE_IDS = 1 << 10;
        /// System calls the filter refuses (`hardening.seccomp` off).
        const UNFILTERED_SYSCALLS = 1 << 11;
        /// Degree of [`Self::SOUND`]: the microphone too (`devices.audio` `Full`). The
        /// Protection tab shows it as a row of its own (ADR-0026).
        const MICROPHONE = 1 << 12;
    }
}

impl Permissions {
    /// The degree bits that belong to the permissions in `self`: [`Self::ALL_INPUT`] for
    /// [`Self::CONTROLLERS`], [`Self::REMOVABLE_WRITE`] for [`Self::REMOVABLE`],
    /// [`Self::MICROPHONE`] for [`Self::SOUND`].
    pub const fn degrees(self) -> Self {
        let mut out = Self::empty();
        if self.contains(Self::CONTROLLERS) {
            out = out.union(Self::ALL_INPUT);
        }
        if self.contains(Self::REMOVABLE) {
            out = out.union(Self::REMOVABLE_WRITE);
        }
        if self.contains(Self::SOUND) {
            out = out.union(Self::MICROPHONE);
        }
        out
    }

    /// The permissions the degrees in `self` widen: granting a degree grants them too.
    pub const fn prerequisites(self) -> Self {
        let mut out = Self::empty();
        if self.contains(Self::ALL_INPUT) {
            out = out.union(Self::CONTROLLERS);
        }
        if self.contains(Self::REMOVABLE_WRITE) {
            out = out.union(Self::REMOVABLE);
        }
        if self.contains(Self::MICROPHONE) {
            out = out.union(Self::SOUND);
        }
        out
    }

    /// `self` without the degrees of permissions that are not granted: a degree says how
    /// far a permission goes and grants nothing by itself.
    pub const fn normalized(self) -> Self {
        let orphans = Self::ALL_INPUT
            .union(Self::REMOVABLE_WRITE)
            .union(Self::MICROPHONE)
            .difference(self.degrees());
        self.difference(orphans)
    }
}

impl SandboxPolicy {
    /// What this policy grants of the permissions that are granted or not.
    pub fn permissions(&self) -> Permissions {
        let mut p = Permissions::empty();
        p.set(Permissions::INTERNET, self.network == NetworkAccess::Full);
        p.set(Permissions::SOUND, self.devices.audio != AudioAccess::None);
        p.set(
            Permissions::MICROPHONE,
            self.devices.audio == AudioAccess::Full,
        );
        p.set(
            Permissions::CONTROLLERS,
            self.devices.controllers != ControllerAccess::None,
        );
        p.set(
            Permissions::ALL_INPUT,
            self.devices.controllers == ControllerAccess::AllInput,
        );
        p.set(Permissions::CAMERA, self.devices.camera);
        p.set(
            Permissions::REMOVABLE,
            self.removable != RemovableAccess::None,
        );
        p.set(
            Permissions::REMOVABLE_WRITE,
            self.removable == RemovableAccess::ReadWrite,
        );
        p.set(Permissions::DIRECT_SOUND, self.devices.alsa_devices);
        p.set(Permissions::COMPUTER_NAME, !self.hardening.mask_hostname);
        p.set(Permissions::MACHINE_ID, !self.hardening.mask_machine_id);
        p.set(Permissions::HARDWARE_IDS, !self.hardening.mask_hardware_ids);
        p.set(
            Permissions::UNFILTERED_SYSCALLS,
            self.hardening.seccomp == SeccompMode::Off,
        );
        p
    }

    /// Grant exactly `granted` of the permissions that are granted or not (after
    /// [`Permissions::normalized`]); every other field keeps its value. `preset` is not
    /// recomputed: that is `sandbox::presets::classify`.
    pub fn set_permissions(&mut self, granted: Permissions) {
        let p = granted.normalized();
        self.network = if p.contains(Permissions::INTERNET) {
            NetworkAccess::Full
        } else {
            NetworkAccess::None
        };
        self.devices.audio = if p.contains(Permissions::MICROPHONE) {
            AudioAccess::Full
        } else if p.contains(Permissions::SOUND) {
            AudioAccess::Playback
        } else {
            AudioAccess::None
        };
        self.devices.controllers = if p.contains(Permissions::ALL_INPUT) {
            ControllerAccess::AllInput
        } else if p.contains(Permissions::CONTROLLERS) {
            ControllerAccess::Detected
        } else {
            ControllerAccess::None
        };
        self.devices.camera = p.contains(Permissions::CAMERA);
        self.removable = if p.contains(Permissions::REMOVABLE_WRITE) {
            RemovableAccess::ReadWrite
        } else if p.contains(Permissions::REMOVABLE) {
            RemovableAccess::ReadOnly
        } else {
            RemovableAccess::None
        };
        self.devices.alsa_devices = p.contains(Permissions::DIRECT_SOUND);
        self.hardening.mask_hostname = !p.contains(Permissions::COMPUTER_NAME);
        self.hardening.mask_machine_id = !p.contains(Permissions::MACHINE_ID);
        self.hardening.mask_hardware_ids = !p.contains(Permissions::HARDWARE_IDS);
        self.hardening.seccomp = if p.contains(Permissions::UNFILTERED_SYSCALLS) {
            SeccompMode::Off
        } else {
            SeccompMode::Default
        };
    }
}

fn require_absolute(field: &'static str, path: &Path) -> Result<(), ValidationError> {
    if path.is_absolute() {
        Ok(())
    } else {
        Err(ValidationError::new(field, Problem::NotAbsolute))
    }
}

/// Whether `c` may be assigned as an extra drive letter (`d`..=`y`; `c` is the prefix and `z`
/// the root filesystem).
pub fn is_valid_drive_letter(c: char) -> bool {
    ('d'..='y').contains(&c)
}

/// One of the three presets, without the custom marker.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BasePreset {
    /// "No internet. Can use only its own folder."
    Offline,
    /// "Internet allowed. Can use only its own folder."
    Online,
    /// "Internet allowed. Fewer restrictions for best compatibility."
    Standard,
}

impl BasePreset {
    /// Every preset, in display order.
    pub const ALL: [BasePreset; 3] = [
        BasePreset::Offline,
        BasePreset::Online,
        BasePreset::Standard,
    ];

    /// Lower-case name as stored in text columns and `config.toml`.
    pub const fn as_str(self) -> &'static str {
        match self {
            BasePreset::Offline => "offline",
            BasePreset::Online => "online",
            BasePreset::Standard => "standard",
        }
    }

    /// Parse the lower-case name (case-insensitive).
    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|p| p.as_str().eq_ignore_ascii_case(s))
    }
}

/// The preset a policy matches, or `Custom` with the preset it differs from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Preset {
    /// Matches Offline exactly.
    Offline,
    /// Matches Online exactly.
    Online,
    /// Matches Standard exactly.
    Standard,
    /// Differs from `base` in at least one field.
    Custom {
        /// The preset this policy started from.
        base: BasePreset,
    },
}

impl Preset {
    /// The preset this policy is or is based on.
    pub const fn base(self) -> BasePreset {
        match self {
            Preset::Offline => BasePreset::Offline,
            Preset::Online => BasePreset::Online,
            Preset::Standard => BasePreset::Standard,
            Preset::Custom { base } => base,
        }
    }

    /// Whether the policy differs from its base.
    pub const fn is_custom(self) -> bool {
        matches!(self, Preset::Custom { .. })
    }

    /// Text form for `sessions.preset`: `offline`, `online`, `standard`, `custom:<base>`.
    pub fn as_column(self) -> String {
        match self {
            Preset::Custom { base } => format!("custom:{}", base.as_str()),
            other => other.base().as_str().to_owned(),
        }
    }

    /// Parse [`Preset::as_column`] text.
    pub fn from_column(s: &str) -> Option<Self> {
        match s.strip_prefix("custom:") {
            Some(base) => BasePreset::parse(base).map(|base| Preset::Custom { base }),
            None => BasePreset::parse(s).map(Preset::from),
        }
    }
}

impl From<BasePreset> for Preset {
    fn from(b: BasePreset) -> Self {
        match b {
            BasePreset::Offline => Preset::Offline,
            BasePreset::Online => Preset::Online,
            BasePreset::Standard => Preset::Standard,
        }
    }
}

/// Network access (SECURITY_MODEL §5.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NetworkAccess {
    /// A new network namespace with only loopback.
    None,
    /// The host network namespace.
    Full,
}

impl NetworkAccess {
    /// Lower-case text form.
    pub const fn as_str(self) -> &'static str {
        match self {
            NetworkAccess::None => "none",
            NetworkAccess::Full => "full",
        }
    }
}

/// Access to the game's folder (SECURITY_MODEL §5.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GameDirAccess {
    /// Read-only bind of the executable file alone (forced for loose executables).
    FileOnly,
    /// Read-only bind of the folder.
    ReadOnly,
    /// Read-write bind of the folder.
    ReadWrite,
    /// Writable overlay; changes stored separately.
    Protected,
}

/// Access mode of a folder grant.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FolderAccess {
    /// Read only.
    ReadOnly,
    /// Read and write.
    ReadWrite,
}

/// A well-known user directory offered as a toggle.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UserDir {
    /// `XDG_DESKTOP_DIR`.
    Desktop,
    /// `XDG_DOCUMENTS_DIR`.
    Documents,
    /// `XDG_DOWNLOAD_DIR`.
    Downloads,
    /// `XDG_MUSIC_DIR`.
    Music,
    /// `XDG_PICTURES_DIR`.
    Pictures,
    /// `XDG_VIDEOS_DIR`.
    Videos,
}

/// How a grant came to exist.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GrantOrigin {
    /// Chosen in a file dialog.
    UserChosen,
    /// One of the well-known directory toggles.
    WellKnown(UserDir),
}

/// An additional folder the game may use, bound at the identical path.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FolderGrant {
    /// As chosen by the user.
    pub path: PathBuf,
    /// Resolved when granted; re-checked at launch.
    pub canonical: PathBuf,
    /// Access mode.
    pub access: FolderAccess,
    /// Origin of the grant.
    pub origin: GrantOrigin,
    /// `d`..=`y`; `None` = reachable through `Z:` only.
    pub drive_letter: Option<char>,
    /// When it was granted.
    pub granted_at: Timestamp,
}

/// Where the game's Windows user profile lives.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SaveDataLocation {
    /// Inside the prefix.
    Internal,
    /// A user-chosen folder bound over the profile.
    External {
        /// The folder.
        dir: PathBuf,
    },
}

/// Removable media access.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RemovableAccess {
    /// Not visible.
    None,
    /// Read only.
    ReadOnly,
    /// Read and write.
    ReadWrite,
}

/// Device access (SECURITY_MODEL §5.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DeviceAccess {
    /// Sound, with or without the microphone.
    pub audio: AudioAccess,
    /// Raw ALSA devices (`/dev/snd`).
    pub alsa_devices: bool,
    /// Game controllers.
    pub controllers: ControllerAccess,
    /// Cameras (`/dev/video*`).
    pub camera: bool,
}

/// Sound access.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AudioAccess {
    /// No sound.
    None,
    /// Playback only: a sound server of the session's own that refuses recording
    /// (ADR-0026). Where the host cannot provide one, no sound.
    Playback,
    /// The host's sound server sockets: playback and the microphone.
    Full,
}

/// Controller access (SECURITY_MODEL §5.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ControllerAccess {
    /// Nothing under `/dev/input`.
    None,
    /// Classified joystick/gamepad nodes only.
    Detected,
    /// All of `/dev/input`, only when no keyboard or mouse is readable.
    AllInput,
}

/// Display access (SECURITY_MODEL §5.5).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DisplayAccess {
    /// Wayland socket.
    pub wayland: SocketAccess,
    /// X11 socket.
    pub x11: SocketAccess,
    /// Nested-compositor isolation.
    pub isolation: DisplayIsolation,
}

/// Whether a display socket is bound when available.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SocketAccess {
    /// Bind when present.
    Auto,
    /// Never bind.
    Deny,
}

/// Display isolation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DisplayIsolation {
    /// None.
    None,
    /// Gamescope (reserved; rejected by validation in 1.0).
    Gamescope,
}

/// IPC access (SECURITY_MODEL §5.6).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct IpcAccess {
    /// Session bus access.
    pub session_bus: SessionBusAccess,
}

/// Session bus access. Only `None` exists in 1.0 (`Filtered { talk }` reserved, ROADMAP R-05).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionBusAccess {
    /// No session bus.
    None,
}

/// Hardening switches (SECURITY_MODEL §5.7).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Hardening {
    /// Seccomp filter.
    pub seccomp: SeccompMode,
    /// `--hostname easyplay`.
    pub mask_hostname: bool,
    /// Synthesized `/etc/machine-id`.
    pub mask_machine_id: bool,
    /// Masked sysfs network nodes and DMI.
    pub mask_hardware_ids: bool,
    /// Runtime isolation mode.
    pub runtime_isolation: RuntimeIsolation,
}

/// Seccomp mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SeccompMode {
    /// The deny-list filter.
    Default,
    /// No filter (compatibility switch).
    Off,
}

/// Requested runtime isolation (ADR-0006, RUNTIME_MANAGEMENT §7).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeIsolation {
    /// Isolated where reflinks make it free, else Shared.
    Auto,
    /// Always a private copy.
    Isolated,
    /// The audited shared runtime.
    Shared,
}

/// A narrow, explicit deviation with a user-written reason (SECURITY_MODEL §5.8).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PolicyException {
    /// Additional read-only bind.
    ExtraReadOnlyPath {
        /// As chosen.
        path: PathBuf,
        /// Resolved.
        canonical: PathBuf,
        /// Why.
        reason: String,
    },
    /// Copy one host variable into the sandbox.
    PassEnv {
        /// Variable name.
        name: String,
        /// Why.
        reason: String,
    },
    /// `--dev-bind` of one device node (Developer Mode only).
    ExtraDevice {
        /// Device path under `/dev`.
        path: PathBuf,
        /// Why.
        reason: String,
    },
}

impl PolicyException {
    /// The user-written reason.
    pub fn reason(&self) -> &str {
        match self {
            PolicyException::ExtraReadOnlyPath { reason, .. }
            | PolicyException::PassEnv { reason, .. }
            | PolicyException::ExtraDevice { reason, .. } => reason,
        }
    }
}
