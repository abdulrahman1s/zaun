//! `explain(plan, policy)`: the plain-language access statements derived from the plan that
//! is actually executed (SECURITY_MODEL §10, SCREEN_SPECS §6, DEVELOPER_MODE §3).
//!
//! Statements are generated from the plan's mounts grouped by their `policy_field`, so every
//! policy-derived mount is covered by exactly one statement; flag-only statements name the
//! option that implements them. Each statement lists its mechanisms (mount indices, flags,
//! variables) for the inspector's *Access* tab.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::plan::{MountKind, MountMode, PolicyField, SandboxPlan, Section};
use super::validate::IssueCode;
use crate::model::{GameDirAccess, GrantOrigin, SandboxPolicy, UserDir};

/// Category, in display order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AccessCategory {
    /// Internet and local network.
    Network,
    /// The game's own storage and folder.
    OwnFolder,
    /// The user's folders.
    YourFolders,
    /// Devices.
    Devices,
    /// Display.
    Display,
    /// Host identity.
    Identity,
    /// Weakened protections.
    HardeningChanges,
    /// Developer Mode exceptions.
    Exceptions,
}

/// How a statement is shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StatementSeverity {
    /// Neutral or reassuring.
    Info,
    /// Access worth knowing about.
    Notice,
    /// A weaker protection.
    Warning,
    /// Exposes something on the deny-list (shown in red).
    Danger,
}

/// One access statement.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccessStatement {
    /// Category.
    pub category: AccessCategory,
    /// Message key, e.g. `access.network.none`.
    pub key: String,
    /// Message arguments (`name`, `path`, `count`).
    pub args: BTreeMap<String, String>,
    /// English text.
    pub text: String,
    /// Optional detail line.
    pub detail: Option<String>,
    /// Severity.
    pub severity: StatementSeverity,
    /// Indices into `plan.mounts` that implement the statement.
    pub mounts: Vec<usize>,
    /// Options or namespace facts that implement it (`--unshare-net`).
    pub flags: Vec<String>,
    /// Variables that implement it.
    pub env: Vec<String>,
}

impl AccessStatement {
    fn new(
        category: AccessCategory,
        key: &str,
        text: impl Into<String>,
        severity: StatementSeverity,
    ) -> Self {
        Self {
            category,
            key: key.to_owned(),
            args: BTreeMap::new(),
            text: text.into(),
            detail: None,
            severity,
            mounts: Vec::new(),
            flags: Vec::new(),
            env: Vec::new(),
        }
    }

    fn arg(mut self, k: &str, v: impl Into<String>) -> Self {
        self.args.insert(k.to_owned(), v.into());
        self
    }

    fn mounts(mut self, m: Vec<usize>) -> Self {
        self.mounts = m;
        self
    }

    fn flag(mut self, f: &str) -> Self {
        self.flags.push(f.to_owned());
        self
    }

    fn detail(mut self, d: impl Into<String>) -> Self {
        self.detail = Some(d.into());
        self
    }

    /// Whether the statement is backed by at least one mechanism.
    pub fn has_mechanism(&self) -> bool {
        !(self.mounts.is_empty() && self.flags.is_empty() && self.env.is_empty())
    }
}

fn display_name(path: &Path, origin: Option<GrantOrigin>) -> String {
    match origin {
        Some(GrantOrigin::WellKnown(d)) => match d {
            UserDir::Desktop => "Desktop",
            UserDir::Documents => "Documents",
            UserDir::Downloads => "Downloads",
            UserDir::Music => "Music",
            UserDir::Pictures => "Pictures",
            UserDir::Videos => "Videos",
        }
        .to_owned(),
        _ => path.file_name().map_or_else(
            || path.display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        ),
    }
}

/// Explain a plan.
pub fn explain(plan: &SandboxPlan, policy: &SandboxPolicy) -> Vec<AccessStatement> {
    use AccessCategory as C;
    use StatementSeverity as S;
    let mut groups: BTreeMap<PolicyField, Vec<usize>> = BTreeMap::new();
    for (i, m) in plan.mounts.iter().enumerate() {
        if let Some(f) = &m.origin.policy_field {
            groups.entry(f.clone()).or_default().push(i);
        }
    }
    let mut out: Vec<AccessStatement> = Vec::new();
    let ns = &plan.namespaces;

    // Network.
    let net_mounts = groups.remove(&PolicyField::Network).unwrap_or_default();
    if ns.net {
        out.push(
            AccessStatement::new(
                C::Network,
                "access.network.none",
                "No internet or local network",
                S::Info,
            )
            .mounts(net_mounts)
            .flag("--unshare-net"),
        );
    } else {
        out.push(
            AccessStatement::new(
                C::Network,
                "access.network.full",
                "Can use the internet and your local network",
                S::Notice,
            )
            .mounts(net_mounts)
            .flag("host network namespace"),
        );
    }

    // Own folder.
    let own: Vec<usize> = plan
        .mounts
        .iter()
        .enumerate()
        .filter(|(_, m)| {
            m.origin.policy_field.is_none()
                && (m.origin.section == Section::Prefix
                    || (m.origin.section == Section::Home
                        && matches!(
                            m.kind,
                            MountKind::Bind {
                                mode: MountMode::Rw,
                                ..
                            } | MountKind::Tmpfs { .. }
                        )))
        })
        .map(|(i, _)| i)
        .collect();
    let home_mount: Vec<usize> = own
        .iter()
        .copied()
        .filter(|i| plan.mounts[*i].origin.section == Section::Home)
        .collect();
    out.push(
        AccessStatement::new(
            C::OwnFolder,
            "access.own.private_storage",
            "Has its own private storage",
            S::Info,
        )
        .mounts(own),
    );
    if let Some(m) = groups.remove(&PolicyField::GameDir) {
        let (key, text) = match policy.game_dir {
            GameDirAccess::FileOnly => ("access.own.file_only", "Can read only its program file"),
            GameDirAccess::ReadOnly => ("access.own.read_only", "Can read its game folder"),
            GameDirAccess::ReadWrite => (
                "access.own.read_write",
                "Can read and change its game folder",
            ),
            GameDirAccess::Protected => (
                "access.own.protected",
                "Can change its game folder; changes are kept separately",
            ),
        };
        let path = m
            .first()
            .map(|i| plan.mounts[*i].kind.dst().display().to_string())
            .unwrap_or_default();
        out.push(
            AccessStatement::new(C::OwnFolder, key, text, S::Info)
                .mounts(m)
                .arg("path", path),
        );
    }

    // Your folders.
    let mut personal = false;
    let folder_fields: Vec<PolicyField> = groups
        .keys()
        .filter(|f| matches!(f, PolicyField::Folder(_)))
        .cloned()
        .collect();
    for f in folder_fields {
        let PolicyField::Folder(path) = &f else {
            continue;
        };
        let m = groups.remove(&f).unwrap_or_default();
        let grant = policy.folders.iter().find(|g| &g.canonical == path);
        let rw = m.iter().any(|i| plan.mounts[*i].kind.writes_host());
        let name = display_name(path, grant.map(|g| g.origin));
        let (key, text) = if rw {
            (
                "access.folders.read_write",
                format!("Can read and change {name}"),
            )
        } else {
            ("access.folders.read_only", format!("Can read {name}"))
        };
        personal = true;
        out.push(
            AccessStatement::new(C::YourFolders, key, text, S::Notice)
                .mounts(m)
                .arg("name", name)
                .arg("path", path.display().to_string()),
        );
    }
    if let Some(m) = groups.remove(&PolicyField::SaveData) {
        personal = true;
        let path = m
            .first()
            .and_then(|i| plan.mounts[*i].kind.src())
            .map(|p| p.display().to_string())
            .unwrap_or_default();
        out.push(
            AccessStatement::new(
                C::YourFolders,
                "access.folders.save_data",
                "Keeps its save data in your folder",
                S::Notice,
            )
            .mounts(m)
            .arg("path", path),
        );
    }
    if let Some(m) = groups.remove(&PolicyField::Removable) {
        personal = true;
        let rw = m.iter().any(|i| plan.mounts[*i].kind.writes_host());
        let (key, text) = if rw {
            (
                "access.folders.removable_rw",
                "Can read and change removable drives",
            )
        } else {
            ("access.folders.removable_ro", "Can read removable drives")
        };
        out.push(AccessStatement::new(C::YourFolders, key, text, S::Notice).mounts(m));
    }
    let exceptions_expose = groups
        .keys()
        .any(|f| matches!(f, PolicyField::ExtraReadOnlyPath(_)));
    if !personal && !exceptions_expose && !home_mount.is_empty() {
        out.push(
            AccessStatement::new(
                C::YourFolders,
                "access.folders.none",
                "Cannot see your personal files",
                S::Info,
            )
            .mounts(home_mount),
        );
    }

    // Devices.
    if let Some(m) = groups.remove(&PolicyField::Audio) {
        out.push(
            AccessStatement::new(
                C::Devices,
                "access.devices.sound",
                "Sound (speakers and microphone)",
                S::Notice,
            )
            .mounts(m)
            .detail("Playback and recording cannot be separated"),
        );
    }
    if let Some(m) = groups.remove(&PolicyField::Controllers) {
        let all = m
            .iter()
            .any(|i| plan.mounts[*i].kind.dst() == Path::new("/dev/input"));
        let st = if all {
            AccessStatement::new(
                C::Devices,
                "access.devices.all_input",
                "Can use all game controllers, including ones connected later",
                S::Notice,
            )
        } else {
            let count = m
                .iter()
                .filter(|i| plan.mounts[**i].kind.dst().starts_with("/dev/input"))
                .count();
            AccessStatement::new(
                C::Devices,
                "access.devices.controllers",
                "Can use the game controllers connected now",
                S::Info,
            )
            .arg("count", count.to_string())
        };
        out.push(st.mounts(m));
    }
    if let Some(m) = groups.remove(&PolicyField::AlsaDevices) {
        out.push(
            AccessStatement::new(
                C::Devices,
                "access.devices.alsa",
                "Can use sound devices directly",
                S::Notice,
            )
            .mounts(m),
        );
    }
    if let Some(m) = groups.remove(&PolicyField::Camera) {
        out.push(
            AccessStatement::new(
                C::Devices,
                "access.devices.camera",
                "Can use your cameras",
                S::Warning,
            )
            .mounts(m),
        );
    }

    // Display.
    if let Some(m) = groups.remove(&PolicyField::Wayland) {
        out.push(
            AccessStatement::new(
                C::Display,
                "access.display.wayland",
                "Shows its windows on your desktop",
                S::Info,
            )
            .mounts(m),
        );
    }
    if let Some(m) = groups.remove(&PolicyField::X11) {
        let mut st = AccessStatement::new(
            C::Display,
            "access.display.x11",
            "Uses the X11 display: it can see and control other X11 windows",
            S::Warning,
        )
        .mounts(m);
        if !ns.ipc {
            st = st.flag("shared IPC namespace (X11 shared memory)");
        }
        out.push(st);
    }

    // Identity.
    if let Some(m) = groups.remove(&PolicyField::MaskMachineId) {
        let masked = m
            .iter()
            .any(|i| matches!(plan.mounts[*i].kind, MountKind::FileData { .. }));
        out.push(
            if masked {
                AccessStatement::new(
                    C::Identity,
                    "access.identity.machine_id_masked",
                    "Sees a stand-in machine ID",
                    S::Info,
                )
            } else {
                AccessStatement::new(
                    C::Identity,
                    "access.identity.machine_id_real",
                    "Sees this computer's machine ID",
                    S::Notice,
                )
            }
            .mounts(m),
        );
    }
    if plan.hostname.is_some() {
        out.push(
            AccessStatement::new(
                C::Identity,
                "access.identity.hostname_masked",
                "Sees the computer name \"easyplay\"",
                S::Info,
            )
            .flag("--hostname"),
        );
    } else {
        out.push(
            AccessStatement::new(
                C::Identity,
                "access.identity.hostname_real",
                "Sees this computer's name",
                S::Notice,
            )
            .flag("--hostname absent"),
        );
    }
    let hw_masks = groups.remove(&PolicyField::MaskHardwareIds);
    let dmi_masked = hw_masks.as_ref().is_some_and(|m| {
        m.iter()
            .any(|i| plan.mounts[*i].kind.dst() == Path::new("/sys/devices/virtual/dmi"))
    });
    if dmi_masked {
        out.push(
            AccessStatement::new(
                C::Identity,
                "access.identity.hardware_masked",
                "Cannot read hardware identifiers",
                S::Info,
            )
            .mounts(hw_masks.unwrap_or_default()),
        );
    } else {
        let sys: Vec<usize> = plan
            .mounts_in(Section::Sysfs)
            .filter(|(_, m)| m.kind.is_host_bind())
            .map(|(i, _)| i)
            .chain(hw_masks.unwrap_or_default())
            .collect();
        out.push(
            AccessStatement::new(
                C::Identity,
                "access.identity.hardware_visible",
                "Can read hardware identifiers",
                S::Notice,
            )
            .mounts(sys),
        );
    }

    // Hardening changes.
    if ns.seccomp_fd.is_none() {
        let st = if policy.hardening.seccomp == crate::model::SeccompMode::Off {
            AccessStatement::new(
                C::HardeningChanges,
                "access.hardening.seccomp_off",
                "System-call filter is off",
                S::Warning,
            )
        } else {
            AccessStatement::new(
                C::HardeningChanges,
                "access.hardening.seccomp_unavailable",
                "System-call filter is not available",
                S::Warning,
            )
        };
        out.push(st.flag("--seccomp absent"));
    }
    if plan.audit_after.is_some() {
        let m: Vec<usize> = plan
            .mounts_in(Section::RuntimeStore)
            .filter(|(_, m)| m.kind.writes_host())
            .map(|(i, _)| i)
            .collect();
        out.push(
            AccessStatement::new(
                C::HardeningChanges,
                "access.hardening.runtime_shared",
                "Shares the compatibility layer with other games; it is checked after each session",
                S::Warning,
            )
            .mounts(m),
        );
    }

    // Exceptions (and anything left).
    let bypassed: Vec<&PathBuf> = plan
        .issues
        .iter()
        .filter(|i| i.code == IssueCode::PathDenyBypassed)
        .filter_map(|i| i.path.as_ref())
        .collect();
    for (field, m) in groups {
        let st = match &field {
            PolicyField::ExtraReadOnlyPath(p) => {
                let danger = bypassed.contains(&p);
                AccessStatement::new(
                    C::Exceptions,
                    "access.exceptions.read_path",
                    format!("Can read {}", p.display()),
                    if danger { S::Danger } else { S::Warning },
                )
                .arg("path", p.display().to_string())
                .detail(reason_of(policy, &field))
            }
            PolicyField::ExtraDevice(p) => AccessStatement::new(
                C::Exceptions,
                "access.exceptions.device",
                format!("Has direct access to the device {}", p.display()),
                S::Warning,
            )
            .arg("path", p.display().to_string())
            .detail(reason_of(policy, &field)),
            other => AccessStatement::new(
                C::Exceptions,
                "access.other",
                format!("{other:?}"),
                S::Notice,
            ),
        };
        out.push(st.mounts(m));
    }
    for e in &policy.exceptions {
        if let crate::model::PolicyException::PassEnv { name, reason } = e
            && plan.env.set.contains_key(name)
        {
            let mut st = AccessStatement::new(
                C::Exceptions,
                "access.exceptions.pass_env",
                format!("Receives the value of {name} from your session"),
                S::Warning,
            )
            .arg("name", name.clone())
            .detail(reason.clone());
            st.env.push(name.clone());
            out.push(st);
        }
    }
    out.sort_by_key(|s| s.category);
    out
}

fn reason_of(policy: &SandboxPolicy, field: &PolicyField) -> String {
    use crate::model::PolicyException as E;
    policy
        .exceptions
        .iter()
        .find_map(|e| match (e, field) {
            (
                E::ExtraReadOnlyPath {
                    canonical, reason, ..
                },
                PolicyField::ExtraReadOnlyPath(p),
            ) if canonical == p => Some(reason.clone()),
            (E::ExtraDevice { path, reason }, PolicyField::ExtraDevice(p)) if path == p => {
                Some(reason.clone())
            }
            _ => None,
        })
        .unwrap_or_default()
}

/// Statements added and removed between two explanations (for the permissions editor).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StatementDiff {
    /// In `new` only.
    pub added: Vec<AccessStatement>,
    /// In `old` only.
    pub removed: Vec<AccessStatement>,
}

/// Compare by category, key and arguments.
pub fn diff(old: &[AccessStatement], new: &[AccessStatement]) -> StatementDiff {
    let same = |a: &AccessStatement, b: &AccessStatement| {
        a.category == b.category && a.key == b.key && a.args == b.args
    };
    StatementDiff {
        added: new
            .iter()
            .filter(|n| !old.iter().any(|o| same(o, n)))
            .cloned()
            .collect(),
        removed: old
            .iter()
            .filter(|o| !new.iter().any(|n| same(o, n)))
            .cloned()
            .collect(),
    }
}
