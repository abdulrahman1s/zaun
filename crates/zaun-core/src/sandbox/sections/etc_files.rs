//! §4.3 `/etc` from a closed allow-list.
//!
//! Nothing outside [`EtcItem`] can be emitted. A host entry that is a symlink is recreated
//! only when both its literal target and its final target lie in a tree that is bound as a
//! whole (`/usr`; on Nix `/nix/store` and `/run/current-system`; on Guix `/gnu/store` and
//! `/run/current-system`); otherwise the resolved target is bound read-only at the `/etc`
//! path. `/etc/static` is deliberately not such a tree: on NixOS many `/etc` links go through
//! it, and a link through `/etc/static` must work even where `/etc/static` is not visible
//! (field-tested on the development host, TECHNICAL_RESEARCH F-222).

use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use super::{Effective, mk};
use crate::host::{HostEntry, HostFacts, LayoutKind, ResolverStrategy};
use crate::ids::GameId;
use crate::model::{AudioAccess, NetworkAccess, SessionKind};
use crate::paths::normalize;
use crate::sandbox::context::SandboxContext;
use crate::sandbox::plan::{FileDataOp, MountKind, PlanBuilder, PolicyField};

/// When an allow-listed entry is included.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EtcCondition {
    /// Always.
    Always,
    /// When sound is granted.
    Audio,
    /// When the network is on.
    Network,
}

/// The closed `/etc` allow-list, in emission order (excluding synthesized files).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EtcItem {
    /// `ld.so.cache`
    LdSoCache,
    /// `ld.so.conf`
    LdSoConf,
    /// `ld.so.conf.d`
    LdSoConfD,
    /// `alternatives`
    Alternatives,
    /// `fonts`
    Fonts,
    /// `os-release`
    OsRelease,
    /// `localtime`
    Localtime,
    /// `timezone`
    Timezone,
    /// `locale.conf`
    LocaleConf,
    /// `zoneinfo` (NixOS keeps the time-zone database here)
    Zoneinfo,
    /// `vulkan`
    Vulkan,
    /// `glvnd`
    Glvnd,
    /// `egl`
    Egl,
    /// `OpenCL`
    OpenCl,
    /// `drirc`
    Drirc,
    /// `nvidia`
    Nvidia,
    /// `libva.conf`
    LibvaConf,
    /// `pulse`
    Pulse,
    /// `alsa`
    Alsa,
    /// `asound.conf`
    AsoundConf,
    /// `pipewire`
    Pipewire,
    /// `openal`
    Openal,
    /// `hosts`
    Hosts,
    /// `resolv.conf`
    ResolvConf,
    /// `host.conf`
    HostConf,
    /// `gai.conf`
    GaiConf,
    /// `ssl`
    Ssl,
    /// `pki`
    Pki,
    /// `ca-certificates`
    CaCertificates,
    /// `ca-certificates.conf`
    CaCertificatesConf,
}

impl EtcItem {
    /// Entries before the synthesized identity files.
    pub const BEFORE_IDENTITY: &'static [EtcItem] = &[
        EtcItem::LdSoCache,
        EtcItem::LdSoConf,
        EtcItem::LdSoConfD,
        EtcItem::Alternatives,
        EtcItem::Fonts,
        EtcItem::OsRelease,
        EtcItem::Localtime,
        EtcItem::Timezone,
        EtcItem::LocaleConf,
        EtcItem::Zoneinfo,
        EtcItem::Vulkan,
        EtcItem::Glvnd,
        EtcItem::Egl,
        EtcItem::OpenCl,
        EtcItem::Drirc,
        EtcItem::Nvidia,
        EtcItem::LibvaConf,
        EtcItem::Pulse,
        EtcItem::Alsa,
        EtcItem::AsoundConf,
        EtcItem::Pipewire,
        EtcItem::Openal,
    ];

    /// Entries after the synthesized identity files.
    pub const AFTER_IDENTITY: &'static [EtcItem] = &[
        EtcItem::Hosts,
        EtcItem::ResolvConf,
        EtcItem::HostConf,
        EtcItem::GaiConf,
        EtcItem::Ssl,
        EtcItem::Pki,
        EtcItem::CaCertificates,
        EtcItem::CaCertificatesConf,
    ];

    /// File name under `/etc`.
    pub const fn name(self) -> &'static str {
        match self {
            EtcItem::LdSoCache => "ld.so.cache",
            EtcItem::LdSoConf => "ld.so.conf",
            EtcItem::LdSoConfD => "ld.so.conf.d",
            EtcItem::Alternatives => "alternatives",
            EtcItem::Fonts => "fonts",
            EtcItem::OsRelease => "os-release",
            EtcItem::Localtime => "localtime",
            EtcItem::Timezone => "timezone",
            EtcItem::LocaleConf => "locale.conf",
            EtcItem::Zoneinfo => "zoneinfo",
            EtcItem::Vulkan => "vulkan",
            EtcItem::Glvnd => "glvnd",
            EtcItem::Egl => "egl",
            EtcItem::OpenCl => "OpenCL",
            EtcItem::Drirc => "drirc",
            EtcItem::Nvidia => "nvidia",
            EtcItem::LibvaConf => "libva.conf",
            EtcItem::Pulse => "pulse",
            EtcItem::Alsa => "alsa",
            EtcItem::AsoundConf => "asound.conf",
            EtcItem::Pipewire => "pipewire",
            EtcItem::Openal => "openal",
            EtcItem::Hosts => "hosts",
            EtcItem::ResolvConf => "resolv.conf",
            EtcItem::HostConf => "host.conf",
            EtcItem::GaiConf => "gai.conf",
            EtcItem::Ssl => "ssl",
            EtcItem::Pki => "pki",
            EtcItem::CaCertificates => "ca-certificates",
            EtcItem::CaCertificatesConf => "ca-certificates.conf",
        }
    }

    /// Inclusion condition.
    pub const fn condition(self) -> EtcCondition {
        match self {
            EtcItem::Pulse
            | EtcItem::Alsa
            | EtcItem::AsoundConf
            | EtcItem::Pipewire
            | EtcItem::Openal => EtcCondition::Audio,
            EtcItem::ResolvConf
            | EtcItem::HostConf
            | EtcItem::GaiConf
            | EtcItem::Ssl
            | EtcItem::Pki
            | EtcItem::CaCertificates
            | EtcItem::CaCertificatesConf => EtcCondition::Network,
            _ => EtcCondition::Always,
        }
    }

    /// Whether the `-try` form is used (the entry is optional on the host).
    pub const fn is_try(self) -> bool {
        !matches!(self, EtcItem::Hosts | EtcItem::ResolvConf)
    }

    /// Every allow-listed name, including the synthesized and identity entries.
    pub fn all_names() -> Vec<&'static str> {
        let mut v: Vec<&'static str> = Self::BEFORE_IDENTITY
            .iter()
            .chain(Self::AFTER_IDENTITY)
            .map(|i| i.name())
            .collect();
        v.extend(["passwd", "group", "nsswitch.conf", "machine-id"]);
        v
    }
}

/// Trees bound as a whole on this layout, in which a recreated symlink resolves correctly.
fn visible_trees(kind: LayoutKind) -> &'static [&'static str] {
    match kind {
        LayoutKind::Fhs => &["/usr"],
        LayoutKind::Nix => &["/usr", "/nix/store", "/run/current-system"],
        LayoutKind::Guix => &["/usr", "/gnu/store", "/run/current-system"],
    }
}

/// Lexically resolve a symlink `target` found directly in `/etc`.
fn lexical_target(target: &Path) -> PathBuf {
    if target.is_absolute() {
        normalize(target)
    } else {
        normalize(&Path::new("/etc").join(target))
    }
}

/// Whether a symlink in `/etc` can be recreated as a link: its literal and final targets
/// both lie in a tree bound as a whole.
pub fn recreate_link(kind: LayoutKind, target: &Path, resolved: &Path) -> bool {
    let under = |p: &Path| visible_trees(kind).iter().any(|t| p.starts_with(t));
    under(&lexical_target(target)) && under(resolved)
}

/// The directory to bind for a symlinked `resolv.conf` when the network is on.
pub fn resolver_dir(host: &HostFacts, eff: &Effective) -> Option<PathBuf> {
    if eff.network != NetworkAccess::Full {
        return None;
    }
    match &host.layout.resolver {
        ResolverStrategy::Symlink { bind_dir, .. } => Some(bind_dir.clone()),
        ResolverStrategy::File | ResolverStrategy::Absent => None,
    }
}

/// The planner's view of one host `/etc` entry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EtcEntry {
    /// A regular file or directory: bound at the identical path.
    Plain,
    /// A symlink with its literal and final target.
    Link {
        /// Literal target.
        target: PathBuf,
        /// Final target (canonical).
        resolved: PathBuf,
    },
}

/// Look up `/etc/<name>` in the facts: the fully resolved link from `etc_links` first (it
/// follows `/etc/static` chains), then the layout entry. Dangling links and other file types
/// are skipped. `resolv.conf` follows `layout.resolver`.
pub fn lookup(host: &HostFacts, name: &str) -> Option<EtcEntry> {
    if name == "resolv.conf" {
        return match &host.layout.resolver {
            ResolverStrategy::Symlink { target, bind_dir } => Some(EtcEntry::Link {
                target: target.clone(),
                resolved: bind_dir.clone(),
            }),
            ResolverStrategy::File => Some(EtcEntry::Plain),
            ResolverStrategy::Absent => None,
        };
    }
    if let Some(l) = host.etc_links.links.get(name) {
        return l.resolved.clone().map(|resolved| EtcEntry::Link {
            target: l.target.clone(),
            resolved,
        });
    }
    match host.layout.etc.get(name)? {
        HostEntry::Dir | HostEntry::File => Some(EtcEntry::Plain),
        HostEntry::Symlink { target, resolved } => {
            resolved.clone().map(|resolved| EtcEntry::Link {
                target: target.clone(),
                resolved,
            })
        }
        HostEntry::Other => None,
    }
}

/// `hex(sha256(host_machine_id ‖ game_id))[..32]`.
pub fn stand_in_machine_id(host_machine_id: Option<&str>, game: GameId) -> String {
    let mut h = Sha256::new();
    h.update(host_machine_id.unwrap_or("").as_bytes());
    h.update(game.to_string().as_bytes());
    let digest = h.finalize();
    let mut hex = String::with_capacity(64);
    for b in digest.iter() {
        hex.push_str(&format!("{b:02x}"));
    }
    hex.truncate(32);
    hex
}

/// Synthesized `/etc/passwd`.
pub fn passwd(ctx: &SandboxContext<'_>) -> String {
    format!(
        "{u}:x:{uid}:{gid}:{u}:{home}:/bin/sh\nnobody:x:65534:65534:nobody:/:/usr/sbin/nologin\n",
        u = ctx.user_name,
        uid = ctx.uid,
        gid = ctx.gid,
        home = ctx.home.display()
    )
}

/// Synthesized `/etc/group`.
pub fn group(ctx: &SandboxContext<'_>) -> String {
    format!(
        "{g}:x:{gid}:\nnobody:x:65534:\n",
        g = ctx.group_name,
        gid = ctx.gid
    )
}

/// Synthesized `/etc/nsswitch.conf`.
pub const NSSWITCH: &str = "passwd: files\ngroup: files\nhosts: files dns\n";

fn wanted(item: EtcItem, eff: &Effective) -> bool {
    match item.condition() {
        EtcCondition::Always => true,
        EtcCondition::Audio => eff.audio != AudioAccess::None,
        EtcCondition::Network => eff.network == NetworkAccess::Full,
    }
}

fn field_of(item: EtcItem) -> Option<PolicyField> {
    match item.condition() {
        EtcCondition::Always => None,
        EtcCondition::Audio => Some(PolicyField::Audio),
        EtcCondition::Network => Some(PolicyField::Network),
    }
}

fn emit_entry(
    kind: LayoutKind,
    name: &str,
    entry: &EtcEntry,
    try_: bool,
    reason: &'static str,
    field: Option<PolicyField>,
    b: &mut PlanBuilder,
) {
    let dst = Path::new("/etc").join(name);
    let mount = match entry {
        EtcEntry::Link { target, resolved } => {
            // resolv.conf is always recreated; its directory is bound by tmp_run.
            if name == "resolv.conf" || recreate_link(kind, target, resolved) {
                mk::symlink(target.clone(), &dst)
            } else {
                mk::bind(resolved, &dst, crate::sandbox::plan::MountMode::Ro, try_)
            }
        }
        EtcEntry::Plain => mk::bind(&dst, &dst, crate::sandbox::plan::MountMode::Ro, try_),
    };
    b.mount(mount, reason, field);
}

/// §4.3.
pub fn emit(ctx: &SandboxContext<'_>, host: &HostFacts, eff: &Effective, b: &mut PlanBuilder) {
    let kind = host.layout.kind;
    b.mount(mk::dir("/etc"), "allow-listed /etc", None);
    for &item in EtcItem::BEFORE_IDENTITY {
        if !wanted(item, eff) {
            continue;
        }
        if let Some(entry) = lookup(host, item.name()) {
            emit_entry(
                kind,
                item.name(),
                &entry,
                item.is_try(),
                "system configuration",
                field_of(item),
                b,
            );
        }
    }
    b.file_data(
        "/etc/passwd".into(),
        passwd(ctx).into_bytes(),
        0o444,
        FileDataOp::RoBindData,
        "synthesized user list",
        None,
    );
    b.file_data(
        "/etc/group".into(),
        group(ctx).into_bytes(),
        0o444,
        FileDataOp::RoBindData,
        "synthesized group list",
        None,
    );
    b.file_data(
        "/etc/nsswitch.conf".into(),
        NSSWITCH.as_bytes().to_vec(),
        0o444,
        FileDataOp::RoBindData,
        "name lookup from files only",
        None,
    );
    if eff.mask_machine_id {
        let id = match (ctx.kind, ctx.game) {
            (SessionKind::Provision, _) | (_, None) => ctx.install_id.to_owned(),
            (_, Some(g)) => stand_in_machine_id(host.machine_id.as_deref(), g.id),
        };
        // `--file`, not `--ro-bind-data`: pressure-vessel re-binds /etc/machine-id into its
        // container, and its inner bwrap cannot bind from a data mount (F-227).
        b.file_data(
            "/etc/machine-id".into(),
            format!("{id}\n").into_bytes(),
            0o444,
            FileDataOp::File,
            "stand-in machine ID",
            Some(PolicyField::MaskMachineId),
        );
    } else if let Some(entry) = lookup(host, "machine-id") {
        emit_entry(
            kind,
            "machine-id",
            &entry,
            false,
            "real machine ID",
            Some(PolicyField::MaskMachineId),
            b,
        );
    }
    for &item in EtcItem::AFTER_IDENTITY {
        if !wanted(item, eff) {
            continue;
        }
        if let Some(entry) = lookup(host, item.name()) {
            emit_entry(
                kind,
                item.name(),
                &entry,
                item.is_try(),
                if item == EtcItem::Hosts {
                    "host names (localhost)"
                } else {
                    "network configuration"
                },
                field_of(item),
                b,
            );
        }
    }
}

/// Whether a mount came from this section's allow-list (used by tests).
pub fn is_allowed_destination(m: &MountKind) -> bool {
    let dst = m.dst();
    if dst == Path::new("/etc") {
        return true;
    }
    dst.parent() == Some(Path::new("/etc"))
        && dst
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| EtcItem::all_names().contains(&n))
}
